//! Live collaborator *presence* for DrillForge.
//!
//! Scope boundary, stated once and enforced everywhere below: this crate
//! transmits **who is looking at what**, and nothing else. No `Document`, no
//! `Edit`, no `History`, no conflict resolution, no persistence. A peer can
//! never cause a local document mutation through this path, which is what
//! makes an unauthenticated room code an acceptable v0 trade: the worst a
//! stranger who guesses a room code can do is display a bogus cursor.
//!
//! Concurrency follows `drill_jobs::Job<T>`: blocking work lives on a named
//! OS thread and talks to the UI thread through `mpsc`, which the UI drains
//! with non-blocking `try_recv` once per frame. The UI thread never blocks,
//! never joins, and never panics on a network fault.

mod client;
pub mod relay;
mod room;

pub use client::{ConnectionStatus, DEFAULT_PORT, HEARTBEAT, PresenceClient};
pub use room::{Rooms, normalize_room_code, room_code_from_path, room_path};

use drill_core::{PerformerId, Point};
use serde::{Deserialize, Serialize};

/// A collaborator's identity for the lifetime of one install.
///
/// Random on first launch and persisted locally by the client application. It
/// is an identity hint for display, never an authorization token -- nothing on
/// the wire is trusted enough for that to matter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UserId(u64);

impl UserId {
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Draws a fresh identity from the OS-seeded hasher state.
    ///
    /// Deliberately not a `rand` dependency: this is a display handle, not a
    /// key, and `RandomState` is already seeded from the OS. Time is mixed in
    /// so two installs first launched inside the same process-seed epoch on
    /// different machines still diverge.
    #[must_use]
    pub fn random() -> Self {
        use std::hash::{BuildHasher, Hasher, RandomState};
        use std::time::{SystemTime, UNIX_EPOCH};

        let mut hasher = RandomState::new().build_hasher();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos() as u64);
        hasher.write_u64(nanos);
        hasher.write_u64(std::process::id().into());
        Self(hasher.finish())
    }

    /// A short, stable, human-readable form for disambiguating two peers who
    /// picked the same display name.
    #[must_use]
    pub fn short(self) -> String {
        format!("{:04x}", self.0 & 0xffff)
    }
}

/// A pleasant, fixed palette. Fixed rather than generated so two peers never
/// land on near-identical hues, and so the colors stay legible against the
/// dark field canvas that DrillForge draws on.
pub const PALETTE: [[u8; 3]; 8] = [
    [242, 116, 116], // coral
    [246, 176, 74],  // amber
    [232, 216, 96],  // wheat
    [126, 216, 172], // mint
    [104, 197, 232], // sky
    [136, 158, 246], // periwinkle
    [186, 142, 238], // orchid
    [240, 142, 196], // rose
];

/// Picks a palette entry deterministically, so a peer keeps the same color
/// across reconnects without the color needing to travel as trusted state.
#[must_use]
pub fn palette_color_for(user: UserId) -> [u8; 3] {
    PALETTE[(user.get() % PALETTE.len() as u64) as usize]
}

/// Everything one collaborator publishes about their viewport.
///
/// Every field is a *view* concern. There is intentionally no field here that
/// could describe a document change.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Presence {
    pub user_id: UserId,
    pub display_name: String,
    pub color: [u8; 3],
    pub current_set: usize,
    pub selected_performers: Vec<PerformerId>,
    pub cursor: Option<Point>,
}

impl Presence {
    #[must_use]
    pub fn new(user_id: UserId, display_name: String, color: [u8; 3]) -> Self {
        Self {
            user_id,
            display_name,
            color,
            current_set: 0,
            selected_performers: Vec::new(),
            cursor: None,
        }
    }

    /// Caps attacker- (or bug-) controlled growth before anything is drawn or
    /// forwarded. A relay peer is untrusted input like any other parsed file.
    pub fn sanitize(&mut self) {
        const MAX_NAME: usize = 48;
        const MAX_SELECTION: usize = 1024;

        self.display_name = self.display_name.trim().chars().take(MAX_NAME).collect();
        if self.display_name.is_empty() {
            self.display_name = format!("Peer {}", self.user_id.short());
        }
        self.selected_performers.truncate(MAX_SELECTION);
        if self
            .cursor
            .is_some_and(|point| !point.x.is_finite() || !point.y.is_finite())
        {
            self.cursor = None;
        }
    }

    /// The label drawn on the field: one glyph, so it never occludes dots.
    #[must_use]
    pub fn initial(&self) -> char {
        self.display_name
            .chars()
            .find(|character| !character.is_whitespace())
            .unwrap_or('?')
    }
}

/// The complete wire vocabulary.
///
/// Externally tagged JSON: `{"update": {..}}` or `{"leave": 1234}`. One
/// message per WebSocket text frame. There is no ordering guarantee and none
/// is needed -- presence is latest-wins and a stale frame is harmless.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresenceMessage {
    Update(Presence),
    Leave(UserId),
}

