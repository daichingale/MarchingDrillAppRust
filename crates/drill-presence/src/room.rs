//! Room scoping: the whole of the relay's routing policy, kept free of socket
//! I/O so it is testable as ordinary data structures.
//!
//! A room code is a shared word, not a secret. It answers "which peers should
//! see each other", on the assumption that everyone on the same rehearsal call
//! already knows it. It is explicitly not an authorization boundary -- see the
//! crate docs for why that is tolerable when presence is all that flows.

use std::collections::HashMap;
use std::sync::mpsc::SyncSender;

/// Identifies one live connection within the relay process.
pub type ConnId = u64;

const MAX_ROOM_CODE: usize = 32;

/// Folds a user-typed room code into the canonical form both ends agree on.
///
/// Case-insensitive because people read codes aloud, and restricted to
/// URL-safe characters so the code can sit in a path segment without percent
/// encoding. Returns `None` when nothing usable survives, which the UI shows
/// as "enter a room code" rather than silently joining a room named "".
#[must_use]
pub fn normalize_room_code(raw: &str) -> Option<String> {
    let code: String = raw
        .trim()
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || *character == '-' || *character == '_'
        })
        .take(MAX_ROOM_CODE)
        .map(|character| character.to_ascii_lowercase())
        .collect();
    (!code.is_empty()).then_some(code)
}

/// The request path a client connects to for `room`.
#[must_use]
pub fn room_path(room: &str) -> String {
    format!("/room/{room}")
}

/// Recovers the room code from an incoming request path.
///
/// Any query string is ignored, and the code is normalized with exactly the
/// same rules the client used, so a peer cannot reach a room the UI could not
/// have produced.
#[must_use]
pub fn room_code_from_path(path: &str) -> Option<String> {
    let path = path.split(['?', '#']).next().unwrap_or(path);
    let rest = path.strip_prefix("/room/")?;
    // Reject nested paths outright rather than guessing which segment was
    // meant: "/room/a/b" is a client bug, not a room named "a".
    if rest.contains('/') {
        return None;
    }
    normalize_room_code(rest)
}

/// Every live connection, grouped by room.
///
/// Each peer is represented only by a bounded outbound queue. The relay
/// therefore cannot inspect, reorder, or rewrite what it forwards -- it moves
/// opaque text between queues, which is the entire server design.
#[derive(Default)]
pub struct Rooms {
    rooms: HashMap<String, Vec<(ConnId, SyncSender<String>)>>,
}

impl Rooms {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn join(&mut self, room: &str, conn: ConnId, outbox: SyncSender<String>) {
        self.rooms
            .entry(room.to_owned())
            .or_default()
            .push((conn, outbox));
    }

    /// Removes a connection, dropping the room entirely once it empties so an
    /// all-day relay does not accumulate one empty `Vec` per rehearsal.
    pub fn leave(&mut self, room: &str, conn: ConnId) {
        let Some(peers) = self.rooms.get_mut(room) else {
            return;
        };
        peers.retain(|(id, _)| *id != conn);
        if peers.is_empty() {
            self.rooms.remove(room);
        }
    }

    /// Fans `payload` out to every peer in `room` except `from`, and returns
    /// how many queues accepted it.
    ///
    /// Never blocks. A closed queue is skipped -- that peer's thread is
    /// already tearing the connection down and will call `leave`. A *full*
    /// queue is also skipped rather than waited on: blocking here would hold
    /// the rooms lock and let one stalled peer freeze the whole relay, and
    /// presence is latest-wins, so the dropped frame is superseded within
    /// milliseconds anyway.
    pub fn broadcast(&self, room: &str, from: ConnId, payload: &str) -> usize {
        let Some(peers) = self.rooms.get(room) else {
            return 0;
        };
        peers
            .iter()
            .filter(|(id, _)| *id != from)
            .filter(|(_, outbox)| outbox.try_send(payload.to_owned()).is_ok())
            .count()
    }

    #[must_use]
    pub fn occupancy(&self, room: &str) -> usize {
        self.rooms.get(room).map_or(0, Vec::len)
    }

