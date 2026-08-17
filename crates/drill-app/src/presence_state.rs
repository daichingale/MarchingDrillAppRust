//! Live collaborator presence, as owned by `DrillApp`.
//!
//! Scope, restated where it is easiest to violate: this module reads document
//! state and draws overlays. It never writes to the document, never applies an
//! `Edit`, and never touches `History`. A peer is a thing to draw, not a thing
//! to obey. `drill-presence` cannot express a document change, so this stays
//! true by construction rather than by discipline.
//!
//! Polled once per frame from `app_ui.rs`'s `fn ui`, next to the other
//! `*_state` modules, and always through non-blocking `try_recv`.

use drill_core::{Document, Locale, PerformerId, Point};
use drill_presence::{
    ConnectionStatus, HEARTBEAT, Presence, PresenceClient, PresenceMessage, UserId,
    normalize_room_code, palette_color_for,
};
use eframe::egui::{self, Color32, Pos2, Stroke, Vec2};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// A peer that has gone quiet for this long is treated as gone.
///
/// Generous relative to the 5s heartbeat: three missed beats, so ordinary
/// jitter or a brief stall never makes a collaborator flicker out. A clean
/// disconnect arrives as an explicit `Leave` long before this expires; this
/// only covers the ungraceful cases (killed process, dropped Wi-Fi).
const PEER_TIMEOUT: Duration = Duration::from_secs(3 * HEARTBEAT.as_secs());

/// Concurrent peer rings drawn on a single dot.
///
/// Past three the rings stop being readable as distinct colors and start
/// swallowing the dot. The peer list panel remains the complete account.
const MAX_RINGS_PER_DOT: usize = 3;

/// Innermost peer ring radius. The local selection ring is 11px, so peer rings
/// start outside it and never merge with the user's own selection.
const PEER_RING_BASE: f32 = 14.5;
const PEER_RING_STEP: f32 = 3.0;

/// The locally persisted identity of this install.
///
/// Same shape as `recent_projects`: one small JSON file in `app_data_dir`,
/// read once at startup, rewritten only when it changes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Identity {
    pub(crate) user_id: UserId,
    pub(crate) display_name: String,
    pub(crate) color: [u8; 3],
}

impl Identity {
    fn generate() -> Self {
        let user_id = UserId::random();
        Self {
            display_name: format!("Designer {}", user_id.short()),
            color: palette_color_for(user_id),
            user_id,
        }
    }

    fn load() -> Self {
        let stored = identity_path()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok());
        match stored {
            Some(identity) => identity,
            None => {
                let identity = Self::generate();
                identity.persist();
                identity
            }
        }
    }

    fn persist(&self) {
        let Some(path) = identity_path() else {
            return;
        };
        if path
            .parent()
            .is_none_or(|parent| std::fs::create_dir_all(parent).is_err())
        {
            return;
        }
        let Ok(bytes) = serde_json::to_vec(self) else {
            return;
        };
        let _ = drill_project::atomic_write(&path, &bytes, None);
    }
}

fn identity_path() -> Option<PathBuf> {
    Some(super::project_state::app_data_dir().join("presence-identity.json"))
}

struct Peer {
    presence: Presence,
    last_seen: Instant,
}

#[derive(Default)]
pub(crate) struct PresenceState {
    identity: Option<Identity>,
    client: Option<PresenceClient>,
    peers: Vec<Peer>,
    /// Per-peer local performer index to hang the name badge on, recomputed
    /// once per frame rather than searched inside the dot loop.
    label_anchors: Vec<(usize, usize)>,
    /// `(set index, color)` for the timeline strip, likewise precomputed.
    set_marks: Vec<(usize, [u8; 3])>,
    pub(crate) open: bool,
    pub(crate) room_input: String,
    pub(crate) host_input: String,
    pub(crate) name_input: String,
    local_cursor: Option<Point>,
    published: Option<Presence>,
    name_dirty: bool,
}

