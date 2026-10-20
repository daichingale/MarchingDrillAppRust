//! Desktop sync and presence across a relay that speaks TLS itself.

use drill_core::{Document, Point};
use drill_presence::{Presence, PresenceClient, PresenceMessage, UserId};
use drill_sync::{DotId, ReplicaId, RoomEvent, ShowState, SyncClient};
use std::net::{IpAddr, Ipv4Addr, TcpListener, TcpStream};
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_secs(15);

fn local_identity() -> (String, String) {
    let mut params = rcgen::CertificateParams::new(vec!["localhost".to_owned()])
        .expect("certificate parameters");
    params
        .subject_alt_names
        .push(rcgen::SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST)));
    let key = rcgen::KeyPair::generate().expect("key pair");
    let cert = params.self_signed(&key).expect("self-signed certificate");
    (cert.pem(), key.serialize_pem())
}

fn start_tls_relay(cert_pem: &str, key_pem: &str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("local address").port();
    let cert_pem = cert_pem.to_owned();
    let key_pem = key_pem.to_owned();
    std::thread::spawn(move || {
        drill_relay::serve_tls(&listener, cert_pem.as_bytes(), key_pem.as_bytes())
            .expect("tls relay");
    });
    let deadline = Instant::now() + DEADLINE;
    let address = format!("127.0.0.1:{port}");
    while Instant::now() < deadline {
        if TcpStream::connect(&address).is_ok() {
            return port;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("tls relay never accepted a connection on {address}");
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

#[test]
fn desktop_sync_and_presence_cross_a_local_tls_relay() {
    let (cert_pem, key_pem) = local_identity();
    drill_sync::trust_extra_roots(cert_pem.as_bytes()).expect("trust the loopback certificate");
    let port = start_tls_relay(&cert_pem, &key_pem);
    let host = format!("wss://127.0.0.1:{port}");

    let document = Document::demo(2, 2);
    let mut alice_show = ShowState::from_document(&document, ReplicaId::from_raw(1));
    let mut bob_show = ShowState::from_document(&document, ReplicaId::from_raw(2));
    let alice_dot = DotId {
        set_id: 1,
        performer_id: 1,
    };
    let bob_dot = DotId {
        set_id: 1,
        performer_id: 2,
    };
    let alice_op = alice_show
        .move_dot(alice_dot, Point { x: 4.0, y: 6.0 })
        .expect("finite");
    let bob_op = bob_show
        .move_dot(bob_dot, Point { x: 18.0, y: 22.0 })
        .expect("finite");

    let mut alice = SyncClient::connect(
        &host,
        "tls-room",
        alice_show.hello("Alice", [242, 116, 116]),
    );
    let mut bob = SyncClient::connect(&host, "tls-room", bob_show.hello("Bob", [104, 197, 232]));
    assert!(alice.url().starts_with("wss://"));

    let mut alice_saw_bob = false;
    let mut bob_saw_alice = false;
    wait_until("both moves over wss", || {
        alice.publish(RoomEvent::Op {
            from: ReplicaId::from_raw(1),
            op: alice_op.clone(),
        });
        bob.publish(RoomEvent::Op {
            from: ReplicaId::from_raw(2),
            op: bob_op.clone(),
        });
        alice.publish(alice_show.hello("Alice", [242, 116, 116]));
        bob.publish(bob_show.hello("Bob", [104, 197, 232]));
        while let Some(event) = alice.poll() {
            alice_show.ingest(&event);
            if alice_show.dot(bob_dot) == Some(Point { x: 18.0, y: 22.0 }) {
                alice_saw_bob = true;
            }
        }
        while let Some(event) = bob.poll() {
            bob_show.ingest(&event);
            if bob_show.dot(alice_dot) == Some(Point { x: 4.0, y: 6.0 }) {
                bob_saw_alice = true;
            }
        }
        alice_saw_bob && bob_saw_alice && alice_show.agrees_with(&bob_show)
    });

    let mut alice_eyes = PresenceClient::connect(
        &host,
        "tls-room",
        Presence::new(UserId::from_raw(11), "Alice".into(), [242, 116, 116]),
    );
    let bob_eyes = PresenceClient::connect(
        &host,
        "tls-room",
        Presence::new(UserId::from_raw(12), "Bob".into(), [104, 197, 232]),
    );
    let mut cursor = Presence::new(UserId::from_raw(12), "Bob".into(), [104, 197, 232]);
    cursor.cursor = Some(Point { x: 9.0, y: 3.0 });
    wait_until("presence over wss", || {
        bob_eyes.publish(cursor.clone());
        while let Some(message) = alice_eyes.poll() {
            if let PresenceMessage::Update(presence) = message
                && presence.user_id == UserId::from_raw(12)
                && presence.cursor == Some(Point { x: 9.0, y: 3.0 })
            {
                return true;
            }
        }
        false
    });
}
