//! "Simple Mode" (かんたんモード): a reduced, guided UI aimed at first-time
//! high-school students who just want to build a formation, move performers,
//! and play it back without learning the full desktop toolset.
//!
//! Invariant: every mutation in this module goes through `DrillApp::execute_edit`
//! (directly or via existing helpers like `commit_layout`/`commit_shape`), the
//! same as the full UI. This module never touches `self.document` fields
//! directly. See `ui_qa.rs` for the guard test that scans this file too.
//!
//! Previous-scene ghosts are a view preference, not drill content. The eye
//! toggle sits in the set-dot overlay (same row, no extra layout line) and
//! remembers on/off per show file (see `previous_ghost_store_path`). The
//! drill JSON, undo stack, and schema do not change. A show with no saved
//! choice starts off.
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
    /// Show-relative seconds the click should line up with. The silence asset
    /// is only a carrier; clicks are scheduled on the output timeline.
    anchor_seconds: f64,
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
            anchor_seconds: 0.0,
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
                        output.seek(self.anchor_source_frames());
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

    /// Clicks from `show_seconds` (0 at count 0). A missing device records the
    /// error on the worker and leaves the drill free to keep moving.
    pub fn start_at(&mut self, bpm: f32, show_seconds: f64) {
        let bpm = bpm.clamp(30.0, 300.0);
        self.anchor_seconds = if show_seconds.is_finite() {
            show_seconds.max(0.0)
        } else {
            0.0
        };
        if (bpm - self.bpm).abs() > f32::EPSILON {
            self.bpm = bpm;
            if self.output.is_some() {
                self.start_when_ready = true;
                self.running = false;
                self.begin_schedule();
                return;
            }
        } else if self.running {
            if let Some(output) = &self.output {
                output.seek(self.anchor_source_frames());
                output.play();
            }
            return;
        }
        self.start();
    }

    /// Source frames of the 48 kHz silence carrier. `seek` converts those into
    /// the output timeline the click schedule is written on.
    fn anchor_source_frames(&self) -> u64 {
        const SOURCE_RATE: f64 = 48_000.0;
        if self.anchor_seconds.is_finite() && self.anchor_seconds > 0.0 {
            (self.anchor_seconds * SOURCE_RATE).round() as u64
        } else {
            0
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
    /// Where everyone stands in this scene. Nothing is written.
    place_sheet_open: bool,
    place_sheet_copied: bool,
    /// Every scene's places, to paste into one message. Nothing is written.
    show_sheet_open: bool,
    show_sheet_copied: bool,
    /// One person's places and walks. Nothing is written until they save a file.
    path_sheet_open: bool,
    path_sheet_for: Option<drill_core::PerformerId>,
    path_sheet_copied: bool,
    /// Every scene's walks, to print and hand out.
    walk_sheet_open: bool,
    walk_sheet_copied: bool,
    walk_sheet_saved: bool,
    walk_sheet_failed: bool,
    /// One person to watch. Others are washed out. Session only, not the file.
    focus: Option<drill_core::PerformerId>,
    /// People sitting out of today's rehearsal. The drill file still has them.
    away: std::collections::BTreeSet<drill_core::PerformerId>,
    /// Watch only the move of the current scene, then start that move again.
    scene_loop: bool,
    /// Draft for the current scene's name. A stock "セット 1" shows as empty.
    scene_name: String,
    scene_name_for: Option<usize>,
    /// Draw the previous scene's formation as faint gray dots. Remembered
    /// per show file, outside the drill JSON. Missing entry means off.
    show_previous: bool,
    /// Show-file key `show_previous` was loaded for. `None` until the first
    /// lookup, so opening another show reads that show's choice.
    previous_key: Option<String>,
    /// `previous_key` names the unnamed draft rather than `current_path`.
    previous_key_is_draft: bool,
    /// `draft_path_override` captured when `previous_key` was last computed.
    previous_key_draft: Option<std::path::PathBuf>,
    previous_store_loaded: bool,
    previous_shows: std::collections::BTreeMap<String, bool>,
    /// Tests redirect the preference file away from real app data.
    previous_store_override: Option<std::path::PathBuf>,
    /// One mesh for every previous-scene ghost. Reused across frames so a
    /// hundred performers do not allocate a new dot list each paint.
    ghost_mesh: Option<std::sync::Arc<egui::Mesh>>,
    /// First-run pictogram hints, one step at a time. Session only.
    hint_closed: bool,
    hint_place_done: bool,
    hint_move_done: bool,
    hint_next_done: bool,
    hint_play_done: bool,
    hint_pulse_only: bool,
    hint_phase_at: Option<Instant>,
    last_input_at: Option<Instant>,
    /// Green check after a keep (autosave or named write).
    check_until: Option<Instant>,
    /// Gentle shake + orange outline after a blocked action.
    fail_until: Option<Instant>,
    /// Trash long-press. Commits at one second, never a dialog.
    delete_hold: Option<Instant>,
    /// Debounce window for the automatic draft write.
    draft_debounce_at: Option<Instant>,
    draft_debounce_cursor: Option<usize>,
    /// Last frame's action tiles, so the ghost finger can point at them.
    action_place: Option<egui::Rect>,
    action_move: Option<egui::Rect>,
    action_next: Option<egui::Rect>,
    action_play: Option<egui::Rect>,
    field_card: Option<egui::Rect>,
    /// Union of floating chrome from the last frame, so field clicks miss it.
    overlay_block: Option<egui::Rect>,
    /// Left-right mirror while editing this session. Not part of the drill
    /// file: drafts and named saves write `document` only.
    pub(super) mirror_moves: bool,
    /// Where unpaired partners are heading during a drag. Cleared on release.
    mirror_preview: Vec<Point>,
    /// The song path is on the show, but the file is not on this machine.
    /// The drill still opens. Session-only; the path stays in the file.
    pub(crate) song_missing: bool,
    /// Metronome used when the song is muted (slow) or not loaded.
    metronome: MetronomeState,
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
pub(crate) const SIMPLE_BLUE: Color32 = Color32::from_rgb(20, 96, 200);
const SIMPLE_ORANGE: Color32 = Color32::from_rgb(196, 112, 24);
const SIMPLE_INK: Color32 = Color32::from_rgb(28, 32, 40);
const SIMPLE_CARD: Color32 = Color32::WHITE;
const SIMPLE_MOTION_SECS: f32 = 0.26;
const SIMPLE_OK: Color32 = Color32::from_rgb(46, 158, 92);
const SIMPLE_WARN: Color32 = Color32::from_rgb(232, 140, 48);
const SIMPLE_OK_SECS: f32 = 0.6;
const SIMPLE_FAIL_SECS: f32 = 0.3;
const SIMPLE_HIT: f32 = 48.0;
const SIMPLE_TILE: f32 = 52.0;
const SIMPLE_LABEL: f32 = 12.0;
const SIMPLE_PLAY_TILE: f32 = 60.0;
const SIMPLE_HINT_MOVE_SECS: f32 = 0.8;
const SIMPLE_HINT_REST_SECS: f32 = 2.0;
const SIMPLE_HINT_IDLE_SECS: f32 = 10.0;
const SIMPLE_HINT_MAX_REPEATS: u8 = 3;
const SIMPLE_DELETE_HOLD_SECS: f32 = 1.0;
const SIMPLE_AUTOSAVE_MS: u64 = 400;
/// School-band 4/4. One count is one beat.
const SIMPLE_BEATS_PER_MEASURE: u16 = 4;
const SIMPLE_TEMPO_STEP: i32 = 4;
const SIMPLE_TEMPO_MIN: i32 = 40;
const SIMPLE_TEMPO_MAX: i32 = 208;
/// Same reach as the simple-field pointer. Ghost dots are not in this test.
const SIMPLE_DOT_HIT_RADIUS: f32 = 18.0;
const PREVIOUS_GHOST_RADIUS: f32 = 2.25;
/// A short hop is not worth a line through an 80-person block.
const PATH_LINE_STEPS: f32 = 4.0;
const PREVIOUS_GHOST_SEGMENTS: usize = 8;
const PREVIOUS_TOGGLE_HEIGHT: f32 = 48.0;
/// One nudge of the shared song offset. Small enough to fix a late entrance
/// without jumping a whole beat.
const SIMPLE_SONG_NUDGE_SECONDS: f32 = 0.05;

fn previous_ghost_color() -> Color32 {
    Color32::from_rgba_unmultiplied(90, 98, 112, 56)
}

/// Identical pictures have no ghost. A fraction of a step is the same spot.
fn previous_ghost_separated(previous: Point, current: Point) -> bool {
    (previous.x - current.x).hypot(previous.y - current.y) > 0.05
}

/// Lines are for a real move, or for the person you are looking at.
fn movement_line_visible(distance: f32, step: f32, selected: bool) -> bool {
    let minimum = if selected {
        0.05
    } else {
        (step * PATH_LINE_STEPS).max(0.05)
    };
    distance > minimum
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DockAction {
    Undo,
    Redo,
    Open,
    More,
}

pub(crate) struct PictogramTile<'a> {
    pub(crate) icon: PhoneIcon,
    pub(crate) label: &'a str,
    pub(crate) tip: &'a str,
    pub(crate) width: f32,
    pub(crate) enabled: bool,
    pub(crate) active: bool,
    pub(crate) pulse: bool,
    pub(crate) filled: bool,
    pub(crate) compact: bool,
    pub(crate) time: f32,
    /// When set, every tile stays `width` instead of growing to fit the label.
    pub(crate) lock_width: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum PhoneIcon {
    Undo,
    Redo,
    Open,
    More,
    Play,
    Stop,
    Pause,
    Next,
    Trash,
    Person,
    PersonPlus,
    PersonMove,
    SheetsNext,
    Check,
    Lock,
    Close,
    ChevronDown,
    Tools,
    Mirror,
    Note,
    NoteOff,
    Minus,
    Plus,
    Chart,
    Book,
    Counts,
    Sheet,
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
    paint_phone_icon_at(painter, center, icon, color, 1.0);
}

pub(crate) fn paint_phone_icon_at(
    painter: &egui::Painter,
    center: Pos2,
    icon: PhoneIcon,
    color: Color32,
    scale: f32,
) {
    let s = scale.max(0.4);
    let stroke = Stroke::new((2.0 * s).max(1.15), color);
    let p = |x: f32, y: f32| center + egui::Vec2::new(x * s, y * s);
    match icon {
        PhoneIcon::Undo => {
            // Head on the left, pointing left. Redo is the mirror.
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
        PhoneIcon::Redo => {
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
                painter.circle_filled(p(x, y), 2.2 * s, color);
            }
        }
        PhoneIcon::Play => {
            painter.add(egui::Shape::convex_polygon(
                vec![p(-5.0, -7.0), p(-5.0, 7.0), p(8.0, 0.0)],
                color,
                Stroke::NONE,
            ));
        }
        PhoneIcon::Stop => {
            painter.rect_filled(
                egui::Rect::from_center_size(center, egui::Vec2::splat(14.0 * s)),
                2.0 * s,
                color,
            );
        }
        PhoneIcon::Pause => {
            painter.rect_filled(
                egui::Rect::from_center_size(p(-3.5, 0.0), egui::Vec2::new(3.0 * s, 14.0 * s)),
                1.0,
                color,
            );
            painter.rect_filled(
                egui::Rect::from_center_size(p(3.5, 0.0), egui::Vec2::new(3.0 * s, 14.0 * s)),
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
        PhoneIcon::Person => paint_person_pictogram(painter, center, s, color),
        PhoneIcon::PersonPlus => {
            paint_person_pictogram(painter, p(-1.2, -1.2), s * 0.82, color);
            painter.line_segment([p(4.2, 4.2), p(4.2, 10.4)], stroke);
            painter.line_segment([p(1.1, 7.3), p(7.3, 7.3)], stroke);
        }
        PhoneIcon::PersonMove => {
            paint_person_pictogram(painter, center, s * 0.72, color);
            paint_cardinal_arrows(painter, center, s, color);
        }
        PhoneIcon::SheetsNext => {
            let back = egui::Rect::from_min_max(p(-8.0, -6.0), p(2.0, 6.0));
            let front = egui::Rect::from_min_max(p(-5.0, -8.0), p(5.0, 4.0));
            painter.rect_stroke(back, 2.0 * s, stroke, StrokeKind::Inside);
            painter.rect_filled(front, 2.0 * s, SIMPLE_CARD);
            painter.rect_stroke(front, 2.0 * s, stroke, StrokeKind::Inside);
            painter.line_segment([p(7.0, -3.0), p(11.0, 0.0)], stroke);
            painter.line_segment([p(7.0, 3.0), p(11.0, 0.0)], stroke);
        }
        PhoneIcon::Check => {
            painter.line_segment([p(-6.0, 1.0), p(-1.5, 6.0)], stroke);
            painter.line_segment([p(-1.5, 6.0), p(7.0, -6.0)], stroke);
        }
        PhoneIcon::Lock => {
            painter.circle_stroke(p(0.0, -4.5), 3.4 * s, stroke);
            painter.rect_filled(
                egui::Rect::from_center_size(p(0.0, 3.0), egui::Vec2::new(12.0 * s, 10.0 * s)),
                2.0 * s,
                color,
            );
        }
        PhoneIcon::Close => {
            painter.line_segment([p(-5.0, -5.0), p(5.0, 5.0)], stroke);
            painter.line_segment([p(5.0, -5.0), p(-5.0, 5.0)], stroke);
        }
        PhoneIcon::ChevronDown => {
            painter.line_segment([p(-6.0, -2.0), p(0.0, 5.0)], stroke);
            painter.line_segment([p(6.0, -2.0), p(0.0, 5.0)], stroke);
        }
        PhoneIcon::Tools => {
            painter.rect_stroke(
                egui::Rect::from_center_size(p(-3.5, -3.5), egui::Vec2::splat(5.0 * s)),
                1.2 * s,
                stroke,
                StrokeKind::Inside,
            );
            painter.rect_stroke(
                egui::Rect::from_center_size(p(3.5, -3.5), egui::Vec2::splat(5.0 * s)),
                1.2 * s,
                stroke,
                StrokeKind::Inside,
            );
            painter.rect_stroke(
                egui::Rect::from_center_size(p(-3.5, 3.5), egui::Vec2::splat(5.0 * s)),
                1.2 * s,
                stroke,
                StrokeKind::Inside,
            );
            painter.rect_filled(
                egui::Rect::from_center_size(p(3.5, 3.5), egui::Vec2::splat(5.0 * s)),
                1.2 * s,
                color,
            );
        }
        PhoneIcon::Mirror => {
            painter.line_segment([p(0.0, -8.0), p(0.0, 8.0)], stroke);
            painter.add(egui::Shape::line(
                vec![p(-3.0, -6.0), p(-8.0, 0.0), p(-3.0, 6.0)],
                stroke,
            ));
            painter.add(egui::Shape::line(
                vec![p(3.0, -6.0), p(8.0, 0.0), p(3.0, 6.0)],
                stroke,
            ));
        }
        PhoneIcon::Note => paint_note_glyph(painter, center, s, color, false),
        PhoneIcon::NoteOff => paint_note_glyph(painter, center, s, color, true),
        PhoneIcon::Minus => {
            painter.line_segment([p(-6.0, 0.0), p(6.0, 0.0)], stroke);
        }
        PhoneIcon::Plus => {
            painter.line_segment([p(-6.0, 0.0), p(6.0, 0.0)], stroke);
            painter.line_segment([p(0.0, -6.0), p(0.0, 6.0)], stroke);
        }
        PhoneIcon::Chart => {
            painter.rect_stroke(
                egui::Rect::from_center_size(center, egui::vec2(16.0 * s, 12.0 * s)),
                1.5 * s,
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.circle_filled(p(-3.0, 1.0), 1.6 * s, color);
            painter.circle_filled(p(2.5, -1.5), 1.6 * s, color);
            painter.circle_filled(p(4.0, 2.5), 1.6 * s, color);
        }
        PhoneIcon::Book => {
            painter.line_segment([p(0.0, -7.0), p(0.0, 7.0)], stroke);
            painter.add(egui::Shape::line(
                vec![p(0.0, -7.0), p(-8.0, -5.0), p(-8.0, 7.0), p(0.0, 5.0)],
                stroke,
            ));
            painter.add(egui::Shape::line(
                vec![p(0.0, -7.0), p(8.0, -5.0), p(8.0, 7.0), p(0.0, 5.0)],
                stroke,
            ));
        }
        PhoneIcon::Counts => {
            for row in [-5.0_f32, 0.0, 5.0] {
                painter.circle_stroke(p(-6.0, row), 1.7 * s, stroke);
                painter.line_segment([p(-2.0, row), p(7.0, row)], stroke);
            }
        }
        PhoneIcon::Sheet => {
            painter.add(egui::Shape::line(
                vec![
                    p(-6.0, -7.0),
                    p(4.0, -7.0),
                    p(7.0, -4.0),
                    p(7.0, 7.0),
                    p(-6.0, 7.0),
                    p(-6.0, -7.0),
                ],
                stroke,
            ));
            painter.line_segment([p(4.0, -7.0), p(4.0, -4.0)], stroke);
            painter.line_segment([p(4.0, -4.0), p(7.0, -4.0)], stroke);
            painter.line_segment([p(-3.0, -1.0), p(3.0, -1.0)], stroke);
            painter.line_segment([p(-3.0, 2.5), p(3.0, 2.5)], stroke);
        }
    }
}

fn paint_note_glyph(
    painter: &egui::Painter,
    center: Pos2,
    scale: f32,
    color: Color32,
    crossed: bool,
) {
    let s = scale.max(0.4);
    let stroke = Stroke::new((2.0 * s).max(1.15), color);
    let p = |x: f32, y: f32| center + egui::Vec2::new(x * s, y * s);
    painter.circle_filled(p(-2.2, 5.0), 3.4 * s, color);
    painter.line_segment([p(1.0, 3.6), p(1.0, -8.0)], stroke);
    painter.add(egui::Shape::line(
        vec![p(1.0, -8.0), p(7.5, -4.0), p(1.2, -1.5)],
        stroke,
    ));
    if crossed {
        painter.line_segment([p(-7.0, -7.0), p(7.0, 7.0)], stroke);
    }
}

fn paint_person_pictogram(painter: &egui::Painter, center: Pos2, scale: f32, color: Color32) {
    let s = scale.max(0.4);
    let p = |x: f32, y: f32| center + egui::Vec2::new(x * s, y * s);
    painter.circle_filled(p(0.0, -5.6), 3.2 * s, color);
    painter.add(egui::Shape::convex_polygon(
        vec![p(-5.2, -1.4), p(5.2, -1.4), p(6.2, 8.0), p(-6.2, 8.0)],
        color,
        Stroke::NONE,
    ));
}

fn paint_cardinal_arrows(painter: &egui::Painter, center: Pos2, scale: f32, color: Color32) {
    let s = scale.max(0.4);
    let stroke = Stroke::new((1.8 * s).max(1.1), color);
    let p = |x: f32, y: f32| center + egui::Vec2::new(x * s, y * s);
    painter.line_segment([p(0.0, -12.0), p(0.0, -9.0)], stroke);
    painter.line_segment([p(-2.0, -10.5), p(0.0, -12.5)], stroke);
    painter.line_segment([p(2.0, -10.5), p(0.0, -12.5)], stroke);
    painter.line_segment([p(0.0, 9.0), p(0.0, 12.0)], stroke);
    painter.line_segment([p(-2.0, 10.5), p(0.0, 12.5)], stroke);
    painter.line_segment([p(2.0, 10.5), p(0.0, 12.5)], stroke);
    painter.line_segment([p(-12.0, 0.0), p(-9.0, 0.0)], stroke);
    painter.line_segment([p(-10.5, -2.0), p(-12.5, 0.0)], stroke);
    painter.line_segment([p(-10.5, 2.0), p(-12.5, 0.0)], stroke);
    painter.line_segment([p(9.0, 0.0), p(12.0, 0.0)], stroke);
    painter.line_segment([p(10.5, -2.0), p(12.5, 0.0)], stroke);
    painter.line_segment([p(10.5, 2.0), p(12.5, 0.0)], stroke);
}

fn paint_ghost_finger(painter: &egui::Painter, tip: Pos2, scale: f32, alpha: u8) {
    let s = scale.max(0.5);
    let color = Color32::from_rgba_unmultiplied(28, 32, 40, alpha);
    let stroke = Stroke::new((2.0 * s).max(1.2), color);
    let p = |x: f32, y: f32| tip + egui::Vec2::new(x * s, y * s);
    painter.circle_filled(tip, 3.2 * s, color);
    painter.add(egui::Shape::convex_polygon(
        vec![p(-3.0, 2.0), p(3.0, 2.0), p(5.5, 16.0), p(-4.5, 16.0)],
        Color32::from_rgba_unmultiplied(28, 32, 40, alpha / 3),
        stroke,
    ));
}

fn simple_reduced_motion(ctx: &egui::Context) -> bool {
    ctx.global_style().animation_time < 0.08 || os_prefers_reduced_motion()
}

fn os_prefers_reduced_motion() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        if std::env::var_os("DRILLFORGE_REDUCE_MOTION").is_some() {
            return true;
        }
        if matches!(std::env::var("GTK_ENABLE_ANIMATIONS").as_deref(), Ok("0")) {
            return true;
        }
        #[cfg(not(test))]
        let os = desktop_reduce_motion_probe();
        #[cfg(test)]
        let os = false;
        os
    })
}

#[cfg(not(test))]
fn desktop_reduce_motion_probe() -> bool {
    if desktop_cmd_stdout_contains(
        "gsettings",
        &["get", "org.gnome.desktop.interface", "enable-animations"],
        "false",
    ) {
        return true;
    }
    if let Some(text) = desktop_cmd_stdout(
        "defaults",
        &["read", "com.apple.universalaccess", "reduceMotion"],
    ) {
        let trimmed = text.trim();
        if trimmed == "1" || trimmed.eq_ignore_ascii_case("true") {
            return true;
        }
    }
    false
}

#[cfg(not(test))]
fn desktop_cmd_stdout(bin: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(bin)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(not(test))]
fn desktop_cmd_stdout_contains(bin: &str, args: &[&str], needle: &str) -> bool {
    desktop_cmd_stdout(bin, args).is_some_and(|text| text.contains(needle))
}

fn overlay_union(block: &mut Option<egui::Rect>, rect: egui::Rect) {
    *block = Some(match *block {
        Some(existing) => existing.union(rect),
        None => rect,
    });
}

fn paint_hold_ring(painter: &egui::Painter, rect: egui::Rect, progress: f32, color: Color32) {
    let progress = progress.clamp(0.0, 1.0);
    if progress <= 0.0 {
        return;
    }
    let center = rect.center();
    let radius = rect.width().max(rect.height()) * 0.5 + 5.0;
    let steps = ((progress * 48.0).ceil() as usize).max(2);
    let mut points = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let t = progress * (i as f32 / steps as f32);
        let angle = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * t;
        points.push(center + egui::Vec2::angled(angle) * radius);
    }
    painter.add(egui::Shape::line(points, Stroke::new(4.0, color)));
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
        if self.app_theme == super::app_theme::AppTheme::Studio {
            self.app_theme = super::app_theme::AppTheme::Daylight;
        }
        self.app_theme.apply(ctx);
        self.simple_mode.chrome_applied = false;
        self.full_daylight_ready = true;
    }

    /// The old Studio theme was the implicit dark default. The first full-mode
    /// frame of a session moves that onto the light paper. A theme picked from
    /// the menu afterward, including Studio, stays for the rest of the session.
    pub(crate) fn prefer_daylight_over_studio(&mut self, ctx: &egui::Context) {
        if self.full_daylight_ready {
            return;
        }
        self.full_daylight_ready = true;
        if self.app_theme == super::app_theme::AppTheme::Studio {
            self.app_theme = super::app_theme::AppTheme::Daylight;
            self.app_theme.apply(ctx);
        }
    }

    pub(crate) fn mirror_moves_on(&self) -> bool {
        self.simple_mode.mirror_moves
    }

    pub(crate) fn previous_ghost_shown(&self) -> bool {
        self.simple_mode.show_previous
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
            self.simple_mode.metronome.stop();
            self.simple_mode.return_hint = true;
            if self.simple_mode.scene_loop {
                self.simple_mode.scene_loop = false;
                self.playback_start = 0;
                self.playback_end = self.document.timeline_counts();
                self.loop_playback = false;
            }
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
            self.simple_close_readouts();
        }
    }

    /// Drops the session-only watch state when a different show is loaded.
    /// The previous-scene eye is per show, so the cached key is dropped and
    /// the next simple-mode frame reads the newly opened file's choice.
    pub(super) fn simple_reset_session_view(&mut self) {
        self.simple_mode.focus = None;
        self.simple_mode.away.clear();
        self.simple_close_readouts();
        self.simple_mode.previous_key = None;
        self.simple_mode.previous_key_is_draft = false;
    }

    fn simple_close_readouts(&mut self) {
        self.simple_mode.memo_open = false;
        self.simple_mode.memo_copied = false;
        self.simple_mode.place_sheet_open = false;
        self.simple_mode.place_sheet_copied = false;
        self.simple_mode.show_sheet_open = false;
        self.simple_mode.show_sheet_copied = false;
        self.simple_mode.path_sheet_open = false;
        self.simple_mode.path_sheet_for = None;
        self.simple_mode.path_sheet_copied = false;
        self.simple_mode.walk_sheet_open = false;
        self.simple_mode.walk_sheet_copied = false;
        self.simple_mode.walk_sheet_saved = false;
        self.simple_mode.walk_sheet_failed = false;
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
            self.simple_mode.draft_debounce_at = None;
            return;
        }
        let cursor = self.history.cursor();
        if self.simple_mode.draft_debounce_cursor != Some(cursor) {
            self.simple_mode.draft_debounce_cursor = Some(cursor);
            self.simple_mode.draft_debounce_at = Some(Instant::now());
        }
        if let Some(started) = self.simple_mode.draft_debounce_at {
            let wait = Duration::from_millis(SIMPLE_AUTOSAVE_MS);
            if started.elapsed() < wait {
                ctx.request_repaint_after(wait.saturating_sub(started.elapsed()));
                return;
            }
        }
        if let Some(since) = self.simple_mode.draft_retry_after
            && since.elapsed() < Duration::from_millis(SIMPLE_AUTOSAVE_MS)
        {
            ctx.request_repaint_after(Duration::from_millis(200));
            return;
        }
        if self.write_simple_draft() {
            self.dirty = false;
            self.simple_mode.draft_saved = true;
            self.simple_mode.draft_retry_after = None;
            self.simple_mode.draft_debounce_at = None;
            self.simple_flash_check();
        } else {
            self.simple_mode.draft_retry_after = Some(Instant::now());
            ctx.request_repaint_after(Duration::from_millis(SIMPLE_AUTOSAVE_MS));
        }
    }

    fn apply_qa_cast(&mut self, stage: &str) {
        self.onboarding.show_welcome = false;
        self.show_guidance = false;
        self.onboarding.coach_dismissed = true;
        self.simple_mode.return_hint = false;
        self.install_document(qa_cast_document());
        let full = stage != "cast";
        self.set_simple_mode(!full);
        self.field_tool = FieldTool::Select;
        if !full {
            // Set 3 has a previous picture, so the faint ghosts are on screen.
            self.navigate_to_set(2);
            self.nav_glide.settle();
            self.simple_mode.show_previous = true;
            self.simple_mode.previous_key_is_draft = true;
            self.simple_mode.previous_key_draft = self.simple_mode.draft_path_override.clone();
            self.simple_mode.previous_key =
                Some(self.simple_draft_path().to_string_lossy().into_owned());
            return;
        }
        self.full_legacy_chrome = false;
        self.full_inspector_collapsed = true;
        self.full_timeline_expanded = false;
        self.clear_selection();
        match stage {
            "cast-one" => {
                self.replace_selection(std::iter::once(3).collect());
                self.full_inspector_collapsed = false;
            }
            "cast-many" => {
                self.replace_selection((0..20).collect());
                self.full_inspector_collapsed = false;
            }
            "cast-roster" => {
                self.full_inspector_collapsed = false;
                if let Some(section) = self.document.sections.get(2) {
                    self.full_section_filter = vec![section.id];
                }
            }
            "cast-timeline" => {
                self.full_timeline_expanded = true;
                self.navigate_to_set(0);
                self.nav_glide.settle();
            }
            "cast-export" => {
                self.print_state.open = true;
                self.full_inspector_collapsed = true;
            }
            "cast-play" => {
                self.navigate_to_set(1);
                self.nav_glide.settle();
                self.playing = true;
                self.speed = 0.0;
                self.count_position = 4.0;
            }
            "cast-play-mid" => {
                self.navigate_to_set(2);
                self.nav_glide.settle();
                self.playing = true;
                self.speed = 0.0;
                let counts = self.document.sets.get(2).map(|set| set.counts).unwrap_or(1);
                self.count_position = f32::from(counts) * 0.5;
            }
            _ => {}
        }
    }

    /// Screenshot harness only. Seeds a frame when `DRILLFORGE_QA_SIMPLE` is
    /// `empty`, `placed`, `play`, `glossary`, `selected`, `recent`, `full`,
    /// `move`, `save`, `done`, `line`, `step`, `circle`, `nudge`, `walk`,
    /// `shows`, `hints`, `memo`, `numbers`, `shape`, `block`, `count`,
    /// `places`, `pair`, `order`, `sheet`, `recover`, `ghosts`, `mirror`,
    /// `guide-place`, `guide-move`, `guide-play`, `guide-playing`,
    /// `guide-collapsed`, `guide-song`, `full-selected`, `full-collapsed`,
    /// `full-timeline`, `full-multi`, `full-list`, `full-export`, `cast`,
    /// `cast-full`, `cast-one`, `cast-many`, `cast-roster`, `cast-timeline`,
    /// `cast-export`, `cast-play`, or `cast-play-mid`.
    pub(crate) fn apply_qa_simple_fixture(&mut self) {
        let Ok(stage) = std::env::var("DRILLFORGE_QA_SIMPLE") else {
            return;
        };
        if stage == "cast" || stage.starts_with("cast-") {
            self.apply_qa_cast(&stage);
            return;
        }
        self.onboarding.show_welcome = false;
        self.simple_mode.enabled = true;
        self.begin_simple_show();
        if stage.starts_with("guide-") {
            self.apply_qa_simple_guide(&stage);
            return;
        }
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
        if stage == "full" || stage.starts_with("full-") {
            self.set_simple_mode(false);
            // The return chip, coach, and guidance banner are real chrome,
            // but they cover the field this fixture exists to show.
            self.simple_mode.return_hint = false;
            self.show_guidance = false;
            self.onboarding.coach_dismissed = true;
            self.full_legacy_chrome = false;
            self.full_inspector_collapsed = true;
            self.full_timeline_expanded = false;
            self.clear_selection();
        }
        if stage == "full-selected" {
            self.replace_selection(std::iter::once(0).collect());
            self.full_inspector_collapsed = false;
        }
        if stage == "full-collapsed" {
            self.replace_selection(std::iter::once(0).collect());
            self.full_inspector_collapsed = true;
        }
        if stage == "full-timeline" {
            // A second set gives the count track a real IN→OUT span.
            self.duplicate_current_set();
            self.navigate_to_set(0);
            self.nav_glide.settle();
            self.full_timeline_expanded = true;
            self.full_inspector_collapsed = true;
            self.clear_selection();
        }
        if stage == "full-multi" {
            self.replace_selection([0, 1].into_iter().collect());
            self.full_inspector_collapsed = false;
        }
        if stage == "full-list" {
            self.clear_selection();
            self.full_inspector_collapsed = false;
        }
        if stage == "full-export" {
            self.print_state.open = true;
            self.full_inspector_collapsed = true;
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
        if stage == "count" || stage == "places" {
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
                        x: origin.x + step * 4.0,
                        y: origin.y,
                    }],
                },
                "qa",
            );
            self.navigate_to_set(self.current_set);
            self.nav_glide.settle();
            self.simple_rename_scene("サビ");
            self.replace_selection(std::iter::once(0).collect());
            self.simple_toggle_scene_loop();
        }
        if stage == "places" {
            self.simple_mode.place_sheet_open = true;
        }
        if stage == "pair" {
            self.place_performer_at(Point { x: 8.0, y: 28.0 }, true);
            self.place_performer_at(Point { x: 30.0, y: 8.0 }, true);
            self.place_performer_at(Point { x: 18.0, y: 22.0 }, true);
            self.place_performer_at(Point { x: 36.0, y: 14.0 }, true);
            self.simple_diagonal_up();
            self.simple_center_up();
            self.duplicate_current_set();
            let step = self.document.grid.horizontal_units
                / f32::from(self.document.grid.horizontal_steps.max(1));
            let origin = self.document.sets[self.current_set].positions[3];
            let set_id = self.document.sets[self.current_set].id;
            let performer_id = self.document.performers[3].id;
            let _ = self.execute_edit(
                Edit::MovePerformers {
                    set_id,
                    performer_ids: vec![performer_id],
                    positions: vec![Point {
                        x: origin.x + step * 4.0,
                        y: origin.y,
                    }],
                },
                "qa",
            );
            self.navigate_to_set(self.current_set);
            self.nav_glide.settle();
            self.replace_selection([0_usize, 1].into_iter().collect());
        }
        if stage == "order" || stage == "sheet" {
            self.place_performer_at(Point { x: 8.0, y: 28.0 }, true);
            self.place_performer_at(Point { x: 30.0, y: 8.0 }, true);
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
                        x: origin.x + step * 4.0,
                        y: origin.y,
                    }],
                },
                "qa",
            );
            self.simple_rename_scene("サビ");
            self.duplicate_current_set();
            self.navigate_to_set(1);
            self.nav_glide.settle();
        }
        if stage == "sheet" {
            self.simple_mode.show_sheet_open = true;
        }
        if stage == "ghosts" {
            self.onboarding.simple_pictogram_done = true;
            self.simple_mode.hint_closed = true;
            let performer_ids: Vec<_> = self
                .document
                .performers
                .iter()
                .map(|person| person.id)
                .collect();
            let _ = self.execute_edit(
                Edit::MovePerformers {
                    set_id: self.document.sets[0].id,
                    performer_ids: performer_ids.clone(),
                    positions: vec![Point { x: 14.0, y: 18.0 }, Point { x: 22.0, y: 18.0 }],
                },
                "qa",
            );
            self.duplicate_current_set();
            let _ = self.execute_edit(
                Edit::MovePerformers {
                    set_id: self.document.sets[self.current_set].id,
                    performer_ids,
                    positions: vec![Point { x: 24.0, y: 26.0 }, Point { x: 32.0, y: 26.0 }],
                },
                "qa",
            );
            self.nav_glide.settle();
            self.playing = false;
            self.count_position = 0.0;
            self.document
                .positions_at(self.current_set, 0.0, &mut self.frame_positions);
            // Keep the eye on without writing the real preference file.
            // `simple_ui` reloads a missing entry as off unless this cache
            // already matches the open draft.
            self.simple_mode.show_previous = true;
            self.simple_mode.previous_key_is_draft = true;
            self.simple_mode.previous_key =
                Some(self.simple_draft_path().to_string_lossy().into_owned());
            self.simple_mode.previous_key_draft = self.simple_mode.draft_path_override.clone();
        }
        if stage == "mirror" {
            self.onboarding.simple_pictogram_done = true;
            self.simple_mode.hint_closed = true;
            self.simple_mode.mirror_moves = true;
            // 12 mirrors to 34 across the indoor axis at 23. The right person
            // sits a step off so the orange "not mirrored" chip is visible.
            self.place_performer_at(Point { x: 32.0, y: 16.0 }, false);
            self.place_performer_at(Point { x: 26.0, y: 22.0 }, false);
            self.replace_selection(std::iter::once(0).collect());
        }
        if stage == "order" || stage == "recover" {
            self.project_state
                .recoveries
                .push(drill_project::recovery::RecoveryCandidate {
                    root: std::path::PathBuf::from("recovery/qa"),
                    autosave: std::path::PathBuf::from("recovery/qa/autosave.drill.json"),
                    meta: drill_project::recovery::SessionMeta {
                        session_id: "qa".into(),
                        app_version: "qa".into(),
                        started_unix_ms: 0,
                        heartbeat_unix_ms: 0,
                        document_title: "文化祭".into(),
                        origin_name: None,
                    },
                    autosave_bytes: 2048,
                    stale_for: Duration::from_secs(90),
                });
        }
        self.settle_qa_simple_chrome();
    }

    fn apply_qa_simple_guide(&mut self, stage: &str) {
        self.onboarding.simple_pictogram_done = false;
        self.simple_mode.hint_closed = false;
        self.simple_mode.hint_pulse_only = false;
        // Pass 2 of the screenshot harness is only a few milliseconds in.
        // Start the ghost finger mid-demo so the PNG is not an empty pulse.
        self.simple_mode.hint_phase_at = Some(Instant::now() - Duration::from_millis(500));
        self.simple_mode.last_input_at = Some(Instant::now());
        match stage {
            "guide-place" => {}
            "guide-move" => {
                self.place_performer_at(Point { x: 12.0, y: 16.0 }, true);
                self.place_performer_at(Point { x: 20.0, y: 16.0 }, true);
                self.place_performer_at(Point { x: 16.0, y: 22.0 }, true);
                self.simple_mark_hint(SimpleGuide::Place);
                self.simple_mode.hint_phase_at = Some(Instant::now() - Duration::from_millis(500));
            }
            "guide-play" => {
                self.place_performer_at(Point { x: 12.0, y: 16.0 }, true);
                self.place_performer_at(Point { x: 20.0, y: 16.0 }, true);
                self.simple_mark_hint(SimpleGuide::Place);
                self.simple_mark_hint(SimpleGuide::Move);
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
                self.simple_mark_hint(SimpleGuide::NextSet);
                self.simple_mode.hint_phase_at = Some(Instant::now() - Duration::from_millis(500));
            }
            "guide-playing" => {
                self.place_performer_at(Point { x: 12.0, y: 16.0 }, true);
                self.place_performer_at(Point { x: 20.0, y: 16.0 }, true);
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
                self.simple_mark_hint(SimpleGuide::Place);
                self.simple_mark_hint(SimpleGuide::Move);
                self.simple_mark_hint(SimpleGuide::NextSet);
                self.simple_mark_hint(SimpleGuide::Play);
                self.current_set = 0;
                self.nav_glide.settle();
                let counts = self
                    .document
                    .sets
                    .first()
                    .map(|set| set.counts.max(1))
                    .unwrap_or(16);
                self.count_position = 6.0_f32.min(f32::from(counts) * 0.5);
                self.playback_start = 0;
                self.playback_end = self.document.timeline_counts();
                self.playing = true;
                self.speed = 0.0;
                self.document.positions_at(
                    self.current_set,
                    self.count_position / f32::from(counts),
                    &mut self.frame_positions,
                );
            }
            "guide-song" => {
                self.apply_qa_simple_guide("guide-play");
                self.onboarding.simple_pictogram_done = true;
                self.simple_mode.hint_closed = true;
                let path = std::env::temp_dir().join("文化祭ステージ.wav");
                write_qa_song_wav(&path);
                let mut track = drill_core::audio::AudioTrack {
                    path: path.display().to_string(),
                    duration_seconds: 0.25,
                    ..drill_core::audio::AudioTrack::default()
                };
                apply_simple_song_offset(&mut track, 0.4);
                let _ = self.simple_commit_song(Some(track), "simple-mode.340");
                self.simple_mode.song_missing = false;
            }
            "guide-collapsed" => {
                self.onboarding.simple_pictogram_done = true;
                self.onboarding.simple_toolbar_collapsed = true;
                self.place_performer_at(Point { x: 12.0, y: 16.0 }, true);
                self.place_performer_at(Point { x: 20.0, y: 16.0 }, true);
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
                self.current_set = 0;
                self.nav_glide.settle();
            }
            _ => {}
        }
        self.settle_qa_simple_chrome();
    }

    fn settle_qa_simple_chrome(&mut self) {
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
        self.sync_previous_ghost_pref();
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
        if !self.playing && self.simple_mode.scene_loop {
            self.simple_apply_watch_range();
        }
        self.poll_simple_metronome();
        self.autosave_simple_draft(ui.ctx());
        self.expire_simple_saved_note(ui.ctx());
        self.note_simple_chrome_motion(ui.ctx());
        self.simple_tick_pictogram(ui);
        self.simple_sync_history_notes();
        self.simple_paint_field(ui);
        self.simple_paint_floating_layer(ui);
        self.simple_paint_overlays(ui);
        self.simple_sync_history_notes();
        self.show_update_notice(ui.ctx());
        self.show_recent_projects(ui.ctx());
        self.simple_name_sheet(ui.ctx());
        self.simple_memo_sheet(ui.ctx());
        self.simple_place_sheet(ui.ctx());
        self.simple_show_sheet(ui.ctx());
        self.simple_path_sheet(ui.ctx());
        self.simple_walk_sheet(ui.ctx());
        self.glossary.show(ui.ctx(), self.locale);
        self.onboarding.help_ui(ui.ctx(), self.locale);
        self.onboarding.persist_if_changed();
    }

    pub(crate) fn show_simple_return_hint(&mut self, ui: &mut egui::Ui) {
        if !self.simple_mode.return_hint {
            return;
        }
        let ctx = ui.ctx().clone();
        egui::Area::new(egui::Id::new("simple-return-hint"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_TOP, egui::Vec2::new(0.0, 8.0))
            .show(&ctx, |ui| {
                egui::Frame::new()
                    .fill(super::app_theme::ACCENT_SOFT)
                    .stroke(Stroke::new(1.0, super::app_theme::HAIRLINE))
                    .inner_margin(egui::Margin::symmetric(16, 10))
                    .corner_radius(16)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(i18n::registered(
                                    self.locale,
                                    "simple-mode.106",
                                ))
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

    fn simple_flash_check(&mut self) {
        self.simple_mode.check_until = Some(Instant::now() + Duration::from_millis(600));
        self.simple_mode.saved_note_until = self.simple_mode.check_until;
    }

    fn simple_fail(&mut self) {
        self.simple_mode.fail_until = Some(Instant::now() + Duration::from_millis(300));
    }

    fn simple_fail_t(&self) -> Option<f32> {
        let until = self.simple_mode.fail_until?;
        let left = until
            .saturating_duration_since(Instant::now())
            .as_secs_f32();
        if left <= 0.0 {
            None
        } else {
            Some((1.0 - left / SIMPLE_FAIL_SECS).clamp(0.0, 1.0))
        }
    }

    fn simple_check_t(&self) -> Option<f32> {
        let until = self.simple_mode.check_until?;
        let left = until
            .saturating_duration_since(Instant::now())
            .as_secs_f32();
        if left <= 0.0 {
            None
        } else {
            Some((1.0 - left / SIMPLE_OK_SECS).clamp(0.0, 1.0))
        }
    }

    fn simple_mark_hint(&mut self, guide: SimpleGuide) {
        match guide {
            SimpleGuide::Place => self.simple_mode.hint_place_done = true,
            SimpleGuide::Move => {
                self.simple_mode.hint_move_done = true;
                self.onboarding.simple_drag_tip_seen = true;
            }
            SimpleGuide::NextSet => self.simple_mode.hint_next_done = true,
            SimpleGuide::Play => self.simple_mode.hint_play_done = true,
        }
        self.simple_mode.hint_phase_at = None;
        self.simple_mode.hint_pulse_only = false;
        if self.simple_mode.hint_place_done
            && self.simple_mode.hint_move_done
            && self.simple_mode.hint_next_done
            && self.simple_mode.hint_play_done
        {
            self.onboarding.simple_pictogram_done = true;
        }
    }

    fn simple_active_hint(&self) -> Option<SimpleGuide> {
        if self.onboarding.simple_pictogram_done || self.simple_mode.hint_closed {
            return None;
        }
        if !self.simple_mode.hint_place_done {
            return Some(SimpleGuide::Place);
        }
        if !self.simple_mode.hint_move_done {
            return Some(SimpleGuide::Move);
        }
        if !self.simple_mode.hint_next_done {
            return Some(SimpleGuide::NextSet);
        }
        if !self.simple_mode.hint_play_done && self.document.sets.len() >= 2 {
            return Some(SimpleGuide::Play);
        }
        None
    }

    fn simple_apply_hint_idle(&mut self, now: Instant) {
        if self.simple_mode.last_input_at.is_none() {
            self.simple_mode.last_input_at = Some(now);
        }
        if let Some(last) = self.simple_mode.last_input_at
            && now.saturating_duration_since(last).as_secs_f32() >= SIMPLE_HINT_IDLE_SECS
            && self.simple_active_hint().is_some()
        {
            self.simple_mode.hint_pulse_only = true;
        }
    }

    fn simple_tick_pictogram(&mut self, ui: &mut egui::Ui) {
        let now = Instant::now();
        if ui.input(|input| input.pointer.any_pressed() || input.key_pressed(egui::Key::Space)) {
            self.simple_mode.last_input_at = Some(now);
        }
        self.simple_apply_hint_idle(now);
        if self.simple_active_hint().is_some() {
            if self.simple_mode.hint_phase_at.is_none() {
                self.simple_mode.hint_phase_at = Some(now);
            }
            ui.ctx().request_repaint_after(Duration::from_millis(32));
        }
        if self
            .simple_mode
            .check_until
            .is_some_and(|until| Instant::now() >= until)
        {
            self.simple_mode.check_until = None;
        }
        if self
            .simple_mode
            .fail_until
            .is_some_and(|until| Instant::now() >= until)
        {
            self.simple_mode.fail_until = None;
        }
        if self.simple_mode.check_until.is_some() || self.simple_mode.fail_until.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(16));
        }
        if self.playing {
            ui.ctx().request_repaint_after(Duration::from_millis(32));
        }
    }

    fn simple_hint_cycle(&self) -> (f32, bool) {
        let Some(started) = self.simple_mode.hint_phase_at else {
            return (0.0, false);
        };
        let elapsed = started.elapsed().as_secs_f32();
        let period = SIMPLE_HINT_MOVE_SECS + SIMPLE_HINT_REST_SECS;
        let repeats = (elapsed / period).floor() as u8;
        let pulse_only = self.simple_mode.hint_pulse_only || repeats >= SIMPLE_HINT_MAX_REPEATS;
        let phase = elapsed % period;
        let t = if pulse_only {
            0.5 + 0.5 * (elapsed * 2.4).sin()
        } else if phase < SIMPLE_HINT_MOVE_SECS {
            (phase / SIMPLE_HINT_MOVE_SECS).clamp(0.0, 1.0)
        } else {
            1.0
        };
        (t, pulse_only)
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
            height += SIMPLE_TILE + SIMPLE_LABEL + 16.0;
        }
        let notes = self.simple_history_lines().len();
        if notes > 0 {
            height += 8.0 + 22.0 * notes as f32;
        }
        height
    }

    /// A crashed session's autosave. Opening it fills the field and asks for
    /// a name on the next save, so the recovery folder is not the show file.
    fn simple_recovery_banner(&mut self, ui: &mut egui::Ui) {
        if !self.simple_mode.enabled || self.project_state.recoveries.is_empty() {
            return;
        }
        let title = self.project_state.recoveries[0]
            .meta
            .document_title
            .trim()
            .to_string();
        ui.add_space(4.0);
        let mut open = false;
        let mut dismiss = false;
        egui::Frame::new()
            .fill(super::app_theme::ACCENT_SOFT)
            .inner_margin(egui::Margin::symmetric(16, 10))
            .corner_radius(16)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::Vec2::new(8.0, 8.0);
                    ui.label(
                        egui::RichText::new(i18n::registered(self.locale, "simple-mode.262"))
                            .size(15.0)
                            .color(SIMPLE_INK),
                    );
                    if !title.is_empty() {
                        ui.label(
                            egui::RichText::new(title)
                                .size(15.0)
                                .strong()
                                .color(SIMPLE_INK),
                        );
                    }
                    if simple_choice_button(
                        ui,
                        i18n::registered(self.locale, "simple-mode.263"),
                        !self.project_state.busy(),
                        true,
                    )
                    .clicked()
                    {
                        open = true;
                    }
                    if simple_choice_button(
                        ui,
                        i18n::registered(self.locale, "simple-mode.264"),
                        true,
                        false,
                    )
                    .clicked()
                    {
                        dismiss = true;
                    }
                });
            });
        if open {
            self.project_state.load_recovery(0);
        }
        if dismiss {
            self.project_state.ignore_recovery(0);
        }
    }

    #[allow(dead_code)]
    fn simple_header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.label(
                egui::RichText::new(i18n::registered(self.locale, "simple-mode.086"))
                    .size(13.0)
                    .color(super::app_theme::SECONDARY_TEXT),
            );
            let set_count = self.document.sets.len();
            let chips_w = (96.0 * set_count as f32).min((ui.available_width() - 180.0).max(56.0));
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
                    self.simple_member_count(ui);
                }
                if self.simple_check_t().is_some() {
                    let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(28.0), Sense::hover());
                    paint_phone_icon_at(
                        ui.painter(),
                        rect.center(),
                        PhoneIcon::Check,
                        SIMPLE_OK,
                        1.2,
                    );
                } else if let Some(title) = self.simple_show_title() {
                    ui.label(
                        egui::RichText::new(title)
                            .size(12.0)
                            .color(super::app_theme::SECONDARY_TEXT),
                    );
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

    /// Eye pictogram beside the set dots. Same overlay row, so the field
    /// stays full-window. The hit target is at least 48px on both sides.
    fn simple_previous_toggle(&mut self, ui: &mut egui::Ui) {
        let label = i18n::registered(self.locale, "simple-mode.330");
        let width = simple_previous_toggle_width(label);
        let enabled = simple_previous_toggle_enabled(self.current_set);
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, response) =
            ui.allocate_exact_size(egui::Vec2::new(width, PREVIOUS_TOGGLE_HEIGHT), sense);
        let mut on = self.simple_mode.show_previous;
        if enabled && response.clicked() {
            on = !on;
            self.set_show_previous(on);
        }
        let hovered = enabled && response.hovered();
        let fill = if on && enabled {
            SIMPLE_BLUE
        } else if on {
            SIMPLE_BLUE.gamma_multiply(0.35)
        } else if hovered {
            super::app_theme::ACCENT_SOFT
        } else {
            Color32::WHITE
        };
        let ink = if on && enabled {
            Color32::WHITE
        } else if enabled {
            SIMPLE_BLUE
        } else {
            SIMPLE_BLUE.gamma_multiply(0.35)
        };
        let label_ink = if on && enabled {
            Color32::WHITE
        } else if enabled {
            SIMPLE_INK
        } else {
            super::app_theme::SECONDARY_TEXT.gamma_multiply(0.55)
        };
        ui.painter().rect_filled(rect, 16.0, fill);
        if !on || !enabled {
            ui.painter().rect_stroke(
                rect,
                16.0,
                Stroke::new(1.0, super::app_theme::HAIRLINE),
                StrokeKind::Inside,
            );
        }
        paint_eye_icon(
            ui.painter(),
            Pos2::new(rect.left() + 24.0, rect.center().y),
            on,
            ink,
        );
        ui.painter().text(
            Pos2::new(rect.left() + 40.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(12.0),
            label_ink,
        );
        let previous_hover = self.key_tip(
            ui.ctx(),
            self.simple_previous_hover_text(),
            super::keybindings::Action::TogglePreviousGhost,
        );
        let _ = response.on_hover_text(previous_hover);
    }

    fn simple_previous_hover_text(&self) -> &'static str {
        if !simple_previous_toggle_enabled(self.current_set) {
            i18n::registered(self.locale, "simple-mode.332")
        } else if self.playing {
            i18n::registered(self.locale, "simple-mode.333")
        } else {
            i18n::registered(self.locale, "simple-mode.331")
        }
    }

    #[allow(dead_code)]
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
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::new(8.0, 8.0);
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
            if self.document.sets.len() >= 2 {
                let earlier = self.current_set > 0;
                if simple_choice_button(
                    ui,
                    i18n::registered(self.locale, "simple-mode.268"),
                    earlier,
                    false,
                )
                .on_hover_text(i18n::registered(self.locale, "simple-mode.269"))
                .clicked()
                {
                    self.simple_shift_scene(false);
                }
                let later = self.current_set + 1 < self.document.sets.len();
                if simple_choice_button(
                    ui,
                    i18n::registered(self.locale, "simple-mode.270"),
                    later,
                    false,
                )
                .on_hover_text(i18n::registered(self.locale, "simple-mode.271"))
                .clicked()
                {
                    self.simple_shift_scene(true);
                }
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
        let stored = self
            .document
            .sets
            .get(index)
            .map(|set| set.name.as_str())
            .unwrap_or("");
        let (rect, mut response) =
            ui.allocate_exact_size(egui::Vec2::new(SIMPLE_HIT, SIMPLE_HIT), Sense::click());
        let radius = if current { 18.0 } else { 11.0 };
        let fill = if current { SIMPLE_BLUE } else { Color32::WHITE };
        let ink = if current { Color32::WHITE } else { SIMPLE_INK };
        ui.painter().circle_filled(rect.center(), radius, fill);
        if !current {
            ui.painter()
                .circle_stroke(rect.center(), radius, Stroke::new(1.5, SIMPLE_BLUE));
        }
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            (index + 1).to_string(),
            egui::FontId::proportional(if current { 18.0 } else { 13.0 }),
            ink,
        );
        response = response.on_hover_text(simple_chip_caption(stored, index));
        if response.clicked() && !current {
            self.simple_mode.overlap_note = false;
            self.simple_mode.finished_playback = false;
            self.simple_mode.hold_count = false;
            self.navigate_to_set(index);
            if self.simple_mode.scene_loop {
                self.simple_apply_watch_range();
            }
        }
    }

    fn simple_member_count(&self, ui: &mut egui::Ui) {
        let n = self.document.performers.len();
        let caption = super::i18n::roster_count(self.locale, n);
        ui.label(
            egui::RichText::new(caption.clone())
                .size(20.0)
                .color(SIMPLE_INK),
        )
        .on_hover_text(caption);
    }

    #[allow(dead_code)]
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
                .size(13.0)
                .color(super::app_theme::SECONDARY_TEXT),
        );
    }

    fn simple_action_bar(&mut self, ui: &mut egui::Ui) {
        let hint = self.simple_active_hint();
        let spotlight = hint.unwrap_or_else(|| self.simple_guide());
        let playing = self.playing;
        let can_place = !playing && self.is_editable_set_start();
        let can_move = can_place && !self.document.performers.is_empty();
        let can_next = can_place && !self.document.performers.is_empty();
        let can_play = playing || self.document.sets.len() >= 2 || self.show_has_motion();
        let time = ui.input(|input| input.time) as f32;
        let mut place = false;
        let mut move_focus = false;
        let mut next = false;
        let mut play = false;
        let mut blocked = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let tile = SIMPLE_TILE;
            let (place_rect, place_click) = self.simple_pictogram_tile(
                ui,
                PictogramTile {
                    icon: PhoneIcon::PersonPlus,
                    label: i18n::registered(self.locale, "simple-mode.303"),
                    tip: i18n::registered(self.locale, "simple-mode.304"),
                    width: tile,
                    enabled: can_place,
                    active: spotlight == SimpleGuide::Place && !playing,
                    pulse: hint == Some(SimpleGuide::Place) && !playing,
                    filled: false,
                    compact: false,
                    time,
                    lock_width: false,
                },
            );
            self.simple_mode.action_place = Some(place_rect);
            if place_click {
                if can_place {
                    place = true;
                } else {
                    blocked = true;
                }
            }
            let (move_rect, move_click) = self.simple_pictogram_tile(
                ui,
                PictogramTile {
                    icon: PhoneIcon::PersonMove,
                    label: i18n::registered(self.locale, "simple-mode.305"),
                    tip: i18n::registered(self.locale, "simple-mode.306"),
                    width: tile,
                    enabled: can_move,
                    active: spotlight == SimpleGuide::Move && !playing,
                    pulse: false,
                    filled: false,
                    compact: false,
                    time,
                    lock_width: false,
                },
            );
            self.simple_mode.action_move = Some(move_rect);
            if move_click {
                if can_move {
                    move_focus = true;
                } else {
                    blocked = true;
                }
            }
            let mirror_on = self.simple_mode.mirror_moves;
            let mirror_tip = self.key_tip(
                ui.ctx(),
                i18n::registered(self.locale, "simple-mode.325"),
                super::keybindings::Action::ToggleSymmetry,
            );
            let (mirror_rect, mirror_click) = self.simple_pictogram_tile(
                ui,
                PictogramTile {
                    icon: PhoneIcon::Mirror,
                    label: i18n::registered(self.locale, "simple-mode.320"),
                    tip: &mirror_tip,
                    width: 72.0,
                    enabled: !playing,
                    active: mirror_on,
                    pulse: false,
                    filled: mirror_on,
                    compact: false,
                    time,
                    lock_width: false,
                },
            );
            let _ = mirror_rect;
            if mirror_click {
                if playing {
                    blocked = true;
                } else {
                    self.simple_toggle_mirror();
                }
            }
            let next_pulse = hint == Some(SimpleGuide::NextSet) && !playing;
            let next_tip = self.key_tip(
                ui.ctx(),
                i18n::registered(self.locale, "simple-mode.308"),
                super::keybindings::Action::Command(UiCommand::DuplicateSet),
            );
            let (next_rect, next_click) = self.simple_pictogram_tile(
                ui,
                PictogramTile {
                    icon: PhoneIcon::SheetsNext,
                    label: i18n::registered(self.locale, "simple-mode.307"),
                    tip: &next_tip,
                    width: tile,
                    enabled: can_next,
                    active: spotlight == SimpleGuide::NextSet && !playing,
                    pulse: next_pulse,
                    filled: false,
                    compact: false,
                    time,
                    lock_width: false,
                },
            );
            self.simple_mode.action_next = Some(next_rect);
            if next_click {
                if can_next {
                    next = true;
                } else {
                    blocked = true;
                }
            }
            let (play_icon, play_label, play_tip) = if playing {
                (
                    PhoneIcon::Stop,
                    i18n::registered(self.locale, "simple-mode.311"),
                    i18n::registered(self.locale, "simple-mode.312"),
                )
            } else {
                (
                    PhoneIcon::Play,
                    i18n::registered(self.locale, "simple-mode.309"),
                    i18n::registered(self.locale, "simple-mode.310"),
                )
            };
            let play_pulse = (hint == Some(SimpleGuide::Play) && !playing)
                || (hint.is_none() && spotlight == SimpleGuide::Play && !playing);
            let play_tip = self.key_tip(
                ui.ctx(),
                play_tip,
                super::keybindings::Action::Command(UiCommand::PlayPause),
            );
            let (play_rect, play_click) = self.simple_pictogram_tile(
                ui,
                PictogramTile {
                    icon: play_icon,
                    label: play_label,
                    tip: &play_tip,
                    width: SIMPLE_PLAY_TILE,
                    enabled: can_play,
                    active: true,
                    pulse: play_pulse,
                    filled: true,
                    compact: false,
                    time,
                    lock_width: false,
                },
            );
            self.simple_mode.action_play = Some(play_rect);
            if play_click {
                if can_play {
                    play = true;
                } else {
                    blocked = true;
                }
            }
            let song_tip = self.key_tip(
                ui.ctx(),
                i18n::registered(self.locale, "simple-mode.341"),
                super::keybindings::Action::ImportSong,
            );
            let (song_rect, song_click) = self.simple_pictogram_tile(
                ui,
                PictogramTile {
                    icon: PhoneIcon::Note,
                    label: i18n::registered(self.locale, "simple-mode.340"),
                    tip: &song_tip,
                    width: SIMPLE_TILE,
                    enabled: true,
                    active: self.document.audio.is_some() && !self.simple_mode.song_missing,
                    pulse: false,
                    filled: false,
                    compact: false,
                    time,
                    lock_width: false,
                },
            );
            let _ = song_rect;
            if song_click {
                self.simple_pick_and_import_song();
            }
            let undo_tip = self.key_tip(
                ui.ctx(),
                i18n::registered(self.locale, "simple-mode.092"),
                super::keybindings::Action::Command(UiCommand::Undo),
            );
            let (undo_rect, undo_click) = self.simple_pictogram_tile(
                ui,
                PictogramTile {
                    icon: PhoneIcon::Undo,
                    label: i18n::registered(self.locale, "simple-mode.087"),
                    tip: &undo_tip,
                    width: tile,
                    enabled: self.history.can_undo(),
                    active: false,
                    pulse: false,
                    filled: false,
                    compact: false,
                    time,
                    lock_width: false,
                },
            );
            let _ = undo_rect;
            if undo_click {
                self.simple_mode.overlap_note = false;
                self.execute_command(UiCommand::Undo, ui.ctx());
            }
            let redo_tip = self.key_tip(
                ui.ctx(),
                i18n::registered(self.locale, "simple-mode.091"),
                super::keybindings::Action::Command(UiCommand::Redo),
            );
            let (redo_rect, redo_click) = self.simple_pictogram_tile(
                ui,
                PictogramTile {
                    icon: PhoneIcon::Redo,
                    label: i18n::registered(self.locale, "simple-mode.090"),
                    tip: &redo_tip,
                    width: tile,
                    enabled: self.history.can_redo(),
                    active: false,
                    pulse: false,
                    filled: false,
                    compact: false,
                    time,
                    lock_width: false,
                },
            );
            let _ = redo_rect;
            if redo_click {
                self.simple_mode.overlap_note = false;
                self.execute_command(UiCommand::Redo, ui.ctx());
            }
            let can_delete = self.simple_can_remove();
            let delete_label = if self.selected.len() == 1 {
                i18n::registered(self.locale, "simple-mode.097")
            } else {
                i18n::registered(self.locale, "simple-mode.098")
            };
            let delete_tip = self.key_tip(
                ui.ctx(),
                i18n::registered(self.locale, "simple-mode.314"),
                super::keybindings::Action::Command(UiCommand::RemoveSelectedPerformers),
            );
            let (delete_rect, delete_response) = self.simple_hold_tile(
                ui,
                PhoneIcon::Trash,
                delete_label,
                &delete_tip,
                tile,
                can_delete,
            );
            let _ = delete_rect;
            if delete_response {
                self.simple_commit_held_delete();
            }
            let hide_tip = self.key_tip(
                ui.ctx(),
                i18n::registered(self.locale, "simple-mode.317"),
                super::keybindings::Action::ToggleToolbar,
            );
            let (hide_rect, hide_click) = self.simple_pictogram_tile(
                ui,
                PictogramTile {
                    icon: PhoneIcon::ChevronDown,
                    label: i18n::registered(self.locale, "simple-mode.316"),
                    tip: &hide_tip,
                    width: SIMPLE_HIT,
                    enabled: true,
                    active: false,
                    pulse: false,
                    filled: false,
                    compact: false,
                    time,
                    lock_width: false,
                },
            );
            let _ = hide_rect;
            if hide_click {
                self.onboarding.simple_toolbar_collapsed = true;
            }
            let keys_tip = self.key_tip(
                ui.ctx(),
                i18n::registered(self.locale, "shortcuts.025"),
                super::keybindings::Action::OpenKeySettings,
            );
            let (keys_rect, keys_click) = self.simple_pictogram_tile(
                ui,
                PictogramTile {
                    icon: PhoneIcon::Tools,
                    label: i18n::registered(self.locale, "shortcuts.024"),
                    tip: &keys_tip,
                    width: SIMPLE_HIT,
                    enabled: true,
                    active: false,
                    pulse: false,
                    filled: false,
                    compact: false,
                    time,
                    lock_width: false,
                },
            );
            let _ = keys_rect;
            if keys_click {
                self.keybindings.settings_open = true;
            }
        });
        if blocked {
            self.simple_fail();
        }
        if place {
            self.simple_add_person();
            self.simple_mark_hint(SimpleGuide::Place);
        }
        if move_focus && self.selected.is_empty() && !self.document.performers.is_empty() {
            self.replace_selection(std::iter::once(0).collect());
        }
        if next {
            self.simple_mode.finished_playback = false;
            let revision = self.history.revision();
            self.duplicate_current_set();
            self.simple_mark(revision, "simple-mode.078");
            self.simple_mark_hint(SimpleGuide::NextSet);
        }
        if play {
            if playing {
                self.toggle_playback(ui.ctx());
            } else {
                self.simple_start_play(ui.ctx());
                self.simple_mark_hint(SimpleGuide::Play);
            }
        }
    }

    pub(crate) fn simple_pictogram_tile(
        &self,
        ui: &mut egui::Ui,
        tile: PictogramTile<'_>,
    ) -> (egui::Rect, bool) {
        let height = if tile.compact || tile.label.is_empty() {
            SIMPLE_TILE.max(SIMPLE_HIT)
        } else {
            SIMPLE_TILE + SIMPLE_LABEL + 8.0
        };
        let text_w = if tile.compact || tile.label.is_empty() {
            0.0
        } else {
            ui.painter()
                .layout_no_wrap(
                    tile.label.to_owned(),
                    egui::FontId::proportional(SIMPLE_LABEL),
                    SIMPLE_INK,
                )
                .size()
                .x
        };
        let width = if tile.lock_width {
            tile.width
        } else if text_w == 0.0 {
            tile.width.max(SIMPLE_HIT)
        } else {
            tile.width.max(text_w + 18.0).clamp(SIMPLE_HIT, 120.0)
        };
        let (rect, response) =
            ui.allocate_exact_size(egui::Vec2::new(width, height), Sense::click());
        let glow = if tile.pulse {
            0.55 + 0.45 * (tile.time * 3.0).sin().abs()
        } else {
            1.0
        };
        let rounding = 16.0;
        let fill = if tile.filled {
            let base = if tile.enabled && response.hovered() {
                Color32::from_rgb(16, 82, 176)
            } else {
                SIMPLE_BLUE
            };
            if tile.enabled {
                base
            } else {
                base.gamma_multiply(0.45)
            }
        } else if tile.active {
            Color32::from_rgba_unmultiplied(20, 96, 200, (48.0 + 90.0 * glow) as u8)
        } else if response.hovered() {
            super::app_theme::ACCENT_SOFT
        } else {
            Color32::WHITE
        };
        let ink = if tile.filled {
            if tile.enabled {
                Color32::WHITE
            } else {
                Color32::from_white_alpha(160)
            }
        } else if tile.enabled {
            SIMPLE_INK
        } else {
            super::app_theme::SECONDARY_TEXT.gamma_multiply(0.55)
        };
        let icon_color = if tile.filled {
            if tile.enabled {
                Color32::WHITE
            } else {
                Color32::from_white_alpha(180)
            }
        } else if tile.enabled {
            SIMPLE_BLUE
        } else {
            Color32::from_rgba_unmultiplied(20, 96, 200, 80)
        };
        ui.painter().rect_filled(rect, rounding, fill);
        if tile.filled && tile.pulse {
            ui.painter().rect_stroke(
                rect.expand(3.0),
                rounding,
                Stroke::new(
                    3.0,
                    Color32::from_rgba_unmultiplied(20, 96, 200, (80.0 + 120.0 * glow) as u8),
                ),
                StrokeKind::Outside,
            );
        } else if !tile.filled {
            let stroke = if tile.active {
                Stroke::new(2.0, SIMPLE_BLUE.gamma_multiply(glow))
            } else {
                Stroke::new(1.0, super::app_theme::HAIRLINE)
            };
            ui.painter()
                .rect_stroke(rect, rounding, stroke, StrokeKind::Inside);
        }
        let icon_scale = if tile.compact {
            if tile.filled { 1.55 } else { 1.35 }
        } else if tile.filled {
            1.85
        } else {
            1.65
        };
        let icon_pos = if tile.compact {
            rect.center()
        } else {
            rect.center() + egui::Vec2::new(0.0, -10.0)
        };
        paint_phone_icon_at(ui.painter(), icon_pos, tile.icon, icon_color, icon_scale);
        if !tile.compact && !tile.label.is_empty() {
            let previous_clip = ui.clip_rect();
            if tile.lock_width {
                ui.set_clip_rect(previous_clip.intersect(rect.shrink(4.0)));
            }
            ui.painter().text(
                rect.center() + egui::Vec2::new(0.0, 24.0),
                egui::Align2::CENTER_CENTER,
                tile.label,
                egui::FontId::proportional(SIMPLE_LABEL),
                ink,
            );
            if tile.lock_width {
                ui.set_clip_rect(previous_clip);
            }
        }
        let tip = if tile.compact && !tile.label.is_empty() {
            format!("{}\n{}", tile.label, tile.tip)
        } else {
            tile.tip.to_string()
        };
        response.clone().on_hover_text(tip);
        (rect, response.clicked())
    }

    fn simple_hold_tile(
        &mut self,
        ui: &mut egui::Ui,
        icon: PhoneIcon,
        label: &str,
        tip: &str,
        width: f32,
        enabled: bool,
    ) -> (egui::Rect, bool) {
        let text_w = ui
            .painter()
            .layout_no_wrap(
                label.to_owned(),
                egui::FontId::proportional(SIMPLE_LABEL),
                SIMPLE_INK,
            )
            .size()
            .x;
        let width = width.max(text_w + 18.0).clamp(SIMPLE_HIT, 120.0);
        let height = SIMPLE_TILE + SIMPLE_LABEL + 8.0;
        let size = egui::Vec2::new(width, height);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let holding = enabled && response.is_pointer_button_down_on();
        let progress = self.simple_delete_hold_progress(holding);
        let fill = if holding {
            super::app_theme::ACCENT_SOFT
        } else if response.hovered() && enabled {
            super::app_theme::ACCENT_HOVER
        } else {
            Color32::WHITE
        };
        ui.painter().rect_filled(rect, 14.0, fill);
        ui.painter().rect_stroke(
            rect,
            14.0,
            Stroke::new(1.0, super::app_theme::HAIRLINE),
            StrokeKind::Inside,
        );
        if progress > 0.0 {
            paint_hold_ring(ui.painter(), rect, progress, SIMPLE_WARN);
        }
        let color = if enabled {
            SIMPLE_BLUE
        } else {
            Color32::from_rgba_unmultiplied(20, 96, 200, 80)
        };
        paint_phone_icon_at(
            ui.painter(),
            rect.center() + egui::Vec2::new(0.0, -10.0),
            icon,
            color,
            1.35,
        );
        let label_ink = if enabled {
            SIMPLE_INK
        } else {
            super::app_theme::SECONDARY_TEXT.gamma_multiply(0.55)
        };
        ui.painter().text(
            rect.center() + egui::Vec2::new(0.0, 24.0),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(SIMPLE_LABEL),
            label_ink,
        );
        response.clone().on_hover_text(format!("{label}\n{tip}"));
        if holding {
            ui.ctx().request_repaint_after(Duration::from_millis(16));
        }
        (rect, enabled && progress >= 1.0)
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

    #[allow(dead_code)]
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

    #[allow(dead_code)]
    fn simple_music_row(&mut self, ui: &mut egui::Ui) {
        let editable = self.is_editable_set_start();
        let bpm = simple_opening_bpm(&self.document.tempo);
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::new(8.0, 8.0);
            if ui
                .add_enabled(
                    editable && bpm > SIMPLE_TEMPO_MIN,
                    egui::Button::new("−").small(),
                )
                .on_hover_text(i18n::registered(self.locale, "simple-mode.220"))
                .clicked()
            {
                self.simple_step_tempo(-SIMPLE_TEMPO_STEP);
            }
            ui.label(
                egui::RichText::new(self.simple_tempo_label())
                    .size(15.0)
                    .strong()
                    .color(SIMPLE_INK),
            );
            if ui
                .add_enabled(
                    editable && bpm < SIMPLE_TEMPO_MAX,
                    egui::Button::new("＋").small(),
                )
                .on_hover_text(i18n::registered(self.locale, "simple-mode.221"))
                .clicked()
            {
                self.simple_step_tempo(SIMPLE_TEMPO_STEP);
            }
            if let Some(seconds) = self.simple_scene_seconds_label() {
                ui.label(
                    egui::RichText::new(seconds)
                        .size(15.0)
                        .color(super::app_theme::SECONDARY_TEXT),
                );
            }
            if let Some(whole) = self.simple_show_length_label() {
                ui.label(
                    egui::RichText::new(whole)
                        .size(15.0)
                        .strong()
                        .color(SIMPLE_BLUE),
                );
            }
            ui.label(
                egui::RichText::new(self.simple_music_caption())
                    .size(15.0)
                    .strong()
                    .color(SIMPLE_BLUE),
            );
        });
    }

    fn simple_tempo_label(&self) -> String {
        let bpm = simple_opening_bpm(&self.document.tempo);
        i18n::registered(self.locale, "simple-mode.219").replace("{0}", &bpm.to_string())
    }

    fn simple_scene_seconds_label(&self) -> Option<String> {
        let seconds = simple_transition_seconds(&self.document, self.current_set)?;
        Some(i18n::registered(self.locale, "simple-mode.225").replace("{0}", &seconds.to_string()))
    }

    fn simple_show_length_label(&self) -> Option<String> {
        if self.document.sets.len() < 2 {
            return None;
        }
        simple_show_seconds(&self.document)
            .map(|seconds| simple_show_length_text(self.locale, seconds))
    }

    fn simple_music_caption(&self) -> String {
        let global = self
            .document
            .global_count(self.current_set, self.count_position);
        simple_music_text(
            self.locale,
            &self.document.tempo,
            global,
            self.count_position,
        )
    }

    /// Four beats per minute at a time, from the opening tempo. A later tempo
    /// change made in the full editor stays. One undo. Positions stay put.
    fn simple_step_tempo(&mut self, delta: i32) {
        if delta == 0 || self.playing || !self.is_editable_set_start() {
            return;
        }
        let current = simple_opening_bpm(&self.document.tempo);
        let next = (current + delta).clamp(SIMPLE_TEMPO_MIN, SIMPLE_TEMPO_MAX);
        if next == current {
            return;
        }
        let tempo = if self.document.tempo.events().len() <= 1 {
            drill_core::tempo::TempoMap::constant(next as f32)
        } else {
            let mut tempo = self.document.tempo.clone();
            tempo.set(0.0, next as f32);
            tempo
        };
        let revision = self.history.revision();
        let changed = self.execute_edit(
            Edit::SetTempoMap { tempo },
            i18n::registered(self.locale, "simple-mode.238"),
        );
        if changed {
            self.tempo_bpm = self.document.tempo.bpm_at(0.0);
            self.simple_mark(revision, "simple-mode.222");
        }
    }

    /// The move this scene shows, played again from its start.
    fn simple_apply_watch_range(&mut self) {
        if self.simple_mode.scene_loop
            && self.show_has_motion()
            && let Some((start, end)) = simple_watch_bounds(&self.document, self.current_set)
        {
            self.playback_start = start;
            self.playback_end = end;
            self.loop_playback = true;
            return;
        }
        self.playback_start = 0;
        self.playback_end = self.document.timeline_counts();
        self.loop_playback = false;
    }

    fn simple_toggle_scene_loop(&mut self) {
        self.simple_mode.scene_loop = !self.simple_mode.scene_loop;
        self.simple_apply_watch_range();
    }

    /// The picture on screen, so the next person is the next one they can see.
    fn simple_cycle_points(&mut self) -> Vec<Point> {
        if self.simple_holding_a_count() {
            let counts = self
                .document
                .sets
                .get(self.current_set)
                .map(|set| set.counts.max(1))
                .unwrap_or(1);
            let progress = (self.count_position / f32::from(counts)).clamp(0.0, 1.0);
            self.document
                .positions_at(self.current_set, progress, &mut self.frame_positions);
            if self.frame_positions.len() == self.document.performers.len() {
                return self.frame_positions.clone();
            }
        }
        self.document
            .sets
            .get(self.current_set)
            .map(|set| set.positions.clone())
            .unwrap_or_default()
    }

    /// One person, from the audience's left. Does not move anyone.
    fn simple_cycle_person(&mut self, forward: bool) {
        if self.playing || self.document.performers.len() < 2 {
            return;
        }
        let points = self.simple_cycle_points();
        if points.len() != self.document.performers.len() {
            return;
        }
        let order = simple_renumber_order(&points);
        if order.is_empty() {
            return;
        }
        let current = if self.selected.len() == 1 {
            self.selected.iter().next().copied()
        } else {
            None
        };
        let next = match current.and_then(|index| order.iter().position(|&item| item == index)) {
            Some(pos) if forward => order[(pos + 1) % order.len()],
            Some(pos) => order[(pos + order.len() - 1) % order.len()],
            None if forward => order[0],
            None => order[order.len() - 1],
        };
        self.replace_selection(std::iter::once(next).collect());
        self.bump_simple_motion(ChromeMotion::Select);
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
    pub(crate) fn simple_step_beat(&mut self, delta: i32) {
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
        if self.playing {
            self.align_simple_listening();
        }
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
        if self.simple_mode.mirror_moves {
            let selected: Vec<usize> = self.selected.iter().copied().collect();
            if selected.len() == points.len() {
                let driven: Vec<(usize, Point)> = selected.into_iter().zip(points).collect();
                self.simple_commit_symmetric_driven(&driven);
            }
        } else {
            self.commit_layout(points);
        }
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

    /// Slides the group sideways until its middle sits on the center line.
    /// The shape stays. With fewer than two chosen, everyone slides.
    fn simple_center_up(&mut self) {
        if !self.simple_arrange_targets() {
            return;
        }
        let points = simple_center_points(&self.document.grid, &self.selected_points());
        self.simple_commit_arrangement(points, "simple-mode.244");
    }

    /// Pulls a rank forward onto the person nearest the audience. A file,
    /// where people already share a left-right spot, is left alone.
    fn simple_dress_front(&mut self) {
        if !self.simple_arrange_targets() {
            return;
        }
        let points = simple_dress_front_points(&self.document.grid, &self.selected_points());
        self.simple_commit_arrangement(points, "simple-mode.246");
    }

    /// A diagonal from the audience's front left toward the back right.
    /// The first person stands at the front left.
    fn simple_diagonal_up(&mut self) {
        if !self.simple_arrange_targets() {
            return;
        }
        let points = simple_diagonal_points(&self.document.grid, &self.selected_points());
        self.simple_commit_arrangement(points, "simple-mode.248");
    }

    /// Pulls a rank back onto the person farthest from the audience. A file,
    /// where people already share a left-right spot, is left alone.
    fn simple_dress_back(&mut self) {
        if !self.simple_arrange_targets() {
            return;
        }
        let points = simple_dress_back_points(&self.document.grid, &self.selected_points());
        self.simple_commit_arrangement(points, "simple-mode.250");
    }

    /// Makes the gaps equal along the longer side of the group. The ends stay.
    /// With fewer than three chosen, everyone is included when the group is
    /// big enough. Two people already have one gap, so there is nothing to even.
    fn simple_even_gaps(&mut self) {
        if !self.is_editable_set_start() || !self.simple_even_ready() {
            return;
        }
        if !self.simple_arrange_targets() || self.selected.len() < 3 {
            return;
        }
        let points = simple_even_points(&self.selected_points());
        self.simple_commit_arrangement(points, "simple-mode.252");
    }

    fn simple_even_ready(&self) -> bool {
        let count = if self.selected.len() >= 2 {
            self.selected.len()
        } else {
            self.document.performers.len()
        };
        count >= 3
    }

    /// A V with the first person at the point, toward the audience. The next
    /// people step back, left then right.
    fn simple_chevron_up(&mut self) {
        if !self.simple_arrange_targets() {
            return;
        }
        let points = simple_chevron_points(&self.document.grid, &self.selected_points());
        self.simple_commit_arrangement(points, "simple-mode.254");
    }

    /// Chooses only the people who change places on the move this scene shows.
    /// Nothing is written.
    fn simple_select_movers(&mut self) {
        if self.playing {
            return;
        }
        let movers = simple_mover_indices(&self.document, self.current_set);
        if movers.is_empty() {
            return;
        }
        self.replace_selection(movers.into_iter().collect());
        self.bump_simple_motion(ChromeMotion::Select);
    }

    /// Chooses only the people who keep their spot while someone else moves.
    /// Nothing is written. An opening picture, where nobody has moved yet,
    /// stays unselected.
    fn simple_select_holders(&mut self) {
        if self.playing {
            return;
        }
        let holders = simple_holder_indices(&self.document, self.current_set);
        if holders.is_empty() {
            return;
        }
        self.replace_selection(holders.into_iter().collect());
        self.bump_simple_motion(ChromeMotion::Select);
    }

    /// The current picture is somewhere else than the previous scene, so a
    /// return scene has a walk to show.
    fn simple_return_ready(&self) -> bool {
        self.simple_can_clear_move()
    }

    /// Adds the next scene on the previous picture, so everyone walks back.
    /// One undo removes that scene. The move that is already written stays.
    fn simple_return_scene(&mut self) {
        if self.playing || !self.is_editable_set_start() || !self.simple_return_ready() {
            return;
        }
        let previous_index = self.current_set - 1;
        let Some(previous) = self
            .document
            .sets
            .get(previous_index)
            .map(|set| set.positions.clone())
        else {
            return;
        };
        let Some(source) = self.document.sets.get(self.current_set).cloned() else {
            return;
        };
        if previous.len() != source.positions.len() {
            return;
        }
        let insert_at = self.current_set + 1;
        let Some(new_id) = self
            .document
            .sets
            .iter()
            .map(|set| set.id.get())
            .max()
            .and_then(|id| id.checked_add(1))
            .and_then(drill_core::SetId::new)
        else {
            return;
        };
        if self.document.sets.iter().any(|set| set.id == new_id) {
            return;
        }
        let mut next = self.document.clone();
        let previous_total = self.document.timeline_counts();
        next.sets.insert(
            insert_at,
            Set {
                id: new_id,
                name: self.simple_default_scene_name(insert_at),
                positions: previous,
                routes: drill_core::RouteTable::default(),
                shape: None,
                generated_by: None,
                annotation: drill_core::SetAnnotation::default(),
                ..source
            },
        );
        let revision = self.history.revision();
        let changed = self.execute_edit(
            Edit::ReplaceDocument {
                document: Box::new(next),
            },
            i18n::registered(self.locale, "simple-mode.261"),
        );
        if !changed {
            return;
        }
        self.current_set = insert_at;
        self.count_position = 0.0;
        self.simple_mode.finished_playback = false;
        self.simple_mode.hold_count = false;
        self.sync_playback_range_to_timeline(previous_total);
        if self.simple_mode.scene_loop {
            self.simple_apply_watch_range();
        }
        self.simple_mark(revision, "simple-mode.259");
        self.bump_simple_motion(ChromeMotion::Move);
    }

    /// Moves this scene one place earlier or later. Stock names stay with the
    /// slot. A typed name stays with the picture. One undo.
    fn simple_shift_scene(&mut self, later: bool) {
        if self.playing || !self.is_editable_set_start() || self.document.sets.len() < 2 {
            return;
        }
        let from = self.current_set;
        let Some(to) = (if later {
            from.checked_add(1)
                .filter(|index| *index < self.document.sets.len())
        } else {
            from.checked_sub(1)
        }) else {
            return;
        };
        let mut next = self.document.clone();
        next.sets.swap(from, to);
        simple_restock_scene_names(&mut next, self.locale);
        let previous_total = self.document.timeline_counts();
        let revision = self.history.revision();
        let changed = self.execute_edit(
            Edit::ReplaceDocument {
                document: Box::new(next),
            },
            i18n::registered(self.locale, "simple-mode.273"),
        );
        if !changed {
            return;
        }
        self.current_set = to;
        self.count_position = 0.0;
        self.simple_mode.finished_playback = false;
        self.simple_mode.hold_count = false;
        self.sync_playback_range_to_timeline(previous_total);
        if self.simple_mode.scene_loop {
            self.simple_apply_watch_range();
        }
        self.simple_mark(
            revision,
            if later {
                "simple-mode.270"
            } else {
                "simple-mode.268"
            },
        );
        self.bump_simple_motion(ChromeMotion::Set);
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

    /// Watch one person, or mark someone sitting out. Neither writes the file.
    #[allow(dead_code)]
    fn simple_watch_row(&mut self, ui: &mut egui::Ui) {
        let one = self.simple_single_selected();
        let away = self.simple_away_count();
        if one.is_none() && away == 0 {
            return;
        }
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::new(8.0, 8.0);
            if one.is_some() {
                if simple_choice_button(
                    ui,
                    i18n::registered(self.locale, "simple-mode.278"),
                    true,
                    self.simple_focus_index().is_some(),
                )
                .on_hover_text(i18n::registered(self.locale, "simple-mode.279"))
                .clicked()
                {
                    self.simple_toggle_focus();
                }
                if simple_choice_button(
                    ui,
                    i18n::registered(self.locale, "simple-mode.280"),
                    true,
                    self.simple_mode.path_sheet_open,
                )
                .on_hover_text(i18n::registered(self.locale, "simple-mode.281"))
                .clicked()
                {
                    self.simple_open_path_sheet();
                }
                let sitting = one.is_some_and(|index| self.simple_is_away(index));
                if simple_choice_button(
                    ui,
                    i18n::registered(self.locale, "simple-mode.287"),
                    true,
                    sitting,
                )
                .on_hover_text(i18n::registered(self.locale, "simple-mode.288"))
                .clicked()
                {
                    self.simple_toggle_away();
                }
            }
            if away > 0 {
                ui.label(
                    egui::RichText::new(
                        i18n::registered(self.locale, "simple-mode.290")
                            .replace("{0}", &away.to_string()),
                    )
                    .size(15.0)
                    .strong()
                    .color(SIMPLE_INK),
                );
            }
        });
    }

    fn simple_single_selected(&self) -> Option<usize> {
        if self.selected.len() != 1 {
            return None;
        }
        self.selected.iter().copied().next()
    }

    fn simple_focus_index(&self) -> Option<usize> {
        let id = self.simple_mode.focus?;
        let index = self
            .document
            .performers
            .iter()
            .position(|performer| performer.id == id)?;
        (self.selected.len() == 1 && self.selected.contains(&index)).then_some(index)
    }

    fn simple_toggle_focus(&mut self) {
        let Some(index) = self.simple_single_selected() else {
            return;
        };
        let Some(id) = self.document.performers.get(index).map(|person| person.id) else {
            return;
        };
        if self.simple_mode.focus == Some(id) {
            self.simple_mode.focus = None;
        } else {
            self.simple_mode.focus = Some(id);
        }
    }

    fn simple_is_away(&self, index: usize) -> bool {
        self.document
            .performers
            .get(index)
            .is_some_and(|person| self.simple_mode.away.contains(&person.id))
    }

    fn simple_away_count(&self) -> usize {
        self.document
            .performers
            .iter()
            .filter(|person| self.simple_mode.away.contains(&person.id))
            .count()
    }

    fn simple_toggle_away(&mut self) {
        let Some(index) = self.simple_single_selected() else {
            return;
        };
        let Some(id) = self.document.performers.get(index).map(|person| person.id) else {
            return;
        };
        if !self.simple_mode.away.remove(&id) {
            self.simple_mode.away.insert(id);
        }
    }

    #[allow(dead_code)]
    fn simple_open_path_sheet(&mut self) {
        let Some(index) = self.simple_single_selected() else {
            return;
        };
        let Some(id) = self.document.performers.get(index).map(|person| person.id) else {
            return;
        };
        let open = !self.simple_mode.path_sheet_open || self.simple_mode.path_sheet_for != Some(id);
        self.simple_close_readouts();
        self.simple_mode.path_sheet_open = open;
        if open {
            self.simple_mode.path_sheet_for = Some(id);
        }
    }

    #[allow(dead_code)]
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
            if simple_choice_button(
                ui,
                i18n::registered(self.locale, "simple-mode.236"),
                true,
                self.simple_mode.scene_loop,
            )
            .on_hover_text(i18n::registered(self.locale, "simple-mode.237"))
            .clicked()
            {
                self.simple_toggle_scene_loop();
            }
            let movers = simple_mover_indices(&self.document, self.current_set);
            if !movers.is_empty() {
                ui.label(
                    egui::RichText::new(
                        i18n::registered(self.locale, "simple-mode.241")
                            .replace("{0}", &movers.len().to_string()),
                    )
                    .size(15.0)
                    .strong()
                    .color(SIMPLE_INK),
                );
                if !self.playing {
                    let chosen = self.selected.len() == movers.len()
                        && movers.iter().all(|index| self.selected.contains(index));
                    if simple_choice_button(
                        ui,
                        i18n::registered(self.locale, "simple-mode.242"),
                        true,
                        chosen,
                    )
                    .on_hover_text(i18n::registered(self.locale, "simple-mode.243"))
                    .clicked()
                    {
                        self.simple_select_movers();
                    }
                }
            }
            let holders = simple_holder_indices(&self.document, self.current_set);
            if !holders.is_empty() {
                ui.label(
                    egui::RichText::new(
                        i18n::registered(self.locale, "simple-mode.256")
                            .replace("{0}", &holders.len().to_string()),
                    )
                    .size(15.0)
                    .strong()
                    .color(SIMPLE_INK),
                );
                if !self.playing {
                    let chosen = self.selected.len() == holders.len()
                        && holders.iter().all(|index| self.selected.contains(index));
                    if simple_choice_button(
                        ui,
                        i18n::registered(self.locale, "simple-mode.257"),
                        true,
                        chosen,
                    )
                    .on_hover_text(i18n::registered(self.locale, "simple-mode.258"))
                    .clicked()
                    {
                        self.simple_select_holders();
                    }
                }
            }
        });
    }

    #[allow(dead_code)]
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
                self.simple_place_button(ui);
                self.simple_show_sheet_button(ui);
                self.simple_walk_button(ui);
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
            if simple_choice_button(
                ui,
                i18n::registered(self.locale, "simple-mode.244"),
                editable,
                false,
            )
            .on_hover_text(i18n::registered(self.locale, "simple-mode.245"))
            .clicked()
            {
                self.simple_center_up();
            }
            if simple_choice_button(
                ui,
                i18n::registered(self.locale, "simple-mode.246"),
                editable,
                false,
            )
            .on_hover_text(i18n::registered(self.locale, "simple-mode.247"))
            .clicked()
            {
                self.simple_dress_front();
            }
            if simple_choice_button(
                ui,
                i18n::registered(self.locale, "simple-mode.248"),
                editable,
                false,
            )
            .on_hover_text(i18n::registered(self.locale, "simple-mode.249"))
            .clicked()
            {
                self.simple_diagonal_up();
            }
            if simple_choice_button(
                ui,
                i18n::registered(self.locale, "simple-mode.250"),
                editable,
                false,
            )
            .on_hover_text(i18n::registered(self.locale, "simple-mode.251"))
            .clicked()
            {
                self.simple_dress_back();
            }
            if self.simple_even_ready()
                && simple_choice_button(
                    ui,
                    i18n::registered(self.locale, "simple-mode.252"),
                    editable,
                    false,
                )
                .on_hover_text(i18n::registered(self.locale, "simple-mode.253"))
                .clicked()
            {
                self.simple_even_gaps();
            }
            if simple_choice_button(
                ui,
                i18n::registered(self.locale, "simple-mode.254"),
                editable,
                false,
            )
            .on_hover_text(i18n::registered(self.locale, "simple-mode.255"))
            .clicked()
            {
                self.simple_chevron_up();
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
            self.simple_place_button(ui);
            self.simple_show_sheet_button(ui);
            self.simple_walk_button(ui);
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
    pub(crate) fn simple_nudge(&mut self, horizontal: i32, vertical: i32) {
        if self.playing
            || !self.is_editable_set_start()
            || self.selected.is_empty()
            || (horizontal == 0 && vertical == 0)
        {
            return;
        }
        let revision = self.history.revision();
        if self.simple_mode.mirror_moves {
            let grid = self.document.grid.clone();
            let dx = grid.horizontal_units / f32::from(grid.horizontal_steps.max(1));
            let dy = grid.vertical_units / f32::from(grid.vertical_steps.max(1));
            let points: Vec<Point> = self
                .selected_points()
                .into_iter()
                .map(|point| Point {
                    x: (point.x + dx * horizontal as f32).clamp(0.0, grid.max_x()),
                    y: (point.y + dy * vertical as f32).clamp(0.0, grid.max_y()),
                })
                .collect();
            let selected: Vec<usize> = self.selected.iter().copied().collect();
            let driven: Vec<(usize, Point)> = selected.into_iter().zip(points).collect();
            self.simple_commit_symmetric_driven(&driven);
        } else {
            self.nudge_selected(horizontal, vertical);
        }
        if self.history.revision() != revision {
            self.simple_mark(revision, "simple-mode.182");
            self.bump_simple_motion(ChromeMotion::Move);
        }
    }

    /// Session-only. Nothing is written into the drill file.
    pub(crate) fn simple_toggle_mirror(&mut self) {
        self.simple_mode.mirror_moves = !self.simple_mode.mirror_moves;
        self.simple_mode.mirror_preview.clear();
    }

    fn simple_mirror_axis(&self) -> f32 {
        self.document.grid.width * 0.5
    }

    fn simple_mirror_tolerance(&self) -> f32 {
        let grid = &self.document.grid;
        let step = grid.horizontal_units / f32::from(grid.horizontal_steps.max(1));
        drill_core::symmetry::center_tolerance(step)
    }

    fn simple_current_positions(&self) -> Option<Vec<Point>> {
        self.document
            .sets
            .get(self.current_set)
            .map(|set| set.positions.clone())
    }

    fn simple_snap_clamp(&self, points: &[Point]) -> Vec<Point> {
        let grid = &self.document.grid;
        points
            .iter()
            .map(|point| {
                let point = grid.snap(*point);
                Point {
                    x: point.x.clamp(0.0, grid.max_x()),
                    y: point.y.clamp(0.0, grid.max_y()),
                }
            })
            .collect()
    }

    fn simple_mirrored_positions(
        &self,
        current: &[Point],
        driven: &[(usize, Point)],
    ) -> Vec<Point> {
        let moved = drill_core::symmetry::mirror_move(
            current,
            self.simple_mirror_axis(),
            self.simple_mirror_tolerance(),
            driven,
        );
        self.simple_snap_clamp(&moved)
    }

    /// One history step for the chosen people and any partner who has to follow.
    fn simple_commit_symmetric_driven(&mut self, driven: &[(usize, Point)]) -> bool {
        let Some(current) = self.simple_current_positions() else {
            return false;
        };
        let next = self.simple_mirrored_positions(&current, driven);
        self.simple_commit_position_list(next)
    }

    fn simple_commit_position_list(&mut self, next: Vec<Point>) -> bool {
        if !self.ensure_editable_set_start() {
            return false;
        }
        let Some(set) = self.document.sets.get(self.current_set) else {
            return false;
        };
        if next.len() != set.positions.len() {
            return false;
        }
        let current = set.positions.clone();
        if next == current {
            return false;
        }
        let generated_by = set.generated_by;
        let set_id = set.id;
        let failure = self.formation_change_failure();
        if let Some(generator_id) = generated_by {
            let mut document = self.document.clone();
            if let Some(set) = document.sets.get_mut(self.current_set) {
                set.positions = next;
            }
            document.detach_generator(generator_id);
            return self.execute_edit(
                Edit::ReplaceDocument {
                    document: Box::new(document),
                },
                failure,
            );
        }
        let mut performer_ids = Vec::new();
        let mut positions = Vec::new();
        for (index, point) in next.iter().enumerate() {
            if current.get(index) == Some(point) {
                continue;
            }
            let Some(performer) = self.document.performers.get(index) else {
                continue;
            };
            performer_ids.push(performer.id);
            positions.push(*point);
        }
        if performer_ids.is_empty() {
            return false;
        }
        self.execute_edit(
            Edit::MovePerformers {
                set_id,
                performer_ids,
                positions,
            },
            failure,
        )
    }

    pub(super) fn sync_simple_mirror_drag(&mut self) {
        self.simple_mode.mirror_preview.clear();
        if !self.simple_mode.mirror_moves {
            return;
        }
        let (Some(before), Some(preview)) = (self.drag_before.clone(), self.drag_preview.clone())
        else {
            return;
        };
        if preview == before {
            return;
        }
        let Some(current) = self.simple_current_positions() else {
            return;
        };
        let selected: Vec<usize> = self.selected.iter().copied().collect();
        if selected.len() != preview.len() {
            return;
        }
        let driven: Vec<(usize, Point)> = selected.iter().copied().zip(preview).collect();
        let snapped = self.simple_mirrored_positions(&current, &driven);
        let Some(preview) = self.drag_preview.as_mut() else {
            return;
        };
        for (slot, &index) in selected.iter().enumerate() {
            if let (Some(slot_point), Some(point)) = (preview.get_mut(slot), snapped.get(index)) {
                *slot_point = *point;
            }
        }
        let partners = drill_core::symmetry::pair_across_center(
            &current,
            self.simple_mirror_axis(),
            self.simple_mirror_tolerance(),
        );
        for &index in &selected {
            let Some(partner) = partners.get(index).copied().flatten() else {
                continue;
            };
            if selected.contains(&partner) {
                continue;
            }
            if let Some(point) = snapped.get(partner).copied()
                && current.get(partner).copied() != Some(point)
            {
                self.simple_mode.mirror_preview.push(point);
            }
        }
    }

    pub(super) fn commit_simple_mirror_drag(&mut self) {
        let Some(before) = self.drag_before.take() else {
            return;
        };
        let after = self.drag_preview.take().unwrap_or_else(|| before.clone());
        self.simple_mode.mirror_preview.clear();
        self.field_viewport.dots_landed();
        self.drag_origin = None;
        if before == after || !self.is_editable_set_start() {
            return;
        }
        let selected: Vec<usize> = self.selected.iter().copied().collect();
        if selected.len() != after.len() {
            return;
        }
        let driven: Vec<(usize, Point)> = selected.into_iter().zip(after).collect();
        if self.simple_commit_symmetric_driven(&driven) {
            self.onboarding.simple_drag_tip_seen = true;
        }
    }

    fn simple_unmirrored_count(&self) -> usize {
        let Some(positions) = self
            .document
            .sets
            .get(self.current_set)
            .map(|set| &set.positions)
        else {
            return 0;
        };
        drill_core::symmetry::unmirrored_count(
            positions,
            self.simple_mirror_axis(),
            self.simple_mirror_tolerance(),
        )
    }

    fn simple_selection_has_partner(&self) -> bool {
        let Some(positions) = self
            .document
            .sets
            .get(self.current_set)
            .map(|set| set.positions.clone())
        else {
            return false;
        };
        let partners = drill_core::symmetry::pair_across_center(
            &positions,
            self.simple_mirror_axis(),
            self.simple_mirror_tolerance(),
        );
        self.selected.iter().any(|&index| {
            partners
                .get(index)
                .copied()
                .flatten()
                .is_some_and(|partner| !self.selected.contains(&partner))
        })
    }

    /// The side the selection sits on. A tie, or a selection that is only the
    /// center line, keeps the left side and moves the right.
    fn simple_master_is_left(&self, positions: &[Point]) -> bool {
        let axis = self.simple_mirror_axis();
        let tolerance = self.simple_mirror_tolerance();
        let mut left = 0usize;
        let mut right = 0usize;
        for &index in &self.selected {
            let Some(point) = positions.get(index) else {
                continue;
            };
            if drill_core::symmetry::on_center_line(*point, axis, tolerance) {
                continue;
            }
            if point.x < axis {
                left += 1;
            } else {
                right += 1;
            }
        }
        left >= right
    }

    fn simple_align_partners(&mut self) {
        if !self.simple_mode.mirror_moves || self.playing || !self.ensure_editable_set_start() {
            return;
        }
        let Some(current) = self.simple_current_positions() else {
            return;
        };
        let aligned = drill_core::symmetry::align_to_side(
            &current,
            self.simple_mirror_axis(),
            self.simple_mirror_tolerance(),
            self.simple_master_is_left(&current),
        );
        let snapped = self.simple_snap_clamp(&aligned);
        let revision = self.history.revision();
        self.simple_commit_position_list(snapped);
        if self.history.revision() != revision {
            self.simple_mark(revision, "simple-mode.323");
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

    #[allow(dead_code)]
    fn simple_touch_row(&mut self, ui: &mut egui::Ui) {
        if self.playing {
            return;
        }
        let editable = self.is_editable_set_start();
        let nudge = editable && !self.selected.is_empty();
        let beside = editable && !self.document.performers.is_empty();
        let clear = editable && self.simple_can_clear_move();
        let go_back = self.simple_return_ready();
        let swap = editable && self.selected.len() == 2;
        let restore = self.simple_can_restore_selected();
        let cycle = self.document.performers.len() >= 2;
        if !nudge && !beside && !clear && !go_back && !swap && !restore && !cycle {
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
        if self.selected.len() == 2
            && let Some(gap) = self.simple_gap_caption()
        {
            ui.label(
                egui::RichText::new(gap)
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
            if cycle
                && simple_choice_button(
                    ui,
                    i18n::registered(self.locale, "simple-mode.234"),
                    true,
                    false,
                )
                .on_hover_text(i18n::registered(self.locale, "simple-mode.235"))
                .clicked()
            {
                self.simple_cycle_person(false);
            }
            if cycle
                && simple_choice_button(
                    ui,
                    i18n::registered(self.locale, "simple-mode.232"),
                    true,
                    false,
                )
                .on_hover_text(i18n::registered(self.locale, "simple-mode.233"))
                .clicked()
            {
                self.simple_cycle_person(true);
            }
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
            if go_back
                && simple_choice_button(
                    ui,
                    i18n::registered(self.locale, "simple-mode.259"),
                    editable,
                    false,
                )
                .on_hover_text(i18n::registered(self.locale, "simple-mode.260"))
                .clicked()
            {
                self.simple_return_scene();
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

    /// How far apart the two chosen people are, in steps. While a count is
    /// held, this is the picture on screen. Reading it never writes.
    fn simple_gap_caption(&self) -> Option<String> {
        if self.selected.len() != 2 {
            return None;
        }
        let mut chosen = self.selected.iter().copied();
        let first = chosen.next()?;
        let second = chosen.next()?;
        let (left, right) = if self.simple_holding_a_count() {
            let counts = self.document.sets.get(self.current_set)?.counts.max(1);
            let progress = (self.count_position / f32::from(counts)).clamp(0.0, 1.0);
            let mut scratch = Vec::new();
            self.document
                .positions_at(self.current_set, progress, &mut scratch);
            (*scratch.get(first)?, *scratch.get(second)?)
        } else {
            let positions = &self.document.sets.get(self.current_set)?.positions;
            (*positions.get(first)?, *positions.get(second)?)
        };
        simple_gap_text(self.locale, &self.document.grid, left, right)
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

    /// Sheets and naming own the keyboard. The central shortcut poll asks
    /// this before it will nudge.
    pub(crate) fn key_nudge_allowed(&self) -> bool {
        if self.playing || self.selected.is_empty() || !self.is_editable_set_start() {
            return false;
        }
        if self.simple_mode.enabled
            && (self.simple_mode.naming
                || self.simple_mode.memo_open
                || self.simple_mode.place_sheet_open
                || self.simple_mode.show_sheet_open
                || self.show_recent_projects)
        {
            return false;
        }
        true
    }

    #[cfg(test)]
    pub(crate) fn key_mirror_on(&self) -> bool {
        self.simple_mode.mirror_moves
    }

    pub(crate) fn toggle_previous_ghost(&mut self) {
        if !simple_previous_toggle_enabled(self.current_set) {
            return;
        }
        self.set_show_previous(!self.simple_mode.show_previous);
    }

    #[allow(dead_code)]
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

    /// People standing outside the contest line, at this picture or the next.
    #[allow(dead_code)]
    fn simple_outside_row(&mut self, ui: &mut egui::Ui) {
        let outside = outside_contest_indices(&self.document, self.current_set);
        if outside.is_empty() {
            return;
        }
        let button_id = if outside.len() > 1 {
            "simple-mode.299"
        } else {
            "simple-mode.298"
        };
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::new(8.0, 8.0);
            ui.label(
                egui::RichText::new(
                    i18n::registered(self.locale, "simple-mode.297")
                        .replace("{0}", &outside.len().to_string()),
                )
                .size(15.0)
                .strong()
                .color(SIMPLE_INK),
            );
            if simple_choice_button(ui, i18n::registered(self.locale, button_id), true, false)
                .on_hover_text(i18n::registered(self.locale, "simple-mode.300"))
                .clicked()
            {
                self.replace_selection(outside.into_iter().collect());
            }
        });
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
            let open = !self.simple_mode.memo_open;
            self.simple_close_readouts();
            self.simple_mode.memo_open = open;
        }
    }

    fn simple_show_sheet_button(&mut self, ui: &mut egui::Ui) {
        if simple_choice_button(
            ui,
            i18n::registered(self.locale, "simple-mode.274"),
            true,
            self.simple_mode.show_sheet_open,
        )
        .on_hover_text(i18n::registered(self.locale, "simple-mode.275"))
        .clicked()
        {
            let open = !self.simple_mode.show_sheet_open;
            self.simple_close_readouts();
            self.simple_mode.show_sheet_open = open;
        }
    }

    fn simple_walk_button(&mut self, ui: &mut egui::Ui) {
        if simple_choice_button(
            ui,
            i18n::registered(self.locale, "simple-mode.291"),
            true,
            self.simple_mode.walk_sheet_open,
        )
        .on_hover_text(i18n::registered(self.locale, "simple-mode.292"))
        .clicked()
        {
            let open = !self.simple_mode.walk_sheet_open;
            self.simple_close_readouts();
            self.simple_mode.walk_sheet_open = open;
        }
    }

    fn simple_place_button(&mut self, ui: &mut egui::Ui) {
        if simple_choice_button(
            ui,
            i18n::registered(self.locale, "simple-mode.226"),
            true,
            self.simple_mode.place_sheet_open,
        )
        .on_hover_text(i18n::registered(self.locale, "simple-mode.227"))
        .clicked()
        {
            let open = !self.simple_mode.place_sheet_open;
            self.simple_close_readouts();
            self.simple_mode.place_sheet_open = open;
        }
    }

    fn simple_place_sheet(&mut self, ctx: &egui::Context) {
        if !self.simple_mode.place_sheet_open {
            return;
        }
        let sheet = simple_position_sheet(&self.document, self.current_set, self.locale);
        let mut open = true;
        let mut copy = false;
        let copied = self.simple_mode.place_sheet_copied;
        super::app_theme::modal_window(i18n::registered(self.locale, "simple-mode.228"), ctx)
            .id(egui::Id::new("simple-place-sheet"))
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
                        ui.label(egui::RichText::new(&sheet).size(16.0).color(SIMPLE_INK));
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
            ctx.copy_text(sheet);
            self.simple_mode.place_sheet_copied = true;
        }
        if !open {
            self.simple_mode.place_sheet_open = false;
            self.simple_mode.place_sheet_copied = false;
        }
    }

    fn simple_show_sheet(&mut self, ctx: &egui::Context) {
        if !self.simple_mode.show_sheet_open {
            return;
        }
        let sheet = simple_show_sheet_text(&self.document, self.locale);
        let mut open = true;
        let mut copy = false;
        let copied = self.simple_mode.show_sheet_copied;
        super::app_theme::modal_window(i18n::registered(self.locale, "simple-mode.277"), ctx)
            .id(egui::Id::new("simple-show-sheet"))
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
                        ui.label(egui::RichText::new(&sheet).size(16.0).color(SIMPLE_INK));
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
            ctx.copy_text(sheet);
            self.simple_mode.show_sheet_copied = true;
        }
        if !open {
            self.simple_mode.show_sheet_open = false;
            self.simple_mode.show_sheet_copied = false;
        }
    }

    fn simple_path_sheet(&mut self, ctx: &egui::Context) {
        if !self.simple_mode.path_sheet_open {
            return;
        }
        let Some(index) = self.simple_mode.path_sheet_for.and_then(|id| {
            self.document
                .performers
                .iter()
                .position(|person| person.id == id)
        }) else {
            self.simple_mode.path_sheet_open = false;
            self.simple_mode.path_sheet_for = None;
            return;
        };
        let sheet = simple_person_path_text(&self.document, index, self.locale);
        let mut open = true;
        let mut copy = false;
        let copied = self.simple_mode.path_sheet_copied;
        super::app_theme::modal_window(i18n::registered(self.locale, "simple-mode.282"), ctx)
            .id(egui::Id::new("simple-path-sheet"))
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
                        ui.label(egui::RichText::new(&sheet).size(16.0).color(SIMPLE_INK));
                    });
                ui.add_space(12.0);
                if ui
                    .add_sized([160.0, 40.0], simple_copy_button(self.locale, copied))
                    .clicked()
                {
                    copy = true;
                }
            });
        if copy {
            ctx.copy_text(sheet);
            self.simple_mode.path_sheet_copied = true;
        }
        if !open {
            self.simple_mode.path_sheet_open = false;
            self.simple_mode.path_sheet_for = None;
            self.simple_mode.path_sheet_copied = false;
        }
    }

    fn simple_walk_sheet(&mut self, ctx: &egui::Context) {
        if !self.simple_mode.walk_sheet_open {
            return;
        }
        let sheet = self.simple_walk_text();
        let mut open = true;
        let mut copy = false;
        let mut save = false;
        let copied = self.simple_mode.walk_sheet_copied;
        let save_id = if self.simple_mode.walk_sheet_failed {
            "simple-mode.296"
        } else if self.simple_mode.walk_sheet_saved {
            "simple-mode.295"
        } else {
            "simple-mode.294"
        };
        super::app_theme::modal_window(i18n::registered(self.locale, "simple-mode.293"), ctx)
            .id(egui::Id::new("simple-walk-sheet"))
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
                        ui.label(egui::RichText::new(&sheet).size(16.0).color(SIMPLE_INK));
                    });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = egui::Vec2::new(8.0, 8.0);
                    if ui
                        .add_sized([120.0, 40.0], simple_copy_button(self.locale, copied))
                        .clicked()
                    {
                        copy = true;
                    }
                    let save_button = egui::Button::new(
                        egui::RichText::new(i18n::registered(self.locale, save_id))
                            .size(16.0)
                            .color(SIMPLE_BLUE),
                    )
                    .fill(Color32::WHITE)
                    .stroke(Stroke::new(1.5, SIMPLE_BLUE))
                    .corner_radius(18.0);
                    if ui.add_sized([160.0, 40.0], save_button).clicked() {
                        save = true;
                    }
                });
            });
        if copy {
            ctx.copy_text(sheet);
            self.simple_mode.walk_sheet_copied = true;
        }
        if save {
            self.simple_save_walk_file();
        }
        if !open {
            self.simple_mode.walk_sheet_open = false;
            self.simple_mode.walk_sheet_copied = false;
            self.simple_mode.walk_sheet_saved = false;
            self.simple_mode.walk_sheet_failed = false;
        }
    }

    fn simple_walk_text(&self) -> String {
        rehearsal_walk_sheet(
            &self.document,
            self.locale,
            self.simple_show_title().as_deref(),
        )
    }

    fn simple_save_walk_file(&mut self) {
        let text = rehearsal_walk_file(&self.simple_walk_text());
        let stem = self
            .simple_show_title()
            .unwrap_or_else(|| self.document.title.clone());
        let name = format!("{}-walk.txt", simple_show_file_stem(&stem));
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Text", &["txt"])
            .set_file_name(name)
            .save_file()
        else {
            return;
        };
        match std::fs::write(&path, text.as_bytes()) {
            Ok(()) => {
                self.simple_mode.walk_sheet_saved = true;
                self.simple_mode.walk_sheet_failed = false;
            }
            Err(_) => {
                self.simple_mode.walk_sheet_saved = false;
                self.simple_mode.walk_sheet_failed = true;
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
        super::app_theme::modal_window(i18n::registered(self.locale, "simple-mode.192"), ctx)
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

    fn simple_paint_field(&mut self, ui: &mut egui::Ui) {
        self.simple_refresh_held_picture();
        let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
        let shake = self
            .simple_fail_t()
            .map(|t| (t * 40.0).sin() * 4.0 * (1.0 - t))
            .unwrap_or(0.0);
        let card = rect.translate(egui::Vec2::new(shake, 0.0));
        self.simple_mode.field_card = Some(card);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(card));
        self.simple_field_full_ui(&mut child, true);
        if self.playing {
            let pulse = 0.55 + 0.45 * (ui.input(|input| input.time) as f32 * 1.6).sin().abs();
            let color = Color32::from_rgba_unmultiplied(20, 96, 200, (pulse * 180.0) as u8);
            ui.painter()
                .rect_stroke(card, 0.0, Stroke::new(4.0, color), StrokeKind::Inside);
        }
        if self.simple_fail_t().is_some() {
            ui.painter()
                .rect_stroke(card, 0.0, Stroke::new(3.0, SIMPLE_WARN), StrokeKind::Inside);
        }
        if let Some((ChromeMotion::Set, ease, _)) = self.simple_motion_ease()
            && self.nav_glide.position().is_none()
        {
            let alpha = ((1.0 - ease) * 110.0) as u8;
            ui.painter()
                .rect_filled(card, 0.0, Color32::from_white_alpha(alpha));
        }
    }

    fn simple_toolbar_open(&self) -> bool {
        !self.onboarding.simple_toolbar_collapsed || self.simple_active_hint().is_some()
    }

    fn simple_paint_floating_layer(&mut self, ui: &mut egui::Ui) {
        let Some(field) = self.simple_mode.field_card else {
            return;
        };
        let ctx = ui.ctx().clone();
        let mut block = None;
        let fade = if self.drag_before.is_some() {
            0.22
        } else {
            1.0
        };
        self.simple_overlay_members(&ctx, field, &mut block);
        self.simple_overlay_chrome(&ctx, field, &mut block);
        self.simple_overlay_recovery(&ctx, field, &mut block);
        // A modal owns the bottom of the window. Hide the floating strips so
        // they cannot cover the reset row, the key chart, or a sheet.
        if !self.simple_modal_open() {
            self.simple_overlay_symmetry(&ctx, field, &mut block);
            let mut toolbar_top = field.bottom() - 8.0;
            if self.simple_toolbar_open() {
                if let Some(rect) = self.simple_overlay_toolbar(&ctx, field, fade, &mut block) {
                    toolbar_top = rect.top();
                }
            } else if let Some(rect) = self.simple_overlay_toolbar_toggle(&ctx, field, &mut block) {
                toolbar_top = rect.top();
            }
            self.simple_overlay_song(&ctx, field, toolbar_top, &mut block);
            let sets_anchor = toolbar_top - 8.0;
            let sets_rect = self.simple_overlay_sets(&ctx, field, sets_anchor, &mut block);
            self.simple_overlay_front_labels(&ctx, field, sets_anchor, sets_rect, &mut block);
        }
        self.simple_mode.overlay_block = block;
    }

    /// 客席 / 左 / 右 sit on the front sideline, above the set strip and the dock.
    fn simple_overlay_front_labels(
        &self,
        ctx: &egui::Context,
        field: egui::Rect,
        dock_ceiling: f32,
        sets: Option<egui::Rect>,
        block: &mut Option<egui::Rect>,
    ) {
        let options = self.field_render_options(false);
        let map = drill_render::FieldMap::with_view(
            self.document.grid.width,
            self.document.grid.height,
            drill_render::Vec2 {
                x: field.width(),
                y: field.height(),
            },
            options.margin,
            options.field_center,
            options.field_zoom,
        );
        let to_screen = |point: drill_core::Point| {
            let mapped = map.map(point);
            Pos2::new(field.left() + mapped.x, field.top() + mapped.y)
        };
        let (left, right) = simple_side_anchors(&self.document.grid);
        let marks = [
            (left, "simple-mode.206"),
            (right, "simple-mode.207"),
            (
                simple_audience_point(&self.document.grid),
                "simple-mode.146",
            ),
        ];
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("simple-front-labels"),
        ));
        for (point, id) in marks {
            let anchor = to_screen(point);
            let ceiling = sets
                .filter(|strip| anchor.x < strip.right() + 12.0)
                .map(|strip| strip.top() - 8.0)
                .unwrap_or(dock_ceiling);
            let chip = paint_front_chip(
                &painter,
                field,
                anchor,
                ceiling,
                i18n::registered(self.locale, id),
            );
            overlay_union(block, chip);
        }
    }

    fn simple_overlay_members(
        &mut self,
        ctx: &egui::Context,
        field: egui::Rect,
        block: &mut Option<egui::Rect>,
    ) {
        if self.document.performers.is_empty() && !self.playing {
            return;
        }
        let pos = field.left_top() + egui::Vec2::new(10.0, 10.0);
        let response = egui::Area::new(egui::Id::new("simple-overlay-members"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 210))
                    .stroke(Stroke::new(1.0, super::app_theme::HAIRLINE))
                    .corner_radius(16.0)
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            if !self.document.performers.is_empty() {
                                self.simple_member_count(ui);
                            }
                            if self.playing {
                                let total = self
                                    .document
                                    .sets
                                    .get(self.current_set)
                                    .map(|set| set.counts)
                                    .unwrap_or(1);
                                let now =
                                    (self.count_position.round() as i32).clamp(0, i32::from(total));
                                ui.label(
                                    egui::RichText::new(format!("{now} / {total}"))
                                        .size(18.0)
                                        .strong()
                                        .color(SIMPLE_BLUE),
                                );
                            }
                        });
                    });
            });
        overlay_union(block, response.response.rect);
    }

    fn simple_overlay_sets(
        &mut self,
        ctx: &egui::Context,
        field: egui::Rect,
        bottom: f32,
        block: &mut Option<egui::Rect>,
    ) -> Option<egui::Rect> {
        let set_count = self.document.sets.len();
        if set_count == 0 {
            return None;
        }
        let pos = egui::pos2(field.left() + 10.0, bottom);
        let response = egui::Area::new(egui::Id::new("simple-overlay-sets"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::LEFT_BOTTOM)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 210))
                    .stroke(Stroke::new(1.0, super::app_theme::HAIRLINE))
                    .corner_radius(20.0)
                    .inner_margin(egui::Margin::symmetric(6, 4))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            for index in 0..set_count {
                                self.simple_scene_chip(ui, index);
                            }
                            self.simple_previous_toggle(ui);
                        });
                    });
            });
        overlay_union(block, response.response.rect);
        Some(response.response.rect)
    }

    fn simple_overlay_chrome(
        &mut self,
        ctx: &egui::Context,
        _field: egui::Rect,
        block: &mut Option<egui::Rect>,
    ) {
        let response = egui::Area::new(egui::Id::new("simple-overlay-chrome"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::RIGHT_TOP, egui::Vec2::new(-10.0, 10.0))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 210))
                    .stroke(Stroke::new(1.0, super::app_theme::HAIRLINE))
                    .corner_radius(18.0)
                    .inner_margin(egui::Margin::symmetric(6, 4))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            if self.simple_check_t().is_some() {
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::Vec2::splat(SIMPLE_HIT),
                                    Sense::hover(),
                                );
                                paint_phone_icon_at(
                                    ui.painter(),
                                    rect.center(),
                                    PhoneIcon::Check,
                                    SIMPLE_OK,
                                    1.15,
                                );
                            }
                            if self.playing {
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::Vec2::splat(SIMPLE_HIT),
                                    Sense::hover(),
                                );
                                paint_phone_icon_at(
                                    ui.painter(),
                                    rect.center(),
                                    PhoneIcon::Lock,
                                    SIMPLE_BLUE,
                                    1.15,
                                );
                                ui.interact(rect, ui.id().with("simple-lock"), Sense::hover())
                                    .on_hover_text(i18n::registered(
                                        self.locale,
                                        "simple-mode.315",
                                    ));
                            }
                            let (open_rect, open_click) = self.simple_pictogram_tile(
                                ui,
                                PictogramTile {
                                    icon: PhoneIcon::Open,
                                    label: i18n::registered(self.locale, "simple-mode.080"),
                                    tip: i18n::registered(self.locale, "simple-mode.094"),
                                    width: SIMPLE_HIT,
                                    enabled: true,
                                    active: false,
                                    pulse: false,
                                    filled: false,
                                    compact: true,
                                    time: 0.0,
                                    lock_width: false,
                                },
                            );
                            let _ = open_rect;
                            if open_click {
                                self.simple_open_shows();
                            }
                            let (more_rect, more_click) = self.simple_pictogram_tile(
                                ui,
                                PictogramTile {
                                    icon: PhoneIcon::More,
                                    label: i18n::registered(self.locale, "simple-mode.079"),
                                    tip: i18n::registered(self.locale, "simple-mode.095"),
                                    width: SIMPLE_HIT,
                                    enabled: true,
                                    active: false,
                                    pulse: false,
                                    filled: false,
                                    compact: true,
                                    time: 0.0,
                                    lock_width: false,
                                },
                            );
                            let _ = more_rect;
                            if more_click {
                                self.set_simple_mode(false);
                            }
                            if self.simple_active_hint().is_some() {
                                let (close_rect, close_click) = self.simple_pictogram_tile(
                                    ui,
                                    PictogramTile {
                                        icon: PhoneIcon::Close,
                                        label: "",
                                        tip: i18n::registered(self.locale, "simple-mode.313"),
                                        width: SIMPLE_HIT,
                                        enabled: true,
                                        active: false,
                                        pulse: false,
                                        filled: false,
                                        compact: false,
                                        time: 0.0,
                                        lock_width: false,
                                    },
                                );
                                let _ = close_rect;
                                if close_click {
                                    self.simple_mode.hint_closed = true;
                                }
                            }
                        });
                    });
            });
        overlay_union(block, response.response.rect);
    }

    fn simple_overlay_recovery(
        &mut self,
        ctx: &egui::Context,
        field: egui::Rect,
        block: &mut Option<egui::Rect>,
    ) {
        if self.project_state.recoveries.is_empty() {
            return;
        }
        let pos = Pos2::new(field.center().x - 180.0, field.top() + 10.0);
        let response = egui::Area::new(egui::Id::new("simple-overlay-recovery"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                self.simple_recovery_banner(ui);
            });
        overlay_union(block, response.response.rect);
    }

    /// Warning and the match button float on the field. They do not take a row.
    fn simple_overlay_symmetry(
        &mut self,
        ctx: &egui::Context,
        field: egui::Rect,
        block: &mut Option<egui::Rect>,
    ) {
        if !self.simple_mode.mirror_moves || self.playing {
            return;
        }
        let count = self.simple_unmirrored_count();
        let show_rule = self.simple_selection_has_partner();
        if count == 0 && !show_rule {
            return;
        }
        let pos = Pos2::new(field.center().x, field.top() + 64.0);
        let response = egui::Area::new(egui::Id::new("simple-overlay-symmetry"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::CENTER_TOP)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 230))
                    .stroke(Stroke::new(1.5, SIMPLE_ORANGE))
                    .corner_radius(16.0)
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.set_max_width((field.width() - 24.0).max(160.0));
                        self.simple_symmetry_overlay(ui);
                    });
            });
        overlay_union(block, response.response.rect);
    }

    fn simple_overlay_toolbar(
        &mut self,
        ctx: &egui::Context,
        field: egui::Rect,
        fade: f32,
        block: &mut Option<egui::Rect>,
    ) -> Option<egui::Rect> {
        let pos = Pos2::new(field.center().x, field.bottom() - 14.0);
        let response = egui::Area::new(egui::Id::new("simple-overlay-toolbar"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::CENTER_BOTTOM)
            .fixed_pos(pos)
            .sense(Sense::click())
            .show(ctx, |ui| {
                ui.set_opacity(fade);
                egui::Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 230))
                    .stroke(Stroke::new(1.0, super::app_theme::HAIRLINE))
                    .corner_radius(28.0)
                    .inner_margin(egui::Margin::symmetric(8, 8))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        self.simple_action_bar(ui);
                    });
            });
        overlay_union(block, response.response.rect);
        Some(response.response.rect)
    }

    fn simple_overlay_toolbar_toggle(
        &mut self,
        ctx: &egui::Context,
        field: egui::Rect,
        block: &mut Option<egui::Rect>,
    ) -> Option<egui::Rect> {
        let pos = Pos2::new(field.center().x, field.bottom() - 14.0);
        let response = egui::Area::new(egui::Id::new("simple-overlay-toolbar-toggle"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::CENTER_BOTTOM)
            .fixed_pos(pos)
            .sense(Sense::click())
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 230))
                    .stroke(Stroke::new(1.0, super::app_theme::HAIRLINE))
                    .corner_radius(18.0)
                    .inner_margin(egui::Margin::symmetric(4, 4))
                    .show(ui, |ui| {
                        let toolbar_tip = self.key_tip(
                            ui.ctx(),
                            i18n::registered(self.locale, "simple-mode.319"),
                            super::keybindings::Action::ToggleToolbar,
                        );
                        let (rect, click) = self.simple_pictogram_tile(
                            ui,
                            PictogramTile {
                                icon: PhoneIcon::Tools,
                                label: i18n::registered(self.locale, "simple-mode.318"),
                                tip: &toolbar_tip,
                                width: SIMPLE_HIT,
                                enabled: true,
                                active: false,
                                pulse: false,
                                filled: false,
                                compact: true,
                                time: 0.0,
                                lock_width: false,
                            },
                        );
                        let _ = rect;
                        if click {
                            self.onboarding.simple_toolbar_collapsed = false;
                        }
                    });
            });
        overlay_union(block, response.response.rect);
        Some(response.response.rect)
    }

    #[allow(dead_code)]
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
        ui.painter().rect_filled(dock, 28.0, SIMPLE_CARD);
        ui.painter().rect_stroke(
            dock,
            28.0,
            Stroke::new(1.0, super::app_theme::HAIRLINE),
            StrokeKind::Inside,
        );
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
    }

    fn simple_remove_bar(&mut self, ui: &mut egui::Ui) {
        let label = if self.selected.len() == 1 {
            "simple-mode.097"
        } else {
            "simple-mode.098"
        };
        ui.horizontal(|ui| {
            ui.add_space(((ui.available_width() - SIMPLE_TILE) * 0.5).max(0.0));
            let (rect, response) = ui.allocate_exact_size(
                egui::Vec2::new(SIMPLE_TILE, SIMPLE_TILE + SIMPLE_LABEL + 8.0),
                Sense::click_and_drag(),
            );
            let holding = response.is_pointer_button_down_on();
            let progress = self.simple_delete_hold_progress(holding);
            let fill = if holding {
                super::app_theme::ACCENT_SOFT
            } else if response.hovered() {
                super::app_theme::ACCENT_HOVER
            } else {
                Color32::WHITE
            };
            ui.painter().rect_filled(rect, 16.0, fill);
            ui.painter().rect_stroke(
                rect,
                16.0,
                Stroke::new(1.5, SIMPLE_BLUE),
                StrokeKind::Inside,
            );
            if progress > 0.0 {
                paint_hold_ring(ui.painter(), rect, progress, SIMPLE_WARN);
            }
            paint_phone_icon_at(
                ui.painter(),
                rect.center() + egui::Vec2::new(0.0, -10.0),
                PhoneIcon::Trash,
                SIMPLE_BLUE,
                1.65,
            );
            ui.painter().text(
                rect.center() + egui::Vec2::new(0.0, 24.0),
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(SIMPLE_LABEL),
                SIMPLE_BLUE,
            );
            response.clone().on_hover_text(label);
            if holding {
                ui.ctx().request_repaint_after(Duration::from_millis(16));
            }
            if progress >= 1.0 {
                self.simple_commit_held_delete();
            }
        });
    }

    fn simple_delete_hold_progress(&mut self, holding: bool) -> f32 {
        if !holding {
            self.simple_mode.delete_hold = None;
            return 0.0;
        }
        let started = *self
            .simple_mode
            .delete_hold
            .get_or_insert_with(Instant::now);
        (started.elapsed().as_secs_f32() / SIMPLE_DELETE_HOLD_SECS).clamp(0.0, 1.0)
    }

    fn simple_commit_held_delete(&mut self) {
        if !self.simple_can_remove() {
            self.simple_mode.delete_hold = None;
            self.simple_fail();
            return;
        }
        self.simple_mode.delete_hold = None;
        self.simple_mode.overlap_note = false;
        let note = if self.selected.len() == 1 {
            "simple-mode.097"
        } else {
            "simple-mode.098"
        };
        let revision = self.history.revision();
        self.remove_selected_performers();
        self.simple_mark(revision, note);
        self.simple_flash_check();
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
            let button_w = ((ui.available_width() - gaps) / count).clamp(SIMPLE_HIT, 112.0);
            let row = button_w * count + gaps;
            ui.add_space(((ui.available_width() - row) / 2.0).max(0.0));
            for (id, tip, icon, action) in labels {
                let enabled = match action {
                    DockAction::Undo => self.history.can_undo(),
                    DockAction::Redo => self.history.can_redo(),
                    DockAction::Open | DockAction::More => true,
                };
                let response = self
                    .simple_dock_button(ui, icon, id, button_w, enabled)
                    .on_hover_text(tip);
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
            egui::Vec2::new(width, SIMPLE_HIT + 24.0),
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
        paint_phone_icon_at(
            ui.painter(),
            rect.center() + egui::Vec2::new(0.0, -10.0),
            icon,
            color,
            1.25,
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

    #[allow(dead_code)]
    fn simple_primary(&mut self, ui: &mut egui::Ui) {
        let Some(action) = self.simple_primary_action() else {
            return;
        };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
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
            let width = ui.available_width().max(48.0);
            let (rect, response) =
                ui.allocate_exact_size(egui::Vec2::new(width, 56.0), Sense::click());
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
        });
    }

    /// Song name and sync controls, floating above the toolbar. The field
    /// stays full-window; this pill only steals its own clicks.
    fn simple_modal_open(&self) -> bool {
        self.keybindings.settings_open
            || self.keybindings.cheat_open
            || self.glossary.open
            || self.simple_mode.naming
            || self.simple_mode.memo_open
            || self.simple_mode.place_sheet_open
            || self.simple_mode.show_sheet_open
            || self.simple_mode.path_sheet_open
            || self.simple_mode.walk_sheet_open
            || self.show_recent_projects
            || self.onboarding.show_help
            || self.onboarding.show_welcome
            || self.command_palette.is_open()
    }

    fn simple_overlay_song(
        &mut self,
        ctx: &egui::Context,
        field: egui::Rect,
        toolbar_top: f32,
        block: &mut Option<egui::Rect>,
    ) {
        if self.document.audio.is_none() {
            return;
        }
        let pos = Pos2::new(field.center().x, toolbar_top - 8.0);
        let response = egui::Area::new(egui::Id::new("simple-overlay-song"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::CENTER_BOTTOM)
            .fixed_pos(pos)
            .sense(Sense::click())
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 230))
                    .stroke(Stroke::new(1.0, super::app_theme::HAIRLINE))
                    .corner_radius(22.0)
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        self.simple_song_overlay_ui(ui);
                    });
            });
        overlay_union(block, response.response.rect);
    }

    fn simple_song_overlay_ui(&mut self, ui: &mut egui::Ui) {
        if self.simple_mode.song_missing {
            ui.label(
                egui::RichText::new(i18n::registered(self.locale, "simple-mode.347"))
                    .size(14.0)
                    .color(SIMPLE_WARN),
            );
            return;
        }
        let time = ui.input(|input| input.time) as f32;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if let Some(track) = &self.document.audio {
                let name = simple_song_name(&track.path);
                ui.label(
                    egui::RichText::new(
                        i18n::registered(self.locale, "simple-mode.342").replace("{0}", &name),
                    )
                    .size(14.0)
                    .color(SIMPLE_INK),
                );
            }
            let downbeat_label = i18n::registered(self.locale, "simple-mode.343");
            let downbeat_tip = self.key_tip(
                ui.ctx(),
                downbeat_label,
                super::keybindings::Action::MarkDownbeat,
            );
            let (downbeat, downbeat_click) = self.simple_pictogram_tile(
                ui,
                PictogramTile {
                    icon: PhoneIcon::Note,
                    label: downbeat_label,
                    tip: &downbeat_tip,
                    width: 108.0,
                    enabled: true,
                    active: false,
                    pulse: false,
                    filled: false,
                    compact: false,
                    time,
                    lock_width: false,
                },
            );
            let _ = downbeat;
            if downbeat_click {
                self.simple_mark_downbeat();
            }
            let earlier_label = i18n::registered(self.locale, "simple-mode.344");
            let earlier_tip = self.key_tip(
                ui.ctx(),
                earlier_label,
                super::keybindings::Action::NudgeSongEarlier,
            );
            let (earlier, earlier_click) = self.simple_pictogram_tile(
                ui,
                PictogramTile {
                    icon: PhoneIcon::Minus,
                    label: earlier_label,
                    tip: &earlier_tip,
                    width: SIMPLE_HIT,
                    enabled: true,
                    active: false,
                    pulse: false,
                    filled: false,
                    compact: false,
                    time,
                    lock_width: false,
                },
            );
            let _ = earlier;
            if earlier_click {
                self.simple_nudge_song(-1);
            }
            let later_label = i18n::registered(self.locale, "simple-mode.345");
            let later_tip = self.key_tip(
                ui.ctx(),
                later_label,
                super::keybindings::Action::NudgeSongLater,
            );
            let (later, later_click) = self.simple_pictogram_tile(
                ui,
                PictogramTile {
                    icon: PhoneIcon::Plus,
                    label: later_label,
                    tip: &later_tip,
                    width: SIMPLE_HIT,
                    enabled: true,
                    active: false,
                    pulse: false,
                    filled: false,
                    compact: false,
                    time,
                    lock_width: false,
                },
            );
            let _ = later;
            if later_click {
                self.simple_nudge_song(1);
            }
            let remove_label = i18n::registered(self.locale, "simple-mode.346");
            let remove_tip = self.key_tip(
                ui.ctx(),
                remove_label,
                super::keybindings::Action::RemoveSong,
            );
            let (remove, remove_click) = self.simple_pictogram_tile(
                ui,
                PictogramTile {
                    icon: PhoneIcon::NoteOff,
                    label: remove_label,
                    tip: &remove_tip,
                    width: 88.0,
                    enabled: true,
                    active: false,
                    pulse: false,
                    filled: false,
                    compact: false,
                    time,
                    lock_width: false,
                },
            );
            let _ = remove;
            if remove_click {
                self.simple_remove_song();
            }
        });
    }

    pub(crate) fn simple_pick_and_import_song(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter(
                i18n::registered(self.locale, "simple-mode.349"),
                &["mp3", "wav"],
            )
            .pick_file()
        else {
            return;
        };
        let _ = self.simple_import_song(path);
    }

    fn simple_import_song(&mut self, path: std::path::PathBuf) -> bool {
        if !simple_song_extension_ok(&path) || !path.is_file() {
            return false;
        }
        let track = drill_core::audio::AudioTrack {
            path: path.display().to_string(),
            ..drill_core::audio::AudioTrack::default()
        };
        if !self.simple_commit_song(Some(track), "simple-mode.340") {
            return false;
        }
        self.simple_mode.song_missing = false;
        self.audio_state.start_decode(path);
        if self.playing {
            self.align_simple_listening();
        }
        true
    }

    pub(crate) fn simple_remove_song(&mut self) {
        if self.document.audio.is_none() {
            return;
        }
        if self.simple_commit_song(None, "simple-mode.346") {
            self.simple_mode.song_missing = false;
            self.audio_state.clear();
            if self.playing {
                self.align_simple_listening();
            }
        }
    }

    pub(crate) fn simple_mark_downbeat(&mut self) {
        if self.simple_mode.song_missing {
            return;
        }
        let Some(heard) = self.simple_heard_song_seconds() else {
            return;
        };
        let Some(mut track) = self.document.audio.clone() else {
            return;
        };
        apply_simple_song_offset(&mut track, simple_downbeat_offset(heard));
        if !self.simple_commit_song(Some(track), "simple-mode.343") {
            return;
        }
        self.nav_glide.settle();
        self.seek_global(0.0);
        self.simple_mode.hold_count = false;
        self.simple_mode.finished_playback = false;
        if self.playing {
            self.align_simple_listening();
        } else {
            self.seek_audio_to_global(0.0);
        }
    }

    pub(crate) fn simple_nudge_song(&mut self, steps: i32) {
        if steps == 0 || self.simple_mode.song_missing {
            return;
        }
        let Some(mut track) = self.document.audio.clone() else {
            return;
        };
        let note = if steps < 0 {
            "simple-mode.344"
        } else {
            "simple-mode.345"
        };
        let offset = simple_nudged_offset(track.offset_seconds, steps);
        apply_simple_song_offset(&mut track, offset);
        if !self.simple_commit_song(Some(track), note) {
            return;
        }
        if self.playing {
            self.align_simple_listening();
        } else {
            let global = self
                .document
                .global_count(self.current_set, self.count_position);
            self.seek_audio_to_global(global);
        }
    }

    fn simple_commit_song(
        &mut self,
        track: Option<drill_core::audio::AudioTrack>,
        note: &'static str,
    ) -> bool {
        let revision = self.history.revision();
        let changed = self.execute_edit(
            Edit::SetAudioTrack { audio: track },
            i18n::registered(self.locale, "simple-mode.348"),
        );
        if changed {
            self.audio_draft = self.document.audio.clone();
            self.audio_draft_dirty = false;
            self.simple_mark(revision, note);
        }
        changed
    }

    fn simple_heard_song_seconds(&self) -> Option<f32> {
        let track = self.document.audio.as_ref()?;
        if let Some(clock) = self.audio_state.clock_seconds() {
            return Some(clock);
        }
        let global = self
            .document
            .global_count(self.current_set, self.count_position);
        Some(simple_song_seconds(track, &self.document.tempo, global))
    }

    /// True when simple-mode playback should follow the song's sample clock.
    /// Slow playback leaves the clock alone and steps the drill from the tempo
    /// map, so the picture can be half speed while the song stays quiet.
    pub(crate) fn simple_follows_song_clock(&self) -> bool {
        if !self.simple_mode.enabled {
            return true;
        }
        simple_song_audible(
            self.document.audio.is_some(),
            self.simple_mode.song_missing,
            self.simple_is_slow(),
        )
    }

    pub(crate) fn align_simple_listening(&mut self) {
        if !self.simple_mode.enabled {
            return;
        }
        let global = self
            .document
            .global_count(self.current_set, self.count_position);
        if self.simple_follows_song_clock() {
            self.simple_mode.metronome.stop();
            if let Some(track) = &self.document.audio {
                let seconds =
                    drill_core::audio::count_to_audio_time(track, &self.document.tempo, global);
                self.audio_state.seek_seconds(seconds);
                self.audio_state.set_mix(track.gain_linear(), track.muted);
                self.audio_state.play();
            }
            return;
        }
        self.audio_state.pause();
        let seconds = f64::from(self.document.tempo.seconds_at(global));
        let bpm = simple_opening_bpm(&self.document.tempo) as f32;
        self.simple_mode.metronome.start_at(bpm, seconds);
    }

    pub(crate) fn stop_simple_metronome(&mut self) {
        self.simple_mode.metronome.stop();
    }

    pub(crate) fn poll_simple_metronome(&mut self) {
        if !self.simple_mode.enabled || !self.playing {
            self.simple_mode.metronome.stop();
        }
        self.simple_mode.metronome.poll();
    }

    /// External path on the shared [`drill_core::audio::AudioTrack`]. A missing
    /// file sets a flag and leaves the show open. Embedded bytes are handled
    /// by the project loader before this runs.
    pub(crate) fn refresh_external_song(&mut self) {
        let Some(track) = self.document.audio.clone() else {
            self.simple_mode.song_missing = false;
            self.audio_state.clear();
            return;
        };
        if simple_stored_song_available(&track.path) {
            self.simple_mode.song_missing = false;
            self.audio_state
                .start_decode(std::path::PathBuf::from(&track.path));
        } else {
            self.simple_mode.song_missing = true;
            self.audio_state.clear();
        }
    }

    fn simple_paint_overlays(&mut self, ui: &mut egui::Ui) {
        let Some(guide) = self.simple_active_hint() else {
            return;
        };
        let ctx = ui.ctx().clone();
        let reduced = simple_reduced_motion(&ctx);
        let (t, pulse_only) = self.simple_hint_cycle();
        let show_hand = !reduced && !pulse_only;
        let show_arrow = reduced;
        let pulse = 0.55 + 0.45 * (t * std::f32::consts::PI).sin().abs();
        let action_place = self.simple_mode.action_place;
        let action_next = self.simple_mode.action_next;
        let action_play = self.simple_mode.action_play;
        let field_card = self.simple_mode.field_card;
        let _ = egui::Area::new(egui::Id::new("simple-overlay-hints"))
            .order(egui::Order::Tooltip)
            .fixed_pos(Pos2::ZERO)
            .interactable(false)
            .show(&ctx, |ui| {
                let painter = ui.painter();
                match guide {
                    SimpleGuide::Place => {
                        if let Some(btn) = action_place {
                            if show_hand {
                                let tap = (t / 0.45).clamp(0.0, 1.0);
                                let y = 28.0 * (1.0 - tap);
                                paint_ghost_finger(
                                    painter,
                                    btn.center() + egui::Vec2::new(6.0, y + 8.0),
                                    1.15,
                                    160,
                                );
                            } else if show_arrow {
                                painter.arrow(
                                    btn.center() + egui::Vec2::new(0.0, 36.0),
                                    egui::Vec2::new(0.0, -18.0),
                                    Stroke::new(2.5, SIMPLE_BLUE),
                                );
                            }
                        }
                        if show_hand
                            && t > 0.4
                            && let Some(card) = field_card
                        {
                            let grow = ((t - 0.4) / 0.6).clamp(0.0, 1.0);
                            painter.circle_filled(
                                card.center(),
                                5.0 + 9.0 * grow,
                                Color32::from_rgba_unmultiplied(
                                    20,
                                    96,
                                    200,
                                    (40.0 + 80.0 * grow) as u8,
                                ),
                            );
                        }
                    }
                    SimpleGuide::Move => {
                        if let Some(card) = field_card {
                            let start = card.center() + egui::Vec2::new(-28.0, 0.0);
                            if show_hand {
                                let end = start + egui::Vec2::new(40.0 * t, 0.0);
                                painter.circle_filled(
                                    end,
                                    8.0,
                                    Color32::from_rgba_unmultiplied(20, 96, 200, 140),
                                );
                                paint_ghost_finger(
                                    painter,
                                    end + egui::Vec2::new(6.0, 10.0),
                                    1.15,
                                    160,
                                );
                            } else if show_arrow {
                                painter.circle_filled(
                                    start,
                                    8.0,
                                    Color32::from_rgba_unmultiplied(20, 96, 200, 140),
                                );
                                painter.arrow(
                                    start,
                                    egui::Vec2::new(24.0, 0.0),
                                    Stroke::new(2.5, SIMPLE_BLUE),
                                );
                            } else {
                                painter.circle_filled(
                                    start,
                                    7.0 + 5.0 * pulse,
                                    Color32::from_rgba_unmultiplied(
                                        20,
                                        96,
                                        200,
                                        (80.0 + 80.0 * pulse) as u8,
                                    ),
                                );
                            }
                        }
                    }
                    SimpleGuide::NextSet => {
                        if let Some(rect) = action_next {
                            painter.rect_stroke(
                                rect.expand(4.0),
                                18.0,
                                Stroke::new(2.5, SIMPLE_BLUE.gamma_multiply(pulse)),
                                StrokeKind::Outside,
                            );
                            if show_hand {
                                paint_ghost_finger(
                                    painter,
                                    rect.center() + egui::Vec2::new(4.0, 8.0),
                                    1.0,
                                    150,
                                );
                            } else if show_arrow {
                                painter.arrow(
                                    rect.left_center() + egui::Vec2::new(-18.0, 0.0),
                                    egui::Vec2::new(16.0, 0.0),
                                    Stroke::new(2.5, SIMPLE_BLUE),
                                );
                            }
                        }
                    }
                    SimpleGuide::Play => {
                        if let Some(rect) = action_play {
                            painter.rect_stroke(
                                rect.expand(5.0),
                                18.0,
                                Stroke::new(3.0, SIMPLE_BLUE.gamma_multiply(pulse)),
                                StrokeKind::Outside,
                            );
                            if show_hand {
                                paint_ghost_finger(
                                    painter,
                                    rect.center() + egui::Vec2::new(6.0, 10.0),
                                    1.05,
                                    150,
                                );
                            } else if show_arrow {
                                painter.arrow(
                                    rect.left_center() + egui::Vec2::new(-18.0, 0.0),
                                    egui::Vec2::new(16.0, 0.0),
                                    Stroke::new(2.5, SIMPLE_BLUE),
                                );
                            }
                        }
                    }
                }
            });
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

    #[allow(dead_code)]
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

    #[allow(dead_code)]
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
        let previous_was_on = self.simple_mode.show_previous;
        self.begin_simple_show();
        if previous_was_on {
            self.set_show_previous(false);
        }
        self.simple_mode.overlap_note = false;
        self.simple_mode.finished_playback = false;
        self.simple_mode.hold_count = false;
        self.simple_mode.naming = false;
        self.simple_mode.naming_pending = false;
        self.simple_mode.person_name.clear();
        self.simple_mode.person_name_for = None;
        self.simple_mode.scene_name.clear();
        self.simple_mode.scene_name_for = None;
        self.simple_close_readouts();
        self.simple_mode.scene_loop = false;
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
        if self.simple_mode.show_previous {
            self.remember_previous_ghost_at(&path);
        }
        self.recent_projects.remember(path);
        true
    }

    /// Preference file for the previous-scene eye. One map of show path to
    /// "on". The drill file stays untouched, so undo and schema do not see it.
    fn previous_ghost_store_path(&self) -> std::path::PathBuf {
        if let Some(path) = &self.simple_mode.previous_store_override {
            return path.clone();
        }
        if let Some(draft) = &self.simple_mode.draft_path_override {
            let mut path = draft.clone();
            path.set_extension("previous.json");
            return path;
        }
        super::project_state::app_data_dir().join("simple-previous-set.json")
    }

    fn previous_ghost_key(&self) -> String {
        self.current_path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.simple_draft_path().to_string_lossy().into_owned())
    }

    fn previous_ghost_cache_fresh(&self) -> bool {
        let Some(cached) = self.simple_mode.previous_key.as_deref() else {
            return false;
        };
        if let Some(path) = &self.current_path {
            return !self.simple_mode.previous_key_is_draft && path == std::path::Path::new(cached);
        }
        self.simple_mode.previous_key_is_draft
            && self.simple_mode.previous_key_draft == self.simple_mode.draft_path_override
    }

    fn sync_previous_ghost_pref(&mut self) {
        if self.previous_ghost_cache_fresh() {
            return;
        }
        self.ensure_previous_ghost_store();
        let key = self.previous_ghost_key();
        self.simple_mode.show_previous = self
            .simple_mode
            .previous_shows
            .get(&key)
            .copied()
            .unwrap_or(false);
        self.remember_previous_ghost_cache(key);
    }

    fn remember_previous_ghost_cache(&mut self, key: String) {
        self.simple_mode.previous_key_is_draft = self.current_path.is_none();
        self.simple_mode.previous_key_draft = self.simple_mode.draft_path_override.clone();
        self.simple_mode.previous_key = Some(key);
    }

    fn ensure_previous_ghost_store(&mut self) {
        if self.simple_mode.previous_store_loaded {
            return;
        }
        self.simple_mode.previous_store_loaded = true;
        let Ok(text) = std::fs::read_to_string(self.previous_ghost_store_path()) else {
            return;
        };
        let Ok(store) = serde_json::from_str::<PreviousGhostStore>(&text) else {
            return;
        };
        self.simple_mode.previous_shows = store.shows.into_iter().filter(|(_, on)| *on).collect();
    }

    fn write_previous_ghost_store(&self) {
        let path = self.previous_ghost_store_path();
        if let Some(parent) = path.parent()
            && std::fs::create_dir_all(parent).is_err()
        {
            return;
        }
        let store = PreviousGhostStore {
            shows: self.simple_mode.previous_shows.clone(),
        };
        let Ok(json) = serde_json::to_string(&store) else {
            return;
        };
        let _ = std::fs::write(path, json);
    }

    fn set_show_previous(&mut self, on: bool) {
        self.ensure_previous_ghost_store();
        let key = self.previous_ghost_key();
        self.simple_mode.show_previous = on;
        if on {
            self.simple_mode.previous_shows.insert(key.clone(), true);
        } else {
            self.simple_mode.previous_shows.remove(&key);
        }
        self.remember_previous_ghost_cache(key);
        self.write_previous_ghost_store();
    }

    fn remember_previous_ghost_at(&mut self, path: &std::path::Path) {
        self.ensure_previous_ghost_store();
        self.simple_mode
            .previous_shows
            .insert(path.to_string_lossy().into_owned(), true);
        self.write_previous_ghost_store();
    }

    /// Naming a show moves the eye choice onto that file. The draft key is
    /// dropped so the next empty field does not inherit it. Saving with the
    /// eye off clears a choice that file may have kept from an older session.
    fn rekey_previous_ghost(&mut self, path: &std::path::Path) {
        self.ensure_previous_ghost_store();
        let new_key = path.to_string_lossy().into_owned();
        if self.simple_mode.previous_key.as_deref() == Some(new_key.as_str())
            && !self.simple_mode.previous_key_is_draft
        {
            return;
        }
        let mut changed = false;
        if let Some(old) = self.simple_mode.previous_key.clone()
            && old != new_key
            && self.simple_mode.previous_shows.remove(&old).is_some()
        {
            changed = true;
        }
        if self.simple_mode.show_previous {
            self.simple_mode
                .previous_shows
                .insert(new_key.clone(), true);
            changed = true;
        } else if self.simple_mode.previous_shows.remove(&new_key).is_some() {
            changed = true;
        }
        if changed {
            self.write_previous_ghost_store();
        }
        self.simple_mode.previous_key_is_draft = false;
        self.simple_mode.previous_key_draft = self.simple_mode.draft_path_override.clone();
        self.simple_mode.previous_key = Some(new_key);
    }

    fn simple_open_shows(&mut self) {
        if self.dirty && self.write_simple_draft() {
            self.dirty = false;
            self.simple_mode.draft_saved = true;
        }
        self.show_recent_projects = true;
    }

    #[allow(dead_code)]
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
        super::app_theme::modal_window(i18n::registered(self.locale, "simple-mode.119"), ctx)
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
        self.simple_flash_check();
        self.recent_projects.remember(path.to_path_buf());
        self.rekey_previous_ghost(path);
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

    /// True when this open is a crash copy. The show then has no file name
    /// yet, so the next save asks for one instead of writing into recovery.
    pub(super) fn prepare_simple_recovery(&mut self, path: &std::path::Path) -> bool {
        if !self.simple_mode.enabled || !path_is_session_autosave(path) {
            return false;
        }
        self.project_state
            .recoveries
            .retain(|candidate| candidate.autosave != path);
        self.simple_mode.memo_open = false;
        self.simple_mode.memo_copied = false;
        self.simple_mode.place_sheet_open = false;
        self.simple_mode.place_sheet_copied = false;
        self.simple_mode.show_sheet_open = false;
        self.simple_mode.show_sheet_copied = false;
        self.simple_mode.naming = false;
        self.simple_mode.naming_pending = false;
        self.simple_mode.finished_playback = false;
        self.simple_mode.hold_count = false;
        self.simple_mode.scene_loop = false;
        self.simple_mode.scene_name.clear();
        self.simple_mode.scene_name_for = None;
        self.simple_mode.person_name.clear();
        self.simple_mode.person_name_for = None;
        self.simple_mode.overlap_note = false;
        true
    }

    pub(crate) fn note_simple_playback_finished(&mut self) {
        if self.simple_mode.enabled {
            self.simple_mode.finished_playback = true;
        }
    }

    fn simple_start_play(&mut self, ctx: &egui::Context) {
        if self.simple_mode.scene_loop {
            self.simple_apply_watch_range();
            self.simple_mode.finished_playback = false;
        } else if self.simple_mode.finished_playback {
            self.simple_mode.finished_playback = false;
            self.jump_to_show_start();
        }
        self.toggle_playback(ctx);
    }

    fn simple_add_person(&mut self) {
        if !self.is_editable_set_start() {
            self.simple_fail();
            return;
        }
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
        self.simple_mark_hint(SimpleGuide::Place);
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
                self.simple_fail();
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
                if !self.is_editable_set_start() {
                    self.simple_fail();
                    return;
                }
                let revision = self.history.revision();
                self.place_performer_at(field_point, snap);
                self.simple_mark(revision, "simple-mode.055");
                self.simple_mark_hint(SimpleGuide::Place);
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
        let rect = response.rect;
        let mut block_pointer = false;
        if interactive {
            let over_overlay = self.drag_before.is_none()
                && ui
                    .input(|input| input.pointer.latest_pos())
                    .is_some_and(|pos| {
                        self.simple_mode
                            .overlay_block
                            .is_some_and(|block| block.contains(pos))
                    });
            if over_overlay {
                block_pointer = true;
            }
            let dt = ui.input(|input| input.stable_dt).clamp(1.0 / 240.0, 0.1);
            let (scroll, zoom_delta) =
                ui.input(|input| (input.smooth_scroll_delta.y, input.zoom_delta()));
            if !over_overlay
                && response.hovered()
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
            if !over_overlay && (middle || empty_pan) {
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
            block_pointer = block_pointer
                || middle
                || self.simple_mode.empty_pan
                || self.field_viewport.pan_last_pointer.is_some();
        }
        let render_options = self.field_render_options(true);
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
        egui_backend::paint_field_ground(&painter, rect);
        egui_backend::paint_below_dots(&painter, rect.min, &self.display_list);
        self.paint_previous_ghosts(&painter, &to_screen);
        egui_backend::paint_dots_and_above(&painter, rect.min, &self.display_list);
        self.paint_simple_paths(&painter, &to_screen);
        self.paint_simple_center_line(&painter, &to_screen);
        self.paint_simple_floor(&painter, rect, &to_screen);
        self.paint_simple_focus_wash(&painter, &to_screen);
        self.paint_simple_outside_rings(&painter, &to_screen);
        self.paint_simple_away(&painter, &to_screen);
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
            let faded = Color32::from_rgba_unmultiplied(20, 96, 200, 170);
            painter.circle_filled(
                center,
                46.0 * pop,
                Color32::from_rgba_unmultiplied(76, 163, 255, 36),
            );
            paint_phone_icon_at(&painter, center, PhoneIcon::PersonPlus, faded, 2.4 * pop);
            let hint = i18n::registered(self.locale, "simple-mode.082");
            let galley = painter.layout_no_wrap(
                hint.to_owned(),
                egui::FontId::proportional(20.0),
                SIMPLE_BLUE,
            );
            let pad = egui::vec2(14.0, 8.0);
            let plate = egui::Align2::CENTER_TOP
                .anchor_size(center + egui::vec2(0.0, 58.0 * pop), galley.size())
                .expand2(pad);
            painter.rect_filled(plate, 12.0, Color32::from_white_alpha(245));
            painter.rect_stroke(
                plate,
                12.0,
                Stroke::new(1.0, super::app_theme::HAIRLINE),
                egui::StrokeKind::Inside,
            );
            painter.galley(plate.min + pad, galley, SIMPLE_BLUE);
        }
        if interactive {
            self.field_pointer = response.hover_pos().map(from_screen);
            if block_pointer {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
            let hover_on_dot = !block_pointer
                && response.hover_pos().is_some_and(|pos| {
                    simple_hit_index(
                        pos,
                        &self.frame_positions,
                        &to_screen,
                        SIMPLE_DOT_HIT_RADIUS,
                        |index| self.is_selectable_index(index),
                    )
                    .is_some()
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
            for &point in &self.simple_mode.mirror_preview {
                let pos = to_screen(point);
                painter.circle_filled(pos, 8.0, Color32::from_rgba_unmultiplied(20, 96, 200, 70));
                painter.circle_stroke(
                    pos,
                    14.0,
                    Stroke::new(2.0, Color32::from_rgba_unmultiplied(20, 96, 200, 180)),
                );
            }
        }
        let select_ease = self
            .simple_motion_ease()
            .filter(|(kind, _, _)| matches!(kind, ChromeMotion::Select | ChromeMotion::Place))
            .map(|(_, ease, _)| ease)
            .unwrap_or(1.0);
        self.paint_simple_partner_rings(&painter, &to_screen);
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
            }
        }
        if let Some((kind, ease, Some(anchor))) = self.simple_motion_ease()
            && matches!(kind, ChromeMotion::Place | ChromeMotion::Move)
        {
            let pos = to_screen(anchor);
            if kind == ChromeMotion::Place {
                let pop = 0.45 + 0.55 * ease;
                painter.circle_filled(
                    pos,
                    12.0 * pop,
                    Color32::from_rgba_unmultiplied(20, 96, 200, ((1.0 - ease) * 140.0) as u8),
                );
            }
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
        let nearest = simple_hit_index(
            pointer,
            &self.frame_positions,
            &to_screen,
            SIMPLE_DOT_HIT_RADIUS,
            |index| self.is_selectable_index(index),
        );
        let shift_held = ui.input(|input| input.modifiers.shift);
        let snap_now = self.document.grid.snap_enabled && !shift_held;
        if let Some(pointer) = response.interact_pointer_pos() {
            if response.double_clicked() && nearest.is_none() {
                self.field_viewport.reset(&self.document.grid);
            } else if response.clicked() {
                let additive = ui.input(|input| {
                    input.modifiers.command || input.modifiers.ctrl || input.modifiers.shift
                });
                self.simple_click(from_screen(pointer), nearest, snap_now, additive);
            }
            if response.drag_started() {
                // Circle hits and box hits share the live positions only.
                // A previous-scene ghost inside the rectangle is not a dot
                // in `frame_positions`, so it cannot start a drag.
                let drag_target = nearest.filter(|&index| {
                    let dot = to_screen(self.frame_positions[index]);
                    let rect =
                        Rect::from_center_size(dot, Vec2::splat(SIMPLE_DOT_HIT_RADIUS * 2.0));
                    simple_box_hits(rect, &self.frame_positions, &to_screen, |candidate| {
                        self.is_selectable_index(candidate)
                    })
                    .contains(&index)
                });
                self.simple_start_drag(drag_target, pointer);
            }
            if response.dragged() && self.is_editable_set_start() && self.drag_before.is_some() {
                self.update_field_drag(pointer, field_map.scale, snap_now);
            }
            if response.drag_stopped() && self.drag_before.is_some() {
                let revision = self.history.revision();
                self.commit_field_drag();
                self.simple_mark(revision, "simple-mode.183");
                self.bump_simple_motion(ChromeMotion::Move);
                self.simple_mark_hint(SimpleGuide::Move);
            }
        }
    }

    #[cfg(test)]
    fn simple_previous_positions(&self) -> Option<&[Point]> {
        if !simple_previous_ghosts_visible(
            self.simple_mode.show_previous,
            self.playing,
            self.current_set,
        ) {
            return None;
        }
        self.document
            .sets
            .get(self.current_set - 1)
            .map(|set| set.positions.as_slice())
    }

    fn paint_previous_ghosts(
        &mut self,
        painter: &egui::Painter,
        to_screen: &impl Fn(Point) -> Pos2,
    ) {
        if !simple_previous_ghosts_visible(
            self.simple_mode.show_previous,
            self.playing,
            self.current_set,
        ) {
            return;
        }
        let Some(previous) = self
            .document
            .sets
            .get(self.current_set - 1)
            .map(|set| set.positions.as_slice())
        else {
            return;
        };
        let current = self
            .document
            .sets
            .get(self.current_set)
            .map(|set| set.positions.as_slice());
        let separated: Vec<Point> = previous
            .iter()
            .enumerate()
            .filter(|(index, previous)| {
                current
                    .and_then(|points| points.get(*index))
                    .is_none_or(|now| previous_ghost_separated(**previous, *now))
            })
            .map(|(_, point)| *point)
            .collect();
        let Some(mesh) =
            prepare_previous_ghost_mesh(&mut self.simple_mode.ghost_mesh, &separated, to_screen)
        else {
            return;
        };
        painter.add(mesh);
    }

    fn paint_simple_paths(&self, painter: &egui::Painter, to_screen: &impl Fn(Point) -> Pos2) {
        if self.drag_preview.is_some() {
            return;
        }
        let only = self.simple_focus_index();
        let step = self.document.grid.horizontal_units
            / f32::from(self.document.grid.horizontal_steps.max(1));
        for (index, start, end) in simple_indexed_paths(&self.document, self.current_set) {
            if only.is_some_and(|focus| focus != index) {
                continue;
            }
            let travel = (start.x - end.x).hypot(start.y - end.y);
            let selected = self.selected.contains(&index);
            if !movement_line_visible(travel, step, selected) {
                continue;
            }
            let ink = if selected {
                Color32::from_rgba_unmultiplied(20, 96, 200, 200)
            } else {
                Color32::from_rgba_unmultiplied(20, 96, 200, 110)
            };
            let from = to_screen(start);
            let to = to_screen(end);
            painter.line_segment(
                [from, to],
                Stroke::new(if selected { 2.0 } else { 1.4 }, ink),
            );
            if selected {
                painter.circle_stroke(to, 6.0, Stroke::new(1.5, ink));
            }
        }
    }

    fn paint_simple_floor(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        to_screen: &impl Fn(Point) -> Pos2,
    ) {
        let Some((point, label)) = simple_floor_mark(&self.document.grid, self.locale) else {
            return;
        };
        paint_simple_chip(painter, rect, to_screen(point), &label);
    }

    fn paint_simple_focus_wash(&self, painter: &egui::Painter, to_screen: &impl Fn(Point) -> Pos2) {
        let Some(focus) = self.simple_focus_index() else {
            return;
        };
        for (index, point) in self.frame_positions.iter().enumerate() {
            if index == focus {
                continue;
            }
            painter.circle_filled(to_screen(*point), 16.0, Color32::from_white_alpha(220));
        }
    }

    fn paint_simple_outside_rings(
        &self,
        painter: &egui::Painter,
        to_screen: &impl Fn(Point) -> Pos2,
    ) {
        let focus = self.simple_focus_index();
        let ink = Color32::from_rgb(176, 64, 48);
        for (index, point) in self.frame_positions.iter().enumerate() {
            if focus.is_some_and(|focus| focus != index) {
                continue;
            }
            if !point_outside_contest(&self.document.grid, *point) {
                continue;
            }
            painter.circle_stroke(to_screen(*point), 14.0, Stroke::new(2.5, ink));
        }
    }

    fn paint_simple_away(&self, painter: &egui::Painter, to_screen: &impl Fn(Point) -> Pos2) {
        if self.simple_mode.away.is_empty() {
            return;
        }
        let label = i18n::registered(self.locale, "simple-mode.289");
        let ink = Color32::from_rgb(90, 98, 112);
        for (index, point) in self.frame_positions.iter().enumerate() {
            if !self.simple_is_away(index) {
                continue;
            }
            let pos = to_screen(*point);
            painter.circle_filled(pos, 12.0, Color32::from_white_alpha(235));
            painter.circle_stroke(pos, 11.0, Stroke::new(2.0, ink));
            painter.text(
                pos,
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(11.0),
                ink,
            );
        }
    }

    fn paint_simple_center_line(
        &self,
        painter: &egui::Painter,
        to_screen: &impl Fn(Point) -> Pos2,
    ) {
        if !self.simple_mode.mirror_moves {
            return;
        }
        let axis = self.simple_mirror_axis();
        let top = to_screen(Point {
            x: axis,
            y: self.document.grid.height,
        });
        let bottom = to_screen(Point { x: axis, y: 0.0 });
        painter.line_segment(
            [top, bottom],
            Stroke::new(2.0, Color32::from_rgba_unmultiplied(20, 96, 200, 90)),
        );
    }

    fn paint_simple_partner_rings(
        &self,
        painter: &egui::Painter,
        to_screen: &impl Fn(Point) -> Pos2,
    ) {
        if !self.simple_mode.mirror_moves || self.drag_preview.is_some() {
            return;
        }
        let positions = if self.frame_positions.len() == self.document.performers.len() {
            self.frame_positions.clone()
        } else {
            let Some(positions) = self.simple_current_positions() else {
                return;
            };
            positions
        };
        let partners = drill_core::symmetry::pair_across_center(
            &positions,
            self.simple_mirror_axis(),
            self.simple_mirror_tolerance(),
        );
        let ink = Color32::from_rgba_unmultiplied(20, 96, 200, 170);
        for &index in &self.selected {
            let Some(partner) = partners.get(index).copied().flatten() else {
                continue;
            };
            if self.selected.contains(&partner) {
                continue;
            }
            let Some(point) = positions.get(partner) else {
                continue;
            };
            let pos = to_screen(*point);
            painter.circle_filled(pos, 16.0, Color32::from_rgba_unmultiplied(20, 96, 200, 28));
            painter.circle_stroke(pos, 14.0, Stroke::new(2.0, ink));
        }
    }

    /// Sits on top of the field so the picture underneath keeps its height.
    fn simple_symmetry_overlay(&mut self, ui: &mut egui::Ui) {
        let count = self.simple_unmirrored_count();
        let show_rule = self.simple_selection_has_partner();
        if count == 0 && !show_rule {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::new(8.0, 8.0);
            if count > 0 {
                ui.label(
                    egui::RichText::new(
                        i18n::registered(self.locale, "simple-mode.322")
                            .replace("{0}", &count.to_string()),
                    )
                    .size(14.0)
                    .strong()
                    .color(SIMPLE_ORANGE),
                );
                let label = i18n::registered(self.locale, "simple-mode.323");
                let button =
                    egui::Button::new(egui::RichText::new(label).size(15.0).color(SIMPLE_ORANGE))
                        .min_size(egui::Vec2::new(72.0, 48.0))
                        .fill(Color32::WHITE)
                        .stroke(Stroke::new(1.5, SIMPLE_ORANGE))
                        .corner_radius(18.0);
                if ui
                    .add_enabled(self.is_editable_set_start(), button)
                    .on_hover_text(i18n::registered(self.locale, "simple-mode.324"))
                    .clicked()
                {
                    self.simple_align_partners();
                }
            }
            if show_rule {
                ui.label(
                    egui::RichText::new(i18n::registered(self.locale, "simple-mode.321"))
                        .size(12.0)
                        .color(super::app_theme::SECONDARY_TEXT),
                );
            }
        });
    }
}