impl PresenceMessage {
    /// The identity this message concerns, for room bookkeeping.
    #[must_use]
    pub fn user_id(&self) -> UserId {
        match self {
            Self::Update(presence) => presence.user_id,
            Self::Leave(user) => *user,
        }
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        let mut message: Self = serde_json::from_str(text)?;
        if let Self::Update(presence) = &mut message {
            presence.sanitize();
        }
        Ok(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn performer(raw: u32) -> PerformerId {
        PerformerId::new(raw).expect("non-zero performer id")
    }

    fn sample() -> Presence {
        Presence {
            user_id: UserId::from_raw(0xabcd_1234),
            display_name: "Aiko".into(),
            color: [12, 240, 33],
            current_set: 7,
            selected_performers: vec![performer(1), performer(42)],
            cursor: Some(Point { x: 1.5, y: -2.25 }),
        }
    }

    #[test]
    fn update_round_trips_through_json() {
        let message = PresenceMessage::Update(sample());
        let json = message.to_json().expect("serialize");
        assert_eq!(PresenceMessage::from_json(&json).expect("parse"), message);
    }

    #[test]
    fn leave_round_trips_through_json() {
        let message = PresenceMessage::Leave(UserId::from_raw(99));
        let json = message.to_json().expect("serialize");
        assert_eq!(json, r#"{"leave":99}"#);
        assert_eq!(PresenceMessage::from_json(&json).expect("parse"), message);
    }

    #[test]
    fn wire_shape_is_the_documented_one() {
        let json = PresenceMessage::Update(sample()).to_json().expect("json");
        let value: serde_json::Value = serde_json::from_str(&json).expect("value");
        let update = &value["update"];
        assert_eq!(update["user_id"], 0xabcd_1234_u64);
        assert_eq!(update["display_name"], "Aiko");
        assert_eq!(update["current_set"], 7);
        assert_eq!(update["selected_performers"][1], 42);
        assert_eq!(update["cursor"]["x"], 1.5);
    }

    #[test]
    fn absent_cursor_survives_the_round_trip() {
        let mut presence = sample();
        presence.cursor = None;
        let message = PresenceMessage::Update(presence);
        let json = message.to_json().expect("serialize");
        assert_eq!(PresenceMessage::from_json(&json).expect("parse"), message);
    }

    #[test]
    fn parsing_sanitizes_hostile_peer_payloads() {
        // Written as literal wire text rather than via `to_json`, because a
        // hostile peer is not using this crate's serializer.
        let json = r#"{"update":{"user_id":2882343476,"display_name":"   ",
            "color":[1,2,3],"current_set":0,"selected_performers":[],
            "cursor":{"x":1.0,"y":2.0}}}"#;
        let PresenceMessage::Update(parsed) = PresenceMessage::from_json(json).expect("parse")
        else {
            panic!("expected an update");
        };
        // 0xABCD1234's short form is its low 16 bits.
        assert_eq!(parsed.display_name, "Peer 1234");
    }

    /// JSON has no NaN or Infinity: `serde_json` emits `null`, which then
    /// fails to parse back into `f32`. A non-finite cursor must therefore be
    /// dropped before it is serialized, or the receiving peer would reject
    /// the entire frame -- so `sanitize` is a send-side duty, not only a
    /// receive-side one.
    #[test]
    fn a_non_finite_cursor_is_dropped_before_serialization() {
        let mut presence = sample();
        presence.cursor = Some(Point {
            x: f32::NAN,
            y: f32::INFINITY,
        });
        presence.sanitize();
        assert_eq!(presence.cursor, None);

        let message = PresenceMessage::Update(presence);
        let json = message.to_json().expect("serialize");
        assert_eq!(PresenceMessage::from_json(&json).expect("parse"), message);
    }

    #[test]
    fn sanitize_caps_name_length() {
        let mut presence = sample();
        presence.display_name = "x".repeat(500);
        presence.sanitize();
        assert_eq!(presence.display_name.chars().count(), 48);
    }

    #[test]
    fn initial_skips_leading_whitespace() {
        let mut presence = sample();
        presence.display_name = " Ken".into();
        assert_eq!(presence.initial(), 'K');
    }

    #[test]
    fn malformed_json_is_an_error_not_a_panic() {
        assert!(PresenceMessage::from_json("not json").is_err());
        assert!(PresenceMessage::from_json(r#"{"nope":1}"#).is_err());
    }

    #[test]
    fn palette_assignment_is_stable_and_in_range() {
        let user = UserId::from_raw(1234);
        assert_eq!(palette_color_for(user), palette_color_for(user));
        assert!(PALETTE.contains(&palette_color_for(UserId::from_raw(u64::MAX))));
    }

    #[test]
    fn random_identities_differ() {
        assert_ne!(UserId::random(), UserId::random());
    }

    #[test]
    fn message_reports_its_subject_identity() {
        let user = UserId::from_raw(5);
        assert_eq!(PresenceMessage::Leave(user).user_id(), user);
        assert_eq!(
            PresenceMessage::Update(Presence::new(user, "A".into(), [0, 0, 0])).user_id(),
            user
        );
    }
}