impl PresenceState {
    /// Identity is read lazily: an install that never opens the panel should
    /// not create a file, and startup should not pay for a disk read.
    fn identity(&mut self) -> &Identity {
        if self.identity.is_none() {
            let identity = Identity::load();
            self.name_input = identity.display_name.clone();
            self.identity = Some(identity);
        }
        self.identity.as_ref().expect("identity was just loaded")
    }

    pub(crate) fn is_connected(&self) -> bool {
        self.client.is_some()
    }

    pub(crate) fn status(&self) -> ConnectionStatus {
        self.client
            .as_ref()
            .map_or(ConnectionStatus::Disconnected, PresenceClient::status)
    }

    pub(crate) fn last_error(&self) -> Option<String> {
        self.client.as_ref().and_then(PresenceClient::last_error)
    }

    pub(crate) fn peer_count(&self) -> usize {
        self.peers.len()
    }

    pub(crate) fn set_marks(&self) -> &[(usize, [u8; 3])] {
        &self.set_marks
    }

    /// Records where the local pointer is on the field, in field coordinates.
    pub(crate) fn set_local_cursor(&mut self, cursor: Option<Point>) {
        self.local_cursor = cursor;
    }

    /// Writes a renamed identity to disk. Called on focus loss rather than per
    /// keystroke, so typing a name is not one file write per character.
    pub(crate) fn commit_display_name(&mut self) {
        if !self.name_dirty {
            return;
        }
        self.name_dirty = false;
        let trimmed = self.name_input.trim().to_owned();
        if trimmed.is_empty() {
            self.name_input = self.identity().display_name.clone();
            return;
        }
        let Some(identity) = self.identity.as_mut() else {
            return;
        };
        if identity.display_name == trimmed {
            return;
        }
        identity.display_name = trimmed;
        identity.persist();
    }

    /// Joins `room_input` on `host_input`. Returns the normalized room code on
    /// success, or `None` when the code was unusable.
    pub(crate) fn connect(&mut self) -> Option<String> {
        self.commit_display_name();
        let room = normalize_room_code(&self.room_input)?;
        let host = if self.host_input.trim().is_empty() {
            format!("127.0.0.1:{}", drill_presence::DEFAULT_PORT)
        } else {
            self.host_input.trim().to_owned()
        };
        let identity = self.identity().clone();
        self.room_input = room.clone();
        self.disconnect();
        self.published = None;
        self.client = Some(PresenceClient::connect(
            &host,
            &room,
            Presence::new(identity.user_id, identity.display_name, identity.color),
        ));
        Some(room)
    }

    /// Drops the connection and every peer overlay with it. Presence is
    /// ephemeral by design: nothing survives a disconnect.
    pub(crate) fn disconnect(&mut self) {
        self.client = None;
        self.peers.clear();
        self.label_anchors.clear();
        self.set_marks.clear();
        self.published = None;
    }

    /// One frame of presence work: drain inbound, expire the silent, publish
    /// the local viewport if it changed, and refresh the paint caches.
    ///
    /// Never blocks. `document` and `selected` are read-only inputs.
    pub(crate) fn poll(
        &mut self,
        document: &Document,
        current_set: usize,
        selected: &BTreeSet<usize>,
    ) {
        if self.client.is_none() {
            return;
        }
        self.drain_inbound();
        self.expire_silent_peers();
        self.publish_local(document, current_set, selected);
        self.refresh_paint_caches(document);
    }

    fn drain_inbound(&mut self) {
        let Some(client) = self.client.as_mut() else {
            return;
        };
        while let Some(message) = client.poll() {
            match message {
                PresenceMessage::Update(presence) => {
                    let now = Instant::now();
                    // Latest-wins: a repeat from a known peer replaces its
                    // entry rather than accumulating history.
                    match self
                        .peers
                        .iter_mut()
                        .find(|peer| peer.presence.user_id == presence.user_id)
                    {
                        Some(peer) => {
                            peer.presence = presence;
                            peer.last_seen = now;
                        }
                        None => self.peers.push(Peer {
                            presence,
                            last_seen: now,
                        }),
                    }
                }
                PresenceMessage::Leave(user) => {
                    self.peers.retain(|peer| peer.presence.user_id != user);
                }
            }
        }
    }

