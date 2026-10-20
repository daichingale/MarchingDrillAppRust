//! Touch field: open a show, play it, tap a performer, drag them.
//!
//! This surface is the flat field only: a plan view, playback, and a share link.

use crate::touch::{
    FieldView, apply_two_finger, chrome_is_compact, dot_radius, hit_radius, pick_performer,
};
use drill_core::{Document, Point};
use drill_render::{FieldMap, Vec2};
use drill_sync::{DotId, ReplicaId, RoomEvent, ShowState};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2 as EVec2};
use std::time::Instant;

const INK: Color32 = Color32::from_rgb(32, 36, 44);
const BLUE: Color32 = Color32::from_rgb(20, 96, 200);
const GRASS: Color32 = Color32::from_rgb(16, 84, 46);
const PAPER: Color32 = Color32::WHITE;
const GOLD: Color32 = Color32::from_rgb(245, 197, 66);

struct SplitDemo {
    left: Document,
    right: Document,
    room: String,
}

pub struct PhoneApp {
    document: Document,
    show: ShowState,
    view: FieldView,
    current_set: usize,
    local_count: f32,
    playing: bool,
    last_tick: Instant,
    selected: Option<usize>,
    press_origin: Option<Pos2>,
    suppress_drag: bool,
    ja: bool,
    more: bool,
    host: String,
    room: String,
    status: String,
    split: Option<SplitDemo>,
    applying: bool,
    view_only: bool,
    editor_limit: u16,
    last_hello: Option<Instant>,
    #[cfg(not(target_arch = "wasm32"))]
    relay: Option<drill_sync::SyncClient>,
    #[cfg(target_arch = "wasm32")]
    relay: Option<crate::socket::BrowserClient>,
}

impl Default for PhoneApp {
    fn default() -> Self {
        Self::new()
    }
}

impl PhoneApp {
    #[must_use]
    pub fn new() -> Self {
        let mut app = Self::from_document(Document::demo(2, 4));
        match std::env::var("DRILLFORGE_WEB_QA").ok().as_deref() {
            Some("split") => app.install_split(),
            Some("phone") => app.selected = Some(0),
            Some("share") => {
                app.more = true;
                app.selected = Some(0);
            }
            _ => {}
        }
        app
    }

    fn from_document(document: Document) -> Self {
        let positions = document
            .sets
            .first()
            .map(|set| set.positions.as_slice())
            .unwrap_or(&[]);
        let view = FieldView::frame_cast(document.grid.width, document.grid.height, positions);
        let show = ShowState::from_document(&document, ReplicaId::from_raw(replica_seed()));
        Self {
            document,
            show,
            view,
            current_set: 0,
            local_count: 0.0,
            playing: false,
            last_tick: Instant::now(),
            selected: None,
            press_origin: None,
            suppress_drag: false,
            ja: true,
            more: false,
            host: "127.0.0.1:8787".into(),
            room: String::new(),
            status: String::new(),
            split: None,
            applying: false,
            view_only: false,
            editor_limit: 0,
            last_hello: None,
            relay: None,
        }
    }

    fn install_split(&mut self) {
        let (left, right) = synced_pair();
        let positions = left
            .sets
            .first()
            .map(|set| set.positions.clone())
            .unwrap_or_default();
        self.view = FieldView::frame_cast(left.grid.width, left.grid.height, &positions);
        self.document = left.clone();
        self.show = ShowState::from_document(&self.document, ReplicaId::from_raw(1));
        self.split = Some(SplitDemo {
            left,
            right,
            room: "brass-night".into(),
        });
        self.selected = Some(0);
    }