fn paint_simple_chip(painter: &egui::Painter, rect: egui::Rect, anchor: Pos2, label: &str) {
    let galley = painter.layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(14.0),
        SIMPLE_BLUE,
    );
    let pad = egui::Vec2::new(10.0, 5.0);
    let size = galley.size() + pad * 2.0;
    let min = simple_chip_origin(rect, anchor, size);
    let background = egui::Rect::from_min_size(min, size);
    if !rect.contains_rect(background) {
        return;
    }
    painter.rect_filled(background, 10.0, Color32::from_white_alpha(235));
    painter.galley(min + pad, galley, SIMPLE_BLUE);
}

/// Eighty people, seven instruments, six sets. Screenshot and round-trip
/// fixture only — not a file the app creates for a new show.
fn qa_cast_document() -> drill_core::Document {
    use drill_core::{PerformerKind, Point, Section, SectionId, Set, SetId, Symbol};
    let mut doc = drill_core::Document::blank(80);
    doc.title = "文化祭".into();
    let specs: [(&str, &str, [u8; 3], usize); 7] = [
        ("トランペット", "Tp", [196, 48, 40], 16),
        ("ホルン", "Hr", [196, 132, 24], 12),
        ("トロンボーン", "Tb", [32, 92, 196], 12),
        ("ユーフォニアム", "Eu", [24, 122, 72], 8),
        ("チューバ", "Tu", [96, 48, 168], 8),
        ("ドラム", "Dr", [48, 52, 60], 12),
        ("カラーガード", "CG", [196, 56, 132], 12),
    ];
    let mut sections = Vec::new();
    let mut assignment = Vec::with_capacity(80);
    for (index, (name, short, color, count)) in specs.iter().enumerate() {
        let id = SectionId::new((index + 1) as u32).expect("section id");
        sections.push(Section {
            id,
            name: (*name).into(),
            short: (*short).into(),
            color: *color,
            order: index as u16,
        });
        assignment.extend(std::iter::repeat_n(id, *count));
    }
    let names = [
        (0usize, "山田"),
        (15, "佐藤"),
        (27, "鈴木"),
        (40, "高橋"),
        (60, "伊藤"),
    ];
    for (index, performer) in doc.performers.iter_mut().enumerate() {
        performer.section = assignment[index];
        performer.symbol = Symbol::Cross;
        performer.color = None.into();
        performer.kind = if index >= 68 {
            PerformerKind::Guard
        } else if index >= 56 {
            PerformerKind::Percussion
        } else {
            PerformerKind::Wind
        };
        let number = index + 1;
        performer.label = names
            .iter()
            .find(|(at, _)| *at == index)
            .map(|(_, name)| format!("{number} {name}"))
            .unwrap_or_else(|| number.to_string());
    }
    doc.sections = sections;
    let grid = doc.grid.clone();
    let step = grid.horizontal_units / f32::from(grid.horizontal_steps.max(1));
    let gap = step * 4.0;
    let counts = [16u16, 12, 24, 8, 32, 16];
    doc.sets = counts
        .iter()
        .enumerate()
        .map(|(set_index, counts)| {
            let positions = (0..80)
                .map(|index| {
                    let col = index % 10;
                    let row = index / 10;
                    let shift = set_index as f32 * step * 2.0;
                    let fan = (set_index as f32 - 2.0) * step * (row as f32 - 3.5) * 0.35;
                    grid.snap(Point {
                        x: 8.0 + col as f32 * gap + shift,
                        y: 6.0 + row as f32 * gap + fan,
                    })
                })
                .collect();
            Set {
                id: SetId::new((set_index + 1) as u32).expect("set id"),
                name: format!("セット {}", set_index + 1),
                annotation: Default::default(),
                counts: *counts,
                hold: 0,
                routes: Default::default(),
                shape: None,
                generated_by: None,
                positions,
            }
        })
        .collect();
    doc
}

