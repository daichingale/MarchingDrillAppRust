//! Blocking room client. One OS thread owns the socket; the UI only uses
//! `try_recv`. Wasm builds omit this module: a browser has no threads, and
//! the same [`crate::RoomEvent`] values are what a `WebSocket` would carry.

use crate::{RoomEvent, SyncOp};
use std::collections::VecDeque;
use std::io::ErrorKind;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::thread::JoinHandle;
use std::time::Duration;
use tungstenite::client::IntoClientRequest;
use tungstenite::{Message, WebSocket};

const POLL_INTERVAL: Duration = Duration::from_millis(50);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const BACKOFF_MIN: Duration = Duration::from_millis(400);
const BACKOFF_MAX: Duration = Duration::from_secs(8);
const MAX_INBOUND: usize = 256;
const MAX_PENDING: usize = 512;
const DEFAULT_PORT: u16 = 8787;

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

/// A live connection to one share-link room.
pub struct SyncClient {
    url: String,
    outgoing: std::sync::mpsc::Sender<RoomEvent>,
    incoming: std::sync::mpsc::Receiver<RoomEvent>,
    status: Arc<AtomicU8>,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl SyncClient {
    /// Starts connecting immediately. The opening `hello` is the snapshot a
    /// peer who is already in the room needs, because the relay keeps no log.
    #[must_use]
    pub fn connect(host: &str, room: &str, hello: RoomEvent) -> Self {
        let url = ws_url(host, room);
        let (outgoing, outbound) = mpsc::channel();
        let (inbound_tx, incoming) = mpsc::sync_channel(MAX_INBOUND);
        let status = Arc::new(AtomicU8::new(ConnectionStatus::Connecting.code()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let _ = outgoing.send(hello);
        let worker = Worker {
            url: url.clone(),
            outbound,
            inbound: inbound_tx,
            status: Arc::clone(&status),
            shutdown: Arc::clone(&shutdown),
            pending: VecDeque::new(),
        };
        let handle = std::thread::Builder::new()
            .name("drill-sync-client".into())
            .spawn(move || worker.run())
            .ok();
        Self {
            url,
            outgoing,
            incoming,
            status,
            shutdown,
            handle,
        }
    }

    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    #[must_use]
    pub fn status(&self) -> ConnectionStatus {
        ConnectionStatus::from_code(self.status.load(Ordering::Relaxed))
    }

    /// Queues an event. Never blocks. A burst of moves of one dot collapses
    /// to the latest point inside the worker.
    pub fn publish(&self, event: RoomEvent) {
        let _ = self.outgoing.send(event);
    }

    /// One inbound event, or `None` when the queue is empty.
    pub fn poll(&mut self) -> Option<RoomEvent> {
        self.incoming.try_recv().ok()
    }
}

impl Drop for SyncClient {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        drop(self.handle.take());
    }
}

struct Worker {
    url: String,
    outbound: Receiver<RoomEvent>,
    inbound: SyncSender<RoomEvent>,
    status: Arc<AtomicU8>,
    shutdown: Arc<AtomicBool>,
    pending: VecDeque<RoomEvent>,
}

impl Worker {
    fn run(mut self) {
        let mut backoff = BACKOFF_MIN;
        while !self.stopping() {
            self.set_status(ConnectionStatus::Connecting);
            match dial(&self.url) {
                Ok(socket) => {
                    backoff = BACKOFF_MIN;
                    self.set_status(ConnectionStatus::Connected);
                    self.serve(socket);
                }
                Err(_) => {
                    self.set_status(ConnectionStatus::Disconnected);
                    if !self.idle(backoff) {
                        break;
                    }
                    backoff = (backoff * 2).min(BACKOFF_MAX);
                }
            }
        }
        self.set_status(ConnectionStatus::Disconnected);
    }

    fn serve(&mut self, mut socket: WebSocket<TcpStream>) {
        while !self.stopping() {
            if !self.drain_outbound() {
                break;
            }
            if !self.flush_pending(&mut socket) {
                break;
            }
            if let Err(error) = socket.flush()
                && !is_retryable(&error)
            {
                break;
            }
            match socket.read() {
                Ok(Message::Text(text)) => {
                    if !self.forward(text.as_str()) {
                        break;
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
        self.set_status(ConnectionStatus::Disconnected);
    }

    fn drain_outbound(&mut self) -> bool {
        loop {
            match self.outbound.try_recv() {
                Ok(event) => self.enqueue(event),
                Err(TryRecvError::Empty) => return true,
                Err(TryRecvError::Disconnected) => return false,
            }
        }
    }

    fn enqueue(&mut self, event: RoomEvent) {
        match &event {
            RoomEvent::Hello { .. } => {
                self.pending
                    .retain(|queued| !matches!(queued, RoomEvent::Hello { .. }));
            }
            RoomEvent::Op {
                op: SyncOp::MoveDot { dot, .. },
                ..
            } => {
                if let Some(index) = self.pending.iter().position(|queued| match queued {
                    RoomEvent::Op {
                        op: SyncOp::MoveDot { dot: existing, .. },
                        ..
                    } => existing == dot,
                    _ => false,
                }) {
                    self.pending[index] = event;
                    return;
                }
            }
            RoomEvent::Op { .. } | RoomEvent::Leave { .. } => {}
        }
        self.pending.push_back(event);
        while self.pending.len() > MAX_PENDING {
            self.pending.pop_front();
        }
    }

    fn flush_pending(&mut self, socket: &mut WebSocket<TcpStream>) -> bool {
        while let Some(event) = self.pending.pop_front() {
            let Ok(json) = event.to_json() else {
                continue;
            };
            if let Err(error) = socket.send(Message::text(json)) {
                if !is_retryable(&error) {
                    self.pending.push_front(event);
                    return false;
                }
                self.pending.push_front(event);
                return true;
            }
        }
        true
    }

    fn forward(&self, text: &str) -> bool {
        let Ok(event) = RoomEvent::from_json(text) else {
            return true;
        };
        match self.inbound.try_send(event) {
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

    fn idle(&mut self, total: Duration) -> bool {
        let mut remaining = total;
        while remaining > Duration::ZERO && !self.stopping() {
            if !self.drain_outbound() {
                return false;
            }
            let slice = remaining.min(POLL_INTERVAL);
            std::thread::sleep(slice);
            remaining = remaining.saturating_sub(slice);
        }
        self.drain_outbound()
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

fn ws_url(host: &str, room: &str) -> String {
    let host = host.trim().trim_end_matches('/');
    let host = host
        .strip_prefix("ws://")
        .or_else(|| host.strip_prefix("http://"))
        .or_else(|| host.strip_prefix("wss://"))
        .unwrap_or(host);
    format!("ws://{host}/room/{}", normalize_room(room))
}

fn normalize_room(room: &str) -> String {
    let code: String = room
        .trim()
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || *character == '-' || *character == '_'
        })
        .take(32)
        .map(|character| character.to_ascii_lowercase())
        .collect();
    if code.is_empty() {
        "room".to_owned()
    } else {
        code
    }
}

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
    use crate::{ReplicaId, ShowState};
    use drill_core::Document;

    #[test]
    fn url_uses_the_room_path() {
        let hello = ShowState::from_document(&Document::demo(1, 1), ReplicaId::from_raw(1))
            .hello("A", [0, 0, 0]);
        let client = SyncClient::connect("ws://127.0.0.1:9/", "Brass Night", hello);
        assert_eq!(client.url(), "ws://127.0.0.1:9/room/brassnight");
    }
}
