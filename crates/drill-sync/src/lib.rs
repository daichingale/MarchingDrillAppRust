//! Collaborative edits for a marching show.
//!
//! A school band does not need accounts, and it does not need a general
//! document CRDT. Two people fixing different dots, on a phone and a laptop,
//! possibly after walking through a dead spot in the stadium wifi, need three
//! things: a room code, a deterministic winner when they grab the same dot,
//! and a snapshot a late joiner can merge.
//!
//! This crate is that state. Each performer dot and each set's counts carry a
//! [Lamport stamp](Stamp). A higher stamp replaces a lower one. Equal stamps
//! break the tie by replica id, so two peers who apply the same pair of
//! operations in opposite orders still land on one show. Phone and desktop
//! send the same [`RoomEvent`] JSON. The relay assigns editor seats; it does
//! not merge the show.
//!
//! `yrs` and Automerge are the wrong weight here: they are not in the offline
//! crate cache, they pull a large wasm build, and a drill only conflicts per
//! dot. A last-writer-wins map is the whole merge.

#[cfg(not(target_arch = "wasm32"))]
mod client;

#[cfg(not(target_arch = "wasm32"))]
pub use client::{ConnectionStatus, SyncClient, dial_socket, trust_extra_roots};

use drill_core::{Document, Edit, PerformerId, Point, SetCounts, SetId};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, VecDeque};
use web_time::{Duration, Instant};

/// How often a coalesced drag is allowed onto the wire.
///
/// A finger can move every frame. One dot only needs about 20 updates a
/// second to look continuous, and the frames in between collapse to the
/// latest point.
pub const DRAG_INTERVAL: Duration = Duration::from_millis(50);

/// One install's identity for the life of a room.
///
/// Not an account. A bigger id wins a tie and nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ReplicaId(u64);

impl ReplicaId {
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Logical time plus the replica that produced it.
///
/// Wall clocks on borrowed school laptops are not trustworthy. A counter that
/// only moves forward, with a stable tie-break, is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamp {
    pub lamport: u64,
    pub replica: ReplicaId,
}

impl Stamp {
    /// `true` when `self` must replace `other`.
    ///
    /// A later stamp wins. The same counter yields to the higher replica id,
    /// which is arbitrary and stable: both sides compute the same winner
    /// without talking again.
    #[must_use]
    pub fn beats(self, other: Self) -> bool {
        match self.lamport.cmp(&other.lamport) {
            Ordering::Greater => true,
            Ordering::Less => false,
            Ordering::Equal => self.replica > other.replica,
        }
    }
}

/// One dot on one set. Stable ids, never a list index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DotId {
    pub set_id: u32,
    pub performer_id: u32,
}

/// The set facts a rehearsal actually argues about.
///
/// `present` is a tombstone bit so a future delete does not get resurrected
/// by an older snapshot. This MVP only ever writes `true`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetFact {
    pub name: String,
    pub counts: u16,
    pub present: bool,
}

/// A value and the stamp that wrote it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Lww<T> {
    pub value: T,
    pub stamp: Stamp,
}

/// A single change a peer can apply on its own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SyncOp {
    MoveDot {
        dot: DotId,
        point: Point,
        stamp: Stamp,
    },
    SetMeta {
        set_id: u32,
        fact: SetFact,
        stamp: Stamp,
    },
}

/// Everything that has to converge between two clients.
///
/// The wire form is a list of entries, not a JSON object keyed by dot. JSON
/// object keys have to be strings, and a struct key makes `serde_json` refuse
/// the whole snapshot — which would strand a late joiner.
#[derive(Clone, Debug, PartialEq)]
pub struct ShowState {
    dots: BTreeMap<DotId, Lww<Point>>,
    sets: BTreeMap<u32, Lww<SetFact>>,
    clock: Stamp,
}