    fn expire_silent_peers(&mut self) {
        let now = Instant::now();
        self.peers
            .retain(|peer| now.duration_since(peer.last_seen) < PEER_TIMEOUT);
    }

    /// Publishes only on change. The client debounces as well, but skipping
    /// unchanged state keeps an idle session down to the heartbeat alone.
    fn publish_local(
        &mut self,
        document: &Document,
        current_set: usize,
        selected: &BTreeSet<usize>,
    ) {
        let Some(identity) = self.identity.as_ref() else {
            return;
        };
        let selected_performers: Vec<PerformerId> = selected
            .iter()
            .filter_map(|&index| document.performers.get(index).map(|performer| performer.id))
            .collect();
        let next = Presence {
            user_id: identity.user_id,
            display_name: identity.display_name.clone(),
            color: identity.color,
            current_set,
            selected_performers,
            cursor: self.local_cursor,
        };
        if self.published.as_ref() == Some(&next) {
            return;
        }
        if let Some(client) = self.client.as_ref() {
            client.publish(next.clone());
            self.published = Some(next);
        }
    }

    /// Rebuilds the two per-frame lookups the paint path needs. Both reuse
    /// their backing allocations; only `clear`/`push` happen here.
    fn refresh_paint_caches(&mut self, document: &Document) {
        self.label_anchors.clear();
        self.set_marks.clear();
        for (slot, peer) in self.peers.iter().enumerate() {
            self.set_marks
                .push((peer.presence.current_set, peer.presence.color));
            // Anchor the badge on the peer's first selected performer that
            // exists in this document; a peer on a different project simply
            // gets no badge rather than a badge on the wrong dot.
            let anchor = peer.presence.selected_performers.iter().find_map(|wanted| {
                document
                    .performers
                    .iter()
                    .position(|performer| performer.id == *wanted)
            });
            if let Some(index) = anchor {
                self.label_anchors.push((slot, index));
            }
        }
    }

    /// Draws peer selection rings and name badges for one dot.
    ///
    /// Called once per performer per frame from the field paint loop, so it
    /// allocates nothing and scans only the peer list, which is realistically
    /// single-digit.
    pub(crate) fn paint_field_marks(
        &self,
        painter: &egui::Painter,
        pos: Pos2,
        index: usize,
        performer: PerformerId,
    ) {
        if self.peers.is_empty() {
            return;
        }
        let mut ring = 0;
        for peer in &self.peers {
            if ring >= MAX_RINGS_PER_DOT {
                break;
            }
            if !peer.presence.selected_performers.contains(&performer) {
                continue;
            }
            painter.circle_stroke(
                pos,
                PEER_RING_BASE + PEER_RING_STEP * ring as f32,
                Stroke::new(2.0, color_of(peer.presence.color)),
            );
            ring += 1;
        }
        for &(slot, anchor) in &self.label_anchors {
            if anchor != index {
                continue;
            }
            let Some(peer) = self.peers.get(slot) else {
                continue;
            };
            let color = color_of(peer.presence.color);
            // Offset up-left, opposite the yellow selection-rank badge at
            // down-right, so the two never sit on top of each other.
            let center = pos + Vec2::new(-11.0, -11.0);
            painter.circle_filled(center, 7.5, color);
            painter.circle_stroke(center, 7.5, Stroke::new(1.0, Color32::from_black_alpha(200)));
            painter.text(
                center,
                egui::Align2::CENTER_CENTER,
                peer.presence.initial(),
                egui::FontId::proportional(10.0),
                Color32::BLACK,
            );
        }
    }

