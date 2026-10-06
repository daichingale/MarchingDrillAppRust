//! "Simple Mode" (かんたんモード): a reduced, guided UI aimed at first-time
//! high-school students who just want to build a formation, move performers,
//! and play it back without learning the full desktop toolset.
//!
//! Invariant: every mutation in this module goes through `DrillApp::execute_edit`
//! (directly or via existing helpers like `commit_layout`/`commit_shape`), the
//! same as the full UI. This module never touches `self.document` fields
//! directly. See `ui_qa.rs` for the guard test that scans this file too.
use super::*;
use drill_jobs::{Job, JobErrorCode, JobFailure, JobKind, JobMsg};

type PreparedClicks = (
    drill_audio::ClickSettings,
    drill_audio::ClickSchedule,
    drill_audio::ClickVoices,
    f32,
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
enum MetronomeFailure {
    Output,
    Schedule,
}

/// Standalone metronome, independent of the open `Document`. Reuses the
/// existing `drill_audio` click-generation/output primitives (the same ones
/// `AudioState::configure_click` drives) rather than inventing a new audio
/// pipeline; this module only orchestrates them.
#[allow(dead_code)]
pub(crate) struct MetronomeState {
    pub bpm: f32,
    running: bool,
    output: Option<drill_audio::AudioOutput>,
    open_job: Option<Job<drill_audio::AudioOutput>>,
    schedule_job: Option<Job<PreparedClicks>>,
    start_when_ready: bool,
    error: Option<MetronomeFailure>,
}

impl Default for MetronomeState {
    fn default() -> Self {
        Self {
            bpm: 120.0,
            running: false,
            output: None,
            open_job: None,
            schedule_job: None,
            start_when_ready: false,
            error: None,
        }
    }
}

#[allow(dead_code)]
impl MetronomeState {
    fn begin_open(&mut self) {
        if self.output.is_some() || self.open_job.is_some() {
            return;
        }
        // A few seconds of silence: `render_block` fills silence past the end
        // of the asset too, so the click mixer keeps ticking indefinitely
        // while the device stream stays open. No music/document dependency.
        self.open_job = Some(Job::spawn_typed(JobKind::AudioDecode, |_| {
            let asset =
                drill_audio::AudioAsset::from_interleaved(vec![0_i16; 48_000 * 2], 48_000, 1, 1.0)
                    .map(std::sync::Arc::new)
                    .map_err(|_| JobFailure::new(JobErrorCode::Decode))?;
            drill_audio::AudioOutput::open_default(asset)
                .map_err(|_| JobFailure::new(JobErrorCode::External))
        }));
    }

    fn begin_schedule(&mut self) {
        let Some(output) = &self.output else { return };
        if self.schedule_job.is_some() {
            return;
        }
        let rate = output.output_sample_rate();
        let bpm = self.bpm;
        self.schedule_job = Some(Job::spawn_typed(JobKind::AudioDecode, move |_| {
            let tempo = drill_core::tempo::TempoMap::constant(bpm);
            let settings = drill_audio::ClickSettings {
                enabled: true,
                ..Default::default()
            };
            let schedule =
                drill_audio::ClickSchedule::build(&tempo, 0.0, 100_000.0, &settings, rate);
            let voices = drill_audio::ClickVoices::render(&settings, rate);
            Ok((settings, schedule, voices, bpm))
        }));
    }

    #[allow(dead_code)]
    pub fn poll(&mut self) {
        if let Some(message) = self.open_job.as_mut().and_then(Job::poll) {
            self.open_job = None;
            match message {
                JobMsg::Done(output) => {
                    self.output = Some(output);
                    self.error = None;
                    self.begin_schedule();
                }
                JobMsg::Failed(_) => self.error = Some(MetronomeFailure::Output),
                JobMsg::Cancelled => {}
            }
        }
        if let Some(message) = self.schedule_job.as_mut().and_then(Job::poll) {
            self.schedule_job = None;
            match message {
                JobMsg::Done((settings, schedule, voices, bpm)) if bpm == self.bpm => {
                    if let Some(output) = &self.output
                        && output.set_clicks(settings, schedule, voices).is_ok()
                        && self.start_when_ready
                    {
                        output.seek(0);
                        output.play();
                        self.running = true;
                        self.start_when_ready = false;
                    }
                }
                JobMsg::Done(_) => self.begin_schedule(),
                JobMsg::Failed(_) => self.error = Some(MetronomeFailure::Schedule),
                JobMsg::Cancelled => {}
            }
        }
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    #[allow(dead_code)]
    fn error(&self) -> Option<MetronomeFailure> {
        self.error
    }

    #[allow(dead_code)]
    pub fn toggle(&mut self) {
        if self.running {
            self.stop();
        } else {
            self.start();
        }
    }

    pub fn start(&mut self) {
        self.start_when_ready = true;
        if self.output.is_none() {
            self.begin_open();
        } else {
            self.begin_schedule();
        }
    }

    pub fn stop(&mut self) {
        if let Some(output) = &self.output {
            output.pause();
        }
        self.running = false;
        self.start_when_ready = false;
    }

    pub fn set_bpm(&mut self, bpm: f32) {
        let bpm = bpm.clamp(30.0, 300.0);
        if (bpm - self.bpm).abs() < f32::EPSILON {
            return;
        }
        self.bpm = bpm;
        if self.running && self.output.is_some() {
            self.start_when_ready = true;
            self.running = false;
            self.begin_schedule();
        }
    }

    /// 0..1 phase within the current beat, driven by the real audio clock
    /// (not a UI timer), for a drift-free visual pulse.
    #[allow(dead_code)]
    pub fn beat_phase(&self) -> f32 {
        let Some(output) = &self.output else {
            return 0.0;
        };
        let sample = output.clock().sample();
        if !sample.playing || sample.sample_rate == 0 {
            return 0.0;
        }
        let seconds = sample.position as f64 / f64::from(sample.sample_rate);
        let beats = seconds * (f64::from(self.bpm) / 60.0);
        beats.rem_euclid(1.0) as f32
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SimpleGuide {
    Place,
    Move,
    NextSet,
    Play,
}

#[derive(Default)]
pub(crate) struct SimpleModeState {
    pub enabled: bool,
    chrome_applied: bool,
    /// Last place landed on someone already standing there. Shown as a quiet
    /// line, never a dialog. The place itself is kept.
    pub(crate) overlap_note: bool,
    /// A draft JSON write succeeded for this session, so the corner can say so.
    draft_saved: bool,
    /// When a draft write fails, wait before hammering the disk again.
    draft_retry_after: Option<Instant>,
    /// Tests point this at a temp file. Production uses the app-data draft.
    draft_path_override: Option<std::path::PathBuf>,
    /// Last frame's roster, set, and selection, so chrome motion only starts
    /// when something the eye should notice actually changed.
    seen_people: usize,
    seen_set: usize,
    seen_selection: (usize, Option<usize>),
    empty_greeted: bool,
    motion: Option<SimpleChromeMotion>,
}

/// One short chrome animation. Cleared as soon as it settles so the window
/// can go idle again.
struct SimpleChromeMotion {
    kind: ChromeMotion,
    started: Instant,
    anchor: Option<Point>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChromeMotion {
    Empty,
    Place,
    Move,
    Set,
    Select,
}

/// Filled controls use a deeper blue than the light accent wash. White text
/// on the accent itself fails contrast; this stays in the same blue family.
const SIMPLE_BLUE: Color32 = Color32::from_rgb(20, 96, 200);
const SIMPLE_INK: Color32 = Color32::from_rgb(28, 32, 40);
const SIMPLE_CARD: Color32 = Color32::WHITE;
const SIMPLE_MOTION_SECS: f32 = 0.26;

#[derive(Clone, Copy, PartialEq, Eq)]
enum DockAction {
    Undo,
    Save,
    Open,
    More,
}

#[derive(Clone, Copy)]
enum PhoneIcon {
    Undo,
    Save,
    Open,
    More,
    Play,
    Pause,
    Next,
}

fn paint_phone_icon(painter: &egui::Painter, center: Pos2, icon: PhoneIcon, color: Color32) {
    let stroke = Stroke::new(2.0, color);
    let p = |x: f32, y: f32| center + egui::Vec2::new(x, y);
    match icon {
        PhoneIcon::Undo => {
            painter.add(egui::Shape::line(
                vec![
                    p(6.0, -5.0),
                    p(-1.0, -5.0),
                    p(-6.0, 0.0),
                    p(-1.0, 5.0),
                    p(6.0, 5.0),
                ],
                stroke,
            ));
            painter.line_segment([p(6.0, -5.0), p(1.0, -9.0)], stroke);
            painter.line_segment([p(6.0, -5.0), p(2.0, -1.0)], stroke);
        }
        PhoneIcon::Save => {
            painter.line_segment([p(0.0, -7.0), p(0.0, 2.0)], stroke);
            painter.line_segment([p(-4.0, -1.0), p(0.0, 3.0)], stroke);
            painter.line_segment([p(4.0, -1.0), p(0.0, 3.0)], stroke);
            painter.line_segment([p(-7.0, 4.0), p(-7.0, 7.0)], stroke);
            painter.line_segment([p(-7.0, 7.0), p(7.0, 7.0)], stroke);
            painter.line_segment([p(7.0, 7.0), p(7.0, 4.0)], stroke);
        }
        PhoneIcon::Open => {
            painter.add(egui::Shape::line(
                vec![p(-7.0, -1.0), p(-7.0, 6.0), p(7.0, 6.0), p(7.0, -1.0)],
                stroke,
            ));
            painter.add(egui::Shape::line(
                vec![p(-7.0, -1.0), p(-4.0, -5.0), p(1.0, -5.0), p(3.0, -1.0)],
                stroke,
            ));
        }
        PhoneIcon::More => {
            for (x, y) in [(-4.0, -4.0), (4.0, -4.0), (-4.0, 4.0), (4.0, 4.0)] {
                painter.circle_filled(p(x, y), 2.2, color);
            }
        }
        PhoneIcon::Play => {
            painter.add(egui::Shape::convex_polygon(
                vec![p(-5.0, -7.0), p(-5.0, 7.0), p(7.0, 0.0)],
                color,
                Stroke::NONE,
            ));
        }
        PhoneIcon::Pause => {
            painter.rect_filled(
                egui::Rect::from_center_size(p(-3.5, 0.0), egui::Vec2::new(3.0, 14.0)),
                1.0,
                color,
            );
            painter.rect_filled(
                egui::Rect::from_center_size(p(3.5, 0.0), egui::Vec2::new(3.0, 14.0)),
                1.0,
                color,
            );
        }
        PhoneIcon::Next => {
            painter.line_segment([p(-6.0, -6.0), p(1.0, 0.0)], stroke);
            painter.line_segment([p(-6.0, 6.0), p(1.0, 0.0)], stroke);
            painter.line_segment([p(2.0, -6.0), p(2.0, 6.0)], stroke);
        }
    }
}

impl DrillApp {
    pub(crate) fn ensure_simple_chrome(&mut self, ctx: &egui::Context) {
        if self.simple_mode.chrome_applied {
            return;
        }
        super::app_theme::AppTheme::Daylight.apply(ctx);
        self.simple_mode.chrome_applied = true;
    }

    pub(crate) fn restore_full_chrome(&mut self, ctx: &egui::Context) {
        if !self.simple_mode.chrome_applied {
            return;
        }
        self.app_theme.apply(ctx);
        self.simple_mode.chrome_applied = false;
    }

    fn show_has_motion(&self) -> bool {
        self.document
            .sets
            .windows(2)
            .any(|pair| pair[0].positions != pair[1].positions)
    }

    fn simple_guide(&self) -> SimpleGuide {
        if self.document.performers.is_empty() {
            SimpleGuide::Place
        } else if self.document.sets.len() < 2 {
            SimpleGuide::NextSet
        } else if !self.show_has_motion() {
            SimpleGuide::Move
        } else {
            SimpleGuide::Play
        }
    }

    pub(crate) fn set_simple_mode(&mut self, enabled: bool) {
        self.simple_mode.enabled = enabled;
        self.onboarding.prefer_simple = enabled;
        self.onboarding.welcome_seen = true;
        self.onboarding.show_welcome = false;
    }

    /// First launch lands on an empty field. Later launches stay on whichever
    /// editor the person last used, and a simple-mode draft reopens itself.
    pub(crate) fn open_into_preferred_editor(&mut self) {
        if !self.onboarding.welcome_seen {
            self.onboarding.prefer_simple = true;
            self.onboarding.welcome_seen = true;
            self.onboarding.show_welcome = false;
        }
        if !self.onboarding.prefer_simple {
            return;
        }
        self.simple_mode.enabled = true;
        if !self.restore_simple_draft() {
            self.begin_simple_show();
        }
    }

    fn simple_draft_path(&self) -> std::path::PathBuf {
        self.simple_mode
            .draft_path_override
            .clone()
            .unwrap_or_else(|| super::project_state::app_data_dir().join("simple-draft.drill.json"))
    }

    fn write_simple_draft(&mut self) -> bool {
        let Ok(json) = self.document.to_json() else {
            return false;
        };
        let path = self.simple_draft_path();
        if let Some(parent) = path.parent()
            && std::fs::create_dir_all(parent).is_err()
        {
            return false;
        }
        drill_project::atomic_write(&path, json.as_bytes(), None).is_ok()
    }

    fn restore_simple_draft(&mut self) -> bool {
        let Ok(json) = std::fs::read_to_string(self.simple_draft_path()) else {
            return false;
        };
        let Ok(document) = drill_core::Document::from_json(&json) else {
            return false;
        };
        if document.sets.is_empty() {
            return false;
        }
        self.install_document(document);
        self.field_tool = FieldTool::Move;
        self.simple_mode.draft_saved = true;
        true
    }

    pub(crate) fn flush_simple_draft(&mut self) {
        if self.simple_mode.enabled && self.dirty && self.write_simple_draft() {
            self.dirty = false;
            self.simple_mode.draft_saved = true;
            self.simple_mode.draft_retry_after = None;
        }
    }

    pub(crate) fn remember_simple_draft(&mut self) {
        if self.simple_mode.enabled && self.write_simple_draft() {
            self.simple_mode.draft_saved = true;
            self.simple_mode.draft_retry_after = None;
        }
    }

    fn autosave_simple_draft(&mut self, ctx: &egui::Context) {
        if !self.dirty {
            return;
        }
        if let Some(since) = self.simple_mode.draft_retry_after
            && since.elapsed() < Duration::from_millis(400)
        {
            ctx.request_repaint_after(Duration::from_millis(200));
            return;
        }
        if self.write_simple_draft() {
            self.dirty = false;
            self.simple_mode.draft_saved = true;
            self.simple_mode.draft_retry_after = None;
        } else {
            self.simple_mode.draft_retry_after = Some(Instant::now());
            ctx.request_repaint_after(Duration::from_millis(400));
        }
    }

    /// Screenshot harness only. Seeds a simple-mode frame when
    /// `DRILLFORGE_QA_SIMPLE` is `empty`, `placed`, or `play`.
    pub(crate) fn apply_qa_simple_fixture(&mut self) {
        let Ok(stage) = std::env::var("DRILLFORGE_QA_SIMPLE") else {
            return;
        };
        self.onboarding.show_welcome = false;
        self.simple_mode.enabled = true;
        self.begin_simple_show();
        if stage == "empty" {
            return;
        }
        self.place_performer_at(Point { x: 12.0, y: 16.0 }, true);
        self.place_performer_at(Point { x: 20.0, y: 16.0 }, true);
        if stage == "play" {
            self.duplicate_current_set();
            let set_id = self.document.sets[self.current_set].id;
            let performer_id = self.document.performers[0].id;
            let moved = self.document.sets[self.current_set].positions[0];
            // One id needs one position. Passing the whole set is an invalid edit.
            let _ = self.execute_edit(
                Edit::MovePerformers {
                    set_id,
                    performer_ids: vec![performer_id],
                    positions: vec![Point {
                        x: moved.x + 6.0,
                        y: moved.y,
                    }],
                },
                "qa",
            );
        }
        // The harness grabs pass 2, before a 260ms ease would finish.
        // Show the settled chrome instead of a half-played ring.
        self.simple_mode.seen_people = self.document.performers.len();
        self.simple_mode.seen_set = self.current_set;
        self.simple_mode.seen_selection =
            (self.selected.len(), self.selected.iter().copied().min());
        self.simple_mode.empty_greeted = true;
        self.simple_mode.motion = None;
    }

    /// Entry point called instead of the full desktop UI while Simple Mode is
    /// enabled. Completely separate code path from the rest of `app_ui.rs`;
    /// every state change still flows through `execute_edit`/`commit_layout`/
    /// `commit_shape`, so undo, autosave and validation all keep working.
    pub(crate) fn simple_ui(&mut self, ui: &mut egui::Ui) {
        // Keep edits on the set's arrival picture. A set-to-set glide is the
        // exception: snapping here would erase the motion the shared loop
        // just computed.
        if !self.playing && self.nav_glide.position().is_none() {
            self.count_position = 0.0;
            self.document
                .positions_at(self.current_set, 0.0, &mut self.frame_positions);
        }
        // One gesture language: tap empty ground to place, drag a person to
        // move. There is no tool to switch.
        self.field_tool = FieldTool::Move;
        self.autosave_simple_draft(ui.ctx());
        self.note_simple_chrome_motion(ui.ctx());
        ui.scope(|ui| {
            ui.style_mut().spacing.item_spacing = egui::Vec2::new(12.0, 10.0);
            ui.style_mut().spacing.button_padding = egui::Vec2::new(16.0, 10.0);
            ui.add_space(8.0);
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(20, 0))
                .show(ui, |ui| {
                    self.simple_header(ui);
                    ui.add_space(14.0);
                    self.simple_cue(ui);
                    if self.simple_mode.overlap_note {
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(i18n::registered(self.locale, "simple-mode.083"))
                                .size(14.0)
                                .color(super::app_theme::SECONDARY_TEXT),
                        );
                    }
                    ui.add_space(14.0);
                    let dock_h = self.simple_dock_height();
                    let field_h = (ui.available_height() - dock_h - 12.0).max(180.0);
                    self.simple_field_card(ui, field_h);
                    ui.add_space(12.0);
                    self.simple_dock(ui);
                });
        });
        self.show_update_notice(ui.ctx());
        self.onboarding.help_ui(ui.ctx(), self.locale);
        self.onboarding.persist_if_changed();
    }

    fn note_simple_chrome_motion(&mut self, ctx: &egui::Context) {
        let people = self.document.performers.len();
        let set = self.current_set;
        let selection = (self.selected.len(), self.selected.iter().copied().min());
        let kind = if people == 0 && !self.simple_mode.empty_greeted {
            self.simple_mode.empty_greeted = true;
            Some(ChromeMotion::Empty)
        } else if people > self.simple_mode.seen_people {
            Some(ChromeMotion::Place)
        } else if people == 0 && self.simple_mode.seen_people > 0 {
            Some(ChromeMotion::Empty)
        } else if set != self.simple_mode.seen_set {
            Some(ChromeMotion::Set)
        } else if selection != self.simple_mode.seen_selection && selection.0 > 0 {
            Some(ChromeMotion::Select)
        } else {
            None
        };
        if let Some(kind) = kind {
            self.simple_mode.motion = Some(SimpleChromeMotion {
                kind,
                started: Instant::now(),
                anchor: self.simple_motion_anchor(),
            });
        } else if self
            .simple_mode
            .motion
            .as_ref()
            .is_some_and(|motion| motion.started.elapsed().as_secs_f32() >= SIMPLE_MOTION_SECS)
        {
            self.simple_mode.motion = None;
        }
        self.simple_mode.seen_people = people;
        self.simple_mode.seen_set = set;
        self.simple_mode.seen_selection = selection;
        if self.simple_mode.motion.is_some() {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }

    fn bump_simple_motion(&mut self, kind: ChromeMotion) {
        self.simple_mode.motion = Some(SimpleChromeMotion {
            kind,
            started: Instant::now(),
            anchor: self.simple_motion_anchor(),
        });
    }

    fn simple_motion_anchor(&self) -> Option<Point> {
        self.selected
            .iter()
            .copied()
            .min()
            .and_then(|index| self.frame_positions.get(index).copied())
            .or_else(|| self.frame_positions.last().copied())
    }

    fn simple_motion_ease(&self) -> Option<(ChromeMotion, f32, Option<Point>)> {
        let motion = self.simple_mode.motion.as_ref()?;
        let t = (motion.started.elapsed().as_secs_f32() / SIMPLE_MOTION_SECS).clamp(0.0, 1.0);
        let ease = 1.0 - (1.0 - t).powi(3);
        Some((motion.kind, ease, motion.anchor))
    }

    fn simple_dock_height(&self) -> f32 {
        let progress = if self.document.sets.len() >= 2 {
            16.0
        } else {
            0.0
        };
        let primary = match self.simple_guide() {
            SimpleGuide::NextSet | SimpleGuide::Play => 66.0,
            SimpleGuide::Place | SimpleGuide::Move => 0.0,
        };
        20.0 + 78.0 + primary + progress
    }

    fn simple_header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.label(
                egui::RichText::new(i18n::registered(self.locale, "simple-mode.086"))
                    .size(13.0)
                    .color(super::app_theme::SECONDARY_TEXT),
            );
            let set_count = self.document.sets.len();
            let chips_w = (56.0 * set_count as f32).min((ui.available_width() - 180.0).max(56.0));
            ui.allocate_ui_with_layout(
                egui::Vec2::new(chips_w, 48.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    egui::ScrollArea::horizontal()
                        .id_salt("simple-scenes")
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            for index in 0..set_count {
                                self.simple_scene_chip(ui, index);
                            }
                        });
                },
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.simple_mode.draft_saved {
                    ui.label(
                        egui::RichText::new(text(self.locale, Text::Saved))
                            .size(13.0)
                            .color(super::app_theme::SECONDARY_TEXT),
                    );
                }
                if !self.document.performers.is_empty() {
                    ui.label(
                        egui::RichText::new(format!(
                            "{}{}",
                            self.document.performers.len(),
                            i18n::registered(self.locale, "simple-mode.084")
                        ))
                        .size(15.0)
                        .strong()
                        .color(SIMPLE_INK),
                    );
                }
            });
        });
    }

    fn simple_scene_chip(&mut self, ui: &mut egui::Ui, index: usize) {
        let current = index == self.current_set;
        let label = format!("{}", index + 1);
        let (rect, response) = ui.allocate_exact_size(egui::Vec2::new(48.0, 48.0), Sense::click());
        let fill = if current { SIMPLE_BLUE } else { Color32::WHITE };
        let ink = if current { Color32::WHITE } else { SIMPLE_INK };
        ui.painter().circle_filled(rect.center(), 22.0, fill);
        if !current {
            ui.painter().circle_stroke(
                rect.center(),
                22.0,
                Stroke::new(1.0, super::app_theme::HAIRLINE),
            );
        }
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(16.0),
            ink,
        );
        if response.clicked() && !current {
            self.simple_mode.overlap_note = false;
            self.navigate_to_set(index);
        }
    }

    fn simple_cue(&mut self, ui: &mut egui::Ui) {
        let message = match self.simple_guide() {
            SimpleGuide::Place => i18n::registered(self.locale, "simple-mode.074"),
            SimpleGuide::NextSet => i18n::registered(self.locale, "simple-mode.075"),
            SimpleGuide::Move => i18n::registered(self.locale, "simple-mode.076"),
            SimpleGuide::Play => i18n::registered(self.locale, "simple-mode.077"),
        };
        ui.label(
            egui::RichText::new(message)
                .size(22.0)
                .strong()
                .color(SIMPLE_INK),
        );
    }

    fn simple_field_card(&mut self, ui: &mut egui::Ui, height: f32) {
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(egui::Vec2::new(width, height), Sense::hover());
        let card = rect.shrink(8.0);
        let shadow = egui::Shadow {
            offset: [0, 6],
            blur: 16,
            spread: 0,
            color: Color32::from_black_alpha(18),
        };
        ui.painter().add(shadow.as_shape(card, 24));
        ui.painter().rect_filled(card, 24.0, SIMPLE_CARD);
        let inner = card.shrink(12.0);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
        self.simple_field_full_ui(&mut child, true);
        if self.playing {
            let pulse = 0.65 + 0.35 * (ui.input(|input| input.time) as f32 * 3.2).sin().abs();
            let color = Color32::from_rgba_unmultiplied(20, 96, 200, (pulse * 255.0) as u8);
            ui.painter()
                .rect_stroke(card, 24.0, Stroke::new(3.0, color), StrokeKind::Inside);
        }
        if let Some((ChromeMotion::Set, ease, _)) = self.simple_motion_ease()
            && self.nav_glide.position().is_none()
        {
            let alpha = ((1.0 - ease) * 110.0) as u8;
            ui.painter()
                .rect_filled(card, 24.0, Color32::from_white_alpha(alpha));
        }
    }

    fn simple_dock(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width().min(560.0);
        let height = self.simple_dock_height();
        let (outer, _) = ui.allocate_exact_size(
            egui::Vec2::new(ui.available_width(), height),
            Sense::hover(),
        );
        let dock = egui::Rect::from_center_size(
            outer.center(),
            egui::Vec2::new(width.min(outer.width()), height),
        );
        let shadow = egui::Shadow {
            offset: [0, 8],
            blur: 18,
            spread: 0,
            color: Color32::from_black_alpha(22),
        };
        ui.painter().add(shadow.as_shape(dock, 28));
        ui.painter().rect_filled(dock, 28.0, SIMPLE_CARD);
        let mut child = ui
            .new_child(egui::UiBuilder::new().max_rect(dock.shrink2(egui::Vec2::new(16.0, 12.0))));
        if self.document.sets.len() >= 2 {
            self.simple_progress(&mut child);
            child.add_space(8.0);
        }
        self.simple_dock_actions(&mut child);
        if matches!(
            self.simple_guide(),
            SimpleGuide::NextSet | SimpleGuide::Play
        ) {
            child.add_space(10.0);
            self.simple_primary(&mut child);
        }
    }

    fn simple_dock_actions(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let labels = [
                ("simple-mode.087", PhoneIcon::Undo, DockAction::Undo),
                ("simple-mode.081", PhoneIcon::Save, DockAction::Save),
                ("simple-mode.080", PhoneIcon::Open, DockAction::Open),
                ("simple-mode.079", PhoneIcon::More, DockAction::More),
            ];
            let count = labels.len() as f32;
            let gaps = 8.0 * (count - 1.0);
            let button_w = ((ui.available_width() - gaps) / count).clamp(72.0, 128.0);
            let row = button_w * count + gaps;
            ui.add_space(((ui.available_width() - row) / 2.0).max(0.0));
            for (id, icon, action) in labels {
                let enabled = action != DockAction::Undo || self.history.can_undo();
                let label = i18n::registered(self.locale, id);
                if self
                    .simple_dock_button(ui, icon, label, button_w, enabled)
                    .clicked()
                {
                    self.simple_dock_invoke(ui, action);
                }
            }
        });
    }

    fn simple_dock_invoke(&mut self, ui: &mut egui::Ui, action: DockAction) {
        match action {
            DockAction::Undo => {
                self.simple_mode.overlap_note = false;
                self.execute_command(UiCommand::Undo, ui.ctx());
            }
            DockAction::Save => self.save_dialog(),
            DockAction::Open => {
                if self.dirty && self.write_simple_draft() {
                    self.dirty = false;
                    self.simple_mode.draft_saved = true;
                }
                self.request_open_document(DocumentOpenKind::LegacyJson);
            }
            DockAction::More => self.set_simple_mode(false),
        }
    }

    fn simple_dock_button(
        &self,
        ui: &mut egui::Ui,
        icon: PhoneIcon,
        label: &str,
        width: f32,
        enabled: bool,
    ) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(
            egui::Vec2::new(width, 72.0),
            if enabled {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        let hovered = enabled && response.hovered();
        if hovered {
            ui.painter()
                .rect_filled(rect, 18.0, super::app_theme::ACCENT_SOFT);
        }
        let color = if enabled {
            SIMPLE_BLUE
        } else {
            Color32::from_rgba_unmultiplied(20, 96, 200, 70)
        };
        let ink = if enabled {
            SIMPLE_INK
        } else {
            super::app_theme::SECONDARY_TEXT.gamma_multiply(0.55)
        };
        paint_phone_icon(
            ui.painter(),
            rect.center() + egui::Vec2::new(0.0, -12.0),
            icon,
            color,
        );
        ui.painter().text(
            rect.center() + egui::Vec2::new(0.0, 16.0),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(13.0),
            ink,
        );
        response
    }

    fn simple_primary(&mut self, ui: &mut egui::Ui) {
        let guide = self.simple_guide();
        let (icon, label) = match guide {
            SimpleGuide::NextSet => (
                PhoneIcon::Next,
                i18n::registered(self.locale, "simple-mode.078"),
            ),
            SimpleGuide::Play if self.playing => (
                PhoneIcon::Pause,
                i18n::registered(self.locale, "simple-mode.089"),
            ),
            SimpleGuide::Play => (
                PhoneIcon::Play,
                i18n::registered(self.locale, "simple-mode.088"),
            ),
            SimpleGuide::Place | SimpleGuide::Move => return,
        };
        let (rect, response) =
            ui.allocate_exact_size(egui::Vec2::new(ui.available_width(), 56.0), Sense::click());
        let fill = if response.hovered() {
            Color32::from_rgb(16, 82, 176)
        } else {
            SIMPLE_BLUE
        };
        ui.painter().rect_filled(rect, rect.height() / 2.0, fill);
        let font = egui::FontId::proportional(18.0);
        let galley = ui
            .painter()
            .layout_no_wrap(label.to_owned(), font, Color32::WHITE);
        let gap = 10.0;
        let icon_w = 22.0;
        let total = icon_w + gap + galley.size().x;
        let left = rect.center().x - total / 2.0;
        paint_phone_icon(
            ui.painter(),
            Pos2::new(left + icon_w / 2.0, rect.center().y),
            icon,
            Color32::WHITE,
        );
        ui.painter().galley(
            Pos2::new(left + icon_w + gap, rect.center().y - galley.size().y / 2.0),
            galley,
            Color32::WHITE,
        );
        if response.clicked() {
            self.simple_mode.overlap_note = false;
            match guide {
                SimpleGuide::NextSet => self.duplicate_current_set(),
                SimpleGuide::Play => self.toggle_playback(ui.ctx()),
                SimpleGuide::Place | SimpleGuide::Move => {}
            }
        }
    }

    fn simple_progress(&self, ui: &mut egui::Ui) {
        let total = self.document.timeline_counts().max(1) as f32;
        let fraction = (self
            .document
            .global_count(self.current_set, self.count_position)
            / total)
            .clamp(0.0, 1.0);
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(egui::Vec2::new(width, 6.0), Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, 3.0, super::app_theme::HAIRLINE);
        if fraction > 0.0 {
            painter.rect_filled(
                egui::Rect::from_min_size(
                    rect.min,
                    egui::Vec2::new((rect.width() * fraction).max(6.0), rect.height()),
                ),
                3.0,
                SIMPLE_BLUE,
            );
        }
    }

    /// Tap on empty ground places. Tap on a person selects. Dragging is handled
    /// by the field painter, which already moves the current selection.
    fn simple_click(
        &mut self,
        field_point: Point,
        nearest: Option<usize>,
        snap: bool,
        additive: bool,
    ) {
        match nearest {
            None => self.place_performer_at(field_point, snap),
            Some(index) if additive => {
                let mut next = self.selected.clone();
                if !next.insert(index) {
                    next.remove(&index);
                }
                self.replace_selection(next);
            }
            Some(index) => self.replace_selection(std::iter::once(index).collect()),
        }
    }

    /// Full-field 2D view shared by every simple-mode screen except "move
    /// performers". Reuses the exact same rendering pipeline as the normal
    /// desktop view (`drill_render::build_field_2d` + `egui_backend::paint`).
    /// When `interactive`, Select/Move/Place match the approved full-mode
    /// field behavior (preview, drag ghost, snap-then-clamp, overlap warn).
    fn simple_field_full_ui(&mut self, ui: &mut egui::Ui, interactive: bool) {
        let available = ui.available_size();
        let sense = if interactive {
            Sense::click_and_drag()
        } else {
            Sense::hover()
        };
        let (response, painter) = ui.allocate_painter(available, sense);
        let rect = response.rect.shrink(12.0);
        let render_options = drill_render::RenderOptions {
            margin: 0.0,
            ..drill_render::RenderOptions::default()
        };
        let scene = drill_render::Scene {
            document: &self.document,
            positions: &self.frame_positions,
            viewport: drill_render::Viewport {
                size: drill_render::Vec2 {
                    x: rect.width(),
                    y: rect.height(),
                },
                ui_scale: 1.0,
            },
            options: &render_options,
            theme: &drill_render::Theme::PRINT_LIGHT,
        };
        drill_render::build_field_2d(&scene, &mut self.render_scratch, &mut self.display_list);
        egui_backend::paint(&painter, rect.min, &self.display_list);
        let field_map = drill_render::FieldMap::new(
            self.document.grid.width,
            self.document.grid.height,
            drill_render::Vec2 {
                x: rect.width(),
                y: rect.height(),
            },
            render_options.margin,
        );
        let to_screen = |point: Point| {
            let v = field_map.map(point);
            Pos2::new(rect.left() + v.x, rect.top() + v.y)
        };
        let from_screen = |pos: Pos2| {
            field_map.unmap(drill_render::Vec2 {
                x: pos.x - rect.left(),
                y: pos.y - rect.top(),
            })
        };
        if self.document.performers.is_empty() {
            let center = rect.center();
            let pop = self
                .simple_motion_ease()
                .filter(|(kind, _, _)| *kind == ChromeMotion::Empty)
                .map(|(_, ease, _)| 0.86 + 0.14 * ease)
                .unwrap_or(1.0);
            let radius = 40.0 * pop;
            painter.circle_filled(
                center,
                radius,
                Color32::from_rgba_unmultiplied(76, 163, 255, 48),
            );
            painter.circle_stroke(center, radius, Stroke::new(2.0, SIMPLE_BLUE));
            painter.text(
                center,
                egui::Align2::CENTER_CENTER,
                "+",
                egui::FontId::proportional(34.0),
                SIMPLE_BLUE,
            );
            painter.text(
                center + egui::Vec2::new(0.0, radius + 16.0),
                egui::Align2::CENTER_TOP,
                i18n::registered(self.locale, "simple-mode.082"),
                egui::FontId::proportional(18.0),
                SIMPLE_BLUE,
            );
        }
        if interactive {
            self.field_pointer = response.hover_pos().map(from_screen);
            let hover_on_dot = response.hover_pos().is_some_and(|pos| {
                self.frame_positions
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| self.is_selectable_index(*index))
                    .any(|(_, point)| to_screen(*point).distance(pos) < 18.0)
            });
            if response.hovered() {
                let editable = self.is_editable_set_start();
                ui.ctx().set_cursor_icon(if self.drag_before.is_some() {
                    egui::CursorIcon::Grabbing
                } else if hover_on_dot && editable {
                    egui::CursorIcon::Grab
                } else if hover_on_dot {
                    egui::CursorIcon::NotAllowed
                } else if editable {
                    egui::CursorIcon::Crosshair
                } else {
                    egui::CursorIcon::Default
                });
            }
            if self.is_editable_set_start()
                && self.drag_before.is_none()
                && !hover_on_dot
                && let Some(raw) = self.field_pointer
            {
                let snap =
                    self.document.grid.snap_enabled && !ui.input(|input| input.modifiers.shift);
                let pos = to_screen(controller::field_point(raw, &self.document, snap));
                painter.circle_filled(pos, 8.0, Color32::from_rgba_unmultiplied(100, 235, 255, 80));
                painter.circle_stroke(pos, 8.0, Stroke::new(2.0, Color32::from_rgb(100, 235, 255)));
            }
        }
        if let Some(preview) = &self.drag_preview {
            if let Some(before) = &self.drag_before {
                for &point in before {
                    painter.circle_filled(to_screen(point), 9.0, Color32::from_black_alpha(110));
                }
            }
            for &point in preview {
                let pos = to_screen(point);
                painter.circle_filled(pos, 8.0, Color32::from_rgb(100, 235, 255));
                painter.circle_stroke(pos, 11.0, Stroke::new(2.0, Color32::WHITE));
            }
        }
        let select_ease = self
            .simple_motion_ease()
            .filter(|(kind, _, _)| matches!(kind, ChromeMotion::Select | ChromeMotion::Place))
            .map(|(_, ease, _)| ease)
            .unwrap_or(1.0);
        for (index, &point) in self.frame_positions.iter().enumerate() {
            if self.selected.contains(&index) && self.drag_preview.is_none() {
                let pos = to_screen(point);
                let ring = 8.0 + 6.0 * select_ease;
                painter.circle_filled(
                    pos,
                    ring + 4.0,
                    Color32::from_rgba_unmultiplied(20, 96, 200, 36),
                );
                painter.circle_stroke(pos, ring, Stroke::new(3.0, SIMPLE_BLUE));
                if let Some(performer) = self.document.performers.get(index) {
                    painter.text(
                        pos + egui::Vec2::new(0.0, ring + 4.0),
                        egui::Align2::CENTER_TOP,
                        &performer.label,
                        egui::FontId::proportional(14.0),
                        SIMPLE_BLUE,
                    );
                }
                if self.selected.len() >= 2 {
                    self.paint_selection_rank_badge(&painter, pos, index);
                }
            }
        }
        if let Some((kind, ease, Some(anchor))) = self.simple_motion_ease()
            && matches!(kind, ChromeMotion::Place | ChromeMotion::Move)
        {
            let pos = to_screen(anchor);
            let radius = 8.0 + 26.0 * ease;
            let alpha = ((1.0 - ease) * 160.0) as u8;
            painter.circle_stroke(
                pos,
                radius,
                Stroke::new(2.0, Color32::from_rgba_unmultiplied(20, 96, 200, alpha)),
            );
        }
        if !interactive {
            return;
        }
        let Some(pointer) = response.interact_pointer_pos().or(response.hover_pos()) else {
            return;
        };
        let nearest = self
            .frame_positions
            .iter()
            .enumerate()
            .filter(|(index, _)| self.is_selectable_index(*index))
            .min_by(|(_, a), (_, b)| {
                to_screen(**a)
                    .distance(pointer)
                    .total_cmp(&to_screen(**b).distance(pointer))
            })
            .filter(|(_, p)| to_screen(**p).distance(pointer) < 18.0)
            .map(|(index, _)| index);
        let shift_held = ui.input(|input| input.modifiers.shift);
        let snap_now = self.document.grid.snap_enabled && !shift_held;
        if let Some(pointer) = response.interact_pointer_pos() {
            if response.clicked() {
                let additive = ui.input(|input| {
                    input.modifiers.command || input.modifiers.ctrl || input.modifiers.shift
                });
                self.simple_click(from_screen(pointer), nearest, snap_now, additive);
            }
            if response.drag_started() {
                if let Some(index) = nearest {
                    if self.is_editable_set_start() {
                        if !self.selected.contains(&index) {
                            self.replace_selection(std::iter::once(index).collect());
                        }
                        self.begin_field_drag(pointer);
                    } else {
                        self.ensure_editable_set_start();
                    }
                } else if self.field_tool == FieldTool::Move && !self.selected.is_empty() {
                    if self.is_editable_set_start() {
                        self.begin_field_drag(pointer);
                    } else {
                        self.ensure_editable_set_start();
                    }
                }
            }
            if response.dragged() && self.is_editable_set_start() && self.drag_before.is_some() {
                self.update_field_drag(pointer, field_map.scale, snap_now);
            }
            if response.drag_stopped() {
                self.commit_field_drag();
                self.bump_simple_motion(ChromeMotion::Move);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_simple_app() -> DrillApp {
        let mut app = DrillApp {
            simple_mode: SimpleModeState {
                enabled: true,
                ..SimpleModeState::default()
            },
            ..DrillApp::default()
        };
        app.begin_simple_show();
        app
    }

    #[test]
    fn first_open_and_first_place_get_a_short_motion() {
        let mut app = empty_simple_app();
        let ctx = egui::Context::default();
        app.note_simple_chrome_motion(&ctx);
        assert!(matches!(
            app.simple_mode.motion.as_ref().map(|motion| motion.kind),
            Some(ChromeMotion::Empty)
        ));
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        app.note_simple_chrome_motion(&ctx);
        assert!(matches!(
            app.simple_mode.motion.as_ref().map(|motion| motion.kind),
            Some(ChromeMotion::Place)
        ));
    }

    #[test]
    fn empty_roster_recommends_place() {
        let app = empty_simple_app();
        assert!(app.document.performers.is_empty());
        assert_eq!(app.simple_guide(), SimpleGuide::Place);
    }

    #[test]
    fn first_place_offers_the_next_scene_before_playback() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        assert_eq!(app.document.performers.len(), 1);
        assert_eq!(app.document.performers[0].label, "1");
        assert_eq!(app.simple_guide(), SimpleGuide::NextSet);
        assert_eq!(app.field_tool, FieldTool::Move);
        app.duplicate_current_set();
        assert_eq!(app.simple_guide(), SimpleGuide::Move);
    }

    #[test]
    fn two_sets_recommend_play_once_someone_has_moved() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        app.duplicate_current_set();
        assert!(app.document.sets.len() >= 2);
        assert_eq!(app.simple_guide(), SimpleGuide::Move);
        app.replace_selection([0].into_iter().collect());
        let set_id = app.document.sets[app.current_set].id;
        let performer_id = app.document.performers[0].id;
        let mut positions = app.document.sets[app.current_set].positions.clone();
        positions[0].x += 2.0;
        assert!(app.execute_edit(
            Edit::MovePerformers {
                set_id,
                performer_ids: vec![performer_id],
                positions,
            },
            "move",
        ));
        assert!(app.show_has_motion());
        assert_eq!(app.simple_guide(), SimpleGuide::Play);
    }

    #[test]
    fn guided_place_move_next_set_and_play_is_edit_routed() {
        let mut app = empty_simple_app();
        let before = app.document.clone();
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        assert_ne!(app.document, before);
        assert!(app.history.can_undo());
        app.replace_selection([0].into_iter().collect());
        let start = app.document.sets[0].positions[0];
        app.begin_field_drag(Pos2::new(0.0, 0.0));
        app.update_field_drag(Pos2::new(50.0, 0.0), 10.0, true);
        app.commit_field_drag();
        assert_ne!(app.document.sets[0].positions[0], start);
        assert!(app.onboarding.simple_drag_tip_seen);
        let sets_before = app.document.sets.len();
        app.duplicate_current_set();
        assert_eq!(app.document.sets.len(), sets_before + 1);
        let context = egui::Context::default();
        app.toggle_playback(&context);
        assert!(app.playing);
        app.toggle_playback(&context);
        assert!(!app.playing);
    }

    #[test]
    fn simple_mode_selection_uses_the_shared_restore_stack() {
        let mut app = DrillApp::default();
        app.replace_selection([0_usize, 2].into_iter().collect());
        app.replace_selection(std::iter::once(1_usize).collect());

        assert_eq!(app.selected, [1_usize].into_iter().collect());
        assert!(app.can_restore_selection());
        app.restore_recent_selection();
        assert_eq!(app.selected, [0_usize, 2].into_iter().collect());
        assert!(!app.history.can_undo());
    }

    #[test]
    fn metronome_runs_independently_of_document_tempo() {
        let mut metronome = MetronomeState::default();
        assert!(!metronome.is_running());
        metronome.set_bpm(180.0);
        assert!((metronome.bpm - 180.0).abs() < 0.01);
        // Starting the metronome may fail in a headless CI environment with
        // no audio device; either outcome must leave `running` consistent
        // with whether an output device was actually opened.
        metronome.start();
        assert_eq!(metronome.is_running(), metronome.output.is_some());
        metronome.stop();
        assert!(!metronome.is_running());
    }

    #[test]
    fn metronome_start_never_opens_device_or_builds_clicks_on_caller_thread() {
        let source = include_str!("simple_mode.rs");
        let start = source
            .split("pub fn start(&mut self)")
            .nth(1)
            .and_then(|tail| tail.split("pub fn stop").next())
            .expect("start method source");
        assert!(!start.contains("AudioOutput::open_default"));
        assert!(!start.contains("ClickSchedule::build"));
        assert!(start.contains("begin_open"));
        assert!(start.contains("begin_schedule"));
    }

    #[test]
    fn simple_mode_place_uses_the_shared_pointer_and_warns_on_overlap() {
        let mut app = empty_simple_app();
        app.field_tool = FieldTool::Place;
        let target = Point { x: 8.0, y: 6.0 };
        app.field_pointer = Some(target);
        let roster = app.document.performers.len();
        app.place_performer_at(target, true);
        assert_eq!(app.document.performers.len(), roster + 1);
        let placed = *app.document.sets[0]
            .positions
            .last()
            .expect("placed performer");
        assert_eq!(placed, controller::field_point(target, &app.document, true));

        app.field_tool = FieldTool::Place;
        app.place_performer_at(placed, true);
        assert_eq!(app.document.performers.len(), roster + 2);
        assert_ne!(app.status, text(Locale::Ja, Text::Ready));
        assert!(app.simple_mode.overlap_note);
    }

    #[test]
    fn simple_mode_place_and_move_respect_playback_lock() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 5.0, y: 5.0 }, true);
        app.playing = true;
        let roster = app.document.performers.len();
        app.place_performer_at(Point { x: 6.0, y: 6.0 }, true);
        assert_eq!(app.document.performers.len(), roster);
        assert!(!app.is_editable_set_start());
        app.replace_selection([0].into_iter().collect());
        let origin = app.document.sets[0].positions[0];
        app.begin_field_drag(Pos2::new(0.0, 0.0));
        app.update_field_drag(Pos2::new(80.0, 0.0), 10.0, true);
        app.commit_field_drag();
        assert_eq!(app.document.sets[0].positions[0], origin);
    }

    #[test]
    fn simple_mode_move_snaps_clamps_and_shift_unsnaps() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        app.field_tool = FieldTool::Move;
        app.replace_selection([0].into_iter().collect());
        let start = app.document.sets[0].positions[0];
        app.document.grid.snap_enabled = true;

        app.begin_field_drag(Pos2::new(0.0, 0.0));
        app.update_field_drag(Pos2::new(50.0, 0.0), 10.0, true);
        let snapped = app
            .drag_preview
            .as_ref()
            .and_then(|preview| preview.first().copied());
        app.commit_field_drag();
        let after = app.document.sets[0].positions[0];
        assert_eq!(snapped, Some(after));
        assert_eq!(
            after,
            controller::drag_point(start, (50.0, 0.0), 10.0, &app.document, true)
        );
        assert_eq!(after, app.document.grid.snap(after));

        app.begin_field_drag(Pos2::new(0.0, 0.0));
        app.update_field_drag(Pos2::new(13.0, 0.0), 10.0, false);
        app.commit_field_drag();
        let unsnapped = app.document.sets[0].positions[0];
        assert_eq!(
            unsnapped,
            controller::drag_point(after, (13.0, 0.0), 10.0, &app.document, false)
        );

        app.begin_field_drag(Pos2::new(0.0, 0.0));
        app.update_field_drag(Pos2::new(10_000.0, 0.0), 10.0, true);
        app.commit_field_drag();
        assert_eq!(
            app.document.sets[0].positions[0].x,
            app.document.grid.max_x()
        );
    }

    #[test]
    fn tapping_empty_ground_places_without_a_tool_change() {
        let mut app = empty_simple_app();
        app.field_tool = FieldTool::Move;
        app.simple_click(Point { x: 8.0, y: 6.0 }, None, true, false);
        assert_eq!(app.document.performers.len(), 1);
        app.simple_click(Point { x: 14.0, y: 6.0 }, None, true, false);
        assert_eq!(app.document.performers.len(), 2);
        app.simple_click(Point { x: 0.0, y: 0.0 }, Some(0), true, false);
        assert_eq!(app.selected, [0_usize].into_iter().collect());
        assert_eq!(app.document.performers.len(), 2);
    }

    #[test]
    fn first_launch_skips_the_menu_and_opens_an_empty_field() {
        let mut app = DrillApp::default();
        let missing = std::env::temp_dir().join(format!(
            "drillforge-missing-draft-{}-{}.json",
            std::process::id(),
            "first"
        ));
        let _ = std::fs::remove_file(&missing);
        app.simple_mode.draft_path_override = Some(missing);
        assert!(app.onboarding.show_welcome);
        app.open_into_preferred_editor();
        assert!(app.simple_mode.enabled);
        assert!(app.onboarding.welcome_seen);
        assert!(!app.onboarding.show_welcome);
        assert!(app.document.performers.is_empty());
        assert_eq!(app.simple_guide(), SimpleGuide::Place);
    }

    #[test]
    fn returning_full_editor_user_is_not_sent_to_simple_mode() {
        let mut app = DrillApp::default();
        app.onboarding.welcome_seen = true;
        app.onboarding.prefer_simple = false;
        app.onboarding.show_welcome = false;
        let performers = app.document.performers.len();
        app.open_into_preferred_editor();
        assert!(!app.simple_mode.enabled);
        assert_eq!(app.document.performers.len(), performers);
    }

    #[test]
    fn simple_draft_reopens_the_same_people() {
        let dir = std::env::temp_dir().join(format!("drillforge-draft-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("simple-draft.drill.json");
        let mut app = empty_simple_app();
        app.simple_mode.draft_path_override = Some(path.clone());
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        app.place_performer_at(Point { x: 12.0, y: 9.0 }, true);
        assert!(app.write_simple_draft());
        let mut reopened = DrillApp::default();
        reopened.onboarding.welcome_seen = true;
        reopened.onboarding.prefer_simple = true;
        reopened.onboarding.show_welcome = false;
        reopened.simple_mode.draft_path_override = Some(path);
        reopened.open_into_preferred_editor();
        assert!(reopened.simple_mode.enabled);
        assert_eq!(reopened.document.performers.len(), 2);
        assert_eq!(reopened.document.performers[0].label, "1");
        assert!(reopened.simple_mode.draft_saved);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn leaving_simple_mode_remembers_the_full_editor() {
        let mut app = empty_simple_app();
        app.set_simple_mode(false);
        assert!(!app.simple_mode.enabled);
        assert!(!app.onboarding.prefer_simple);
        assert!(app.onboarding.welcome_seen);
        assert!(!app.onboarding.show_welcome);
    }
}
