//! Touch field: open a show, play it, tap a performer, drag them.
//!
//! This surface is the flat field only: a plan view, playback, and a share link.

use crate::touch::{FieldView, apply_two_finger, chrome_is_compact, hit_radius, pick_performer};
use drill_core::{
    Document, Edit, Locale, PerformerId, Point, RouteTable, Set, SetAnnotation, Symbol,
};
#[cfg(target_arch = "wasm32")]
use drill_presence::PresenceMessage;
use drill_presence::{Presence, UserId};
use drill_render::{
    BuildScratch, DisplayList, DrawCmd, FieldMap, RenderOptions, Rgba, Scene, Theme, Vec2, Viewport,
};
use drill_sync::{DotId, ReplicaId, RoomEvent, SetFact, ShowState};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2 as EVec2};
use web_time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const INK: Color32 = Color32::from_rgb(32, 36, 44);
const BLUE: Color32 = Color32::from_rgb(20, 96, 200);
const PAPER: Color32 = Color32::WHITE;
const HAIRLINE: Color32 = Color32::from_rgb(186, 192, 200);
/// Finger target. The platform minimum is 44px; the bar uses 48.
const HIT: f32 = 48.0;
const _: () = assert!(HIT >= 44.0);
const TOOL_H: f32 = 56.0;
const CHIP_H: f32 = 40.0;
const STRIP_H: f32 = 56.0;
const FIELD_MARGIN: f32 = 26.0;
const MORE_H: f32 = 156.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum PhoneTool {
    Select,
    Move,
}

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
    peer_name: String,
    peer_color: [u8; 3],
    history: drill_core::History,
    tool: PhoneTool,
    drag_from: Option<Point>,
    fitted: Option<(u32, u32)>,
    #[cfg(target_arch = "wasm32")]
    peers: Vec<Presence>,
    #[cfg(target_arch = "wasm32")]
    last_presence: Option<Instant>,
    #[cfg(target_arch = "wasm32")]
    last_sent: Option<Presence>,
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
        #[cfg(target_arch = "wasm32")]
        apply_page_query(&mut app);
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
        let view = FieldView::fit(document.grid.width, document.grid.height);
        let show = ShowState::from_document(&document, ReplicaId::from_raw(replica_seed()));
        let peer_color = drill_presence::palette_color_for(UserId::from_raw(show.replica().get()));
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
            peer_name: "スマホ".into(),
            peer_color,
            history: drill_core::History::with_limit(100),
            tool: PhoneTool::Move,
            drag_from: None,
            fitted: None,
            #[cfg(target_arch = "wasm32")]
            peers: Vec::new(),
            #[cfg(target_arch = "wasm32")]
            last_presence: None,
            #[cfg(target_arch = "wasm32")]
            last_sent: None,
            relay: None,
        }
    }

    fn install_split(&mut self) {
        let (left, right) = synced_pair();
        self.view = FieldView::fit(left.grid.width, left.grid.height);
        self.fitted = None;
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
        let view = FieldView::fit(document.grid.width, document.grid.height);
        self.show = ShowState::from_document(&document, self.show.replica());
        self.document = document;
        self.view = view;
        self.fitted = None;
        self.history = drill_core::History::with_limit(100);
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
        self.poll_presence();
        self.publish_presence();
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
        self.clamp_set();
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
            .is_none_or(|last| last.elapsed() >= Duration::from_secs(2));
        if !due {
            return;
        }
        let hello = self.show.hello(&self.peer_name, self.peer_color);
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
        self.peer_color = color_for_name(self.show.replica().get(), &self.peer_name);
        let hello = self.show.hello(&self.peer_name, self.peer_color);
        self.status = word(self.ja, "入室しました", "Joined").to_owned();
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
        #[cfg(target_arch = "wasm32")]
        if let Some(client) = &self.relay {
            client.bind_repaint(ui.ctx());
        }
        self.poll_room();
        self.advance();
        // Playback still animates. A joined room does not: a 50ms redraw
        // stalled touch input in the browser. Remote edits wake the frame
        // from the socket, and a slow tick covers the late-joiner hello.
        if self.playing {
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        } else if self.relay.is_some() {
            ui.ctx().request_repaint_after(Duration::from_secs(2));
        }

        let full = ui.max_rect();
        let compact = chrome_is_compact(full.width());
        let (bar, chips, field, strip) = phone_layout(full, self.more);
        self.toolbar(ui, bar, compact);
        self.presence_row(ui, chips);
        if self.split.is_some() {
            self.split_body(ui, field);
        } else if field.height() > 8.0 {
            self.field_body(ui, field, compact);
        }
        self.set_strip(ui, strip);
    }
}