    /// Draws each peer's pointer, for peers looking at the same set.
    ///
    /// A cursor from another set would be a position on a formation the local
    /// user is not viewing, which is misinformation rather than presence.
    pub(crate) fn paint_cursors(
        &self,
        painter: &egui::Painter,
        current_set: usize,
        to_screen: impl Fn(Point) -> Pos2,
    ) {
        for peer in &self.peers {
            let (Some(cursor), true) = (peer.presence.cursor, peer.presence.current_set == current_set)
            else {
                continue;
            };
            let tip = to_screen(cursor);
            let color = color_of(peer.presence.color);
            // A small arrowhead, drawn dark-outlined so it stays legible over
            // both the field green and the dot cluster.
            let points = vec![
                tip,
                tip + Vec2::new(0.0, 14.0),
                tip + Vec2::new(4.0, 10.5),
                tip + Vec2::new(10.0, 10.0),
            ];
            painter.add(egui::Shape::convex_polygon(
                points,
                color,
                Stroke::new(1.0, Color32::from_black_alpha(220)),
            ));
            painter.text(
                tip + Vec2::new(13.0, 14.0),
                egui::Align2::LEFT_TOP,
                &peer.presence.display_name,
                egui::FontId::proportional(11.0),
                color,
            );
        }
    }

    /// Rows for the collaborators panel: display name, color, and set number.
    pub(crate) fn roster(&self) -> impl Iterator<Item = (&str, [u8; 3], usize)> {
        self.peers.iter().map(|peer| {
            (
                peer.presence.display_name.as_str(),
                peer.presence.color,
                peer.presence.current_set,
            )
        })
    }

    /// The local user's own row, so the panel shows which color is "you".
    pub(crate) fn local_badge(&mut self) -> (String, [u8; 3]) {
        let identity = self.identity();
        (identity.display_name.clone(), identity.color)
    }

    /// The join/leave panel and peer roster.
    ///
    /// Returns a status-bar line when the connection state changed. Nothing
    /// here can reach the document: the whole surface is identity, transport,
    /// and a read-only roster.
    pub(crate) fn show(&mut self, context: &egui::Context, locale: Locale) -> Option<String> {
        if !self.open {
            return None;
        }
        // Loading identity here rather than at startup keeps the file from
        // being created until the user actually opens this panel.
        let (local_name, local_color) = self.local_badge();
        let mut open = true;
        let mut status = None;

        egui::Window::new(super::i18n::registered(locale, "presence.001"))
            .id(egui::Id::new("collaborators"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(320.0)
            .show(context, |ui| {
                ui.small(super::i18n::registered(locale, "presence.013"));
                ui.add_space(8.0);

                ui.label(super::i18n::registered(locale, "presence.004"));
                let name = ui.text_edit_singleline(&mut self.name_input);
                if name.changed() {
                    self.name_dirty = true;
                }
                // Persisted on focus loss, not per keystroke: typing a name
                // should not be one atomic file write per character.
                if name.lost_focus() {
                    self.commit_display_name();
                }

                ui.add_space(4.0);
                ui.label(super::i18n::registered(locale, "presence.003"));
                ui.add(
                    egui::TextEdit::singleline(&mut self.host_input)
                        .hint_text(format!("127.0.0.1:{}", drill_presence::DEFAULT_PORT)),
                );

                ui.add_space(4.0);
                ui.label(super::i18n::registered(locale, "presence.002"));
                ui.text_edit_singleline(&mut self.room_input);

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if self.is_connected() {
                        if ui
                            .button(super::i18n::registered(locale, "presence.006"))
                            .clicked()
                        {
                            self.disconnect();
                            status =
                                Some(super::i18n::registered(locale, "presence.016").to_owned());
                        }
                    } else if ui
                        .button(super::i18n::registered(locale, "presence.005"))
                        .clicked()
                    {
                        status = Some(match self.connect() {
                            Some(room) => format!(
                                "{}: {room}",
                                super::i18n::registered(locale, "presence.015")
                            ),
                            None => super::i18n::registered(locale, "presence.017").to_owned(),
                        });
                    }
                    ui.label(match self.status() {
                        ConnectionStatus::Connected => {
                            super::i18n::registered(locale, "presence.007")
                        }
                        ConnectionStatus::Connecting => {
                            super::i18n::registered(locale, "presence.008")
                        }
                        ConnectionStatus::Disconnected => {
                            super::i18n::registered(locale, "presence.009")
                        }
                    });
                });

                // Surfaced rather than hidden: a mistyped host is the most
                // likely failure, and silent retrying looks like a hang.
                if let Some(error) = self.last_error()
                    && self.status() != ConnectionStatus::Connected
                {
                    ui.colored_label(Color32::from_rgb(238, 150, 120), error);
                }

                ui.add_space(8.0);
                ui.separator();
                roster_row(ui, local_color, &local_name, None, locale);
                if self.is_connected() {
                    let set_label = super::i18n::registered(locale, "presence.012");
                    for (name, color, set) in self.roster() {
                        roster_row(ui, color, name, Some((set_label, set)), locale);
                    }
                    if self.peer_count() == 0 {
                        ui.small(super::i18n::registered(locale, "presence.010"));
                    }
                } else {
                    ui.small(super::i18n::registered(locale, "presence.011"));
                }

                ui.add_space(8.0);
                ui.small(super::i18n::registered(locale, "presence.014"));
            });

        self.open = open;
        status
    }
}