/// Front-sideline chip drawn above the dock. `ceiling` is the top of the
/// set strip and toolbar; the chip's bottom stays on or above it.
fn paint_front_chip(
    painter: &egui::Painter,
    field: egui::Rect,
    anchor: Pos2,
    ceiling: f32,
    label: &str,
) -> egui::Rect {
    let galley = painter.layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(16.0),
        SIMPLE_BLUE,
    );
    let pad = egui::Vec2::new(12.0, 6.0);
    let size = galley.size() + pad * 2.0;
    let min = front_chip_origin(field, anchor, size, ceiling);
    let background = egui::Rect::from_min_size(min, size);
    painter.rect_filled(background, 12.0, Color32::from_white_alpha(245));
    painter.rect_stroke(
        background,
        12.0,
        Stroke::new(1.0, super::app_theme::HAIRLINE),
        egui::StrokeKind::Inside,
    );
    painter.galley(min + pad, galley, SIMPLE_BLUE);
    background
}

fn front_chip_origin(field: egui::Rect, anchor: Pos2, size: egui::Vec2, ceiling: f32) -> Pos2 {
    let mut min = egui::pos2(anchor.x - size.x * 0.5, anchor.y - 22.0 - size.y);
    if min.y + size.y > ceiling {
        min.y = ceiling - size.y;
    }
    let max_x = (field.right() - size.x - 8.0).max(field.left() + 8.0);
    let max_y = (field.bottom() - size.y - 8.0).max(field.top() + 8.0);
    min.x = min.x.clamp(field.left() + 8.0, max_x);
    min.y = min.y.clamp(field.top() + 8.0, max_y);
    min
}

