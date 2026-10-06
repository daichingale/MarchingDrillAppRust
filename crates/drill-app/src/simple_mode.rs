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

enum SimplePrimary {
    Next,
    Play,
    Pause,
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
    /// Shown once after leaving simple mode, so the full editor is not a trap.
    pub(crate) return_hint: bool,
    /// Left-drag on empty ground pans the field instead of moving people.
    empty_pan: bool,
    /// Playback reached the end, so the next play watches from the start.
    finished_playback: bool,
    /// In-app name sheet. The file dialog stays on the full editor.
    naming: bool,
    name_draft: String,
    /// A named save is in flight. Failure reopens the sheet.
    naming_pending: bool,
    saved_note_until: Option<Instant>,
    /// Looking at a count between pictures. The arrival picture stays put
    /// until they go back to the start of the scene or press play.
    hold_count: bool,
    /// Name field for the single selected person.
    person_name: String,
    person_name_for: Option<usize>,
    /// Tests redirect the shows folder away from real app data.
    shows_dir_override: Option<std::path::PathBuf>,
    /// Short catalog id for the edit that just landed, consumed when the
    /// history cursor moves. Session-only; the document JSON is unchanged.
    pending_note: Option<&'static str>,
    undo_notes: Vec<&'static str>,
    redo_notes: Vec<&'static str>,
    seen_cursor: Option<usize>,
    seen_redo: Option<usize>,
    /// Readable walk list for the current scene. Nothing is written.
    memo_open: bool,
    memo_copied: bool,
    /// Draft for the current scene's name. A stock "セット 1" shows as empty.
    scene_name: String,
    scene_name_for: Option<usize>,
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
    Redo,
    Save,
    Open,
    More,
}

#[derive(Clone, Copy)]
enum PhoneIcon {
    Undo,
    Redo,
    Save,
    Open,
    More,
    Play,
    Pause,
    Next,
    Trash,
}