impl ShowState {
    /// A baseline taken from a document the user already has open.
    ///
    /// Loaded facts share one neutral stamp (replica 0, lamport 0). Two peers
    /// who open the same file are not in conflict until one of them edits.
    /// The first real edit on any replica beats that baseline. The clock's
    /// replica id is this install, and that is what later stamps carry.
    #[must_use]
    pub fn from_document(document: &Document, replica: ReplicaId) -> Self {
        let stamp = Stamp {
            lamport: 0,
            replica: ReplicaId::from_raw(0),
        };
        let mut dots = BTreeMap::new();
        let mut sets = BTreeMap::new();
        for set in &document.sets {
            sets.insert(
                set.id.get(),
                Lww {
                    value: SetFact {
                        name: clip_name(&set.name),
                        counts: set.counts,
                        present: true,
                    },
                    stamp,
                },
            );
            for (index, performer) in document.performers.iter().enumerate() {
                let Some(point) = set.positions.get(index).copied() else {
                    continue;
                };
                if !point_is_finite(point) {
                    continue;
                }
                dots.insert(
                    DotId {
                        set_id: set.id.get(),
                        performer_id: performer.id.get(),
                    },
                    Lww {
                        value: point,
                        stamp,
                    },
                );
            }
        }
        Self {
            dots,
            sets,
            clock: Stamp {
                lamport: 0,
                replica,
            },
        }
    }

    #[must_use]
    pub fn replica(&self) -> ReplicaId {
        self.clock.replica
    }

    #[must_use]
    pub fn dot(&self, dot: DotId) -> Option<Point> {
        self.dots.get(&dot).map(|entry| entry.value)
    }

    #[must_use]
    pub fn set_fact(&self, set_id: u32) -> Option<&SetFact> {
        self.sets.get(&set_id).map(|entry| &entry.value)
    }

    /// Both shows carry the same dots and set facts, ignoring which replica
    /// owns the local clock.
    #[must_use]
    pub fn agrees_with(&self, other: &ShowState) -> bool {
        self.dots == other.dots && self.sets == other.sets
    }

    /// Records a local move and returns the operation to publish.
    ///
    /// `None` when the point cannot travel in JSON. The local document should
    /// already have rejected that point.
    pub fn move_dot(&mut self, dot: DotId, point: Point) -> Option<SyncOp> {
        if !point_is_finite(point) {
            return None;
        }
        let stamp = self.tick();
        self.dots.insert(
            dot,
            Lww {
                value: point,
                stamp,
            },
        );
        Some(SyncOp::MoveDot { dot, point, stamp })
    }

    /// Records a local change to a set's name or counts.
    pub fn set_meta(&mut self, set_id: u32, mut fact: SetFact) -> SyncOp {
        fact.name = clip_name(&fact.name);
        let stamp = self.tick();
        self.sets.insert(
            set_id,
            Lww {
                value: fact.clone(),
                stamp,
            },
        );
        SyncOp::SetMeta {
            set_id,
            fact,
            stamp,
        }
    }

    /// Merges one remote operation. `true` when the value was replaced.
    ///
    /// Applying the same operation twice is a no-op the second time, which is
    /// what makes a replayed socket frame safe.
    pub fn apply(&mut self, op: &SyncOp) -> bool {
        match op {
            SyncOp::MoveDot { dot, point, stamp } => {
                if !point_is_finite(*point) {
                    self.observe(*stamp);
                    return false;
                }
                self.insert_dot(*dot, *point, *stamp)
            }
            SyncOp::SetMeta {
                set_id,
                fact,
                stamp,
            } => self.insert_set(*set_id, fact.clone(), *stamp),
        }
    }

    /// Pointwise merge of a peer snapshot. The receiver keeps its replica id.
    pub fn merge(&mut self, other: &ShowState) {
        for (dot, incoming) in &other.dots {
            let _ = self.insert_dot(*dot, incoming.value, incoming.stamp);
        }
        for (set_id, incoming) in &other.sets {
            let _ = self.insert_set(*set_id, incoming.value.clone(), incoming.stamp);
        }
    }

    /// Writes winning dots and counts into `document`.
    ///
    /// The desktop app applies [`Edit`] values instead, so undo still works
    /// there. The phone surface has no history stack and calls this directly.
    pub fn write_document(&self, document: &mut Document) {
        for (dot, entry) in &self.dots {
            let Some(index) = document
                .performers
                .iter()
                .position(|performer| performer.id.get() == dot.performer_id)
            else {
                continue;
            };
            let Some(set) = document
                .sets
                .iter_mut()
                .find(|set| set.id.get() == dot.set_id)
            else {
                continue;
            };
            if let Some(slot) = set.positions.get_mut(index)
                && points_differ(*slot, entry.value)
            {
                *slot = entry.value;
            }
        }
        for (set_id, entry) in &self.sets {
            if let Some(set) = document.sets.iter_mut().find(|set| set.id.get() == *set_id)
                && set.counts != entry.value.counts
            {
                set.counts = entry.value.counts;
            }
        }
    }

