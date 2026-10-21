//! One fanout for a rehearsal room.
//!
//! Presence cursors and collaborative edits are different JSON shapes, and
//! each client drops what it does not understand. Editor seats are the one
//! decision this process makes: past the cap, a joiner watches. A drop still
//! announces both a presence `Leave` and a sync `Leave`.
//!
//! The listener is plain TCP unless [`serve_tls`] is given a certificate.
//! Desktop and phone clients speak `wss://` with rustls when the host is
//! written that way, so a public relay does not have to stay on the LAN.
//! A stranger who can guess the room code can edit; room codes have to be
//! unguessable.

mod seats;

use drill_presence::{PresenceMessage, Rooms, UserId, room_code_from_path};
use drill_sync::{ReplicaId, RoomEvent};
use seats::Seats;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::{Message, WebSocket};

pub const DEFAULT_BIND: &str = "0.0.0.0:8787";

/// Editors who may drag at once.
///
/// Load sweep on an 80-performer, 10-set show, drags and presence coalesced
/// at 20Hz: end-to-end p95 stayed near 100ms through 20 editors (relay CPU
/// about 16%, RSS 45MB) and jumped to about 480ms at 30 (CPU about 29%,
/// RSS 97MB). 20 is the largest measured count that still feels live.
/// Further people join as viewers and do not publish drags.
pub const EDITOR_LIMIT: u16 = 20;

const POLL_INTERVAL: Duration = Duration::from_millis(50);
const MAX_OUTBOUND_BACKLOG: usize = 256;

static NEXT_CONN_ID: AtomicU64 = AtomicU64::new(1);

/// Accepts forever. The binary and the loopback tests both call this.
pub fn serve(listener: &TcpListener) {
    serve_with_limit(listener, editor_limit());
}

/// Same fanout as [`serve`], with an explicit editor cap. The load test uses
/// this so a measurement of 30 drags is not cut off by the rehearsal cap.
pub fn serve_with_limit(listener: &TcpListener, limit: u16) {
    let rooms = Arc::new(Mutex::new(Rooms::new()));
    let seats = Arc::new(Mutex::new(Seats::new(limit)));
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let rooms = Arc::clone(&rooms);
        let seats = Arc::clone(&seats);
        let _ = std::thread::Builder::new()
            .name("drill-relay-conn".into())
            .spawn(move || serve_connection(stream, &rooms, &seats));
    }
}

/// Same fanout as [`serve`], with the TCP listener wrapped in rustls.
///
/// `cert_pem` and `key_pem` are PEM bytes. The process exits the loop only
/// when the listener closes. A bad certificate returns before any accept.
pub fn serve_tls(listener: &TcpListener, cert_pem: &[u8], key_pem: &[u8]) -> Result<(), String> {
    let config = tls_server_config(cert_pem, key_pem)?;
    serve_tls_with_limit(listener, config, editor_limit());
    Ok(())
}

fn serve_tls_with_limit(listener: &TcpListener, config: Arc<rustls::ServerConfig>, limit: u16) {
    let rooms = Arc::new(Mutex::new(Rooms::new()));
    let seats = Arc::new(Mutex::new(Seats::new(limit)));
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let rooms = Arc::clone(&rooms);
        let seats = Arc::clone(&seats);
        let config = Arc::clone(&config);
        let _ = std::thread::Builder::new()
            .name("drill-relay-tls".into())
            .spawn(move || serve_tls_connection(stream, config, &rooms, &seats));
    }
}

fn tls_server_config(cert_pem: &[u8], key_pem: &[u8]) -> Result<Arc<rustls::ServerConfig>, String> {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
    let _ = rustls::crypto::ring::default_provider().install_default();
    let certs = CertificateDer::pem_slice_iter(cert_pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("certificate: {error}"))?;
    if certs.is_empty() {
        return Err("certificate PEM contained no certificate".into());
    }
    let key = PrivateKeyDer::from_pem_slice(key_pem).map_err(|error| {
        if matches!(error, rustls::pki_types::pem::Error::NoItemsFound) {
            "private key PEM contained no key".to_owned()
        } else {
            format!("private key: {error}")
        }
    })?;
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|error| format!("tls: {error}"))?;
    Ok(Arc::new(config))
}

fn editor_limit() -> u16 {
    std::env::var("DRILLFORGE_EDITOR_LIMIT")
        .ok()
        .and_then(|raw| raw.parse().ok())
        .filter(|limit| (1..=64).contains(limit))
        .unwrap_or(EDITOR_LIMIT)
}

#[allow(clippy::result_large_err)]
pub(crate) fn serve_connection(stream: TcpStream, rooms: &Mutex<Rooms>, seats: &Mutex<Seats>) {
    let _ = stream.set_nodelay(true);
    if stream.set_read_timeout(Some(POLL_INTERVAL)).is_err() {
        return;
    }
    let Some((socket, room)) = accept_room(stream) else {
        return;
    };
    run_session(socket, rooms, seats, room);
}

