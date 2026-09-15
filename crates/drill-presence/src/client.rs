//! The blocking presence client.
//!
//! One named OS thread owns the socket. The UI thread only ever pushes into an
//! `mpsc::Sender<Presence>` and drains an `mpsc::Receiver<PresenceMessage>`
//! with `try_recv` -- the same non-blocking contract as `drill_jobs::Job::poll`.
//! Nothing here can block, join, or unwind into a frame.

use crate::{Presence, PresenceMessage, room_path};
use std::io::ErrorKind;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tungstenite::client::IntoClientRequest;
use tungstenite::{Message, WebSocket};

/// Outbound rate limit.
///
/// Selection and cursor can change on every frame (~16ms at 60fps); forwarding
/// that raw would put 60 frames/second on the wire per peer to convey
/// information a human reads as continuous well below that rate. 200ms caps a
/// peer at 5 updates/second while staying under the ~250ms mark where a remote
/// cursor starts to feel like a slideshow rather than a live pointer. Updates
/// are coalesced rather than queued, so a burst collapses to its final state.
const SEND_INTERVAL: Duration = Duration::from_millis(200);

/// Socket read timeout, which doubles as the worker's loop period: it bounds
/// how long a pending outbound update waits and how quickly shutdown is
/// noticed. tungstenite retains partially-read frames across a timed-out read,
/// so interleaving reads and writes on one thread this way is sound.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// How long a peer may stay silent before it republishes unchanged state.
///
/// Updates are only sent when something actually changed, so a collaborator
/// who steps away would otherwise emit nothing at all -- leaving receivers
/// unable to distinguish "idle" from "gone" until TCP eventually notices,
/// which can take minutes. This heartbeat costs 0.2 messages/second per idle
/// peer and lets receivers expire a silent peer on a human timescale.
pub const HEARTBEAT: Duration = Duration::from_secs(5);

/// Handshakes are one round trip on a LAN; anything slower is a dead relay.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

const BACKOFF_MIN: Duration = Duration::from_millis(500);
const BACKOFF_MAX: Duration = Duration::from_secs(8);

/// Bound on how far the UI may fall behind before inbound frames are dropped.
/// Presence is latest-wins, so shedding beats growing without limit: only a
/// UI that has stopped polling entirely can reach this, and it has worse
/// problems than a missing cursor by then.
const MAX_INBOUND_BACKLOG: usize = 256;

pub const DEFAULT_PORT: u16 = 8787;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ConnectionStatus {
    #[default]
    Disconnected,
    Connecting,
    Connected,
}

impl ConnectionStatus {
    const fn code(self) -> u8 {
        match self {
            Self::Disconnected => 0,
            Self::Connecting => 1,
            Self::Connected => 2,
        }
    }

    const fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Connecting,
            2 => Self::Connected,
            _ => Self::Disconnected,
        }
    }
}

/// A live (or endlessly retrying) connection to one presence room.
pub struct PresenceClient {
    room: String,
    url: String,
    outgoing: Sender<Presence>,
    incoming: Receiver<PresenceMessage>,
    status: Arc<AtomicU8>,
    last_error: Arc<Mutex<Option<String>>>,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl PresenceClient {
    /// Starts the worker thread for `room` on `host` and returns immediately.
    ///
    /// `host` is `"name"` or `"name:port"`. Connection failure is not an error
    /// here: the worker retries with backoff and reports through `status`, so
    /// a relay that starts late still gets picked up without user action.
    #[must_use]
    pub fn connect(host: &str, room: &str, mut initial: Presence) -> Self {
        initial.sanitize();
        let host = host.trim().trim_end_matches('/');
        let host = host
            .strip_prefix("ws://")
            .or_else(|| host.strip_prefix("http://"))
            .unwrap_or(host);
        let url = format!("ws://{host}{}", room_path(room));

        let (outgoing, outbound_rx) = mpsc::channel::<Presence>();
        let (inbound_tx, incoming) = mpsc::sync_channel::<PresenceMessage>(MAX_INBOUND_BACKLOG);
        let status = Arc::new(AtomicU8::new(ConnectionStatus::Connecting.code()));
        let last_error = Arc::new(Mutex::new(None));
        let shutdown = Arc::new(AtomicBool::new(false));

        let worker = Worker {
            url: url.clone(),
            outbound: outbound_rx,
            inbound: inbound_tx,
            status: Arc::clone(&status),
            last_error: Arc::clone(&last_error),
            shutdown: Arc::clone(&shutdown),
            latest: Some(initial),
            dirty: true,
        };

        let handle = std::thread::Builder::new()
            .name("drill-presence-client".into())
            .spawn(move || worker.run())
            .ok();

        Self {
            room: room.to_owned(),
            url,
            outgoing,
            incoming,
            status,
            last_error,
            shutdown,
            handle,
        }
    }

    #[must_use]
    pub fn room(&self) -> &str {
        &self.room
    }

    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    #[must_use]
    pub fn status(&self) -> ConnectionStatus {
        ConnectionStatus::from_code(self.status.load(Ordering::Relaxed))
    }

    /// The most recent transport failure, for a diagnostic line in the UI.
    #[must_use]
    pub fn last_error(&self) -> Option<String> {
        self.last_error.lock().ok().and_then(|slot| slot.clone())
    }

    /// Queues the local viewport state. Never blocks; the worker coalesces.
    pub fn publish(&self, presence: Presence) {
        let _ = self.outgoing.send(presence);
    }

    /// Non-blocking drain of one inbound message, mirroring `Job::poll`.
    ///
    /// A disconnected worker reads the same as an empty queue on purpose: the
    /// UI learns about connection state from `status`, and must never have to
    /// distinguish the two here to stay non-blocking.
    pub fn poll(&mut self) -> Option<PresenceMessage> {
        self.incoming.try_recv().ok()
    }
}

impl Drop for PresenceClient {
    /// Signals the worker and detaches. Joining is deliberately avoided: a
    /// socket stuck in a kernel-level connect must never stall a UI teardown.
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        drop(self.handle.take());
    }
}

