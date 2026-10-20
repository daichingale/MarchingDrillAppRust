//! Two sync clients, one real loopback relay, one room.

use drill_core::{Document, Point};
use drill_sync::{DotId, ReplicaId, RoomEvent, ShowState, SyncClient};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_secs(15);

fn start_relay() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let address = listener.local_addr().expect("local address").to_string();
    std::thread::spawn(move || drill_relay::serve(&listener));
    let deadline = Instant::now() + DEADLINE;
    while Instant::now() < deadline {
        if TcpStream::connect(&address).is_ok() {
            return address;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("relay never accepted a connection on {address}");
}

fn wait_until(deadline_label: &str, mut probe: impl FnMut() -> bool) {
    let deadline = Instant::now() + DEADLINE;
    while Instant::now() < deadline {
        if probe() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for {deadline_label}");
}

#[test]
fn two_clients_converge_on_different_dots() {
    let host = start_relay();
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
        "brass-night",
        alice_show.hello("Alice", [242, 116, 116]),
    );
    let mut bob = SyncClient::connect(&host, "brass-night", bob_show.hello("Bob", [104, 197, 232]));

    let mut alice_saw_bob = false;
    let mut bob_saw_alice = false;
    wait_until("both moves", || {
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
    assert!(alice_show.agrees_with(&bob_show));
}

#[test]
fn a_disconnect_announces_leave() {
    let host = start_relay();
    let document = Document::demo(1, 1);
    let alice_show = ShowState::from_document(&document, ReplicaId::from_raw(11));
    let bob_show = ShowState::from_document(&document, ReplicaId::from_raw(12));
    let alice = SyncClient::connect(&host, "leave-room", alice_show.hello("Alice", [1, 2, 3]));
    let mut bob = SyncClient::connect(&host, "leave-room", bob_show.hello("Bob", [4, 5, 6]));

    wait_until("bob sees alice", || {
        alice.publish(alice_show.hello("Alice", [1, 2, 3]));
        while let Some(event) = bob.poll() {
            if matches!(
                event,
                RoomEvent::Hello { from, .. } if from == ReplicaId::from_raw(11)
            ) {
                return true;
            }
        }
        false
    });
    drop(alice);
    wait_until("bob sees leave", || {
        while let Some(event) = bob.poll() {
            if matches!(
                event,
                RoomEvent::Leave { from } if from == ReplicaId::from_raw(11)
            ) {
                return true;
            }
        }
        false
    });
}

#[test]
fn set_counts_travel_with_the_move() {
    let host = start_relay();
    let document = Document::demo(1, 1);
    let mut alice_show = ShowState::from_document(&document, ReplicaId::from_raw(3));
    let mut bob_show = ShowState::from_document(&document, ReplicaId::from_raw(4));
    let op = alice_show.set_meta(
        1,
        drill_sync::SetFact {
            name: "セット 1".into(),
            counts: 48,
            present: true,
        },
    );
    let alice = SyncClient::connect(&host, "counts", alice_show.hello("A", [0, 0, 0]));
    let mut bob = SyncClient::connect(&host, "counts", bob_show.hello("B", [0, 0, 0]));
    wait_until("counts", || {
        alice.publish(RoomEvent::Op {
            from: ReplicaId::from_raw(3),
            op: op.clone(),
        });
        while let Some(event) = bob.poll() {
            bob_show.ingest(&event);
        }
        bob_show.set_fact(1).is_some_and(|fact| fact.counts == 48)
    });
}