fn simple_choice_button(
    ui: &mut egui::Ui,
    label: &str,
    enabled: bool,
    active: bool,
) -> egui::Response {
    let color = if active { Color32::WHITE } else { SIMPLE_BLUE };
    let mut button = egui::Button::new(egui::RichText::new(label).size(15.0).color(color))
        .corner_radius(18.0)
        .min_size(egui::Vec2::new(72.0, 36.0));
    if active {
        button = button.fill(SIMPLE_BLUE);
    } else {
        button = button
            .fill(Color32::WHITE)
            .stroke(Stroke::new(1.5, SIMPLE_BLUE));
    }
    ui.add_enabled(enabled, button)
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
        PhoneIcon::Redo => {
            painter.add(egui::Shape::line(
                vec![
                    p(-6.0, -5.0),
                    p(1.0, -5.0),
                    p(6.0, 0.0),
                    p(1.0, 5.0),
                    p(-6.0, 5.0),
                ],
                stroke,
            ));
            painter.line_segment([p(-6.0, -5.0), p(-1.0, -9.0)], stroke);
            painter.line_segment([p(-6.0, -5.0), p(-2.0, -1.0)], stroke);
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
        PhoneIcon::Trash => {
            painter.line_segment([p(-6.0, -4.0), p(6.0, -4.0)], stroke);
            painter.line_segment([p(-2.0, -4.0), p(-2.0, -7.0)], stroke);
            painter.line_segment([p(-2.0, -7.0), p(2.0, -7.0)], stroke);
            painter.line_segment([p(2.0, -7.0), p(2.0, -4.0)], stroke);
            painter.add(egui::Shape::line(
                vec![p(-5.0, -2.0), p(-4.0, 7.0), p(4.0, 7.0), p(5.0, -2.0)],
                stroke,
            ));
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
        let was_enabled = self.simple_mode.enabled;
        self.simple_mode.enabled = enabled;
        self.onboarding.prefer_simple = enabled;
        self.onboarding.welcome_seen = true;
        self.onboarding.show_welcome = false;
        if was_enabled && !enabled {
            self.simple_mode.return_hint = true;
        }
        if enabled {
            self.simple_mode.return_hint = false;
        }
        if was_enabled != enabled {
            self.simple_mode.pending_note = None;
            self.simple_mode.undo_notes.clear();
            self.simple_mode.redo_notes.clear();
            self.simple_mode.seen_cursor = None;
            self.simple_mode.seen_redo = None;
            self.simple_mode.memo_open = false;
            self.simple_mode.memo_copied = false;
        }
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
        if drill_project::atomic_write(&path, json.as_bytes(), None).is_err() {
            return false;
        }
        self.mirror_named_show(json.as_bytes());
        self.write_simple_path_sidecar();
        true
    }

    fn simple_path_sidecar(&self) -> std::path::PathBuf {
        let mut sidecar = self.simple_draft_path();
        sidecar.set_extension("path");
        sidecar
    }

    /// The draft is the crash copy. Once a show has a name, the same bytes
    /// also stay in that file so opening it later is not an older picture.
    fn mirror_named_show(&self, json: &[u8]) {
        let Some(path) = &self.current_path else {
            return;
        };
        if path == &self.simple_draft_path() || !path.extension().is_some_and(|ext| ext == "json") {
            return;
        }
        let backup = path.with_extension("backup.drill.json");
        let _ = drill_project::atomic_write(path, json, path.exists().then_some(backup.as_path()));
    }

    fn write_simple_path_sidecar(&self) {
        let sidecar = self.simple_path_sidecar();
        match &self.current_path {
            Some(path) => {
                let _ = std::fs::write(&sidecar, path.to_string_lossy().as_bytes());
            }
            None => {
                let _ = std::fs::remove_file(&sidecar);
            }
        }
    }

    fn restore_simple_named_path(&mut self) {
        let Ok(text) = std::fs::read_to_string(self.simple_path_sidecar()) else {
            return;
        };
        let path = std::path::PathBuf::from(text.trim());
        if path.is_file() {
            self.current_path = Some(path);
        }
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
        self.restore_simple_named_path();
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

    /// Screenshot harness only. Seeds a frame when `DRILLFORGE_QA_SIMPLE` is
    /// `empty`, `placed`, `play`, `glossary`, `selected`, `recent`, `full`,
    /// `move`, `save`, `done`, `line`, `step`, `circle`, `nudge`, `walk`,
    /// `shows`, `hints`, `memo`, `numbers`, `shape`, or `block`.
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
        if stage == "glossary" {
            self.glossary.open();
        }
        if stage == "selected" {
            self.replace_selection(std::iter::once(0).collect());
        }
        if stage == "recent" {
            self.recent_projects.preview_empty_for_screenshot();
            self.show_recent_projects = true;
        }
        if stage == "full" {
            self.set_simple_mode(false);
        }
        if stage == "move" || stage == "play" || stage == "done" {
            self.duplicate_current_set();
        }
        if stage == "play" || stage == "done" {
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
        if stage == "play" {
            // Hold a mid-move count so the screenshot shows the readout
            // instead of racing through the transition on the first frames.
            self.playing = true;
            self.speed = 0.0;
            self.count_position = 6.0;
        }
        if stage == "done" {
            self.playing = false;
            self.count_position = 0.0;
            self.simple_mode.finished_playback = true;
        }
        if stage == "save" {
            self.simple_mode.naming = true;
            self.simple_mode.name_draft = "文化祭".to_string();
        }
        if stage == "line" {
            self.place_performer_at(Point { x: 8.0, y: 28.0 }, true);
            self.place_performer_at(Point { x: 30.0, y: 8.0 }, true);
            self.simple_line_up();
        }
        if stage == "circle" {
            self.place_performer_at(Point { x: 8.0, y: 28.0 }, true);
            self.place_performer_at(Point { x: 30.0, y: 8.0 }, true);
            self.simple_circle_up();
            self.duplicate_current_set();
            self.navigate_to_set(0);
            self.nav_glide.settle();
        }
        if stage == "step" {
            self.duplicate_current_set();
            let set_id = self.document.sets[self.current_set].id;
            let performer_id = self.document.performers[0].id;
            let moved = self.document.sets[self.current_set].positions[0];
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
            self.playing = false;
            self.navigate_to_set(0);
            self.nav_glide.settle();
            self.simple_step_beat(6);
            self.nav_glide.settle();
        }
        if stage == "nudge" {
            self.replace_selection(std::iter::once(0).collect());
        }
        if stage == "walk" {
            self.duplicate_current_set();
            let step = self.document.grid.horizontal_units
                / f32::from(self.document.grid.horizontal_steps.max(1));
            let set_id = self.document.sets[self.current_set].id;
            let performer_id = self.document.performers[0].id;
            let origin = self.document.sets[self.current_set].positions[0];
            let _ = self.execute_edit(
                Edit::MovePerformers {
                    set_id,
                    performer_ids: vec![performer_id],
                    positions: vec![Point {
                        x: origin.x + step * 8.0,
                        y: origin.y,
                    }],
                },
                "qa",
            );
            self.navigate_to_set(self.current_set);
            self.nav_glide.settle();
            self.replace_selection(std::iter::once(0).collect());
        }
        if stage == "shows" {
            let dir = std::env::temp_dir().join("drillforge-qa-shows");
            let _ = std::fs::create_dir_all(&dir);
            let path = dir.join("文化祭.drill.json");
            if let Ok(json) = self.document.to_json() {
                let _ = std::fs::write(&path, json);
            }
            self.recent_projects
                .preview_paths_for_screenshot(vec![path]);
            self.show_recent_projects = true;
        }
        if stage == "hints" || stage == "memo" || stage == "numbers" {
            self.simple_seed_hint_picture();
        }
        if stage == "memo" {
            self.simple_mode.memo_open = true;
        }
        if stage == "numbers" {
            self.simple_renumber();
            self.simple_sync_history_notes();
        }
        if stage == "shape" {
            self.place_performer_at(Point { x: 8.0, y: 28.0 }, true);
            self.place_performer_at(Point { x: 30.0, y: 8.0 }, true);
            self.place_performer_at(Point { x: 18.0, y: 22.0 }, true);
            self.simple_arc_up();
            self.simple_rename_scene("サビ");
        }
        if stage == "block" {
            self.place_performer_at(Point { x: 8.0, y: 28.0 }, true);
            self.place_performer_at(Point { x: 30.0, y: 8.0 }, true);
            self.place_performer_at(Point { x: 18.0, y: 22.0 }, true);
            self.place_performer_at(Point { x: 36.0, y: 14.0 }, true);
            self.simple_block_up();
            self.duplicate_current_set();
            let step = self.document.grid.horizontal_units
                / f32::from(self.document.grid.horizontal_steps.max(1));
            let origin = self.document.sets[self.current_set].positions[0];
            let set_id = self.document.sets[self.current_set].id;
            let performer_id = self.document.performers[0].id;
            let _ = self.execute_edit(
                Edit::MovePerformers {
                    set_id,
                    performer_ids: vec![performer_id],
                    positions: vec![Point {
                        x: (origin.x - step * 4.0).max(0.0),
                        y: origin.y,
                    }],
                },
                "qa",
            );
            self.navigate_to_set(self.current_set);
            self.nav_glide.settle();
            self.replace_selection(std::iter::once(0).collect());
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
        // just computed. Stepping one count is the other exception, so a
        // paused picture can stay where they stopped to look.
        if self.playing {
            self.simple_mode.hold_count = false;
        } else if self.nav_glide.position().is_none() && !self.simple_holding_a_count() {
            self.count_position = 0.0;
            self.simple_mode.hold_count = false;
            self.document
                .positions_at(self.current_set, 0.0, &mut self.frame_positions);
        }
        // One gesture language: tap empty ground to place, drag a person to
        // move. There is no tool to switch.
        self.field_tool = FieldTool::Move;
        if self.simple_mode.seen_cursor.is_none() {
            self.simple_mode.seen_cursor = Some(self.history.cursor());
            self.simple_mode.seen_redo = Some(self.history.redo_len());
        }
        self.simple_try_arrow_nudge(ui);
        self.autosave_simple_draft(ui.ctx());
        self.expire_simple_saved_note(ui.ctx());
        self.note_simple_chrome_motion(ui.ctx());
        ui.scope(|ui| {
            ui.style_mut().spacing.item_spacing = egui::Vec2::new(12.0, 10.0);
            ui.style_mut().spacing.button_padding = egui::Vec2::new(16.0, 10.0);
            ui.add_space(8.0);
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(20, 0))
                .show(ui, |ui| {
                    self.simple_header(ui);
                    self.simple_scene_name_row(ui);
                    ui.add_space(8.0);
                    self.simple_cue(ui);
                    ui.add_space(6.0);
                    self.simple_beat_row(ui);
                    self.simple_rehearsal_row(ui);
                    self.simple_arrange_row(ui);
                    self.simple_touch_row(ui);
                    if !self.playing {
                        self.simple_caution_row(ui);
                    }
                    if !self.playing
                        && !simple_move_paths(&self.document, self.current_set).is_empty()
                    {
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(i18n::registered(self.locale, "simple-mode.147"))
                                .size(14.0)
                                .color(super::app_theme::SECONDARY_TEXT),
                        );
                        if let Some(caption) = self.simple_travel_caption() {
                            ui.label(
                                egui::RichText::new(caption)
                                    .size(16.0)
                                    .strong()
                                    .color(SIMPLE_BLUE),
                            );
                        }
                    }
                    if self.simple_mode.overlap_note {
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(i18n::registered(self.locale, "simple-mode.083"))
                                .size(14.0)
                                .color(super::app_theme::SECONDARY_TEXT),
                        );
                    }
                    if ui.input(|input| input.modifiers.shift)
                        && !self.document.performers.is_empty()
                    {
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(i18n::registered(self.locale, "simple-mode.103"))
                                .size(14.0)
                                .color(super::app_theme::SECONDARY_TEXT),
                        );
                    }
                    ui.add_space(10.0);
                    self.simple_sync_history_notes();
                    let dock_h = self.simple_dock_height();
                    let field_h = (ui.available_height() - dock_h - 12.0).max(180.0);
                    self.simple_field_card(ui, field_h);
                    ui.add_space(12.0);
                    self.simple_dock(ui);
                });
        });
        self.simple_sync_history_notes();
        self.show_update_notice(ui.ctx());
        self.show_recent_projects(ui.ctx());
        self.simple_name_sheet(ui.ctx());
        self.simple_memo_sheet(ui.ctx());
        self.glossary.show(ui.ctx(), self.locale);
        self.onboarding.help_ui(ui.ctx(), self.locale);
        self.onboarding.persist_if_changed();
    }

    pub(crate) fn show_simple_return_hint(&mut self, ui: &mut egui::Ui) {
        if !self.simple_mode.return_hint {
            return;
        }
        ui.add_space(6.0);
        egui::Frame::new()
            .fill(super::app_theme::ACCENT_SOFT)
            .inner_margin(egui::Margin::symmetric(16, 10))
            .corner_radius(16)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(i18n::registered(self.locale, "simple-mode.106"))
                            .color(SIMPLE_INK),
                    );
                    if ui
                        .button(i18n::registered(self.locale, "simple-mode.105"))
                        .clicked()
                    {
                        self.set_simple_mode(true);
                    }
                    if ui.small_button("×").clicked() {
                        self.simple_mode.return_hint = false;
                    }
                });
            });
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

    fn simple_primary_action(&self) -> Option<SimplePrimary> {
        if self.playing {
            return Some(SimplePrimary::Pause);
        }
        match self.simple_guide() {
            SimpleGuide::NextSet => Some(SimplePrimary::Next),
            SimpleGuide::Play => Some(SimplePrimary::Play),
            SimpleGuide::Place | SimpleGuide::Move => None,
        }
    }

    fn simple_can_remove(&self) -> bool {
        !self.selected.is_empty() && self.is_editable_set_start()
    }

    fn simple_view_fitted(&self) -> bool {
        (self.field_viewport.zoom - 1.0).abs() < 0.05
    }

    fn simple_dock_height(&self) -> f32 {
        let mut height = 24.0 + 72.0 + 8.0;
        if self.document.sets.len() >= 2 {
            height += 14.0;
        }
        if self.simple_name_row_visible() {
            height += 48.0;
        }
        if self.simple_can_remove() {
            height += 52.0;
        }
        if self.simple_primary_action().is_some() {
            height += 66.0;
        }
        let notes = self.simple_history_lines().len();
        if notes > 0 {
            height += 8.0 + 22.0 * notes as f32;
        }
        height
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
                ui.spacing_mut().item_spacing.x = 8.0;
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
                if let Some((status, saved)) = self.simple_status_label() {
                    let color = if saved {
                        SIMPLE_BLUE
                    } else {
                        super::app_theme::SECONDARY_TEXT
                    };
                    ui.label(egui::RichText::new(status).size(13.0).color(color));
                }
                if ui
                    .button(i18n::registered(self.locale, "glossary.001"))
                    .clicked()
                {
                    self.glossary.open();
                }
                if ui
                    .button(i18n::registered(self.locale, "simple-mode.099"))
                    .on_hover_text(i18n::registered(self.locale, "simple-mode.110"))
                    .clicked()
                {
                    self.show_recent_projects = true;
                }
            });
        });
    }

    fn simple_scene_name_row(&mut self, ui: &mut egui::Ui) {
        if self.playing || self.document.performers.is_empty() || !self.is_editable_set_start() {
            return;
        }
        let Some(stored) = self
            .document
            .sets
            .get(self.current_set)
            .map(|set| set.name.clone())
        else {
            return;
        };
        if self.simple_mode.scene_name_for != Some(self.current_set) {
            self.simple_mode.scene_name = simple_scene_name_draft(&stored);
            self.simple_mode.scene_name_for = Some(self.current_set);
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label(
                egui::RichText::new(i18n::registered(self.locale, "simple-mode.203"))
                    .size(15.0)
                    .color(SIMPLE_INK),
            );
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.simple_mode.scene_name)
                    .hint_text(i18n::registered(self.locale, "simple-mode.204"))
                    .desired_width(220.0),
            );
            let submit = response.lost_focus()
                || (response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)));
            if submit {
                let name = self.simple_mode.scene_name.clone();
                self.simple_rename_scene(&name);
            }
            if !response.has_focus()
                && let Some(set) = self.document.sets.get(self.current_set)
            {
                self.simple_mode.scene_name = simple_scene_name_draft(&set.name);
            }
        });
    }

    fn simple_default_scene_name(&self, index: usize) -> String {
        match self.locale {
            Locale::Ja => format!("セット {}", index + 1),
            Locale::En => format!("Set {}", index + 1),
        }
    }

    /// Keeps a typed name, or the usual "セット N" when the field is cleared.
    /// One undo. The JSON shape does not change.
    fn simple_rename_scene(&mut self, raw: &str) {
        if self.playing || !self.is_editable_set_start() {
            return;
        }
        let Some(current) = self.document.sets.get(self.current_set) else {
            return;
        };
        let cleaned = simple_scene_name_text(raw);
        let next_name = if cleaned.is_empty() {
            self.simple_default_scene_name(self.current_set)
        } else {
            cleaned
        };
        if current.name == next_name {
            return;
        }
        let mut next = self.document.clone();
        next.sets[self.current_set].name = next_name;
        let revision = self.history.revision();
        self.execute_edit(
            Edit::ReplaceDocument {
                document: Box::new(next),
            },
            i18n::registered(self.locale, "simple-mode.205"),
        );
        self.simple_mark(revision, "simple-mode.203");
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
            self.simple_mode.finished_playback = false;
            self.simple_mode.hold_count = false;
            self.navigate_to_set(index);
        }
    }

    fn simple_cue(&mut self, ui: &mut egui::Ui) {
        if self.playing {
            let total = self
                .document
                .sets
                .get(self.current_set)
                .map(|set| set.counts)
                .unwrap_or(1);
            let now = (self.count_position.round() as i32).clamp(0, i32::from(total));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(
                    egui::RichText::new(now.to_string())
                        .size(28.0)
                        .strong()
                        .color(SIMPLE_BLUE),
                );
                ui.label(
                    egui::RichText::new(format!("/ {total}"))
                        .size(18.0)
                        .color(super::app_theme::SECONDARY_TEXT),
                );
                ui.label(
                    egui::RichText::new(i18n::registered(self.locale, "simple-mode.115"))
                        .size(16.0)
                        .color(super::app_theme::SECONDARY_TEXT),
                );
            });
            ui.label(
                egui::RichText::new(i18n::registered(self.locale, "simple-mode.102"))
                    .size(14.0)
                    .color(super::app_theme::SECONDARY_TEXT),
            );
            return;
        }
        if self.simple_holding_a_count() {
            ui.label(
                egui::RichText::new(i18n::registered(self.locale, "simple-mode.136"))
                    .size(18.0)
                    .strong()
                    .color(SIMPLE_INK),
            );
            return;
        }
        let message = if self.simple_offers_replay() {
            i18n::registered(self.locale, "simple-mode.123")
        } else {
            match self.simple_guide() {
                SimpleGuide::Place => i18n::registered(self.locale, "simple-mode.074"),
                SimpleGuide::NextSet => i18n::registered(self.locale, "simple-mode.075"),
                SimpleGuide::Move => i18n::registered(self.locale, "simple-mode.076"),
                SimpleGuide::Play => i18n::registered(self.locale, "simple-mode.077"),
            }
        };
        ui.label(
            egui::RichText::new(message)
                .size(22.0)
                .strong()
                .color(SIMPLE_INK),
        );
    }

    fn simple_step_counts(&mut self, delta: i32) {
        if !self.is_editable_set_start() {
            return;
        }
        let Some(set) = self.document.sets.get(self.current_set) else {
            return;
        };
        let next = (i32::from(set.counts) + delta).clamp(1, 256) as u16;
        let revision = self.history.revision();
        self.commit_transition_counts(self.current_set, next);
        self.simple_mark(revision, "simple-mode.191");
    }

    fn simple_zoom_by(&mut self, factor: f32) {
        let next = self.field_viewport.zoom * factor;
        self.field_viewport.set_zoom(next);
    }

    fn simple_beat_row(&mut self, ui: &mut egui::Ui) {
        let counts = self
            .document
            .sets
            .get(self.current_set)
            .map(|set| set.counts)
            .unwrap_or(16);
        let editable = self.is_editable_set_start();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if ui
                .add_enabled(editable, egui::Button::new("−").small())
                .on_hover_text(i18n::registered(self.locale, "simple-mode.113"))
                .clicked()
            {
                self.simple_step_counts(-8);
            }
            ui.label(
                egui::RichText::new(format!(
                    "{counts}{}",
                    i18n::registered(self.locale, "simple-mode.100")
                ))
                .size(15.0)
                .strong()
                .color(SIMPLE_INK),
            );
            if ui
                .add_enabled(editable, egui::Button::new("＋").small())
                .on_hover_text(i18n::registered(self.locale, "simple-mode.114"))
                .clicked()
            {
                self.simple_step_counts(8);
            }
            ui.add_space(8.0);
            if ui
                .small_button(i18n::registered(self.locale, "simple-mode.112"))
                .clicked()
            {
                self.simple_zoom_by(0.8);
            }
            if ui
                .small_button(i18n::registered(self.locale, "simple-mode.111"))
                .clicked()
            {
                self.simple_zoom_by(1.25);
            }
            if !self.playing
                && !self.simple_places_on_empty_tap()
                && ui
                    .small_button(i18n::registered(self.locale, "simple-mode.116"))
                    .clicked()
            {
                self.simple_add_person();
            }
            if !self.simple_view_fitted()
                && ui
                    .small_button(i18n::registered(self.locale, "simple-mode.104"))
                    .clicked()
            {
                self.field_viewport.reset(&self.document.grid);
            }
            if self.document.sets.len() > 1 {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(
                            editable,
                            egui::Button::new(i18n::registered(self.locale, "simple-mode.101"))
                                .small(),
                        )
                        .clicked()
                    {
                        let revision = self.history.revision();
                        self.delete_current_set();
                        self.simple_mark(revision, "simple-mode.101");
                    }
                });
            }
        });
    }

    fn simple_holding_a_count(&self) -> bool {
        self.simple_mode.hold_count && self.count_position > 0.05
    }

    fn simple_step_limits(&self) -> (bool, bool) {
        let total = self.document.timeline_counts();
        let current = self
            .document
            .global_count(self.current_set, self.count_position)
            .round()
            .clamp(0.0, total as f32) as u32;
        (current > 0, current < total)
    }

    /// One count forward or back, without writing the picture. Editing stays
    /// locked until the count is a scene's arrival again.
    fn simple_step_beat(&mut self, delta: i32) {
        if self.playing || delta == 0 || !self.show_has_motion() {
            return;
        }
        let total = self.document.timeline_counts();
        let current = self
            .document
            .global_count(self.current_set, self.count_position)
            .round()
            .clamp(0.0, total as f32) as u32;
        let next = if let Ok(steps) = u32::try_from(delta) {
            current.saturating_add(steps).min(total)
        } else {
            current.saturating_sub(delta.unsigned_abs())
        };
        if next == current {
            return;
        }
        self.simple_mode.finished_playback = false;
        self.navigate_to_global_count(next);
        self.simple_mode.hold_count = self.count_position > 0.05;
    }

    fn simple_is_slow(&self) -> bool {
        (self.speed - 0.5).abs() < 0.01
    }

    fn simple_toggle_slow(&mut self) {
        self.speed = if self.simple_is_slow() { 1.0 } else { 0.5 };
    }

    fn simple_everyone_selected(&self) -> bool {
        !self.document.performers.is_empty()
            && self.selected.len() == self.document.performers.len()
    }

    fn simple_toggle_everyone(&mut self) {
        if self.document.performers.len() < 2 {
            return;
        }
        if self.simple_everyone_selected() {
            self.replace_selection(BTreeSet::new());
        } else {
            self.replace_selection((0..self.document.performers.len()).collect());
        }
    }

    /// Uses the chosen people, or everyone when fewer than two are chosen.
    /// Refuses a mid-count picture so a reshape cannot bake a preview.
    fn simple_arrange_targets(&mut self) -> bool {
        if !self.is_editable_set_start() || self.document.performers.len() < 2 {
            return false;
        }
        if self.selected.len() < 2 {
            self.replace_selection((0..self.document.performers.len()).collect());
        }
        self.selected.len() >= 2
    }

    fn simple_commit_arrangement(&mut self, points: Vec<Point>, note: &'static str) {
        let revision = self.history.revision();
        self.commit_layout(points);
        if self.history.revision() != revision {
            self.simple_mark(revision, note);
            self.bump_simple_motion(ChromeMotion::Move);
        }
    }

    /// Lines the chosen people up, left to right in the order they were
    /// placed. With fewer than two chosen, everyone lines up.
    fn simple_line_up(&mut self) {
        if !self.simple_arrange_targets() {
            return;
        }
        let count = self.selected.len();
        let grid = self.document.grid.clone();
        let y = self
            .selected_points()
            .iter()
            .map(|point| point.y)
            .sum::<f32>()
            / count as f32;
        let step = grid.horizontal_units / f32::from(grid.horizontal_steps.max(1));
        let ideal = step * 2.0;
        let span = (ideal * count.saturating_sub(1) as f32).min(grid.width * 0.72);
        let gap = span / (count - 1) as f32;
        let left = ((grid.width - span) * 0.5).max(0.0);
        let points = (0..count)
            .map(|index| Point {
                x: left + gap * index as f32,
                y,
            })
            .collect();
        self.simple_commit_arrangement(points, "simple-mode.132");
    }

    /// A file facing the audience: the first chosen person stands closest
    /// to the front sideline.
    fn simple_file_up(&mut self) {
        if !self.simple_arrange_targets() {
            return;
        }
        let points = simple_file_points(&self.document.grid, &self.selected_points());
        self.simple_commit_arrangement(points, "simple-mode.148");
    }

    /// A block with the front row toward the audience. With fewer than two
    /// chosen, everyone joins the block.
    fn simple_block_up(&mut self) {
        if !self.simple_arrange_targets() {
            return;
        }
        let points = simple_block_points(&self.document.grid, &self.selected_points());
        self.simple_commit_arrangement(points, "simple-mode.216");
    }

    /// A circle with the first chosen person on the audience side.
    fn simple_circle_up(&mut self) {
        if !self.simple_arrange_targets() {
            return;
        }
        let points = simple_circle_points(&self.document.grid, &self.selected_points());
        self.simple_commit_arrangement(points, "simple-mode.150");
    }

    /// Swaps left and right around the group's own center.
    fn simple_swap_sides(&mut self) {
        if !self.simple_arrange_targets() {
            return;
        }
        let points = drill_core::editing::flip_horizontal(&self.selected_points());
        self.simple_commit_arrangement(points, "simple-mode.152");
    }

    /// Swaps front and back around the group's own center.
    fn simple_swap_ends(&mut self) {
        if !self.simple_arrange_targets() {
            return;
        }
        let points = drill_core::editing::flip_vertical(&self.selected_points());
        self.simple_commit_arrangement(points, "simple-mode.193");
    }

    /// A quarter turn to the right, as the audience sees the field.
    fn simple_turn_right(&mut self) {
        if !self.simple_arrange_targets() {
            return;
        }
        let turned = drill_core::editing::rotate_about_centroid(
            &self.selected_points(),
            -std::f32::consts::FRAC_PI_2,
        );
        let points = simple_fit_points(&self.document.grid, &turned);
        self.simple_commit_arrangement(points, "simple-mode.195");
    }

    /// A curve with the middle of the group toward the audience.
    fn simple_arc_up(&mut self) {
        if !self.simple_arrange_targets() {
            return;
        }
        let points = simple_arc_points(&self.document.grid, &self.selected_points());
        self.simple_commit_arrangement(points, "simple-mode.197");
    }

    /// Opens or closes the gaps by about one step, around the group's center.
    fn simple_change_spacing(&mut self, outward: bool) {
        if !self.simple_arrange_targets() {
            return;
        }
        let points = simple_spacing_points(&self.document.grid, &self.selected_points(), outward);
        let note = if outward {
            "simple-mode.199"
        } else {
            "simple-mode.201"
        };
        self.simple_commit_arrangement(points, note);
    }

    fn simple_add_scene(&mut self) {
        if !self.is_editable_set_start()
            || self.document.performers.is_empty()
            || self.document.sets.len() < 2
        {
            return;
        }
        self.simple_mode.finished_playback = false;
        self.simple_mode.hold_count = false;
        let revision = self.history.revision();
        self.duplicate_current_set();
        self.simple_mark(revision, "simple-mode.129");
    }

    fn simple_rehearsal_row(&mut self, ui: &mut egui::Ui) {
        if !self.show_has_motion() {
            return;
        }
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::new(8.0, 8.0);
            if !self.playing {
                let (back, forward) = self.simple_step_limits();
                if simple_choice_button(
                    ui,
                    i18n::registered(self.locale, "simple-mode.133"),
                    back,
                    false,
                )
                .on_hover_text(i18n::registered(self.locale, "simple-mode.142"))
                .clicked()
                {
                    self.simple_step_beat(-1);
                }
                let counts = self
                    .document
                    .sets
                    .get(self.current_set)
                    .map(|set| set.counts)
                    .unwrap_or(1);
                let now = (self.count_position.round() as i32).clamp(0, i32::from(counts));
                ui.label(
                    egui::RichText::new(format!(
                        "{now} / {counts}{}",
                        i18n::registered(self.locale, "simple-mode.138")
                    ))
                    .size(16.0)
                    .strong()
                    .color(SIMPLE_BLUE),
                );
                if simple_choice_button(
                    ui,
                    i18n::registered(self.locale, "simple-mode.134"),
                    forward,
                    false,
                )
                .on_hover_text(i18n::registered(self.locale, "simple-mode.143"))
                .clicked()
                {
                    self.simple_step_beat(1);
                }
                if self.simple_holding_a_count()
                    && simple_choice_button(
                        ui,
                        i18n::registered(self.locale, "simple-mode.135"),
                        true,
                        false,
                    )
                    .on_hover_text(i18n::registered(self.locale, "simple-mode.144"))
                    .clicked()
                {
                    self.simple_mode.hold_count = false;
                    self.navigate_to_set(self.current_set);
                }
            }
            if simple_choice_button(
                ui,
                i18n::registered(self.locale, "simple-mode.137"),
                true,
                self.simple_is_slow(),
            )
            .on_hover_text(i18n::registered(self.locale, "simple-mode.145"))
            .clicked()
            {
                self.simple_toggle_slow();
            }
        });
    }

    fn simple_arrange_row(&mut self, ui: &mut egui::Ui) {
        if self.playing || self.document.performers.is_empty() {
            return;
        }
        let editable = self.is_editable_set_start();
        let can_shape = self.document.performers.len() >= 2;
        if !can_shape {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::new(8.0, 8.0);
                self.simple_memo_button(ui);
            });
            return;
        }
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::new(8.0, 8.0);
            if simple_choice_button(
                ui,
                i18n::registered(self.locale, "simple-mode.130"),
                true,
                self.simple_everyone_selected(),
            )
            .on_hover_text(i18n::registered(self.locale, "simple-mode.140"))
            .clicked()
            {
                self.simple_toggle_everyone();
            }
            if simple_choice_button(
                ui,
                i18n::registered(self.locale, "simple-mode.132"),
                editable,
                false,
            )
            .on_hover_text(i18n::registered(self.locale, "simple-mode.141"))
            .clicked()
            {
                self.simple_line_up();
            }
            if simple_choice_button(
                ui,
                i18n::registered(self.locale, "simple-mode.148"),
                editable,
                false,
            )
            .on_hover_text(i18n::registered(self.locale, "simple-mode.149"))
            .clicked()
            {
                self.simple_file_up();
            }
            if simple_choice_button(
                ui,
                i18n::registered(self.locale, "simple-mode.150"),
                editable,
                false,
            )
            .on_hover_text(i18n::registered(self.locale, "simple-mode.151"))
            .clicked()
            {
                self.simple_circle_up();
            }
            if simple_choice_button(
                ui,
                i18n::registered(self.locale, "simple-mode.216"),
                editable,
                false,
            )
            .on_hover_text(i18n::registered(self.locale, "simple-mode.217"))
            .clicked()
            {
                self.simple_block_up();
            }
            if simple_choice_button(
                ui,
                i18n::registered(self.locale, "simple-mode.152"),
                editable,
                false,
            )
            .on_hover_text(i18n::registered(self.locale, "simple-mode.153"))
            .clicked()
            {
                self.simple_swap_sides();
            }
            if simple_choice_button(
                ui,
                i18n::registered(self.locale, "simple-mode.193"),
                editable,
                false,
            )
            .on_hover_text(i18n::registered(self.locale, "simple-mode.194"))
            .clicked()
            {
                self.simple_swap_ends();
            }
            if simple_choice_button(
                ui,
                i18n::registered(self.locale, "simple-mode.195"),
                editable,
                false,
            )
            .on_hover_text(i18n::registered(self.locale, "simple-mode.196"))
            .clicked()
            {
                self.simple_turn_right();
            }
            if simple_choice_button(
                ui,
                i18n::registered(self.locale, "simple-mode.197"),
                editable,
                false,
            )
            .on_hover_text(i18n::registered(self.locale, "simple-mode.198"))
            .clicked()
            {
                self.simple_arc_up();
            }
            if simple_choice_button(
                ui,
                i18n::registered(self.locale, "simple-mode.199"),
                editable,
                false,
            )
            .on_hover_text(i18n::registered(self.locale, "simple-mode.200"))
            .clicked()
            {
                self.simple_change_spacing(true);
            }
            if simple_choice_button(
                ui,
                i18n::registered(self.locale, "simple-mode.201"),
                editable,
                false,
            )
            .on_hover_text(i18n::registered(self.locale, "simple-mode.202"))
            .clicked()
            {
                self.simple_change_spacing(false);
            }
            if self.document.sets.len() >= 2
                && simple_choice_button(
                    ui,
                    i18n::registered(self.locale, "simple-mode.129"),
                    editable,
                    false,
                )
                .on_hover_text(i18n::registered(self.locale, "simple-mode.139"))
                .clicked()
            {
                self.simple_add_scene();
            }
            if simple_choice_button(
                ui,
                i18n::registered(self.locale, "simple-mode.175"),
                editable,
                false,
            )
            .on_hover_text(i18n::registered(self.locale, "simple-mode.176"))
            .clicked()
            {
                self.simple_renumber();
            }
            self.simple_memo_button(ui);
        });
        if self.selected.len() >= 2 && editable {
            ui.label(
                egui::RichText::new(i18n::registered(self.locale, "simple-mode.131"))
                    .size(14.0)
                    .color(super::app_theme::SECONDARY_TEXT),
            );
        }
    }

    /// One grid step. Forward is toward the audience (decreasing field y).
    fn simple_nudge(&mut self, horizontal: i32, vertical: i32) {
        if self.playing
            || !self.is_editable_set_start()
            || self.selected.is_empty()
            || (horizontal == 0 && vertical == 0)
        {
            return;
        }
        let revision = self.history.revision();
        self.nudge_selected(horizontal, vertical);
        if self.history.revision() != revision {
            self.simple_mark(revision, "simple-mode.182");
            self.bump_simple_motion(ChromeMotion::Move);
        }
    }

    /// Puts another person two steps to the right of the rightmost chosen
    /// person, or of the last person when nobody is chosen. Falls back to the
    /// other sides when that spot is off the field or already taken.
    fn simple_add_beside(&mut self) {
        if self.playing || !self.is_editable_set_start() || self.document.performers.is_empty() {
            return;
        }
        let Some(positions) = self
            .document
            .sets
            .get(self.current_set)
            .map(|set| set.positions.clone())
        else {
            return;
        };
        if positions.is_empty() {
            return;
        }
        let index = self
            .selected
            .iter()
            .copied()
            .filter_map(|index| positions.get(index).map(|point| (index, point.x)))
            .max_by(|(_, left), (_, right)| left.total_cmp(right))
            .map(|(index, _)| index)
            .unwrap_or(positions.len() - 1);
        let origin = positions[index];
        let grid = self.document.grid.clone();
        let dx = grid.horizontal_units / f32::from(grid.horizontal_steps.max(1));
        let dy = grid.vertical_units / f32::from(grid.vertical_steps.max(1));
        let candidates = [
            Point {
                x: origin.x + dx * 2.0,
                y: origin.y,
            },
            Point {
                x: origin.x - dx * 2.0,
                y: origin.y,
            },
            Point {
                x: origin.x,
                y: origin.y + dy * 2.0,
            },
            Point {
                x: origin.x,
                y: origin.y - dy * 2.0,
            },
        ];
        let spot = candidates
            .into_iter()
            .find(|point| {
                (0.0..=grid.max_x()).contains(&point.x)
                    && (0.0..=grid.max_y()).contains(&point.y)
                    && !self.positions_overlap(*point)
            })
            .unwrap_or(candidates[0]);
        let before_len = self.document.performers.len();
        let revision = self.history.revision();
        self.place_performer_at(spot, true);
        if self.document.performers.len() > before_len {
            self.simple_mark(revision, "simple-mode.159");
            self.bump_simple_motion(ChromeMotion::Place);
        }
    }

    fn simple_can_clear_move(&self) -> bool {
        let Some(previous_index) = self.current_set.checked_sub(1) else {
            return false;
        };
        let Some(current) = self.document.sets.get(self.current_set) else {
            return false;
        };
        let Some(previous) = self.document.sets.get(previous_index) else {
            return false;
        };
        !self.document.performers.is_empty() && current.positions != previous.positions
    }

    /// Puts this scene back on the previous picture, as one undo.
    fn simple_clear_move(&mut self) {
        if self.playing || !self.is_editable_set_start() || !self.simple_can_clear_move() {
            return;
        }
        let Some(previous) = self
            .document
            .sets
            .get(self.current_set - 1)
            .map(|set| set.positions.clone())
        else {
            return;
        };
        if previous.len() != self.document.performers.len() {
            return;
        }
        self.replace_selection((0..previous.len()).collect());
        let revision = self.history.revision();
        self.commit_layout(previous);
        if self.history.revision() != revision {
            self.simple_mark(revision, "simple-mode.164");
            self.bump_simple_motion(ChromeMotion::Move);
        }
    }

    fn simple_touch_row(&mut self, ui: &mut egui::Ui) {
        if self.playing {
            return;
        }
        let editable = self.is_editable_set_start();
        let nudge = editable && !self.selected.is_empty();
        let beside = editable && !self.document.performers.is_empty();
        let clear = editable && self.simple_can_clear_move();
        let swap = editable && self.selected.len() == 2;
        let restore = self.simple_can_restore_selected();
        if !nudge && !beside && !clear && !swap && !restore {
            return;
        }
        ui.add_space(4.0);
        if nudge
            && self.selected.len() == 1
            && let Some(place) = self.simple_place_caption()
        {
            ui.label(
                egui::RichText::new(place)
                    .size(16.0)
                    .strong()
                    .color(SIMPLE_BLUE),
            );
        }
        if nudge {
            ui.label(
                egui::RichText::new(i18n::registered(self.locale, "simple-mode.158"))
                    .size(14.0)
                    .color(super::app_theme::SECONDARY_TEXT),
            );
            ui.label(
                egui::RichText::new(i18n::registered(self.locale, "simple-mode.181"))
                    .size(14.0)
                    .color(super::app_theme::SECONDARY_TEXT),
            );
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::new(8.0, 8.0);
            if nudge {
                let steps = [
                    ("simple-mode.154", 0, -1),
                    ("simple-mode.155", 0, 1),
                    ("simple-mode.156", -1, 0),
                    ("simple-mode.157", 1, 0),
                ];
                for (id, horizontal, vertical) in steps {
                    if simple_choice_button(ui, i18n::registered(self.locale, id), true, false)
                        .clicked()
                    {
                        self.simple_nudge(horizontal, vertical);
                    }
                }
            }
            if beside
                && simple_choice_button(
                    ui,
                    i18n::registered(self.locale, "simple-mode.159"),
                    true,
                    false,
                )
                .on_hover_text(i18n::registered(self.locale, "simple-mode.160"))
                .clicked()
            {
                self.simple_add_beside();
            }
            if swap
                && simple_choice_button(
                    ui,
                    i18n::registered(self.locale, "simple-mode.211"),
                    true,
                    false,
                )
                .on_hover_text(i18n::registered(self.locale, "simple-mode.212"))
                .clicked()
            {
                self.simple_swap_pair();
            }
            if restore {
                let label = if self.selected.len() == 1 {
                    "simple-mode.213"
                } else {
                    "simple-mode.214"
                };
                if simple_choice_button(ui, i18n::registered(self.locale, label), true, false)
                    .on_hover_text(i18n::registered(self.locale, "simple-mode.215"))
                    .clicked()
                {
                    self.simple_restore_selected();
                }
            }
            if clear
                && simple_choice_button(
                    ui,
                    i18n::registered(self.locale, "simple-mode.164"),
                    true,
                    false,
                )
                .on_hover_text(i18n::registered(self.locale, "simple-mode.165"))
                .clicked()
            {
                self.simple_clear_move();
            }
        });
    }

    /// Trades the two chosen people's places. One undo.
    fn simple_swap_pair(&mut self) {
        if self.playing || !self.is_editable_set_start() || self.selected.len() != 2 {
            return;
        }
        let points = self.selected_points();
        if points.len() != 2 {
            return;
        }
        self.simple_commit_arrangement(vec![points[1], points[0]], "simple-mode.211");
    }

    /// Some chosen people, but not everyone, stand somewhere else than in
    /// the previous scene.
    fn simple_can_restore_selected(&self) -> bool {
        if !self.is_editable_set_start()
            || self.selected.is_empty()
            || self.simple_everyone_selected()
        {
            return false;
        }
        let Some(previous_index) = self.current_set.checked_sub(1) else {
            return false;
        };
        let Some(current) = self.document.sets.get(self.current_set) else {
            return false;
        };
        let Some(previous) = self.document.sets.get(previous_index) else {
            return false;
        };
        current.positions.len() == previous.positions.len()
            && self
                .selected
                .iter()
                .any(|&index| current.positions.get(index) != previous.positions.get(index))
    }

    /// Puts only the chosen people back on their previous-scene spots.
    fn simple_restore_selected(&mut self) {
        if !self.simple_can_restore_selected() {
            return;
        }
        let Some(previous_index) = self.current_set.checked_sub(1) else {
            return;
        };
        let Some(previous) = self
            .document
            .sets
            .get(previous_index)
            .map(|set| set.positions.clone())
        else {
            return;
        };
        let points: Vec<Point> = self
            .selected
            .iter()
            .filter_map(|&index| previous.get(index).copied())
            .collect();
        if points.len() != self.selected.len() {
            return;
        }
        let note = if self.selected.len() == 1 {
            "simple-mode.213"
        } else {
            "simple-mode.214"
        };
        let revision = self.history.revision();
        self.commit_layout(points);
        if self.history.revision() != revision {
            self.simple_mark(revision, note);
            self.bump_simple_motion(ChromeMotion::Move);
        }
    }

    /// Where the one chosen person stands, in steps from the center and the
    /// front. Reading this never writes the document.
    fn simple_place_caption(&self) -> Option<String> {
        if self.selected.len() != 1 {
            return None;
        }
        let index = *self.selected.iter().next()?;
        let point = self
            .document
            .sets
            .get(self.current_set)?
            .positions
            .get(index)
            .copied()?;
        simple_place_line(&self.document.grid, point, self.locale)
    }

    fn simple_mark(&mut self, before: drill_core::Revision, note: &'static str) {
        if self.history.revision() != before {
            self.simple_mode.pending_note = Some(note);
        }
    }

    /// Names the edit that just landed, and moves that name onto redo when
    /// the cursor walks back. A history reset (opening another show) drops
    /// names it can no longer match.
    fn simple_sync_history_notes(&mut self) {
        let cursor = self.history.cursor();
        let redo = self.history.redo_len();
        let (Some(seen_cursor), Some(seen_redo)) =
            (self.simple_mode.seen_cursor, self.simple_mode.seen_redo)
        else {
            self.simple_mode.seen_cursor = Some(cursor);
            self.simple_mode.seen_redo = Some(redo);
            self.simple_mode.pending_note = None;
            return;
        };
        let pending = self.simple_mode.pending_note.take();
        if cursor > seen_cursor {
            let steps = cursor - seen_cursor;
            let redo_drop = seen_redo.saturating_sub(redo);
            let is_redo = pending.is_none() && redo_drop > 0 && steps == redo_drop;
            if is_redo {
                for _ in 0..steps {
                    if let Some(note) = self.simple_mode.redo_notes.pop() {
                        self.simple_mode.undo_notes.push(note);
                    }
                }
            } else {
                self.simple_mode.redo_notes.clear();
                let note = pending.unwrap_or("simple-mode.169");
                for _ in 0..steps {
                    self.simple_mode.undo_notes.push(note);
                }
            }
        } else if cursor < seen_cursor {
            let steps = seen_cursor - cursor;
            if steps > self.simple_mode.undo_notes.len() {
                self.simple_mode.undo_notes.clear();
                self.simple_mode.redo_notes.clear();
            } else {
                for _ in 0..steps {
                    if let Some(note) = self.simple_mode.undo_notes.pop() {
                        self.simple_mode.redo_notes.push(note);
                    }
                }
            }
        }
        self.simple_mode.seen_cursor = Some(cursor);
        self.simple_mode.seen_redo = Some(redo);
    }

    fn simple_history_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if self.history.can_undo()
            && let Some(id) = self.simple_mode.undo_notes.last().copied()
        {
            let name = i18n::registered(self.locale, id);
            lines.push(i18n::registered(self.locale, "simple-mode.170").replace("{0}", name));
        }
        if self.history.can_redo()
            && let Some(id) = self.simple_mode.redo_notes.last().copied()
        {
            let name = i18n::registered(self.locale, id);
            lines.push(i18n::registered(self.locale, "simple-mode.171").replace("{0}", name));
        }
        lines
    }

    /// Arrow keys follow the screen: up is away from the audience. One step,
    /// same as the 前 / 後ろ / 左 / 右 buttons.
    fn simple_try_arrow_nudge(&mut self, ui: &mut egui::Ui) {
        if self.playing
            || self.simple_mode.naming
            || self.simple_mode.memo_open
            || self.show_recent_projects
            || self.selected.is_empty()
            || !self.is_editable_set_start()
            || ui.ctx().egui_wants_keyboard_input()
        {
            return;
        }
        let step = ui.input_mut(|input| {
            if input.modifiers.command
                || input.modifiers.ctrl
                || input.modifiers.alt
                || input.modifiers.shift
            {
                return None;
            }
            for key in [
                egui::Key::ArrowLeft,
                egui::Key::ArrowRight,
                egui::Key::ArrowUp,
                egui::Key::ArrowDown,
            ] {
                if input.consume_key(egui::Modifiers::NONE, key) {
                    return simple_arrow_step(key);
                }
            }
            None
        });
        if let Some((horizontal, vertical)) = step {
            self.simple_nudge(horizontal, vertical);
        }
    }

    fn simple_caution_row(&mut self, ui: &mut egui::Ui) {
        let set_index = self.current_set;
        let caution = simple_caution(&self.document, set_index, &mut self.clinic_scratch);
        if caution.collisions.is_empty() && caution.long_stride.is_none() {
            return;
        }
        ui.add_space(4.0);
        if !caution.collisions.is_empty() {
            let button_id = if caution.collisions.len() > 2 {
                "simple-mode.186"
            } else {
                "simple-mode.173"
            };
            let people = caution.collisions.clone();
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::new(8.0, 8.0);
                ui.label(
                    egui::RichText::new(i18n::registered(self.locale, "simple-mode.172"))
                        .size(15.0)
                        .strong()
                        .color(SIMPLE_INK),
                );
                if simple_choice_button(ui, i18n::registered(self.locale, button_id), true, false)
                    .on_hover_text(i18n::registered(self.locale, "simple-mode.185"))
                    .clicked()
                {
                    self.replace_selection(people.into_iter().collect());
                }
            });
        }
        if let Some(stride) = caution.long_stride {
            let label = self
                .document
                .performers
                .get(stride.index)
                .map(|performer| performer.label.as_str())
                .unwrap_or("");
            let text = i18n::registered(self.locale, "simple-mode.174")
                .replace("{0}", label)
                .replace("{1}", &stride.counts.to_string())
                .replace("{2}", &format_step_count(stride.steps));
            ui.label(
                egui::RichText::new(text)
                    .size(15.0)
                    .strong()
                    .color(SIMPLE_INK),
            );
        }
    }

    fn simple_memo_button(&mut self, ui: &mut egui::Ui) {
        if simple_choice_button(
            ui,
            i18n::registered(self.locale, "simple-mode.177"),
            true,
            self.simple_mode.memo_open,
        )
        .on_hover_text(i18n::registered(self.locale, "simple-mode.178"))
        .clicked()
        {
            self.simple_mode.memo_open = !self.simple_mode.memo_open;
            if !self.simple_mode.memo_open {
                self.simple_mode.memo_copied = false;
            }
        }
    }

    fn simple_memo_sheet(&mut self, ctx: &egui::Context) {
        if !self.simple_mode.memo_open {
            return;
        }
        let memo = simple_scene_memo(&self.document, self.current_set, self.locale);
        let mut open = true;
        let mut copy = false;
        let copied = self.simple_mode.memo_copied;
        egui::Window::new(i18n::registered(self.locale, "simple-mode.192"))
            .id(egui::Id::new("simple-scene-memo"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(300.0);
                ui.set_max_width(440.0);
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new(&memo).size(16.0).color(SIMPLE_INK));
                    });
                ui.add_space(12.0);
                let copy_id = if copied {
                    "simple-mode.180"
                } else {
                    "simple-mode.179"
                };
                let button = egui::Button::new(
                    egui::RichText::new(i18n::registered(self.locale, copy_id))
                        .size(16.0)
                        .color(Color32::WHITE),
                )
                .fill(SIMPLE_BLUE)
                .corner_radius(18.0);
                if ui.add_sized([160.0, 40.0], button).clicked() {
                    copy = true;
                }
            });
        if copy {
            ctx.copy_text(memo);
            self.simple_mode.memo_copied = true;
        }
        if !open {
            self.simple_mode.memo_open = false;
            self.simple_mode.memo_copied = false;
        }
    }

    /// Numbers everyone from the audience's left, then front to back.
    /// A name after the number is kept. One undo.
    fn simple_renumber(&mut self) {
        if self.playing || !self.is_editable_set_start() || self.document.performers.len() < 2 {
            return;
        }
        let Some(points) = self
            .document
            .sets
            .get(self.current_set)
            .map(|set| set.positions.clone())
        else {
            return;
        };
        if points.len() != self.document.performers.len() {
            return;
        }
        let order = simple_renumber_order(&points);
        let mut next = self.document.clone();
        let mut changed = false;
        for (rank, index) in order.into_iter().enumerate() {
            let Some(performer) = next.performers.get_mut(index) else {
                continue;
            };
            let label =
                join_performer_label(&(rank + 1).to_string(), &simple_kept_name(&performer.label));
            if performer.label != label {
                performer.label = label;
                changed = true;
            }
        }
        if !changed {
            return;
        }
        let revision = self.history.revision();
        self.execute_edit(
            Edit::ReplaceDocument {
                document: Box::new(next),
            },
            i18n::registered(self.locale, "simple-mode.184"),
        );
        self.simple_mark(revision, "simple-mode.175");
    }

    /// Screenshot picture: two people cross, and one walks farther than the
    /// counts allow. Reading the warnings does not write the document.
    fn simple_seed_hint_picture(&mut self) {
        while self.document.performers.len() < 3 {
            let n = self.document.performers.len() as f32;
            self.place_performer_at(
                Point {
                    x: 12.0 + n * 4.0,
                    y: 16.0,
                },
                true,
            );
        }
        self.replace_selection(std::iter::once(0).collect());
        self.simple_rename_selected("山田");
        let step = self.document.grid.horizontal_units
            / f32::from(self.document.grid.horizontal_steps.max(1));
        let y = 16.0;
        let left = 16.0;
        let set_id = self.document.sets[0].id;
        let ids: Vec<_> = self
            .document
            .performers
            .iter()
            .take(3)
            .map(|performer| performer.id)
            .collect();
        let _ = self.execute_edit(
            Edit::MovePerformers {
                set_id,
                performer_ids: ids.clone(),
                positions: vec![
                    Point {
                        x: left + step * 8.0,
                        y,
                    },
                    Point { x: left, y },
                    Point {
                        x: left + step * 4.0,
                        y,
                    },
                ],
            },
            "qa",
        );
        self.duplicate_current_set();
        let set_id = self.document.sets[1].id;
        let start = self.document.sets[0].positions.clone();
        let _ = self.execute_edit(
            Edit::MovePerformers {
                set_id,
                performer_ids: ids,
                positions: vec![
                    Point {
                        x: start[0].x + step * 20.0,
                        y,
                    },
                    start[2],
                    start[1],
                ],
            },
            "qa",
        );
        let _ = self.commit_transition_counts(0, 8);
        self.navigate_to_set(1);
        self.nav_glide.settle();
        self.replace_selection(std::iter::once(0).collect());
        self.simple_mode.undo_notes.push("simple-mode.183");
        self.simple_mode.seen_cursor = Some(self.history.cursor());
        self.simple_mode.seen_redo = Some(self.history.redo_len());
    }

    fn simple_refresh_held_picture(&mut self) {
        if self.playing || self.nav_glide.position().is_some() || !self.simple_holding_a_count() {
            return;
        }
        let counts = self
            .document
            .sets
            .get(self.current_set)
            .map(|set| set.counts.max(1))
            .unwrap_or(1);
        let progress = (self.count_position / f32::from(counts)).clamp(0.0, 1.0);
        self.document
            .positions_at(self.current_set, progress, &mut self.frame_positions);
    }

    fn simple_field_card(&mut self, ui: &mut egui::Ui, height: f32) {
        self.simple_refresh_held_picture();
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
        if self.simple_name_row_visible() {
            self.simple_name_row(&mut child);
            child.add_space(8.0);
        }
        if self.simple_can_remove() {
            self.simple_remove_bar(&mut child);
            child.add_space(8.0);
        }
        let history_lines = self.simple_history_lines();
        if !history_lines.is_empty() {
            for line in &history_lines {
                child.label(
                    egui::RichText::new(line)
                        .size(14.0)
                        .color(super::app_theme::SECONDARY_TEXT),
                );
            }
            child.add_space(6.0);
        }
        self.simple_dock_actions(&mut child);
        if self.simple_primary_action().is_some() {
            child.add_space(10.0);
            self.simple_primary(&mut child);
        }
    }

    fn simple_remove_bar(&mut self, ui: &mut egui::Ui) {
        let label = if self.selected.len() == 1 {
            i18n::registered(self.locale, "simple-mode.097")
        } else {
            i18n::registered(self.locale, "simple-mode.098")
        };
        let (rect, response) =
            ui.allocate_exact_size(egui::Vec2::new(ui.available_width(), 44.0), Sense::click());
        let fill = if response.hovered() {
            super::app_theme::ACCENT_SOFT
        } else {
            Color32::WHITE
        };
        ui.painter().rect_filled(rect, rect.height() / 2.0, fill);
        ui.painter().rect_stroke(
            rect,
            rect.height() / 2.0,
            Stroke::new(1.5, SIMPLE_BLUE),
            StrokeKind::Inside,
        );
        paint_phone_icon(
            ui.painter(),
            rect.center() + egui::Vec2::new(-36.0, 0.0),
            PhoneIcon::Trash,
            SIMPLE_BLUE,
        );
        ui.painter().text(
            rect.center() + egui::Vec2::new(8.0, 0.0),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(16.0),
            SIMPLE_BLUE,
        );
        if response.clicked() {
            self.simple_mode.overlap_note = false;
            let note = if self.selected.len() == 1 {
                "simple-mode.097"
            } else {
                "simple-mode.098"
            };
            let revision = self.history.revision();
            self.remove_selected_performers();
            self.simple_mark(revision, note);
        }
    }

    fn simple_dock_actions(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let labels = [
                (
                    "simple-mode.087",
                    "simple-mode.092",
                    PhoneIcon::Undo,
                    DockAction::Undo,
                ),
                (
                    "simple-mode.090",
                    "simple-mode.091",
                    PhoneIcon::Redo,
                    DockAction::Redo,
                ),
                (
                    "simple-mode.081",
                    "simple-mode.093",
                    PhoneIcon::Save,
                    DockAction::Save,
                ),
                (
                    "simple-mode.080",
                    "simple-mode.094",
                    PhoneIcon::Open,
                    DockAction::Open,
                ),
                (
                    "simple-mode.079",
                    "simple-mode.095",
                    PhoneIcon::More,
                    DockAction::More,
                ),
            ];
            let count = labels.len() as f32;
            let gaps = 8.0 * (count - 1.0);
            let button_w = ((ui.available_width() - gaps) / count).clamp(64.0, 112.0);
            let row = button_w * count + gaps;
            ui.add_space(((ui.available_width() - row) / 2.0).max(0.0));
            for (id, tip, icon, action) in labels {
                let enabled = match action {
                    DockAction::Undo => self.history.can_undo(),
                    DockAction::Redo => self.history.can_redo(),
                    DockAction::Save | DockAction::Open | DockAction::More => true,
                };
                let label = i18n::registered(self.locale, id);
                let response = self
                    .simple_dock_button(ui, icon, label, button_w, enabled)
                    .on_hover_text(i18n::registered(self.locale, tip));
                if response.clicked() {
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
            DockAction::Redo => {
                self.simple_mode.overlap_note = false;
                self.execute_command(UiCommand::Redo, ui.ctx());
            }
            DockAction::Save => self.simple_begin_save(),
            DockAction::Open => self.simple_open_shows(),
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
        let Some(action) = self.simple_primary_action() else {
            return;
        };
        let (icon, label) = match action {
            SimplePrimary::Next => (
                PhoneIcon::Next,
                i18n::registered(self.locale, "simple-mode.078"),
            ),
            SimplePrimary::Pause => (
                PhoneIcon::Pause,
                i18n::registered(self.locale, "simple-mode.089"),
            ),
            SimplePrimary::Play if self.simple_offers_replay() => (
                PhoneIcon::Play,
                i18n::registered(self.locale, "simple-mode.124"),
            ),
            SimplePrimary::Play => (
                PhoneIcon::Play,
                i18n::registered(self.locale, "simple-mode.088"),
            ),
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
            match action {
                SimplePrimary::Next => {
                    self.simple_mode.finished_playback = false;
                    let revision = self.history.revision();
                    self.duplicate_current_set();
                    self.simple_mark(revision, "simple-mode.078");
                }
                SimplePrimary::Pause => self.toggle_playback(ui.ctx()),
                SimplePrimary::Play => self.simple_start_play(ui.ctx()),
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

    fn simple_places_on_empty_tap(&self) -> bool {
        matches!(
            self.simple_guide(),
            SimpleGuide::Place | SimpleGuide::NextSet
        )
    }

    fn simple_offers_replay(&self) -> bool {
        self.simple_mode.finished_playback
            && !self.playing
            && self.simple_guide() == SimpleGuide::Play
    }

    fn simple_name_row_visible(&self) -> bool {
        !self.playing
            && !self.simple_offers_replay()
            && self.selected.len() == 1
            && self.is_editable_set_start()
    }

    fn simple_travel_caption(&self) -> Option<String> {
        let chosen: Vec<usize> = self.selected.iter().copied().collect();
        let note = simple_travel_note(&self.document, self.current_set, &chosen)?;
        if note.kind == drill_core::continuity::TravelDirection::Hold {
            return Some(i18n::registered(self.locale, "simple-mode.163").to_string());
        }
        let template = if self.selected.len() == 1 {
            i18n::registered(self.locale, "simple-mode.161")
        } else {
            i18n::registered(self.locale, "simple-mode.162")
        };
        let label = self
            .document
            .performers
            .get(note.index)
            .map(|performer| performer.label.as_str())
            .unwrap_or("");
        Some(
            template
                .replace("{0}", note.kind.text(self.locale))
                .replace("{1}", &format_step_count(note.steps))
                .replace("{2}", label),
        )
    }

    fn simple_show_title(&self) -> Option<String> {
        let name = self.current_path.as_ref()?.file_name()?.to_str()?;
        let title = name
            .strip_suffix(".drill.json")
            .or_else(|| name.strip_suffix(".json"))
            .unwrap_or(name);
        (!title.is_empty()).then(|| title.to_string())
    }

    fn simple_status_label(&self) -> Option<(String, bool)> {
        let saved = self
            .simple_mode
            .saved_note_until
            .is_some_and(|until| Instant::now() < until);
        if saved {
            return Some((
                i18n::registered(self.locale, "simple-mode.121").to_string(),
                true,
            ));
        }
        let draft = i18n::registered(self.locale, "simple-mode.096");
        match self.simple_show_title() {
            Some(title) if !self.dirty => Some((title, false)),
            Some(title) => Some((format!("{title} · {draft}"), false)),
            None if self.simple_mode.draft_saved => Some((draft.to_string(), false)),
            None => None,
        }
    }

    fn expire_simple_saved_note(&mut self, ctx: &egui::Context) {
        let Some(until) = self.simple_mode.saved_note_until else {
            return;
        };
        if Instant::now() >= until {
            self.simple_mode.saved_note_until = None;
        } else {
            ctx.request_repaint_after(until.saturating_duration_since(Instant::now()));
        }
    }

    fn simple_shows_dir(&self) -> std::path::PathBuf {
        self.simple_mode
            .shows_dir_override
            .clone()
            .unwrap_or_else(|| super::project_state::app_data_dir().join("shows"))
    }

    fn simple_default_show_name(&self) -> String {
        i18n::registered(self.locale, "simple-mode.122").to_string()
    }

    fn simple_named_save_path(&self, raw: &str) -> std::path::PathBuf {
        let dir = self.simple_shows_dir();
        let stem = simple_show_file_stem(raw);
        let candidate = dir.join(format!("{stem}.drill.json"));
        if self.current_path.as_ref() == Some(&candidate) || !candidate.exists() {
            return candidate;
        }
        for n in 2..50 {
            let next = dir.join(format!("{stem} {n}.drill.json"));
            if self.current_path.as_ref() == Some(&next) || !next.exists() {
                return next;
            }
        }
        candidate
    }

    /// Keeps an unnamed picture in the shows list, then opens an empty field.
    /// A named show is left as it is. The draft becomes the empty field so the
    /// next launch does not bring the previous picture back.
    pub(crate) fn simple_start_fresh(&mut self) -> bool {
        if self.dirty && !self.write_simple_draft() {
            return false;
        }
        self.dirty = false;
        let keep_unnamed = self.current_path.is_none() && !self.document.performers.is_empty();
        if keep_unnamed && !self.simple_archive_unnamed_show() {
            return false;
        }
        self.begin_simple_show();
        self.simple_mode.overlap_note = false;
        self.simple_mode.finished_playback = false;
        self.simple_mode.hold_count = false;
        self.simple_mode.naming = false;
        self.simple_mode.naming_pending = false;
        self.simple_mode.person_name.clear();
        self.simple_mode.person_name_for = None;
        self.simple_mode.scene_name.clear();
        self.simple_mode.scene_name_for = None;
        self.simple_mode.empty_pan = false;
        self.simple_mode.saved_note_until = None;
        if !self.write_simple_draft() {
            return false;
        }
        self.simple_mode.draft_saved = true;
        self.simple_mode.draft_retry_after = None;
        if keep_unnamed {
            self.simple_mode.saved_note_until = Some(Instant::now() + Duration::from_secs(3));
        }
        true
    }

    fn simple_archive_unnamed_show(&mut self) -> bool {
        if self.current_path.is_some() || self.document.performers.is_empty() {
            return true;
        }
        let Ok(json) = self.document.to_json() else {
            return false;
        };
        let path = self.simple_named_save_path(i18n::registered(self.locale, "simple-mode.168"));
        if let Some(parent) = path.parent()
            && std::fs::create_dir_all(parent).is_err()
        {
            return false;
        }
        if drill_project::atomic_write(&path, json.as_bytes(), None).is_err() {
            return false;
        }
        self.recent_projects.remember(path);
        true
    }

    fn simple_open_shows(&mut self) {
        if self.dirty && self.write_simple_draft() {
            self.dirty = false;
            self.simple_mode.draft_saved = true;
        }
        self.show_recent_projects = true;
    }

    fn simple_begin_save(&mut self) {
        if let Some(path) = self.current_path.clone().filter(|path| path.is_file()) {
            self.simple_mode.naming_pending = true;
            self.project_state
                .save_legacy_json(path, self.document.clone());
            return;
        }
        if self.simple_mode.name_draft.trim().is_empty() {
            self.simple_mode.name_draft = self.simple_default_show_name();
        }
        self.simple_mode.naming = true;
    }

    fn simple_commit_named_save(&mut self) {
        let typed = self.simple_mode.name_draft.trim().to_string();
        let raw = if typed.is_empty() {
            self.simple_default_show_name()
        } else {
            typed
        };
        let path = self.simple_named_save_path(&raw);
        if let Some(parent) = path.parent()
            && std::fs::create_dir_all(parent).is_err()
        {
            return;
        }
        self.simple_mode.name_draft = simple_show_file_stem(&raw);
        self.simple_mode.naming = false;
        self.simple_mode.naming_pending = true;
        self.project_state
            .save_legacy_json(path, self.document.clone());
    }

    fn simple_name_sheet(&mut self, ctx: &egui::Context) {
        if !self.simple_mode.naming {
            return;
        }
        let mut save = false;
        let mut cancel = false;
        egui::Window::new(i18n::registered(self.locale, "simple-mode.119"))
            .id(egui::Id::new("simple-name-sheet"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.set_min_width(320.0);
                ui.add_space(4.0);
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.simple_mode.name_draft)
                        .desired_width(f32::INFINITY),
                );
                if edit.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                    save = true;
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let save_button = egui::Button::new(
                        egui::RichText::new(i18n::registered(self.locale, "simple-mode.120"))
                            .size(16.0)
                            .color(Color32::WHITE),
                    )
                    .fill(SIMPLE_BLUE)
                    .corner_radius(18.0);
                    if ui.add_sized([140.0, 40.0], save_button).clicked() {
                        save = true;
                    }
                    if ui
                        .add_sized(
                            [100.0, 40.0],
                            egui::Button::new(i18n::registered(self.locale, "simple-mode.128"))
                                .corner_radius(18.0),
                        )
                        .clicked()
                    {
                        cancel = true;
                    }
                });
            });
        if cancel {
            self.simple_mode.naming = false;
        }
        if save {
            self.simple_commit_named_save();
        }
    }

    pub(crate) fn note_simple_save_finished(&mut self, path: &std::path::Path) {
        if !self.simple_mode.enabled {
            return;
        }
        self.simple_mode.naming_pending = false;
        self.simple_mode.naming = false;
        self.simple_mode.saved_note_until = Some(Instant::now() + Duration::from_secs(3));
        self.recent_projects.remember(path.to_path_buf());
    }

    pub(crate) fn note_simple_save_failed(&mut self) {
        if !self.simple_mode.enabled || !self.simple_mode.naming_pending {
            return;
        }
        self.simple_mode.naming_pending = false;
        if self.simple_mode.name_draft.trim().is_empty() {
            self.simple_mode.name_draft = self
                .simple_show_title()
                .unwrap_or_else(|| self.simple_default_show_name());
        }
        self.simple_mode.naming = true;
    }

    pub(crate) fn note_simple_playback_finished(&mut self) {
        if self.simple_mode.enabled {
            self.simple_mode.finished_playback = true;
        }
    }

    fn simple_start_play(&mut self, ctx: &egui::Context) {
        if self.simple_mode.finished_playback {
            self.simple_mode.finished_playback = false;
            self.jump_to_show_start();
        }
        self.toggle_playback(ctx);
    }

    fn simple_add_person(&mut self) {
        let grid = &self.document.grid;
        let step = grid.horizontal_units / f32::from(grid.horizontal_steps.max(1));
        let n = self.document.performers.len() as f32;
        let raw = Point {
            x: grid.width * 0.5 + (n - 3.0) * step * 2.0,
            y: grid.height * 0.5,
        };
        let revision = self.history.revision();
        self.place_performer_at(raw, true);
        self.simple_mark(revision, "simple-mode.055");
    }

    fn simple_rename_selected(&mut self, name: &str) {
        if self.selected.len() != 1 || !self.is_editable_set_start() {
            return;
        }
        self.ensure_performer_draft();
        let Some(draft) = self.performer_draft.as_mut() else {
            return;
        };
        if draft.name == name {
            return;
        }
        draft.name = name.to_string();
        draft.label_dirty = true;
        self.commit_performer_draft();
    }

    fn simple_name_row(&mut self, ui: &mut egui::Ui) {
        if !self.simple_name_row_visible() {
            self.simple_mode.person_name_for = None;
            return;
        }
        let index = *self.selected.iter().next().expect("one person");
        let existing = self
            .document
            .performers
            .get(index)
            .map(|performer| split_performer_label(&performer.label).1)
            .unwrap_or_default();
        if self.simple_mode.person_name_for != Some(index) {
            self.simple_mode.person_name = existing;
            self.simple_mode.person_name_for = Some(index);
        }
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(i18n::registered(self.locale, "simple-mode.117"))
                    .size(15.0)
                    .color(SIMPLE_INK),
            );
            let width = (ui.available_width() - 4.0).max(80.0);
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.simple_mode.person_name)
                    .hint_text(i18n::registered(self.locale, "simple-mode.118"))
                    .desired_width(width),
            );
            let submit = response.lost_focus()
                || (response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)));
            if submit {
                let name = self.simple_mode.person_name.clone();
                let revision = self.history.revision();
                self.simple_rename_selected(&name);
                self.simple_mark(revision, "simple-mode.190");
            }
            if !response.has_focus()
                && let Some(performer) = self.document.performers.get(index)
            {
                self.simple_mode.person_name = split_performer_label(&performer.label).1;
            }
        });
    }

    /// Dragging a person moves them. Dragging empty ground pans the field.
    fn simple_start_drag(&mut self, nearest: Option<usize>, pointer: Pos2) {
        if let Some(index) = nearest.filter(|&index| self.is_selectable_index(index)) {
            self.simple_mode.empty_pan = false;
            if self.is_editable_set_start() {
                if !self.selected.contains(&index) {
                    self.replace_selection(std::iter::once(index).collect());
                }
                self.begin_field_drag(pointer);
            } else {
                self.ensure_editable_set_start();
            }
        } else {
            self.field_viewport.begin_pan(Some(pointer));
            self.simple_mode.empty_pan = true;
        }
    }

    /// Tap on empty ground places while the first picture is being built.
    /// After that, an empty tap only clears the selection. Dragging is handled
    /// by the field painter.
    fn simple_click(
        &mut self,
        field_point: Point,
        nearest: Option<usize>,
        snap: bool,
        additive: bool,
    ) {
        match nearest {
            None if self.simple_places_on_empty_tap() => {
                let revision = self.history.revision();
                self.place_performer_at(field_point, snap);
                self.simple_mark(revision, "simple-mode.055");
            }
            None => {
                self.simple_mode.overlap_note = false;
                self.replace_selection(BTreeSet::new());
            }
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
        let mut block_pointer = false;
        if interactive {
            let dt = ui.input(|input| input.stable_dt).clamp(1.0 / 240.0, 0.1);
            let (scroll, zoom_delta) =
                ui.input(|input| (input.smooth_scroll_delta.y, input.zoom_delta()));
            if response.hovered()
                && let Some(pointer) = response.hover_pos()
            {
                if scroll != 0.0 {
                    self.field_viewport.zoom_toward(
                        (scroll * 0.0025).exp(),
                        pointer,
                        rect,
                        &self.document.grid,
                    );
                }
                if (zoom_delta - 1.0).abs() > 0.001 {
                    self.field_viewport
                        .zoom_toward(zoom_delta, pointer, rect, &self.document.grid);
                }
            }
            let middle = ui.input(|input| input.pointer.button_down(egui::PointerButton::Middle));
            let empty_pan = self.simple_mode.empty_pan;
            if middle || empty_pan {
                if middle && response.drag_started() {
                    self.field_viewport
                        .begin_pan(response.interact_pointer_pos());
                }
                if let (Some(previous), Some(pointer)) = (
                    self.field_viewport.pan_last_pointer,
                    response.interact_pointer_pos(),
                ) {
                    self.field_viewport.drag_pan(
                        pointer - previous,
                        dt,
                        &self.document.grid,
                        rect.size(),
                    );
                    self.field_viewport.pan_last_pointer = Some(pointer);
                }
                if empty_pan && response.drag_stopped() {
                    self.field_viewport.end_pan();
                    self.simple_mode.empty_pan = false;
                }
            } else if self.field_viewport.pan_last_pointer.is_some() {
                self.field_viewport.end_pan();
            }
            if self.field_viewport.tick(dt, &self.document.grid, rect) {
                ui.ctx().request_repaint();
            }
            block_pointer = middle
                || self.simple_mode.empty_pan
                || self.field_viewport.pan_last_pointer.is_some();
        }
        let render_options = drill_render::RenderOptions {
            margin: 0.0,
            field_center: Some(self.field_viewport.center),
            field_zoom: self.field_viewport.zoom,
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
        let field_map = drill_render::FieldMap::with_view(
            self.document.grid.width,
            self.document.grid.height,
            drill_render::Vec2 {
                x: rect.width(),
                y: rect.height(),
            },
            render_options.margin,
            render_options.field_center,
            render_options.field_zoom,
        );
        let to_screen = |point: Point| {
            let v = field_map.map(point);
            Pos2::new(rect.left() + v.x, rect.top() + v.y)
        };
        self.paint_simple_paths(&painter, &to_screen);
        self.paint_simple_audience(&painter, rect, &to_screen);
        self.paint_simple_sides(&painter, rect, &to_screen);
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
            if block_pointer {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
            let hover_on_dot = !block_pointer
                && response.hover_pos().is_some_and(|pos| {
                    self.frame_positions
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| self.is_selectable_index(*index))
                        .any(|(_, point)| to_screen(*point).distance(pos) < 18.0)
                });
            if response.hovered() && !block_pointer {
                let editable = self.is_editable_set_start();
                ui.ctx().set_cursor_icon(if self.drag_before.is_some() {
                    egui::CursorIcon::Grabbing
                } else if hover_on_dot && editable {
                    egui::CursorIcon::Grab
                } else if hover_on_dot {
                    egui::CursorIcon::NotAllowed
                } else if self.simple_places_on_empty_tap() && editable {
                    egui::CursorIcon::Crosshair
                } else {
                    egui::CursorIcon::Grab
                });
            }
            if !block_pointer
                && self.simple_places_on_empty_tap()
                && self.is_editable_set_start()
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
        if !interactive || block_pointer {
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
                self.simple_start_drag(nearest, pointer);
            }
            if response.dragged() && self.is_editable_set_start() && self.drag_before.is_some() {
                self.update_field_drag(pointer, field_map.scale, snap_now);
            }
            if response.drag_stopped() && self.drag_before.is_some() {
                let revision = self.history.revision();
                self.commit_field_drag();
                self.simple_mark(revision, "simple-mode.183");
                self.bump_simple_motion(ChromeMotion::Move);
            }
        }
    }

    fn paint_simple_paths(&self, painter: &egui::Painter, to_screen: &impl Fn(Point) -> Pos2) {
        if self.drag_preview.is_some() {
            return;
        }
        let ink = Color32::from_rgba_unmultiplied(20, 96, 200, 180);
        for (start, end) in simple_move_paths(&self.document, self.current_set) {
            let from = to_screen(start);
            let to = to_screen(end);
            painter.line_segment([from, to], Stroke::new(2.5, ink));
            painter.circle_stroke(to, 8.0, Stroke::new(1.5, ink));
        }
    }

    fn paint_simple_audience(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        to_screen: &impl Fn(Point) -> Pos2,
    ) {
        let label = i18n::registered(self.locale, "simple-mode.146");
        let anchor = to_screen(simple_audience_point(&self.document.grid));
        paint_simple_chip(painter, rect, anchor, label);
    }

    /// Audience's left and right, on the same front edge as 客席.
    fn paint_simple_sides(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        to_screen: &impl Fn(Point) -> Pos2,
    ) {
        let (left, right) = simple_side_anchors(&self.document.grid);
        paint_simple_chip(
            painter,
            rect,
            to_screen(left),
            i18n::registered(self.locale, "simple-mode.206"),
        );
        paint_simple_chip(
            painter,
            rect,
            to_screen(right),
            i18n::registered(self.locale, "simple-mode.207"),
        );
    }
}

