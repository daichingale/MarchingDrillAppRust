//! Phone and PC speak one JSON protocol.
//!
//! A "phone" here is a browser-shaped socket: it writes `RoomEvent::to_json`
//! and reads with `RoomEvent::from_json`, through the same coalesced outbox
//! the wasm client uses. A "PC" is `SyncClient`, the desktop socket. The
//! relay is the same process either way.

use drill_core::{Document, Point};
use drill_sync::{CoalescedOutbox, DotId, ReplicaId, RoomEvent, ShowState, SyncClient};
use std::io::ErrorKind;
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};
use tungstenite::client::IntoClientRequest;
use tungstenite::{Message, WebSocket};

const DEADLINE: Duration = Duration::from_secs(15);

struct BrowserPeer {
    outgoing: Sender<RoomEvent>,
    incoming: Receiver<RoomEvent>,
}

impl BrowserPeer {
    fn connect(host: &str, room: &str, hello: RoomEvent) -> Self {
        let url = drill_sync::socket_url(host, room, false);
        let (outgoing, outbound) = mpsc::channel();
        let (inbound, incoming) = mpsc::channel();
        std::thread::Builder::new()
            .name("drill-browser-peer".into())
            .spawn(move || browser_loop(url, outbound, inbound, hello))
            .expect("browser peer thread");
        Self { outgoing, incoming }
    }

    fn send_event(&self, event: RoomEvent) {
        let _ = self.outgoing.send(event);
    }

    fn recv_event(&mut self) -> Option<RoomEvent> {
        self.incoming.try_recv().ok()
    }
}