impl PhoneApp {
    fn toolbar(&mut self, ui: &mut egui::Ui, rect: Rect, _compact: bool) {
        ui.painter().rect_filled(rect, 0.0, PAPER);
        ui.painter()
            .hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, HAIRLINE));
        let top = Rect::from_min_size(
            rect.min + EVec2::new(0.0, 4.0),
            EVec2::new(rect.width(), HIT),
        );
        let can_edit = !self.view_only;
        let playing = self.playing;
        let tool = self.tool;
        let can_undo = self.history.can_undo();
        let can_redo = self.history.can_redo();
        let mut action = None;
        ui.scope_builder(egui::UiBuilder::new().max_rect(top), |ui| {
            egui::ScrollArea::horizontal()
                .id_salt("phone-tools")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.add_space(4.0);
                        if icon_button(ui, Icon::Prev, false, true).clicked() {
                            action = Some(BarAction::Prev);
                        }
                        if icon_button(ui, Icon::Next, false, true).clicked() {
                            action = Some(BarAction::Next);
                        }
                        if icon_button(ui, Icon::Add, false, can_edit).clicked() {
                            action = Some(BarAction::Add);
                        }
                        let play_icon = if playing { Icon::Stop } else { Icon::Play };
                        if icon_button(ui, play_icon, playing, true).clicked() {
                            action = Some(BarAction::Play);
                        }
                        if icon_button(ui, Icon::Undo, false, can_edit && can_undo).clicked() {
                            action = Some(BarAction::Undo);
                        }
                        if icon_button(ui, Icon::Redo, false, can_edit && can_redo).clicked() {
                            action = Some(BarAction::Redo);
                        }
                        if icon_button(ui, Icon::Select, tool == PhoneTool::Select, true).clicked()
                        {
                            action = Some(BarAction::Select);
                        }
                        if icon_button(ui, Icon::Move, tool == PhoneTool::Move, true).clicked() {
                            action = Some(BarAction::Move);
                        }
                        if icon_button(ui, Icon::Share, false, true).clicked() {
                            action = Some(BarAction::Share);
                        }
                        if icon_button(ui, Icon::Menu, self.more, true).clicked() {
                            action = Some(BarAction::Menu);
                        }
                    });
                });
        });
        let ctx = ui.ctx().clone();
        match action {
            Some(BarAction::Prev) => self.step_set(-1),
            Some(BarAction::Next) => self.step_set(1),
            Some(BarAction::Add) => self.add_set(),
            Some(BarAction::Play) => {
                self.playing = !self.playing;
                self.last_tick = Instant::now();
            }
            Some(BarAction::Undo) => self.undo_edit(),
            Some(BarAction::Redo) => self.redo_edit(),
            Some(BarAction::Select) => self.tool = PhoneTool::Select,
            Some(BarAction::Move) => self.tool = PhoneTool::Move,
            Some(BarAction::Share) => self.copy_link(&ctx),
            Some(BarAction::Menu) => self.more = !self.more,
            None => {}
        }
        if !self.more {
            return;
        }
        let lower = Rect::from_min_max(
            Pos2::new(rect.min.x + 8.0, rect.min.y + TOOL_H),
            Pos2::new(rect.max.x - 8.0, rect.max.y - 4.0),
        );
        let ja = self.ja;
        let mut sample = false;
        let mut toggle_lang = false;
        let mut join = false;
        ui.scope_builder(egui::UiBuilder::new().max_rect(lower), |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if text_button(ui, word(ja, "見本", "Sample")).clicked() {
                    sample = true;
                }
                if text_button(ui, if ja { "EN" } else { "JA" }).clicked() {
                    toggle_lang = true;
                }
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.add_sized(
                    EVec2::new(150.0, HIT),
                    egui::TextEdit::singleline(&mut self.host).hint_text("127.0.0.1:8787"),
                );
                ui.add_sized(
                    EVec2::new(120.0, HIT),
                    egui::TextEdit::singleline(&mut self.room).hint_text(word(ja, "部屋", "room")),
                );
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.add_sized(
                    EVec2::new(160.0, HIT),
                    egui::TextEdit::singleline(&mut self.peer_name)
                        .hint_text(word(ja, "名前", "name")),
                );
                if text_button(ui, word(ja, "入室", "Join")).clicked() {
                    join = true;
                }
            });
        });
        if sample {
            self.load_document(Document::demo(2, 4));
        }
        if toggle_lang {
            self.ja = !self.ja;
        }
        if join {
            self.join_room();
        }
    }

    fn field_body(&mut self, ui: &mut egui::Ui, rect: Rect, compact: bool) {
        self.ensure_fit(rect);
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
        let tapping = response.clicked() || response.drag_started();
        if tapping && ui.input(|input| input.multi_touch()).is_none() {
            self.suppress_drag = false;
            let local = response
                .interact_pointer_pos()
                .map(|pos| local_vec(rect, pos));
            self.selected = local
                .and_then(|point| pick_performer(&map, &positions, point, hit_radius(compact)));
            self.press_origin = response.interact_pointer_pos();
            if response.drag_started() && self.tool == PhoneTool::Move {
                self.drag_from = self
                    .selected
                    .and_then(|index| positions.get(index).copied());
            }
        }
        if self.tool == PhoneTool::Move
            && response.dragged()
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
            let start = self.drag_from.take();
            if !self.suppress_drag
                && self.tool == PhoneTool::Move
                && let (Some(index), Some(start)) = (self.selected, start)
            {
                self.note_move(index, start);
            }
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
            self.peer_marks(),
            self.ja,
        );
        if self.view_only {
            let banner = Rect::from_min_size(rect.min, EVec2::new(rect.width(), 48.0));
            ui.painter()
                .rect_filled(banner, 0.0, Color32::from_rgb(255, 244, 214));
            ui.scope_builder(egui::UiBuilder::new().max_rect(banner.shrink(8.0)), |ui| {
                ui.label(
                    egui::RichText::new(viewer_message(self.ja, self.editor_limit))
                        .size(14.0)
                        .color(INK),
                );
            });
        }
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
                &[],
                self.ja,
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
                &[],
                self.ja,
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
        FIELD_MARGIN,
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
    peers: &[Presence],
    ja: bool,
) {
    let painter = ui.painter().with_clip_rect(rect);
    let map = field_map(document, rect, view);
    let radius = (map.scale * 1.1).clamp(5.0, 11.0);
    let options = RenderOptions {
        margin: FIELD_MARGIN,
        show_step_grid: document.grid.show_step_grid,
        show_hashes: true,
        show_labels: true,
        dot_radius: radius,
        label_size: 12.0,
        field_center: Some(view.center),
        field_zoom: view.zoom,
        extend_grid: false,
        symbol_override: Some(Symbol::Circle),
        label_inside: true,
        ..RenderOptions::default()
    };
    let scene = Scene {
        document,
        positions,
        viewport: Viewport {
            size: Vec2 {
                x: rect.width(),
                y: rect.height(),
            },
            ui_scale: 1.0,
        },
        options: &options,
        theme: &Theme::PRINT_LIGHT,
    };
    let mut list = DisplayList::new();
    drill_render::build_field_2d(&scene, &mut BuildScratch, &mut list);
    for command in list.paint_order() {
        paint_command(&painter, rect.min, &list, command);
    }
    paint_yard_numbers(&painter, rect, document, &map, ja);
    for (index, performer) in document.performers.iter().enumerate() {
        let Some(point) = positions.get(index).copied() else {
            continue;
        };
        let pos = screen_pos(rect, &map, point);
        if selected == Some(index) {
            painter.circle_stroke(pos, radius + 4.0, Stroke::new(3.0, ring));
        }
        paint_peer_marks(&painter, pos, performer.id, peers, radius);
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

fn paint_yard_numbers(
    painter: &egui::Painter,
    rect: Rect,
    document: &Document,
    map: &FieldMap,
    ja: bool,
) {
    let locale = if ja { Locale::Ja } else { Locale::En };
    let font = FontId::proportional(11.0);
    let color = Color32::from_rgb(36, 84, 158);
    for mark in drill_core::coordinates::field_marks(&document.grid, locale) {
        let mapped = map.map(mark.at);
        let at = Pos2::new(rect.min.x + mapped.x, rect.min.y + mapped.y);
        let (pos, align) = match mark.kind {
            drill_core::coordinates::FieldMarkKind::Depth => {
                (at + EVec2::new(-6.0, 0.0), Align2::RIGHT_CENTER)
            }
            _ => (at + EVec2::new(0.0, 6.0), Align2::CENTER_TOP),
        };
        painter.text(pos, align, mark.text, font.clone(), color);
    }
}

fn paint_command(painter: &egui::Painter, origin: Pos2, list: &DisplayList, command: &DrawCmd) {
    match *command {
        DrawCmd::FieldFill { rect, fill } => {
            painter.rect_filled(offset_rect(origin, rect), 0.0, rgba(fill));
        }
        DrawCmd::Line { a, b, width, color } => {
            painter.line_segment(
                [offset_pos(origin, a), offset_pos(origin, b)],
                Stroke::new(width, rgba(color)),
            );
        }
        DrawCmd::Dot {
            center,
            radius,
            fill,
            stroke,
            symbol,
        } => {
            paint_symbol(
                painter,
                offset_pos(origin, center),
                radius,
                rgba(fill),
                rgba(stroke),
                symbol,
            );
        }
        DrawCmd::Text {
            at,
            text,
            size,
            color,
        } => {
            painter.text(
                offset_pos(origin, at),
                Align2::CENTER_BOTTOM,
                list.text(text),
                FontId::monospace(size),
                rgba(color),
            );
        }
    }
}

fn paint_symbol(
    painter: &egui::Painter,
    center: Pos2,
    radius: f32,
    fill: Color32,
    stroke: Color32,
    symbol: Symbol,
) {
    match symbol {
        Symbol::Circle => {
            painter.circle_filled(center, radius, fill);
            painter.circle_stroke(center, radius, Stroke::new(1.0, stroke));
        }
        Symbol::Square => {
            let half = radius * 0.8;
            let rect = Rect::from_center_size(center, EVec2::splat(half * 2.0));
            painter.rect_filled(rect, 0.0, fill);
            painter.rect_stroke(
                rect,
                0.0,
                Stroke::new(1.0, stroke),
                egui::StrokeKind::Inside,
            );
        }
        Symbol::Cross => {
            let width = (radius * 0.4).max(1.0);
            painter.line_segment(
                [
                    Pos2::new(center.x - radius, center.y - radius),
                    Pos2::new(center.x + radius, center.y + radius),
                ],
                Stroke::new(width, stroke),
            );
            painter.line_segment(
                [
                    Pos2::new(center.x - radius, center.y + radius),
                    Pos2::new(center.x + radius, center.y - radius),
                ],
                Stroke::new(width, stroke),
            );
        }
        Symbol::Triangle | Symbol::Diamond | Symbol::Star => {
            let points = drill_render::symbol_points(
                Vec2 {
                    x: center.x,
                    y: center.y,
                },
                radius,
                symbol,
            )
            .into_iter()
            .map(|point| Pos2::new(point.x, point.y))
            .collect();
            painter.add(egui::Shape::convex_polygon(
                points,
                fill,
                Stroke::new(1.0, stroke),
            ));
        }
    }
}

fn offset_pos(origin: Pos2, point: Vec2) -> Pos2 {
    Pos2::new(origin.x + point.x, origin.y + point.y)
}

fn offset_rect(origin: Pos2, rect: drill_render::Rect) -> Rect {
    Rect::from_min_max(offset_pos(origin, rect.min), offset_pos(origin, rect.max))
}

fn rgba(color: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(color.0, color.1, color.2, color.3)
}

fn text_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add_sized(
        EVec2::new(HIT, HIT),
        egui::Button::new(egui::RichText::new(label).size(16.0).color(INK))
            .fill(PAPER)
            .stroke(Stroke::new(1.0, HAIRLINE)),
    )
}

enum BarAction {
    Prev,
    Next,
    Add,
    Play,
    Undo,
    Redo,
    Select,
    Move,
    Share,
    Menu,
}

enum Icon {
    Prev,
    Next,
    Add,
    Play,
    Stop,
    Undo,
    Redo,
    Select,
    Move,
    Share,
    Menu,
}

fn icon_button(ui: &mut egui::Ui, icon: Icon, selected: bool, enabled: bool) -> egui::Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(EVec2::splat(HIT), sense);
    let fill = if selected {
        if matches!(icon, Icon::Stop) {
            Color32::from_rgb(176, 48, 48)
        } else {
            BLUE
        }
    } else {
        PAPER
    };
    let ink = if !enabled {
        HAIRLINE
    } else if selected {
        Color32::WHITE
    } else {
        INK
    };
    ui.painter().rect_filled(rect, 10.0, fill);
    ui.painter().rect_stroke(
        rect,
        10.0,
        Stroke::new(1.0, HAIRLINE),
        egui::StrokeKind::Inside,
    );
    paint_icon(ui.painter(), rect.center(), icon, ink);
    response
}