fn paint_simple_chip(painter: &egui::Painter, rect: egui::Rect, anchor: Pos2, label: &str) {
    let pos = anchor + egui::Vec2::new(0.0, -16.0);
    if !rect.contains(pos) {
        return;
    }
    let galley = painter.layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(13.0),
        SIMPLE_BLUE,
    );
    let pad = egui::Vec2::new(10.0, 4.0);
    let text_pos = pos - egui::Vec2::new(galley.size().x * 0.5, galley.size().y);
    let background = egui::Rect::from_min_size(text_pos - pad, galley.size() + pad * 2.0);
    if !rect.contains_rect(background) {
        return;
    }
    painter.rect_filled(background, 10.0, Color32::from_white_alpha(235));
    painter.galley(text_pos, galley, SIMPLE_BLUE);
}

/// Front sideline, centered. Field `y = 0` is the audience side, and the
/// 2D map draws that edge at the bottom of the field.
fn simple_audience_point(grid: &drill_core::GridConfig) -> Point {
    Point {
        x: grid.width * 0.5,
        y: 0.0,
    }
}

/// Front-sideline anchors for the audience's left and right. Increasing x is
/// right, the same way a step toward the right of the field is named.
fn simple_side_anchors(grid: &drill_core::GridConfig) -> (Point, Point) {
    let inset = (grid.width * 0.08).clamp(1.0, 6.0);
    let right = (grid.max_x() - inset).max(inset);
    (Point { x: inset, y: 0.0 }, Point { x: right, y: 0.0 })
}