fn serve_tls_connection(
    stream: TcpStream,
    config: Arc<rustls::ServerConfig>,
    rooms: &Mutex<Rooms>,
    seats: &Mutex<Seats>,
) {
    let _ = stream.set_nodelay(true);
    if stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .is_err()
    {
        return;
    }
    let Ok(conn) = rustls::ServerConnection::new(config) else {
        return;
    };
    let tls = rustls::StreamOwned::new(conn, stream);
    let Some((mut socket, room)) = accept_room(tls) else {
        return;
    };
    let _ = socket.get_mut().sock.set_read_timeout(Some(POLL_INTERVAL));
    run_session(socket, rooms, seats, room);
}

#[allow(clippy::result_large_err)]
fn accept_room<S: Read + Write>(stream: S) -> Option<(WebSocket<S>, String)> {
    let mut room = None;
    let socket = tungstenite::accept_hdr(
        stream,
        |request: &Request, response: Response| -> Result<Response, ErrorResponse> {
            match room_code_from_path(request.uri().path()) {
                Some(code) => {
                    room = Some(code);
                    Ok(response)
                }
                None => Err(Response::builder()
                    .status(404)
                    .body(Some("expected /room/<code>".to_owned()))
                    .expect("static error response builds")),
            }
        },
    )
    .ok()?;
    Some((socket, room?))
}

fn run_session<S: Read + Write>(
    socket: WebSocket<S>,
    rooms: &Mutex<Rooms>,
    seats: &Mutex<Seats>,
    room: String,
) {
    let conn = NEXT_CONN_ID.fetch_add(1, Ordering::Relaxed);
    let (outbox, inbox) = mpsc::sync_channel::<String>(MAX_OUTBOUND_BACKLOG);
    if let Ok(mut guard) = rooms.lock() {
        guard.join(&room, conn, outbox);
    }
    let identity = pump(socket, &inbox, rooms, seats, &room, conn);
    let promoted = seats
        .lock()
        .map(|mut guard| guard.disconnect(&room, conn))
        .unwrap_or_default();
    if let Ok(mut guard) = rooms.lock() {
        guard.leave(&room, conn);
        if let Some(user) = identity.user
            && let Ok(json) = PresenceMessage::Leave(user).to_json()
        {
            guard.broadcast(&room, conn, &json);
        }
        if let Some(replica) = identity.replica
            && let Ok(json) = (RoomEvent::Leave { from: replica }).to_json()
        {
            guard.broadcast(&room, conn, &json);
        }
        send_direct(&guard, &room, &promoted);
    }
}

struct Identity {
    user: Option<UserId>,
    replica: Option<ReplicaId>,
}

fn pump<S: Read + Write>(
    mut socket: WebSocket<S>,
    inbox: &Receiver<String>,
    rooms: &Mutex<Rooms>,
    seats: &Mutex<Seats>,
    room: &str,
    conn: u64,
) -> Identity {
    let mut identity = Identity {
        user: None,
        replica: None,
    };
    while drain_outbox(&mut socket, inbox).is_ok() {
        match socket.read() {
            Ok(Message::Text(text)) => {
                if let Ok(message) = PresenceMessage::from_json(text.as_str()) {
                    identity.user = Some(message.user_id());
                    if let Ok(guard) = rooms.lock() {
                        guard.broadcast(room, conn, text.as_str());
                    }
                    continue;
                }
                if let Ok(event) = RoomEvent::from_json(text.as_str()) {
                    identity.replica = Some(event.replica());
                    let decision = seats
                        .lock()
                        .map(|mut guard| guard.observe(room, conn, &event))
                        .unwrap_or_default();
                    if let Ok(guard) = rooms.lock() {
                        if decision.relay {
                            guard.broadcast(room, conn, text.as_str());
                        }
                        send_direct(&guard, room, &decision);
                    }
                    continue;
                }
                if let Ok(guard) = rooms.lock() {
                    guard.broadcast(room, conn, text.as_str());
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(error) if is_retryable(&error) => {}
            Err(_) => break,
        }
    }
    let _ = socket.close(None);
    let _ = socket.flush();
    identity
}

fn send_direct(rooms: &Rooms, room: &str, decision: &seats::Decision) {
    for notice in &decision.direct {
        let Ok(json) = notice.event.to_json() else {
            continue;
        };
        rooms.send_to(room, notice.conn, &json);
    }
}

fn drain_outbox<S: Read + Write>(
    socket: &mut WebSocket<S>,
    inbox: &Receiver<String>,
) -> Result<(), ()> {
    loop {
        match inbox.try_recv() {
            Ok(payload) => {
                if socket.send(Message::text(payload)).is_err() {
                    return Err(());
                }
            }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => return Err(()),
        }
    }
    match socket.flush() {
        Ok(()) => Ok(()),
        Err(error) if is_retryable(&error) => Ok(()),
        Err(_) => Err(()),
    }
}

fn is_retryable(error: &tungstenite::Error) -> bool {
    matches!(
        error,
        tungstenite::Error::Io(io)
            if matches!(
                io.kind(),
                ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
            )
    )
}