    /// Folds a room event into this show. `Leave` does not touch the dots.
    pub fn ingest(&mut self, event: &RoomEvent) {
        match event {
            RoomEvent::Op { op, .. } => {
                let _ = self.apply(op);
            }
            RoomEvent::Hello { state, .. } => self.merge(state),
            RoomEvent::Leave { .. } | RoomEvent::Seat { .. } => {}
        }
    }

    /// A full snapshot for someone who joined after the relay forgot the log.
    ///
    /// The relay stores nothing. Without this, a phone that opens the link
    /// late would see an empty room forever.
    #[must_use]
    pub fn hello(&self, name: &str, color: [u8; 3]) -> RoomEvent {
        RoomEvent::Hello {
            from: self.clock.replica,
            name: clip_name(name),
            color,
            state: self.clone(),
        }
    }

    /// Edits that would make `document` match this show.
    ///
    /// Positions that already agree are omitted, so a repeated hello does not
    /// push a new undo entry every two seconds.
    pub fn edits_against(&self, document: &Document) -> Vec<Edit> {
        let mut edits = Vec::new();
        let mut grouped: BTreeMap<u32, (Vec<PerformerId>, Vec<Point>)> = BTreeMap::new();
        for (dot, entry) in &self.dots {
            let Some(set_index) = document
                .sets
                .iter()
                .position(|set| set.id.get() == dot.set_id)
            else {
                continue;
            };
            let Some(performer_index) = document
                .performers
                .iter()
                .position(|performer| performer.id.get() == dot.performer_id)
            else {
                continue;
            };
            let Some(current) = document.sets[set_index].positions.get(performer_index) else {
                continue;
            };
            if !points_differ(*current, entry.value) {
                continue;
            }
            let Some(performer_id) = PerformerId::new(dot.performer_id) else {
                continue;
            };
            let bucket = grouped.entry(dot.set_id).or_default();
            bucket.0.push(performer_id);
            bucket.1.push(entry.value);
        }
        for (set_raw, (performer_ids, positions)) in grouped {
            let Some(set_id) = SetId::new(set_raw) else {
                continue;
            };
            edits.push(Edit::MovePerformers {
                set_id,
                performer_ids,
                positions,
            });
        }
        for (set_raw, entry) in &self.sets {
            let Some(set) = document.sets.iter().find(|set| set.id.get() == *set_raw) else {
                continue;
            };
            if set.counts == entry.value.counts {
                continue;
            }
            let Some(set_id) = SetId::new(*set_raw) else {
                continue;
            };
            edits.push(Edit::SetCounts {
                set_id,
                counts: SetCounts {
                    moves: entry.value.counts,
                    hold: set.hold,
                },
            });
        }
        edits
    }

    fn tick(&mut self) -> Stamp {
        self.clock.lamport = self.clock.lamport.saturating_add(1);
        self.clock
    }

    fn observe(&mut self, stamp: Stamp) {
        if stamp.lamport > self.clock.lamport {
            self.clock.lamport = stamp.lamport;
        }
    }

    fn insert_dot(&mut self, dot: DotId, point: Point, stamp: Stamp) -> bool {
        self.observe(stamp);
        if self
            .dots
            .get(&dot)
            .is_some_and(|existing| !stamp.beats(existing.stamp))
        {
            return false;
        }
        self.dots.insert(
            dot,
            Lww {
                value: point,
                stamp,
            },
        );
        true
    }

    fn insert_set(&mut self, set_id: u32, mut fact: SetFact, stamp: Stamp) -> bool {
        fact.name = clip_name(&fact.name);
        self.observe(stamp);
        if self
            .sets
            .get(&set_id)
            .is_some_and(|existing| !stamp.beats(existing.stamp))
        {
            return false;
        }
        self.sets.insert(set_id, Lww { value: fact, stamp });
        true
    }
}