/// Steps from the 50 and from the front sideline. Zero on an axis is the
/// center, or the front itself.
fn simple_place_line(
    grid: &drill_core::GridConfig,
    point: Point,
    locale: drill_core::Locale,
) -> Option<String> {
    let hstep = grid.horizontal_units / f32::from(grid.horizontal_steps.max(1));
    let vstep = grid.vertical_units / f32::from(grid.vertical_steps.max(1));
    if hstep <= f32::EPSILON || vstep <= f32::EPSILON {
        return None;
    }
    let side_steps = simple_round_quarter((point.x - grid.width * 0.5) / hstep);
    let front_steps = simple_round_quarter(point.y / vstep).max(0.0);
    let front = format_step_count(front_steps);
    let across = format_step_count(side_steps.abs());
    // The 50 is not always a grid point. Anything closer to it than to a
    // full step away still reads as the center.
    let detail = if side_steps.abs() < 0.75 {
        i18n::registered(locale, "simple-mode.208").replace("{0}", &front)
    } else if side_steps > 0.0 {
        i18n::registered(locale, "simple-mode.209")
            .replace("{0}", &across)
            .replace("{1}", &front)
    } else {
        i18n::registered(locale, "simple-mode.210")
            .replace("{0}", &across)
            .replace("{1}", &front)
    };
    Some(i18n::registered(locale, "simple-mode.218").replace("{0}", &detail))
}