fn paint_icon(painter: &egui::Painter, center: Pos2, icon: Icon, ink: Color32) {
    let stroke = Stroke::new(2.0, ink);
    let at = |x: f32, y: f32| center + EVec2::new(x, y);
    match icon {
        Icon::Prev => {
            painter.line_segment([at(-4.0, 0.0), at(7.0, -8.0)], stroke);
            painter.line_segment([at(-4.0, 0.0), at(7.0, 8.0)], stroke);
        }
        Icon::Next => {
            painter.line_segment([at(4.0, 0.0), at(-7.0, -8.0)], stroke);
            painter.line_segment([at(4.0, 0.0), at(-7.0, 8.0)], stroke);
        }
        Icon::Add => {
            painter.line_segment([at(-8.0, 0.0), at(8.0, 0.0)], stroke);
            painter.line_segment([at(0.0, -8.0), at(0.0, 8.0)], stroke);
        }
        Icon::Play => {
            painter.add(egui::Shape::convex_polygon(
                vec![at(-6.0, -8.0), at(-6.0, 8.0), at(8.0, 0.0)],
                ink,
                Stroke::NONE,
            ));
        }
        Icon::Stop => {
            painter.rect_filled(Rect::from_center_size(center, EVec2::splat(14.0)), 2.0, ink);
        }
        Icon::Undo => {
            painter.line_segment([at(8.0, -3.0), at(-5.0, -3.0)], stroke);
            painter.line_segment([at(-5.0, -3.0), at(-5.0, 7.0)], stroke);
            painter.line_segment([at(-5.0, -3.0), at(-1.0, -8.0)], stroke);
            painter.line_segment([at(-5.0, -3.0), at(-10.0, 1.0)], stroke);
        }
        Icon::Redo => {
            painter.line_segment([at(-8.0, -3.0), at(5.0, -3.0)], stroke);
            painter.line_segment([at(5.0, -3.0), at(5.0, 7.0)], stroke);
            painter.line_segment([at(5.0, -3.0), at(1.0, -8.0)], stroke);
            painter.line_segment([at(5.0, -3.0), at(10.0, 1.0)], stroke);
        }
        Icon::Select => {
            let box_rect = Rect::from_center_size(center, EVec2::splat(16.0));
            painter.rect_stroke(box_rect, 2.0, stroke, egui::StrokeKind::Inside);
        }
        Icon::Move => {
            painter.line_segment([at(0.0, -9.0), at(0.0, 9.0)], stroke);
            painter.line_segment([at(-9.0, 0.0), at(9.0, 0.0)], stroke);
            painter.line_segment([at(0.0, -9.0), at(-4.0, -4.0)], stroke);
            painter.line_segment([at(0.0, -9.0), at(4.0, -4.0)], stroke);
            painter.line_segment([at(0.0, 9.0), at(-4.0, 4.0)], stroke);
            painter.line_segment([at(0.0, 9.0), at(4.0, 4.0)], stroke);
            painter.line_segment([at(-9.0, 0.0), at(-4.0, -4.0)], stroke);
            painter.line_segment([at(-9.0, 0.0), at(-4.0, 4.0)], stroke);
            painter.line_segment([at(9.0, 0.0), at(4.0, -4.0)], stroke);
            painter.line_segment([at(9.0, 0.0), at(4.0, 4.0)], stroke);
        }
        Icon::Share => {
            painter.line_segment([at(-8.0, 2.0), at(-8.0, 8.0)], stroke);
            painter.line_segment([at(-8.0, 8.0), at(4.0, 8.0)], stroke);
            painter.line_segment([at(4.0, 8.0), at(4.0, 2.0)], stroke);
            painter.line_segment([at(0.0, 2.0), at(8.0, -8.0)], stroke);
            painter.line_segment([at(8.0, -8.0), at(2.0, -8.0)], stroke);
            painter.line_segment([at(8.0, -8.0), at(8.0, -2.0)], stroke);
        }
        Icon::Menu => {
            for y in [-6.0, 0.0, 6.0] {
                painter.line_segment([at(-8.0, y), at(8.0, y)], stroke);
            }
        }
    }
}