/// What crosses the room socket, one JSON object per text frame.
///
/// Distinct from presence frames (`{"update": ...}` / `{"leave": n}`), so a
/// cursor client and an edit client can share a fanout and each drop the
/// frames it does not understand.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RoomEvent {
    Op {
        from: ReplicaId,
        op: SyncOp,
    },
    Hello {
        from: ReplicaId,
        name: String,
        color: [u8; 3],
        state: ShowState,
    },
    Leave {
        from: ReplicaId,
    },
    /// The relay telling one connection whether it may edit.
    ///
    /// `editor` is false when the room is already full. That peer still
    /// receives everyone else's operations and must not publish its own.
    Seat {
        replica: ReplicaId,
        editor: bool,
        editors: u16,
        limit: u16,
    },
}

impl RoomEvent {
    #[must_use]
    pub fn replica(&self) -> ReplicaId {
        match self {
            Self::Op { from, .. }
            | Self::Hello { from, .. }
            | Self::Leave { from }
            | Self::Seat { replica: from, .. } => *from,
        }
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Parses one frame. Hostile names are clipped. Oversized frames are refused.
    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        const MAX_FRAME: usize = 4_000_000;
        if text.len() > MAX_FRAME {
            return Err(serde::de::Error::custom("sync frame is too large"));
        }
        let mut event: Self = serde_json::from_str(text)?;
        if let Self::Hello { name, .. } = &mut event {
            *name = clip_name(name);
        }
        Ok(event)
    }
}

#[derive(Serialize, Deserialize)]
struct DotWire {
    dot: DotId,
    point: Point,
    stamp: Stamp,
}

#[derive(Serialize, Deserialize)]
struct SetWire {
    set_id: u32,
    fact: SetFact,
    stamp: Stamp,
}

#[derive(Serialize, Deserialize)]
struct ShowWire {
    dots: Vec<DotWire>,
    sets: Vec<SetWire>,
    clock: Stamp,
}

impl Serialize for ShowState {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let wire = ShowWire {
            dots: self
                .dots
                .iter()
                .map(|(dot, entry)| DotWire {
                    dot: *dot,
                    point: entry.value,
                    stamp: entry.stamp,
                })
                .collect(),
            sets: self
                .sets
                .iter()
                .map(|(set_id, entry)| SetWire {
                    set_id: *set_id,
                    fact: entry.value.clone(),
                    stamp: entry.stamp,
                })
                .collect(),
            clock: self.clock,
        };
        wire.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ShowState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = ShowWire::deserialize(deserializer)?;
        let mut show = Self {
            dots: BTreeMap::new(),
            sets: BTreeMap::new(),
            clock: wire.clock,
        };
        for dot in wire.dots {
            let _ = show.insert_dot(dot.dot, dot.point, dot.stamp);
        }
        for set in wire.sets {
            let _ = show.insert_set(set.set_id, set.fact, set.stamp);
        }
        Ok(show)
    }
}

/// Outbound room frames with drags collapsed to the latest point.
///
/// The desktop socket and the browser socket both use this, so a phone and a
/// PC publish the same shape at the same rate.
#[derive(Default)]
pub struct CoalescedOutbox {
    ready: VecDeque<RoomEvent>,
    held_moves: Vec<RoomEvent>,
    last_move: Option<Instant>,
}

const OUTBOX_CAP: usize = 512;

impl CoalescedOutbox {
    pub fn push(&mut self, event: RoomEvent) {
        if let Some(dot) = move_dot(&event) {
            if let Some(slot) = self
                .ready
                .iter_mut()
                .find(|queued| move_dot(queued) == Some(dot))
            {
                *slot = event;
                return;
            }
            if let Some(slot) = self
                .held_moves
                .iter_mut()
                .find(|queued| move_dot(queued) == Some(dot))
            {
                *slot = event;
                return;
            }
            self.held_moves.push(event);
            return;
        }
        if matches!(event, RoomEvent::Hello { .. }) {
            self.ready
                .retain(|queued| !matches!(queued, RoomEvent::Hello { .. }));
        }
        self.ready.push_back(event);
        while self.ready.len() > OUTBOX_CAP {
            self.ready.pop_front();
        }
    }

    /// Puts a frame back when the socket refused it.
    pub fn unsend(&mut self, event: RoomEvent) {
        self.ready.push_front(event);
    }

    /// Next frame that may be written now. Moves wait for [`DRAG_INTERVAL`].
    pub fn pop_ready(&mut self, now: Instant) -> Option<RoomEvent> {
        self.release_moves(now);
        self.ready.pop_front()
    }