/// Puts the chip just above the anchor, then slides it into `rect` so the
/// dock and the window edge cannot cover 左 / 右.
fn simple_chip_origin(rect: egui::Rect, anchor: Pos2, size: egui::Vec2) -> Pos2 {
    let mut min = egui::pos2(anchor.x - size.x * 0.5, anchor.y - 18.0 - size.y);
    let max_x = (rect.right() - size.x).max(rect.left());
    let max_y = (rect.bottom() - size.y).max(rect.top());
    min.x = min.x.clamp(rect.left(), max_x);
    min.y = min.y.clamp(rect.top(), max_y);
    min
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
/// center, or the front itself. No "where they stand" prefix.
fn simple_place_detail(
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
    Some(detail)
}

fn simple_place_line(
    grid: &drill_core::GridConfig,
    point: Point,
    locale: drill_core::Locale,
) -> Option<String> {
    let detail = simple_place_detail(grid, point, locale)?;
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
    #[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
fn simple_move_paths(document: &drill_core::Document, set_index: usize) -> Vec<(Point, Point)> {
    simple_indexed_paths(document, set_index)
        .into_iter()
        .map(|(_, start, end)| (start, end))
        .collect()
}

fn simple_indexed_paths(
    document: &drill_core::Document,
    set_index: usize,
) -> Vec<(usize, Point, Point)> {
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
        .enumerate()
        .filter(|&(_, (from_point, to_point))| {
            (from_point.x - to_point.x).hypot(from_point.y - to_point.y) > 0.05
        })
        .map(|(index, (from_point, to_point))| (index, *from_point, *to_point))
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

fn simple_axis_step(grid: &drill_core::GridConfig) -> f32 {
    grid.horizontal_units / f32::from(grid.horizontal_steps.max(1))
}

/// Straight-line steps between two dots. The same step size as a walk.
fn simple_pair_steps(grid: &drill_core::GridConfig, left: Point, right: Point) -> Option<f32> {
    let step = simple_axis_step(grid);
    if step <= f32::EPSILON {
        return None;
    }
    Some(simple_round_quarter(
        (left.x - right.x).hypot(left.y - right.y) / step,
    ))
}

fn simple_gap_text(
    locale: drill_core::Locale,
    grid: &drill_core::GridConfig,
    left: Point,
    right: Point,
) -> Option<String> {
    let steps = simple_pair_steps(grid, left, right)?;
    Some(i18n::registered(locale, "simple-mode.240").replace("{0}", &format_step_count(steps)))
}

/// People who leave their spot on the move this scene shows.
fn simple_mover_indices(document: &drill_core::Document, set_index: usize) -> Vec<usize> {
    let Some((from, to)) = simple_transition_ends(document, set_index) else {
        return Vec::new();
    };
    let Some(start) = document.sets.get(from).map(|set| &set.positions) else {
        return Vec::new();
    };
    let Some(end) = document.sets.get(to).map(|set| &set.positions) else {
        return Vec::new();
    };
    start
        .iter()
        .zip(end.iter())
        .enumerate()
        .filter(|(_, (from_point, to_point))| {
            (from_point.x - to_point.x).hypot(from_point.y - to_point.y) > 0.05
        })
        .map(|(index, _)| index)
        .collect()
}

/// People who keep their spot on the move this scene shows. Empty when
/// nobody moves, so an opening picture does not look like a hold.
fn simple_holder_indices(document: &drill_core::Document, set_index: usize) -> Vec<usize> {
    let movers = simple_mover_indices(document, set_index);
    if movers.is_empty() {
        return Vec::new();
    }
    let Some((from, to)) = simple_transition_ends(document, set_index) else {
        return Vec::new();
    };
    let Some(end) = document.sets.get(to) else {
        return Vec::new();
    };
    if document
        .sets
        .get(from)
        .is_none_or(|set| set.positions.len() != end.positions.len())
    {
        return Vec::new();
    }
    (0..end.positions.len())
        .filter(|index| !movers.contains(index))
        .collect()
}

/// Sideways slide so the group's middle lands on the center line.
fn simple_center_points(grid: &drill_core::GridConfig, points: &[Point]) -> Vec<Point> {
    if points.len() < 2 {
        return points.to_vec();
    }
    let step = simple_axis_step(grid);
    if step <= f32::EPSILON {
        return points.to_vec();
    }
    let center = drill_core::editing::centroid(points);
    let dx = grid.width * 0.5 - center.x;
    // A snap can leave the middle a fraction of a step off the 50. That
    // still reads as the center, so a second press does not creep.
    if dx.abs() < step * 0.75 {
        return points.to_vec();
    }
    let shifted = points
        .iter()
        .map(|point| Point {
            x: point.x + dx,
            y: point.y,
        })
        .collect::<Vec<_>>();
    simple_fit_points(grid, &shifted)
}

/// Same left-right spots, pulled forward to the person nearest the audience.
/// People who already share a side-to-side spot are left where they are, so
/// a file does not collapse into one dot.
fn simple_dress_front_points(grid: &drill_core::GridConfig, points: &[Point]) -> Vec<Point> {
    if points.len() < 2 {
        return points.to_vec();
    }
    let step = simple_axis_step(grid);
    if step <= f32::EPSILON {
        return points.to_vec();
    }
    for (index, left) in points.iter().enumerate() {
        if points
            .iter()
            .skip(index + 1)
            .any(|right| (left.x - right.x).abs() < step * 0.75)
        {
            return points.to_vec();
        }
    }
    let front = points.iter().map(|point| point.y).fold(f32::MAX, f32::min);
    if points
        .iter()
        .all(|point| (point.y - front).abs() < step * 0.25)
    {
        return points.to_vec();
    }
    points
        .iter()
        .map(|point| Point {
            x: point.x,
            y: front,
        })
        .collect()
}

/// Even diagonal. Index 0 is the audience's front left; the last person is
/// toward the back right. Neighbors stay about two steps apart.
fn simple_diagonal_points(grid: &drill_core::GridConfig, points: &[Point]) -> Vec<Point> {
    let count = points.len();
    if count < 2 {
        return points.to_vec();
    }
    let step = simple_axis_step(grid);
    let gap = (step * 2.0).max(0.5);
    let mut axis = gap / std::f32::consts::SQRT_2;
    let along = axis * (count - 1) as f32;
    let span = along.hypot(along);
    let limit = grid.width.min(grid.height) * 0.72;
    if span > limit && span > 0.0 {
        axis *= limit / span;
    }
    let mid = drill_core::editing::centroid(points);
    let start_x = mid.x - axis * (count - 1) as f32 * 0.5;
    let start_y = mid.y - axis * (count - 1) as f32 * 0.5;
    let laid = (0..count)
        .map(|index| Point {
            x: start_x + axis * index as f32,
            y: start_y + axis * index as f32,
        })
        .collect::<Vec<_>>();
    simple_fit_points(grid, &laid)
}

/// Same left-right spots, pulled back to the person farthest from the audience.
/// People who already share a side-to-side spot are left where they are, so
/// a file does not collapse into one dot.
fn simple_dress_back_points(grid: &drill_core::GridConfig, points: &[Point]) -> Vec<Point> {
    if points.len() < 2 {
        return points.to_vec();
    }
    let step = simple_axis_step(grid);
    if step <= f32::EPSILON {
        return points.to_vec();
    }
    for (index, left) in points.iter().enumerate() {
        if points
            .iter()
            .skip(index + 1)
            .any(|right| (left.x - right.x).abs() < step * 0.75)
        {
            return points.to_vec();
        }
    }
    let back = points.iter().map(|point| point.y).fold(f32::MIN, f32::max);
    if points
        .iter()
        .all(|point| (point.y - back).abs() < step * 0.25)
    {
        return points.to_vec();
    }
    points
        .iter()
        .map(|point| Point {
            x: point.x,
            y: back,
        })
        .collect()
}

/// Equal gaps along the longer side. The two ends stay put, and each person's
/// other coordinate stays. Fewer than three points are already even.
fn simple_even_points(points: &[Point]) -> Vec<Point> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let (min_x, max_x, min_y, max_y) = simple_bounds(points);
    if (max_y - min_y) > (max_x - min_x) {
        drill_core::editing::distribute_vertical(points)
    } else {
        drill_core::editing::distribute_horizontal(points)
    }
}

/// A V around the group's own middle. Index 0 is the point, toward the
/// audience. Later people step back, audience-left first.
fn simple_chevron_points(grid: &drill_core::GridConfig, points: &[Point]) -> Vec<Point> {
    let count = points.len();
    if count < 2 {
        return points.to_vec();
    }
    let step = simple_axis_step(grid);
    if step <= f32::EPSILON {
        return points.to_vec();
    }
    let gap = (step * 2.0).max(0.5);
    let local = (0..count)
        .map(|index| {
            if index == 0 {
                Point { x: 0.0, y: 0.0 }
            } else {
                let rank = index.div_ceil(2) as f32;
                let side = if index % 2 == 1 { -1.0 } else { 1.0 };
                Point {
                    x: side * gap * rank,
                    y: gap * rank,
                }
            }
        })
        .collect::<Vec<_>>();
    let local_mid = drill_core::editing::centroid(&local);
    let mid = drill_core::editing::centroid(points);
    let laid = local
        .into_iter()
        .map(|point| Point {
            x: point.x - local_mid.x + mid.x,
            y: point.y - local_mid.y + mid.y,
        })
        .collect::<Vec<_>>();
    let fitted = simple_fit_points(grid, &laid);
    let snapped = fitted
        .into_iter()
        .map(|point| {
            let point = grid.snap(point);
            Point {
                x: point.x.clamp(0.0, grid.max_x()),
                y: point.y.clamp(0.0, grid.max_y()),
            }
        })
        .collect::<Vec<_>>();
    // Snap can leave the point a fraction of a step off the ideal. A second
    // press should not walk the V across the field.
    if snapped.len() == points.len()
        && snapped.iter().zip(points).all(|(next, prev)| {
            (next.x - prev.x).abs() < step * 1.25 && (next.y - prev.y).abs() < step * 1.25
        })
    {
        return points.to_vec();
    }
    snapped
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
#[cfg(test)]
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

fn finite_count_steps(count: f32) -> u32 {
    if count.is_finite() {
        count.round().max(0.0) as u32
    } else {
        0
    }
}

/// At a picture, the measure that is about to start. After a step, the beat
/// just reached. Four beats to a measure, counted from the top of the show.
fn simple_music_place(
    tempo: &drill_core::tempo::TempoMap,
    global_count: f32,
    local_count: f32,
) -> (u32, Option<u32>) {
    let local_steps = finite_count_steps(local_count);
    let global_steps = finite_count_steps(global_count);
    let anchor = if local_steps == 0 {
        global_steps
    } else {
        global_steps.saturating_sub(1)
    };
    let (measure, beat) = tempo.measure_beat(anchor as f32, SIMPLE_BEATS_PER_MEASURE);
    if local_steps == 0 {
        (measure, None)
    } else {
        let per = f32::from(SIMPLE_BEATS_PER_MEASURE.max(1));
        (measure, Some(beat.round().clamp(1.0, per) as u32))
    }
}

fn simple_music_text(
    locale: drill_core::Locale,
    tempo: &drill_core::tempo::TempoMap,
    global_count: f32,
    local_count: f32,
) -> String {
    let (measure, beat) = simple_music_place(tempo, global_count, local_count);
    match beat {
        Some(beat) => i18n::registered(locale, "simple-mode.223")
            .replace("{0}", &measure.to_string())
            .replace("{1}", &beat.to_string()),
        None => i18n::registered(locale, "simple-mode.224").replace("{0}", &measure.to_string()),
    }
}

fn simple_opening_bpm(tempo: &drill_core::tempo::TempoMap) -> i32 {
    let bpm = tempo.bpm_at(0.0).round();
    if !bpm.is_finite() {
        return 120;
    }
    bpm.clamp(1.0, 999.0) as i32
}

/// The move this scene is showing: the one that leaves it, or the one that
/// arrives when this is the last picture.
fn simple_watch_bounds(document: &drill_core::Document, set_index: usize) -> Option<(u32, u32)> {
    let (from, _) = simple_transition_ends(document, set_index)?;
    let set = document.sets.get(from)?;
    let start = finite_count_steps(document.global_count(from, 0.0));
    let span = u32::from(set.counts) + u32::from(set.hold);
    if span == 0 {
        return None;
    }
    let end = (start + span).min(document.timeline_counts());
    (end > start).then_some((start, end))
}

fn simple_transition_seconds(document: &drill_core::Document, set_index: usize) -> Option<u32> {
    let (start, end) = simple_watch_bounds(document, set_index)?;
    let seconds = document.tempo.seconds_at(end as f32) - document.tempo.seconds_at(start as f32);
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    let whole = if seconds < 0.5 {
        1
    } else {
        seconds.round() as u32
    };
    Some(whole)
}

/// Plain text of where everyone stands, to paste into a message. Nothing is written.
fn simple_chip_caption(name: &str, index: usize) -> String {
    let number = (index + 1).to_string();
    let custom = simple_scene_name_draft(name);
    if custom.is_empty() {
        number
    } else {
        let short: String = custom.chars().take(6).collect();
        format!("{number} {short}")
    }
}

fn simple_restock_scene_names(document: &mut drill_core::Document, locale: drill_core::Locale) {
    for (index, set) in document.sets.iter_mut().enumerate() {
        if simple_stock_scene_name(&set.name) {
            set.name = match locale {
                drill_core::Locale::Ja => format!("セット {}", index + 1),
                drill_core::Locale::En => format!("Set {}", index + 1),
            };
        }
    }
}

fn path_is_session_autosave(path: &std::path::Path) -> bool {
    let file = path
        .file_name()
        .is_some_and(|name| name == "autosave.drill.json");
    let folder = path
        .ancestors()
        .any(|dir| dir.file_name().is_some_and(|name| name == "recovery"));
    file && folder
}

fn simple_show_seconds(document: &drill_core::Document) -> Option<u32> {
    let total = document.timeline_counts();
    if total == 0 {
        return None;
    }
    let seconds = document.tempo.seconds_at(total as f32);
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    let whole = if seconds < 0.5 {
        1
    } else {
        seconds.round() as u32
    };
    Some(whole)
}

fn simple_show_length_text(locale: drill_core::Locale, seconds: u32) -> String {
    let minutes = seconds / 60;
    let rest = seconds % 60;
    if minutes == 0 {
        i18n::registered(locale, "simple-mode.265").replace("{0}", &seconds.to_string())
    } else if rest == 0 {
        i18n::registered(locale, "simple-mode.266").replace("{0}", &minutes.to_string())
    } else {
        i18n::registered(locale, "simple-mode.267")
            .replace("{0}", &minutes.to_string())
            .replace("{1}", &rest.to_string())
    }
}

fn simple_show_sheet_text(document: &drill_core::Document, locale: drill_core::Locale) -> String {
    let mut lines = vec![i18n::registered(locale, "simple-mode.276").to_string()];
    if let Some(seconds) = simple_show_seconds(document) {
        lines.push(simple_show_length_text(locale, seconds));
    }
    for index in 0..document.sets.len() {
        lines.push(String::new());
        lines.push(simple_position_sheet(document, index, locale));
    }
    lines.join("\n")
}

fn simple_position_sheet(
    document: &drill_core::Document,
    set_index: usize,
    locale: drill_core::Locale,
) -> String {
    let mut lines = vec![
        i18n::registered(locale, "simple-mode.229").replace("{0}", &(set_index + 1).to_string()),
    ];
    if let Some(set) = document.sets.get(set_index) {
        if !simple_stock_scene_name(&set.name) {
            lines.push(set.name.clone());
        }
        let global = document.global_count(set_index, 0.0);
        lines.push(simple_music_text(locale, &document.tempo, global, 0.0));
    }
    if let Some((from, _)) = simple_transition_ends(document, set_index)
        && let Some(seconds) = simple_transition_seconds(document, set_index)
        && let Some(set) = document.sets.get(from)
    {
        lines.push(
            i18n::registered(locale, "simple-mode.239")
                .replace("{0}", &set.counts.to_string())
                .replace("{1}", &seconds.to_string()),
        );
    }
    for index in simple_roster_order(document) {
        let label = document
            .performers
            .get(index)
            .map(|performer| performer.label.as_str())
            .unwrap_or("");
        let detail = document
            .sets
            .get(set_index)
            .and_then(|set| set.positions.get(index).copied())
            .and_then(|point| simple_place_detail(&document.grid, point, locale));
        if let Some(detail) = detail {
            lines.push(format!("{label}  {detail}"));
        } else if !label.is_empty() {
            lines.push(label.to_string());
        }
    }
    lines.join("\n")
}

fn simple_copy_button(locale: drill_core::Locale, copied: bool) -> egui::Button<'static> {
    let id = if copied {
        "simple-mode.180"
    } else {
        "simple-mode.179"
    };
    egui::Button::new(
        egui::RichText::new(i18n::registered(locale, id))
            .size(16.0)
            .color(Color32::WHITE),
    )
    .fill(SIMPLE_BLUE)
    .corner_radius(18.0)
}

/// One person's place at every picture, and the walk to the next picture.
fn simple_person_path_text(
    document: &drill_core::Document,
    index: usize,
    locale: drill_core::Locale,
) -> String {
    let label = document
        .performers
        .get(index)
        .map(|person| person.label.as_str())
        .unwrap_or("");
    let mut lines = vec![i18n::registered(locale, "simple-mode.283").replace("{0}", label)];
    for set_index in 0..document.sets.len() {
        lines.push(String::new());
        let mut heading = i18n::registered(locale, "simple-mode.286")
            .replace("{0}", &(set_index + 1).to_string());
        if let Some(set) = document.sets.get(set_index)
            && !simple_stock_scene_name(&set.name)
        {
            heading.push(' ');
            heading.push_str(&set.name);
        }
        lines.push(heading);
        if let Some(point) = document
            .sets
            .get(set_index)
            .and_then(|set| set.positions.get(index).copied())
            && let Some(detail) = simple_place_detail(&document.grid, point, locale)
        {
            lines.push(detail);
        }
        if set_index + 1 >= document.sets.len() {
            continue;
        }
        let Some(travel) = simple_person_travel(document, index, set_index, set_index + 1) else {
            continue;
        };
        if travel.kind == drill_core::continuity::TravelDirection::Hold {
            lines.push(i18n::registered(locale, "simple-mode.285").to_string());
        } else {
            lines.push(
                i18n::registered(locale, "simple-mode.284")
                    .replace("{0}", travel.kind.text(locale))
                    .replace("{1}", &format_step_count(travel.steps)),
            );
        }
    }
    lines.join("\n")
}

/// Plain text of every scene's walk, for a printed handout. `heading` replaces
/// the document title when the show already has a file name.
pub(crate) fn rehearsal_walk_sheet(
    document: &drill_core::Document,
    locale: drill_core::Locale,
    heading: Option<&str>,
) -> String {
    let mut lines = Vec::new();
    let heading = heading.map(str::trim).filter(|text| !text.is_empty());
    if let Some(heading) = heading {
        lines.push(heading.to_string());
    } else {
        let title = document.title.trim();
        if !title.is_empty() {
            lines.push(title.to_string());
        }
    }
    if let Some(seconds) = simple_show_seconds(document) {
        lines.push(simple_show_length_text(locale, seconds));
    }
    for index in 0..document.sets.len() {
        lines.push(String::new());
        lines.push(simple_scene_memo(document, index, locale));
    }
    lines.join("\n")
}

/// UTF-8 with a BOM so Windows Notepad opens the handout as Japanese.
pub(crate) fn rehearsal_walk_file(sheet: &str) -> String {
    format!("\u{FEFF}{sheet}")
}

/// The outer contest line, when the grid has one. People on the line count
/// as inside. A field with no contest line has nothing to flag.
pub(crate) fn outside_contest_indices(
    document: &drill_core::Document,
    set_index: usize,
) -> Vec<usize> {
    if document.grid.reference_frame_bounds().is_empty() {
        return Vec::new();
    }
    let mut indices = Vec::new();
    let mut consider = |index: usize| {
        let Some(positions) = document.sets.get(index).map(|set| &set.positions) else {
            return;
        };
        for (person, point) in positions.iter().enumerate() {
            if point_outside_contest(&document.grid, *point) && !indices.contains(&person) {
                indices.push(person);
            }
        }
    };
    consider(set_index);
    if let Some((from, to)) = simple_transition_ends(document, set_index) {
        consider(from);
        consider(to);
    }
    indices.sort_unstable();
    indices
}

fn point_outside_contest(grid: &drill_core::GridConfig, point: Point) -> bool {
    let Some((min_x, min_y, max_x, max_y)) = grid.reference_frame_bounds().into_iter().next()
    else {
        return false;
    };
    const EDGE: f32 = 0.05;
    point.x < min_x - EDGE
        || point.x > max_x + EDGE
        || point.y < min_y - EDGE
        || point.y > max_y + EDGE
}

/// Front edge of the outer contest line, labeled with its side length.
fn simple_floor_mark(
    grid: &drill_core::GridConfig,
    locale: drill_core::Locale,
) -> Option<(Point, String)> {
    let side = *grid.reference_frames.first()?;
    if !side.is_finite() || side < 1.0 {
        return None;
    }
    let (min_x, min_y, max_x, _) = grid.reference_frame_bounds().into_iter().next()?;
    let distance = if (side - side.round()).abs() < 0.05 {
        format!("{}", side.round() as i32)
    } else {
        format!("{side:.1}")
    };
    let id = match grid.unit {
        drill_core::Unit::Meters => "simple-mode.301",
        drill_core::Unit::Yards => "simple-mode.302",
    };
    Some((
        Point {
            x: (min_x + max_x) * 0.5,
            y: min_y,
        },
        i18n::registered(locale, id).replace("{0}", &distance),
    ))
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

/// Previous-scene ghosts are remembered per show path. Only `true` is stored.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
struct PreviousGhostStore {
    #[serde(default)]
    shows: std::collections::BTreeMap<String, bool>,
}

fn simple_previous_toggle_enabled(set_index: usize) -> bool {
    set_index > 0
}

fn simple_previous_ghosts_visible(enabled: bool, playing: bool, set_index: usize) -> bool {
    enabled && !playing && simple_previous_toggle_enabled(set_index)
}

fn simple_previous_toggle_width(label: &str) -> f32 {
    (36.0 + label.chars().count() as f32 * 12.0 + 16.0).max(PREVIOUS_TOGGLE_HEIGHT)
}

/// Closest live dot inside `radius`. Ghost positions are not in `live`.
fn simple_hit_index(
    pointer: Pos2,
    live: &[Point],
    to_screen: &impl Fn(Point) -> Pos2,
    radius: f32,
    selectable: impl Fn(usize) -> bool,
) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (index, point) in live.iter().enumerate() {
        if !selectable(index) {
            continue;
        }
        let distance = to_screen(*point).distance(pointer);
        let closer = match best {
            Some((_, best_distance)) => distance < best_distance,
            None => true,
        };
        if distance < radius && closer {
            best = Some((index, distance));
        }
    }
    best.map(|(index, _)| index)
}

/// Indices of live dots inside a rectangle. A ghost is selectable only if a
/// caller puts it in `live`, which the field painter never does.
fn simple_box_hits(
    rect: Rect,
    live: &[Point],
    to_screen: &impl Fn(Point) -> Pos2,
    selectable: impl Fn(usize) -> bool,
) -> Vec<usize> {
    let mut hits = Vec::new();
    for (index, point) in live.iter().enumerate() {
        if selectable(index) && rect.contains(to_screen(*point)) {
            hits.push(index);
        }
    }
    hits
}

fn paint_eye_icon(painter: &egui::Painter, center: Pos2, open: bool, color: Color32) {
    let stroke = Stroke::new(2.0, color);
    let p = |x: f32, y: f32| center + egui::Vec2::new(x, y);
    let upper = [
        p(-10.0, 0.0),
        p(-6.0, -4.5),
        p(0.0, -6.0),
        p(6.0, -4.5),
        p(10.0, 0.0),
    ];
    let lower = [
        p(-10.0, 0.0),
        p(-6.0, 4.5),
        p(0.0, 6.0),
        p(6.0, 4.5),
        p(10.0, 0.0),
    ];
    for pair in upper.windows(2) {
        painter.line_segment([pair[0], pair[1]], stroke);
    }
    for pair in lower.windows(2) {
        painter.line_segment([pair[0], pair[1]], stroke);
    }
    if open {
        painter.circle_filled(center, 2.4, color);
    } else {
        painter.line_segment([p(-8.0, 6.0), p(8.0, -6.0)], stroke);
    }
}

/// Fills one mesh for every previous-scene dot. `clear` keeps capacity, so a
/// second paint of the same cast does not allocate vertex or index storage.
fn fill_previous_ghost_mesh(
    mesh: &mut egui::Mesh,
    points: &[Point],
    to_screen: &impl Fn(Point) -> Pos2,
) {
    mesh.clear();
    let segments = PREVIOUS_GHOST_SEGMENTS;
    mesh.vertices
        .reserve(points.len().saturating_mul(segments + 1));
    mesh.indices
        .reserve(points.len().saturating_mul(segments * 3));
    let uv = egui::epaint::WHITE_UV;
    for &point in points {
        if !point.x.is_finite() || !point.y.is_finite() {
            continue;
        }
        let center = to_screen(point);
        if !center.x.is_finite() || !center.y.is_finite() {
            continue;
        }
        let base = u32::try_from(mesh.vertices.len()).unwrap_or(u32::MAX);
        mesh.vertices.push(egui::epaint::Vertex {
            pos: center,
            uv,
            color: previous_ghost_color(),
        });
        for step in 0..segments {
            let angle = (step as f32) * std::f32::consts::TAU / segments as f32;
            let (sin, cos) = angle.sin_cos();
            mesh.vertices.push(egui::epaint::Vertex {
                pos: center
                    + egui::Vec2::new(cos * PREVIOUS_GHOST_RADIUS, sin * PREVIOUS_GHOST_RADIUS),
                uv,
                color: previous_ghost_color(),
            });
        }
        for step in 0..segments {
            let next = (step + 1) % segments;
            mesh.indices
                .extend_from_slice(&[base, base + 1 + step as u32, base + 1 + next as u32]);
        }
    }
}

fn prepare_previous_ghost_mesh(
    slot: &mut Option<std::sync::Arc<egui::Mesh>>,
    points: &[Point],
    to_screen: &impl Fn(Point) -> Pos2,
) -> Option<std::sync::Arc<egui::Mesh>> {
    let reusable = slot
        .as_ref()
        .is_some_and(|arc| std::sync::Arc::strong_count(arc) == 1);
    if !reusable {
        *slot = Some(std::sync::Arc::new(egui::Mesh::default()));
    }
    let mesh = std::sync::Arc::get_mut(slot.as_mut().expect("ghost mesh slot"))
        .expect("ghost mesh is unique before it is submitted");
    fill_previous_ghost_mesh(mesh, points, to_screen);
    if mesh.vertices.is_empty() {
        None
    } else {
        Some(std::sync::Arc::clone(
            slot.as_ref().expect("ghost mesh slot"),
        ))
    }
}

fn write_qa_song_wav(path: &std::path::Path) {
    let samples = 2_000u32;
    let data_len = samples * 2;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(b"fmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&8_000u32.to_le_bytes());
    bytes.extend_from_slice(&16_000u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    bytes.extend(std::iter::repeat_n(0u8, data_len as usize));
    let _ = std::fs::write(path, bytes);
}

fn simple_song_extension_ok(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("mp3") || ext.eq_ignore_ascii_case("wav"))
}

fn simple_stored_song_available(path: &str) -> bool {
    let path = std::path::Path::new(path);
    simple_song_extension_ok(path) && path.is_file()
}

fn simple_song_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
        .to_string()
}

/// File position that lines up with scene 1, beat 1 (global count 0).
fn simple_downbeat_offset(file_seconds: f32) -> f32 {
    if file_seconds.is_finite() {
        file_seconds
    } else {
        0.0
    }
}

fn simple_nudged_offset(offset: f32, steps: i32) -> f32 {
    let base = if offset.is_finite() { offset } else { 0.0 };
    base + steps as f32 * SIMPLE_SONG_NUDGE_SECONDS
}

/// The song is heard only at normal speed, and only when the file is here.
/// Slow playback keeps the metronome and leaves the recording quiet.
fn simple_song_audible(has_track: bool, missing: bool, slow: bool) -> bool {
    has_track && !missing && !slow
}

fn apply_simple_song_offset(track: &mut drill_core::audio::AudioTrack, offset: f32) {
    let offset = if offset.is_finite() { offset } else { 0.0 };
    track.offset_seconds = offset;
    if offset >= 0.0
        && let Ok(anchors) = drill_core::audio::AnchorMap::from_offset(f64::from(offset))
    {
        track.anchors = anchors;
        return;
    }
    track.anchors = drill_core::audio::AnchorMap::default();
}

fn simple_song_seconds(
    track: &drill_core::audio::AudioTrack,
    tempo: &drill_core::tempo::TempoMap,
    global_count: f32,
) -> f32 {
    drill_core::audio::count_to_audio_time(track, tempo, global_count)
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

    #[test]
    fn tempo_steps_by_four_and_undoes_without_moving_anyone() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 12.0, y: 16.0 }, true);
        app.place_performer_at(Point { x: 20.0, y: 16.0 }, true);
        let positions = app.document.sets[0].positions.clone();
        let labels: Vec<_> = app
            .document
            .performers
            .iter()
            .map(|performer| performer.label.clone())
            .collect();
        let schema = app.document.schema_version;
        assert_eq!(simple_opening_bpm(&app.document.tempo), 120);
        app.simple_sync_history_notes();
        app.simple_step_tempo(SIMPLE_TEMPO_STEP);
        assert_eq!(simple_opening_bpm(&app.document.tempo), 124);
        assert_eq!(app.document.sets[0].positions, positions);
        assert_eq!(
            app.document
                .performers
                .iter()
                .map(|performer| performer.label.as_str())
                .collect::<Vec<_>>(),
            labels.iter().map(String::as_str).collect::<Vec<_>>()
        );
        assert_eq!(app.document.schema_version, schema);
        app.simple_sync_history_notes();
        let lines = app.simple_history_lines();
        assert!(
            lines.iter().any(|line| line.contains("テンポ")),
            "{lines:?}"
        );
        let json = app.document.to_json().expect("json");
        let loaded = drill_core::Document::from_json(&json).expect("reload");
        assert_eq!(simple_opening_bpm(&loaded.tempo), 124);
        assert_eq!(loaded.sets[0].positions, positions);
        assert_eq!(loaded.schema_version, schema);
        assert!(app.history.undo(&mut app.document));
        assert_eq!(simple_opening_bpm(&app.document.tempo), 120);
        assert_eq!(app.document.sets[0].positions, positions);

        for _ in 0..40 {
            app.simple_step_tempo(-SIMPLE_TEMPO_STEP);
        }
        assert_eq!(simple_opening_bpm(&app.document.tempo), SIMPLE_TEMPO_MIN);
        let revision = app.history.revision();
        app.simple_step_tempo(-SIMPLE_TEMPO_STEP);
        assert_eq!(app.history.revision(), revision);
        for _ in 0..50 {
            app.simple_step_tempo(SIMPLE_TEMPO_STEP);
        }
        assert_eq!(simple_opening_bpm(&app.document.tempo), SIMPLE_TEMPO_MAX);
        let revision = app.history.revision();
        app.simple_step_tempo(SIMPLE_TEMPO_STEP);
        assert_eq!(app.history.revision(), revision);

        let mut later = app.document.tempo.clone();
        later.set(16.0, 90.0);
        assert!(app.execute_edit(Edit::SetTempoMap { tempo: later }, "qa"));
        app.simple_step_tempo(-SIMPLE_TEMPO_STEP);
        assert_eq!(
            simple_opening_bpm(&app.document.tempo),
            SIMPLE_TEMPO_MAX - 4
        );
        assert!((app.document.tempo.bpm_at(16.0) - 90.0).abs() < 0.01);
        assert_eq!(app.document.sets[0].positions, positions);

        app.duplicate_current_set();
        let moved = app.document.sets[1].positions.clone();
        app.navigate_to_set(1);
        app.nav_glide.settle();
        let set_id = app.document.sets[1].id;
        let ids: Vec<_> = app.document.performers.iter().map(|p| p.id).collect();
        assert!(app.execute_edit(
            Edit::MovePerformers {
                set_id,
                performer_ids: ids,
                positions: vec![Point { x: 18.0, y: 16.0 }, Point { x: 28.0, y: 16.0 },],
            },
            "move",
        ));
        app.navigate_to_set(0);
        app.nav_glide.settle();
        app.simple_step_beat(1);
        app.nav_glide.settle();
        assert!(app.simple_holding_a_count());
        let bpm = simple_opening_bpm(&app.document.tempo);
        let revision = app.history.revision();
        app.simple_step_tempo(SIMPLE_TEMPO_STEP);
        assert_eq!(simple_opening_bpm(&app.document.tempo), bpm);
        assert_eq!(app.history.revision(), revision);
        let context = egui::Context::default();
        app.count_position = 0.0;
        app.simple_mode.hold_count = false;
        app.toggle_playback(&context);
        assert!(app.playing);
        app.simple_step_tempo(SIMPLE_TEMPO_STEP);
        assert_eq!(simple_opening_bpm(&app.document.tempo), bpm);
        app.toggle_playback(&context);
        assert_eq!(app.document.sets[0].positions, positions);
        assert_ne!(app.document.sets[1].positions, moved);
    }

    #[test]
    fn music_place_names_the_measure_and_the_scene_length() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 12.0, y: 16.0 }, true);
        assert_eq!(app.simple_music_caption(), "1小節から");
        assert!(simple_transition_seconds(&app.document, 0).is_none());
        app.duplicate_current_set();
        app.navigate_to_set(0);
        app.nav_glide.settle();
        assert_eq!(simple_transition_seconds(&app.document, 0), Some(8));
        assert_eq!(app.simple_scene_seconds_label().as_deref(), Some("約 8秒"));
        app.count_position = 5.0;
        assert_eq!(app.simple_music_caption(), "2小節 1拍");
        app.count_position = 4.0;
        assert_eq!(app.simple_music_caption(), "1小節 4拍");
        app.count_position = 0.0;
        app.navigate_to_set(1);
        app.nav_glide.settle();
        assert_eq!(app.simple_music_caption(), "5小節から");
        assert_eq!(simple_transition_seconds(&app.document, 1), Some(8));
        let mut tempo = app.document.tempo.clone();
        tempo.set(0.0, 60.0);
        assert!(app.execute_edit(Edit::SetTempoMap { tempo }, "qa"));
        assert_eq!(simple_transition_seconds(&app.document, 1), Some(16));
        let (measure, beat) = simple_music_place(&app.document.tempo, 16.0, 0.0);
        assert_eq!(measure, 5);
        assert_eq!(beat, None);
        let (measure, beat) = simple_music_place(&app.document.tempo, 5.0, 5.0);
        assert_eq!((measure, beat), (2, Some(1)));
        assert_eq!(app.document.sets[0].counts, 16);
    }

    #[test]
    fn position_sheet_lists_places_without_writing() {
        let mut app = empty_simple_app();
        let (step, _) = grid_step(&app);
        let mid_x = app.document.grid.width * 0.5;
        let on_center = app.document.grid.snap(Point {
            x: mid_x,
            y: step * 8.0,
        });
        app.place_performer_at(on_center, true);
        app.place_performer_at(
            app.document.grid.snap(Point {
                x: on_center.x + step * 4.0,
                y: on_center.y,
            }),
            true,
        );
        app.replace_selection(std::iter::once(0).collect());
        app.simple_rename_selected("山田");
        app.simple_rename_scene("サビ");
        let document = app.document.clone();
        let revision = app.history.revision();
        let sheet = simple_position_sheet(&app.document, 0, app.locale);
        assert!(sheet.starts_with("場面 1"), "{sheet}");
        assert!(sheet.contains("サビ"), "{sheet}");
        assert!(sheet.contains("1小節から"), "{sheet}");
        assert!(sheet.contains("山田"), "{sheet}");
        assert!(sheet.contains("中央"), "{sheet}");
        assert!(sheet.contains("右へ"), "{sheet}");
        assert!(sheet.contains("前から"), "{sheet}");
        assert!(!sheet.contains("いまの場所"), "{sheet}");
        assert_eq!(app.document, document);
        assert_eq!(app.history.revision(), revision);
        let json = app.document.to_json().expect("json");
        let loaded = drill_core::Document::from_json(&json).expect("reload");
        assert_eq!(loaded.sets[0].name, "サビ");
        assert_eq!(loaded.sets[0].positions, document.sets[0].positions);
        assert_eq!(loaded.schema_version, document.schema_version);
        app.duplicate_current_set();
        let with_move = simple_position_sheet(&app.document, 0, app.locale);
        assert!(with_move.contains("16拍"), "{with_move}");
        assert!(with_move.contains("約 8秒"), "{with_move}");
    }

    #[test]
    fn scene_loop_watches_one_move_and_restores_the_whole_show() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 12.0, y: 16.0 }, true);
        app.place_performer_at(Point { x: 24.0, y: 16.0 }, true);
        app.duplicate_current_set();
        app.duplicate_current_set();
        app.navigate_to_set(1);
        app.nav_glide.settle();
        let set_id = app.document.sets[1].id;
        let ids: Vec<_> = app.document.performers.iter().map(|p| p.id).collect();
        let start = app.document.sets[1].positions.clone();
        assert!(app.execute_edit(
            Edit::MovePerformers {
                set_id,
                performer_ids: ids,
                positions: vec![
                    Point {
                        x: start[0].x + 4.0,
                        y: start[0].y,
                    },
                    Point {
                        x: start[1].x + 4.0,
                        y: start[1].y,
                    },
                ],
            },
            "move",
        ));
        app.navigate_to_set(0);
        app.nav_glide.settle();
        let document = app.document.clone();
        let revision = app.history.revision();
        assert_eq!(app.document.timeline_counts(), 32);
        app.simple_toggle_scene_loop();
        assert!(app.simple_mode.scene_loop);
        assert!(app.loop_playback);
        assert_eq!((app.playback_start, app.playback_end), (0, 16));
        app.navigate_to_set(1);
        app.simple_apply_watch_range();
        assert_eq!((app.playback_start, app.playback_end), (16, 32));
        app.navigate_to_set(2);
        app.simple_apply_watch_range();
        assert_eq!((app.playback_start, app.playback_end), (16, 32));
        app.simple_toggle_scene_loop();
        assert!(!app.simple_mode.scene_loop);
        assert!(!app.loop_playback);
        assert_eq!((app.playback_start, app.playback_end), (0, 32));
        assert_eq!(app.document, document);
        assert_eq!(app.history.revision(), revision);
        app.navigate_to_set(0);
        app.nav_glide.settle();
        assert!(app.commit_transition_counts(0, 8));
        app.simple_toggle_scene_loop();
        assert_eq!((app.playback_start, app.playback_end), (0, 8));
        assert!(app.loop_playback);
        app.set_simple_mode(false);
        assert!(!app.simple_mode.scene_loop);
        assert!(!app.loop_playback);
        assert_eq!(app.playback_start, 0);
        assert_eq!(app.playback_end, app.document.timeline_counts());
    }

    #[test]
    fn next_person_walks_left_to_right_without_moving() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 30.0, y: 20.0 }, true);
        app.place_performer_at(Point { x: 10.0, y: 18.0 }, true);
        app.place_performer_at(Point { x: 20.0, y: 24.0 }, true);
        let positions = app.document.sets[0].positions.clone();
        let revision = app.history.revision();
        app.replace_selection(BTreeSet::new());
        app.simple_cycle_person(true);
        assert_eq!(app.selected, std::iter::once(1).collect());
        app.simple_cycle_person(true);
        assert_eq!(app.selected, std::iter::once(2).collect());
        app.simple_cycle_person(true);
        assert_eq!(app.selected, std::iter::once(0).collect());
        app.simple_cycle_person(true);
        assert_eq!(app.selected, std::iter::once(1).collect());
        app.simple_cycle_person(false);
        assert_eq!(app.selected, std::iter::once(0).collect());
        app.replace_selection([0_usize, 2].into_iter().collect());
        app.simple_cycle_person(false);
        assert_eq!(app.selected, std::iter::once(0).collect());
        assert_eq!(app.document.sets[0].positions, positions);
        assert_eq!(app.history.revision(), revision);
        app.duplicate_current_set();
        app.navigate_to_set(1);
        app.nav_glide.settle();
        let set_id = app.document.sets[1].id;
        let ids: Vec<_> = app.document.performers.iter().map(|p| p.id).collect();
        let start = app.document.sets[1].positions.clone();
        assert!(
            app.execute_edit(
                Edit::MovePerformers {
                    set_id,
                    performer_ids: ids,
                    positions: start
                        .iter()
                        .map(|point| Point {
                            x: point.x + 2.0,
                            y: point.y,
                        })
                        .collect(),
                },
                "move",
            )
        );
        let arrived = app.document.sets[1].positions.clone();
        app.navigate_to_set(0);
        app.nav_glide.settle();
        app.simple_step_beat(3);
        app.nav_glide.settle();
        assert!(app.simple_holding_a_count());
        let revision = app.history.revision();
        app.simple_cycle_person(true);
        assert_eq!(app.selected.len(), 1);
        assert_eq!(app.document.sets[1].positions, arrived);
        assert_eq!(app.history.revision(), revision);
    }

    #[test]
    fn two_people_show_the_steps_between_them_without_writing() {
        let mut app = empty_simple_app();
        let (step, _) = grid_step(&app);
        let origin = app.document.grid.snap(Point { x: 12.0, y: 16.0 });
        app.place_performer_at(origin, true);
        app.place_performer_at(
            Point {
                x: origin.x + step * 4.0,
                y: origin.y,
            },
            true,
        );
        app.place_performer_at(
            Point {
                x: origin.x,
                y: origin.y + step * 6.0,
            },
            true,
        );
        let document = app.document.clone();
        let revision = app.history.revision();
        app.replace_selection([0_usize, 1].into_iter().collect());
        let gap = app.simple_gap_caption().expect("gap");
        assert!(gap.contains('4'), "{gap}");
        assert!(gap.contains("あいだ"), "{gap}");
        app.replace_selection([0_usize, 2].into_iter().collect());
        let deep = app.simple_gap_caption().expect("vertical gap");
        assert!(deep.contains('6'), "{deep}");
        assert_eq!(app.document, document);
        assert_eq!(app.history.revision(), revision);

        app.duplicate_current_set();
        let set_id = app.document.sets[1].id;
        let mover = app.document.performers[1].id;
        let from = app.document.sets[1].positions[1];
        assert!(app.execute_edit(
            Edit::MovePerformers {
                set_id,
                performer_ids: vec![mover],
                positions: vec![Point {
                    x: from.x + step * 8.0,
                    y: from.y,
                }],
            },
            "apart",
        ));
        app.navigate_to_set(0);
        app.nav_glide.settle();
        app.replace_selection([0_usize, 1].into_iter().collect());
        app.simple_step_beat(8);
        assert!(app.simple_holding_a_count());
        let mid = app.simple_gap_caption().expect("mid gap");
        assert!(mid.contains('8'), "{mid}");
        let parked = app.document.clone();
        let parked_revision = app.history.revision();
        assert!(app.simple_gap_caption().is_some());
        assert_eq!(app.document, parked);
        assert_eq!(app.history.revision(), parked_revision);
    }

    #[test]
    fn choosing_who_moves_leaves_the_drill_unchanged() {
        let mut app = empty_simple_app();
        let (step, _) = grid_step(&app);
        app.place_performer_at(Point { x: 12.0, y: 16.0 }, true);
        app.place_performer_at(Point { x: 20.0, y: 16.0 }, true);
        app.place_performer_at(Point { x: 28.0, y: 16.0 }, true);
        app.duplicate_current_set();
        let set_id = app.document.sets[1].id;
        let mover = app.document.performers[0].id;
        let from = app.document.sets[1].positions[0];
        assert!(app.execute_edit(
            Edit::MovePerformers {
                set_id,
                performer_ids: vec![mover],
                positions: vec![Point {
                    x: from.x + step * 4.0,
                    y: from.y,
                }],
            },
            "one moves",
        ));
        app.navigate_to_set(1);
        app.nav_glide.settle();
        let document = app.document.clone();
        let revision = app.history.revision();
        assert_eq!(simple_mover_indices(&app.document, 1), vec![0]);
        app.replace_selection([1_usize, 2].into_iter().collect());
        app.simple_select_movers();
        assert_eq!(app.selected, std::iter::once(0).collect());
        assert_eq!(app.document, document);
        assert_eq!(app.history.revision(), revision);
        let json = app.document.to_json().expect("json");
        let loaded = drill_core::Document::from_json(&json).expect("reload");
        assert_eq!(loaded.sets[1].positions, document.sets[1].positions);
        assert_eq!(loaded.schema_version, document.schema_version);

        app.playing = true;
        app.replace_selection(std::iter::once(2).collect());
        app.simple_select_movers();
        assert_eq!(app.selected, std::iter::once(2).collect());
        app.playing = false;
        app.navigate_to_set(0);
        app.nav_glide.settle();
        assert_eq!(simple_mover_indices(&app.document, 0), vec![0]);
    }

    #[test]
    fn centering_slides_sideways_and_undoes_in_one_step() {
        let mut app = empty_simple_app();
        let (step, _) = grid_step(&app);
        app.place_performer_at(Point { x: 6.0, y: 12.0 }, true);
        app.place_performer_at(
            Point {
                x: 6.0 + step * 4.0,
                y: 18.0,
            },
            true,
        );
        app.place_performer_at(
            Point {
                x: 6.0 + step * 8.0,
                y: 14.0,
            },
            true,
        );
        let before = app.document.sets[0].positions.clone();
        let labels: Vec<_> = app
            .document
            .performers
            .iter()
            .map(|performer| performer.label.clone())
            .collect();
        let schema = app.document.schema_version;
        let spread_before = span(&before);
        app.simple_sync_history_notes();
        app.simple_center_up();
        let after = app.document.sets[0].positions.clone();
        let middle = drill_core::editing::centroid(&after);
        assert!((middle.x - app.document.grid.width * 0.5).abs() < step);
        assert!((span(&after).1 - spread_before.1).abs() < step);
        assert!(after[0].x < after[2].x);
        assert!(on_field(&app, &after));
        app.simple_sync_history_notes();
        let lines = app.simple_history_lines();
        assert!(
            lines.iter().any(|line| line.contains("中央へ")),
            "{lines:?}"
        );
        let json = app.document.to_json().expect("json");
        let loaded = drill_core::Document::from_json(&json).expect("reload");
        assert_eq!(loaded.sets[0].positions, after);
        assert_eq!(loaded.schema_version, schema);
        assert_eq!(
            loaded
                .performers
                .iter()
                .map(|performer| performer.label.as_str())
                .collect::<Vec<_>>(),
            labels.iter().map(String::as_str).collect::<Vec<_>>()
        );
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.sets[0].positions, before);
        app.simple_center_up();
        let settled = app.document.sets[0].positions.clone();
        let revision = app.history.revision();
        app.simple_center_up();
        assert_eq!(app.document.sets[0].positions, settled);
        assert_eq!(app.history.revision(), revision);

        let parked = settled[2];
        app.replace_selection([0_usize, 1].into_iter().collect());
        app.simple_center_up();
        assert_eq!(app.document.sets[0].positions[2], parked);

        app.playing = true;
        let locked = app.document.sets[0].positions.clone();
        let revision = app.history.revision();
        app.simple_center_up();
        assert_eq!(app.document.sets[0].positions, locked);
        assert_eq!(app.history.revision(), revision);
    }

    #[test]
    fn dressing_the_front_keeps_side_to_side_spots() {
        let mut app = empty_simple_app();
        let (step, _) = grid_step(&app);
        let front = app.document.grid.snap(Point { x: 10.0, y: 8.0 });
        app.place_performer_at(front, true);
        app.place_performer_at(
            Point {
                x: front.x + step * 4.0,
                y: front.y + step * 6.0,
            },
            true,
        );
        app.place_performer_at(
            Point {
                x: front.x + step * 8.0,
                y: front.y + step * 3.0,
            },
            true,
        );
        let before = app.document.sets[0].positions.clone();
        let schema = app.document.schema_version;
        app.simple_sync_history_notes();
        app.simple_dress_front();
        let after = app.document.sets[0].positions.clone();
        assert!((after[0].y - after[1].y).abs() < 0.05);
        assert!((after[0].y - after[2].y).abs() < 0.05);
        assert!((after[0].y - before[0].y).abs() < 0.05);
        assert!((after[1].x - before[1].x).abs() < 0.05);
        assert!((after[2].x - before[2].x).abs() < 0.05);
        assert!(on_field(&app, &after));
        app.simple_sync_history_notes();
        let lines = app.simple_history_lines();
        assert!(
            lines.iter().any(|line| line.contains("前をそろえる")),
            "{lines:?}"
        );
        let json = app.document.to_json().expect("json");
        let loaded = drill_core::Document::from_json(&json).expect("reload");
        assert_eq!(loaded.sets[0].positions, after);
        assert_eq!(loaded.schema_version, schema);
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.sets[0].positions, before);

        let mut file = empty_simple_app();
        let spine = file.document.grid.snap(Point { x: 16.0, y: 10.0 });
        file.place_performer_at(spine, true);
        file.place_performer_at(
            Point {
                x: spine.x,
                y: spine.y + step * 4.0,
            },
            true,
        );
        file.place_performer_at(
            Point {
                x: spine.x,
                y: spine.y + step * 8.0,
            },
            true,
        );
        let filed = file.document.sets[0].positions.clone();
        let revision = file.history.revision();
        file.simple_dress_front();
        assert_eq!(file.document.sets[0].positions, filed);
        assert_eq!(file.history.revision(), revision);

        app.duplicate_current_set();
        app.navigate_to_set(0);
        app.nav_glide.settle();
        let set_id = app.document.sets[1].id;
        let person = app.document.performers[0].id;
        let moved = app.document.sets[1].positions[0];
        assert!(app.execute_edit(
            Edit::MovePerformers {
                set_id,
                performer_ids: vec![person],
                positions: vec![Point {
                    x: moved.x + step * 2.0,
                    y: moved.y,
                }],
            },
            "so we can step",
        ));
        app.simple_step_beat(4);
        assert!(app.simple_holding_a_count());
        let held = app.document.sets.clone();
        let revision = app.history.revision();
        app.simple_dress_front();
        assert_eq!(app.document.sets, held);
        assert_eq!(app.history.revision(), revision);
    }

    #[test]
    fn a_diagonal_runs_from_the_front_left_and_undoes() {
        let mut app = empty_simple_app();
        let (step, _) = grid_step(&app);
        app.place_performer_at(Point { x: 8.0, y: 28.0 }, true);
        app.place_performer_at(Point { x: 30.0, y: 8.0 }, true);
        app.place_performer_at(Point { x: 18.0, y: 22.0 }, true);
        app.place_performer_at(Point { x: 36.0, y: 14.0 }, true);
        let before = app.document.sets[0].positions.clone();
        let schema = app.document.schema_version;
        app.simple_sync_history_notes();
        app.replace_selection([0_usize, 1, 2].into_iter().collect());
        let parked = before[3];
        app.simple_diagonal_up();
        assert_eq!(app.document.sets[0].positions[3], parked);
        let laid = &app.document.sets[0].positions[..3];
        assert!(laid[0].x + 0.2 < laid[1].x && laid[1].x + 0.2 < laid[2].x);
        assert!(laid[0].y + 0.2 < laid[1].y && laid[1].y + 0.2 < laid[2].y);
        for pair in laid.windows(2) {
            let apart = (pair[0].x - pair[1].x).hypot(pair[0].y - pair[1].y);
            assert!(apart > step * 0.5 && apart < step * 4.0, "{apart}");
        }
        assert!(on_field(&app, &app.document.sets[0].positions));
        app.simple_sync_history_notes();
        let lines = app.simple_history_lines();
        assert!(lines.iter().any(|line| line.contains("斜め")), "{lines:?}");
        let json = app.document.to_json().expect("json");
        let loaded = drill_core::Document::from_json(&json).expect("reload");
        assert_eq!(loaded.sets[0].positions, app.document.sets[0].positions);
        assert_eq!(loaded.schema_version, schema);
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.sets[0].positions, before);

        app.playing = true;
        let revision = app.history.revision();
        app.simple_diagonal_up();
        assert_eq!(app.document.sets[0].positions, before);
        assert_eq!(app.history.revision(), revision);
    }

    #[test]
    fn dressing_the_back_keeps_side_to_side_spots() {
        let mut app = empty_simple_app();
        let (step, _) = grid_step(&app);
        let front = app.document.grid.snap(Point { x: 10.0, y: 8.0 });
        app.place_performer_at(front, true);
        app.place_performer_at(
            Point {
                x: front.x + step * 4.0,
                y: front.y + step * 6.0,
            },
            true,
        );
        app.place_performer_at(
            Point {
                x: front.x + step * 8.0,
                y: front.y + step * 3.0,
            },
            true,
        );
        let before = app.document.sets[0].positions.clone();
        let schema = app.document.schema_version;
        let back = before.iter().map(|point| point.y).fold(f32::MIN, f32::max);
        app.simple_sync_history_notes();
        app.simple_dress_back();
        let after = app.document.sets[0].positions.clone();
        assert!(after.iter().all(|point| (point.y - back).abs() < 0.05));
        for (from, to) in before.iter().zip(&after) {
            assert!((to.x - from.x).abs() < 0.05);
        }
        assert!(on_field(&app, &after));
        app.simple_sync_history_notes();
        let lines = app.simple_history_lines();
        assert!(
            lines.iter().any(|line| line.contains("後ろをそろえる")),
            "{lines:?}"
        );
        let json = app.document.to_json().expect("json");
        let loaded = drill_core::Document::from_json(&json).expect("reload");
        assert_eq!(loaded.sets[0].positions, after);
        assert_eq!(loaded.schema_version, schema);
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.sets[0].positions, before);
        let settled = after.clone();
        app.simple_dress_back();
        let revision = app.history.revision();
        app.simple_dress_back();
        assert_eq!(app.document.sets[0].positions, settled);
        assert_eq!(app.history.revision(), revision);

        let mut file = empty_simple_app();
        let spine = file.document.grid.snap(Point { x: 16.0, y: 10.0 });
        file.place_performer_at(spine, true);
        file.place_performer_at(
            Point {
                x: spine.x,
                y: spine.y + step * 4.0,
            },
            true,
        );
        file.place_performer_at(
            Point {
                x: spine.x,
                y: spine.y + step * 8.0,
            },
            true,
        );
        let filed = file.document.sets[0].positions.clone();
        let revision = file.history.revision();
        file.simple_dress_back();
        assert_eq!(file.document.sets[0].positions, filed);
        assert_eq!(file.history.revision(), revision);

        app.duplicate_current_set();
        app.navigate_to_set(0);
        app.nav_glide.settle();
        let set_id = app.document.sets[1].id;
        let person = app.document.performers[0].id;
        let moved = app.document.sets[1].positions[0];
        assert!(app.execute_edit(
            Edit::MovePerformers {
                set_id,
                performer_ids: vec![person],
                positions: vec![Point {
                    x: moved.x + step * 2.0,
                    y: moved.y,
                }],
            },
            "so we can step",
        ));
        app.simple_step_beat(4);
        assert!(app.simple_holding_a_count());
        let held = app.document.sets.clone();
        let revision = app.history.revision();
        app.simple_dress_back();
        assert_eq!(app.document.sets, held);
        assert_eq!(app.history.revision(), revision);
    }

    #[test]
    fn evening_gaps_keeps_the_ends_and_undoes() {
        let mut app = empty_simple_app();
        let (step, _) = grid_step(&app);
        let origin = app.document.grid.snap(Point { x: 12.0, y: 20.0 });
        for steps in [0.0, 2.0, 10.0, 12.0] {
            app.place_performer_at(
                Point {
                    x: origin.x + step * steps,
                    y: origin.y,
                },
                true,
            );
        }
        let before = app.document.sets[0].positions.clone();
        let schema = app.document.schema_version;
        app.simple_sync_history_notes();
        app.simple_even_gaps();
        let after = app.document.sets[0].positions.clone();
        assert!((after[0].x - before[0].x).abs() < 0.05);
        assert!((after[3].x - before[3].x).abs() < 0.05);
        assert!((after[1].x - (before[0].x + step * 4.0)).abs() < 0.05);
        assert!((after[2].x - (before[0].x + step * 8.0)).abs() < 0.05);
        for (from, to) in before.iter().zip(&after) {
            assert!((to.y - from.y).abs() < 0.05);
        }
        assert!(on_field(&app, &after));
        app.simple_sync_history_notes();
        let lines = app.simple_history_lines();
        assert!(
            lines.iter().any(|line| line.contains("間隔をそろえる")),
            "{lines:?}"
        );
        let json = app.document.to_json().expect("json");
        let loaded = drill_core::Document::from_json(&json).expect("reload");
        assert_eq!(loaded.sets[0].positions, after);
        assert_eq!(loaded.schema_version, schema);
        let revision = app.history.revision();
        app.simple_even_gaps();
        assert_eq!(app.document.sets[0].positions, after);
        assert_eq!(app.history.revision(), revision);
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.sets[0].positions, before);

        let parked = before[3];
        app.replace_selection([0_usize, 1, 2].into_iter().collect());
        app.simple_even_gaps();
        assert_eq!(app.document.sets[0].positions[3], parked);
        assert!((app.document.sets[0].positions[1].x - (before[0].x + step * 5.0)).abs() < 0.05);

        let mut column = empty_simple_app();
        let spine = column.document.grid.snap(Point { x: 18.0, y: 8.0 });
        for steps in [0.0, 2.0, 10.0, 12.0] {
            column.place_performer_at(
                Point {
                    x: spine.x,
                    y: spine.y + step * steps,
                },
                true,
            );
        }
        let stacked = column.document.sets[0].positions.clone();
        column.simple_even_gaps();
        let opened = &column.document.sets[0].positions;
        assert!((opened[0].y - stacked[0].y).abs() < 0.05);
        assert!((opened[3].y - stacked[3].y).abs() < 0.05);
        assert!((opened[1].y - (stacked[0].y + step * 4.0)).abs() < 0.05);
        assert!((opened[2].y - (stacked[0].y + step * 8.0)).abs() < 0.05);
        for (from, to) in stacked.iter().zip(opened) {
            assert!((to.x - from.x).abs() < 0.05);
        }

        let mut pair = empty_simple_app();
        pair.place_performer_at(origin, true);
        pair.place_performer_at(
            Point {
                x: origin.x + step * 6.0,
                y: origin.y,
            },
            true,
        );
        let two = pair.document.sets[0].positions.clone();
        let revision = pair.history.revision();
        pair.simple_even_gaps();
        assert_eq!(pair.document.sets[0].positions, two);
        assert_eq!(pair.history.revision(), revision);

        app.playing = true;
        let locked = app.document.sets[0].positions.clone();
        let revision = app.history.revision();
        app.simple_even_gaps();
        assert_eq!(app.document.sets[0].positions, locked);
        assert_eq!(app.history.revision(), revision);
    }

    #[test]
    fn a_chevron_points_toward_the_audience_and_undoes() {
        let mut app = empty_simple_app();
        let (step, _) = grid_step(&app);
        let mid = app.document.grid.snap(Point {
            x: app.document.grid.width * 0.5,
            y: app.document.grid.height * 0.5,
        });
        let spots = [
            Point {
                x: mid.x - step * 6.0,
                y: mid.y + step * 4.0,
            },
            Point {
                x: mid.x + step * 2.0,
                y: mid.y - step * 3.0,
            },
            Point {
                x: mid.x + step * 7.0,
                y: mid.y + step,
            },
            Point {
                x: mid.x - step,
                y: mid.y + step * 6.0,
            },
            Point {
                x: mid.x + step * 4.0,
                y: mid.y + step * 5.0,
            },
        ];
        for spot in spots {
            app.place_performer_at(spot, true);
        }
        let before = app.document.sets[0].positions.clone();
        let schema = app.document.schema_version;
        app.simple_sync_history_notes();
        app.simple_chevron_up();
        let after = app.document.sets[0].positions.clone();
        assert!(after[0].y + 0.2 < after[1].y.min(after[2].y));
        assert!(after[1].x + 0.2 < after[0].x);
        assert!(after[0].x + 0.2 < after[2].x);
        assert!(after[3].x + 0.2 < after[1].x);
        assert!(after[1].y + 0.2 < after[3].y);
        assert!(after[2].x + 0.2 < after[4].x);
        assert!(after[2].y + 0.2 < after[4].y);
        assert!(on_field(&app, &after));
        app.simple_sync_history_notes();
        let lines = app.simple_history_lines();
        assert!(
            lines.iter().any(|line| line.contains("くの字")),
            "{lines:?}"
        );
        let json = app.document.to_json().expect("json");
        let loaded = drill_core::Document::from_json(&json).expect("reload");
        assert_eq!(loaded.sets[0].positions, after);
        assert_eq!(loaded.schema_version, schema);
        let revision = app.history.revision();
        app.simple_chevron_up();
        assert_eq!(app.document.sets[0].positions, after);
        assert_eq!(app.history.revision(), revision);
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.sets[0].positions, before);

        app.simple_chevron_up();
        let parked = app.document.sets[0].positions[4];
        app.replace_selection([0_usize, 1, 2, 3].into_iter().collect());
        app.simple_chevron_up();
        assert_eq!(app.document.sets[0].positions[4], parked);

        app.playing = true;
        let locked = app.document.sets[0].positions.clone();
        let revision = app.history.revision();
        app.simple_chevron_up();
        assert_eq!(app.document.sets[0].positions, locked);
        assert_eq!(app.history.revision(), revision);
    }

    #[test]
    fn choosing_who_stays_leaves_the_drill_unchanged() {
        let mut app = empty_simple_app();
        let (step, _) = grid_step(&app);
        app.place_performer_at(Point { x: 12.0, y: 16.0 }, true);
        app.place_performer_at(Point { x: 20.0, y: 16.0 }, true);
        app.place_performer_at(Point { x: 28.0, y: 16.0 }, true);
        assert!(simple_holder_indices(&app.document, 0).is_empty());
        app.duplicate_current_set();
        let set_id = app.document.sets[1].id;
        let mover = app.document.performers[0].id;
        let from = app.document.sets[1].positions[0];
        assert!(app.execute_edit(
            Edit::MovePerformers {
                set_id,
                performer_ids: vec![mover],
                positions: vec![Point {
                    x: from.x + step * 4.0,
                    y: from.y,
                }],
            },
            "one moves",
        ));
        app.navigate_to_set(1);
        app.nav_glide.settle();
        let document = app.document.clone();
        let revision = app.history.revision();
        assert_eq!(simple_holder_indices(&app.document, 1), vec![1, 2]);
        app.replace_selection(std::iter::once(0).collect());
        app.simple_select_holders();
        assert_eq!(app.selected, [1_usize, 2].into_iter().collect());
        assert_eq!(app.document, document);
        assert_eq!(app.history.revision(), revision);
        let json = app.document.to_json().expect("json");
        let loaded = drill_core::Document::from_json(&json).expect("reload");
        assert_eq!(loaded.sets[1].positions, document.sets[1].positions);
        assert_eq!(loaded.schema_version, document.schema_version);

        app.playing = true;
        app.replace_selection(std::iter::once(0).collect());
        app.simple_select_holders();
        assert_eq!(app.selected, std::iter::once(0).collect());
        app.playing = false;

        let ids: Vec<_> = app
            .document
            .performers
            .iter()
            .map(|person| person.id)
            .collect();
        let all = app.document.sets[1].positions.clone();
        assert!(
            app.execute_edit(
                Edit::MovePerformers {
                    set_id,
                    performer_ids: ids,
                    positions: all
                        .iter()
                        .map(|point| Point {
                            x: point.x + step * 2.0,
                            y: point.y,
                        })
                        .collect(),
                },
                "everyone moves",
            )
        );
        assert!(simple_holder_indices(&app.document, 1).is_empty());
        app.replace_selection(std::iter::once(2).collect());
        app.simple_select_holders();
        assert_eq!(app.selected, std::iter::once(2).collect());
    }

    #[test]
    fn going_back_adds_one_scene_and_undoes_it() {
        let mut app = empty_simple_app();
        let (step, _) = grid_step(&app);
        app.place_performer_at(Point { x: 12.0, y: 16.0 }, true);
        app.place_performer_at(Point { x: 24.0, y: 18.0 }, true);
        let opening = app.document.sets[0].positions.clone();
        let labels: Vec<_> = app
            .document
            .performers
            .iter()
            .map(|person| person.label.clone())
            .collect();
        let schema = app.document.schema_version;
        app.duplicate_current_set();
        app.navigate_to_set(1);
        app.nav_glide.settle();
        let set_id = app.document.sets[1].id;
        let ids: Vec<_> = app
            .document
            .performers
            .iter()
            .map(|person| person.id)
            .collect();
        let arrived: Vec<_> = app.document.sets[1]
            .positions
            .iter()
            .map(|point| Point {
                x: point.x + step * 4.0,
                y: point.y - step * 2.0,
            })
            .collect();
        assert!(app.execute_edit(
            Edit::MovePerformers {
                set_id,
                performer_ids: ids,
                positions: arrived.clone(),
            },
            "move",
        ));
        let revision = app.history.revision();
        app.navigate_to_set(0);
        app.nav_glide.settle();
        app.simple_return_scene();
        assert_eq!(app.document.sets.len(), 2);
        assert_eq!(app.history.revision(), revision);

        app.navigate_to_set(1);
        app.nav_glide.settle();
        app.simple_sync_history_notes();
        app.simple_return_scene();
        assert_eq!(app.document.sets.len(), 3);
        assert_eq!(app.current_set, 2);
        assert_eq!(app.document.sets[0].positions, opening);
        assert_eq!(app.document.sets[1].positions, arrived);
        assert_eq!(app.document.sets[2].positions, opening);
        assert!(app.document.sets[2].routes.is_trivial());
        assert!(app.document.sets[2].generated_by.is_none());
        assert_eq!(app.document.sets[2].name, "セット 3");
        assert_eq!(app.count_position, 0.0);
        app.simple_sync_history_notes();
        let lines = app.simple_history_lines();
        assert!(
            lines.iter().any(|line| line.contains("もどる")),
            "{lines:?}"
        );
        let json = app.document.to_json().expect("json");
        let loaded = drill_core::Document::from_json(&json).expect("reload");
        assert_eq!(loaded.sets.len(), 3);
        assert_eq!(loaded.sets[2].positions, opening);
        assert_eq!(loaded.sets[1].positions, arrived);
        assert_eq!(loaded.schema_version, schema);
        assert_eq!(
            loaded
                .performers
                .iter()
                .map(|person| person.label.as_str())
                .collect::<Vec<_>>(),
            labels.iter().map(String::as_str).collect::<Vec<_>>()
        );
        assert!(app.history.undo(&mut app.document));
        app.clamp_session_to_document();
        assert_eq!(app.document.sets.len(), 2);
        assert_eq!(app.document.sets[1].positions, arrived);
        assert_eq!(app.current_set, 1);

        app.simple_step_beat(-4);
        assert!(app.simple_holding_a_count());
        let held = app.document.sets.clone();
        let revision = app.history.revision();
        app.simple_return_scene();
        assert_eq!(app.document.sets, held);
        assert_eq!(app.history.revision(), revision);

        app.navigate_to_set(1);
        app.nav_glide.settle();
        app.playing = true;
        let revision = app.history.revision();
        app.simple_return_scene();
        assert_eq!(app.document.sets.len(), 2);
        assert_eq!(app.history.revision(), revision);
        app.playing = false;
        app.navigate_to_set(0);
        app.nav_glide.settle();
        let revision = app.history.revision();
        app.simple_return_scene();
        assert_eq!(app.document.sets.len(), 2);
        assert_eq!(app.history.revision(), revision);
    }

    #[test]
    fn whole_show_length_adds_every_move() {
        let mut doc = drill_core::Document::blank(2);
        let mut second = doc.sets[0].clone();
        second.id = doc.next_set_id().expect("id");
        second.name = "セット 2".into();
        doc.sets.push(second);
        assert_eq!(simple_show_seconds(&doc), Some(8));
        assert_eq!(
            simple_show_length_text(drill_core::Locale::Ja, 8),
            "全体 約 8秒"
        );
        assert_eq!(
            simple_show_length_text(drill_core::Locale::Ja, 60),
            "全体 約 1分"
        );
        assert_eq!(
            simple_show_length_text(drill_core::Locale::Ja, 80),
            "全体 約 1分20秒"
        );
        assert_eq!(
            simple_show_length_text(drill_core::Locale::En, 80),
            "Whole show, about 1 min 20 sec"
        );
        doc.sets[0].counts = 160;
        assert_eq!(simple_show_seconds(&doc), Some(80));
        assert_eq!(simple_chip_caption("セット 3", 0), "1");
        assert_eq!(simple_chip_caption("サビ", 1), "2 サビ");
        assert_eq!(
            simple_chip_caption("とても長い場面の名前", 2),
            "3 とても長い場"
        );
    }

    #[test]
    fn shifting_a_scene_keeps_the_typed_name_and_undoes() {
        let mut app = moved_pair();
        app.simple_rename_scene("サビ");
        let first = app.document.sets[0].positions.clone();
        let second = app.document.sets[1].positions.clone();
        assert_ne!(first, second);
        app.simple_sync_history_notes();
        app.simple_shift_scene(false);
        assert_eq!(app.current_set, 0);
        assert_eq!(app.document.sets[0].positions, second);
        assert_eq!(app.document.sets[1].positions, first);
        assert_eq!(app.document.sets[0].name, "サビ");
        assert!(simple_stock_scene_name(&app.document.sets[1].name));
        app.simple_sync_history_notes();
        let lines = app.simple_history_lines();
        assert!(
            lines.iter().any(|line| line.contains("前と入れ替え")),
            "{lines:?}"
        );
        let json = app.document.to_json().expect("json");
        let loaded = drill_core::Document::from_json(&json).expect("reload");
        assert_eq!(loaded.sets[0].name, "サビ");
        assert_eq!(loaded.sets[0].positions, second);
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.sets[0].positions, first);
        assert_eq!(app.document.sets[1].positions, second);
        assert_eq!(app.document.sets[1].name, "サビ");

        app.navigate_to_set(0);
        app.nav_glide.settle();
        let revision = app.history.revision();
        app.simple_shift_scene(false);
        assert_eq!(app.history.revision(), revision);

        app.count_position = 4.0;
        app.simple_shift_scene(true);
        assert_eq!(app.history.revision(), revision);
        assert_eq!(app.document.sets[0].positions, first);
    }

    #[test]
    fn the_whole_show_sheet_lists_every_scene() {
        let mut app = moved_pair();
        app.simple_rename_scene("サビ");
        let sheet = simple_show_sheet_text(&app.document, drill_core::Locale::Ja);
        assert!(sheet.contains("全体の位置表"), "{sheet}");
        assert!(sheet.contains("全体 約"), "{sheet}");
        assert!(sheet.contains("サビ"), "{sheet}");
        assert!(sheet.contains('1'), "{sheet}");
        assert!(sheet.contains('2'), "{sheet}");
        assert!(sheet.matches("場面").count() >= 2, "{sheet}");
    }

    fn loaded_copy(document: drill_core::Document) -> drill_project::container::LoadedProject {
        drill_project::container::LoadedProject {
            document,
            manifest: drill_project::container::Manifest {
                container_version: 1,
                document_schema_version: drill_core::SCHEMA_VERSION,
                document_entry: "document.json".into(),
                app_version: "test".into(),
                created_utc: String::new(),
                modified_utc: String::new(),
                summary: Default::default(),
                assets: Vec::new(),
            },
            embedded: std::collections::BTreeMap::new(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn simple_recovery_opens_without_keeping_the_autosave_path() {
        let dir = std::env::temp_dir().join(format!(
            "drillforge-recover-{}-{}",
            std::process::id(),
            "copy"
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let mut app = empty_simple_app();
        app.simple_mode.draft_path_override = Some(dir.join("simple-draft.drill.json"));
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        app.current_path = Some(std::path::PathBuf::from("文化祭.drill.json"));
        let recovered = drill_core::Document::blank(3);
        let path = std::path::PathBuf::from("recovery/session/autosave.drill.json");
        app.project_state
            .recoveries
            .push(drill_project::recovery::RecoveryCandidate {
                root: std::path::PathBuf::from("recovery/session"),
                autosave: path.clone(),
                meta: drill_project::recovery::SessionMeta {
                    session_id: "session".into(),
                    app_version: "test".into(),
                    started_unix_ms: 0,
                    heartbeat_unix_ms: 0,
                    document_title: "文化祭".into(),
                    origin_name: None,
                },
                autosave_bytes: 128,
                stale_for: std::time::Duration::from_secs(40),
            });
        app.apply_loaded_project(path, Box::new(loaded_copy(recovered)));
        assert!(app.current_path.is_none());
        assert!(app.dirty);
        assert!(app.project_state.recoveries.is_empty());
        assert_eq!(app.document.performers.len(), 3);

        let named = std::path::PathBuf::from("shows/文化祭.drill.json");
        app.apply_loaded_project(
            named.clone(),
            Box::new(loaded_copy(drill_core::Document::blank(1))),
        );
        assert_eq!(app.current_path.as_ref(), Some(&named));
        assert!(!app.dirty);
        assert_eq!(app.document.performers.len(), 1);

        app.set_simple_mode(false);
        let autosave = std::path::PathBuf::from("recovery/session/autosave.drill.json");
        app.apply_loaded_project(
            autosave.clone(),
            Box::new(loaded_copy(drill_core::Document::blank(2))),
        );
        assert_eq!(app.current_path.as_ref(), Some(&autosave));
        assert!(!app.dirty);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn contest_line_flags_the_edge_and_ignores_a_field_without_one() {
        let grid = drill_core::GridConfig::japan_floor();
        let (min_x, min_y, _max_x, max_y) = grid.reference_frame_bounds()[0];
        let mid_y = (min_y + max_y) * 0.5;
        assert!(!point_outside_contest(&grid, Point { x: min_x, y: mid_y }));
        assert!(point_outside_contest(
            &grid,
            Point {
                x: min_x - 0.2,
                y: mid_y,
            }
        ));
        assert!(outside_contest_indices(&drill_core::Document::demo(1, 2), 0).is_empty());

        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 23.0, y: 20.0 }, true);
        app.place_performer_at(Point { x: 1.0, y: 20.0 }, true);
        let outside = outside_contest_indices(&app.document, 0);
        assert_eq!(outside, vec![1]);
        let mark = simple_floor_mark(&app.document.grid, drill_core::Locale::Ja).expect("30m");
        assert_eq!(mark.1, "30m");

        app.duplicate_current_set();
        let set_id = app.document.sets[0].id;
        let inside = app.document.performers[0].id;
        assert!(app.execute_edit(
            Edit::MovePerformers {
                set_id,
                performer_ids: vec![inside],
                positions: vec![Point { x: 1.0, y: 20.0 }],
            },
            "edge",
        ));
        app.navigate_to_set(1);
        app.nav_glide.settle();
        let leaving = outside_contest_indices(&app.document, 1);
        assert!(leaving.contains(&0));
    }

    #[test]
    fn one_person_path_names_each_scene_and_the_walk() {
        let app = moved_pair();
        let path = simple_person_path_text(&app.document, 0, drill_core::Locale::Ja);
        assert!(path.contains("1 の道"));
        assert!(path.contains("場面 1"));
        assert!(path.contains("場面 2"));
        assert!(path.contains("次まで"));
        assert!(path.contains("歩"));
        let still = simple_person_path_text(&app.document, 1, drill_core::Locale::Ja);
        assert!(still.contains("動きません"));
        assert!(!still.contains("次まで"));
        let english = simple_person_path_text(&app.document, 0, drill_core::Locale::En);
        assert!(english.contains("path"));
        assert_ne!(path, english);
    }

    #[test]
    fn walk_sheet_lists_every_scene_without_writing() {
        let app = moved_pair();
        let before = app.document.clone();
        let sheet = rehearsal_walk_sheet(&app.document, drill_core::Locale::Ja, Some("文化祭"));
        assert!(sheet.starts_with("文化祭\n"));
        assert!(sheet.contains("全体"));
        assert!(sheet.matches("場面").count() >= 2);
        assert_eq!(app.document, before);
        let file = rehearsal_walk_file(&sheet);
        assert!(file.starts_with('\u{FEFF}'));
        assert!(file.contains("文化祭"));
        let plain = rehearsal_walk_sheet(&app.document, drill_core::Locale::En, None);
        assert!(plain.contains(&app.document.title));
        assert!(plain.contains("Scene"));
    }

    #[test]
    fn focus_and_away_stay_out_of_the_show_file() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 12.0, y: 16.0 }, true);
        app.place_performer_at(Point { x: 20.0, y: 16.0 }, true);
        let before = app.document.to_json().expect("json");
        let revision = app.history.revision();
        app.simple_toggle_focus();
        app.simple_toggle_away();
        assert!(app.simple_focus_index().is_some());
        assert_eq!(app.simple_away_count(), 1);
        assert!(app.simple_is_away(app.simple_single_selected().expect("one")));
        assert_eq!(app.document.to_json().expect("json"), before);
        assert_eq!(app.history.revision(), revision);
        app.simple_toggle_focus();
        assert!(app.simple_focus_index().is_none());
        app.simple_toggle_away();
        assert_eq!(app.simple_away_count(), 0);
        app.simple_toggle_away();
        app.begin_simple_show();
        assert!(app.simple_mode.focus.is_none());
        assert!(app.simple_mode.away.is_empty());
    }

    #[test]
    fn pictogram_hints_follow_place_drag_next_then_play() {
        let mut app = empty_simple_app();
        assert_eq!(app.simple_active_hint(), Some(SimpleGuide::Place));
        app.simple_add_person();
        assert!(app.simple_mode.hint_place_done);
        assert_eq!(app.simple_active_hint(), Some(SimpleGuide::Move));
        app.replace_selection([0].into_iter().collect());
        let start = app.document.sets[0].positions[0];
        app.begin_field_drag(Pos2::new(0.0, 0.0));
        app.update_field_drag(Pos2::new(50.0, 0.0), 10.0, true);
        app.commit_field_drag();
        app.simple_mark_hint(SimpleGuide::Move);
        assert_ne!(app.document.sets[0].positions[0], start);
        assert_eq!(app.simple_active_hint(), Some(SimpleGuide::NextSet));
        app.duplicate_current_set();
        app.simple_mark_hint(SimpleGuide::NextSet);
        assert_eq!(app.document.sets.len(), 2);
        assert_eq!(app.simple_active_hint(), Some(SimpleGuide::Play));
        app.simple_mark_hint(SimpleGuide::Play);
        assert!(app.onboarding.simple_pictogram_done);
        assert!(app.simple_active_hint().is_none());
    }

    #[test]
    fn closing_the_hint_hides_it_for_the_session() {
        let mut app = empty_simple_app();
        assert!(app.simple_active_hint().is_some());
        app.simple_mode.hint_closed = true;
        assert!(app.simple_active_hint().is_none());
    }

    #[test]
    fn trash_commits_only_after_a_one_second_hold() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        app.replace_selection([0].into_iter().collect());
        assert_eq!(app.simple_delete_hold_progress(false), 0.0);
        app.simple_mode.delete_hold = Some(Instant::now());
        assert!(app.simple_delete_hold_progress(true) < 1.0);
        assert_eq!(app.document.performers.len(), 1);
        app.simple_mode.delete_hold = Some(Instant::now() - Duration::from_millis(1100));
        assert_eq!(app.simple_delete_hold_progress(true), 1.0);
        app.simple_commit_held_delete();
        assert!(app.document.performers.is_empty());
    }

    #[test]
    fn a_blocked_edit_flashes_the_orange_warn() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        app.playing = true;
        app.simple_add_person();
        assert_eq!(app.document.performers.len(), 1);
        assert!(app.simple_fail_t().is_some());
        assert!(app.simple_mode.fail_until.is_some());
    }

    #[test]
    fn member_tally_uses_a_numeral_past_twelve() {
        assert_eq!(super::i18n::roster_count(drill_core::Locale::Ja, 2), "2人");
        assert_eq!(
            super::i18n::roster_count(drill_core::Locale::Ja, 80),
            "80人"
        );
    }

    #[test]
    fn cast_show_roundtrips_eighty_people_and_six_sets() {
        let doc = qa_cast_document();
        assert_eq!(doc.performers.len(), 80);
        assert!(doc.sections.len() >= 5);
        assert_eq!(doc.sets.len(), 6);
        let counts: Vec<u16> = doc.sets.iter().map(|set| set.counts).collect();
        assert!(counts.windows(2).any(|pair| pair[0] != pair[1]));
        assert!(
            doc.performers
                .iter()
                .any(|person| person.label.contains('山'))
        );
        let json = doc.to_json().expect("json");
        let loaded = drill_core::Document::from_json(&json).expect("reload");
        assert_eq!(loaded.performers.len(), 80);
        assert_eq!(loaded.sets.len(), 6);
        assert_eq!(loaded.sets[3].counts, doc.sets[3].counts);
        assert_eq!(loaded.sections.len(), doc.sections.len());
    }

    #[test]
    fn short_moves_skip_the_path_line_unless_selected() {
        let step = 0.5;
        assert!(!movement_line_visible(step * 2.0, step, false));
        assert!(movement_line_visible(step * 5.0, step, false));
        assert!(movement_line_visible(step * 2.0, step, true));
        assert!(!movement_line_visible(0.0, step, true));
        let same = Point { x: 4.0, y: 4.0 };
        assert!(!previous_ghost_separated(same, same));
        assert!(previous_ghost_separated(
            same,
            Point {
                x: same.x + 1.0,
                y: same.y
            }
        ));
    }

    #[test]
    fn dragging_twenty_people_snaps_then_undo_restores_them() {
        let mut app = DrillApp::default();
        app.install_document(qa_cast_document());
        app.set_simple_mode(false);
        app.navigate_to_set(0);
        app.nav_glide.settle();
        app.count_position = 0.0;
        app.playing = false;
        app.replace_selection((0..20).collect());
        let before: Vec<Point> = app.document.sets[0]
            .positions
            .iter()
            .take(20)
            .copied()
            .collect();
        let origin = egui::pos2(400.0, 300.0);
        app.begin_field_drag(origin);
        app.update_field_drag(origin + egui::vec2(48.0, 0.0), 20.0, true);
        let preview = app.drag_preview.clone().expect("preview");
        assert_eq!(preview.len(), 20);
        for (start, end) in before.iter().zip(preview.iter()) {
            assert!((end.x - start.x).abs() > 0.2, "drag did not move {start:?}");
            let snapped = app.document.grid.snap(*end);
            assert!((snapped.x - end.x).abs() < 1e-3 && (snapped.y - end.y).abs() < 1e-3);
        }
        app.commit_field_drag();
        let moved = app.document.sets[0].positions[0];
        assert_ne!(moved, before[0]);
        assert!(app.history.can_undo());
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.sets[0].positions[0], before[0]);
        assert!(app.history.redo(&mut app.document));
        assert_eq!(app.document.sets[0].positions[0], moved);
    }

    #[test]
    fn range_select_keeps_only_dots_inside_the_rectangle() {
        let points = [
            Point { x: 1.0, y: 1.0 },
            Point { x: 4.0, y: 4.0 },
            Point { x: 9.0, y: 9.0 },
        ];
        let map = |point: Point| egui::pos2(point.x * 10.0, point.y * 10.0);
        let marquee = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(50.0, 50.0));
        let hit: Vec<usize> = points
            .iter()
            .enumerate()
            .filter(|(_, point)| marquee.contains(map(**point)))
            .map(|(index, _)| index)
            .collect();
        assert_eq!(hit, vec![0, 1]);
    }

    #[test]
    fn midway_count_sits_between_the_two_pictures() {
        let doc = qa_cast_document();
        let mut mid = Vec::new();
        doc.positions_at(2, 0.5, &mut mid);
        let start = doc.sets[2].positions[0];
        let end = doc.sets[3].positions[0];
        assert!((start.x - end.x).hypot(start.y - end.y) > 0.4);
        let at = mid[0];
        let expected = Point {
            x: (start.x + end.x) * 0.5,
            y: (start.y + end.y) * 0.5,
        };
        assert!(
            (at.x - expected.x).hypot(at.y - expected.y) < 0.2,
            "mid {at:?} expected near {expected:?}"
        );
    }

    #[test]
    fn current_set_export_is_one_chart_and_the_file_reopens() {
        let doc = qa_cast_document();
        let state = super::print_state::PrintState::default();
        let all = state.report(&doc, 4, &[]);
        let charts = all
            .items
            .iter()
            .filter(|item| matches!(item, drill_export::page::PageItem::FieldDiagram { .. }))
            .count();
        assert_eq!(charts, 6);
        let mut one = super::print_state::PrintState::default();
        one.current_set_only = true;
        one.scope_set = 2;
        let narrowed = one.report(&doc, 4, &[]);
        let one_chart = narrowed
            .items
            .iter()
            .filter(|item| matches!(item, drill_export::page::PageItem::FieldDiagram { .. }))
            .count();
        assert_eq!(one_chart, 1);
        let dir = std::env::temp_dir().join(format!("drillforge-reopen-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("文化祭.drill.json");
        let json = doc.to_json().expect("json");
        drill_project::atomic_write(&path, json.as_bytes(), None).expect("write");
        let loaded =
            drill_core::Document::from_json(&std::fs::read_to_string(&path).expect("read"))
                .expect("reload");
        assert_eq!(loaded.title, doc.title);
        assert_eq!(loaded.sets[2].positions, doc.sets[2].positions);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn side_chips_slide_above_the_dock() {
        let field = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1280.0, 800.0));
        let size = egui::vec2(40.0, 28.0);
        let ceiling = 680.0;
        let origin = front_chip_origin(field, egui::pos2(250.0, 790.0), size, ceiling);
        let chip = egui::Rect::from_min_size(origin, size);
        assert!(
            chip.bottom() <= ceiling + 0.5,
            "chip overlaps the dock: {chip:?}"
        );
        assert!(field.contains_rect(chip), "{chip:?} not inside {field:?}");
        let right = front_chip_origin(field, egui::pos2(1260.0, 790.0), size, ceiling);
        assert!(right.x + size.x <= field.right() - 7.0, "{right:?}");
    }

    #[test]
    fn autosave_flashes_a_green_check_without_a_save_button() {
        let mut app = empty_simple_app();
        app.simple_flash_check();
        assert!(app.simple_check_t().is_some());
        assert!(app.simple_mode.check_until.is_some());
    }

    #[test]
    fn idle_hint_keeps_only_the_pulse() {
        let mut app = empty_simple_app();
        assert!(!app.simple_mode.hint_pulse_only);
        app.simple_mode.last_input_at = Some(Instant::now() - Duration::from_secs(11));
        app.simple_apply_hint_idle(Instant::now());
        assert!(app.simple_mode.hint_pulse_only);
        app.simple_mark_hint(SimpleGuide::Place);
        assert!(!app.simple_mode.hint_pulse_only);
        assert_eq!(app.simple_active_hint(), Some(SimpleGuide::Move));
    }

    #[test]
    fn hint_hand_stops_after_three_repeats() {
        let mut app = empty_simple_app();
        let period = SIMPLE_HINT_MOVE_SECS + SIMPLE_HINT_REST_SECS;
        app.simple_mode.hint_phase_at = Some(
            Instant::now()
                - Duration::from_secs_f32(f32::from(SIMPLE_HINT_MAX_REPEATS) * period + 0.05),
        );
        let (_, pulse_only) = app.simple_hint_cycle();
        assert!(pulse_only);
    }

    #[test]
    fn play_hint_waits_until_there_are_two_sets() {
        let mut app = empty_simple_app();
        app.simple_mark_hint(SimpleGuide::Place);
        app.simple_mark_hint(SimpleGuide::Move);
        app.simple_mark_hint(SimpleGuide::NextSet);
        assert!(app.simple_active_hint().is_none());
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        app.duplicate_current_set();
        assert_eq!(app.simple_active_hint(), Some(SimpleGuide::Play));
    }

    #[test]
    fn closing_hints_does_not_persist_them() {
        let mut app = empty_simple_app();
        app.simple_mode.hint_closed = true;
        assert!(app.simple_active_hint().is_none());
        assert!(!app.onboarding.simple_pictogram_done);
    }

    #[test]
    fn autosave_waits_for_the_debounce_window() {
        let dir = std::env::temp_dir().join(format!(
            "drillforge-debounce-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("simple-draft.drill.json");
        let mut app = empty_simple_app();
        app.simple_mode.draft_path_override = Some(path.clone());
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        assert!(app.dirty);
        let ctx = egui::Context::default();
        app.autosave_simple_draft(&ctx);
        assert!(app.simple_mode.draft_debounce_at.is_some());
        assert!(!path.exists());
        app.simple_mode.draft_debounce_at =
            Some(Instant::now() - Duration::from_millis(SIMPLE_AUTOSAVE_MS + 20));
        app.autosave_simple_draft(&ctx);
        assert!(path.exists());
        assert!(app.simple_mode.check_until.is_some());
    }

    fn attach_song(app: &mut DrillApp, path: &str, offset: f32) {
        let mut track = drill_core::audio::AudioTrack {
            path: path.to_string(),
            duration_seconds: 180.0,
            offset_seconds: offset,
            ..drill_core::audio::AudioTrack::default()
        };
        apply_simple_song_offset(&mut track, offset);
        assert!(app.simple_commit_song(Some(track), "simple-mode.340"));
    }

    #[test]
    fn song_sync_offset_persists_in_the_shared_audio_track() {
        let dir = std::env::temp_dir().join(format!(
            "drillforge-song-{}-{}",
            std::process::id(),
            "offset"
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let draft = dir.join("simple-draft.drill.json");
        let mut app = empty_simple_app();
        app.simple_mode.draft_path_override = Some(draft.clone());
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        attach_song(&mut app, "文化祭ステージ.mp3", 1.25);
        app.simple_nudge_song(2);
        app.simple_nudge_song(-1);
        let offset = app.document.audio.as_ref().unwrap().offset_seconds;
        assert!((offset - 1.30).abs() < 1e-4);
        let at_four = simple_song_seconds(
            app.document.audio.as_ref().unwrap(),
            &app.document.tempo,
            4.0,
        );
        assert!((at_four - (1.30 + 2.0)).abs() < 1e-3);
        app.simple_nudge_song(-40);
        let negative = app.document.audio.as_ref().unwrap();
        assert!(negative.offset_seconds < 0.0);
        assert!(negative.anchors.is_empty());
        assert!(app.history.undo(&mut app.document));
        assert!((app.document.audio.as_ref().unwrap().offset_seconds - 1.30).abs() < 1e-4);
        let json = app.document.to_json().expect("json");
        let loaded = drill_core::Document::from_json(&json).expect("reload");
        let loaded_track = loaded.audio.as_ref().expect("song");
        assert!((loaded_track.offset_seconds - 1.30).abs() < 1e-4);
        assert!((simple_song_seconds(loaded_track, &loaded.tempo, 0.0) - 1.30).abs() < 1e-3);
        assert_eq!(loaded_track.anchors.anchors().len(), 1);
        assert!(app.write_simple_draft());
        app.set_simple_mode(false);
        assert!((app.document.audio.as_ref().unwrap().offset_seconds - 1.30).abs() < 1e-4);
        let mut reopened = DrillApp::default();
        reopened.onboarding.welcome_seen = true;
        reopened.onboarding.prefer_simple = true;
        reopened.onboarding.show_welcome = false;
        reopened.simple_mode.draft_path_override = Some(draft);
        reopened.open_into_preferred_editor();
        assert!(reopened.simple_mode.enabled);
        assert_eq!(reopened.document.performers.len(), 1);
        assert!((reopened.document.audio.as_ref().unwrap().offset_seconds - 1.30).abs() < 1e-4);
        assert!(reopened.simple_mode.song_missing);
        assert!(!reopened.document.sets.is_empty());
        assert!(include_str!("i18n_generated.rs").contains(
            "(Locale::Ja, \"simple-mode.347\") => \"曲が見つかりません。もう一度選んでください\""
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn qa_guide_stages_open_the_matching_hint() {
        let mut place = empty_simple_app();
        place.apply_qa_simple_guide("guide-place");
        assert!(place.document.performers.is_empty());
        assert_eq!(place.simple_active_hint(), Some(SimpleGuide::Place));

        let mut moved = empty_simple_app();
        moved.apply_qa_simple_guide("guide-move");
        assert_eq!(moved.document.performers.len(), 3);
        assert_eq!(moved.simple_active_hint(), Some(SimpleGuide::Move));

        let mut play = empty_simple_app();
        play.apply_qa_simple_guide("guide-play");
        assert_eq!(play.document.sets.len(), 2);
        assert_eq!(play.simple_active_hint(), Some(SimpleGuide::Play));

        let mut playing = empty_simple_app();
        playing.apply_qa_simple_guide("guide-playing");
        assert!(playing.playing);
        assert_eq!(playing.current_set, 0);
        assert!(playing.playback_end >= playing.document.timeline_counts());
        assert!(playing.onboarding.simple_pictogram_done);
        assert!(playing.simple_active_hint().is_none());

        let mut collapsed = empty_simple_app();
        collapsed.apply_qa_simple_guide("guide-collapsed");
        assert!(collapsed.onboarding.simple_toolbar_collapsed);
        assert!(collapsed.onboarding.simple_pictogram_done);
        assert!(collapsed.simple_active_hint().is_none());
        assert!(!collapsed.simple_toolbar_open());
        assert_eq!(collapsed.document.performers.len(), 2);
        assert_eq!(collapsed.document.sets.len(), 2);

        let mut song = empty_simple_app();
        song.apply_qa_simple_guide("guide-song");
        let loaded = song.document.audio.as_ref().expect("song");
        assert!(!song.simple_mode.song_missing);
        assert!(simple_song_name(&loaded.path).contains("文化祭"));
        assert!(song.simple_toolbar_open());
    }

    #[test]
    fn collapsed_toolbar_reopens_while_a_hint_is_showing() {
        let mut app = empty_simple_app();
        app.onboarding.simple_toolbar_collapsed = true;
        assert_eq!(app.simple_active_hint(), Some(SimpleGuide::Place));
        assert!(app.simple_toolbar_open());
        app.onboarding.simple_pictogram_done = true;
        app.simple_mode.hint_closed = true;
        assert!(app.simple_active_hint().is_none());
        assert!(!app.simple_toolbar_open());
    }

    #[test]
    fn overlay_rects_union_into_one_block() {
        let mut block = None;
        overlay_union(
            &mut block,
            egui::Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(10.0, 10.0)),
        );
        overlay_union(
            &mut block,
            egui::Rect::from_min_max(Pos2::new(8.0, 8.0), Pos2::new(20.0, 12.0)),
        );
        let block = block.expect("union");
        assert_eq!(block.min, Pos2::new(0.0, 0.0));
        assert_eq!(block.max, Pos2::new(20.0, 12.0));
    }

    fn moved_second_scene() -> (DrillApp, Point, Point) {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 4.0, y: 8.0 }, true);
        app.duplicate_current_set();
        let set_id = app.document.sets[app.current_set].id;
        let performer_id = app.document.performers[0].id;
        let mut positions = app.document.sets[app.current_set].positions.clone();
        positions[0].x = 26.0;
        assert!(app.execute_edit(
            Edit::MovePerformers {
                set_id,
                performer_ids: vec![performer_id],
                positions,
            },
            "move",
        ));
        let ghost = app.document.sets[0].positions[0];
        let live = app.document.sets[1].positions[0];
        (app, ghost, live)
    }

    #[test]
    fn previous_ghosts_are_not_click_drag_or_box_targets() {
        let (mut app, ghost, live) = moved_second_scene();
        let gap = (ghost.x - live.x).hypot(ghost.y - live.y);
        assert!(gap > SIMPLE_DOT_HIT_RADIUS);
        let map = |point: Point| Pos2::new(point.x, point.y);
        let ghost_screen = map(ghost);
        let live_only = [live];
        let ghost_only = [ghost];
        assert_eq!(
            simple_hit_index(
                ghost_screen,
                &live_only,
                &map,
                SIMPLE_DOT_HIT_RADIUS,
                |_| true
            ),
            None
        );
        assert_eq!(
            simple_hit_index(map(live), &live_only, &map, SIMPLE_DOT_HIT_RADIUS, |_| true),
            Some(0)
        );
        let ghost_box = Rect::from_center_size(ghost_screen, Vec2::splat(4.0));
        assert!(simple_box_hits(ghost_box, &live_only, &map, |_| true).is_empty());
        assert_eq!(
            simple_box_hits(ghost_box, &ghost_only, &map, |_| true),
            vec![0]
        );
        let both = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(40.0, 40.0));
        assert_eq!(simple_box_hits(both, &live_only, &map, |_| true), vec![0]);

        app.simple_mode.show_previous = true;
        app.replace_selection(std::iter::once(0).collect());
        app.simple_click(ghost, None, true, false);
        assert!(app.selected.is_empty());
        app.simple_start_drag(None, ghost_screen);
        assert!(app.drag_before.is_none());
        assert!(app.simple_mode.empty_pan);
        assert!(simple_previous_toggle_width("前の場面") >= 48.0);
        assert!(simple_previous_toggle_width("Previous") >= 48.0);
    }

    #[test]
    fn previous_ghosts_hide_on_the_first_scene_and_while_playing() {
        let (mut app, _, _) = moved_second_scene();
        assert_eq!(app.current_set, 1);
        app.simple_mode.show_previous = true;
        assert!(simple_previous_toggle_enabled(1));
        assert!(simple_previous_ghosts_visible(true, false, 1));
        assert!(app.simple_previous_positions().is_some());
        app.playing = true;
        assert!(!simple_previous_ghosts_visible(true, true, 1));
        assert!(app.simple_previous_positions().is_none());
        app.playing = false;
        app.current_set = 0;
        assert!(!simple_previous_toggle_enabled(0));
        assert!(!simple_previous_ghosts_visible(true, false, 0));
        assert!(app.simple_previous_positions().is_none());
        assert!(!simple_previous_ghosts_visible(false, false, 2));
    }

    #[test]
    fn previous_ghost_mesh_is_one_batch_and_reused_for_a_hundred_dots() {
        let points: Vec<Point> = (0..100)
            .map(|index| Point {
                x: index as f32,
                y: 2.0,
            })
            .collect();
        let map = |point: Point| Pos2::new(point.x, point.y);
        let mut slot = None;
        let first = prepare_previous_ghost_mesh(&mut slot, &points, &map).expect("mesh");
        assert_eq!(first.vertices.len(), 100 * (PREVIOUS_GHOST_SEGMENTS + 1));
        assert_eq!(first.indices.len(), 100 * PREVIOUS_GHOST_SEGMENTS * 3);
        let ptr = std::sync::Arc::as_ptr(&first);
        let vertex_cap = first.vertices.capacity();
        let index_cap = first.indices.capacity();
        drop(first);
        let second = prepare_previous_ghost_mesh(&mut slot, &points, &map).expect("reused");
        assert_eq!(std::sync::Arc::as_ptr(&second), ptr);
        assert_eq!(second.vertices.capacity(), vertex_cap);
        assert_eq!(second.indices.capacity(), index_cap);
        assert_eq!(second.vertices.len(), 100 * (PREVIOUS_GHOST_SEGMENTS + 1));
    }

    #[test]
    fn previous_ghost_choice_is_remembered_per_show() {
        let dir = std::env::temp_dir().join(format!(
            "drillforge-ghost-pref-{}-{}",
            std::process::id(),
            "show"
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let draft = dir.join("simple-draft.drill.json");
        let named = dir.join("文化祭.drill.json");
        let store = dir.join("ghosts.json");
        let mut app = empty_simple_app();
        app.simple_mode.draft_path_override = Some(draft.clone());
        app.simple_mode.shows_dir_override = Some(dir.join("shows"));
        app.simple_mode.previous_store_override = Some(store.clone());
        app.sync_previous_ghost_pref();
        assert!(!app.simple_mode.show_previous);
        app.set_show_previous(true);
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        assert!(app.history.undo(&mut app.document));
        assert!(app.simple_mode.show_previous);
        assert_eq!(app.document.performers.len(), 0);

        let mut reopened = empty_simple_app();
        reopened.simple_mode.draft_path_override = Some(draft.clone());
        reopened.simple_mode.previous_store_override = Some(store.clone());
        reopened.sync_previous_ghost_pref();
        assert!(reopened.simple_mode.show_previous);

        reopened.current_path = Some(named.clone());
        reopened.simple_mode.previous_key = None;
        reopened.sync_previous_ghost_pref();
        assert!(!reopened.simple_mode.show_previous);
        reopened.set_show_previous(true);

        let mut named_again = empty_simple_app();
        named_again.simple_mode.draft_path_override = Some(draft.clone());
        named_again.simple_mode.previous_store_override = Some(store.clone());
        named_again.current_path = Some(named.clone());
        named_again.sync_previous_ghost_pref();
        assert!(named_again.simple_mode.show_previous);

        let mut draft_app = empty_simple_app();
        draft_app.simple_mode.draft_path_override = Some(draft.clone());
        draft_app.simple_mode.shows_dir_override = Some(dir.join("shows"));
        draft_app.simple_mode.previous_store_override = Some(store.clone());
        draft_app.sync_previous_ghost_pref();
        assert!(draft_app.simple_mode.show_previous);
        draft_app.current_path = Some(named.clone());
        draft_app.rekey_previous_ghost(&named);
        assert!(draft_app.simple_mode.show_previous);
        assert!(
            !draft_app
                .simple_mode
                .previous_shows
                .contains_key(&draft.to_string_lossy().into_owned())
        );
        assert_eq!(
            draft_app
                .simple_mode
                .previous_shows
                .get(&named.to_string_lossy().into_owned()),
            Some(&true)
        );

        draft_app.current_path = None;
        draft_app.simple_mode.previous_key = None;
        draft_app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        draft_app.set_show_previous(true);
        assert!(draft_app.simple_start_fresh());
        assert!(!draft_app.simple_mode.show_previous);
        assert!(
            !draft_app
                .simple_mode
                .previous_shows
                .contains_key(&draft.to_string_lossy().into_owned())
        );
        assert!(
            draft_app
                .simple_mode
                .previous_shows
                .keys()
                .any(|key| key.ends_with("下書き.drill.json"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn place_free(app: &mut DrillApp, points: &[Point]) {
        for point in points {
            app.place_performer_at(*point, false);
        }
    }

    fn field_step(app: &DrillApp) -> f32 {
        let grid = &app.document.grid;
        grid.horizontal_units / f32::from(grid.horizontal_steps.max(1))
    }

    fn near_point(a: Point, b: Point, tol: f32) -> bool {
        (a.x - b.x).hypot(a.y - b.y) <= tol
    }

    fn mirrored_pair() -> DrillApp {
        let mut app = empty_simple_app();
        // Axis of the indoor floor is 23. 10 mirrors to 36.
        place_free(
            &mut app,
            &[
                Point { x: 10.0, y: 20.0 },
                Point { x: 36.0, y: 20.0 },
                Point { x: 10.0, y: 28.0 },
                Point { x: 36.0, y: 28.0 },
            ],
        );
        app.simple_mode.mirror_moves = true;
        app
    }

    #[test]
    fn step_and_arrow_nudge_mirrors_the_partner_and_one_undo_restores_both() {
        let mut app = mirrored_pair();
        let before = app.document.sets[0].positions.clone();
        app.replace_selection(std::iter::once(0).collect());
        let cursor = app.history.cursor();
        // Arrow keys and the step buttons both call this.
        app.simple_nudge(4, 1);
        assert_eq!(app.history.cursor(), cursor + 1);
        let after = &app.document.sets[0].positions;
        let axis = app.document.grid.width * 0.5;
        let expected = drill_core::editing::mirror(std::slice::from_ref(&after[0]), axis)[0];
        assert!(
            near_point(after[1], expected, field_step(&app) * 0.5 + 0.02),
            "partner {:?} should mirror {:?} ({expected:?})",
            after[1],
            after[0]
        );
        assert!(after[0].x > before[0].x);
        assert_eq!(after[2], before[2]);
        assert_eq!(after[3], before[3]);
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.sets[0].positions, before);
        assert_eq!(app.history.cursor(), cursor);
    }

    #[test]
    fn drag_mirrors_the_partner_in_one_undo() {
        let mut app = mirrored_pair();
        let before = app.document.sets[0].positions.clone();
        app.replace_selection(std::iter::once(0).collect());
        let cursor = app.history.cursor();
        app.begin_field_drag(Pos2::new(0.0, 0.0));
        app.update_field_drag(Pos2::new(40.0, -20.0), 10.0, true);
        assert!(
            !app.simple_mode.mirror_preview.is_empty(),
            "the partner preview follows the drag"
        );
        app.commit_field_drag();
        assert_eq!(app.history.cursor(), cursor + 1);
        let after = &app.document.sets[0].positions;
        let axis = app.document.grid.width * 0.5;
        let expected = drill_core::editing::mirror(std::slice::from_ref(&after[0]), axis)[0];
        assert!(near_point(
            after[1],
            expected,
            field_step(&app) * 0.5 + 0.02
        ));
        assert_ne!(after[0], before[0]);
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.sets[0].positions, before);
    }

    #[test]
    fn line_and_arc_place_partners_on_the_mirror() {
        let mut app = mirrored_pair();
        app.replace_selection([0_usize, 2].into_iter().collect());
        let partners = drill_core::symmetry::pair_across_center(
            &app.document.sets[0].positions,
            app.document.grid.width * 0.5,
            app.simple_mirror_tolerance(),
        );
        assert_eq!(partners[0], Some(1));
        assert_eq!(partners[2], Some(3));
        app.simple_line_up();
        assert_partners_mirror(&app);
        assert!(app.history.undo(&mut app.document));
        app.simple_arc_up();
        assert_partners_mirror(&app);
    }

    fn assert_partners_mirror(app: &DrillApp) {
        let positions = &app.document.sets[0].positions;
        let axis = app.document.grid.width * 0.5;
        // 0 and 2 were the driven side. Their original partners are 1 and 3.
        for (master, partner) in [(0, 1), (2, 3)] {
            let expected =
                drill_core::editing::mirror(std::slice::from_ref(&positions[master]), axis)[0];
            assert!(
                near_point(positions[partner], expected, field_step(app) * 0.5 + 0.02),
                "partner {partner} at {:?} is not the mirror of {master} at {:?} ({expected:?})",
                positions[partner],
                positions[master]
            );
        }
    }

    #[test]
    fn unpaired_people_move_alone_and_center_people_only_move_forward() {
        let mut app = empty_simple_app();
        let axis = app.document.grid.width * 0.5;
        place_free(
            &mut app,
            &[
                Point { x: 8.0, y: 18.0 },
                Point { x: 12.0, y: 24.0 },
                Point { x: axis, y: 16.0 },
            ],
        );
        app.simple_mode.mirror_moves = true;
        let before = app.document.sets[0].positions.clone();
        app.replace_selection(std::iter::once(0).collect());
        app.simple_nudge(3, 0);
        let moved = app.document.sets[0].positions[0];
        assert!(moved.x > before[0].x);
        assert_eq!(app.document.sets[0].positions[1], before[1]);
        assert_eq!(app.document.sets[0].positions[2], before[2]);

        app.replace_selection(std::iter::once(2).collect());
        let center_before = app.document.sets[0].positions[2];
        app.simple_nudge(4, -2);
        let center = app.document.sets[0].positions[2];
        assert!(
            (center.x - axis).abs() <= field_step(&app) * 0.5 + 0.02,
            "center x {} drifted off {axis}",
            center.x
        );
        assert!(center.y < center_before.y);
        assert_eq!(app.document.sets[0].positions[0], moved);
    }

    #[test]
    fn mirror_off_moves_only_the_chosen_person() {
        let mut app = mirrored_pair();
        app.simple_mode.mirror_moves = false;
        let before = app.document.sets[0].positions.clone();
        app.replace_selection(std::iter::once(0).collect());
        app.simple_nudge(2, 0);
        let after = &app.document.sets[0].positions;
        assert_ne!(after[0], before[0]);
        assert_eq!(after[1], before[1]);
    }

    #[test]
    fn align_matches_the_other_side_and_one_undo_restores_it() {
        let mut app = empty_simple_app();
        place_free(
            &mut app,
            &[Point { x: 10.0, y: 20.0 }, Point { x: 30.0, y: 22.0 }],
        );
        app.simple_mode.mirror_moves = true;
        let before = app.document.sets[0].positions.clone();
        assert_eq!(app.simple_unmirrored_count(), 2);
        app.replace_selection(std::iter::once(0).collect());
        assert!(app.simple_selection_has_partner());
        let cursor = app.history.cursor();
        app.simple_align_partners();
        assert_eq!(app.history.cursor(), cursor + 1);
        let after = &app.document.sets[0].positions;
        let axis = app.document.grid.width * 0.5;
        let expected = drill_core::editing::mirror(std::slice::from_ref(&after[0]), axis)[0];
        assert!(near_point(
            after[1],
            expected,
            field_step(&app) * 0.5 + 0.02
        ));
        assert_eq!(app.simple_unmirrored_count(), 0);
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.sets[0].positions, before);
        assert_eq!(app.simple_unmirrored_count(), 2);
    }

    #[test]
    fn choosing_the_right_side_aligns_the_left_to_it() {
        let mut app = empty_simple_app();
        place_free(
            &mut app,
            &[Point { x: 10.0, y: 20.0 }, Point { x: 30.0, y: 24.0 }],
        );
        app.simple_mode.mirror_moves = true;
        app.replace_selection(std::iter::once(1).collect());
        app.simple_align_partners();
        let after = &app.document.sets[0].positions;
        let axis = app.document.grid.width * 0.5;
        let expected = drill_core::editing::mirror(std::slice::from_ref(&after[1]), axis)[0];
        let tol = field_step(&app) * 0.5 + 0.02;
        assert!(near_point(after[0], expected, tol));
        assert!(
            (after[1].x - 30.0).abs() <= tol && (after[1].y - 24.0).abs() <= tol,
            "the chosen side should stay put, got {:?}",
            after[1]
        );
    }

    #[test]
    fn mirror_toggle_is_not_written_into_the_file() {
        let mut app = mirrored_pair();
        let dirty = app.dirty;
        let revision = app.history.revision();
        let before = app.document.to_json().expect("json");
        app.simple_toggle_mirror();
        assert!(!app.simple_mode.mirror_moves);
        app.simple_toggle_mirror();
        assert!(app.simple_mode.mirror_moves);
        assert_eq!(app.dirty, dirty);
        assert_eq!(app.history.revision(), revision);
        let json = app.document.to_json().expect("json");
        assert_eq!(json, before);
        assert!(!json.contains("mirror_moves"));
        let dir = std::env::temp_dir().join(format!(
            "drillforge-mirror-{}-{}",
            std::process::id(),
            "toggle"
        ));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("show.drill.json");
        app.simple_mode.draft_path_override = Some(path.clone());
        assert!(app.write_simple_draft());
        let saved = std::fs::read_to_string(&path).expect("draft");
        assert!(!saved.contains("mirror_moves"));
        let loaded = drill_core::Document::from_json(&saved).expect("load");
        assert_eq!(loaded.performers.len(), app.document.performers.len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mid_count_still_blocks_mirrored_edits() {
        let mut app = mirrored_pair();
        app.replace_selection(std::iter::once(0).collect());
        app.count_position = 4.0;
        assert!(!app.is_editable_set_start());
        let before = app.document.sets[0].positions.clone();
        let cursor = app.history.cursor();
        app.simple_nudge(2, 0);
        app.simple_line_up();
        app.simple_arc_up();
        app.simple_align_partners();
        app.begin_field_drag(Pos2::new(0.0, 0.0));
        app.update_field_drag(Pos2::new(40.0, 0.0), 10.0, true);
        app.commit_field_drag();
        assert_eq!(app.document.sets[0].positions, before);
        assert_eq!(app.history.cursor(), cursor);
    }

    #[test]
    fn full_mode_symmetry_does_not_follow_the_simple_mirror_flag() {
        let mut app = mirrored_pair();
        app.simple_mode.enabled = false;
        let before = app.document.sets[0].positions.clone();
        app.replace_selection(std::iter::once(0).collect());
        app.nudge_selected(2, 0);
        assert_ne!(app.document.sets[0].positions[0], before[0]);
        assert_eq!(app.document.sets[0].positions[1], before[1]);

        app.document.sets[0].positions = before.clone();
        app.replace_selection(std::iter::once(0).collect());
        app.mirror_selection_across_field();
        assert_eq!(app.document.sets[0].positions[1], before[1]);
        let axis = app.document.grid.width * 0.5;
        let mirrored = drill_core::editing::mirror(std::slice::from_ref(&before[0]), axis)[0];
        let snapped = app.document.grid.snap(mirrored);
        assert!(near_point(
            app.document.sets[0].positions[0],
            Point {
                x: snapped.x.clamp(0.0, app.document.grid.max_x()),
                y: snapped.y.clamp(0.0, app.document.grid.max_y()),
            },
            0.05
        ));

        app.document.sets[0].positions = before;
        app.replace_selection((0..4).collect());
        let radial_before = app.document.sets[0].positions.clone();
        app.apply_radial_selection(2);
        assert_ne!(app.document.sets[0].positions, radial_before);
    }
    #[test]
    fn song_position_follows_pause_seek_step_and_scene_start() {
        let mut app = moved_pair();
        attach_song(&mut app, "reference.wav", 1.5);
        let tempo = app.document.tempo.clone();
        let track = |app: &DrillApp| app.document.audio.clone().unwrap();
        let global = |app: &DrillApp| {
            app.document
                .global_count(app.current_set, app.count_position)
        };
        app.navigate_to_set(0);
        app.nav_glide.settle();
        assert!((global(&app) - 0.0).abs() < 1e-3);
        assert!((app.audio_seconds_at_global(0.0).unwrap() - 1.5).abs() < 1e-3);

        app.simple_step_beat(4);
        app.nav_glide.settle();
        let at_step = global(&app);
        assert!((at_step - 4.0).abs() < 1e-3);
        let mapped = app.audio_seconds_at_global(at_step).unwrap();
        assert!((mapped - simple_song_seconds(&track(&app), &tempo, at_step)).abs() < 1e-4);
        assert!((mapped - 3.5).abs() < 1e-3);

        let context = egui::Context::default();
        app.toggle_playback(&context);
        assert!(app.playing);
        app.toggle_playback(&context);
        assert!(!app.playing);
        assert!((global(&app) - 4.0).abs() < 1e-3);
        assert!((app.audio_seconds_at_global(global(&app)).unwrap() - 3.5).abs() < 1e-3);

        app.navigate_to_set(1);
        app.nav_glide.settle();
        let scene = global(&app);
        assert!((scene - 16.0).abs() < 1e-3);
        assert!((app.audio_seconds_at_global(scene).unwrap() - 9.5).abs() < 1e-3);

        app.navigate_to_set(0);
        app.nav_glide.settle();
        assert!((global(&app) - 0.0).abs() < 1e-3);
        assert!((app.audio_seconds_at_global(0.0).unwrap() - 1.5).abs() < 1e-3);

        app.simple_mark_downbeat();
        assert!((global(&app) - 0.0).abs() < 1e-3);
        assert!((app.document.audio.as_ref().unwrap().offset_seconds - 1.5).abs() < 1e-3);
        app.navigate_to_global_count(4);
        app.nav_glide.settle();
        app.simple_mark_downbeat();
        assert!((global(&app) - 0.0).abs() < 1e-3);
        let anchored = app.document.audio.as_ref().unwrap().offset_seconds;
        assert!((anchored - 3.5).abs() < 1e-3);
        assert!((simple_song_seconds(&track(&app), &tempo, 0.0) - 3.5).abs() < 1e-3);
        assert!((simple_song_seconds(&track(&app), &tempo, 4.0) - 5.5).abs() < 1e-3);

        app.simple_toggle_slow();
        assert!(app.simple_is_slow());
        assert!(!app.simple_follows_song_clock());
        assert!(!simple_song_audible(true, false, true));
        app.toggle_playback(&context);
        assert!(app.playing);
        app.toggle_playback(&context);
        assert!(!app.playing);
        assert!(!app.document.sets.is_empty());
    }

    #[test]
    fn missing_song_and_absent_output_still_play_the_picture() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        app.duplicate_current_set();
        attach_song(&mut app, "no-such-song.mp3", 0.0);
        app.refresh_external_song();
        assert!(app.simple_mode.song_missing);
        assert!(app.document.audio.is_some());
        assert_eq!(app.document.performers.len(), 1);
        let context = egui::Context::default();
        app.toggle_playback(&context);
        assert!(app.playing);
        app.simple_step_beat(1);
        app.toggle_playback(&context);
        assert!(!app.playing);
        assert!(simple_song_extension_ok(std::path::Path::new("a.wav")));
        assert!(simple_song_extension_ok(std::path::Path::new("a.MP3")));
        assert!(!simple_song_extension_ok(std::path::Path::new("a.ogg")));
        assert_eq!(
            simple_song_name("folder/文化祭ステージ.mp3"),
            "文化祭ステージ.mp3"
        );
    }
}