struct Worker {
    url: String,
    outbound: Receiver<Presence>,
    inbound: SyncSender<PresenceMessage>,
    status: Arc<AtomicU8>,
    last_error: Arc<Mutex<Option<String>>>,
    shutdown: Arc<AtomicBool>,
    latest: Option<Presence>,
    dirty: bool,
}

impl Worker {
    fn run(mut self) {
        let mut backoff = BACKOFF_MIN;
        while !self.stopping() {
            self.set_status(ConnectionStatus::Connecting);
            match dial(&self.url) {
                Ok(socket) => {
                    backoff = BACKOFF_MIN;
                    self.record_error(None);
                    self.set_status(ConnectionStatus::Connected);
                    // A reconnect must restate the local viewport: peers
                    // cleared this user on the Leave that the relay sent when
                    // the old socket dropped.
                    self.dirty = true;
                    self.serve(socket);
                }
                Err(error) => {
                    self.record_error(Some(error));
                    self.set_status(ConnectionStatus::Disconnected);
                    if !self.idle(backoff) {
                        break; // The client handle was dropped mid-backoff.
                    }
                    backoff = (backoff * 2).min(BACKOFF_MAX);
                }
            }
        }
        self.set_status(ConnectionStatus::Disconnected);
    }

    /// Pumps one connection until it fails or shutdown is requested.
    fn serve(&mut self, mut socket: WebSocket<TcpStream>) {
        let mut last_send = Instant::now() - SEND_INTERVAL;
        while !self.stopping() {
            if !self.drain_outbound() {
                break; // The client handle was dropped.
            }
            if last_send.elapsed() >= HEARTBEAT {
                self.dirty = true;
            }
            if self.dirty
                && last_send.elapsed() >= SEND_INTERVAL
                && let Some(presence) = self.latest.clone()
            {
                let Ok(json) = PresenceMessage::Update(presence).to_json() else {
                    // Unreachable for this shape; drop rather than spin on it.
                    self.dirty = false;
                    continue;
                };
                if let Err(error) = socket.send(Message::text(json)) {
                    self.record_error(Some(error.to_string()));
                    break;
                }
                self.dirty = false;
                last_send = Instant::now();
            }
            // Flushes queued protocol replies (notably automatic pongs) even
            // during stretches with no presence to publish.
            if let Err(error) = socket.flush()
                && !is_retryable(&error)
            {
                self.record_error(Some(error.to_string()));
                break;
            }
            match socket.read() {
                Ok(Message::Text(text)) => {
                    if !self.forward(text.as_str()) {
                        break;
                    }
                }
                Ok(Message::Close(_)) => break,
                // Binary/Ping/Pong/Frame carry nothing this protocol defines.
                Ok(_) => {}
                Err(error) if is_retryable(&error) => {}
                Err(error) => {
                    self.record_error(Some(error.to_string()));
                    break;
                }
            }
        }
        let _ = socket.close(None);
        let _ = socket.flush();
        self.set_status(ConnectionStatus::Disconnected);
    }

    /// Collapses every queued update to the newest one. Returns `false` when
    /// the UI side has hung up.
    fn drain_outbound(&mut self) -> bool {
        loop {
            match self.outbound.try_recv() {
                Ok(mut presence) => {
                    // Send-side sanitize as well as receive-side: JSON cannot
                    // represent NaN, so an accidental non-finite cursor would
                    // serialize to `null` and make every peer reject the whole
                    // frame. Normalizing here keeps one local glitch local.
                    presence.sanitize();
                    self.dirty = true;
                    self.latest = Some(presence);
                }
                Err(TryRecvError::Empty) => return true,
                Err(TryRecvError::Disconnected) => return false,
            }
        }
    }

    /// Parses and hands one inbound frame to the UI. Malformed peer input is
    /// dropped, never fatal: one bad frame must not end the session.
    fn forward(&self, text: &str) -> bool {
        let Ok(message) = PresenceMessage::from_json(text) else {
            return true;
        };
        match self.inbound.try_send(message) {
            Ok(()) | Err(TrySendError::Full(_)) => true,
            Err(TrySendError::Disconnected(_)) => false,
        }
    }