fn chip(ui: &mut egui::Ui, color: [u8; 3], name: &str) {
    let width = (name.chars().count() as f32 * 8.0 + 36.0).clamp(72.0, 160.0);
    let (rect, _) = ui.allocate_exact_size(EVec2::new(width, 32.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, 16.0, Color32::from_rgb(236, 239, 244));
    let dot = Pos2::new(rect.min.x + 16.0, rect.center().y);
    ui.painter()
        .circle_filled(dot, 8.0, Color32::from_rgb(color[0], color[1], color[2]));
    ui.painter().text(
        Pos2::new(rect.min.x + 30.0, rect.center().y),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(13.0),
        INK,
    );
}

fn set_card(ui: &mut egui::Ui, name: &str, selected: bool) -> egui::Response {
    let width = (name.chars().count() as f32 * 9.0 + 28.0).max(72.0);
    let (rect, response) = ui.allocate_exact_size(EVec2::new(width, HIT), Sense::click());
    let fill = if selected {
        BLUE
    } else {
        Color32::from_rgb(236, 239, 244)
    };
    let ink = if selected { Color32::WHITE } else { INK };
    ui.painter().rect_filled(rect, 10.0, fill);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        name,
        FontId::proportional(14.0),
        ink,
    );
    response
}

fn points_differ(left: Point, right: Point) -> bool {
    (left.x - right.x).abs() > 0.001 || (left.y - right.y).abs() > 0.001
}