fn browser_loop(
    url: String,
    outbound: Receiver<RoomEvent>,
    inbound: Sender<RoomEvent>,
    hello: RoomEvent,
) {
    let Ok(mut socket) = dial(&url) else {
        return;
    };
    let mut outbox = CoalescedOutbox::default();
    outbox.push(hello);
    loop {
        loop {
            match outbound.try_recv() {
                Ok(event) => outbox.push(event),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        let now = Instant::now();
        while let Some(event) = outbox.pop_ready(now) {
            let Ok(json) = event.to_json() else {
                continue;
            };
            if socket.send(Message::text(json)).is_err() {
                return;
            }
        }
        let _ = socket.flush();
        match socket.read() {
            Ok(Message::Text(text)) => {
                if let Ok(event) = RoomEvent::from_json(text.as_str()) {
                    let _ = inbound.send(event);
                }
            }
            Ok(Message::Close(_)) => return,
            Ok(_) => {}
            Err(error) if is_retryable(&error) => {}
            Err(_) => return,
        }
    }
}

fn start_relay(limit: u16) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = listener.local_addr().expect("addr").to_string();
    std::thread::spawn(move || drill_relay::serve_with_limit(&listener, limit));
    let deadline = Instant::now() + DEADLINE;
    while Instant::now() < deadline {
        if TcpStream::connect(&address).is_ok() {
            return address;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("relay did not accept");
}

fn wait_until(label: &str, mut probe: impl FnMut() -> bool) {
    let deadline = Instant::now() + DEADLINE;
    while Instant::now() < deadline {
        if probe() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for {label}");
}

fn dial(url: &str) -> Result<WebSocket<TcpStream>, String> {
    let request = url
        .into_client_request()
        .map_err(|error| error.to_string())?;
    let uri = request.uri().clone();
    let host = uri.host().unwrap_or("127.0.0.1").to_owned();
    let port = uri.port_u16().unwrap_or(8787);
    let address = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|error| error.to_string())?
        .next()
        .ok_or_else(|| format!("cannot resolve {host}"))?;
    let stream = TcpStream::connect_timeout(&address, Duration::from_secs(5))
        .map_err(|error| error.to_string())?;
    let _ = stream.set_nodelay(true);
    stream
        .set_read_timeout(Some(Duration::from_millis(50)))
        .map_err(|error| error.to_string())?;
    let (socket, _) = tungstenite::client(request, stream).map_err(|error| error.to_string())?;
    socket
        .get_ref()
        .set_read_timeout(Some(Duration::from_millis(50)))
        .map_err(|error| error.to_string())?;
    Ok(socket)
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

fn exchange(
    mut left: impl Peer,
    mut right: impl Peer,
    mut left_show: ShowState,
    mut right_show: ShowState,
) {
    let left_dot = DotId {
        set_id: 1,
        performer_id: 1,
    };
    let right_dot = DotId {
        set_id: 1,
        performer_id: 2,
    };
    let left_op = left_show
        .move_dot(left_dot, Point { x: 6.0, y: 7.0 })
        .expect("finite");
    let right_op = right_show
        .move_dot(right_dot, Point { x: 21.0, y: 9.0 })
        .expect("finite");
    let left_from = left_show.replica();
    let right_from = right_show.replica();
    wait_until("both phones see both drags", || {
        left.publish(RoomEvent::Op {
            from: left_from,
            op: left_op.clone(),
        });
        right.publish(RoomEvent::Op {
            from: right_from,
            op: right_op.clone(),
        });
        while let Some(event) = left.poll() {
            left_show.ingest(&event);
        }
        while let Some(event) = right.poll() {
            right_show.ingest(&event);
        }
        left_show.dot(right_dot) == Some(Point { x: 21.0, y: 9.0 })
            && right_show.dot(left_dot) == Some(Point { x: 6.0, y: 7.0 })
    });
    assert!(left_show.agrees_with(&right_show));
}

trait Peer {
    fn publish(&mut self, event: RoomEvent);
    fn poll(&mut self) -> Option<RoomEvent>;
}

impl Peer for BrowserPeer {
    fn publish(&mut self, event: RoomEvent) {
        self.send_event(event);
    }
    fn poll(&mut self) -> Option<RoomEvent> {
        self.recv_event()
    }
}

impl Peer for SyncClient {
    fn publish(&mut self, event: RoomEvent) {
        SyncClient::publish(self, event);
    }
    fn poll(&mut self) -> Option<RoomEvent> {
        SyncClient::poll(self)
    }
}

#[test]
fn phone_and_phone_exchange_drags() {
    let host = start_relay(8);
    let document = Document::demo(2, 2);
    let alice = ShowState::from_document(&document, ReplicaId::from_raw(1));
    let bob = ShowState::from_document(&document, ReplicaId::from_raw(2));
    let phone_a = BrowserPeer::connect(&host, "phone-phone", alice.hello("Phone A", [1, 2, 3]));
    let phone_b = BrowserPeer::connect(&host, "phone-phone", bob.hello("Phone B", [4, 5, 6]));
    exchange(phone_a, phone_b, alice, bob);
}

#[test]
fn phone_and_pc_exchange_drags() {
    let host = start_relay(8);
    let document = Document::demo(2, 2);
    let phone_show = ShowState::from_document(&document, ReplicaId::from_raw(3));
    let pc_show = ShowState::from_document(&document, ReplicaId::from_raw(4));
    let phone = BrowserPeer::connect(&host, "phone-pc", phone_show.hello("Phone", [9, 8, 7]));
    let pc = SyncClient::connect(&host, "phone-pc", pc_show.hello("PC", [7, 8, 9]));
    exchange(phone, pc, phone_show, pc_show);
}

#[test]
fn a_full_room_lets_the_phone_watch_until_a_seat_opens() {
    let host = start_relay(1);
    let document = Document::demo(1, 1);
    let mut pc_show = ShowState::from_document(&document, ReplicaId::from_raw(20));
    let mut phone_show = ShowState::from_document(&document, ReplicaId::from_raw(21));
    let pc = SyncClient::connect(&host, "one-seat", pc_show.hello("PC", [1, 1, 1]));
    let deadline = Instant::now() + DEADLINE;
    while Instant::now() < deadline && pc.status() != drill_sync::ConnectionStatus::Connected {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(pc.status(), drill_sync::ConnectionStatus::Connected);
    std::thread::sleep(Duration::from_millis(100));
    let mut phone = BrowserPeer::connect(&host, "one-seat", phone_show.hello("Phone", [2, 2, 2]));
    let mut watching = false;
    wait_until("phone is told to watch", || {
        while let Some(event) = phone.recv_event() {
            phone_show.ingest(&event);
            if let RoomEvent::Seat {
                editor: false,
                limit: 1,
                ..
            } = event
            {
                watching = true;
            }
        }
        watching
    });
    let phone_dot = DotId {
        set_id: 1,
        performer_id: 1,
    };
    // The phone builds an operation to offer, but does not keep it locally:
    // a viewer must follow the editor, not its own rejected drag.
    let phone_op = phone_show
        .clone()
        .move_dot(phone_dot, Point { x: 30.0, y: 30.0 })
        .expect("finite");
    let pc_op = pc_show
        .move_dot(phone_dot, Point { x: 4.0, y: 5.0 })
        .expect("finite");
    let phone_from = phone_show.replica();
    let pc_from = pc_show.replica();
    let mut pc = pc;
    let mut pc_saw_phone = false;
    wait_until(
        "pc move reaches the phone and the phone move does not",
        || {
            phone.send_event(RoomEvent::Op {
                from: phone_from,
                op: phone_op.clone(),
            });
            pc.publish(RoomEvent::Op {
                from: pc_from,
                op: pc_op.clone(),
            });
            while let Some(event) = phone.recv_event() {
                phone_show.ingest(&event);
            }
            while let Some(event) = pc.poll() {
                if let RoomEvent::Op {
                    op: drill_sync::SyncOp::MoveDot { point, .. },
                    ..
                } = &event
                    && *point == (Point { x: 30.0, y: 30.0 })
                {
                    pc_saw_phone = true;
                }
                pc_show.ingest(&event);
            }
            phone_show.dot(phone_dot) == Some(Point { x: 4.0, y: 5.0 }) && !pc_saw_phone
        },
    );
}