    fn load_document(&mut self, document: Document) {
        let positions = document
            .sets
            .first()
            .map(|set| set.positions.as_slice())
            .unwrap_or(&[]);
        let view = FieldView::frame_cast(document.grid.width, document.grid.height, positions);
        self.show = ShowState::from_document(&document, self.show.replica());
        self.document = document;
        self.view = view;
        self.current_set = 0;
        self.local_count = 0.0;
        self.playing = false;
        self.selected = None;
        self.split = None;
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.relay = None;
        }
    }

    fn open_bytes(&mut self, bytes: &[u8]) {
        let Ok(text) = std::str::from_utf8(bytes) else {
            return;
        };
        if let Ok(document) = Document::from_json(text) {
            self.load_document(document);
            self.status = word(self.ja, "開きました", "Opened").to_owned();
        }
    }

    fn take_drops(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|input| input.raw.dropped_files.clone());
        for file in dropped {
            if let Some(bytes) = file.bytes {
                self.open_bytes(&bytes);
            } else if let Some(path) = file.path {
                self.open_path(&path);
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn open_path(&mut self, path: &std::path::Path) {
        if let Ok(text) = std::fs::read_to_string(path)
            && let Ok(document) = Document::from_json(&text)
        {
            self.load_document(document);
            self.status = word(self.ja, "開きました", "Opened").to_owned();
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn open_path(&mut self, _path: &std::path::Path) {}

    fn advance(&mut self) {
        if !self.playing {
            return;
        }
        let dt = self.last_tick.elapsed().as_secs_f32().min(0.1);
        self.last_tick = Instant::now();
        if dt <= 0.0 {
            return;
        }
        let global = self
            .document
            .global_count(self.current_set, self.local_count);
        let bpm = self.document.tempo.bpm_at(global).max(1.0);
        self.local_count += dt * bpm / 60.0;
        let span = self
            .document
            .sets
            .get(self.current_set)
            .map(|set| f32::from(set.counts) + f32::from(set.hold))
            .unwrap_or(0.0);
        if self.local_count >= span {
            if self.current_set + 1 < self.document.sets.len() {
                self.current_set += 1;
                self.local_count = 0.0;
            } else {
                self.playing = false;
                self.local_count = 0.0;
            }
        }
    }

    fn positions_now(&self) -> Vec<Point> {
        if self.playing {
            let mut out = Vec::new();
            self.document
                .positions_at_count(self.current_set, self.local_count, &mut out);
            out
        } else {
            self.document
                .sets
                .get(self.current_set)
                .map(|set| set.positions.clone())
                .unwrap_or_default()
        }
    }

    fn place_selected(&mut self, index: usize, point: Point) {
        if self.view_only
            || self.applying
            || self.playing
            || !point.x.is_finite()
            || !point.y.is_finite()
        {
            if self.view_only {
                self.status = viewer_message(self.ja, self.editor_limit);
            }
            return;
        }
        let Some(set_id) = self
            .document
            .sets
            .get(self.current_set)
            .map(|set| set.id.get())
        else {
            return;
        };
        let Some(performer_id) = self
            .document
            .performers
            .get(index)
            .map(|performer| performer.id.get())
        else {
            return;
        };
        let Some(slot) = self
            .document
            .sets
            .get_mut(self.current_set)
            .and_then(|set| set.positions.get_mut(index))
        else {
            return;
        };
        *slot = point;
        let Some(op) = self.show.move_dot(
            DotId {
                set_id,
                performer_id,
            },
            point,
        ) else {
            return;
        };
        self.publish(RoomEvent::Op {
            from: self.show.replica(),
            op,
        });
    }

    fn publish(&self, event: RoomEvent) {
        if self.view_only && matches!(event, RoomEvent::Op { .. } | RoomEvent::Hello { .. }) {
            return;
        }
        if let Some(client) = &self.relay {
            client.publish(event);
        }
    }

    fn poll_room(&mut self) {
        self.announce_if_due();
        let mut events = Vec::new();
        if let Some(client) = self.relay.as_mut() {
            while let Some(event) = client.poll() {
                events.push(event);
            }
        }
        if events.is_empty() {
            return;
        }
        for event in &events {
            if let RoomEvent::Seat { editor, limit, .. } = event {
                self.note_seat(*editor, *limit);
            }
        }
        self.applying = true;
        for event in &events {
            self.show.ingest(event);
        }
        self.show.write_document(&mut self.document);
        self.applying = false;
    }

    fn note_seat(&mut self, editor: bool, limit: u16) {
        self.editor_limit = limit;
        if editor && self.view_only {
            self.view_only = false;
            self.status = word(self.ja, "編集できるようになりました。", "You can edit now.").into();
        } else if !editor && !self.view_only {
            self.view_only = true;
            self.status = viewer_message(self.ja, limit);
        }
    }

    fn announce_if_due(&mut self) {
        if self.view_only || self.relay.is_none() {
            return;
        }
        let due = self
            .last_hello
            .is_none_or(|last| last.elapsed() >= std::time::Duration::from_secs(2));
        if !due {
            return;
        }
        let hello = self
            .show
            .hello(word(self.ja, "スマホ", "Phone"), [242, 116, 116]);
        self.publish(hello);
        self.last_hello = Some(Instant::now());
    }

    fn join_room(&mut self) {
        let room = self.room.trim().to_owned();
        if room.is_empty() {
            self.status =
                word(self.ja, "部屋の名前を入れてください", "Enter a room code").to_owned();
            return;
        }
        self.room = room;
        self.view_only = false;
        let hello = self
            .show
            .hello(word(self.ja, "スマホ", "Phone"), [242, 116, 116]);
        let secure = page_is_https();
        self.status = drill_sync::socket_url(self.host.trim(), &self.room, secure);
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.relay = Some(drill_sync::SyncClient::connect(
                self.host.trim(),
                &self.room,
                hello,
            ));
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.relay = crate::socket::BrowserClient::connect(self.host.trim(), &self.room, hello);
            if self.relay.is_none() {
                self.status = word(
                    self.ja,
                    "リレーに接続できません",
                    "Could not open the room socket",
                )
                .into();
            }
        }
        self.last_hello = Some(Instant::now());
    }

    fn step_set(&mut self, delta: isize) {
        let len = self.document.sets.len();
        if len == 0 {
            return;
        }
        let next = self.current_set as isize + delta;
        if (0..len as isize).contains(&next) {
            self.current_set = next as usize;
            self.local_count = 0.0;
        }
    }
}

impl eframe::App for PhoneApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        Color32::from_rgb(244, 246, 248).to_normalized_gamma_f32()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.take_drops(ui.ctx());
        self.poll_room();
        self.advance();
        if self.playing || self.relay.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(50));
        }

        let full = ui.max_rect();
        let compact = chrome_is_compact(full.width());
        let bar_h = if self.more { 132.0 } else { 64.0 };
        let bar = Rect::from_min_size(full.min, EVec2::new(full.width(), bar_h));
        let field = Rect::from_min_max(Pos2::new(full.min.x, bar.max.y), full.max);
        self.toolbar(ui, bar, compact);
        if self.split.is_some() {
            self.split_body(ui, field);
        } else {
            self.field_body(ui, field, compact);
        }
    }
}