fn color_for_name(replica: u64, name: &str) -> [u8; 3] {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in name.trim().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let mixed = replica.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ hash.rotate_left(17);
    drill_presence::palette_color_for(UserId::from_raw(mixed))
}

fn percent_encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(*byte));
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn page_base() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|window| {
                let location = window.location();
                Some(format!(
                    "{}{}",
                    location.origin().ok()?,
                    location.pathname().ok()?
                ))
            })
            .unwrap_or_else(|| "https://drillforge.local/".to_owned())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        "https://drillforge.local/".to_owned()
    }
}

fn write_clipboard(text: &str) {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(window) = web_sys::window() {
            let _ = window.navigator().clipboard().write_text(text);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = text;
    }
}

fn phone_layout(full: Rect, more: bool) -> (Rect, Rect, Rect, Rect) {
    let bar_h = if more { TOOL_H + MORE_H } else { TOOL_H };
    let bar = Rect::from_min_size(full.min, EVec2::new(full.width(), bar_h));
    let chips = Rect::from_min_size(
        Pos2::new(full.min.x, bar.max.y),
        EVec2::new(full.width(), CHIP_H),
    );
    let strip = Rect::from_min_max(
        Pos2::new(full.min.x, (full.max.y - STRIP_H).max(chips.max.y)),
        full.max,
    );
    let field = Rect::from_min_max(
        Pos2::new(full.min.x, chips.max.y),
        Pos2::new(full.max.x, strip.min.y),
    );
    (bar, chips, field, strip)
}

fn word(ja: bool, ja_text: &'static str, en_text: &'static str) -> &'static str {
    if ja { ja_text } else { en_text }
}

impl PhoneApp {
    fn ensure_fit(&mut self, rect: Rect) {
        let key = (rect.width().round() as u32, rect.height().round() as u32);
        if key.0 == 0 || key.1 == 0 || self.fitted == Some(key) {
            return;
        }
        self.fitted = Some(key);
        self.view = FieldView::fit(self.document.grid.width, self.document.grid.height);
    }

