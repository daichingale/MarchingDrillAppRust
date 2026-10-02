//! End-to-end presence over a real loopback socket, against the real relay.
//!
//! An OS-assigned port (`127.0.0.1:0`) keeps these runnable in parallel with
//! anything else on the machine. Each test waits on a condition with a
//! deadline rather than sleeping a fixed amount, so a slow machine gets more
//! time instead of a flake.

use drill_core::PerformerId;
use drill_presence::{Presence, PresenceClient, PresenceMessage, UserId};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_secs(15);

/// Starts the real relay on a loopback port and returns its `host:port`.
fn start_relay() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let address = listener.local_addr().expect("local address").to_string();
    // Detached: the process exiting is what stops it, which is what the real
    // binary does too.
    std::thread::spawn(move || drill_presence::relay::serve(&listener));

    // The listener is already bound before the thread starts, so a connect
    // cannot race the bind; this just confirms it before the test proceeds.
    let deadline = Instant::now() + DEADLINE;
    while Instant::now() < deadline {
        if TcpStream::connect(&address).is_ok() {
            return address;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("relay never accepted a connection on {address}");
}

fn presence(raw: u64, name: &str) -> Presence {
    Presence::new(UserId::from_raw(raw), name.to_owned(), [10, 20, 30])
}

/// Polls until `predicate` accepts a message, or the deadline passes.
fn wait_for(
    client: &mut PresenceClient,
    label: &str,
    mut predicate: impl FnMut(&PresenceMessage) -> bool,
) -> PresenceMessage {
    let deadline = Instant::now() + DEADLINE;
    while Instant::now() < deadline {
        if let Some(message) = client.poll()
            && predicate(&message)
        {
            return message;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {label}");
}

#[test]
fn two_clients_in_one_room_see_each_other() {
    let host = start_relay();

    let mut alice = PresenceClient::connect(&host, "brass", presence(1, "Alice"));
    let mut bob = PresenceClient::connect(&host, "brass", presence(2, "Bob"));

    // Both published an initial presence on connect. Because a peer that
    // connects second may miss the first peer's opening frame (the relay keeps
    // no history, by design), keep republishing until each side has been seen.
    let deadline = Instant::now() + DEADLINE;
    let mut saw_bob = false;
    let mut saw_alice = false;
    while Instant::now() < deadline && !(saw_bob && saw_alice) {
        let mut alice_state = presence(1, "Alice");
        alice_state.current_set = 4;
        alice_state.selected_performers = vec![PerformerId::new(7).expect("id")];
        alice.publish(alice_state);
        bob.publish(presence(2, "Bob"));

        while let Some(message) = alice.poll() {
            if let PresenceMessage::Update(update) = message
                && update.user_id == UserId::from_raw(2)
            {
                assert_eq!(update.display_name, "Bob");
                saw_bob = true;
            }
        }
        while let Some(message) = bob.poll() {
            if let PresenceMessage::Update(update) = message
                && update.user_id == UserId::from_raw(1)
            {
                assert_eq!(update.display_name, "Alice");
                assert_eq!(update.current_set, 4);
                assert_eq!(
                    update.selected_performers,
                    vec![PerformerId::new(7).expect("id")]
                );
                saw_alice = true;
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }

    assert!(saw_bob, "Alice never received Bob's presence");
    assert!(saw_alice, "Bob never received Alice's presence");
}

#[test]
fn a_peer_in_another_room_is_not_visible() {
    let host = start_relay();

    let mut insider = PresenceClient::connect(&host, "brass", presence(1, "Insider"));
    let outsider = PresenceClient::connect(&host, "drums", presence(2, "Outsider"));
    let partner = PresenceClient::connect(&host, "brass", presence(3, "Partner"));

    // Establish that the room does deliver, so the negative below is a real
    // isolation result rather than "nothing was working yet".
    let deadline = Instant::now() + DEADLINE;
    let mut connected = false;
    while Instant::now() < deadline && !connected {
        partner.publish(presence(3, "Partner"));
        outsider.publish(presence(2, "Outsider"));
        while let Some(PresenceMessage::Update(update)) = insider.poll() {
            assert_ne!(
                update.user_id,
                UserId::from_raw(2),
                "a peer from room 'drums' leaked into room 'brass'"
            );
            if update.user_id == UserId::from_raw(3) {
                connected = true;
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(connected, "same-room peer never arrived");

    // Drain once more now that both peers have certainly published.
    while let Some(PresenceMessage::Update(update)) = insider.poll() {
        assert_ne!(
            update.user_id,
            UserId::from_raw(2),
            "a peer from room 'drums' leaked into room 'brass'"
        );
    }
}

#[test]
fn dropping_a_client_announces_a_leave() {
    let host = start_relay();

    let mut watcher = PresenceClient::connect(&host, "pit", presence(1, "Watcher"));
    let mut leaver = Some(PresenceClient::connect(&host, "pit", presence(2, "Leaver")));

    // Wait until the watcher has actually seen the leaver, otherwise the
    // disconnect could happen before the relay ever learned its identity.
    let deadline = Instant::now() + DEADLINE;
    let mut seen = false;
    while Instant::now() < deadline && !seen {
        if let Some(client) = leaver.as_ref() {
            client.publish(presence(2, "Leaver"));
        }
        while let Some(PresenceMessage::Update(update)) = watcher.poll() {
            if update.user_id == UserId::from_raw(2) {
                seen = true;
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(seen, "watcher never saw the leaver arrive");

    drop(leaver.take());

    let message = wait_for(
        &mut watcher,
        "a Leave for the departed peer",
        |message| matches!(message, PresenceMessage::Leave(user) if *user == UserId::from_raw(2)),
    );
    assert_eq!(message, PresenceMessage::Leave(UserId::from_raw(2)));
}