/// Columns and rows for a block that is square or a little wider than deep.
/// A short last row is allowed. A single-file line is not, once two columns fit.
fn simple_block_dims(count: usize) -> (usize, usize) {
    if count <= 1 {
        return (count.max(1), 1);
    }
    let mut best = (count, 1usize);
    let mut best_key = (i32::MAX, usize::MAX);
    for cols in 2..=count {
        let rows = count.div_ceil(cols);
        if rows > cols {
            continue;
        }
        let leftover = cols * rows - count;
        let ratio = ((cols as f32 / rows as f32 - 1.5).abs() * 1000.0).round() as i32;
        let penalty = if leftover == 0 { 0 } else { 850 };
        let key = (ratio + penalty, cols);
        if key < best_key {
            best_key = key;
            best = (cols, rows);
        }
    }
    best
}

/// Front row toward the audience, left to right in the order given. Gaps are
/// two steps, and the block stays centered on the group.
fn simple_block_points(grid: &drill_core::GridConfig, points: &[Point]) -> Vec<Point> {
    let count = points.len();
    if count < 2 {
        return points.to_vec();
    }
    let (cols, rows) = simple_block_dims(count);
    let step_x = grid.horizontal_units / f32::from(grid.horizontal_steps.max(1));
    let step_y = grid.vertical_units / f32::from(grid.vertical_steps.max(1));
    if step_x <= f32::EPSILON || step_y <= f32::EPSILON {
        return points.to_vec();
    }
    let gap_x = step_x * 2.0;
    let gap_y = step_y * 2.0;
    let center = grid.snap(drill_core::editing::centroid(points));
    let raw: Vec<Point> = (0..count)
        .map(|index| {
            let row = index / cols;
            let col = index % cols;
            let row_count = if row + 1 == rows {
                count - row * cols
            } else {
                cols
            };
            Point {
                x: center.x + (col as f32 - (row_count as f32 - 1.0) * 0.5) * gap_x,
                y: center.y + (row as f32 - (rows as f32 - 1.0) * 0.5) * gap_y,
            }
        })
        .collect();
    simple_fit_points(grid, &raw)
        .into_iter()
        .map(|point| {
            let snapped = grid.snap(point);
            Point {
                x: snapped.x.clamp(0.0, grid.max_x()),
                y: snapped.y.clamp(0.0, grid.max_y()),
            }
        })
        .collect()
}

/// A quarter step or less is a hold, matching drill-core continuity.
const SIMPLE_HOLD_STEPS: f32 = 0.25;

struct SimpleTravel {
    index: usize,
    kind: drill_core::continuity::TravelDirection,
    steps: f32,
}

fn simple_round_quarter(value: f32) -> f32 {
    (value * 4.0).round() / 4.0
}

fn format_step_count(steps: f32) -> String {
    let quarter = simple_round_quarter(steps);
    if (quarter - quarter.round()).abs() < 0.01 {
        format!("{}", quarter.round() as i32)
    } else if (quarter * 2.0 - (quarter * 2.0).round()).abs() < 0.01 {
        format!("{quarter:.1}")
    } else {
        format!("{quarter:.2}")
    }
}

/// Same 8-way names as `drill_core::continuity`: y decreases toward the audience.
fn simple_travel_direction(steps_x: f32, steps_y: f32) -> drill_core::continuity::TravelDirection {
    use drill_core::continuity::TravelDirection::{
        Backward, Forward, Hold, Left, LeftBackward, LeftForward, Right, RightBackward,
        RightForward,
    };
    let horizontal = steps_x.abs() >= SIMPLE_HOLD_STEPS;
    let vertical = steps_y.abs() >= SIMPLE_HOLD_STEPS;
    match (
        horizontal,
        vertical,
        steps_x.is_sign_positive(),
        steps_y.is_sign_positive(),
    ) {
        (false, false, _, _) => Hold,
        (true, false, true, _) => Right,
        (true, false, false, _) => Left,
        (false, true, _, false) => Forward,
        (false, true, _, true) => Backward,
        (true, true, true, false) => RightForward,
        (true, true, true, true) => RightBackward,
        (true, true, false, false) => LeftForward,
        (true, true, false, true) => LeftBackward,
    }
}

fn simple_transition_ends(
    document: &drill_core::Document,
    set_index: usize,
) -> Option<(usize, usize)> {
    let sets = document.sets.len();
    if sets < 2 || set_index >= sets {
        return None;
    }
    if set_index + 1 < sets {
        Some((set_index, set_index + 1))
    } else {
        Some((set_index - 1, set_index))
    }
}

fn simple_person_travel(
    document: &drill_core::Document,
    index: usize,
    from: usize,
    to: usize,
) -> Option<SimpleTravel> {
    let start = document.sets.get(from)?.positions.get(index).copied()?;
    let end = document.sets.get(to)?.positions.get(index).copied()?;
    let grid = &document.grid;
    let hstep = grid.horizontal_units / f32::from(grid.horizontal_steps.max(1));
    if hstep <= f32::EPSILON {
        return None;
    }
    let vstep = grid.vertical_units / f32::from(grid.vertical_steps.max(1));
    let steps_x = simple_round_quarter((end.x - start.x) / hstep);
    let steps_y = if vstep <= f32::EPSILON {
        0.0
    } else {
        simple_round_quarter((end.y - start.y) / vstep)
    };
    let raw = (end.x - start.x).hypot(end.y - start.y) / hstep;
    let kind = if raw < SIMPLE_HOLD_STEPS {
        drill_core::continuity::TravelDirection::Hold
    } else {
        simple_travel_direction(steps_x, steps_y)
    };
    Some(SimpleTravel {
        index,
        kind,
        steps: simple_round_quarter(raw),
    })
}

/// The farthest walk on the move this scene shows. One chosen person who
/// stays put is reported as a hold. Reading this never writes the document.
fn simple_travel_note(
    document: &drill_core::Document,
    set_index: usize,
    chosen: &[usize],
) -> Option<SimpleTravel> {
    let (from, to) = simple_transition_ends(document, set_index)?;
    let indices: Vec<usize> = if chosen.is_empty() {
        (0..document.performers.len()).collect()
    } else {
        chosen
            .iter()
            .copied()
            .filter(|index| *index < document.performers.len())
            .collect()
    };
    if indices.is_empty() {
        return None;
    }
    let mut best: Option<SimpleTravel> = None;
    for index in indices {
        let Some(travel) = simple_person_travel(document, index, from, to) else {
            continue;
        };
        let replace = best
            .as_ref()
            .is_none_or(|current| travel.steps > current.steps);
        if replace {
            best = Some(travel);
        }
    }
    let best = best?;
    if best.kind == drill_core::continuity::TravelDirection::Hold && chosen.len() != 1 {
        None
    } else {
        Some(best)
    }
}

/// The move leaving this scene, or the move arriving at the last scene.
/// A person who stays put is left out. Reading this never writes the document.
fn simple_move_paths(document: &drill_core::Document, set_index: usize) -> Vec<(Point, Point)> {
    let sets = document.sets.len();
    if sets < 2 || set_index >= sets {
        return Vec::new();
    }
    let (from, to) = if set_index + 1 < sets {
        (set_index, set_index + 1)
    } else {
        (set_index - 1, set_index)
    };
    let start = &document.sets[from].positions;
    let end = &document.sets[to].positions;
    start
        .iter()
        .zip(end.iter())
        .filter(|&(from_point, to_point)| {
            (from_point.x - to_point.x).hypot(from_point.y - to_point.y) > 0.05
        })
        .map(|(from_point, to_point)| (*from_point, *to_point))
        .collect()
}

/// Front-to-back file. Index 0 stands closest to the audience.
fn simple_file_points(grid: &drill_core::GridConfig, points: &[Point]) -> Vec<Point> {
    let count = points.len();
    if count < 2 {
        return points.to_vec();
    }
    let x = points.iter().map(|point| point.x).sum::<f32>() / count as f32;
    let step = grid.vertical_units / f32::from(grid.vertical_steps.max(1));
    let ideal = step * 2.0;
    let span = (ideal * count.saturating_sub(1) as f32).min(grid.height * 0.72);
    let gap = span / (count - 1) as f32;
    let front = ((grid.height - span) * 0.5).max(0.0);
    (0..count)
        .map(|index| Point {
            x,
            y: front + gap * index as f32,
        })
        .collect()
}

/// Circle around the group, kept on the field. Index 0 faces the audience.
fn simple_circle_points(grid: &drill_core::GridConfig, points: &[Point]) -> Vec<Point> {
    let count = points.len();
    if count < 2 {
        return points.to_vec();
    }
    let center = drill_core::editing::centroid(points);
    let step = grid.vertical_units / f32::from(grid.vertical_steps.max(1));
    let spacing = (step * 2.0).max(0.5);
    let chord_radius = spacing / (2.0 * (std::f32::consts::PI / count as f32).sin().max(0.05));
    // A two-step chord is correct for a large cast, but four people at that
    // spacing collapse into one blob on screen. Keep a circle big enough to read.
    let min_radius = (grid.width.min(grid.height) * 0.18).max(spacing);
    let desired = chord_radius.max(min_radius);
    let max_radius = (grid.width.min(grid.height) * 0.42).max(spacing);
    let radius = desired.clamp(spacing * 0.5, max_radius);
    let center = Point {
        x: center.x.clamp(radius, (grid.width - radius).max(radius)),
        y: center.y.clamp(radius, (grid.height - radius).max(radius)),
    };
    drill_core::shapes::circle(center, radius, count)
}

fn simple_bounds(points: &[Point]) -> (f32, f32, f32, f32) {
    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for point in points {
        min_x = min_x.min(point.x);
        max_x = max_x.max(point.x);
        min_y = min_y.min(point.y);
        max_y = max_y.max(point.y);
    }
    (min_x, max_x, min_y, max_y)
}

/// Slides a group onto the field. If it is larger than the field, shrinks it
/// about its center first so people are not piled on the sideline.
fn simple_fit_points(grid: &drill_core::GridConfig, points: &[Point]) -> Vec<Point> {
    if points.is_empty() {
        return Vec::new();
    }
    let limit_x = grid.max_x().max(0.0);
    let limit_y = grid.max_y().max(0.0);
    let mut fitted = points.to_vec();
    let (min_x, max_x, min_y, max_y) = simple_bounds(&fitted);
    let width = (max_x - min_x).max(0.0);
    let height = (max_y - min_y).max(0.0);
    let shrink_x = if width > limit_x && width > 0.0 {
        limit_x / width
    } else {
        1.0
    };
    let shrink_y = if height > limit_y && height > 0.0 {
        limit_y / height
    } else {
        1.0
    };
    let shrink = shrink_x.min(shrink_y).min(1.0);
    if shrink < 1.0 {
        let pivot = drill_core::editing::centroid(&fitted);
        fitted = drill_core::editing::scale(&fitted, shrink, shrink, pivot);
    }
    let (min_x, max_x, min_y, max_y) = simple_bounds(&fitted);
    let dx = if min_x < 0.0 {
        -min_x
    } else if max_x > limit_x {
        limit_x - max_x
    } else {
        0.0
    };
    let dy = if min_y < 0.0 {
        -min_y
    } else if max_y > limit_y {
        limit_y - max_y
    } else {
        0.0
    };
    if dx != 0.0 || dy != 0.0 {
        for point in &mut fitted {
            point.x += dx;
            point.y += dy;
        }
    }
    fitted
}

/// Left to right, with the middle of the curve toward the audience.
fn simple_arc_points(grid: &drill_core::GridConfig, points: &[Point]) -> Vec<Point> {
    let count = points.len();
    if count < 2 {
        return points.to_vec();
    }
    let mid = drill_core::editing::centroid(points);
    let step = grid.horizontal_units / f32::from(grid.horizontal_steps.max(1));
    let spacing = (step * 2.0).max(0.5);
    let sweep = std::f32::consts::FRAC_PI_2 * 4.0 / 3.0;
    let chord = spacing * (count - 1) as f32;
    let min_radius = (grid.width.min(grid.height) * 0.18).max(spacing);
    let max_radius = (grid.width.min(grid.height) * 0.7).max(min_radius);
    let radius = (chord / sweep).clamp(min_radius, max_radius);
    let center = Point {
        x: mid.x,
        y: mid.y + radius,
    };
    let start = -std::f32::consts::FRAC_PI_2 - sweep * 0.5;
    let end = -std::f32::consts::FRAC_PI_2 + sweep * 0.5;
    let arc = drill_core::evenly_spaced_arc(center, radius, start, end, count);
    simple_fit_points(grid, &arc)
}

/// One step farther from, or closer to, the group's center. Already-tight
/// groups stay at about one step so people are not stacked by the button.
fn simple_spacing_points(
    grid: &drill_core::GridConfig,
    points: &[Point],
    outward: bool,
) -> Vec<Point> {
    if points.len() < 2 {
        return points.to_vec();
    }
    let center = drill_core::editing::centroid(points);
    let step = grid.horizontal_units / f32::from(grid.horizontal_steps.max(1));
    let farthest = points
        .iter()
        .map(|point| (point.x - center.x).hypot(point.y - center.y))
        .fold(0.0_f32, f32::max);
    if farthest < step * 0.25 {
        return points.to_vec();
    }
    let next = if outward {
        farthest + step
    } else {
        (farthest - step).max(step)
    };
    let factor = (next / farthest).clamp(0.2, 4.0);
    if (factor - 1.0).abs() < 0.02 {
        return points.to_vec();
    }
    let scaled = drill_core::editing::scale(points, factor, factor, center);
    simple_fit_points(grid, &scaled)
}

/// "セット 1" and "Set 2" are the names a new scene already has.
fn simple_stock_scene_name(name: &str) -> bool {
    let name = name.trim();
    let rest = name
        .strip_prefix("セット")
        .or_else(|| name.strip_prefix("Set"))
        .map(str::trim);
    rest.is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
}

fn simple_scene_name_draft(name: &str) -> String {
    if simple_stock_scene_name(name) {
        String::new()
    } else {
        name.to_string()
    }
}

fn simple_scene_name_text(raw: &str) -> String {
    let mut cleaned = String::new();
    for c in raw.chars() {
        if c.is_control() {
            cleaned.push(' ');
        } else {
            cleaned.push(c);
        }
    }
    cleaned
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(20)
        .collect()
}

/// Screen arrows. Up moves away from the audience, because the audience
/// label sits on the bottom edge of the field.
fn simple_arrow_step(key: egui::Key) -> Option<(i32, i32)> {
    match key {
        egui::Key::ArrowLeft => Some((-1, 0)),
        egui::Key::ArrowRight => Some((1, 0)),
        egui::Key::ArrowUp => Some((0, 1)),
        egui::Key::ArrowDown => Some((0, -1)),
        _ => None,
    }
}