    fn presence_row(&mut self, ui: &mut egui::Ui, rect: Rect) {
        ui.painter().rect_filled(rect, 0.0, PAPER);
        let status = self.status.clone();
        let people = self.presence_people();
        let body = if status.is_empty() {
            rect
        } else {
            Rect::from_min_max(rect.min, Pos2::new(rect.max.x - 168.0, rect.max.y))
        };
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(body.shrink2(EVec2::new(4.0, 4.0))),
            |ui| {
                egui::ScrollArea::horizontal()
                    .id_salt("phone-people")
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            for (color, name) in people {
                                chip(ui, color, &name);
                            }
                        });
                    });
            },
        );
        if !status.is_empty() {
            ui.painter().text(
                Pos2::new(rect.max.x - 8.0, rect.center().y),
                Align2::RIGHT_CENTER,
                status,
                FontId::proportional(12.0),
                INK,
            );
        }
    }

    fn presence_people(&self) -> Vec<([u8; 3], String)> {
        let mut people = vec![(self.peer_color, self.peer_name.clone())];
        self.push_remote_people(&mut people);
        people
    }

    #[cfg(target_arch = "wasm32")]
    fn push_remote_people(&self, people: &mut Vec<([u8; 3], String)>) {
        let mine = self.show.replica().get();
        for peer in &self.peers {
            if peer.user_id.get() == mine {
                continue;
            }
            people.push((peer.color, peer.display_name.clone()));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn push_remote_people(&self, _people: &mut Vec<([u8; 3], String)>) {}

    fn set_strip(&mut self, ui: &mut egui::Ui, rect: Rect) {
        ui.painter().rect_filled(rect, 0.0, PAPER);
        ui.painter()
            .hline(rect.x_range(), rect.top(), Stroke::new(1.0, HAIRLINE));
        let names: Vec<(usize, String, bool)> = self
            .document
            .sets
            .iter()
            .enumerate()
            .map(|(index, set)| (index, set.name.clone(), index == self.current_set))
            .collect();
        let mut pick = None;
        let row = Rect::from_min_size(
            rect.min + EVec2::new(0.0, 6.0),
            EVec2::new(rect.width(), HIT),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(row), |ui| {
            egui::ScrollArea::horizontal()
                .id_salt("phone-sets")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        ui.add_space(6.0);
                        for (index, name, selected) in names {
                            if set_card(ui, &name, selected).clicked() {
                                pick = Some(index);
                            }
                        }
                    });
                });
        });
        if let Some(index) = pick {
            self.current_set = index;
            self.local_count = 0.0;
            self.playing = false;
        }
    }

    fn add_set(&mut self) {
        if self.view_only {
            self.status = viewer_message(self.ja, self.editor_limit);
            return;
        }
        let mut next = self.document.clone();
        let Some(id) = next.next_set_id() else {
            return;
        };
        let (name_index, counts, positions) = {
            let current = next.sets.get(self.current_set);
            let counts = current.map(|set| set.counts).unwrap_or(16);
            let mut positions = current
                .map(|set| set.positions.clone())
                .unwrap_or_else(|| vec![Point { x: 0.0, y: 0.0 }; next.performers.len()]);
            positions.resize(next.performers.len(), Point { x: 0.0, y: 0.0 });
            (next.sets.len() + 1, counts, positions)
        };
        next.sets.push(Set {
            id,
            name: format!("セット {name_index}"),
            annotation: SetAnnotation::default(),
            counts,
            hold: 0,
            routes: RouteTable::default(),
            shape: None,
            generated_by: None,
            positions,
        });
        let index = next.sets.len() - 1;
        if self
            .history
            .execute(
                &mut self.document,
                Edit::ReplaceDocument {
                    document: Box::new(next),
                },
            )
            .is_err()
        {
            return;
        }
        self.current_set = index;
        self.local_count = 0.0;
        self.playing = false;
        self.align_show();
    }

    fn note_move(&mut self, index: usize, start: Point) {
        if self.view_only || self.applying {
            return;
        }
        let Some(set_id) = self.document.sets.get(self.current_set).map(|set| set.id) else {
            return;
        };
        let Some(performer_id) = self.document.performers.get(index).map(|person| person.id) else {
            return;
        };
        let Some(end) = self
            .document
            .sets
            .get(self.current_set)
            .and_then(|set| set.positions.get(index).copied())
        else {
            return;
        };
        if !points_differ(start, end) {
            return;
        }
        if let Some(slot) = self
            .document
            .sets
            .get_mut(self.current_set)
            .and_then(|set| set.positions.get_mut(index))
        {
            *slot = start;
        }
        if self
            .history
            .execute(
                &mut self.document,
                Edit::MovePerformers {
                    set_id,
                    performer_ids: vec![performer_id],
                    positions: vec![end],
                },
            )
            .is_err()
        {
            return;
        }
        self.align_show();
    }

    fn undo_edit(&mut self) {
        if self.view_only {
            self.status = viewer_message(self.ja, self.editor_limit);
            return;
        }
        self.playing = false;
        if self.history.undo(&mut self.document) {
            self.clamp_set();
            self.align_show();
        }
    }

    fn redo_edit(&mut self) {
        if self.view_only {
            self.status = viewer_message(self.ja, self.editor_limit);
            return;
        }
        self.playing = false;
        if self.history.redo(&mut self.document) {
            self.clamp_set();
            self.align_show();
        }
    }

    fn clamp_set(&mut self) {
        if self.document.sets.is_empty() {
            self.current_set = 0;
            return;
        }
        if self.current_set >= self.document.sets.len() {
            self.current_set = self.document.sets.len() - 1;
        }
    }

    fn align_show(&mut self) {
        let mut ops = Vec::new();
        let max_id = self
            .document
            .sets
            .iter()
            .map(|set| set.id.get())
            .max()
            .unwrap_or(0);
        let limit = max_id.saturating_add(4);
        for id in 1..=limit {
            if let Some(set) = self.document.sets.iter().find(|set| set.id.get() == id) {
                let name = set.name.clone();
                let counts = set.counts;
                let positions: Vec<(u32, Point)> = self
                    .document
                    .performers
                    .iter()
                    .enumerate()
                    .filter_map(|(index, performer)| {
                        set.positions
                            .get(index)
                            .copied()
                            .map(|point| (performer.id.get(), point))
                    })
                    .collect();
                let differs = self
                    .show
                    .set_fact(id)
                    .is_none_or(|fact| !fact.present || fact.counts != counts || fact.name != name);
                if differs {
                    ops.push(self.show.set_meta(
                        id,
                        SetFact {
                            name,
                            counts,
                            present: true,
                        },
                    ));
                }
                for (performer_id, point) in positions {
                    let dot = DotId {
                        set_id: id,
                        performer_id,
                    };
                    if self
                        .show
                        .dot(dot)
                        .is_none_or(|current| points_differ(current, point))
                        && let Some(op) = self.show.move_dot(dot, point)
                    {
                        ops.push(op);
                    }
                }
            } else if let Some(fact) =
                self.show
                    .set_fact(id)
                    .filter(|fact| fact.present)
                    .map(|fact| SetFact {
                        name: fact.name.clone(),
                        counts: fact.counts,
                        present: false,
                    })
            {
                ops.push(self.show.set_meta(id, fact));
            }
        }
        let from = self.show.replica();
        for op in ops {
            self.publish(RoomEvent::Op { from, op });
        }
    }

    fn copy_link(&mut self, ctx: &egui::Context) {
        if self.room.trim().is_empty() {
            self.more = true;
            self.status =
                word(self.ja, "部屋の名前を入れてください", "Enter a room code").to_owned();
            return;
        }
        let link = self.share_link();
        ctx.copy_text(link.clone());
        write_clipboard(&link);
        self.status = word(self.ja, "リンクをコピーしました", "Link copied").to_owned();
    }

    fn share_link(&self) -> String {
        format!(
            "{}?host={}&room={}&join=1&name={}",
            page_base(),
            percent_encode(self.host.trim()),
            percent_encode(self.room.trim()),
            percent_encode(self.peer_name.trim()),
        )
    }

    fn peer_marks(&self) -> &[Presence] {
        #[cfg(target_arch = "wasm32")]
        {
            &self.peers
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            &[]
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn poll_presence(&mut self) {
        let Some(client) = self.relay.as_mut() else {
            return;
        };
        let mut messages = Vec::new();
        while let Some(message) = client.poll_presence() {
            messages.push(message);
        }
        for message in messages {
            match message {
                PresenceMessage::Update(presence) => {
                    if let Some(slot) = self
                        .peers
                        .iter_mut()
                        .find(|peer| peer.user_id == presence.user_id)
                    {
                        *slot = presence;
                    } else {
                        self.peers.push(presence);
                    }
                }
                PresenceMessage::Leave(user) => {
                    self.peers.retain(|peer| peer.user_id != user);
                }
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn poll_presence(&mut self) {}

    #[cfg(target_arch = "wasm32")]
    fn publish_presence(&mut self) {
        if self.relay.is_none() {
            return;
        }
        let presence = self.local_presence();
        let changed = self.last_sent.as_ref() != Some(&presence);
        let due = self
            .last_presence
            .is_none_or(|last| last.elapsed() >= Duration::from_secs(5));
        if !changed && !due {
            return;
        }
        let Ok(json) = PresenceMessage::Update(presence.clone()).to_json() else {
            return;
        };
        let sent = self
            .relay
            .as_ref()
            .is_some_and(|client| client.send_text(&json));
        if !sent {
            return;
        }
        self.last_sent = Some(presence);
        self.last_presence = Some(Instant::now());
    }

    #[cfg(target_arch = "wasm32")]
    fn local_presence(&self) -> Presence {
        let selected = self.selected.and_then(|index| {
            self.document
                .performers
                .get(index)
                .map(|performer| performer.id)
        });
        let cursor = self
            .selected
            .and_then(|index| self.positions_now().get(index).copied());
        let mut presence = Presence::new(
            UserId::from_raw(self.show.replica().get()),
            self.peer_name.clone(),
            self.peer_color,
        );
        presence.current_set = self.current_set;
        presence.selected_performers = selected.into_iter().collect();
        presence.cursor = cursor;
        presence
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn publish_presence(&mut self) {}
}

fn paint_peer_marks(
    painter: &egui::Painter,
    pos: Pos2,
    performer: PerformerId,
    peers: &[Presence],
    radius: f32,
) {
    let mut ring = 0.0;
    for peer in peers {
        if !peer.selected_performers.contains(&performer) {
            continue;
        }
        let color = Color32::from_rgb(peer.color[0], peer.color[1], peer.color[2]);
        painter.circle_stroke(pos, radius + 4.0 + ring, Stroke::new(3.0, color));
        ring += 4.0;
        if peer.selected_performers.first() == Some(&performer) {
            let chip = pos + EVec2::new(-18.0, -18.0);
            painter.circle_filled(chip, 11.0, color);
            painter.text(
                chip,
                Align2::CENTER_CENTER,
                peer.initial().to_string(),
                FontId::proportional(12.0),
                Color32::from_rgb(20, 24, 28),
            );
        }
        if ring > 12.0 {
            break;
        }
    }
}

fn viewer_message(ja: bool, limit: u16) -> String {
    if ja {
        format!("この部屋の編集は{limit}人までです。見学で参加しています。")
    } else {
        format!("This room already has {limit} editors. You are watching.")
    }
}

#[cfg(target_arch = "wasm32")]
fn apply_page_query(app: &mut PhoneApp) {
    let params = page_query();
    let value = |key: &str| {
        params
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, item)| item.clone())
    };
    if let Some(host) = value("host") {
        app.host = host;
    }
    if let Some(room) = value("room") {
        app.room = room;
    }
    if let Some(name) = value("name")
        && !name.trim().is_empty()
    {
        app.peer_name = name;
    }
    if value("menu").as_deref() == Some("1") {
        app.more = true;
    }
    if value("join").as_deref() == Some("1") && !app.room.trim().is_empty() {
        app.join_room();
    }
}

#[cfg(target_arch = "wasm32")]
fn page_query() -> Vec<(String, String)> {
    let search = web_sys::window()
        .and_then(|window| window.location().search().ok())
        .unwrap_or_default();
    let search = search.trim_start_matches('?');
    if search.is_empty() {
        return Vec::new();
    }
    search
        .split('&')
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (percent_decode(key), percent_decode(value))
        })
        .collect()
}

#[cfg(target_arch = "wasm32")]
fn percent_decode(raw: &str) -> String {
    let mut bytes = Vec::new();
    let input = raw.as_bytes();
    let mut index = 0;
    while index < input.len() {
        match input[index] {
            b'+' => {
                bytes.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < input.len() => {
                let hex = &raw[index + 1..index + 3];
                if let Ok(value) = u8::from_str_radix(hex, 16) {
                    bytes.push(value);
                    index += 3;
                } else {
                    bytes.push(b'%');
                    index += 1;
                }
            }
            byte => {
                bytes.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(bytes)
        .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned())
}

fn replica_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
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

    #[test]
    fn the_field_keeps_the_screen_width() {
        let full = Rect::from_min_max(Pos2::ZERO, Pos2::new(390.0, 844.0));
        let (bar, chips, field, strip) = phone_layout(full, false);
        assert!((bar.width() - 390.0).abs() < 0.1);
        assert!((field.width() - 390.0).abs() < 0.1);
        assert!(field.height() > 400.0);
        assert!(chips.height() >= 32.0);
        assert!(strip.height() >= 44.0);
        assert!(strip.min.y + 0.1 >= field.max.y);
        let wide = Rect::from_min_max(Pos2::ZERO, Pos2::new(844.0, 390.0));
        let (_, _, field, _) = phone_layout(wide, false);
        assert!((field.width() - 844.0).abs() < 0.1);
        assert!(field.height() > 100.0);
    }

    #[test]
    fn a_new_phone_fits_the_floor() {
        let mut app = PhoneApp::from_document(Document::demo(2, 4));
        assert!((app.view.zoom - 1.0).abs() < f32::EPSILON);
        app.ensure_fit(Rect::from_min_size(Pos2::ZERO, EVec2::new(390.0, 640.0)));
        app.view.zoom = 3.0;
        app.ensure_fit(Rect::from_min_size(Pos2::ZERO, EVec2::new(390.0, 640.0)));
        assert!((app.view.zoom - 3.0).abs() < 0.01);
        app.ensure_fit(Rect::from_min_size(Pos2::ZERO, EVec2::new(800.0, 280.0)));
        assert!((app.view.zoom - 1.0).abs() < 0.01);
    }

    #[test]
    fn undo_restores_a_moved_performer() {
        let mut app = PhoneApp::from_document(Document::demo(2, 4));
        let start = app.document.sets[0].positions[0];
        app.place_selected(0, Point { x: 3.0, y: 4.0 });
        app.note_move(0, start);
        assert_eq!(app.document.sets[0].positions[0], Point { x: 3.0, y: 4.0 });
        app.undo_edit();
        assert_eq!(app.document.sets[0].positions[0], start);
        app.redo_edit();
        assert_eq!(app.document.sets[0].positions[0], Point { x: 3.0, y: 4.0 });
    }

    #[test]
    fn two_names_pick_different_colors() {
        let replica = 1_792_512_737_893_000_001;
        assert_ne!(
            color_for_name(replica, "Aya"),
            color_for_name(replica, "Ken")
        );
    }

    #[test]
    fn share_link_carries_the_room_and_name() {
        let mut app = PhoneApp::from_document(Document::demo(1, 1));
        app.host = "wss://relay.example".into();
        app.room = "brass night".into();
        app.peer_name = "Aya".into();
        let link = app.share_link();
        assert!(link.contains("room=brass%20night"));
        assert!(link.contains("name=Aya"));
        assert!(link.contains("join=1"));
        assert!(link.contains("host=wss%3A%2F%2Frelay.example"));
    }
}