impl PhoneApp {
    fn toolbar(&mut self, ui: &mut egui::Ui, rect: Rect, compact: bool) {
        ui.painter().rect_filled(rect, 0.0, PAPER);
        ui.painter().hline(
            rect.x_range(),
            rect.bottom(),
            Stroke::new(1.0, Color32::from_rgb(210, 214, 220)),
        );
        let ja = self.ja;
        let playing = self.playing;
        let set_name = self
            .document
            .sets
            .get(self.current_set)
            .map(|set| set.name.clone())
            .unwrap_or_default();
        let top = Rect::from_min_size(
            rect.min + EVec2::new(8.0, 8.0),
            EVec2::new((rect.width() - 16.0).max(1.0), 48.0),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(top), |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let play = if playing {
                    word(ja, "停止", "Stop")
                } else {
                    word(ja, "再生", "Play")
                };
                let play_fill = if playing {
                    Color32::from_rgb(160, 48, 48)
                } else {
                    BLUE
                };
                if fill_button(ui, play, play_fill).clicked() {
                    self.playing = !self.playing;
                    self.last_tick = Instant::now();
                }
                if ink_button(ui, "◀").clicked() {
                    self.step_set(-1);
                }
                if ink_button(ui, "▶").clicked() {
                    self.step_set(1);
                }
                ui.label(egui::RichText::new(set_name).size(16.0).color(INK));
                if compact {
                    let menu = word(ja, "メニュー", "Menu");
                    if ink_button(ui, menu).clicked() {
                        self.more = !self.more;
                    }
                } else {
                    if ink_button(ui, word(ja, "見本", "Sample")).clicked() {
                        self.load_document(Document::demo(2, 4));
                    }
                    if ink_button(ui, word(ja, "共有", "Share")).clicked() {
                        self.more = !self.more;
                    }
                    if ink_button(ui, if ja { "EN" } else { "JA" }).clicked() {
                        self.ja = !self.ja;
                    }
                }
            });
        });
        if self.more {
            let second = Rect::from_min_size(
                Pos2::new(rect.min.x + 8.0, rect.min.y + 64.0),
                EVec2::new((rect.width() - 16.0).max(1.0), 60.0),
            );
            ui.scope_builder(egui::UiBuilder::new().max_rect(second), |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if compact && ink_button(ui, word(ja, "見本", "Sample")).clicked() {
                        self.load_document(Document::demo(2, 4));
                    }
                    if compact && ink_button(ui, if ja { "EN" } else { "JA" }).clicked() {
                        self.ja = !self.ja;
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut self.host)
                            .desired_width(150.0)
                            .hint_text("127.0.0.1:8787"),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.room)
                            .desired_width(120.0)
                            .hint_text(word(ja, "部屋", "room")),
                    );
                    if fill_button(ui, word(ja, "入室", "Join"), BLUE).clicked() {
                        self.join_room();
                    }
                });
            });
            if !self.status.is_empty() {
                ui.painter().text(
                    Pos2::new(rect.min.x + 12.0, rect.bottom() - 14.0),
                    Align2::LEFT_CENTER,
                    &self.status,
                    FontId::proportional(12.0),
                    INK,
                );
            }
        }
    }

    fn field_body(&mut self, ui: &mut egui::Ui, rect: Rect, compact: bool) {
        let response = ui.interact(rect, ui.id().with("phone-field"), Sense::click_and_drag());
        let positions = self.positions_now();
        let mut map = field_map(&self.document, rect, self.view);
        if let Some(touch) = ui.input(|input| input.multi_touch()) {
            self.view = apply_two_finger(
                self.view,
                touch.zoom_delta,
                touch.translation_delta.x,
                touch.translation_delta.y,
                map.scale,
            );
            self.suppress_drag = true;
            map = field_map(&self.document, rect, self.view);
        }
        if response.drag_started() && ui.input(|input| input.multi_touch()).is_none() {
            self.suppress_drag = false;
            let local = response
                .interact_pointer_pos()
                .map(|pos| local_vec(rect, pos));
            self.selected = local
                .and_then(|point| pick_performer(&map, &positions, point, hit_radius(compact)));
            self.press_origin = response.interact_pointer_pos();
        }
        if response.dragged()
            && !self.suppress_drag
            && !self.playing
            && let (Some(index), Some(origin), Some(pos)) = (
                self.selected,
                self.press_origin,
                response.interact_pointer_pos(),
            )
            && origin.distance(pos) > 6.0
        {
            let point = map.unmap(local_vec(rect, pos));
            self.place_selected(index, point);
        }
        if response.drag_stopped() {
            self.suppress_drag = false;
        }
        let positions = self.positions_now();
        paint_field(
            ui,
            rect,
            &self.document,
            &positions,
            self.view,
            self.selected,
            BLUE,
            None,
        );
    }

    fn split_body(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let Some(split) = &self.split else {
            return;
        };
        let header = Rect::from_min_size(rect.min, EVec2::new(rect.width(), 36.0));
        ui.painter()
            .rect_filled(header, 0.0, Color32::from_rgb(12, 28, 48));
        ui.painter().text(
            header.center(),
            Align2::CENTER_CENTER,
            format!("Alice   {}   Bob", split.room),
            FontId::proportional(16.0),
            Color32::WHITE,
        );
        let body = Rect::from_min_max(Pos2::new(rect.min.x, header.max.y), rect.max);
        let left = Rect::from_min_max(body.min, Pos2::new(body.center().x - 3.0, body.max.y));
        let right = Rect::from_min_max(Pos2::new(body.center().x + 3.0, body.min.y), body.max);
        let left_positions = split.left.sets.first().map(|set| set.positions.clone());
        let right_positions = split.right.sets.first().map(|set| set.positions.clone());
        let left_doc = split.left.clone();
        let right_doc = split.right.clone();
        if let Some(positions) = left_positions {
            paint_field(
                ui,
                left,
                &left_doc,
                &positions,
                self.view,
                Some(0),
                Color32::from_rgb(242, 116, 116),
                Some("A"),
            );
        }
        if let Some(positions) = right_positions {
            paint_field(
                ui,
                right,
                &right_doc,
                &positions,
                self.view,
                Some(1),
                Color32::from_rgb(104, 197, 232),
                Some("B"),
            );
        }
    }
}

