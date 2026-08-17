//! The DrillForge presence relay.
//!
//! Its entire job: take a text frame from one peer and hand it, unmodified, to
//! every other peer in the same room. It parses messages only far enough to
//! learn who a connection belongs to, so it can announce a `Leave` when that
//! connection drops. It has no document awareness, no storage, and no state
//! that outlives a connection.
//!
//! Blocking sockets, one OS thread per connection. Presence traffic is a few
//! small frames per second per peer and a rehearsal is dozens of peers, so the
//! thread-per-connection cost is irrelevant and the code stays readable.
//!
//! This lives in the library rather than the binary so the integration test
//! drives the same code the shipped relay runs.
//!
//! Known limitation, deliberate for v0: plain `ws://` with no TLS and no
//! authentication. Run it on a trusted LAN or loopback.

use crate::{PresenceMessage, Rooms, UserId, room_code_from_path};
use std::io::ErrorKind;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::{Message, WebSocket};

pub const DEFAULT_BIND: &str = "0.0.0.0:8787";

/// Doubles as the per-connection loop period: bounds how long a queued
/// broadcast waits before it is written out.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Backlog per peer. Presence is latest-wins, so a peer too slow to keep up is
/// better served by dropped frames than by unbounded memory on the relay.
const MAX_OUTBOUND_BACKLOG: usize = 256;

static NEXT_CONN_ID: AtomicU64 = AtomicU64::new(1);

/// Accepts forever, handing each connection to its own thread.
///
/// Never returns under normal operation.
pub fn serve(listener: &TcpListener) {
    let rooms = Arc::new(Mutex::new(Rooms::new()));
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let rooms = Arc::clone(&rooms);
        // A failed spawn drops the connection, which is the correct response
        // to thread exhaustion: shed load rather than stall the accept loop.
        let _ = std::thread::Builder::new()
            .name("drill-presence-conn".into())
            .spawn(move || serve_connection(stream, &rooms));
    }
}

/// Runs one peer's connection to completion: handshake, pump, then announce
/// the departure to the rest of the room.
// The handshake callback's `Result<Response, ErrorResponse>` is imposed by
// `tungstenite::accept_hdr`; boxing it is not an option we control, and it is
// constructed at most once per connection.
#[allow(clippy::result_large_err)]
pub fn serve_connection(stream: TcpStream, rooms: &Mutex<Rooms>) {
    let _ = stream.set_nodelay(true);
    if stream.set_read_timeout(Some(POLL_INTERVAL)).is_err() {
        return;
    }

    // The room lives in the request path, so it is captured during the
    // handshake and a pathless or unknown route is refused there.
    let mut room = None;
    let accepted = tungstenite::accept_hdr(
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
    // Discarded eagerly: the handshake error type owns the callback, and with
    // it the borrow of `room` that the next line needs to move out of.
    .ok();
    let (Some(socket), Some(room)) = (accepted, room) else {
        return;
    };

    let conn = NEXT_CONN_ID.fetch_add(1, Ordering::Relaxed);
    let (outbox, inbox) = mpsc::sync_channel::<String>(MAX_OUTBOUND_BACKLOG);
    if let Ok(mut guard) = rooms.lock() {
        guard.join(&room, conn, outbox);
    }

    let identity = pump(socket, &inbox, rooms, &room, conn);

    if let Ok(mut guard) = rooms.lock() {
        guard.leave(&room, conn);
        // Only a peer that identified itself can be cleared from the others'
        // rosters; a connection that never sent an Update was never drawn.
        if let Some(user) = identity
            && let Ok(json) = PresenceMessage::Leave(user).to_json()
        {
            guard.broadcast(&room, conn, &json);
        }
    }
}

/// Interleaves "write anything the room queued for me" with "read one frame
/// and fan it out", on a single thread, using the socket read timeout as the
/// clock. Returns the identity this connection claimed, if any.
fn pump(
    mut socket: WebSocket<TcpStream>,
    inbox: &Receiver<String>,
    rooms: &Mutex<Rooms>,
    room: &str,
    conn: u64,
) -> Option<UserId> {
    let mut identity = None;
    while drain_outbox(&mut socket, inbox).is_ok() {
        match socket.read() {
            Ok(Message::Text(text)) => {
                // Parsed only to learn who this is. The forwarded payload is
                // the original text: the relay is not in the content business.
                if let Ok(message) = PresenceMessage::from_json(text.as_str()) {
                    identity = Some(message.user_id());
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

/// Writes every queued broadcast. `Err(())` means the connection is finished.
fn drain_outbox(socket: &mut WebSocket<TcpStream>, inbox: &Receiver<String>) -> Result<(), ()> {
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
    // Keeps automatic pong replies moving during quiet periods.
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