    #[must_use]
    pub fn room_count(&self) -> usize {
        self.rooms.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn room_codes_normalize_case_and_padding() {
        assert_eq!(
            normalize_room_code("  Fall-Show  ").as_deref(),
            Some("fall-show")
        );
        assert_eq!(
            normalize_room_code("Rehearsal_1").as_deref(),
            Some("rehearsal_1")
        );
    }

    #[test]
    fn room_codes_drop_unsafe_characters_and_empties() {
        assert_eq!(normalize_room_code("a/b c?d").as_deref(), Some("abcd"));
        assert_eq!(normalize_room_code("   "), None);
        assert_eq!(normalize_room_code("///"), None);
    }

    #[test]
    fn room_codes_are_length_capped() {
        let code = normalize_room_code(&"z".repeat(500)).expect("code");
        assert_eq!(code.len(), MAX_ROOM_CODE);
    }

    #[test]
    fn path_round_trips_through_normalization() {
        let code = normalize_room_code("Fall Show 2026").expect("code");
        assert_eq!(room_path(&code), "/room/fallshow2026");
        assert_eq!(
            room_code_from_path(&room_path(&code)).as_deref(),
            Some(code.as_str())
        );
    }

    #[test]
    fn path_parsing_ignores_query_strings() {
        assert_eq!(
            room_code_from_path("/room/brass?v=1").as_deref(),
            Some("brass")
        );
    }

    #[test]
    fn path_parsing_rejects_unrelated_or_nested_paths() {
        assert_eq!(room_code_from_path("/"), None);
        assert_eq!(room_code_from_path("/rooms/brass"), None);
        assert_eq!(room_code_from_path("/room/"), None);
        assert_eq!(room_code_from_path("/room/a/b"), None);
    }

    #[test]
    fn broadcast_reaches_every_peer_but_the_sender() {
        let mut rooms = Rooms::new();
        let (tx_a, rx_a) = mpsc::sync_channel(8);
        let (tx_b, rx_b) = mpsc::sync_channel(8);
        let (tx_c, rx_c) = mpsc::sync_channel(8);
        rooms.join("brass", 1, tx_a);
        rooms.join("brass", 2, tx_b);
        rooms.join("brass", 3, tx_c);

        assert_eq!(rooms.broadcast("brass", 1, "hello"), 2);
        assert!(rx_a.try_recv().is_err());
        assert_eq!(rx_b.try_recv().expect("b"), "hello");
        assert_eq!(rx_c.try_recv().expect("c"), "hello");
    }

    #[test]
    fn rooms_are_isolated_from_each_other() {
        let mut rooms = Rooms::new();
        let (tx_a, rx_a) = mpsc::sync_channel(8);
        let (tx_b, rx_b) = mpsc::sync_channel(8);
        rooms.join("brass", 1, tx_a);
        rooms.join("drums", 2, tx_b);

        assert_eq!(rooms.broadcast("brass", 9, "only-brass"), 1);
        assert_eq!(rx_a.try_recv().expect("a"), "only-brass");
        assert!(rx_b.try_recv().is_err());
    }

    #[test]
    fn broadcasting_into_an_unknown_room_is_a_no_op() {
        let rooms = Rooms::new();
        assert_eq!(rooms.broadcast("ghost", 1, "hi"), 0);
    }

    #[test]
    fn leaving_prunes_the_peer_then_the_room() {
        let mut rooms = Rooms::new();
        let (tx_a, _rx_a) = mpsc::sync_channel(8);
        let (tx_b, _rx_b) = mpsc::sync_channel(8);
        rooms.join("brass", 1, tx_a);
        rooms.join("brass", 2, tx_b);

        rooms.leave("brass", 1);
        assert_eq!(rooms.occupancy("brass"), 1);
        rooms.leave("brass", 2);
        assert_eq!(rooms.occupancy("brass"), 0);
        assert_eq!(rooms.room_count(), 0);
        rooms.leave("brass", 2);
    }

    #[test]
    fn a_stalled_peer_sheds_frames_instead_of_blocking() {
        let mut rooms = Rooms::new();
        let (tx_stalled, _rx_stalled) = mpsc::sync_channel(2);
        let (tx_healthy, rx_healthy) = mpsc::sync_channel(8);
        rooms.join("brass", 1, tx_stalled);
        rooms.join("brass", 2, tx_healthy);

        // The stalled peer's queue fills after two frames; broadcasting must
        // keep returning promptly and keep serving the healthy peer.
        for _ in 0..5 {
            rooms.broadcast("brass", 9, "tick");
        }
        assert_eq!(rx_healthy.try_iter().count(), 5);
    }

    #[test]
    fn a_dropped_receiver_does_not_count_as_delivered() {
        let mut rooms = Rooms::new();
        let (tx_live, rx_live) = mpsc::sync_channel(8);
        let (tx_dead, rx_dead) = mpsc::sync_channel(8);
        rooms.join("brass", 1, tx_live);
        rooms.join("brass", 2, tx_dead);
        drop(rx_dead);

        assert_eq!(rooms.broadcast("brass", 9, "ping"), 1);
        assert_eq!(rx_live.try_recv().expect("live"), "ping");
    }
}