fn field_map(document: &Document, rect: Rect, view: FieldView) -> FieldMap {
    FieldMap::with_view(
        document.grid.width,
        document.grid.height,
        Vec2 {
            x: rect.width(),
            y: rect.height(),
        },
        12.0,
        Some(view.center),
        view.zoom,
    )
}

fn local_vec(rect: Rect, pos: Pos2) -> Vec2 {
    Vec2 {
        x: pos.x - rect.min.x,
        y: pos.y - rect.min.y,
    }
}

fn screen_pos(rect: Rect, map: &FieldMap, point: Point) -> Pos2 {
    let mapped = map.map(point);
    Pos2::new(rect.min.x + mapped.x, rect.min.y + mapped.y)
}

#[allow(clippy::too_many_arguments)]
fn paint_field(
    ui: &mut egui::Ui,
    rect: Rect,
    document: &Document,
    positions: &[Point],
    view: FieldView,
    selected: Option<usize>,
    ring: Color32,
    cursor: Option<&str>,
) {
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, GRASS);
    let map = field_map(document, rect, view);
    let grid = &document.grid;
    let line = Stroke::new(1.0, Color32::from_white_alpha(70));
    let mut x = 0.0;
    while x <= grid.width + 0.01 {
        painter.line_segment(
            [
                screen_pos(rect, &map, Point { x, y: 0.0 }),
                screen_pos(rect, &map, Point { x, y: grid.height }),
            ],
            line,
        );
        x += grid.major_line_interval.max(5.0);
    }
    let mut y = 0.0;
    while y <= grid.height + 0.01 {
        painter.line_segment(
            [
                screen_pos(rect, &map, Point { x: 0.0, y }),
                screen_pos(rect, &map, Point { x: grid.width, y }),
            ],
            line,
        );
        y += grid.major_line_interval.max(5.0);
    }
    let radius = dot_radius(rect.width() < 520.0);
    for (index, performer) in document.performers.iter().enumerate() {
        let Some(point) = positions.get(index).copied() else {
            continue;
        };
        let pos = screen_pos(rect, &map, point);
        let color = performer
            .color
            .0
            .map(|rgb| Color32::from_rgb(rgb[0], rgb[1], rgb[2]))
            .unwrap_or(GOLD);
        if selected == Some(index) {
            painter.circle_stroke(pos, radius + 6.0, Stroke::new(3.0, ring));
        }
        painter.circle_filled(pos, radius, color);
        painter.text(
            pos,
            Align2::CENTER_CENTER,
            &performer.label,
            FontId::proportional(10.0),
            Color32::from_rgb(20, 24, 28),
        );
    }
    if let (Some(label), Some(index)) = (cursor, selected)
        && let Some(point) = positions.get(index).copied()
    {
        let pos = screen_pos(rect, &map, point) + EVec2::new(22.0, -18.0);
        painter.circle_filled(pos, 12.0, ring);
        painter.text(
            pos,
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(14.0),
            Color32::WHITE,
        );
    }
}