    fn release_moves(&mut self, now: Instant) {
        if self.held_moves.is_empty() {
            return;
        }
        let due = self
            .last_move
            .is_none_or(|sent| now.duration_since(sent) >= DRAG_INTERVAL);
        if !due {
            return;
        }
        self.ready.extend(self.held_moves.drain(..));
        self.last_move = Some(now);
        while self.ready.len() > OUTBOX_CAP {
            self.ready.pop_front();
        }
    }
}

fn move_dot(event: &RoomEvent) -> Option<DotId> {
    match event {
        RoomEvent::Op {
            op: SyncOp::MoveDot { dot, .. },
            ..
        } => Some(*dot),
        _ => None,
    }
}

/// `ws://` or `wss://` URL for `/room/<code>`.
///
/// A typed `wss://` or `https://` host stays on TLS. A typed `ws://` or
/// `http://` stays plain. A bare host uses `secure`, which a browser sets
/// from the page. Room codes follow the same folding as presence.
#[must_use]
pub fn socket_url(host: &str, room: &str, secure: bool) -> String {
    let trimmed = host.trim().trim_end_matches('/');
    let explicit = if trimmed.starts_with("wss://") || trimmed.starts_with("https://") {
        Some(true)
    } else if trimmed.starts_with("ws://") || trimmed.starts_with("http://") {
        Some(false)
    } else {
        None
    };
    let host = trimmed
        .strip_prefix("wss://")
        .or_else(|| trimmed.strip_prefix("ws://"))
        .or_else(|| trimmed.strip_prefix("https://"))
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    let scheme = if explicit.unwrap_or(secure) {
        "wss"
    } else {
        "ws"
    };
    format!("{scheme}://{host}/room/{}", normalize_room_code(room))
}