/// The name half of a label. A label with no number, such as "山田", is a name.
fn simple_kept_name(label: &str) -> String {
    let (number, name) = split_performer_label(label);
    if name.is_empty() && number.parse::<u32>().is_err() {
        number
    } else {
        name
    }
}

/// Audience's left first (smaller x), then closer to the audience (smaller y).
fn simple_renumber_order(points: &[Point]) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..points.len()).collect();
    indices.sort_by(|&left, &right| {
        points[left]
            .x
            .total_cmp(&points[right].x)
            .then(points[left].y.total_cmp(&points[right].y))
            .then(left.cmp(&right))
    });
    indices
}

struct SimpleLongStride {
    index: usize,
    steps: f32,
    counts: u16,
}

struct SimpleCaution {
    collisions: Vec<usize>,
    long_stride: Option<SimpleLongStride>,
}

/// People whose paths come within a step of each other, and anyone walking
/// more than one step per count. Reads the document; the scratch buffer is
/// only workspace for the existing clinic.
fn simple_caution(
    document: &drill_core::Document,
    set_index: usize,
    scratch: &mut clinic::ScanScratch,
) -> SimpleCaution {
    let Some((from, _)) = simple_transition_ends(document, set_index) else {
        return SimpleCaution {
            collisions: Vec::new(),
            long_stride: None,
        };
    };
    let counts = document
        .sets
        .get(from)
        .map(|set| set.counts.max(1))
        .unwrap_or(1);
    let report = clinic::scan_transition(
        document,
        from,
        super::smart_transition_state::clinic_params(),
        scratch,
    );
    let mut collisions = Vec::new();
    for event in report.collisions {
        for id in [event.a, event.b] {
            if let Some(index) = document
                .performers
                .iter()
                .position(|performer| performer.id == id)
                && !collisions.contains(&index)
            {
                collisions.push(index);
            }
        }
    }
    collisions.sort_unstable();
    let mut long_stride = None;
    let limit = f32::from(counts);
    let to = from + 1;
    for index in 0..document.performers.len() {
        let Some(travel) = simple_person_travel(document, index, from, to) else {
            continue;
        };
        if travel.steps > limit
            && long_stride
                .as_ref()
                .is_none_or(|current: &SimpleLongStride| travel.steps > current.steps)
        {
            long_stride = Some(SimpleLongStride {
                index,
                steps: travel.steps,
                counts,
            });
        }
    }
    SimpleCaution {
        collisions,
        long_stride,
    }
}

fn simple_roster_order(document: &drill_core::Document) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..document.performers.len()).collect();
    indices.sort_by(|&left, &right| {
        let left_label = document.performers[left].label.as_str();
        let right_label = document.performers[right].label.as_str();
        let (left_number, _) = split_performer_label(left_label);
        let (right_number, _) = split_performer_label(right_label);
        match (
            left_number.parse::<u32>().ok(),
            right_number.parse::<u32>().ok(),
        ) {
            (Some(a), Some(b)) => a.cmp(&b).then(left.cmp(&right)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => left_label.cmp(right_label).then(left.cmp(&right)),
        }
    });
    indices
}

/// Plain text a director can read or paste into a message. No file is written.
fn simple_scene_memo(
    document: &drill_core::Document,
    set_index: usize,
    locale: drill_core::Locale,
) -> String {
    let ends = simple_transition_ends(document, set_index);
    let (which, counts) = if let Some((from, to)) = ends {
        (
            format!("{} → {}", from + 1, to + 1),
            document.sets.get(from).map(|set| set.counts).unwrap_or(0),
        )
    } else {
        (
            (set_index + 1).to_string(),
            document
                .sets
                .get(set_index)
                .map(|set| set.counts)
                .unwrap_or(0),
        )
    };
    let header = i18n::registered(locale, "simple-mode.187")
        .replace("{0}", &which)
        .replace("{1}", &counts.to_string());
    let mut lines = vec![header];
    if let Some(set) = document.sets.get(set_index)
        && !simple_stock_scene_name(&set.name)
    {
        lines.push(set.name.clone());
    }
    for index in simple_roster_order(document) {
        let label = document
            .performers
            .get(index)
            .map(|performer| performer.label.as_str())
            .unwrap_or("");
        let walk = ends.and_then(|(from, to)| simple_person_travel(document, index, from, to));
        let detail = match walk {
            Some(travel) if travel.kind == drill_core::continuity::TravelDirection::Hold => {
                i18n::registered(locale, "simple-mode.188").to_string()
            }
            Some(travel) => i18n::registered(locale, "simple-mode.189")
                .replace("{0}", travel.kind.text(locale))
                .replace("{1}", &format_step_count(travel.steps)),
            None => String::new(),
        };
        if detail.is_empty() {
            lines.push(label.to_string());
        } else {
            lines.push(format!("{label}  {detail}"));
        }
    }
    lines.join("\n")
}