fn ink_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(label).size(16.0).color(INK))
            .min_size(EVec2::new(44.0, 44.0))
            .fill(PAPER)
            .stroke(Stroke::new(1.0, Color32::from_rgb(186, 192, 200))),
    )
}

fn fill_button(ui: &mut egui::Ui, label: &str, fill: Color32) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(label).size(16.0).color(Color32::WHITE))
            .min_size(EVec2::new(44.0, 44.0))
            .fill(fill),
    )
}

fn word(ja: bool, ja_text: &'static str, en_text: &'static str) -> &'static str {
    if ja { ja_text } else { en_text }
}

fn viewer_message(ja: bool, limit: u16) -> String {
    if ja {
        format!("この部屋の編集は{limit}人までです。見学で参加しています。")
    } else {
        format!("This room already has {limit} editors. You are watching.")
    }
}

fn page_is_https() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|window| window.location().protocol().ok())
            .is_some_and(|protocol| protocol == "https:")
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        false
    }
}

fn replica_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos() as u64)
        .unwrap_or(1)
        | 1
}

/// Two replicas edit different dots, exchange operations, and both documents
/// show both moves. This is the side-by-side screenshot.
pub fn synced_pair() -> (Document, Document) {
    let document = Document::demo(2, 4);
    let mut alice = ShowState::from_document(&document, ReplicaId::from_raw(1));
    let mut bob = ShowState::from_document(&document, ReplicaId::from_raw(2));
    let alice_op = alice
        .move_dot(
            DotId {
                set_id: 1,
                performer_id: 1,
            },
            Point { x: 8.0, y: 12.0 },
        )
        .expect("finite point");
    let bob_op = bob
        .move_dot(
            DotId {
                set_id: 1,
                performer_id: 2,
            },
            Point { x: 22.0, y: 40.0 },
        )
        .expect("finite point");
    assert!(alice.apply(&bob_op));
    assert!(bob.apply(&alice_op));
    assert!(alice.agrees_with(&bob));
    let mut left = document.clone();
    let mut right = document.clone();
    alice.write_document(&mut left);
    bob.write_document(&mut right);
    (left, right)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phone_surface_has_no_depth_view() {
        let three = ['3', 'D'].into_iter().collect::<String>();
        let tool = ['b', 'l', 'e', 'n', 'd', 'e', 'r']
            .into_iter()
            .collect::<String>();
        let sources = [
            include_str!("app.rs"),
            include_str!("lib.rs"),
            include_str!("touch.rs"),
            include_str!("main.rs"),
            include_str!("socket.rs"),
            include_str!("../web/index.html"),
        ];
        for source in sources {
            let lower = source.to_ascii_lowercase();
            assert!(
                !lower.contains(&three.to_ascii_lowercase()),
                "depth view leaked into a phone source"
            );
            assert!(
                !lower.contains(&tool),
                "native render tool leaked into a phone source"
            );
        }
    }

    #[test]
    fn exchanged_moves_land_on_both_documents() {
        let (left, right) = synced_pair();
        assert_eq!(left.sets[0].positions[0], Point { x: 8.0, y: 12.0 });
        assert_eq!(left.sets[0].positions[1], Point { x: 22.0, y: 40.0 });
        assert_eq!(left.sets[0].positions, right.sets[0].positions);
        assert_ne!(
            left.sets[0].positions[0],
            Document::demo(2, 4).sets[0].positions[0]
        );
    }
}