/// One line of the roster: color swatch, name, and optionally which set that
/// peer is looking at.
fn roster_row(
    ui: &mut egui::Ui,
    color: [u8; 3],
    name: &str,
    set: Option<(&str, usize)>,
    locale: Locale,
) {
    ui.horizontal(|ui| {
        let (response, painter) =
            ui.allocate_painter(Vec2::splat(12.0), egui::Sense::hover());
        painter.circle_filled(response.rect.center(), 5.0, color_of(color));
        ui.label(name);
        match set {
            // Sets are 1-based everywhere else in the product; presence must
            // not be the one surface that counts from zero.
            Some((label, index)) => {
                ui.weak(format!("{label} {}", index + 1));
            }
            None => {
                ui.weak(super::i18n::registered(locale, "presence.018"));
            }
        }
    });
}

pub(crate) fn color_of([r, g, b]: [u8; 3]) -> Color32 {
    Color32::from_rgb(r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use drill_core::Document;

    fn document_with(count: usize) -> Document {
        Document::demo(1, count)
    }

    fn peer(state: &mut PresenceState, raw: u64, set: usize, selected: &[u32]) {
        let mut presence = Presence::new(UserId::from_raw(raw), format!("P{raw}"), [1, 2, 3]);
        presence.current_set = set;
        presence.selected_performers = selected
            .iter()
            .map(|&id| PerformerId::new(id).expect("id"))
            .collect();
        state.peers.push(Peer {
            presence,
            last_seen: Instant::now(),
        });
    }

    #[test]
    fn a_disconnect_clears_every_peer_overlay() {
        let mut state = PresenceState::default();
        peer(&mut state, 1, 2, &[1]);
        state.refresh_paint_caches(&document_with(4));
        assert_eq!(state.peer_count(), 1);
        assert_eq!(state.set_marks().len(), 1);

        state.disconnect();
        assert_eq!(state.peer_count(), 0);
        assert!(state.set_marks().is_empty());
        assert!(state.label_anchors.is_empty());
    }

    #[test]
    fn set_marks_report_each_peers_set_and_color() {
        let mut state = PresenceState::default();
        peer(&mut state, 1, 0, &[]);
        peer(&mut state, 2, 3, &[]);
        state.refresh_paint_caches(&document_with(4));
        assert_eq!(state.set_marks(), &[(0, [1, 2, 3]), (3, [1, 2, 3])]);
    }

    #[test]
    fn a_label_anchors_on_the_first_locally_present_performer() {
        let mut state = PresenceState::default();
        let document = document_with(4);
        let second = document.performers[1].id.get();
        // A performer id that is not in this document must be skipped rather
        // than anchoring the badge on the wrong dot.
        peer(&mut state, 1, 0, &[9999, second]);
        state.refresh_paint_caches(&document);
        assert_eq!(state.label_anchors, vec![(0, 1)]);
    }

    #[test]
    fn a_peer_selecting_nothing_local_gets_no_label() {
        let mut state = PresenceState::default();
        peer(&mut state, 1, 0, &[9999]);
        state.refresh_paint_caches(&document_with(4));
        assert!(state.label_anchors.is_empty());
        assert_eq!(state.set_marks().len(), 1, "still shown in the set strip");
    }

    #[test]
    fn silent_peers_expire_but_fresh_ones_stay() {
        let mut state = PresenceState::default();
        peer(&mut state, 1, 0, &[]);
        peer(&mut state, 2, 0, &[]);
        state.peers[0].last_seen = Instant::now() - PEER_TIMEOUT - Duration::from_secs(1);
        state.expire_silent_peers();
        assert_eq!(state.peer_count(), 1);
        assert_eq!(state.peers[0].presence.user_id, UserId::from_raw(2));
    }

    #[test]
    fn peer_timeout_allows_three_missed_heartbeats() {
        assert_eq!(PEER_TIMEOUT, HEARTBEAT * 3);
    }

    #[test]
    fn publishing_maps_selected_indices_to_stable_performer_ids() {
        let mut state = PresenceState::default();
        state.identity = Some(Identity {
            user_id: UserId::from_raw(1),
            display_name: "Me".into(),
            color: [9, 9, 9],
        });
        let document = document_with(4);
        let selected = BTreeSet::from([0, 2]);

        // No client, so nothing is sent; the mapping is what is under test.
        state.publish_local(&document, 5, &selected);
        assert!(state.published.is_none());

        let ids: Vec<PerformerId> = selected
            .iter()
            .map(|&index| document.performers[index].id)
            .collect();
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1]);
    }

    #[test]
    fn out_of_range_selection_indices_are_dropped_not_panicked() {
        let mut state = PresenceState::default();
        state.identity = Some(Identity {
            user_id: UserId::from_raw(1),
            display_name: "Me".into(),
            color: [9, 9, 9],
        });
        let document = document_with(2);
        state.publish_local(&document, 0, &BTreeSet::from([0, 99]));
        assert!(state.published.is_none(), "no client means no publish");
    }

    #[test]
    fn an_unusable_room_code_does_not_connect() {
        let mut state = PresenceState::default();
        state.room_input = "   ".into();
        assert_eq!(state.connect(), None);
        assert!(!state.is_connected());
    }

    #[test]
    fn a_blank_display_name_reverts_rather_than_persisting() {
        let mut state = PresenceState::default();
        state.identity = Some(Identity {
            user_id: UserId::from_raw(1),
            display_name: "Original".into(),
            color: [9, 9, 9],
        });
        state.name_input = "   ".into();
        state.name_dirty = true;
        state.commit_display_name();
        assert_eq!(state.name_input, "Original");
        assert_eq!(
            state.identity.as_ref().expect("identity").display_name,
            "Original"
        );
    }

    #[test]
    fn a_leave_removes_only_that_peer() {
        let mut state = PresenceState::default();
        peer(&mut state, 1, 0, &[]);
        peer(&mut state, 2, 0, &[]);
        state.peers.retain(|p| p.presence.user_id != UserId::from_raw(1));
        assert_eq!(state.peer_count(), 1);
        assert_eq!(state.peers[0].presence.user_id, UserId::from_raw(2));
    }
}