    fn stopping(&self) -> bool {
        self.shutdown.load(Ordering::Relaxed)
    }

    fn set_status(&self, status: ConnectionStatus) {
        self.status.store(status.code(), Ordering::Relaxed);
    }

    fn record_error(&self, error: Option<String>) {
        if let Ok(mut slot) = self.last_error.lock() {
            *slot = error;
        }
    }

    /// Backs off in short slices so a disconnect request is honoured promptly.
    ///
    /// The outbound queue is drained throughout. Without this, a UI that keeps
    /// publishing every frame while the relay is down would grow that queue
    /// without bound for as long as the outage lasts; coalescing here keeps it
    /// at one entry. Returns `false` once the client handle has been dropped.
    fn idle(&mut self, total: Duration) -> bool {
        let mut remaining = total;
        while remaining > Duration::ZERO && !self.stopping() {
            if !self.drain_outbound() {
                return false;
            }
            let slice = remaining.min(POLL_INTERVAL);
            std::thread::sleep(slice);
            remaining -= slice;
        }
        self.drain_outbound()
    }
}

/// A read that produced nothing yet, rather than a broken connection.
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

/// Opens one connection with bounded timeouts.
///
/// `tungstenite::connect` is not used because it offers no connect timeout and
/// no hook to configure the socket before the handshake; a wrong host would
/// otherwise park the worker on the OS default for tens of seconds.
fn dial(url: &str) -> Result<WebSocket<TcpStream>, String> {
    let request = url
        .into_client_request()
        .map_err(|error| format!("invalid relay address: {error}"))?;
    let uri = request.uri();
    let host = uri.host().ok_or("relay address has no host")?.to_owned();
    let port = uri.port_u16().unwrap_or(DEFAULT_PORT);

    let address = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|error| format!("cannot resolve {host}: {error}"))?
        .next()
        .ok_or_else(|| format!("cannot resolve {host}"))?;

    let stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT)
        .map_err(|error| format!("cannot reach relay: {error}"))?;
    let _ = stream.set_nodelay(true);
    stream
        .set_read_timeout(Some(HANDSHAKE_TIMEOUT))
        .map_err(|error| error.to_string())?;

    let (socket, _response) =
        tungstenite::client(request, stream).map_err(|error| error.to_string())?;
    socket
        .get_ref()
        .set_read_timeout(Some(POLL_INTERVAL))
        .map_err(|error| error.to_string())?;
    Ok(socket)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UserId;

    #[test]
    fn status_codes_round_trip() {
        for status in [
            ConnectionStatus::Disconnected,
            ConnectionStatus::Connecting,
            ConnectionStatus::Connected,
        ] {
            assert_eq!(ConnectionStatus::from_code(status.code()), status);
        }
        assert_eq!(
            ConnectionStatus::from_code(200),
            ConnectionStatus::Disconnected
        );
    }

    #[test]
    fn url_is_built_from_host_and_room() {
        let client = PresenceClient::connect(
            "127.0.0.1:9",
            "brass",
            Presence::new(UserId::from_raw(1), "A".into(), [0, 0, 0]),
        );
        assert_eq!(client.url(), "ws://127.0.0.1:9/room/brass");
        assert_eq!(client.room(), "brass");
    }

    #[test]
    fn host_accepts_a_pasted_scheme() {
        let client = PresenceClient::connect(
            "ws://relay.local:8787/",
            "drums",
            Presence::new(UserId::from_raw(1), "A".into(), [0, 0, 0]),
        );
        assert_eq!(client.url(), "ws://relay.local:8787/room/drums");
    }

    #[test]
    fn an_unreachable_relay_never_blocks_the_caller() {
        // Port 9 (discard) is closed on the loopback: this exercises the
        // failure path without touching the network.
        let mut client = PresenceClient::connect(
            "127.0.0.1:9",
            "ghost",
            Presence::new(UserId::from_raw(7), "Ghost".into(), [1, 2, 3]),
        );
        client.publish(Presence::new(
            UserId::from_raw(7),
            "Ghost".into(),
            [1, 2, 3],
        ));
        assert!(client.poll().is_none());
        assert_ne!(client.status(), ConnectionStatus::Connected);
    }

    #[test]
    fn retryable_io_kinds_are_classified() {
        let would_block = tungstenite::Error::Io(std::io::Error::from(ErrorKind::WouldBlock));
        let timed_out = tungstenite::Error::Io(std::io::Error::from(ErrorKind::TimedOut));
        let refused = tungstenite::Error::Io(std::io::Error::from(ErrorKind::ConnectionRefused));
        assert!(is_retryable(&would_block));
        assert!(is_retryable(&timed_out));
        assert!(!is_retryable(&refused));
        assert!(!is_retryable(&tungstenite::Error::ConnectionClosed));
    }
}