fn simple_show_file_stem(raw: &str) -> String {
    let raw = raw.trim();
    let raw = raw
        .strip_suffix(".drill.json")
        .or_else(|| raw.strip_suffix(".json"))
        .unwrap_or(raw);
    let mut cleaned = String::new();
    for c in raw.chars() {
        if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
            cleaned.push(' ');
        } else {
            cleaned.push(c);
        }
    }
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim_matches(|c: char| c == '.' || c.is_whitespace());
    if trimmed.is_empty() {
        "show".to_string()
    } else {
        trimmed.chars().take(40).collect()
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
        assert!(app.simple_mode.return_hint);
    }

    #[test]
    fn return_hint_goes_back_to_simple_mode() {
        let mut app = empty_simple_app();
        app.set_simple_mode(false);
        app.set_simple_mode(true);
        assert!(app.simple_mode.enabled);
        assert!(app.onboarding.prefer_simple);
        assert!(!app.simple_mode.return_hint);
    }

    #[test]
    fn simple_mode_can_remove_the_last_person_and_undo() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        app.replace_selection([0].into_iter().collect());
        app.remove_selected_performers();
        assert!(app.document.performers.is_empty());
        assert!(app.history.can_undo());
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.performers.len(), 1);
    }

    #[test]
    fn beat_length_changes_in_one_undo_step() {
        let mut app = empty_simple_app();
        let before = app.document.sets[0].counts;
        app.simple_step_counts(8);
        assert_eq!(app.document.sets[0].counts, before + 8);
        assert!(app.history.can_undo());
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.sets[0].counts, before);
        app.simple_step_counts(-8);
        assert_eq!(app.document.sets[0].counts, before - 8);
    }

    #[test]
    fn an_extra_scene_can_be_removed() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        app.duplicate_current_set();
        assert_eq!(app.document.sets.len(), 2);
        app.delete_current_set();
        assert_eq!(app.document.sets.len(), 1);
    }

    #[test]
    fn playback_keeps_a_pause_action_before_anyone_has_moved() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        app.duplicate_current_set();
        assert_eq!(app.simple_guide(), SimpleGuide::Move);
        assert!(app.simple_primary_action().is_none());
        app.playing = true;
        assert!(matches!(
            app.simple_primary_action(),
            Some(SimplePrimary::Pause)
        ));
    }

    #[test]
    fn new_work_in_simple_mode_opens_an_empty_field() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        app.dirty = false;
        app.request_new_show();
        assert!(app.simple_mode.enabled);
        assert!(app.onboarding.prefer_simple);
        assert!(app.document.performers.is_empty());
        assert_eq!(app.simple_guide(), SimpleGuide::Place);
    }

    #[test]
    fn redo_is_available_after_undo_in_simple_mode() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        assert!(app.history.can_undo());
        assert!(app.history.undo(&mut app.document));
        assert!(app.document.performers.is_empty());
        assert!(app.history.can_redo());
        assert!(app.history.redo(&mut app.document));
        assert_eq!(app.document.performers.len(), 1);
    }

    #[test]
    fn empty_tap_adds_people_only_on_the_first_picture() {
        let mut app = empty_simple_app();
        app.simple_click(Point { x: 8.0, y: 6.0 }, None, true, false);
        app.simple_click(Point { x: 14.0, y: 6.0 }, None, true, false);
        assert_eq!(app.document.performers.len(), 2);
        app.duplicate_current_set();
        assert!(!app.simple_places_on_empty_tap());
        app.replace_selection([0].into_iter().collect());
        app.simple_click(Point { x: 1.0, y: 1.0 }, None, true, false);
        assert_eq!(app.document.performers.len(), 2);
        assert!(app.selected.is_empty());
    }

    #[test]
    fn empty_drag_pans_and_a_person_drag_still_moves() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        let before = app.document.sets[0].positions[0];
        app.simple_start_drag(None, Pos2::new(10.0, 10.0));
        assert!(app.simple_mode.empty_pan);
        assert!(app.drag_before.is_none());
        assert_eq!(app.document.sets[0].positions[0], before);
        app.simple_mode.empty_pan = false;
        app.simple_start_drag(Some(0), Pos2::new(0.0, 0.0));
        assert!(!app.simple_mode.empty_pan);
        assert!(app.drag_before.is_some());
    }

    #[test]
    fn a_selected_person_can_be_named_and_the_json_keeps_the_label() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        app.replace_selection([0].into_iter().collect());
        app.simple_rename_selected("山田");
        assert_eq!(app.document.performers[0].label, "1 山田");
        let json = app.document.to_json().expect("json");
        let loaded = drill_core::Document::from_json(&json).expect("reload");
        assert_eq!(loaded.performers[0].label, "1 山田");
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.performers[0].label, "1");
    }

    #[test]
    fn an_extra_person_can_be_added_after_the_first_picture() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        app.duplicate_current_set();
        app.simple_add_person();
        assert_eq!(app.document.performers.len(), 2);
        assert_eq!(app.document.sets[0].positions.len(), 2);
        assert_eq!(app.document.sets[1].positions.len(), 2);
    }

    #[test]
    fn named_save_path_stays_inside_the_shows_folder() {
        let dir = std::env::temp_dir().join(format!(
            "drillforge-shows-{}-{}",
            std::process::id(),
            "path"
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("shows dir");
        let mut app = empty_simple_app();
        app.simple_mode.shows_dir_override = Some(dir.clone());
        assert_eq!(simple_show_file_stem(".."), "show");
        assert_eq!(simple_show_file_stem("文化祭.drill.json"), "文化祭");
        let path = app.simple_named_save_path("../../etc/passwd");
        assert_eq!(path.parent(), Some(dir.as_path()));
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("name");
        assert!(name.ends_with(".drill.json"));
        assert!(!name.contains(".."));
        assert!(!name.contains('/'));
        std::fs::write(&path, b"taken").expect("occupy");
        let second = app.simple_named_save_path("../../etc/passwd");
        assert_ne!(path, second);
        assert_eq!(second.parent(), Some(dir.as_path()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_named_show_reopens_with_the_same_name() {
        let dir = std::env::temp_dir().join(format!(
            "drillforge-named-{}-{}",
            std::process::id(),
            "keep"
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let draft = dir.join("simple-draft.drill.json");
        let named = dir.join("文化祭.drill.json");
        let mut app = empty_simple_app();
        app.simple_mode.draft_path_override = Some(draft.clone());
        app.current_path = Some(named.clone());
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        assert!(app.write_simple_draft());
        let saved = std::fs::read_to_string(&named).expect("named file");
        assert!(drill_core::Document::from_json(&saved).is_ok());
        let mut reopened = DrillApp::default();
        reopened.onboarding.welcome_seen = true;
        reopened.onboarding.prefer_simple = true;
        reopened.onboarding.show_welcome = false;
        reopened.simple_mode.draft_path_override = Some(draft);
        reopened.open_into_preferred_editor();
        assert_eq!(reopened.current_path.as_deref(), Some(named.as_path()));
        assert_eq!(reopened.simple_show_title().as_deref(), Some("文化祭"));
        assert_eq!(reopened.document.performers.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn opening_in_simple_mode_shows_the_work_list() {
        let mut app = empty_simple_app();
        app.simple_open_shows();
        assert!(app.show_recent_projects);
        assert_eq!(app.document_open_guard, DocumentOpenGuard::Idle);
    }

    #[test]
    fn finishing_playback_offers_another_look_from_the_start() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        app.duplicate_current_set();
        let set_id = app.document.sets[1].id;
        let performer_id = app.document.performers[0].id;
        assert!(app.execute_edit(
            Edit::MovePerformers {
                set_id,
                performer_ids: vec![performer_id],
                positions: vec![Point { x: 20.0, y: 10.0 }],
            },
            "move",
        ));
        app.navigate_to_set(1);
        app.note_simple_playback_finished();
        assert!(app.simple_offers_replay());
        let ctx = egui::Context::default();
        app.simple_start_play(&ctx);
        assert!(!app.simple_mode.finished_playback);
        assert!(app.playing);
        assert_eq!(app.current_set, 0);
        assert!(app.count_position.abs() < f32::EPSILON);
    }

    fn moved_pair() -> DrillApp {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        app.place_performer_at(Point { x: 14.0, y: 18.0 }, true);
        app.duplicate_current_set();
        let set_id = app.document.sets[app.current_set].id;
        let performer_id = app.document.performers[0].id;
        assert!(app.execute_edit(
            Edit::MovePerformers {
                set_id,
                performer_ids: vec![performer_id],
                positions: vec![Point { x: 24.0, y: 10.0 }],
            },
            "move",
        ));
        app
    }

    #[test]
    fn another_scene_can_be_added_after_the_first_move() {
        let mut app = moved_pair();
        assert_eq!(app.document.sets.len(), 2);
        let copied = app.document.sets[1].positions.clone();
        app.simple_add_scene();
        assert_eq!(app.document.sets.len(), 3);
        assert_eq!(app.document.sets[2].positions, copied);
        assert_eq!(app.current_set, 2);
        assert!(app.history.can_undo());
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.sets.len(), 2);
    }

    #[test]
    fn a_held_count_does_not_accept_a_new_scene() {
        let mut app = moved_pair();
        app.navigate_to_set(0);
        app.nav_glide.settle();
        app.simple_step_beat(2);
        app.nav_glide.settle();
        assert!(app.simple_holding_a_count());
        let sets = app.document.sets.len();
        app.simple_add_scene();
        assert_eq!(app.document.sets.len(), sets);
    }

    #[test]
    fn everyone_can_be_selected_and_dragged_together() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        app.place_performer_at(Point { x: 14.0, y: 6.0 }, true);
        app.place_performer_at(Point { x: 20.0, y: 6.0 }, true);
        app.simple_toggle_everyone();
        assert_eq!(app.selected.len(), 3);
        assert!(app.simple_everyone_selected());
        let before = app.document.sets[0].positions.clone();
        app.begin_field_drag(Pos2::new(0.0, 0.0));
        app.update_field_drag(Pos2::new(40.0, 0.0), 10.0, false);
        app.commit_field_drag();
        for (from, to) in before.iter().zip(&app.document.sets[0].positions) {
            assert!((to.x - from.x) > 0.5);
            assert!((to.y - from.y).abs() < 0.01);
        }
        app.simple_toggle_everyone();
        assert!(app.selected.is_empty());
    }

    #[test]
    fn line_up_is_one_undo_and_the_json_stays_readable() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 4.0, y: 4.0 }, true);
        app.place_performer_at(Point { x: 10.0, y: 30.0 }, true);
        app.place_performer_at(Point { x: 36.0, y: 12.0 }, true);
        let parked = app.document.sets[0].positions[2];
        let before = app.document.sets[0].positions.clone();
        app.replace_selection([0_usize, 1].into_iter().collect());
        app.simple_line_up();
        let positions = &app.document.sets[0].positions;
        assert!((positions[0].y - positions[1].y).abs() < 0.01);
        assert!(positions[0].x < positions[1].x);
        assert_eq!(positions[2], parked);
        let json = app.document.to_json().expect("json");
        let loaded = drill_core::Document::from_json(&json).expect("reload");
        assert_eq!(loaded.sets[0].positions, *positions);
        assert_eq!(loaded.performers.len(), 3);
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.sets[0].positions, before);
    }

    #[test]
    fn stepping_one_count_does_not_write_the_document() {
        let mut app = moved_pair();
        app.navigate_to_set(0);
        app.nav_glide.settle();
        let revision = app.history.revision();
        let document = app.document.clone();
        app.simple_step_beat(1);
        app.nav_glide.settle();
        assert_eq!(app.document, document);
        assert_eq!(app.history.revision(), revision);
        assert!(app.count_position > 0.5);
        assert!(app.simple_holding_a_count());
        assert!(!app.is_editable_set_start());
        app.simple_step_beat(-1);
        app.nav_glide.settle();
        assert!(!app.simple_holding_a_count());
        assert!(app.is_editable_set_start());
        assert_eq!(app.document, document);
    }

    #[test]
    fn slow_playback_toggles_between_half_and_normal() {
        let mut app = empty_simple_app();
        assert!((app.speed - 1.0).abs() < 0.01);
        app.simple_toggle_slow();
        assert!(app.simple_is_slow());
        assert!((app.speed - 0.5).abs() < 0.01);
        app.simple_toggle_slow();
        assert!(!app.simple_is_slow());
        assert!((app.speed - 1.0).abs() < 0.01);
    }

    #[test]
    fn audience_mark_is_the_front_sideline_drawn_at_the_bottom() {
        let grid = drill_core::GridConfig::japan_floor();
        let point = simple_audience_point(&grid);
        assert!((point.x - grid.width * 0.5).abs() < 0.01);
        assert!(point.y.abs() < 0.01);
        let map = drill_render::FieldMap::with_view(
            grid.width,
            grid.height,
            drill_render::Vec2 { x: 400.0, y: 300.0 },
            0.0,
            None,
            1.0,
        );
        let front = map.map(point);
        let back = map.map(Point {
            x: point.x,
            y: grid.height,
        });
        assert!(front.y > back.y);
    }

    #[test]
    fn path_lines_follow_a_move_without_writing_the_document() {
        let app = moved_pair();
        let revision = app.history.revision();
        let document = app.document.clone();
        let paths = simple_move_paths(&app.document, app.current_set);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].0, app.document.sets[0].positions[0]);
        assert_eq!(paths[0].1, app.document.sets[1].positions[0]);
        assert_eq!(simple_move_paths(&app.document, 0), paths);
        assert_eq!(app.document, document);
        assert_eq!(app.history.revision(), revision);

        let mut still = empty_simple_app();
        still.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        still.place_performer_at(Point { x: 14.0, y: 18.0 }, true);
        still.duplicate_current_set();
        assert!(simple_move_paths(&still.document, 0).is_empty());
        assert!(simple_move_paths(&still.document, 1).is_empty());
    }

    #[test]
    fn file_up_puts_the_first_person_toward_the_audience() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 8.0, y: 28.0 }, true);
        app.place_performer_at(Point { x: 30.0, y: 8.0 }, true);
        app.place_performer_at(Point { x: 18.0, y: 18.0 }, true);
        let parked = app.document.sets[0].positions[2];
        let before = app.document.sets[0].positions.clone();
        app.replace_selection([0_usize, 1].into_iter().collect());
        app.simple_file_up();
        let positions = &app.document.sets[0].positions;
        assert!(positions[0].y < positions[1].y);
        assert!((positions[0].x - positions[1].x).abs() < 0.01);
        assert_eq!(positions[2], parked);
        assert!(app.document.sets[0].shape.is_none());
        let json = app.document.to_json().expect("json");
        let loaded = drill_core::Document::from_json(&json).expect("reload");
        assert_eq!(loaded.sets[0].positions, *positions);
        assert!(loaded.sets[0].shape.is_none());
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.sets[0].positions, before);
    }

    #[test]
    fn circle_puts_the_first_person_toward_the_audience() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 8.0, y: 8.0 }, true);
        app.place_performer_at(Point { x: 16.0, y: 24.0 }, true);
        app.place_performer_at(Point { x: 28.0, y: 12.0 }, true);
        app.place_performer_at(Point { x: 22.0, y: 30.0 }, true);
        let before = app.document.sets[0].positions.clone();
        let intended = simple_circle_points(&app.document.grid, &before);
        assert_eq!(intended.len(), 4);
        assert!(intended[0].y + 0.01 < intended[1].y.min(intended[2].y).min(intended[3].y));
        let width = intended
            .iter()
            .map(|point| point.x)
            .fold(f32::NEG_INFINITY, f32::max)
            - intended
                .iter()
                .map(|point| point.x)
                .fold(f32::INFINITY, f32::min);
        assert!(width > 8.0, "a small group still needs a readable circle");
        app.simple_circle_up();
        let positions = &app.document.sets[0].positions;
        for (got, want) in positions.iter().zip(&intended) {
            assert!((got.x - want.x).abs() < 1.0);
            assert!((got.y - want.y).abs() < 1.0);
        }
        assert!(positions[0].y < positions[1].y);
        assert!(app.document.sets[0].shape.is_none());
        let json = app.document.to_json().expect("json");
        let loaded = drill_core::Document::from_json(&json).expect("reload");
        assert_eq!(loaded.sets[0].positions, *positions);
        assert_eq!(loaded.performers.len(), 4);
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.sets[0].positions, before);
    }

    #[test]
    fn swapping_sides_is_one_undo_and_the_json_stays_readable() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 6.0, y: 16.0 }, true);
        app.place_performer_at(Point { x: 18.0, y: 16.0 }, true);
        app.place_performer_at(Point { x: 32.0, y: 16.0 }, true);
        let before = app.document.sets[0].positions.clone();
        app.simple_swap_sides();
        let after = &app.document.sets[0].positions;
        let left_before = before
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| a.x.total_cmp(&b.x))
            .map(|(index, _)| index)
            .expect("left");
        let right_after = after
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.x.total_cmp(&b.x))
            .map(|(index, _)| index)
            .expect("right");
        assert_eq!(left_before, right_after);
        for (from, to) in before.iter().zip(after.iter()) {
            assert!((from.y - to.y).abs() < 0.01);
        }
        assert!(app.document.sets[0].shape.is_none());
        let json = app.document.to_json().expect("json");
        let loaded = drill_core::Document::from_json(&json).expect("reload");
        assert_eq!(loaded.sets[0].positions, *after);
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.sets[0].positions, before);
    }

    #[test]
    fn a_held_count_does_not_reshape_the_group() {
        let mut app = moved_pair();
        app.navigate_to_set(0);
        app.nav_glide.settle();
        app.simple_step_beat(2);
        app.nav_glide.settle();
        assert!(app.simple_holding_a_count());
        let positions = app.document.sets[0].positions.clone();
        let name = app.document.sets[0].name.clone();
        let revision = app.history.revision();
        app.simple_file_up();
        app.simple_circle_up();
        app.simple_swap_sides();
        app.simple_swap_ends();
        app.simple_turn_right();
        app.simple_arc_up();
        app.simple_change_spacing(true);
        app.simple_change_spacing(false);
        app.simple_line_up();
        app.simple_rename_scene("サビ");
        app.replace_selection([0_usize, 1].into_iter().collect());
        app.simple_swap_pair();
        app.simple_block_up();
        app.simple_restore_selected();
        assert_eq!(app.document.sets[0].positions, positions);
        assert_eq!(app.document.sets[0].name, name);
        assert_eq!(app.history.revision(), revision);
    }

    fn grid_step(app: &DrillApp) -> (f32, f32) {
        let grid = &app.document.grid;
        (
            grid.horizontal_units / f32::from(grid.horizontal_steps.max(1)),
            grid.vertical_units / f32::from(grid.vertical_steps.max(1)),
        )
    }

    #[test]
    fn nudge_moves_one_step_and_one_undo_restores_it() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 12.0, y: 16.0 }, true);
        app.place_performer_at(Point { x: 20.0, y: 16.0 }, true);
        app.replace_selection([0_usize, 1].into_iter().collect());
        let before = app.document.sets[0].positions.clone();
        let (dx, dy) = grid_step(&app);
        app.simple_nudge(1, 0);
        for (from, to) in before.iter().zip(&app.document.sets[0].positions) {
            assert!((to.x - from.x - dx).abs() < 0.02);
            assert!((to.y - from.y).abs() < 0.02);
        }
        app.simple_nudge(0, -1);
        assert!(app.document.sets[0].positions[0].y < before[0].y - dy * 0.5);
        let json = app.document.to_json().expect("json");
        let loaded = drill_core::Document::from_json(&json).expect("reload");
        assert_eq!(loaded.sets[0].positions, app.document.sets[0].positions);
        assert!(app.history.undo(&mut app.document));
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.sets[0].positions, before);
    }

    #[test]
    fn a_held_count_does_not_nudge_or_add_or_clear() {
        let mut app = moved_pair();
        app.navigate_to_set(0);
        app.nav_glide.settle();
        app.simple_step_beat(2);
        app.nav_glide.settle();
        assert!(app.simple_holding_a_count());
        let positions = app.document.clone();
        let revision = app.history.revision();
        let people = app.document.performers.len();
        app.replace_selection(std::iter::once(0).collect());
        app.simple_nudge(1, 0);
        app.simple_add_beside();
        app.simple_clear_move();
        assert_eq!(app.document.sets, positions.sets);
        assert_eq!(app.document.performers.len(), people);
        assert_eq!(app.history.revision(), revision);
    }

    #[test]
    fn add_beside_places_two_steps_to_the_right_and_undoes() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 12.0, y: 16.0 }, true);
        let first = app.document.sets[0].positions[0];
        app.replace_selection(std::iter::once(0).collect());
        app.simple_add_beside();
        assert_eq!(app.document.performers.len(), 2);
        let second = app.document.sets[0].positions[1];
        let (dx, _) = grid_step(&app);
        assert!((second.x - first.x - dx * 2.0).abs() < 0.05);
        assert!((second.y - first.y).abs() < 0.05);
        let json = app.document.to_json().expect("json");
        let loaded = drill_core::Document::from_json(&json).expect("reload");
        assert_eq!(loaded.performers.len(), 2);
        assert_eq!(loaded.sets[0].positions, app.document.sets[0].positions);
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.performers.len(), 1);
        assert_eq!(app.document.sets[0].positions[0], first);
    }

    #[test]
    fn add_beside_steps_the_other_way_at_the_right_edge() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 12.0, y: 16.0 }, true);
        let y = app.document.sets[0].positions[0].y;
        let set_id = app.document.sets[0].id;
        let performer_id = app.document.performers[0].id;
        let edge = app.document.grid.max_x();
        assert!(app.execute_edit(
            Edit::MovePerformers {
                set_id,
                performer_ids: vec![performer_id],
                positions: vec![Point { x: edge, y }],
            },
            "edge",
        ));
        app.replace_selection(std::iter::once(0).collect());
        app.simple_add_beside();
        assert_eq!(app.document.performers.len(), 2);
        let second = app.document.sets[0].positions[1];
        assert!(second.x < edge);
        assert!(second.x >= 0.0);
        assert!(second.y >= 0.0 && second.y <= app.document.grid.max_y());
    }

    #[test]
    fn travel_note_reports_eight_steps_right_and_a_hold() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 10.0, y: 16.0 }, true);
        app.place_performer_at(Point { x: 18.0, y: 16.0 }, true);
        app.duplicate_current_set();
        let (dx, _) = grid_step(&app);
        let origin = app.document.sets[1].positions[0];
        let set_id = app.document.sets[1].id;
        let performer_id = app.document.performers[0].id;
        assert!(app.execute_edit(
            Edit::MovePerformers {
                set_id,
                performer_ids: vec![performer_id],
                positions: vec![Point {
                    x: origin.x + dx * 8.0,
                    y: origin.y,
                }],
            },
            "walk",
        ));
        let document = app.document.clone();
        let revision = app.history.revision();
        let note = simple_travel_note(&app.document, 1, &[]).expect("walk");
        assert_eq!(note.index, 0);
        assert_eq!(note.kind, drill_core::continuity::TravelDirection::Right);
        assert!((note.steps - 8.0).abs() < 0.01);
        app.replace_selection(BTreeSet::new());
        let group = app.simple_travel_caption().expect("group");
        assert!(group.contains('8'), "{group}");
        assert!(group.contains("右"), "{group}");
        assert!(group.contains('1'), "{group}");
        let from_core = drill_core::continuity::performer_continuity(&app.document, 0);
        let segment = from_core
            .iter()
            .find(|segment| segment.from_set == 0)
            .expect("segment");
        assert_eq!(segment.direction_kind, note.kind);
        assert!((segment.distance_steps - note.steps).abs() < 0.01);
        app.replace_selection(std::iter::once(0).collect());
        let caption = app.simple_travel_caption().expect("caption");
        assert!(caption.contains('8'));
        assert!(caption.contains("右"));
        app.replace_selection(std::iter::once(1).collect());
        let hold = simple_travel_note(&app.document, 1, &[1]).expect("hold");
        assert_eq!(hold.kind, drill_core::continuity::TravelDirection::Hold);
        let still = app.simple_travel_caption().expect("still");
        assert!(still.contains("動きません"));
        assert_eq!(app.document, document);
        assert_eq!(app.history.revision(), revision);
    }

    #[test]
    fn clear_move_matches_the_previous_scene_in_one_undo() {
        let mut app = moved_pair();
        app.navigate_to_set(1);
        app.nav_glide.settle();
        let previous = app.document.sets[0].positions.clone();
        let moved = app.document.sets[1].positions.clone();
        assert_ne!(previous, moved);
        assert!(app.simple_can_clear_move());
        app.simple_clear_move();
        assert_eq!(app.document.sets[1].positions, previous);
        assert!(!app.simple_can_clear_move());
        let json = app.document.to_json().expect("json");
        let loaded = drill_core::Document::from_json(&json).expect("reload");
        assert_eq!(loaded.sets[1].positions, previous);
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.sets[1].positions, moved);
        app.navigate_to_set(0);
        app.nav_glide.settle();
        let stayed = app.document.sets[0].positions.clone();
        app.simple_clear_move();
        assert_eq!(app.document.sets[0].positions, stayed);
    }

    #[test]
    fn starting_fresh_keeps_a_named_show_and_empties_the_draft() {
        let dir = std::env::temp_dir().join(format!(
            "drillforge-fresh-named-{}-{}",
            std::process::id(),
            "keep"
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let draft = dir.join("simple-draft.drill.json");
        let named = dir.join("文化祭.drill.json");
        let mut app = empty_simple_app();
        app.simple_mode.draft_path_override = Some(draft.clone());
        app.simple_mode.shows_dir_override = Some(dir.join("shows"));
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        app.current_path = Some(named.clone());
        app.dirty = true;
        assert!(app.simple_start_fresh());
        assert!(app.document.performers.is_empty());
        assert!(app.current_path.is_none());
        let saved = std::fs::read_to_string(&named).expect("named file");
        let named_doc = drill_core::Document::from_json(&saved).expect("named json");
        assert_eq!(named_doc.performers.len(), 1);
        let draft_text = std::fs::read_to_string(&draft).expect("draft");
        let draft_doc = drill_core::Document::from_json(&draft_text).expect("draft json");
        assert!(draft_doc.performers.is_empty());
        assert!(!app.simple_path_sidecar().exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn starting_fresh_keeps_an_unnamed_show_in_the_list() {
        let dir = std::env::temp_dir().join(format!(
            "drillforge-fresh-draft-{}-{}",
            std::process::id(),
            "keep"
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let draft = dir.join("simple-draft.drill.json");
        let shows = dir.join("shows");
        let mut app = empty_simple_app();
        app.simple_mode.draft_path_override = Some(draft.clone());
        app.simple_mode.shows_dir_override = Some(shows.clone());
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        app.dirty = true;
        assert!(app.current_path.is_none());
        assert!(app.simple_start_fresh());
        assert!(app.document.performers.is_empty());
        let kept = shows.join("下書き.drill.json");
        let saved = std::fs::read_to_string(&kept).expect("archived show");
        let archived = drill_core::Document::from_json(&saved).expect("archived json");
        assert_eq!(archived.performers.len(), 1);
        assert_eq!(app.recent_projects.paths().first(), Some(&kept));
        let draft_text = std::fs::read_to_string(&draft).expect("draft");
        let draft_doc = drill_core::Document::from_json(&draft_text).expect("draft json");
        assert!(draft_doc.performers.is_empty());
        assert!(app.simple_mode.saved_note_until.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn arrow_keys_step_with_the_screen_and_toward_the_audience() {
        assert_eq!(simple_arrow_step(egui::Key::ArrowLeft), Some((-1, 0)));
        assert_eq!(simple_arrow_step(egui::Key::ArrowRight), Some((1, 0)));
        assert_eq!(simple_arrow_step(egui::Key::ArrowUp), Some((0, 1)));
        assert_eq!(simple_arrow_step(egui::Key::ArrowDown), Some((0, -1)));
        assert_eq!(simple_arrow_step(egui::Key::A), None);
    }

    #[test]
    fn renumber_goes_left_to_right_keeps_names_and_is_one_undo() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 30.0, y: 20.0 }, true);
        app.place_performer_at(Point { x: 10.0, y: 20.0 }, true);
        app.place_performer_at(Point { x: 20.0, y: 10.0 }, true);
        app.replace_selection(std::iter::once(0).collect());
        app.simple_rename_selected("山田");
        assert_eq!(app.document.performers[0].label, "1 山田");
        let before = app.document.clone();
        app.simple_sync_history_notes();
        app.simple_renumber();
        assert_eq!(app.document.performers[1].label, "1");
        assert_eq!(app.document.performers[2].label, "2");
        assert_eq!(app.document.performers[0].label, "3 山田");
        app.simple_sync_history_notes();
        let lines = app.simple_history_lines();
        assert!(
            lines.iter().any(|line| line.contains("番号を振る")),
            "{lines:?}"
        );
        let json = app.document.to_json().expect("json");
        let loaded = drill_core::Document::from_json(&json).expect("reload");
        assert_eq!(loaded.performers[0].label, "3 山田");
        assert_eq!(loaded.performers[1].label, "1");
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.performers[0].label, before.performers[0].label);
        assert_eq!(app.document.performers[1].label, before.performers[1].label);
        assert_eq!(app.document.performers[2].label, before.performers[2].label);
        app.simple_renumber();
        assert!(!app.history.can_redo());
    }

    #[test]
    fn a_name_without_a_number_survives_renumbering() {
        assert_eq!(simple_kept_name("山田"), "山田");
        assert_eq!(simple_kept_name("1 山田"), "山田");
        assert_eq!(simple_kept_name("4"), "");
    }

    #[test]
    fn crossing_paths_warn_and_a_long_walk_names_the_farthest_person() {
        let mut app = empty_simple_app();
        app.simple_seed_hint_picture();
        let document = app.document.clone();
        let revision = app.history.revision();
        let caution = simple_caution(&app.document, 1, &mut app.clinic_scratch);
        assert!(caution.collisions.len() >= 2, "{:?}", caution.collisions);
        let stride = caution.long_stride.expect("long walk");
        assert_eq!(stride.index, 0);
        assert!(stride.steps > f32::from(stride.counts));
        assert_eq!(stride.counts, 8);
        app.replace_selection(caution.collisions.into_iter().collect());
        assert!(app.selected.len() >= 2);
        assert_eq!(app.document, document);
        assert_eq!(app.history.revision(), revision);
        let parallel = {
            let mut quiet = empty_simple_app();
            quiet.place_performer_at(Point { x: 10.0, y: 16.0 }, true);
            quiet.place_performer_at(Point { x: 18.0, y: 16.0 }, true);
            quiet.duplicate_current_set();
            let (dx, _) = grid_step(&quiet);
            let set_id = quiet.document.sets[1].id;
            let ids: Vec<_> = quiet.document.performers.iter().map(|p| p.id).collect();
            let start = quiet.document.sets[1].positions.clone();
            assert!(quiet.execute_edit(
                Edit::MovePerformers {
                    set_id,
                    performer_ids: ids,
                    positions: vec![
                        Point {
                            x: start[0].x + dx * 4.0,
                            y: start[0].y,
                        },
                        Point {
                            x: start[1].x + dx * 4.0,
                            y: start[1].y,
                        },
                    ],
                },
                "parallel",
            ));
            simple_caution(&quiet.document, 1, &mut quiet.clinic_scratch)
        };
        assert!(parallel.collisions.is_empty());
        assert!(parallel.long_stride.is_none());
    }

    #[test]
    fn scene_memo_lists_the_walk_and_a_hold_without_writing() {
        let mut app = empty_simple_app();
        app.simple_seed_hint_picture();
        let document = app.document.clone();
        let revision = app.history.revision();
        let memo = simple_scene_memo(&app.document, 1, app.locale);
        assert!(memo.contains("1 → 2"), "{memo}");
        assert!(memo.contains('8'), "{memo}");
        assert!(memo.contains("山田"), "{memo}");
        assert!(memo.contains("歩"), "{memo}");
        assert!(memo.contains("動きません") || memo.contains("右"), "{memo}");
        assert_eq!(app.document, document);
        assert_eq!(app.history.revision(), revision);
        let json = app.document.to_json().expect("json");
        assert!(drill_core::Document::from_json(&json).is_ok());
    }

    #[test]
    fn undo_names_the_line_up_and_redo_keeps_that_name() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 10.0, y: 16.0 }, true);
        app.place_performer_at(Point { x: 20.0, y: 24.0 }, true);
        app.simple_sync_history_notes();
        app.simple_line_up();
        app.simple_sync_history_notes();
        let undo = app.simple_history_lines();
        assert!(undo.iter().any(|line| line.contains("横一列")), "{undo:?}");
        assert!(app.history.undo(&mut app.document));
        app.simple_sync_history_notes();
        let redo = app.simple_history_lines();
        assert!(redo.iter().any(|line| line.contains("横一列")), "{redo:?}");
        assert!(app.history.redo(&mut app.document));
        app.simple_sync_history_notes();
        let again = app.simple_history_lines();
        assert!(
            again.iter().any(|line| line.contains("横一列")),
            "{again:?}"
        );
        assert!(
            !again.iter().any(|line| line.contains("やり直す")),
            "{again:?}"
        );
    }

    fn on_field(app: &DrillApp, points: &[Point]) -> bool {
        let grid = &app.document.grid;
        points.iter().all(|point| {
            (0.0..=grid.max_x() + 0.05).contains(&point.x)
                && (0.0..=grid.max_y() + 0.05).contains(&point.y)
        })
    }

    fn span(points: &[Point]) -> (f32, f32) {
        let (min_x, max_x, min_y, max_y) = simple_bounds(points);
        (max_x - min_x, max_y - min_y)
    }

    #[test]
    fn swapping_ends_flips_front_and_back_in_one_undo() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 16.0, y: 8.0 }, true);
        app.place_performer_at(Point { x: 16.0, y: 24.0 }, true);
        app.place_performer_at(Point { x: 28.0, y: 16.0 }, true);
        let parked = app.document.sets[0].positions[2];
        let before = app.document.sets[0].positions.clone();
        app.replace_selection([0_usize, 1].into_iter().collect());
        app.simple_sync_history_notes();
        app.simple_swap_ends();
        let after = app.document.sets[0].positions.clone();
        let front_before = before
            .iter()
            .take(2)
            .enumerate()
            .min_by(|(_, a), (_, b)| a.y.total_cmp(&b.y))
            .map(|(index, _)| index)
            .expect("front");
        let back_after = after
            .iter()
            .take(2)
            .enumerate()
            .max_by(|(_, a), (_, b)| a.y.total_cmp(&b.y))
            .map(|(index, _)| index)
            .expect("back");
        assert_eq!(front_before, back_after);
        for (from, to) in before.iter().take(2).zip(after.iter().take(2)) {
            assert!((from.x - to.x).abs() < 0.05);
        }
        assert_eq!(after[2], parked);
        assert!(on_field(&app, &after));
        assert!(app.document.sets[0].shape.is_none());
        app.simple_sync_history_notes();
        let lines = app.simple_history_lines();
        assert!(lines.iter().any(|line| line.contains("前後")), "{lines:?}");
        let json = app.document.to_json().expect("json");
        let loaded = drill_core::Document::from_json(&json).expect("reload");
        assert_eq!(loaded.sets[0].positions, after);
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.sets[0].positions, before);
    }

    #[test]
    fn a_quarter_turn_makes_a_line_into_a_file_and_stays_on_the_field() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 14.0, y: 20.0 }, true);
        app.place_performer_at(Point { x: 20.0, y: 20.0 }, true);
        app.place_performer_at(Point { x: 26.0, y: 20.0 }, true);
        app.place_performer_at(Point { x: 32.0, y: 20.0 }, true);
        let before = app.document.sets[0].positions.clone();
        let (wide, flat) = span(&before);
        assert!(wide > flat + 4.0);
        app.simple_turn_right();
        let once = app.document.sets[0].positions.clone();
        let (narrow, tall) = span(&once);
        assert!(tall > narrow + 4.0, "a right turn stands the line up");
        assert!(on_field(&app, &once));
        assert!(app.document.sets[0].shape.is_none());
        app.simple_turn_right();
        let (wide_again, flat_again) = span(&app.document.sets[0].positions);
        assert!(
            wide_again > flat_again + 4.0,
            "the next turn lays them down"
        );
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.sets[0].positions, once);

        let mut front = empty_simple_app();
        front.place_performer_at(Point { x: 8.0, y: 4.0 }, true);
        front.place_performer_at(Point { x: 16.0, y: 4.0 }, true);
        front.place_performer_at(Point { x: 24.0, y: 4.0 }, true);
        front.place_performer_at(Point { x: 32.0, y: 4.0 }, true);
        front.simple_turn_right();
        assert!(on_field(&front, &front.document.sets[0].positions));
        let (_, tall_front) = span(&front.document.sets[0].positions);
        assert!(tall_front > 4.0);
    }

    #[test]
    fn an_arc_puts_the_middle_toward_the_audience() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 8.0, y: 28.0 }, true);
        app.place_performer_at(Point { x: 16.0, y: 10.0 }, true);
        app.place_performer_at(Point { x: 28.0, y: 22.0 }, true);
        app.place_performer_at(Point { x: 36.0, y: 14.0 }, true);
        app.place_performer_at(Point { x: 22.0, y: 30.0 }, true);
        let before = app.document.sets[0].positions.clone();
        app.replace_selection([0_usize, 1, 2, 3].into_iter().collect());
        app.simple_arc_up();
        let positions = &app.document.sets[0].positions;
        assert_eq!(positions[4], before[4]);
        assert!(positions[0].x + 1.0 < positions[3].x);
        let middle = positions[1].y.min(positions[2].y);
        assert!(positions[0].y > middle + 1.0);
        assert!(positions[3].y > middle + 1.0);
        assert!(on_field(&app, positions));
        assert!(app.document.sets[0].shape.is_none());
        let json = app.document.to_json().expect("json");
        let loaded = drill_core::Document::from_json(&json).expect("reload");
        assert_eq!(loaded.sets[0].positions, *positions);
        assert_eq!(loaded.performers.len(), 5);
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.sets[0].positions, before);
    }

    #[test]
    fn spacing_opens_and_closes_by_about_one_step() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 12.0, y: 18.0 }, true);
        app.place_performer_at(Point { x: 18.0, y: 18.0 }, true);
        app.place_performer_at(Point { x: 24.0, y: 18.0 }, true);
        let before = app.document.sets[0].positions.clone();
        let (start_w, _) = span(&before);
        let step = grid_step(&app).0;
        app.simple_sync_history_notes();
        app.simple_change_spacing(true);
        let wider = app.document.sets[0].positions.clone();
        let (wide, _) = span(&wider);
        assert!(wide > start_w + step * 0.6, "{start_w} -> {wide}");
        assert!(on_field(&app, &wider));
        app.simple_sync_history_notes();
        let lines = app.simple_history_lines();
        assert!(
            lines.iter().any(|line| line.contains("ひろげる")),
            "{lines:?}"
        );
        app.simple_change_spacing(false);
        let closed = &app.document.sets[0].positions;
        let (back, _) = span(closed);
        assert!(back + step * 0.4 < wide, "{wide} -> {back}");
        assert!(on_field(&app, closed));
        for _ in 0..6 {
            app.simple_change_spacing(false);
        }
        let tight = &app.document.sets[0].positions;
        let (tight_w, _) = span(tight);
        assert!(tight_w + 0.05 >= step, "tightening stops before a pile");
        assert!(on_field(&app, tight));
        let mut edge = empty_simple_app();
        edge.place_performer_at(Point { x: 2.0, y: 20.0 }, true);
        edge.place_performer_at(
            Point {
                x: edge.document.grid.max_x() - 1.0,
                y: 20.0,
            },
            true,
        );
        edge.simple_change_spacing(true);
        assert!(on_field(&edge, &edge.document.sets[0].positions));
    }

    #[test]
    fn renaming_a_scene_is_one_undo_and_the_json_keeps_the_name() {
        assert!(simple_stock_scene_name("セット 1"));
        assert!(simple_stock_scene_name("Set 2"));
        assert!(!simple_stock_scene_name("サビ"));
        assert_eq!(simple_scene_name_draft("セット 1"), "");
        assert_eq!(simple_scene_name_text("  オープニング  "), "オープニング");
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 12.0, y: 16.0 }, true);
        app.place_performer_at(Point { x: 20.0, y: 16.0 }, true);
        assert_eq!(app.document.sets[0].name, "セット 1");
        let positions = app.document.sets[0].positions.clone();
        let labels: Vec<_> = app
            .document
            .performers
            .iter()
            .map(|performer| performer.label.clone())
            .collect();
        app.simple_sync_history_notes();
        app.simple_rename_scene("  サビ  ");
        assert_eq!(app.document.sets[0].name, "サビ");
        assert_eq!(app.document.sets[0].positions, positions);
        assert_eq!(
            app.document
                .performers
                .iter()
                .map(|performer| performer.label.as_str())
                .collect::<Vec<_>>(),
            labels.iter().map(String::as_str).collect::<Vec<_>>()
        );
        app.simple_sync_history_notes();
        let lines = app.simple_history_lines();
        assert!(
            lines.iter().any(|line| line.contains("場面の名前")),
            "{lines:?}"
        );
        let memo = simple_scene_memo(&app.document, 0, app.locale);
        assert!(memo.contains("サビ"), "{memo}");
        let json = app.document.to_json().expect("json");
        let loaded = drill_core::Document::from_json(&json).expect("reload");
        assert_eq!(loaded.sets[0].name, "サビ");
        assert_eq!(loaded.sets[0].positions, positions);
        assert_eq!(loaded.schema_version, app.document.schema_version);
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.sets[0].name, "セット 1");
        app.duplicate_current_set();
        app.simple_rename_scene("サビ");
        assert_eq!(app.document.sets[1].name, "サビ");
        assert_ne!(app.document.sets[0].name, "サビ");
        app.navigate_to_set(1);
        app.nav_glide.settle();
        app.simple_rename_scene("");
        assert_eq!(app.document.sets[1].name, "セット 2");
        let long = "あ".repeat(30);
        app.simple_rename_scene(&long);
        assert_eq!(app.document.sets[1].name.chars().count(), 20);
    }

    #[test]
    fn side_labels_sit_on_the_front_sideline() {
        let grid = drill_core::GridConfig::default();
        let (left, right) = simple_side_anchors(&grid);
        assert!(left.x + 1.0 < grid.width * 0.5);
        assert!(right.x > grid.width * 0.5 + 1.0);
        assert!(left.y.abs() < 0.01 && right.y.abs() < 0.01);
        assert!(right.x <= grid.max_x() + 0.01);
    }

    #[test]
    fn place_line_counts_steps_from_center_and_the_front() {
        let mut app = empty_simple_app();
        let (step, _) = grid_step(&app);
        let mid_x = app.document.grid.width * 0.5;
        let on_center = app.document.grid.snap(Point {
            x: mid_x,
            y: step * 8.0,
        });
        app.place_performer_at(on_center, true);
        app.replace_selection(std::iter::once(0).collect());
        let document = app.document.clone();
        let revision = app.history.revision();
        let middle = app.simple_place_caption().expect("center");
        assert!(middle.contains("中央"), "{middle}");
        assert!(middle.contains("前から"), "{middle}");
        assert!(middle.contains('8'), "{middle}");
        assert!(!middle.contains("右へ"), "{middle}");
        assert!(!middle.contains("左へ"), "{middle}");
        assert!(middle.contains("いまの場所"), "{middle}");
        assert_eq!(app.document, document);
        assert_eq!(app.history.revision(), revision);
        let right_of_center = app.document.grid.snap(Point {
            x: on_center.x + step * 4.0,
            y: on_center.y,
        });
        app.place_performer_at(right_of_center, true);
        app.replace_selection(std::iter::once(1).collect());
        let right = app.simple_place_caption().expect("right");
        assert!(right.contains("右へ"), "{right}");
        assert!(right.contains("前から"), "{right}");
        assert!(app.document.sets[0].positions[1].x > mid_x + step * 2.0);
        let left_of_center = app.document.grid.snap(Point {
            x: on_center.x - step * 4.0,
            y: on_center.y,
        });
        let set_id = app.document.sets[0].id;
        let performer_id = app.document.performers[0].id;
        assert!(app.execute_edit(
            Edit::MovePerformers {
                set_id,
                performer_ids: vec![performer_id],
                positions: vec![left_of_center],
            },
            "left",
        ));
        let left = simple_place_line(&app.document.grid, left_of_center, app.locale).expect("left");
        assert!(left.contains("左へ"), "{left}");
        assert!(left.contains("前から"), "{left}");
        let quiet = simple_place_line(&app.document.grid, left_of_center, app.locale);
        assert_eq!(quiet.as_deref(), Some(left.as_str()));
        assert_eq!(app.document.sets[0].positions[0], left_of_center);
        assert!(left_of_center.x + step * 2.0 < mid_x);
    }

    #[test]
    fn a_block_faces_the_audience_and_undoes() {
        let mut app = empty_simple_app();
        let spots = [
            Point { x: 10.0, y: 30.0 },
            Point { x: 40.0, y: 12.0 },
            Point { x: 22.0, y: 24.0 },
            Point { x: 48.0, y: 18.0 },
            Point { x: 16.0, y: 36.0 },
            Point { x: 34.0, y: 8.0 },
        ];
        for spot in spots {
            app.place_performer_at(spot, true);
        }
        let before = app.document.sets[0].positions.clone();
        let labels: Vec<_> = app
            .document
            .performers
            .iter()
            .map(|performer| performer.label.clone())
            .collect();
        app.simple_sync_history_notes();
        app.simple_block_up();
        let positions = app.document.sets[0].positions.clone();
        let (step_x, step_y) = grid_step(&app);
        for index in 0..3 {
            assert!((positions[index].y - positions[0].y).abs() < 0.05);
            assert!((positions[index + 3].y - positions[3].y).abs() < 0.05);
        }
        assert!(positions[0].y + step_y < positions[3].y);
        assert!(positions[0].x + step_x < positions[1].x);
        assert!(positions[1].x + step_x < positions[2].x);
        assert!(positions[3].x + step_x < positions[4].x);
        assert!((positions[1].x - positions[0].x - step_x * 2.0).abs() < 0.08);
        assert!((positions[3].y - positions[0].y - step_y * 2.0).abs() < 0.08);
        assert!(on_field(&app, &positions));
        assert!(app.document.sets[0].shape.is_none());
        assert_eq!(
            app.document
                .performers
                .iter()
                .map(|performer| performer.label.as_str())
                .collect::<Vec<_>>(),
            labels.iter().map(String::as_str).collect::<Vec<_>>()
        );
        app.simple_sync_history_notes();
        let lines = app.simple_history_lines();
        assert!(
            lines.iter().any(|line| line.contains("かたまり")),
            "{lines:?}"
        );
        let json = app.document.to_json().expect("json");
        let loaded = drill_core::Document::from_json(&json).expect("reload");
        assert_eq!(loaded.sets[0].positions, positions);
        assert_eq!(loaded.schema_version, app.document.schema_version);
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.sets[0].positions, before);

        let mut five = empty_simple_app();
        for spot in spots.iter().take(5) {
            five.place_performer_at(*spot, true);
        }
        five.simple_block_up();
        let five_points = &five.document.sets[0].positions;
        assert!((five_points[0].y - five_points[2].y).abs() < 0.05);
        assert!(five_points[0].y + 0.5 < five_points[3].y);
        assert!((five_points[3].y - five_points[4].y).abs() < 0.05);
        let front_mid = (five_points[0].x + five_points[2].x) * 0.5;
        let back_mid = (five_points[3].x + five_points[4].x) * 0.5;
        assert!((front_mid - back_mid).abs() < 0.2);
        assert!(on_field(&five, five_points));

        let mut part = empty_simple_app();
        for spot in spots {
            part.place_performer_at(spot, true);
        }
        let parked = part.document.sets[0].positions[4..].to_vec();
        part.replace_selection([0_usize, 1, 2, 3].into_iter().collect());
        part.simple_block_up();
        assert_eq!(&part.document.sets[0].positions[4..], parked.as_slice());
        assert!(
            (part.document.sets[0].positions[0].y - part.document.sets[0].positions[1].y).abs()
                < 0.05
        );
        assert!(part.document.sets[0].positions[0].y + 0.5 < part.document.sets[0].positions[2].y);

        let mut edge = empty_simple_app();
        edge.place_performer_at(Point { x: 8.0, y: 0.0 }, true);
        edge.place_performer_at(Point { x: 14.0, y: 0.0 }, true);
        edge.place_performer_at(Point { x: 20.0, y: 0.0 }, true);
        edge.place_performer_at(Point { x: 26.0, y: 0.0 }, true);
        edge.simple_block_up();
        let edged = &edge.document.sets[0].positions;
        assert!(on_field(&edge, edged));
        let distinct = edged
            .iter()
            .map(|point| {
                (
                    (point.x * 100.0).round() as i32,
                    (point.y * 100.0).round() as i32,
                )
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(distinct.len(), 4);
    }

    #[test]
    fn swapping_two_people_is_one_undo() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 12.0, y: 16.0 }, true);
        app.place_performer_at(Point { x: 20.0, y: 16.0 }, true);
        app.place_performer_at(Point { x: 28.0, y: 24.0 }, true);
        let before = app.document.sets[0].positions.clone();
        app.replace_selection([0_usize, 2].into_iter().collect());
        app.simple_sync_history_notes();
        app.simple_swap_pair();
        assert_eq!(app.document.sets[0].positions[0], before[2]);
        assert_eq!(app.document.sets[0].positions[2], before[0]);
        assert_eq!(app.document.sets[0].positions[1], before[1]);
        app.simple_sync_history_notes();
        let lines = app.simple_history_lines();
        assert!(
            lines.iter().any(|line| line.contains("入れ替え")),
            "{lines:?}"
        );
        let json = app.document.to_json().expect("json");
        let loaded = drill_core::Document::from_json(&json).expect("reload");
        assert_eq!(loaded.sets[0].positions, app.document.sets[0].positions);
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.sets[0].positions, before);
        app.replace_selection(std::iter::once(1).collect());
        app.simple_swap_pair();
        assert_eq!(app.document.sets[0].positions, before);
    }

    #[test]
    fn restoring_chosen_people_leaves_the_others_where_they_moved() {
        let mut app = moved_pair();
        let set_id = app.document.sets[1].id;
        let other = app.document.performers[1].id;
        let other_from = app.document.sets[1].positions[1];
        let other_to = Point {
            x: other_from.x + 6.0,
            y: other_from.y,
        };
        assert!(app.execute_edit(
            Edit::MovePerformers {
                set_id,
                performer_ids: vec![other],
                positions: vec![other_to],
            },
            "other",
        ));
        app.navigate_to_set(1);
        app.nav_glide.settle();
        let previous = app.document.sets[0].positions.clone();
        let moved = app.document.sets[1].positions.clone();
        assert_ne!(moved[0], previous[0]);
        assert_ne!(moved[1], previous[1]);
        app.replace_selection(std::iter::once(0).collect());
        assert!(app.simple_can_restore_selected());
        app.simple_sync_history_notes();
        app.simple_restore_selected();
        assert_eq!(app.document.sets[1].positions[0], previous[0]);
        assert_eq!(app.document.sets[1].positions[1], other_to);
        assert!(!app.simple_can_restore_selected());
        app.simple_sync_history_notes();
        let lines = app.simple_history_lines();
        assert!(
            lines.iter().any(|line| line.contains("この人を戻す")),
            "{lines:?}"
        );
        let json = app.document.to_json().expect("json");
        let loaded = drill_core::Document::from_json(&json).expect("reload");
        assert_eq!(loaded.sets[1].positions[0], previous[0]);
        assert_eq!(loaded.sets[1].positions[1], other_to);
        assert!(app.history.undo(&mut app.document));
        app.simple_sync_history_notes();
        assert_eq!(app.document.sets[1].positions, moved);

        app.replace_selection([0_usize, 1].into_iter().collect());
        assert!(!app.simple_can_restore_selected());
        app.simple_restore_selected();
        assert_eq!(app.document.sets[1].positions, moved);
        app.navigate_to_set(0);
        app.nav_glide.settle();
        app.replace_selection(std::iter::once(0).collect());
        assert!(!app.simple_can_restore_selected());

        app.navigate_to_set(1);
        app.nav_glide.settle();
        app.place_performer_at(Point { x: 36.0, y: 20.0 }, true);
        let stayed = app.document.sets[1].positions[2];
        app.replace_selection([0_usize, 1].into_iter().collect());
        assert!(app.simple_can_restore_selected());
        app.simple_restore_selected();
        assert_eq!(app.document.sets[1].positions[0], previous[0]);
        assert_eq!(app.document.sets[1].positions[1], previous[1]);
        assert_eq!(app.document.sets[1].positions[2], stayed);
        app.simple_sync_history_notes();
        let many = app.simple_history_lines();
        assert!(
            many.iter().any(|line| line.contains("選んだ人を戻す")),
            "{many:?}"
        );
    }
}