#[must_use]
pub fn normalize_room_code(room: &str) -> String {
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

fn clip_name(name: &str) -> String {
    let clipped: String = name.trim().chars().take(48).collect();
    if clipped.is_empty() {
        "Peer".to_owned()
    } else {
        clipped
    }
}

fn point_is_finite(point: Point) -> bool {
    point.x.is_finite() && point.y.is_finite()
}

fn points_differ(left: Point, right: Point) -> bool {
    (left.x - right.x).abs() > 0.001 || (left.y - right.y).abs() > 0.001
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dot(set_id: u32, performer_id: u32) -> DotId {
        DotId {
            set_id,
            performer_id,
        }
    }

    fn pair() -> (Document, ShowState, ShowState) {
        let document = Document::demo(2, 2);
        let alice = ShowState::from_document(&document, ReplicaId::from_raw(1));
        let bob = ShowState::from_document(&document, ReplicaId::from_raw(2));
        (document, alice, bob)
    }

    #[test]
    fn a_typed_wss_host_stays_on_tls() {
        assert_eq!(
            socket_url("wss://relay.example", "Brass Night", false),
            "wss://relay.example/room/brassnight"
        );
        assert_eq!(
            socket_url("https://relay.example:443/", "brass", false),
            "wss://relay.example:443/room/brass"
        );
        assert_eq!(
            socket_url("ws://127.0.0.1:8787", "brass", true),
            "ws://127.0.0.1:8787/room/brass"
        );
        assert_eq!(
            socket_url("127.0.0.1:8787", "brass", true),
            "wss://127.0.0.1:8787/room/brass"
        );
    }

    #[test]
    fn different_dots_both_survive() {
        let (_document, mut alice, mut bob) = pair();
        let first = dot(1, 1);
        let second = dot(1, 2);
        let alice_op = alice
            .move_dot(first, Point { x: 3.0, y: 4.0 })
            .expect("finite");
        let bob_op = bob
            .move_dot(second, Point { x: 8.0, y: 9.0 })
            .expect("finite");
        assert!(bob.apply(&alice_op));
        assert!(alice.apply(&bob_op));
        assert_eq!(alice.dot(first), Some(Point { x: 3.0, y: 4.0 }));
        assert_eq!(alice.dot(second), Some(Point { x: 8.0, y: 9.0 }));
        assert_eq!(alice.dots, bob.dots);
    }

    #[test]
    fn same_dot_higher_lamport_wins() {
        let (document, mut alice, mut bob) = pair();
        let target = dot(1, 1);
        let early = alice
            .move_dot(target, Point { x: 1.0, y: 1.0 })
            .expect("finite");
        let late = alice
            .move_dot(target, Point { x: 4.0, y: 5.0 })
            .expect("finite");
        let bob_op = bob
            .move_dot(target, Point { x: 9.0, y: 9.0 })
            .expect("finite");
        // Fresh views, so each side applies the same three operations.
        // Bob's replica id is higher, but his counter is still 1. Alice's
        // second write is counter 2 and must win on both sides.
        let mut alice_view = ShowState::from_document(&document, ReplicaId::from_raw(1));
        let mut bob_view = ShowState::from_document(&document, ReplicaId::from_raw(2));
        assert!(alice_view.apply(&bob_op));
        assert!(alice_view.apply(&late));
        assert!(bob_view.apply(&early));
        assert!(bob_view.apply(&bob_op));
        assert!(bob_view.apply(&late));
        assert!(!alice.apply(&bob_op));
        assert_eq!(alice_view.dot(target), Some(Point { x: 4.0, y: 5.0 }));
        assert_eq!(bob_view.dot(target), alice_view.dot(target));
    }

    #[test]
    fn equal_lamport_higher_replica_wins() {
        let (_document, mut alice, mut bob) = pair();
        let target = dot(1, 3);
        let alice_op = alice
            .move_dot(target, Point { x: 1.0, y: 2.0 })
            .expect("finite");
        let bob_op = bob
            .move_dot(target, Point { x: 7.0, y: 8.0 })
            .expect("finite");
        assert!(alice.apply(&bob_op));
        assert!(!bob.apply(&alice_op));
        assert_eq!(alice.dot(target), Some(Point { x: 7.0, y: 8.0 }));
        assert_eq!(bob.dot(target), alice.dot(target));
    }

    #[test]
    fn apply_is_idempotent() {
        let (_document, mut alice, mut bob) = pair();
        let op = bob
            .move_dot(dot(1, 1), Point { x: 6.0, y: 6.0 })
            .expect("finite");
        assert!(alice.apply(&op));
        assert!(!alice.apply(&op));
        assert_eq!(alice.dot(dot(1, 1)), Some(Point { x: 6.0, y: 6.0 }));
    }

    #[test]
    fn merge_is_commutative() {
        let (_document, mut alice, mut bob) = pair();
        let _ = alice.move_dot(dot(1, 1), Point { x: 2.0, y: 3.0 });
        let _ = alice.set_meta(
            1,
            SetFact {
                name: "オープニング".into(),
                counts: 24,
                present: true,
            },
        );
        let _ = bob.move_dot(dot(1, 2), Point { x: 11.0, y: 12.0 });
        let _ = bob.set_meta(
            2,
            SetFact {
                name: "セット 2".into(),
                counts: 8,
                present: true,
            },
        );
        let mut left = alice.clone();
        let mut right = bob.clone();
        left.merge(&bob);
        right.merge(&alice);
        assert_eq!(left.dots, right.dots);
        assert_eq!(left.sets, right.sets);
        assert_eq!(left.clock.lamport, right.clock.lamport);
        assert_eq!(left.set_fact(1).expect("set").counts, 24);
        assert_eq!(left.set_fact(2).expect("set").counts, 8);
        assert_eq!(left.dot(dot(1, 1)), Some(Point { x: 2.0, y: 3.0 }));
        assert_eq!(left.dot(dot(1, 2)), Some(Point { x: 11.0, y: 12.0 }));
    }

    #[test]
    fn offline_logs_converge_when_exchanged() {
        let (_document, mut alice, mut bob) = pair();
        let mut alice_log = Vec::new();
        let mut bob_log = Vec::new();
        alice_log.push(
            alice
                .move_dot(dot(1, 1), Point { x: 4.0, y: 1.0 })
                .expect("finite"),
        );
        alice_log.push(alice.set_meta(
            1,
            SetFact {
                name: "セット 1".into(),
                counts: 32,
                present: true,
            },
        ));
        bob_log.push(
            bob.move_dot(dot(2, 4), Point { x: 20.0, y: 30.0 })
                .expect("finite"),
        );
        bob_log.push(
            bob.move_dot(dot(1, 1), Point { x: 5.0, y: 5.0 })
                .expect("finite"),
        );
        for op in &bob_log {
            alice.apply(op);
        }
        for op in &alice_log {
            bob.apply(op);
        }
        assert_eq!(alice.dots, bob.dots);
        assert_eq!(alice.sets, bob.sets);
        // Alice's write of the shared dot is lamport 1. Bob writes it second,
        // at lamport 2, so Bob's point wins on both sides after the exchange.
        assert_eq!(alice.dot(dot(1, 1)), Some(Point { x: 5.0, y: 5.0 }));
        assert_eq!(alice.set_fact(1).expect("set").counts, 32);
        assert_eq!(alice.dot(dot(2, 4)), Some(Point { x: 20.0, y: 30.0 }));
    }

    #[test]
    fn a_repeated_hello_does_not_invent_edits() {
        let (document, alice, _bob) = pair();
        assert!(alice.edits_against(&document).is_empty());
        let mut moved = alice.clone();
        let _ = moved.move_dot(dot(1, 1), Point { x: 1.5, y: 2.5 });
        let edits = moved.edits_against(&document);
        assert_eq!(edits.len(), 1);
        let Edit::MovePerformers {
            performer_ids,
            positions,
            ..
        } = &edits[0]
        else {
            panic!("expected a move");
        };
        assert_eq!(performer_ids.len(), 1);
        assert_eq!(positions[0], Point { x: 1.5, y: 2.5 });
        let mut document = document;
        let _ = edits[0].clone().apply(&mut document);
        assert!(moved.edits_against(&document).is_empty());
    }

    #[test]
    fn presence_frames_are_not_sync_events() {
        assert!(RoomEvent::from_json(r#"{"leave":9}"#).is_err());
        assert!(RoomEvent::from_json(r#"{"update":{"user_id":1}}"#).is_err());
    }

    #[test]
    fn hello_round_trips_and_clips_names() {
        let (_document, alice, _bob) = pair();
        let hello = alice.hello("  指揮者  ", [1, 2, 3]);
        let json = hello.to_json().expect("json");
        let parsed = RoomEvent::from_json(&json).expect("parse");
        assert_eq!(parsed, hello);
        let RoomEvent::Hello { name, .. } = parsed else {
            panic!("hello");
        };
        assert_eq!(name, "指揮者");
    }

    #[test]
    fn drags_of_one_dot_collapse_to_the_latest_point_per_interval() {
        let (_document, mut alice, _) = pair();
        let dot = dot(1, 1);
        let mut outbox = CoalescedOutbox::default();
        for step in 0..8 {
            let op = alice
                .move_dot(
                    dot,
                    Point {
                        x: step as f32,
                        y: 1.0,
                    },
                )
                .expect("finite");
            outbox.push(RoomEvent::Op {
                from: alice.replica(),
                op,
            });
        }
        let now = Instant::now();
        let RoomEvent::Op {
            op: SyncOp::MoveDot { point, .. },
            ..
        } = outbox.pop_ready(now).expect("one drag")
        else {
            panic!("move");
        };
        assert_eq!(point, Point { x: 7.0, y: 1.0 });
        assert!(outbox.pop_ready(now).is_none());
        let op = alice
            .move_dot(dot, Point { x: 9.0, y: 4.0 })
            .expect("finite");
        outbox.push(RoomEvent::Op {
            from: alice.replica(),
            op,
        });
        assert!(outbox.pop_ready(now).is_none());
        let later = now + DRAG_INTERVAL;
        let RoomEvent::Op {
            op: SyncOp::MoveDot { point, .. },
            ..
        } = outbox.pop_ready(later).expect("next interval")
        else {
            panic!("move");
        };
        assert_eq!(point, Point { x: 9.0, y: 4.0 });
    }

    #[test]
    fn phone_and_desktop_encode_the_same_drag() {
        let (_document, mut alice, _) = pair();
        let op = alice
            .move_dot(dot(1, 1), Point { x: 3.0, y: 4.0 })
            .expect("finite");
        let event = RoomEvent::Op {
            from: alice.replica(),
            op,
        };
        let json = event.to_json().expect("json");
        assert_eq!(RoomEvent::from_json(&json).expect("parse"), event);
        assert!(json.contains("\"kind\":\"op\""));
        assert!(json.contains("\"op\":\"move_dot\""));
    }
}
