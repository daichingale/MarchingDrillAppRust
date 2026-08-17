#[path = "analytics_state.rs"]
mod analytics_state;
#[path = "app_theme.rs"]
mod app_theme;
#[path = "app_ui.rs"]
mod app_ui;
#[path = "audio_state.rs"]
mod audio_state;
#[path = "bootstrap.rs"]
mod bootstrap;
#[path = "command_palette.rs"]
mod command_palette;
#[path = "commands.rs"]
mod commands;
#[path = "controller.rs"]
mod controller;
#[path = "egui_backend.rs"]
mod egui_backend;
#[path = "export_state.rs"]
mod export_state;
#[path = "field_view.rs"]
mod field_view;
#[path = "go_to_count.rs"]
mod go_to_count;
#[path = "gpu_bridge.rs"]
mod gpu_bridge;
#[path = "i18n.rs"]
mod i18n;
#[path = "import_state.rs"]
mod import_state;
#[path = "inspector_media.rs"]
mod inspector_media;
#[path = "legal_notices.rs"]
mod legal_notices;
#[path = "mobile_viewer_state.rs"]
mod mobile_viewer_state;
#[path = "onboarding.rs"]
mod onboarding;
#[path = "perf_hud.rs"]
mod perf_hud;
#[path = "plugin_state.rs"]
mod plugin_state;
#[path = "print_state.rs"]
mod print_state;
#[path = "production_markers_panel.rs"]
mod production_markers_panel;
#[path = "production_sheet_workspace.rs"]
mod production_sheet_workspace;
#[path = "project_state.rs"]
mod project_state;
#[path = "recent_projects.rs"]
mod recent_projects;
#[path = "section_manager.rs"]
mod section_manager;
#[path = "set_navigator.rs"]
mod set_navigator;
#[path = "simple_mode.rs"]
mod simple_mode;
#[path = "stadium_inspector.rs"]
mod stadium_inspector;
#[path = "subset_snapshot_state.rs"]
mod subset_snapshot_state;
#[path = "text_export_state.rs"]
mod text_export_state;
#[path = "timeline.rs"]
mod timeline;
#[path = "presence_state.rs"]
mod presence_state;
#[cfg(test)]
#[path = "ui_qa.rs"]
mod ui_qa;
#[path = "underlay_state.rs"]
mod underlay_state;
#[path = "update_state.rs"]
mod update_state;
#[path = "workspace_inspector.rs"]
mod workspace_inspector;

use commands::{Command as UiCommand, Menu as CommandMenu};
use drill_core::Locale;
use drill_core::route_suggestions::{
    RouteSuggestion, SuggestionConstraints, SuggestionLimits, SuggestionReason, suggest_routes,
};
use drill_core::transition::SetCounts;
use drill_core::video::{ExportPreset, VideoExportConfig};
use drill_core::{
    Document, Edit, GridConfig, GridLine, GridStyle, History, PerformerId, Point, Set, Unit,
    camera::Camera, clinic, continuity, coordinates, editing, evenly_spaced_arc,
    evenly_spaced_line, pathing, shapes,
};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use i18n::{Text, text};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use timeline::{TimelineViewport, adjacent_production_marker_count, draw_count_track};

#[inline]
pub(crate) fn tr(locale: Locale, japanese: &'static str, english: &'static str) -> &'static str {
    match locale {
        Locale::Ja => japanese,
        Locale::En => english,
    }
}

/// A range endpoint expressed in the terms a drill writer uses at rehearsal,
/// rather than as an opaque global timeline number. `end` is deliberately the
/// first count *not* played: playback uses the half-open interval [start, end).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlaybackRangeSummary {
    pub(crate) start_set: String,
    pub(crate) start_count: u32,
    pub(crate) end_set: String,
    pub(crate) end_count: u32,
    pub(crate) length: u32,
}

pub(crate) fn playback_range_summary(
    document: &Document,
    playback_start: u32,
    playback_end: u32,
) -> PlaybackRangeSummary {
    let total = document.timeline_counts();
    let start = playback_start.min(total);
    let end = playback_end.clamp(start, total);
    let endpoint = |global| {
        let (set_index, local_count) = document.locate_count(global as f32);
        let name = document
            .sets
            .get(set_index)
            .map(|set| set.name.clone())
            .unwrap_or_default();
        // Counts are presented to people as one-based positions. At a set
        // boundary the exclusive OUT endpoint therefore reads "next set · 1".
        (name, local_count as u32 + 1)
    };
    let (start_set, start_count) = endpoint(start);
    let (end_set, end_count) = endpoint(end);
    PlaybackRangeSummary {
        start_set,
        start_count,
        end_set,
        end_count,
        length: end.saturating_sub(start),
    }
}

#[cfg(test)]
mod playback_range_summary_tests {
    use super::*;

    #[test]
    fn summary_uses_named_one_based_set_endpoints_and_an_exclusive_out() {
        let mut document = Document::demo(1, 3);
        document.sets[0].name = "Opening".into();
        document.sets[1].name = "Impact".into();
        document.sets[0].counts = 8;
        document.sets[1].counts = 12;

        let summary = playback_range_summary(&document, 3, 8);

        assert_eq!(summary.start_set, "Opening");
        assert_eq!(summary.start_count, 4);
        assert_eq!(summary.end_set, "Impact");
        assert_eq!(summary.end_count, 1);
        assert_eq!(summary.length, 5);
    }

    #[test]
    fn summary_clamps_endpoints_to_the_integer_timeline_contract() {
        let document = Document::demo(1, 2);
        let total = document.timeline_counts();
        let summary = playback_range_summary(&document, total + 9, total + 20);

        assert_eq!(summary.length, 0);
        assert_eq!(summary.end_count, 1);
    }
}

#[cfg(test)]
mod comparison_session_tests {
    use super::*;

    #[test]
    fn comparison_reference_is_session_only_and_never_changes_document_history() {
        let mut app = DrillApp::default();
        let before = app.document.clone();
        let revision = app.history.revision();

        app.set_comparison = Some(SetComparison {
            reference_set: 1,
            show_paths: true,
        });
        app.set_comparison = None;

        assert_eq!(app.document, before);
        assert_eq!(app.history.revision(), revision);
        assert!(!app.dirty);
    }
}

#[cfg(test)]
mod workspace_preset_tests {
    use super::*;

    #[test]
    fn workspace_presets_are_session_only_and_apply_a_predictable_working_surface() {
        let mut app = DrillApp::default();
        let document = app.document.clone();
        let revision = app.history.revision();

        app.apply_workspace_preset(WorkspacePreset::Review);
        assert_eq!(app.workspace_preset, WorkspacePreset::Review);
        assert_eq!(app.view_mode, ViewMode::Field2D);
        assert!(app.show_inspector);
        assert!(app.heatmap_enabled);
        assert_eq!(app.workspace_focus, Some(WorkspaceFocus::Clinic));

        app.apply_workspace_preset(WorkspacePreset::Present);
        assert_eq!(app.workspace_preset, WorkspacePreset::Present);
        assert_eq!(app.view_mode, ViewMode::Stadium3D);
        assert!(!app.show_inspector);
        assert!(!app.heatmap_enabled);
        assert_eq!(app.workspace_focus, None);
        assert_eq!(app.document, document);
        assert_eq!(app.history.revision(), revision);
        assert!(!app.dirty);
    }
}

#[cfg(test)]
mod focus_field_tests {
    use super::*;

    #[test]
    fn focus_field_is_session_only_and_keeps_playback_state_intact() {
        let context = eframe::egui::Context::default();
        let mut app = DrillApp::default();
        let document = app.document.clone();
        let revision = app.history.revision();
        app.current_set = 1;
        app.count_position = 3.0;
        app.playing = true;

        app.execute_command(commands::Command::ToggleFocusField, &context);
        assert!(app.focus_field);
        assert!(app.playing);
        assert_eq!(app.current_set, 1);
        assert_eq!(app.count_position, 3.0);
        assert_eq!(app.document, document);
        assert_eq!(app.history.revision(), revision);
        assert!(!app.dirty);

        app.execute_command(commands::Command::ToggleFocusField, &context);
        assert!(!app.focus_field);
        assert!(app.playing);
    }
}

#[cfg(test)]
mod document_open_guard_tests {
    use super::*;

    #[test]
    fn dirty_document_defers_open_until_the_writer_chooses() {
        let mut app = DrillApp::default();
        let before = app.document.clone();
        let revision = app.history.revision();
        app.dirty = true;

        app.request_open_document(DocumentOpenKind::Project);

        assert_eq!(
            app.document_open_guard,
            DocumentOpenGuard::Prompt(DocumentOpenTarget::Dialog(DocumentOpenKind::Project))
        );
        assert_eq!(app.document, before);
        assert_eq!(app.history.revision(), revision);
        assert!(app.dirty);
    }
}

pub(crate) fn run() -> eframe::Result {
    bootstrap::run()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ViewMode {
    Field2D,
    Stadium3D,
}

/// A familiar, task-oriented arrangement of the working surface.  This is
/// intentionally session state: switching between writing, checking, and
/// presenting a show must never create an edit, affect exports, or alter undo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum WorkspacePreset {
    #[default]
    Design,
    Review,
    Present,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkspaceFocus {
    Performer,
    Clinic,
    Grid,
    Tempo,
    Video,
    Audio,
    ProductionSheet,
}

/// A session-only A/B reference for judging a form without changing the show.
/// It deliberately holds only a set index: the source dots remain canonical in
/// `Document`, so opening, changing, or closing comparison cannot affect save
/// state or undo history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SetComparison {
    pub(crate) reference_set: usize,
    pub(crate) show_paths: bool,
}

/// A deliberately small, session-only formation clipboard.  The first slice
/// only pastes onto the same stable performer IDs: this makes Copy/Paste a
/// reliable way to recover a known picture without pretending that a guessed
/// correspondence between two groups is safe.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct FormationClipboard {
    entries: Vec<(PerformerId, Point)>,
}

fn clipboard_centre(entries: &[(PerformerId, Point)]) -> Point {
    let count = entries.len() as f32;
    Point {
        x: entries.iter().map(|(_, point)| point.x).sum::<f32>() / count,
        y: entries.iter().map(|(_, point)| point.y).sum::<f32>() / count,
    }
}

/// A field-order sort makes target correspondence repeatable regardless of
/// which order the user shift-clicked the performers. Rows are ordered first,
/// then files, which reads naturally in the conventional drill field view.
fn point_field_order(left: &Point, right: &Point) -> std::cmp::Ordering {
    left.y
        .total_cmp(&right.y)
        .then_with(|| left.x.total_cmp(&right.x))
}

/// Exit is deliberately a small state machine instead of a boolean. A native
/// save is asynchronous, so the app must keep the close request cancelled
/// until `ProjectEvent::Saved` confirms the bytes reached disk.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CloseGuard {
    #[default]
    Idle,
    Prompt,
    Saving,
}

/// The document picker is deferred until the writer has decided what to do
/// with their current edits, matching the native Save / Don't Save / Cancel
/// flow instead of replacing work as soon as Cmd+O is pressed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum DocumentOpenGuard {
    #[default]
    Idle,
    Prompt(DocumentOpenTarget),
    Saving(DocumentOpenTarget),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DocumentOpenKind {
    LegacyJson,
    Project,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DocumentOpenTarget {
    Dialog(DocumentOpenKind),
    Recent(PathBuf),
}

/// A deliberately session-only draft for timing changes.  Count changes can
/// alter every later global count, so the inspector never writes while a
/// slider is being dragged; the writer sees the exact shift first, then makes
/// one normal undoable `Edit::SetCounts` decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SetCountDraft {
    pub(crate) set_id: drill_core::SetId,
    pub(crate) moves: u16,
}

/// The result of a Knife cut, kept only long enough for the writer to pick a
/// side. Session-only: drawing, inverting, or dismissing a cut never records
/// an `Edit`; the resulting selection change goes through the same direct
/// field mutation as every other selection change in this file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KnifeSplit {
    /// Performers where the cut line's cross product is `>= 0`.
    pub(crate) side_a: BTreeSet<usize>,
    /// Performers on the other side of the cut line.
    pub(crate) side_b: BTreeSet<usize>,
    /// True when `side_a` is the currently active selection.
    pub(crate) active_side_a: bool,
}

/// A short, purely *visual* easing of the playhead toward a navigation
/// target, so a jump between sets reads as a move the eye can follow instead
/// of a cut to an unrelated picture.
///
/// This deliberately holds no authority over anything. `current_set` and
/// `count_position` are still assigned synchronously by the navigation
/// functions, so every headless caller -- including the ui_qa acceptance
/// tests, which call `navigate_to_set` and assert on the destination on the
/// very next line -- observes the same state it always has. This struct only
/// answers "where should the renderer draw the playhead *this* frame", in
/// global count space, and is never read by the document, the undo history,
/// or the audio clock.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct NavGlide {
    active: Option<Glide>,
}

#[derive(Clone, Copy, Debug)]
struct Glide {
    /// Global count the glide departs from.
    from: f32,
    /// Global count it converges to; always the already-committed logical
    /// position.
    to: f32,
    elapsed: f32,
    duration: f32,
}

impl NavGlide {
    /// Below this the jump is already within a set's own visual noise, and
    /// easing it just adds latency to something that read as instant anyway.
    const MIN_DISTANCE_COUNTS: f32 = 0.75;
    /// Floor and ceiling on the glide length. A drill writer triggers set
    /// navigation thousands of times a session, so this budget is about
    /// legibility, not spectacle: long enough to show direction, short enough
    /// that it can never be what the user is waiting on.
    const MIN_DURATION: f32 = 0.10;
    const MAX_DURATION: f32 = 0.20;
    /// Longer jumps get slightly longer glides, so crossing the whole show
    /// does not have to move at an absurd apparent speed to fit the floor.
    const SECONDS_PER_COUNT: f32 = 0.0016;

    /// Starts (or redirects) a glide arriving at `to_global`.
    ///
    /// Call this *before* committing the logical seek, passing the position
    /// being left. If a glide is already in flight the new one departs from
    /// wherever the eye currently is rather than from the stale original
    /// origin, so rapid next/next/next never snaps backwards.
    fn begin(&mut self, from_global: f32, to_global: f32) {
        let from = self.position().unwrap_or(from_global);
        let distance = (to_global - from).abs();
        if !distance.is_finite() || distance < Self::MIN_DISTANCE_COUNTS {
            self.active = None;
            return;
        }
        self.active = Some(Glide {
            from,
            to: to_global,
            elapsed: 0.0,
            duration: (Self::MIN_DURATION + distance * Self::SECONDS_PER_COUNT)
                .min(Self::MAX_DURATION),
        });
    }

    /// Abandons any glide in flight, so the next frame draws the logical
    /// position exactly.
    fn settle(&mut self) {
        self.active = None;
    }

    /// The global count to draw, or `None` when settled (draw the logical
    /// position).
    fn position(&self) -> Option<f32> {
        self.active.map(|glide| {
            let t = (glide.elapsed / glide.duration).clamp(0.0, 1.0);
            // Cubic ease-out: leaves immediately -- so the jump still feels
            // like a direct response to the keystroke -- and decelerates into
            // the target, which is the part that tells the eye where to stop
            // looking.
            let eased = 1.0 - (1.0 - t).powi(3);
            glide.from + (glide.to - glide.from) * eased
        })
    }

    /// Advances by `dt` and reports whether a glide is still running. Time is
    /// accumulated rather than the position being stepped by a per-frame
    /// fraction, so the curve is identical at 60, 144 and 240Hz by
    /// construction.
    fn advance(&mut self, dt: f32) -> bool {
        let Some(glide) = self.active.as_mut() else {
            return false;
        };
        glide.elapsed += dt.max(0.0);
        if glide.elapsed >= glide.duration {
            // Settling by dropping the glide (rather than pinning it at t=1)
            // means the renderer falls back to the logical position exactly,
            // with no residual float error, and the app goes idle.
            self.active = None;
            return false;
        }
        true
    }
}

pub(crate) struct DrillApp {
    document: Document,
    view_mode: ViewMode,
    workspace_preset: WorkspacePreset,
    camera: Camera,
    /// Desktop 2D pan/zoom is session UI, never a document edit.
    field_viewport: field_view::FieldViewport,
    camera_program_preview: bool,
    beats_per_measure: u16,
    current_set: usize,
    count_position: f32,
    playing: bool,
    speed: f32,
    tempo_bpm: f32,
    playback_start: u32,
    playback_end: u32,
    loop_playback: bool,
    last_frame: Instant,
    frame_positions: Vec<Point>,
    audio_state: audio_state::AudioState,
    click_settings: drill_audio::ClickSettings,
    display_list: drill_render::DisplayList,
    render_scratch: drill_render::BuildScratch,
    /// Reused across frames so the collision/stride clinic never reallocates
    /// its spatial-hash buffers in the UI hot path (see `analyze_transition`'s
    /// doc comment: it makes a fresh `ScanScratch` per call, which is fine for
    /// one-off callers but wrong for something drawn every frame).
    clinic_scratch: clinic::ScanScratch,
    selected: BTreeSet<usize>,
    /// Per-performer presentation and interaction filters.  These intentionally
    /// use stable IDs and live outside `Document`: a writer can focus a
    /// rehearsal working group without changing a file, export, or undo stack.
    locked_performers: BTreeSet<PerformerId>,
    hidden_performers: BTreeSet<PerformerId>,
    /// Latest session-only filter action, kept for one-click recovery.
    last_filtered_performers: BTreeSet<PerformerId>,
    /// Last Real View diagnostic target. This is a stable ID because diagnostic
    /// focus is navigation state, never part of the saved production.
    visibility_focus: Option<PerformerId>,
    command_palette: command_palette::CommandPalette,
    /// A compact, keyboard-first jump surface for navigating a long show.
    /// This is session UI only: opening or searching it never edits the drill.
    set_navigator: set_navigator::SetNavigator,
    /// Modal, accessible exact-count navigation. Kept out of `Document` so
    /// opening it never creates an edit or changes undo history.
    go_to_count: go_to_count::GoToCount,
    /// Session-only selection stack. It is deliberately outside Document and
    /// History: restoring a working group must never dirty the drill or alter
    /// an undo transaction.
    selection_stack: Vec<BTreeSet<usize>>,
    history: History,
    drag_before: Option<Vec<Point>>,
    drag_preview: Option<Vec<Point>>,
    drag_origin: Option<Pos2>,
    marquee_origin: Option<Pos2>,
    current_path: Option<PathBuf>,
    dirty: bool,
    close_guard: CloseGuard,
    document_open_guard: DocumentOpenGuard,
    recent_projects: recent_projects::RecentProjects,
    show_recent_projects: bool,
    status: String,
    last_autosave: Instant,
    show_guidance: bool,
    /// Keeps the document canvas spacious during hands-on design. A focused
    /// workspace command always reopens this inspector so no command can lead
    /// to a hidden destination.
    show_inspector: bool,
    /// A session-only canvas-first arrangement. It deliberately changes no
    /// document state and does not replace the user's workspace preference.
    focus_field: bool,
    video_export: VideoExportConfig,
    video_preset: ExportPreset,
    video_advanced: bool,
    export_state: export_state::ExportState,
    project_state: project_state::ProjectState,
    embed_audio_in_project: bool,
    project_warnings: Vec<drill_project::container::LoadWarning>,
    section_manager: section_manager::SectionManager,
    timeline_view: TimelineViewport,
    timeline_follow: bool,
    /// Where playback follow wants `timeline_view.start` to end up, in counts.
    /// `None` when the viewport is already there. Purely a view concern, like
    /// `timeline_view` itself.
    timeline_follow_glide: Option<f32>,
    /// Visual-only easing of the playhead after a navigation jump. See
    /// [`NavGlide`]: the logical playhead has already arrived.
    nav_glide: NavGlide,
    onboarding: onboarding::OnboardingState,
    app_theme: app_theme::AppTheme,
    /// Runs the brief dissolve between color themes; idle otherwise.
    theme_fade: app_theme::ThemeFade,
    simple_mode: simple_mode::SimpleModeState,
    ever_played: bool,
    locale: Locale,
    crash_notice_dismissed: bool,
    print_state: print_state::PrintState,
    mobile_viewer_state: mobile_viewer_state::MobileViewerState,
    workspace_focus: Option<WorkspaceFocus>,
    show_legal_notices: bool,
    update_state: update_state::UpdateState,
    import_state: import_state::ImportState,
    /// Live collaborator presence. View-only by construction: this can draw
    /// who is looking at what, and can never mutate the document.
    presence: presence_state::PresenceState,
    gpu: Option<gpu_bridge::Bridge>,
    /// Toggleable frame-pacing overlay. Session-only diagnostics: never part
    /// of the document, never touches undo history.
    perf_hud: perf_hud::PerfHud,
    plugin_state: plugin_state::PluginUiState,
    text_export_state: text_export_state::TextExportState,
    subset_snapshot_state: subset_snapshot_state::SubsetSnapshotState,
    route_suggestions: Vec<RouteSuggestion>,
    route_suggestion_selected: usize,
    production_markers_panel: production_markers_panel::ProductionMarkersPanel,
    production_sheet_workspace: production_sheet_workspace::ProductionSheetWorkspace,
    grid_draft: Option<GridConfig>,
    grid_draft_dirty: bool,
    tempo_draft: Option<drill_core::tempo::TempoMap>,
    tempo_draft_dirty: bool,
    audio_draft: Option<drill_core::audio::AudioTrack>,
    audio_draft_dirty: bool,
    stadium_inspector: stadium_inspector::StadiumInspector,
    formation_preview_spec: Option<shapes::ShapeSpec>,
    formation_preview_points: Vec<Point>,
    free_draw_active: bool,
    free_draw_raw: Vec<Point>,
    /// True while a Knife drag across the field is being captured. Mirrors
    /// `marquee_origin`'s exclusivity with click-select, drag-move, marquee,
    /// and free-draw so the field view never interprets a cut drag as one of
    /// those instead.
    knife_active: bool,
    /// Screen-space origin of the in-progress Knife drag.
    knife_origin: Option<Pos2>,
    /// The most recent completed cut. Kept so the toolbar can offer
    /// "invert" without redrawing the line; cleared by a new cut, an
    /// explicit dismiss, or Escape.
    knife_result: Option<KnifeSplit>,
    formation_text: String,
    /// Number of intermediate Sets the "Follow the Leader" tool generates
    /// between the current set and the next one. Session UI state only,
    /// deliberately kept out of the document/undo history like
    /// `formation_text`.
    follow_leader_steps: u32,
    underlay_state: underlay_state::UnderlayState,
    /// Revision-gated background analytics; no heavy analysis executes in an
    /// egui frame callback.
    analytics_state: analytics_state::AnalyticsState,
    /// Whether the "Show DNA" field-usage heatmap overlay is drawn. The
    /// underlying `FieldOccupancy` is computed by `analytics_state` only
    /// while this is on, so leaving it off costs nothing per frame.
    heatmap_enabled: bool,
    /// Which performers' movement trails to overlay on the field view; see
    /// `drill_render::TrailSelection`. Trails are cheap to resample (backed
    /// by a warm thread-local scratch buffer in `drill_render`), so unlike
    /// the three caches above this is not cached -- it is recomputed from
    /// `frame_positions`-adjacent state every frame it is visible.
    trail_selection: drill_render::TrailSelection,
    /// Optional translucent reference form for A/B visual checks.
    set_comparison: Option<SetComparison>,
    set_count_draft: Option<SetCountDraft>,
    formation_clipboard: FormationClipboard,
    /// Pending positions, keyed by stable performer ID.  Like shape previews,
    /// this never changes the document until the writer explicitly applies it.
    clipboard_paste_preview: Option<Vec<(PerformerId, Point)>>,
    /// True when a copied shape is being transplanted onto an explicitly
    /// selected, equally-sized group rather than restored to its source IDs.
    /// This is session state, deliberately kept out of undo and the document.
    clipboard_paste_targets_selection: bool,
}

impl Default for DrillApp {
    fn default() -> Self {
        let mut document = Document::demo(8, 10);
        // `Document::demo`'s block/arc formations are authored against the
        // default 100x53.333yd football grid. Swap in the standard Japanese
        // floor-drill footprint (30m square, All-Japan Marching Contest) and
        // let `replace_grid` proportionally rescale performer positions so
        // nobody starts off-field. The camera program is grid-dependent too
        // (it frames the field), so it's rebuilt after the swap rather than
        // reused from `demo`.
        document.replace_grid(GridConfig::japan_floor(), true);
        // Proportional rescaling lands positions near, but not exactly on,
        // the new grid's snap lattice (its step size doesn't evenly divide
        // the old one's). Re-snap so the demo starts fully on-grid, matching
        // what a real snapped edit would produce.
        let grid = document.grid.clone();
        for set in &mut document.sets {
            for point in &mut set.positions {
                *point = grid.snap(*point);
            }
        }
        document.camera_program = drill_core::camera::CameraProgram::default_for_grid(&document.grid);
        let playback_end = document.timeline_counts();
        let camera = Camera::press_box(&document.grid);
        Self {
            frame_positions: Vec::with_capacity(document.performers.len()),
            audio_state: audio_state::AudioState::default(),
            click_settings: drill_audio::ClickSettings::default(),
            display_list: drill_render::DisplayList::new(),
            render_scratch: drill_render::BuildScratch,
            clinic_scratch: clinic::ScanScratch::default(),
            view_mode: ViewMode::Field2D,
            workspace_preset: WorkspacePreset::Design,
            camera,
            field_viewport: field_view::FieldViewport::fit(&document.grid),
            camera_program_preview: true,
            beats_per_measure: 4,
            document,
            current_set: 0,
            count_position: 0.0,
            playing: false,
            speed: 1.0,
            tempo_bpm: 120.0,
            playback_start: 0,
            playback_end,
            loop_playback: false,
            last_frame: Instant::now(),
            selected: BTreeSet::new(),
            locked_performers: BTreeSet::new(),
            hidden_performers: BTreeSet::new(),
            last_filtered_performers: BTreeSet::new(),
            visibility_focus: None,
            command_palette: command_palette::CommandPalette::default(),
            set_navigator: set_navigator::SetNavigator::default(),
            go_to_count: go_to_count::GoToCount::default(),
            selection_stack: Vec::new(),
            history: History::with_limit(500),
            drag_before: None,
            drag_preview: None,
            drag_origin: None,
            marquee_origin: None,
            current_path: None,
            dirty: false,
            close_guard: CloseGuard::Idle,
            document_open_guard: DocumentOpenGuard::Idle,
            recent_projects: recent_projects::RecentProjects::load(),
            show_recent_projects: false,
            status: text(Locale::Ja, Text::Ready).into(),
            last_autosave: Instant::now(),
            show_guidance: true,
            show_inspector: true,
            focus_field: false,
            video_export: VideoExportConfig::default(),
            video_preset: ExportPreset::Standard,
            video_advanced: false,
            export_state: export_state::ExportState::default(),
            project_state: project_state::ProjectState::new(),
            embed_audio_in_project: true,
            project_warnings: Vec::new(),
            section_manager: section_manager::SectionManager::default(),
            timeline_view: TimelineViewport::fit(playback_end),
            timeline_follow: true,
            timeline_follow_glide: None,
            nav_glide: NavGlide::default(),
            onboarding: onboarding::OnboardingState::default(),
            app_theme: app_theme::AppTheme::default(),
            theme_fade: app_theme::ThemeFade::default(),
            simple_mode: simple_mode::SimpleModeState::default(),
            ever_played: false,
            locale: Locale::Ja,
            crash_notice_dismissed: false,
            print_state: print_state::PrintState::default(),
            mobile_viewer_state: mobile_viewer_state::MobileViewerState::default(),
            workspace_focus: None,
            show_legal_notices: false,
            update_state: update_state::UpdateState::default(),
            import_state: import_state::ImportState::default(),
            presence: presence_state::PresenceState::default(),
            gpu: None,
            perf_hud: perf_hud::PerfHud::default(),
            plugin_state: plugin_state::PluginUiState::default(),
            text_export_state: text_export_state::TextExportState::default(),
            subset_snapshot_state: subset_snapshot_state::SubsetSnapshotState::default(),
            route_suggestions: Vec::new(),
            route_suggestion_selected: 0,
            production_markers_panel: production_markers_panel::ProductionMarkersPanel::default(),
            production_sheet_workspace:
                production_sheet_workspace::ProductionSheetWorkspace::default(),
            grid_draft: None,
            grid_draft_dirty: false,
            tempo_draft: None,
            tempo_draft_dirty: false,
            audio_draft: None,
            audio_draft_dirty: false,
            stadium_inspector: stadium_inspector::StadiumInspector::default(),
            formation_preview_spec: None,
            formation_preview_points: Vec::new(),
            free_draw_active: false,
            free_draw_raw: Vec::with_capacity(512),
            knife_active: false,
            knife_origin: None,
            knife_result: None,
            formation_text: "DRILL".into(),
            follow_leader_steps: 6,
            underlay_state: underlay_state::UnderlayState::default(),
            analytics_state: analytics_state::AnalyticsState::default(),
            heatmap_enabled: false,
            trail_selection: drill_render::TrailSelection::None,
            set_comparison: None,
            set_count_draft: None,
            formation_clipboard: FormationClipboard::default(),
            clipboard_paste_preview: None,
            clipboard_paste_targets_selection: false,
        }
    }
}

impl DrillApp {
    const MAX_SELECTION_HISTORY: usize = 10;

    fn begin_set_count_draft(&mut self) {
        let set = &self.document.sets[self.current_set];
        self.set_count_draft = Some(SetCountDraft {
            set_id: set.id,
            moves: set.counts,
        });
    }

    fn discard_set_count_draft(&mut self) {
        self.set_count_draft = None;
    }

    fn apply_set_count_draft(&mut self) {
        let Some(draft) = self.set_count_draft else {
            return;
        };
        let Some(index) = self
            .document
            .sets
            .iter()
            .position(|set| set.id == draft.set_id)
        else {
            self.set_count_draft = None;
            return;
        };
        let set = &self.document.sets[index];
        if draft.moves == set.counts {
            self.set_count_draft = None;
            return;
        }
        let previous_total = self.document.timeline_counts();
        let was_following_whole_show = self.playback_end >= previous_total;
        let edit = Edit::SetCounts {
            set_id: draft.set_id,
            counts: SetCounts {
                moves: draft.moves,
                hold: set.hold,
            },
        };
        if self.execute_edit(edit, i18n::registered(self.locale, "count-adjust.008")) {
            let new_total = self.document.timeline_counts();
            self.playback_end = if was_following_whole_show {
                new_total
            } else {
                self.playback_end.min(new_total)
            };
            self.playback_start = self.playback_start.min(self.playback_end.saturating_sub(1));
            self.timeline_view.normalize(self.playback_end);
            self.set_count_draft = None;
            self.status = i18n::registered(self.locale, "count-adjust.009").into();
        }
    }

    fn remember_selection(&mut self) {
        if self.selected.is_empty() || self.selection_stack.last() == Some(&self.selected) {
            return;
        }
        self.selection_stack.push(self.selected.clone());
        if self.selection_stack.len() > Self::MAX_SELECTION_HISTORY {
            self.selection_stack.remove(0);
        }
    }

    fn clear_selection(&mut self) {
        self.remember_selection();
        self.selected.clear();
    }

    fn replace_selection(&mut self, selection: BTreeSet<usize>) {
        let selection = selection
            .into_iter()
            .filter(|&index| self.is_selectable_index(index))
            .collect();
        if self.selected != selection {
            self.remember_selection();
            self.selected = selection;
        }
    }

    fn can_restore_selection(&self) -> bool {
        !self.selection_stack.is_empty()
    }

    fn restore_recent_selection(&mut self) {
        let Some(previous) = self.selection_stack.pop() else {
            return;
        };
        // Filters are session state too.  A saved selection may predate a
        // temporary lock/hide action, so restoring it must not resurrect
        // performers that the writer deliberately took out of interaction.
        self.selected = previous
            .into_iter()
            .filter(|&index| self.is_selectable_index(index))
            .collect();
    }

    /// Restores an entry from the session-only working-group history.
    /// `recency` is zero for the most recent group. The current selection is
    /// retained as a history entry, allowing quick A/B group switching.
    fn restore_selection_history_at(&mut self, recency: usize) {
        let Some(index) = self
            .selection_stack
            .len()
            .checked_sub(recency.saturating_add(1))
        else {
            return;
        };
        let previous = self.selection_stack.remove(index);
        self.remember_selection();
        self.selected = previous
            .into_iter()
            .filter(|&index| self.is_selectable_index(index))
            .collect();
    }

    fn selection_history_label(&self, selection: &BTreeSet<usize>) -> String {
        let mut labels = selection
            .iter()
            .filter_map(|&index| self.document.performers.get(index))
            .map(|performer| performer.label.as_str());
        let Some(first) = labels.next() else {
            return "0".to_owned();
        };
        let remaining = labels.count();
        if remaining == 0 {
            format!("1 · {first}")
        } else {
            format!("{} · {first} +{remaining}", remaining + 1)
        }
    }

    /// Performer indexes are document-local. A document replacement must also
    /// discard the session-only restore stack, otherwise Restore Previous
    /// Selection could target unrelated performers in the new production.
    fn reset_selection_for_document(&mut self) {
        self.selected.clear();
        self.selection_stack.clear();
        self.locked_performers.clear();
        self.hidden_performers.clear();
        self.last_filtered_performers.clear();
        self.visibility_focus = None;
        self.cancel_knife();
    }

    /// Splits `base` into the two sides of the straight line from `start` to
    /// `end`, using a 2D cross-product half-plane test against each
    /// performer's CURRENT position. A point exactly on the line (`cross ==
    /// 0.0`) is treated as `side_a`. Returns `None` for a near-zero-length
    /// line (almost always an accidental click rather than an intended cut)
    /// or when `base` yields no on-document indexes.
    fn knife_cut(&self, start: Point, end: Point, base: impl Iterator<Item = usize>) -> Option<KnifeSplit> {
        let dx = end.x - start.x;
        let dy = end.y - start.y;
        if dx.hypot(dy) < 1e-4 {
            return None;
        }
        let positions = &self.document.sets[self.current_set].positions;
        let mut side_a = BTreeSet::new();
        let mut side_b = BTreeSet::new();
        for index in base {
            let Some(point) = positions.get(index) else {
                continue;
            };
            let cross = dx * (point.y - start.y) - dy * (point.x - start.x);
            if cross >= 0.0 {
                side_a.insert(index);
            } else {
                side_b.insert(index);
            }
        }
        if side_a.is_empty() && side_b.is_empty() {
            return None;
        }
        // Default to whichever side holds the lower-indexed performer: a
        // deterministic, predictable anchor the writer can flip with one
        // click rather than a coin toss.
        let active_side_a = match (side_a.iter().next(), side_b.iter().next()) {
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (Some(a), Some(b)) => a <= b,
            (None, None) => unreachable!("guarded above: both sides are non-empty"),
        };
        Some(KnifeSplit {
            side_a,
            side_b,
            active_side_a,
        })
    }

    /// Starts a Knife drag. Any pending result from a previous cut is
    /// discarded: a fresh drag begins a fresh decision. Any other exclusive
    /// canvas mode (a proposed formation, an in-progress free draw, or a
    /// pending clipboard paste) is cancelled first so the field view never
    /// has two competing pending actions at once.
    fn begin_knife(&mut self) {
        self.cancel_shape_preview();
        self.cancel_clipboard_paste_preview();
        self.knife_active = true;
        self.knife_origin = None;
        self.knife_result = None;
        self.status = i18n::registered(self.locale, "app-state.156").into();
    }

    /// Cancels an in-progress drag and/or dismisses a shown result. Used by
    /// the toolbar's Cancel/Close buttons and by Escape.
    fn cancel_knife(&mut self) {
        self.knife_active = false;
        self.knife_origin = None;
        self.knife_result = None;
    }

    /// Finishes a Knife drag: splits the working selection (or, if nothing
    /// is selected, the entire cast) by the line from `start` to `end`, and
    /// applies the default side as the new selection. The pre-cut selection
    /// is preserved on the session-only selection stack via
    /// `replace_selection`, exactly like any other selection change, so it
    /// remains one click away via Restore Previous Selection.
    fn apply_knife_cut(&mut self, start: Point, end: Point) {
        self.knife_active = false;
        self.knife_origin = None;
        let base: Box<dyn Iterator<Item = usize> + '_> = if self.selected.is_empty() {
            Box::new(0..self.document.performers.len())
        } else {
            Box::new(self.selected.iter().copied())
        };
        let Some(split) = self.knife_cut(start, end, base) else {
            self.knife_result = None;
            self.status = i18n::registered(self.locale, "app-state.157").into();
            return;
        };
        let active = if split.active_side_a {
            split.side_a.clone()
        } else {
            split.side_b.clone()
        };
        self.replace_selection(active);
        self.knife_result = Some(split);
        self.status = i18n::registered(self.locale, "app-state.158").into();
    }

    /// Swaps which side of the last cut is active. This does not touch the
    /// selection-history stack: the pre-cut selection was already
    /// remembered once, in `apply_knife_cut`, and flipping sides mid-decision
    /// is not itself a new selection worth remembering.
    fn invert_knife_side(&mut self) {
        let Some(result) = &mut self.knife_result else {
            return;
        };
        result.active_side_a = !result.active_side_a;
        let next = if result.active_side_a {
            result.side_a.clone()
        } else {
            result.side_b.clone()
        };
        self.selected = next
            .into_iter()
            .filter(|&index| self.is_selectable_index(index))
            .collect();
    }

    /// Merges the selection at `recency` (0 = most recent) into the current
    /// working selection and removes it from the history stack --
    /// consistent with `restore_selection_history_at`: pulling a past group
    /// back out consumes it rather than leaving a stale duplicate behind.
    fn glue_merge_one(&mut self, recency: usize) {
        let Some(index) = self
            .selection_stack
            .len()
            .checked_sub(recency.saturating_add(1))
        else {
            return;
        };
        let source = self.selection_stack.remove(index);
        let mut merged = self.selected.clone();
        merged.extend(source.iter().copied());
        let merged: BTreeSet<usize> = merged
            .into_iter()
            .filter(|&index| self.is_selectable_index(index))
            .collect();
        if merged != self.selected {
            self.remember_selection();
            self.selected = merged;
        }
    }

    /// Merges the current selection with the last `count` entries on the
    /// history stack in one action -- the "combine all shown" Glue
    /// shortcut. Merged performer indexes are deduplicated by construction
    /// (`self.selected` and `selection_stack` entries are already
    /// `BTreeSet`s); there is no per-performer click order to preserve here,
    /// since `self.selected` itself carries none.
    fn glue_merge_recent(&mut self, count: usize) {
        let take = count.min(self.selection_stack.len());
        if take == 0 {
            return;
        }
        let start = self.selection_stack.len() - take;
        let drained: Vec<BTreeSet<usize>> = self.selection_stack.drain(start..).collect();
        let mut merged = self.selected.clone();
        for set in &drained {
            merged.extend(set.iter().copied());
        }
        let merged: BTreeSet<usize> = merged
            .into_iter()
            .filter(|&index| self.is_selectable_index(index))
            .collect();
        if merged != self.selected {
            self.remember_selection();
            self.selected = merged;
        }
    }

    fn is_locked_index(&self, index: usize) -> bool {
        self.document
            .performers
            .get(index)
            .is_some_and(|performer| self.locked_performers.contains(&performer.id))
    }

    fn is_hidden_index(&self, index: usize) -> bool {
        self.document
            .performers
            .get(index)
            .is_some_and(|performer| self.hidden_performers.contains(&performer.id))
    }

    fn is_selectable_index(&self, index: usize) -> bool {
        index < self.document.performers.len()
            && !self.is_locked_index(index)
            && !self.is_hidden_index(index)
    }

    fn lock_selected_performers(&mut self) {
        let ids: Vec<_> = self
            .selected
            .iter()
            .filter_map(|&index| self.document.performers.get(index).map(|p| p.id))
            .collect();
        self.last_filtered_performers = ids.iter().copied().collect();
        self.locked_performers.extend(ids);
        let performer_count = self.document.performers.len();
        let locked = &self.locked_performers;
        let hidden = &self.hidden_performers;
        let performers = &self.document.performers;
        self.selected.retain(|&index| {
            performers.get(index).is_some_and(|performer| {
                index < performer_count
                    && !locked.contains(&performer.id)
                    && !hidden.contains(&performer.id)
            })
        });
        self.status = format!(
            "{} {}",
            self.last_filtered_performers.len(),
            i18n::registered(self.locale, "app-ui.151")
        );
    }

    fn hide_selected_performers(&mut self) {
        let ids: Vec<_> = self
            .selected
            .iter()
            .filter_map(|&index| self.document.performers.get(index).map(|p| p.id))
            .collect();
        self.last_filtered_performers = ids.iter().copied().collect();
        self.hidden_performers.extend(ids);
        let performer_count = self.document.performers.len();
        let locked = &self.locked_performers;
        let hidden = &self.hidden_performers;
        let performers = &self.document.performers;
        self.selected.retain(|&index| {
            performers.get(index).is_some_and(|performer| {
                index < performer_count
                    && !locked.contains(&performer.id)
                    && !hidden.contains(&performer.id)
            })
        });
        self.status = format!(
            "{} {}",
            self.last_filtered_performers.len(),
            i18n::registered(self.locale, "app-ui.152")
        );
    }

    fn clear_performer_filters(&mut self) {
        self.locked_performers.clear();
        self.hidden_performers.clear();
        self.last_filtered_performers.clear();
    }

    /// Restore one performer to the editable field. These presentation filters
    /// intentionally live only in the app session, so recovery never creates
    /// an undo entry or modifies the production document.
    fn restore_filtered_performer(&mut self, id: PerformerId) {
        self.locked_performers.remove(&id);
        self.hidden_performers.remove(&id);
        self.last_filtered_performers.remove(&id);
    }

    /// Reverses the latest filter action without changing the drill or history.
    fn restore_last_filtered_performers(&mut self) -> usize {
        let ids = std::mem::take(&mut self.last_filtered_performers);
        let restored = ids
            .iter()
            .filter(|id| self.locked_performers.contains(id) || self.hidden_performers.contains(id))
            .count();
        for id in ids {
            self.locked_performers.remove(&id);
            self.hidden_performers.remove(&id);
        }
        restored
    }

    /// Restore every temporarily filtered performer belonging to a section.
    /// Returning the count lets the UI give a concrete, reassuring status.
    fn restore_filtered_section(&mut self, section: drill_core::SectionId) -> usize {
        let ids: Vec<_> = self
            .document
            .performers
            .iter()
            .filter(|performer| performer.section == section)
            .map(|performer| performer.id)
            .collect();
        let restored = ids
            .iter()
            .filter(|id| self.locked_performers.contains(id) || self.hidden_performers.contains(id))
            .count();
        for id in ids {
            self.restore_filtered_performer(id);
        }
        restored
    }

    /// Select an eligible Real View diagnostic target. Locked and presentation-
    /// hidden performers deliberately remain unavailable, matching pointer and
    /// marquee selection elsewhere in the field editor.
    fn focus_visibility_target(&mut self, candidates: &[usize], direction: i8) {
        let eligible: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|&index| self.is_selectable_index(index))
            .collect();
        let Some(index) = (!eligible.is_empty()).then(|| {
            let current = self.visibility_focus.and_then(|id| {
                eligible
                    .iter()
                    .position(|&candidate| self.document.performers[candidate].id == id)
            });
            let position = match (current, direction.cmp(&0)) {
                (Some(position), std::cmp::Ordering::Less) => {
                    (position + eligible.len() - 1) % eligible.len()
                }
                (Some(position), _) => (position + 1) % eligible.len(),
                (None, std::cmp::Ordering::Less) => eligible.len() - 1,
                (None, _) => 0,
            };
            eligible[position]
        }) else {
            return;
        };
        self.visibility_focus = Some(self.document.performers[index].id);
        self.replace_selection([index].into_iter().collect());
        self.workspace_focus = Some(WorkspaceFocus::Performer);
    }

    fn select_visibility_targets(&mut self, candidates: &[usize]) {
        let selection = candidates
            .iter()
            .copied()
            .filter(|&index| self.is_selectable_index(index))
            .collect();
        self.visibility_focus = None;
        self.replace_selection(selection);
    }

    fn show_update_notice(&mut self, context: &egui::Context) {
        let Some(release) = self.update_state.available.clone() else {
            return;
        };
        let mut open = true;
        egui::Window::new(i18n::registered(self.locale, "app-state.001"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(context, |ui| {
                ui.heading(format!("DrillForge {}", release.version));
                ui.label(if self.locale == Locale::Ja {
                    &release.release_notes_ja
                } else {
                    &release.release_notes_en
                });
                ui.small(i18n::registered(self.locale, "app-state.002"));
                ui.horizontal(|ui| {
                    if ui
                        .button(i18n::registered(self.locale, "app-state.003"))
                        .on_hover_text(i18n::registered(self.locale, "app-state.004"))
                        .clicked()
                    {
                        context.copy_text(release.artifact.download_url.clone());
                        self.update_state.status =
                            update_state::UpdateStatus::DownloadAddressCopied;
                        self.update_state.available = None;
                    }
                    if ui
                        .button(i18n::registered(self.locale, "app-state.005"))
                        .clicked()
                    {
                        self.update_state.available = None;
                    }
                    if ui
                        .button(i18n::registered(self.locale, "app-state.006"))
                        .clicked()
                    {
                        self.update_state.skip_available();
                    }
                });
            });
        if !open {
            self.update_state.available = None;
        }
    }

    fn command_context(&self) -> commands::Context {
        let current_count = self
            .document
            .global_count(self.current_set, self.count_position)
            .round()
            .clamp(0.0, self.document.timeline_counts() as f32) as u32;
        commands::Context {
            can_undo: self.history.can_undo(),
            can_redo: self.history.can_redo(),
            has_performers: !self.document.performers.is_empty(),
            has_selection: !self.selected.is_empty(),
            has_recent_selection: self.can_restore_selection(),
            has_formation_clipboard: !self.formation_clipboard.entries.is_empty(),
            has_multiple_selection: self.selected.len() >= 2,
            can_edit_selection: self.is_editable_set_start(),
            has_sets: !self.document.sets.is_empty(),
            has_previous_production_marker: adjacent_production_marker_count(
                &self.document,
                current_count,
                false,
            )
            .is_some(),
            has_next_production_marker: adjacent_production_marker_count(
                &self.document,
                current_count,
                true,
            )
            .is_some(),
            has_previous_set: self.current_set > 0,
            has_next_set: self.current_set.saturating_add(1) < self.document.sets.len(),
        }
    }

    fn command_menu(&mut self, ui: &mut egui::Ui, menu: CommandMenu) {
        if let Some(command) = commands::show_menu(ui, menu, self.command_context(), self.locale) {
            self.execute_command(command, ui.ctx());
        }
    }

    fn duplicate_current_set(&mut self) {
        let Some(source) = self.document.sets.get(self.current_set).cloned() else {
            return;
        };
        let insert_at = self.current_set + 1;
        let new_id = self
            .document
            .sets
            .iter()
            .map(|set| set.id.get())
            .max()
            .and_then(|id| id.checked_add(1))
            .and_then(drill_core::SetId::new)
            .unwrap_or(source.id);
        if new_id == source.id {
            self.status = i18n::registered(self.locale, "app-state.136").into();
            return;
        }
        let mut next = self.document.clone();
        next.sets.insert(
            insert_at,
            Set {
                id: new_id,
                name: format!("セット {}", insert_at + 1),
                ..source
            },
        );
        if let Err(error) = self.history.execute(
            &mut self.document,
            Edit::ReplaceDocument {
                document: Box::new(next),
            },
        ) {
            self.status = error.message(self.locale);
            return;
        }
        self.current_set = insert_at;
        self.count_position = 0.0;
        self.dirty = true;
    }

    /// Auto-generates a "follow the leader" snake maneuver as `steps`
    /// ordinary intermediate Sets inserted between the current set and the
    /// next one, rather than modeling it as a continuous path-with-timing
    /// primitive. Pyware's version of this maneuver authors one continuous
    /// path with live per-performer timing offsets; the user found that hard
    /// to fine-tune after the fact. Because every inserted Set here is just
    /// an ordinary `Set`, the user can go back and hand-edit any individual
    /// dot afterward exactly like any other set.
    ///
    /// `self.selected` (in ascending performer-index order -- this codebase
    /// does not track click order, see the caller's selection UI) supplies
    /// the ordered group: rank 0 is the leader, furthest along `spec` at
    /// every inserted set; rank r follows `1/len` of a lap behind rank r-1.
    /// Performers outside the group hold their existing position, copied
    /// unchanged from the set being split, in every inserted set.
    fn apply_follow_the_leader(&mut self, spec: shapes::ShapeSpec, steps: usize) {
        let group: Vec<usize> = self.selected.iter().copied().collect();
        if group.len() < 2 {
            self.status = i18n::registered(self.locale, "workspace-inspector.173").into();
            return;
        }
        if steps == 0 || spec.validate().is_err() {
            self.status = i18n::registered(self.locale, "workspace-inspector.153").into();
            return;
        }
        let Some(source) = self.document.sets.get(self.current_set).cloned() else {
            return;
        };

        // Dense arc-length-uniform samples of the path so any fractional
        // progress in [0, 1] can be looked up by interpolating between the
        // two nearest samples; a fixed high resolution keeps this accurate
        // regardless of `steps` or the group size.
        const RESOLUTION: usize = 512;
        let mut dense = Vec::with_capacity(RESOLUTION);
        spec.sample(RESOLUTION, &mut dense);
        if dense.len() < 2 {
            self.status = i18n::registered(self.locale, "workspace-inspector.175").into();
            return;
        }
        let sample_progress = |progress: f32| -> Point {
            let progress = progress.clamp(0.0, 1.0);
            let scaled = progress * (dense.len() - 1) as f32;
            let i0 = scaled.floor() as usize;
            let i1 = (i0 + 1).min(dense.len() - 1);
            dense[i0].lerp(dense[i1], scaled - i0 as f32)
        };

        // Each rank lags the one ahead of it by a fixed 1/len share of the
        // path: the leader (rank 0) reaches the end exactly at the final
        // inserted set, and each follower's own window opens later so the
        // group reads as a staggered snake rather than a straight-line move.
        // Followers whose window hasn't opened yet (progress would be
        // negative) sit at the path's start point; a rank whose window
        // finishes before `steps` runs out holds at the path's end point.
        let delay_per_rank = 1.0 / group.len() as f32;
        let insert_at = self.current_set + 1;
        let mut next = self.document.clone();
        let grid = next.grid.clone();

        // Snap each performer's own trajectory across the inserted sets as
        // one sequence (error diffusion), not each (rank, t) point in
        // isolation: a performer's path over time is exactly the kind of
        // smooth curve `snap_sequence` is for, and quantizing it
        // independently at each set would make their motion look jerky
        // between frames instead of a smooth follow.
        let snapped_trajectories: Vec<Vec<Point>> = group
            .iter()
            .enumerate()
            .map(|(rank, _)| {
                let raw: Vec<Point> = (0..steps)
                    .map(|t| {
                        let t_norm = (t + 1) as f32 / steps as f32;
                        let progress = t_norm - rank as f32 * delay_per_rank;
                        sample_progress(progress)
                    })
                    .collect();
                grid.snap_sequence(&raw)
            })
            .collect();

        let mut next_raw_id = next.sets.iter().map(|set| set.id.get()).max().unwrap_or(0);
        let mut inserted = Vec::with_capacity(steps);
        // `t` is used for more than indexing here (it also feeds the
        // inserted set's `t + 1` display name and the sequential set-ID
        // allocation with its own early-return path), so an
        // iterator/enumerate rewrite wouldn't be clearer than the loop.
        #[allow(clippy::needless_range_loop)]
        for t in 0..steps {
            let Some(next_id) = next_raw_id.checked_add(1) else {
                self.status = i18n::registered(self.locale, "workspace-inspector.150").into();
                return;
            };
            next_raw_id = next_id;
            let Some(set_id) = drill_core::SetId::new(next_id) else {
                self.status = i18n::registered(self.locale, "workspace-inspector.174").into();
                return;
            };
            let mut positions = source.positions.clone();
            for (rank, &index) in group.iter().enumerate() {
                if let Some(slot) = positions.get_mut(index) {
                    *slot = snapped_trajectories[rank][t];
                }
            }
            inserted.push(Set {
                id: set_id,
                name: format!(
                    "{} {}",
                    i18n::registered(self.locale, "workspace-inspector.148"),
                    t + 1
                ),
                annotation: Default::default(),
                counts: 4,
                hold: 0,
                routes: Default::default(),
                shape: None,
                positions,
            });
        }

        let step_count = inserted.len();
        for (offset, set) in inserted.into_iter().enumerate() {
            next.sets.insert(insert_at + offset, set);
        }

        if self.execute_edit(
            Edit::ReplaceDocument {
                document: Box::new(next),
            },
            i18n::registered(self.locale, "workspace-inspector.151"),
        ) {
            self.current_set = insert_at;
            self.count_position = 0.0;
            self.status = format!(
                "{} {}",
                i18n::registered(self.locale, "workspace-inspector.152"),
                step_count
            );
        }
    }

    fn execute_command(&mut self, command: UiCommand, context: &egui::Context) {
        match command {
            UiCommand::OpenDocument => self.request_open_document(DocumentOpenKind::LegacyJson),
            UiCommand::OpenProject => self.request_open_document(DocumentOpenKind::Project),
            UiCommand::OpenRecent => self.show_recent_projects = true,
            UiCommand::Save => self.save_dialog(),
            UiCommand::SaveAs => self.save_as_dialog(),
            UiCommand::SaveProjectAs => self.save_project_dialog(),
            UiCommand::ImportCoordinates => self.import_coordinates_dialog(),
            UiCommand::ImportMusicalTimeline => self.import_musical_dialog(),
            UiCommand::LoadImageUnderlay => self.load_underlay_dialog(),
            UiCommand::Undo => {
                if self.history.undo(&mut self.document) {
                    self.dirty = true;
                    self.status = i18n::registered(self.locale, "app-state.149").into();
                }
            }
            UiCommand::Redo => {
                if self.history.redo(&mut self.document) {
                    self.dirty = true;
                    self.status = i18n::registered(self.locale, "app-state.150").into();
                }
            }
            UiCommand::SelectAll => {
                self.replace_selection((0..self.document.performers.len()).collect())
            }
            UiCommand::ClearSelection => {
                if self.clipboard_paste_preview.is_some() {
                    self.cancel_clipboard_paste_preview();
                } else if self.formation_preview_spec.is_some() || self.free_draw_active {
                    self.cancel_shape_preview();
                } else {
                    self.clear_selection();
                }
            }
            UiCommand::RestoreRecentSelection => self.restore_recent_selection(),
            UiCommand::CopyFormation => self.copy_selected_formation(),
            UiCommand::PasteFormation => self.begin_clipboard_paste_preview(),
            UiCommand::AlignHorizontal
            | UiCommand::AlignVertical
            | UiCommand::DistributeHorizontal
            | UiCommand::DistributeVertical
            | UiCommand::FlipHorizontal
            | UiCommand::FlipVertical
            | UiCommand::MakeLine => self.arrange_selection(command),
            UiCommand::LockSelection => self.lock_selected_performers(),
            UiCommand::HideSelection => self.hide_selected_performers(),
            UiCommand::DuplicateSet => self.duplicate_current_set(),
            UiCommand::ManageSections => self.section_manager.open = true,
            UiCommand::PlayPause => self.toggle_playback(context),
            UiCommand::RangeStart => self.navigate_to_global_count(self.playback_start),
            UiCommand::RangeCurrentSet => {
                let start = self.document.global_count(self.current_set, 0.0) as u32;
                self.playback_start = start;
                self.playback_end = (start
                    + u32::from(self.document.sets[self.current_set].counts))
                .min(self.document.timeline_counts());
            }
            UiCommand::RangeWholeShow => {
                self.playback_start = 0;
                self.playback_end = self.document.timeline_counts();
            }
            UiCommand::MarkRangeStart => {
                let current_global = self
                    .document
                    .global_count(self.current_set, self.count_position);
                self.playback_start =
                    (current_global.round() as u32).min(self.playback_end.saturating_sub(1));
            }
            UiCommand::MarkRangeEnd => {
                let current_global = self
                    .document
                    .global_count(self.current_set, self.count_position);
                let total_counts = self.document.timeline_counts();
                self.playback_end = (current_global.round() as u32)
                    .max(self.playback_start + 1)
                    .min(total_counts);
            }
            UiCommand::PreviousProductionMarker => self.navigate_production_marker(false),
            UiCommand::NextProductionMarker => self.navigate_production_marker(true),
            UiCommand::PreviousSet => self.navigate_relative_set(-1),
            UiCommand::NextSet => self.navigate_relative_set(1),
            UiCommand::GoToGlobalCount => self.go_to_count.open(),
            UiCommand::FocusPerformerTools => {
                self.show_inspector = true;
                self.workspace_focus = Some(WorkspaceFocus::Performer)
            }
            UiCommand::FocusClinic => {
                self.show_inspector = true;
                self.workspace_focus = Some(WorkspaceFocus::Clinic);
            }
            UiCommand::FocusGrid => {
                self.show_inspector = true;
                self.workspace_focus = Some(WorkspaceFocus::Grid);
            }
            UiCommand::FocusTempo => {
                self.show_inspector = true;
                self.workspace_focus = Some(WorkspaceFocus::Tempo);
            }
            UiCommand::FocusVideo => {
                self.show_inspector = true;
                self.workspace_focus = Some(WorkspaceFocus::Video);
            }
            UiCommand::OpenPrint => self.print_state.open = true,
            UiCommand::FocusAudio => {
                self.show_inspector = true;
                self.workspace_focus = Some(WorkspaceFocus::Audio);
            }
            UiCommand::OpenProductionSheet => {
                self.production_sheet_workspace.open = true;
                self.workspace_focus = Some(WorkspaceFocus::ProductionSheet);
            }
            UiCommand::View2d => self.view_mode = ViewMode::Field2D,
            UiCommand::View3d => self.view_mode = ViewMode::Stadium3D,
            UiCommand::ToggleFocusField => {
                self.focus_field = !self.focus_field;
                self.status = i18n::registered(
                    self.locale,
                    if self.focus_field {
                        "focus-field.002"
                    } else {
                        "focus-field.003"
                    },
                )
                .into();
            }
            UiCommand::WorkspaceDesign => self.apply_workspace_preset(WorkspacePreset::Design),
            UiCommand::WorkspaceReview => self.apply_workspace_preset(WorkspacePreset::Review),
            UiCommand::WorkspacePresent => self.apply_workspace_preset(WorkspacePreset::Present),
            UiCommand::ToggleGuidance => self.show_guidance = !self.show_guidance,
            UiCommand::TogglePerfHud => self.perf_hud.toggle(),
            UiCommand::ToggleCollaborators => self.presence.open = !self.presence.open,
            UiCommand::GettingStarted => self.onboarding.show_help = true,
            UiCommand::LegalNotices => self.show_legal_notices = true,
        }
    }

    /// Apply a workspace-only arrangement.  Keep this central so a palette,
    /// menu, or future shortcut cannot accidentally make a document edit.
    fn apply_workspace_preset(&mut self, preset: WorkspacePreset) {
        self.workspace_preset = preset;
        match preset {
            WorkspacePreset::Design => {
                self.view_mode = ViewMode::Field2D;
                self.show_inspector = true;
                self.show_guidance = true;
                self.simple_mode.enabled = false;
                self.heatmap_enabled = false;
                self.workspace_focus = Some(WorkspaceFocus::Performer);
            }
            WorkspacePreset::Review => {
                self.view_mode = ViewMode::Field2D;
                self.show_inspector = true;
                self.show_guidance = false;
                self.simple_mode.enabled = false;
                self.heatmap_enabled = true;
                self.workspace_focus = Some(WorkspaceFocus::Clinic);
            }
            WorkspacePreset::Present => {
                self.view_mode = ViewMode::Stadium3D;
                self.show_inspector = false;
                self.show_guidance = false;
                self.simple_mode.enabled = false;
                self.heatmap_enabled = false;
                self.camera_program_preview = true;
                self.workspace_focus = None;
            }
        }
        self.status = match preset {
            WorkspacePreset::Design => i18n::registered(self.locale, "workspace-preset.004"),
            WorkspacePreset::Review => i18n::registered(self.locale, "workspace-preset.005"),
            WorkspacePreset::Present => i18n::registered(self.locale, "workspace-preset.006"),
        }
        .into();
    }

    /// Stop at the exact integer count of the neighboring production marker.
    /// This is intentionally separate from smooth playback: a navigation
    /// command is a rehearsal editing action and must never land between beats.
    fn navigate_production_marker(&mut self, forward: bool) {
        let current_count = self
            .document
            .global_count(self.current_set, self.count_position)
            .round()
            .clamp(0.0, self.document.timeline_counts() as f32) as u32;
        let Some(target_count) =
            adjacent_production_marker_count(&self.document, current_count, forward)
        else {
            return;
        };
        self.navigation_seek_to_count(target_count);
        self.playing = false;
        self.audio_state.pause();
        if let Some(track) = &self.document.audio {
            self.audio_state
                .seek_seconds(drill_core::audio::count_to_audio_time(
                    track,
                    &self.document.tempo,
                    target_count as f32,
                ));
        }
        self.status = i18n::registered(
            self.locale,
            if forward {
                "app-state.151"
            } else {
                "app-state.152"
            },
        )
        .into();
    }

    fn show_print_workspace(&mut self, context: &egui::Context) {
        self.print_state.poll(self.locale);
        if !self.print_state.open {
            return;
        }
        let selected = self.selected.iter().copied().collect::<Vec<_>>();
        let mut open = self.print_state.open;
        let mut notation = self.document.grid.coordinate_notation.clone();
        let mut notation_changed = false;
        egui::Window::new(i18n::registered(self.locale, "app-state.007"))
            .open(&mut open).default_width(720.0).resizable(true).scroll(true).show(context, |ui| {
                ui.heading(i18n::registered(self.locale, "app-state.008"));
                ui.label(i18n::registered(self.locale, "app-state.009"));
                ui.separator();
                ui.columns(2, |columns| {
                    let ui=&mut columns[0]; ui.strong(i18n::registered(self.locale, "app-state.010"));
                    ui.radio_value(&mut self.print_state.kind, drill_export::report::ReportKind::SetChart, i18n::registered(self.locale, "app-state.011"));
                    ui.radio_value(&mut self.print_state.kind, drill_export::report::ReportKind::PerformerDrillBook, i18n::registered(self.locale, "app-state.012"));
                    if self.print_state.kind == drill_export::report::ReportKind::PerformerDrillBook { ui.small(if self.locale == Locale::Ja { if selected.is_empty() { "全演者を出力します" } else { "選択中の演者だけを出力します" } } else if selected.is_empty() { "Exports every performer" } else { "Exports selected performers only" }); }
                    ui.radio_value(&mut self.print_state.kind, drill_export::report::ReportKind::CountSheet, i18n::registered(self.locale, "app-state.013"));
                    ui.radio_value(&mut self.print_state.kind, drill_export::report::ReportKind::ProductionSheet, i18n::registered(self.locale, "app-state.014"));
                    ui.add_space(10.0); ui.strong(i18n::registered(self.locale, "app-state.015"));
                    ui.horizontal(|ui| {
                        if ui.button(i18n::registered(self.locale, "app-state.016")).clicked() { notation=Default::default(); notation_changed=true; }
                        if ui.button("DCI 8-to-5").clicked() { notation=drill_core::coordinates::CoordinateNotation::dci(); notation_changed=true; }
                        if ui.button(i18n::registered(self.locale, "app-state.017")).clicked() { notation=drill_core::coordinates::CoordinateNotation::indoor(); notation_changed=true; }
                    });
                    egui::ComboBox::from_id_salt("coordinate-rounding").selected_text(format!("{:?}",notation.rounding)).show_ui(ui,|ui| {
                        use drill_core::coordinates::StepRounding::*;
                        notation_changed |= ui.selectable_value(&mut notation.rounding,Eighth,"1/8 step").changed();
                        notation_changed |= ui.selectable_value(&mut notation.rounding,Quarter,"1/4 step").changed();
                        notation_changed |= ui.selectable_value(&mut notation.rounding,Half,"1/2 step").changed();
                        notation_changed |= ui.selectable_value(&mut notation.rounding,Whole,"1 step").changed();
                    });
                    egui::ComboBox::from_id_salt("yard-line-interval").selected_text(format!("{:?}",notation.yard_lines)).show_ui(ui,|ui| {
                        use drill_core::coordinates::YardLineInterval::*;
                        notation_changed |= ui.selectable_value(&mut notation.yard_lines,Grid,i18n::registered(self.locale, "app-state.018")).changed();
                        notation_changed |= ui.selectable_value(&mut notation.yard_lines,Five,"5 yd").changed();
                        notation_changed |= ui.selectable_value(&mut notation.yard_lines,Ten,"10 yd").changed();
                        let custom = match notation.yard_lines { Custom(v)=>v, _=>5.0 };
                        notation_changed |= ui.selectable_value(&mut notation.yard_lines,Custom(custom),i18n::registered(self.locale, "app-state.019")).changed();
                    });
                    if let drill_core::coordinates::YardLineInterval::Custom(value)=&mut notation.yard_lines { ui.horizontal(|ui| { ui.label(i18n::registered(self.locale, "app-state.020")); notation_changed |= ui.add(egui::DragValue::new(value).range(1.0..=50.0).speed(0.25)).changed(); }); }
                    ui.horizontal(|ui| {
                        use drill_core::coordinates::OnLineStyle::*;
                        ui.label(i18n::registered(self.locale, "app-state.021"));
                        notation_changed |= ui.selectable_value(&mut notation.on_line,Explicit,i18n::registered(self.locale, "app-state.022")).changed();
                        notation_changed |= ui.selectable_value(&mut notation.on_line,Short,i18n::registered(self.locale, "app-state.023")).changed();
                    });
                    egui::ComboBox::from_id_salt("step-notation-style").selected_text(format!("{:?}",notation.step_style)).show_ui(ui,|ui| {
                        use drill_core::coordinates::StepNotationStyle::*;
                        notation_changed |= ui.selectable_value(&mut notation.step_style,Steps,i18n::registered(self.locale, "app-state.024")).changed();
                        notation_changed |= ui.selectable_value(&mut notation.step_style,EightToFive,"8-to-5").changed();
                        notation_changed |= ui.selectable_value(&mut notation.step_style,SixToFive,"6-to-5").changed();
                    });
                    egui::ComboBox::from_id_salt("depth-reference").selected_text(match &notation.front_back { drill_core::coordinates::FrontBackReference::NearestLine=>i18n::registered(self.locale, "app-state.026"), drill_core::coordinates::FrontBackReference::NearestHash=>i18n::registered(self.locale, "app-state.025"), drill_core::coordinates::FrontBackReference::FixedLabel(v)=>v.as_str() }).show_ui(ui,|ui| {
                        use drill_core::coordinates::FrontBackReference::*;
                        notation_changed |= ui.selectable_value(&mut notation.front_back,NearestLine,i18n::registered(self.locale, "app-state.027")).changed();
                        notation_changed |= ui.selectable_value(&mut notation.front_back,NearestHash,i18n::registered(self.locale, "app-state.028")).changed();
                        for line in &self.document.grid.hashes { notation_changed |= ui.selectable_value(&mut notation.front_back,FixedLabel(line.label.clone()),&line.label).changed(); }
                    });
                    egui::ComboBox::from_id_salt("production-template").selected_text(format!("{:?}",notation.production_template)).show_ui(ui,|ui| {
                        use drill_core::coordinates::ProductionTemplatePreset::*;
                        notation_changed |= ui.selectable_value(&mut notation.production_template,Standard,i18n::registered(self.locale, "app-state.029")).changed();
                        notation_changed |= ui.selectable_value(&mut notation.production_template,Compact,i18n::registered(self.locale, "app-state.030")).changed();
                        notation_changed |= ui.selectable_value(&mut notation.production_template,Rehearsal,i18n::registered(self.locale, "app-state.031")).changed();
                    });
                    ui.add_space(10.0); ui.strong(i18n::registered(self.locale, "app-state.032"));
                    egui::ComboBox::from_id_salt("print-page-size").selected_text(self.print_state.page_size_label()).show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.print_state.settings.page_size, drill_export::page::PageSize::A4, "A4");
                        ui.selectable_value(&mut self.print_state.settings.page_size, drill_export::page::PageSize::Letter, "Letter");
                        ui.selectable_value(&mut self.print_state.settings.page_size, drill_export::page::PageSize::Tabloid, "Tabloid");
                    });
                    egui::ComboBox::from_id_salt("print-orientation").selected_text(self.print_state.orientation_label(self.locale)).show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.print_state.settings.orientation, drill_export::page::Orientation::Portrait, i18n::registered(self.locale, "app-state.033"));
                        ui.selectable_value(&mut self.print_state.settings.orientation, drill_export::page::Orientation::Landscape, i18n::registered(self.locale, "app-state.034"));
                    });
                    ui.label(i18n::registered(self.locale, "app-state.137"));
                    ui.horizontal(|ui| { ui.label(i18n::registered(self.locale, "app-state.138")); ui.add(egui::DragValue::new(&mut self.print_state.settings.margins.top_mm).range(3.0..=40.0)); ui.label(i18n::registered(self.locale, "app-state.139")); ui.add(egui::DragValue::new(&mut self.print_state.settings.margins.bottom_mm).range(3.0..=40.0)); });
                    ui.horizontal(|ui| { ui.label(i18n::registered(self.locale, "app-state.140")); ui.add(egui::DragValue::new(&mut self.print_state.settings.margins.left_mm).range(3.0..=40.0)); ui.label(i18n::registered(self.locale, "app-state.141")); ui.add(egui::DragValue::new(&mut self.print_state.settings.margins.right_mm).range(3.0..=40.0)); });
                    ui.add_space(12.0);
                    if let Some(progress)=self.print_state.progress() { ui.add(egui::ProgressBar::new(progress).show_percentage().text(i18n::registered(self.locale, "app-state.142"))); if ui.button(i18n::registered(self.locale, "app-state.143")).clicked() { self.print_state.cancel(); } }
                    else if ui.button(i18n::registered(self.locale, "app-state.035")).clicked() && let Some(path)=rfd::FileDialog::new().set_file_name(match self.print_state.kind { drill_export::report::ReportKind::SetChart=>"set_charts.pdf",drill_export::report::ReportKind::PerformerDrillBook=>"drill_book.pdf",drill_export::report::ReportKind::CountSheet=>"count_sheet.pdf",drill_export::report::ReportKind::ProductionSheet=>"production_sheet.pdf" }).add_filter("PDF", &["pdf"]).save_file() { self.print_state.start(&self.document,self.beats_per_measure,&selected,path,self.locale,self.underlay_state.asset_bytes.clone()); }
                    if !self.print_state.status.is_empty() { ui.label(&self.print_state.status); }
                    if let Some((path,_))=&self.print_state.completed && ui.button(i18n::registered(self.locale, "app-state.144")).clicked() { let target=path.parent().unwrap_or(path); let _=std::process::Command::new("explorer").arg(target).spawn(); }

                    let ui=&mut columns[1]; ui.strong(i18n::registered(self.locale, "app-state.036"));
                    let pages=self.print_state.page_count(&self.document,self.beats_per_measure,&selected);
                    let (w,h)=self.print_state.settings.dimensions_mm(); ui.label(format!("{pages} ページ · {w:.1} × {h:.1} mm"));
                    let available=ui.available_width().min(300.0); let ratio=h/w; let (rect,_)=ui.allocate_exact_size(Vec2::new(available, (available*ratio).min(430.0)),Sense::hover());
                    ui.painter().rect_filled(rect,3.0,Color32::from_gray(245)); ui.painter().rect_stroke(rect,3.0,Stroke::new(1.0,Color32::from_gray(100)),StrokeKind::Inside);
                    let inner=rect.shrink(16.0); ui.painter().text(inner.left_top(),egui::Align2::LEFT_TOP,&self.document.title,egui::FontId::proportional(13.0),Color32::from_gray(25));
                    let report_name=match self.print_state.kind { drill_export::report::ReportKind::SetChart=>"SET CHARTS",drill_export::report::ReportKind::PerformerDrillBook=>"PERFORMER DRILL BOOK",drill_export::report::ReportKind::CountSheet=>"COUNT SHEET",drill_export::report::ReportKind::ProductionSheet=>"PRODUCTION SHEET" };
                    ui.painter().text(Pos2::new(inner.left(),inner.top()+28.0),egui::Align2::LEFT_TOP,report_name,egui::FontId::monospace(10.0),Color32::from_gray(60));
                    for row in 0..8 { let y=inner.top()+55.0+row as f32*18.0; ui.painter().line_segment([Pos2::new(inner.left(),y),Pos2::new(inner.right(),y)],Stroke::new(1.0,Color32::from_gray(190))); }
                    ui.small("ページ内容の概要表示です。PDFでは実寸レイアウトとフィールド図を生成します。");
                });
            });
        self.print_state.open = open;
        if notation_changed {
            let mut grid = self.document.grid.clone();
            grid.coordinate_notation = notation;
            self.execute_edit(
                Edit::ReplaceGrid {
                    grid,
                    scale_positions: false,
                },
                "座標表記を更新できませんでした",
            );
        }
    }
    fn new(creation: &eframe::CreationContext<'_>) -> Self {
        let app_theme = app_theme::AppTheme::load();
        app_theme.apply(&creation.egui_ctx);
        Self {
            onboarding: onboarding::OnboardingState::load(),
            gpu: gpu_bridge::Bridge::install(creation),
            app_theme,
            ..Self::default()
        }
    }

    fn execute_edit(&mut self, edit: Edit, failure: &str) -> bool {
        match self.history.execute(&mut self.document, edit) {
            Ok(()) => {
                self.dirty = true;
                true
            }
            Err(_) => {
                self.status = failure.into();
                false
            }
        }
    }

    fn show_subset_snapshot_workspace(&mut self, context: &egui::Context) {
        let selected_ids = self
            .selected
            .iter()
            .filter_map(|&index| self.document.performers.get(index).map(|p| p.id))
            .collect::<BTreeSet<_>>();
        let action =
            self.subset_snapshot_state
                .show(context, self.locale, &self.document, &selected_ids);
        let Some(action) = action else { return };
        use subset_snapshot_state::Action;
        match action {
            Action::AddSubset { name } => {
                let raw = self
                    .document
                    .subsets
                    .iter()
                    .map(|s| s.id.get())
                    .max()
                    .unwrap_or(0)
                    .saturating_add(1);
                let Some(id) = drill_core::SubsetId::new(raw) else {
                    self.status = i18n::registered(self.locale, "app-state.037").into();
                    return;
                };
                let subset = drill_core::Subset {
                    id,
                    name,
                    members: selected_ids.iter().copied().collect(),
                };
                let failure = i18n::registered(self.locale, "app-state.038");
                if self.execute_edit(Edit::AddSubset { subset, at: None }, failure) {
                    self.subset_snapshot_state.subset_added();
                }
            }
            Action::RenameSubset { id, name } => {
                let failure = i18n::registered(self.locale, "app-state.039");
                self.execute_edit(Edit::RenameSubset { id, name }, failure);
            }
            Action::SetMembers { id, members } => {
                let failure = i18n::registered(self.locale, "app-state.040");
                self.execute_edit(Edit::SetSubsetMembers { id, members }, failure);
            }
            Action::SelectMembers { id, mode } => {
                let member_ids = self
                    .document
                    .subsets
                    .iter()
                    .find(|s| s.id == id)
                    .map(|s| s.members.iter().copied().collect::<BTreeSet<_>>())
                    .unwrap_or_default();
                let members = self
                    .document
                    .performers
                    .iter()
                    .enumerate()
                    .filter_map(|(index, performer)| {
                        member_ids.contains(&performer.id).then_some(index)
                    })
                    .collect();
                let next = match mode {
                    subset_snapshot_state::SelectionMode::Replace => members,
                    subset_snapshot_state::SelectionMode::Add => {
                        self.selected.union(&members).copied().collect()
                    }
                    subset_snapshot_state::SelectionMode::Exclude => {
                        self.selected.difference(&members).copied().collect()
                    }
                };
                self.replace_selection(next);
                self.status = if self.locale == Locale::Ja {
                    format!("サブセットから{}人を選択しました", self.selected.len())
                } else {
                    format!(
                        "Selected {} performers from the subset",
                        self.selected.len()
                    )
                };
            }
            Action::FocusChanges {
                set_ids,
                performer_ids,
            } => {
                if let Some(set_index) = set_ids
                    .iter()
                    .find_map(|id| self.document.sets.iter().position(|set| set.id == *id))
                {
                    self.navigate_to_set(set_index);
                }
                if !performer_ids.is_empty() {
                    let changed = self
                        .document
                        .performers
                        .iter()
                        .enumerate()
                        .filter_map(|(index, performer)| {
                            performer_ids.contains(&performer.id).then_some(index)
                        })
                        .collect();
                    self.replace_selection(changed);
                    self.status = format!(
                        "{}: {}",
                        i18n::registered(self.locale, "subset-snapshot-state.009"),
                        self.selected.len()
                    );
                }
            }
            Action::RemoveSubset { id } => {
                let failure = i18n::registered(self.locale, "app-state.041");
                self.execute_edit(Edit::RemoveSubset { id }, failure);
            }
            Action::CaptureSnapshot { name } => {
                match self.subset_snapshot_state.captured(name, &self.document) {
                    Ok(()) => self.status = i18n::registered(self.locale, "app-state.042").into(),
                    Err(error) => self.status = error.localized(self.locale).into(),
                }
            }
            Action::RestoreSnapshot { index } => {
                let Some(snapshot) = self.subset_snapshot_state.snapshot(index).cloned() else {
                    return;
                };
                let failure = i18n::registered(self.locale, "app-state.043");
                if self.execute_edit(
                    Edit::ReplaceDocument {
                        document: Box::new(snapshot.document),
                    },
                    failure,
                ) {
                    self.current_set = 0;
                    self.count_position = 0.0;
                    self.playback_start = 0;
                    self.playback_end = self.document.timeline_counts();
                    self.playing = false;
                    self.audio_state.pause();
                    self.selected.clear();
                    self.status = if self.locale == Locale::Ja {
                        format!("「{}」を復元しました。Undoで戻せます", snapshot.name)
                    } else {
                        format!("Restored “{}”. Undo is available", snapshot.name)
                    };
                }
            }
            Action::ForkBranch { name } => {
                match self.subset_snapshot_state.forked(name, &self.document) {
                    Ok(()) => self.status = i18n::registered(self.locale, "app-state.044").into(),
                    Err(error) => self.status = error.localized(self.locale).into(),
                }
            }
            Action::CheckpointBranch => {
                match self.subset_snapshot_state.checkpoint(&self.document) {
                    Ok(()) => self.status = i18n::registered(self.locale, "app-state.045").into(),
                    Err(error) => self.status = error.localized(self.locale).into(),
                }
            }
            Action::SwitchBranch { id } => {
                let Some(document) = self.subset_snapshot_state.branch_document(id) else {
                    return;
                };
                let failure = i18n::registered(self.locale, "app-state.046");
                if self.execute_edit(
                    Edit::ReplaceDocument {
                        document: Box::new(document),
                    },
                    failure,
                ) {
                    if let Err(error) = self.subset_snapshot_state.switched(id) {
                        self.status = error.localized(self.locale).into();
                        return;
                    }
                    self.current_set = 0;
                    self.count_position = 0.0;
                    self.playback_start = 0;
                    self.playback_end = self.document.timeline_counts();
                    self.playing = false;
                    self.audio_state.pause();
                    self.selected.clear();
                    self.status = i18n::registered(self.locale, "app-state.047").into();
                }
            }
            Action::MergeBranch { id, use_theirs } => {
                let mut preview = match self.subset_snapshot_state.merge_candidate(id) {
                    Ok(value) => value,
                    Err(error) => {
                        self.status = error.localized(self.locale).into();
                        return;
                    }
                };
                if let Err(error) = self
                    .subset_snapshot_state
                    .resolve_merge(&mut preview, use_theirs)
                {
                    self.status = error.localized(self.locale).into();
                    return;
                }
                let failure = i18n::registered(self.locale, "app-state.048");
                if self.execute_edit(
                    Edit::ReplaceDocument {
                        document: Box::new(preview.candidate.clone()),
                    },
                    failure,
                ) {
                    match self.subset_snapshot_state.merged(&preview, &self.document) {
                        Ok(()) => {
                            self.status = i18n::registered(self.locale, "app-state.049").into()
                        }
                        Err(error) => self.status = error.localized(self.locale).into(),
                    }
                }
            }
        }
    }

    fn show_section_manager(&mut self, context: &egui::Context) {
        let action =
            self.section_manager
                .show(context, self.locale, &self.document, &self.selected);
        match action {
            Some(section_manager::Action::Add { name, short }) => {
                let next = self
                    .document
                    .sections
                    .iter()
                    .map(|section| section.id.get())
                    .max()
                    .unwrap_or(0)
                    .saturating_add(1);
                if let Some(id) = drill_core::SectionId::new(next) {
                    let palette = [
                        [64, 180, 255],
                        [255, 112, 132],
                        [120, 220, 145],
                        [190, 135, 255],
                    ];
                    let section = drill_core::Section {
                        id,
                        name,
                        short,
                        color: palette[self.document.sections.len() % palette.len()],
                        order: u16::try_from(self.document.sections.len()).unwrap_or(u16::MAX),
                    };
                    if self.execute_edit(
                        Edit::AddSection { section, at: None },
                        i18n::registered(self.locale, "app-state.050"),
                    ) {
                        self.section_manager.section_added();
                    }
                }
            }
            Some(section_manager::Action::Rename(id, name, short)) => {
                let failure = i18n::registered(self.locale, "app-state.051");
                self.execute_edit(Edit::RenameSection { id, name, short }, failure);
            }
            Some(section_manager::Action::Assign(section)) => {
                let assignments = self
                    .selected
                    .iter()
                    .filter_map(|&index| {
                        self.document.performers.get(index).map(|p| (p.id, section))
                    })
                    .collect();
                let failure = i18n::registered(self.locale, "app-state.052");
                self.execute_edit(Edit::AssignPerformersToSection { assignments }, failure);
            }
            Some(section_manager::Action::Remove(id, reassign_to))
                if self.execute_edit(
                    Edit::RemoveSection { id, reassign_to },
                    i18n::registered(self.locale, "app-state.053"),
                ) =>
            {
                self.section_manager.section_removed(id);
            }
            Some(section_manager::Action::Remove(..)) | None => {}
        }
    }

    /// Moves an editor-controlled playhead to an exact, valid count. Playback
    /// itself still advances through [`Self::seek_global`] with fractional
    /// count precision so visual/audio motion remains smooth.
    fn seek_to_count(&mut self, count: u32) {
        self.seek_global(count.min(self.document.timeline_counts()) as f32);
    }

    fn seek_global(&mut self, count: f32) {
        let (set_index, local_count) = self.document.locate_count(count);
        self.current_set = set_index;
        self.count_position = local_count;
    }

    /// [`Self::seek_to_count`] plus a visual glide, for the *rehearsal
    /// navigation* commands (set list, next/prev set, return to range start,
    /// Go To Count, production markers).
    ///
    /// The logical result is byte-for-byte what `seek_to_count` produces, on
    /// the same line -- the glide is bookkeeping on the side. Playback and the
    /// audio clock are untouched here: the callers still pause and seek audio
    /// straight to the destination, because a gliding audio scrub would be a
    /// bug, not a feature.
    fn navigation_seek_to_count(&mut self, count: u32) {
        let leaving = self
            .document
            .global_count(self.current_set, self.count_position);
        let target = count.min(self.document.timeline_counts()) as f32;
        self.nav_glide.begin(leaving, target);
        self.seek_global(target);
    }

    /// Steps every purely-visual animation for this frame and keeps the
    /// repaint loop alive for exactly as long as something is moving.
    ///
    /// Nothing in here writes logical state, so when everything is settled
    /// this costs one branch and the app is free to go fully idle.
    pub(crate) fn advance_view_motion(&mut self, context: &egui::Context, dt: f32) {
        // Two cases where easing the playhead would be wrong rather than
        // nice. During playback the playhead is already moving continuously,
        // and a second interpolation layered on top reads as a stutter. While
        // a pointer button is held the user is manipulating the field, and
        // `frame_positions` -- which is both what gets drawn and what drag and
        // hit-testing measure against -- must describe the set actually being
        // edited, not an in-between picture.
        if self.playing || context.input(|input| input.pointer.any_down()) {
            self.nav_glide.settle();
        }
        if self.nav_glide.advance(dt) {
            context.request_repaint();
        }
        if let Some(target) = self.timeline_follow_glide {
            let total = self.document.timeline_counts();
            if self.timeline_view.glide_start_toward(target, dt, total) {
                context.request_repaint();
            } else {
                self.timeline_follow_glide = None;
            }
        }
        if self.theme_fade.paint(context, dt) {
            context.request_repaint();
        }
    }

    /// Where the playhead should be *drawn* this frame, as
    /// `(set_index, local_count)`.
    ///
    /// Equal to the logical `(current_set, count_position)` unless a
    /// navigation glide is in flight. The conversion runs through
    /// `locate_count` every frame precisely because `count_position` is local
    /// to a set: interpolating it directly would run backwards, or off the
    /// end, the moment a jump crossed a set boundary.
    pub(crate) fn render_playhead(&self) -> (usize, f32) {
        self.nav_glide
            .position()
            .map_or((self.current_set, self.count_position), |global| {
                self.document.locate_count(global)
            })
    }

    /// A set's written coordinates are its arrival picture. Counts between
    /// set starts are a playback/interpolation preview, never a second
    /// mutable copy of that picture.
    fn is_editable_set_start(&self) -> bool {
        !self.playing && self.count_position.abs() <= f32::EPSILON
    }

    /// Shared formation-edit safety boundary for buttons, shortcuts, and
    /// pointer manipulation. Keeping it here prevents one interaction route
    /// from silently writing a mid-count preview into the document.
    fn ensure_editable_set_start(&mut self) -> bool {
        if self.is_editable_set_start() {
            true
        } else {
            self.status = i18n::registered(self.locale, "app-state.154").into();
            false
        }
    }

    /// Pause at the current set's exact start without disturbing the current
    /// working selection.
    fn return_to_editable_set_start(&mut self) {
        self.navigate_to_set(self.current_set);
        self.status = i18n::registered(self.locale, "app-state.155").into();
    }

    /// Jump to a set's first exact count without changing the working
    /// performer selection. Navigation is a rehearsal/viewing action, not a
    /// formation edit.
    fn navigate_to_set(&mut self, set_index: usize) {
        let Some(set) = self.document.sets.get(set_index) else {
            return;
        };
        let set_name = set.name.clone();
        let target = self.document.global_count(set_index, 0.0).round() as u32;
        self.navigation_seek_to_count(target);
        self.playing = false;
        self.audio_state.pause();
        if let Some(track) = &self.document.audio {
            self.audio_state
                .seek_seconds(drill_core::audio::count_to_audio_time(
                    track,
                    &self.document.tempo,
                    target as f32,
                ));
        }
        self.status = format!(
            "{}: {set_name}",
            i18n::registered(self.locale, "set-navigator.010"),
        );
    }

    fn navigate_relative_set(&mut self, direction: isize) {
        let target = if direction.is_negative() {
            self.current_set.checked_sub(direction.unsigned_abs())
        } else {
            self.current_set.checked_add(direction as usize)
        };
        if let Some(target) = target.filter(|&index| index < self.document.sets.len()) {
            self.navigate_to_set(target);
        }
    }

    /// Rehearsal navigation always lands on a whole global count, pauses both
    /// clocks, and leaves the current performer working group untouched.
    fn navigate_to_global_count(&mut self, target: u32) {
        let target = target.min(self.document.timeline_counts());
        self.navigation_seek_to_count(target);
        self.playing = false;
        self.audio_state.pause();
        if let Some(track) = &self.document.audio {
            self.audio_state
                .seek_seconds(drill_core::audio::count_to_audio_time(
                    track,
                    &self.document.tempo,
                    target as f32,
                ));
        }
        self.status = format!(
            "{} {}",
            i18n::registered(self.locale, "go-to-count.008"),
            target + 1,
        );
    }

    fn toggle_playback(&mut self, context: &egui::Context) {
        if self.playing {
            self.playing = false;
            self.audio_state.pause();
            return;
        }
        let global = self
            .document
            .global_count(self.current_set, self.count_position);
        if global < self.playback_start as f32 || global >= self.playback_end as f32 {
            self.seek_to_count(self.playback_start);
        }
        self.playing = self.playback_end > self.playback_start;
        if self.playing {
            self.ever_played = true;
            if let Some(track) = &self.document.audio {
                let global = self
                    .document
                    .global_count(self.current_set, self.count_position);
                let seconds =
                    drill_core::audio::count_to_audio_time(track, &self.document.tempo, global);
                self.audio_state.seek_seconds(seconds);
                self.audio_state.set_mix(track.gain_linear(), track.muted);
                self.audio_state.play();
            }
            // Advance playback is `dt`-based (see `controller::playback_decision`),
            // so it does not need a fixed 60fps tick -- it needs "as often as
            // vsync allows." `request_repaint_after(16ms)` hard-caps playback
            // at 60fps even on a 144/240Hz monitor; a bare `request_repaint()`
            // asks for the very next frame and lets the compositor pace it.
            // Only reached inside `if self.playing`, so this cannot busy-spin
            // while paused.
            context.request_repaint();
        }
    }

    fn selected_points(&self) -> Vec<Point> {
        self.selected
            .iter()
            .map(|&i| self.document.sets[self.current_set].positions[i])
            .collect()
    }

    fn copy_selected_formation(&mut self) {
        let entries = self
            .selected
            .iter()
            .filter_map(|&index| {
                self.document
                    .performers
                    .get(index)
                    .zip(self.document.sets[self.current_set].positions.get(index))
            })
            .map(|(performer, &point)| (performer.id, point))
            .collect::<Vec<_>>();
        if entries.is_empty() {
            self.status = i18n::registered(self.locale, "clipboard.001").into();
            return;
        }
        self.formation_clipboard.entries = entries;
        self.clipboard_paste_preview = None;
        self.clipboard_paste_targets_selection = false;
        self.status = format!(
            "{} {}",
            self.formation_clipboard.entries.len(),
            i18n::registered(self.locale, "clipboard.002")
        );
    }

    fn begin_clipboard_paste_preview(&mut self) {
        if !self.ensure_editable_set_start() || self.formation_clipboard.entries.is_empty() {
            if self.formation_clipboard.entries.is_empty() {
                self.status = i18n::registered(self.locale, "clipboard.003").into();
            }
            return;
        }
        let current_ids = self
            .document
            .performers
            .iter()
            .map(|performer| performer.id)
            .collect::<BTreeSet<_>>();
        let entries = self
            .formation_clipboard
            .entries
            .iter()
            .copied()
            .filter(|(id, _)| current_ids.contains(id) && !self.locked_performers.contains(id))
            .collect::<Vec<_>>();
        if entries.is_empty() {
            self.status = i18n::registered(self.locale, "clipboard.022").into();
            return;
        }
        // Selecting a different group with exactly the copied headcount means
        // "put this picture here".  Preserve the target group's centre, so a
        // paste transfers a form rather than unexpectedly teleporting it.
        // Sorting both groups by field position gives deterministic, visible
        // correspondence without relying on unstable selection click order.
        let selected_targets = self
            .selected
            .iter()
            .filter_map(|&index| {
                self.document.performers.get(index).and_then(|performer| {
                    self.document.sets[self.current_set]
                        .positions
                        .get(index)
                        .copied()
                        .map(|point| (performer.id, point))
                })
            })
            .filter(|(id, _)| !self.locked_performers.contains(id))
            .collect::<Vec<_>>();
        let source_ids = entries.iter().map(|(id, _)| *id).collect::<BTreeSet<_>>();
        let target_ids = selected_targets
            .iter()
            .map(|(id, _)| *id)
            .collect::<BTreeSet<_>>();
        let targets_selection = selected_targets.len() == entries.len() && target_ids != source_ids;
        let preview = if targets_selection {
            let source_centre = clipboard_centre(&entries);
            let target_centre = clipboard_centre(&selected_targets);
            let mut source_points = entries.iter().map(|(_, point)| *point).collect::<Vec<_>>();
            let mut targets = selected_targets;
            source_points.sort_by(point_field_order);
            targets.sort_by(|left, right| point_field_order(&left.1, &right.1));
            targets
                .into_iter()
                .zip(source_points)
                .map(|((id, _), source)| {
                    (
                        id,
                        Point {
                            x: target_centre.x + source.x - source_centre.x,
                            y: target_centre.y + source.y - source_centre.y,
                        },
                    )
                })
                .collect()
        } else {
            entries
        };
        self.clipboard_paste_targets_selection = targets_selection;
        self.clipboard_paste_preview = Some(preview);
        self.status = i18n::registered(self.locale, "clipboard.005").into();
    }

    fn cancel_clipboard_paste_preview(&mut self) {
        if self.clipboard_paste_preview.take().is_some() {
            self.clipboard_paste_targets_selection = false;
            self.status = i18n::registered(self.locale, "clipboard.006").into();
        }
    }

    fn apply_clipboard_paste_preview(&mut self) {
        let Some(entries) = self.clipboard_paste_preview.take() else {
            return;
        };
        self.clipboard_paste_targets_selection = false;
        if !self.ensure_editable_set_start() {
            return;
        }
        let set_id = self.document.sets[self.current_set].id;
        let mut performer_ids = Vec::with_capacity(entries.len());
        let mut positions = Vec::with_capacity(entries.len());
        for (id, point) in entries {
            if self
                .document
                .performers
                .iter()
                .any(|performer| performer.id == id)
            {
                performer_ids.push(id);
                positions.push(self.document.grid.snap(point));
            }
        }
        if performer_ids.is_empty() {
            self.status = i18n::registered(self.locale, "clipboard.004").into();
            return;
        }
        if self.execute_edit(
            Edit::MovePerformers {
                set_id,
                performer_ids,
                positions,
            },
            i18n::registered(self.locale, "clipboard.007"),
        ) {
            self.status = i18n::registered(self.locale, "clipboard.008").into();
        }
    }

    fn commit_layout(&mut self, points: Vec<Point>) {
        if !self.ensure_editable_set_start() {
            return;
        }
        let before = self.selected_points();
        let after = points
            .into_iter()
            .map(|point| {
                let point = self.document.grid.snap(point);
                Point {
                    x: point.x.clamp(0.0, self.document.grid.max_x()),
                    y: point.y.clamp(0.0, self.document.grid.max_y()),
                }
            })
            .collect::<Vec<_>>();
        if before == after {
            return;
        }
        let performer_ids = self
            .selected
            .iter()
            .filter_map(|&index| self.document.performers.get(index).map(|p| p.id))
            .collect();
        let set_id = self.document.sets[self.current_set].id;
        self.execute_edit(
            Edit::MovePerformers {
                set_id,
                performer_ids,
                positions: after,
            },
            i18n::registered(self.locale, "app-state.054"),
        );
    }

    /// The one authoritative route for all command-surface arrangement
    /// actions.  Toolbars and contextual controls may retain their compact
    /// direct affordances, while menus and the palette share this path so
    /// they cannot drift in editability, snapping, undo, or preview safety.
    fn arrange_selection(&mut self, command: UiCommand) {
        let points = self.selected_points();
        match command {
            UiCommand::AlignHorizontal => self.commit_layout(editing::align_horizontal(&points)),
            UiCommand::AlignVertical => self.commit_layout(editing::align_vertical(&points)),
            UiCommand::DistributeHorizontal => {
                self.commit_layout(editing::distribute_horizontal(&points))
            }
            UiCommand::DistributeVertical => {
                self.commit_layout(editing::distribute_vertical(&points))
            }
            UiCommand::FlipHorizontal => self.commit_layout(editing::flip_horizontal(&points)),
            UiCommand::FlipVertical => self.commit_layout(editing::flip_vertical(&points)),
            UiCommand::MakeLine => {
                if let Some((min, max)) = self.selection_bounds() {
                    let y = (min.y + max.y) * 0.5;
                    self.preview_shape(shapes::ShapeSpec::Line {
                        start: Point { x: min.x, y },
                        end: Point { x: max.x, y },
                    });
                }
            }
            _ => unreachable!("non-arrangement command routed to arrange_selection"),
        }
    }

    /// Moves the active working group by whole grid divisions. Every nudge
    /// shares the snapping, bounds, and one-Undo-transaction path of drag.
    fn nudge_selected(&mut self, horizontal_divisions: i32, vertical_divisions: i32) {
        if self.selected.is_empty() {
            return;
        }
        let grid = &self.document.grid;
        let dx = grid.horizontal_units / f32::from(grid.horizontal_steps.max(1));
        let dy = grid.vertical_units / f32::from(grid.vertical_steps.max(1));
        let points = self
            .selected_points()
            .into_iter()
            .map(|point| Point {
                x: (point.x + dx * horizontal_divisions as f32).clamp(0.0, grid.max_x()),
                y: (point.y + dy * vertical_divisions as f32).clamp(0.0, grid.max_y()),
            })
            .collect();
        let revision = self.history.revision();
        self.commit_layout(points);
        if self.history.revision() != revision {
            self.status = i18n::registered(self.locale, "app-state.153").into();
        }
    }

    /// Applies a Formation Designer result as one validated undo transaction,
    /// retaining its parametric source for later editing.
    fn commit_shape(&mut self, spec: shapes::ShapeSpec) {
        if !self.ensure_editable_set_start() {
            return;
        }
        if self.selected.is_empty() || spec.validate().is_err() {
            self.status = i18n::registered(self.locale, "app-state.055").into();
            return;
        }
        let mut sampled = Vec::with_capacity(self.selected.len());
        spec.sample(self.selected.len(), &mut sampled);
        let current = self.selected_points();
        // Assignment is computed against the unsnapped samples so minimal-
        // movement pairing isn't perturbed by quantization; the snapped
        // sequence below is only used for the final committed positions.
        let assignment = pathing::optimal_assignment(&current, &sampled);
        let mut next = self.document.clone();
        let grid = next.grid.clone();
        // Snapping the samples as a sequence (in the shape's own parametric
        // order, via error diffusion) keeps the placed curve smooth; see
        // `GridConfig::snap_sequence` doc comment for why per-point
        // independent rounding looks jagged here.
        let snapped = grid.snap_sequence(&sampled);
        let Some(set) = next.sets.get_mut(self.current_set) else {
            return;
        };
        for (rank, &index) in self.selected.iter().enumerate() {
            if let Some(&target) = assignment.get(rank).and_then(|&i| snapped.get(i)) {
                set.positions[index] = target;
            }
        }
        set.shape = Some(spec);
        if self.execute_edit(
            Edit::ReplaceDocument {
                document: Box::new(next),
            },
            i18n::registered(self.locale, "app-state.056"),
        ) {
            self.status = i18n::registered(self.locale, "app-state.057").into();
        }
    }

    fn preview_shape(&mut self, spec: shapes::ShapeSpec) {
        if self.selected.is_empty() || spec.validate().is_err() {
            return;
        }
        spec.sample(self.selected.len(), &mut self.formation_preview_points);
        self.formation_preview_spec = Some(spec);
    }

    fn formation_preview_is_closed(&self) -> bool {
        matches!(
            self.formation_preview_spec,
            Some(
                shapes::ShapeSpec::Circle { .. }
                    | shapes::ShapeSpec::Ellipse { .. }
                    | shapes::ShapeSpec::Star { .. }
                    | shapes::ShapeSpec::Polygon { .. }
                    | shapes::ShapeSpec::Cross { .. }
                    | shapes::ShapeSpec::Text { .. }
            )
        )
    }

    fn apply_shape_preview(&mut self) {
        if let Some(spec) = self.formation_preview_spec.take() {
            self.formation_preview_points.clear();
            self.free_draw_active = false;
            self.free_draw_raw.clear();
            self.commit_shape(spec);
        }
    }

    fn cancel_shape_preview(&mut self) {
        self.formation_preview_spec = None;
        self.formation_preview_points.clear();
        self.free_draw_active = false;
        self.free_draw_raw.clear();
        self.status = i18n::registered(self.locale, "app-state.058").into();
    }

    fn begin_free_draw(&mut self) {
        if !self.ensure_editable_set_start() {
            return;
        }
        // A form can be the starting point of a design. If nothing is selected,
        // choose the complete cast as the pending target; document positions
        // remain untouched until the user explicitly applies the preview.
        if self.selected.is_empty() {
            self.selected = (0..self.document.performers.len()).collect();
        }
        self.formation_preview_spec = None;
        self.formation_preview_points.clear();
        self.free_draw_raw.clear();
        self.free_draw_active = true;
        self.status = i18n::registered(self.locale, "app-state.059").into();
    }

    fn preview_formation_text(&mut self) {
        const NOTO_SANS_JP: &[u8] = include_bytes!("../../../assets/NotoSansJP.ttf");
        let points = self.selected_points();
        if points.is_empty() {
            return;
        }
        let min_x = points.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
        let min_y = points.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
        let max_y = points.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max);
        let options = drill_interop::font_outline::TextOutlineOptions {
            origin: Point { x: min_x, y: min_y },
            height: (max_y - min_y).max(8.0),
            ..Default::default()
        };
        match drill_interop::font_outline::outline_text(NOTO_SANS_JP, &self.formation_text, options)
        {
            Ok(outline) => {
                let warnings = outline.warnings.len();
                self.preview_shape(outline.shape);
                self.status = if warnings == 0 {
                    i18n::registered(self.locale, "app-state.060").into()
                } else {
                    format!(
                        "{}: {warnings}",
                        i18n::registered(self.locale, "app-state.061")
                    )
                };
            }
            Err(error) => {
                self.formation_preview_spec = None;
                self.formation_preview_points.clear();
                self.status = format!(
                    "{}: {error}",
                    i18n::registered(self.locale, "app-state.062")
                );
            }
        }
    }

    fn finish_free_draw_preview(&mut self) {
        self.free_draw_active = false;
        let tolerance = (self.document.grid.width / 800.0).max(0.05);
        let vertices = shapes::simplify_free_path(&self.free_draw_raw, tolerance);
        self.free_draw_raw.clear();
        if vertices.len() < 2 {
            self.status = i18n::registered(self.locale, "app-state.063").into();
            return;
        }
        self.preview_shape(shapes::ShapeSpec::FreePath { vertices });
        self.status = i18n::registered(self.locale, "app-state.064").into();
    }

    fn commit_designer_positions(
        &mut self,
        positions: Vec<Point>,
        shape: Option<shapes::ShapeSpec>,
        success_ja: &'static str,
        success_en: &'static str,
    ) {
        if !self.ensure_editable_set_start() {
            return;
        }
        if positions.len() != self.selected.len()
            || positions
                .iter()
                .any(|p| !p.x.is_finite() || !p.y.is_finite())
        {
            self.status = i18n::registered(self.locale, "app-state.065").into();
            return;
        }
        let mut next = self.document.clone();
        let grid = next.grid.clone();
        let Some(set) = next.sets.get_mut(self.current_set) else {
            return;
        };
        for (&index, point) in self.selected.iter().zip(positions) {
            set.positions[index] = grid.snap(point);
        }
        set.shape = shape;
        if self.execute_edit(
            Edit::ReplaceDocument {
                document: Box::new(next),
            },
            i18n::registered(self.locale, "app-state.066"),
        ) {
            self.status = tr(self.locale, success_ja, success_en).into();
        }
    }

    fn apply_morph_preview(&mut self, amount: f32) {
        let Some(next_set) = self.document.sets.get(self.current_set + 1) else {
            self.status = i18n::registered(self.locale, "app-state.067").into();
            return;
        };
        let from = self.selected_points();
        let to = self
            .selected
            .iter()
            .filter_map(|&i| next_set.positions.get(i).copied())
            .collect::<Vec<_>>();
        let preview = shapes::morph(&from, &to, amount);
        self.commit_designer_positions(
            preview,
            None,
            "次セットへのモーフを適用しました",
            "Morph toward next set applied",
        );
    }

    fn apply_radial_selection(&mut self, fold: u32) {
        use std::collections::BTreeMap;
        let ids = self
            .selected
            .iter()
            .filter_map(|&i| self.document.performers.get(i).map(|p| p.id))
            .collect::<Vec<_>>();
        if ids.len() < fold as usize {
            self.status = i18n::registered(self.locale, "app-state.068").into();
            return;
        }
        let center = editing::centroid(&self.selected_points());
        let masters = ids.len().div_ceil(fold as usize);
        let groups = (0..masters)
            .map(|i| {
                (0..fold as usize)
                    .filter_map(|k| ids.get(i + k * masters).copied())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let mut map: BTreeMap<_, _> = ids
            .iter()
            .zip(self.selected_points())
            .map(|(&id, p)| (id, p))
            .collect();
        shapes::apply_radial(center, fold, &groups, &mut map);
        let points = ids.iter().filter_map(|id| map.get(id).copied()).collect();
        self.commit_designer_positions(
            points,
            None,
            "放射対称を適用しました",
            "Radial symmetry applied",
        );
    }

    fn apply_constraint_cleanup(&mut self) {
        use drill_core::constraint_solver::{Constraint, SolverLimits, solve_constraints};
        let initial = self.selected_points();
        if initial.len() < 2 {
            self.status = i18n::registered(self.locale, "app-state.069").into();
            return;
        }
        let mut constraints = vec![
            Constraint::InsideField {
                min: Point { x: 0.0, y: 0.0 },
                max: Point {
                    x: self.document.grid.width,
                    y: self.document.grid.height,
                },
            },
            Constraint::MinimumDistance { distance: 1.5 },
        ];
        if self.current_set > 0 {
            let previous = &self.document.sets[self.current_set - 1].positions;
            let origins = self
                .selected
                .iter()
                .filter_map(|&i| previous.get(i).copied())
                .collect::<Vec<_>>();
            if origins.len() == initial.len() {
                constraints.push(Constraint::MaximumStep {
                    origins,
                    distance: f32::from(self.document.sets[self.current_set].counts).max(1.0) * 1.5,
                });
            }
        }
        if let Some(spec) = self.document.sets[self.current_set].shape.as_ref() {
            let mut targets = Vec::new();
            spec.sample(initial.len(), &mut targets);
            if targets.len() == initial.len() {
                constraints.push(Constraint::ShapeFollow {
                    targets,
                    strength: 0.15,
                });
            }
        }
        match solve_constraints(&initial, &constraints, SolverLimits::default(), |_| true) {
            Ok(proposal) => {
                let converged = proposal.converged;
                let iterations = proposal.iterations;
                self.commit_layout(proposal.positions);
                self.status = format!(
                    "{} · {} {}{}",
                    i18n::registered(self.locale, "app-state.070"),
                    iterations,
                    i18n::registered(self.locale, "app-state.071"),
                    if converged {
                        ""
                    } else {
                        i18n::registered(self.locale, "app-state.072")
                    }
                );
            }
            Err(error) => {
                self.status = format!(
                    "{}: {error}",
                    i18n::registered(self.locale, "app-state.073")
                )
            }
        }
    }

    fn apply_section_shape_assignment(&mut self) {
        use std::collections::BTreeMap;
        let Some(spec) = self
            .document
            .sets
            .get(self.current_set)
            .and_then(|s| s.shape.clone())
        else {
            self.status = i18n::registered(self.locale, "app-state.074").into();
            return;
        };
        let selected_ids = self
            .selected
            .iter()
            .filter_map(|&i| self.document.performers.get(i).map(|p| p.id))
            .collect::<Vec<_>>();
        let mut groups = Vec::new();
        for section in &self.document.sections {
            let performers = selected_ids
                .iter()
                .copied()
                .filter(|id| {
                    self.document
                        .performers
                        .iter()
                        .any(|p| p.id == *id && p.section == section.id)
                })
                .collect::<Vec<_>>();
            if !performers.is_empty() {
                groups.push(shapes::AssignmentGroup {
                    section: section.id,
                    performers,
                });
            }
        }
        let current: BTreeMap<_, _> = selected_ids
            .iter()
            .zip(self.selected_points())
            .map(|(&id, p)| (id, p))
            .collect();
        let mut targets = Vec::new();
        spec.sample(selected_ids.len(), &mut targets);
        let (assigned, unplaced) = shapes::assign_to_shape(&groups, &targets, &current);
        if !unplaced.is_empty() || assigned.len() != selected_ids.len() {
            self.status = i18n::registered(self.locale, "app-state.075").into();
            return;
        }
        let points = selected_ids
            .iter()
            .filter_map(|id| assigned.get(id).copied())
            .collect();
        self.commit_designer_positions(
            points,
            Some(spec),
            "セクションを保って再配置しました",
            "Reassigned while preserving sections",
        );
    }

    fn selection_bounds(&self) -> Option<(Point, Point)> {
        let points = self.selected_points();
        let first = *points.first()?;
        Some(
            points
                .iter()
                .skip(1)
                .fold((first, first), |(min, max), point| {
                    (
                        Point {
                            x: min.x.min(point.x),
                            y: min.y.min(point.y),
                        },
                        Point {
                            x: max.x.max(point.x),
                            y: max.y.max(point.y),
                        },
                    )
                }),
        )
    }

    fn transform_selection(&mut self, scale: f32, angle: f32) {
        let points = self.selected_points();
        if points.is_empty() {
            return;
        }
        let center = Point {
            x: points.iter().map(|p| p.x).sum::<f32>() / points.len() as f32,
            y: points.iter().map(|p| p.y).sum::<f32>() / points.len() as f32,
        };
        let (sin, cos) = angle.sin_cos();
        let transformed = points
            .into_iter()
            .map(|point| {
                let x = (point.x - center.x) * scale;
                let y = (point.y - center.y) * scale;
                Point {
                    x: (center.x + x * cos - y * sin).clamp(0.0, self.document.grid.max_x()),
                    y: (center.y + x * sin + y * cos).clamp(0.0, self.document.grid.max_y()),
                }
            })
            .collect();
        self.commit_layout(transformed);
    }

    fn export_text(&mut self, default_name: &str, filter_name: &str, ext: &str, contents: String) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter(filter_name, &[ext])
            .set_file_name(default_name)
            .save_file()
        {
            match self.text_export_state.start(path.clone(), contents) {
                Ok(()) => {
                    self.status = format!(
                        "{}: {}",
                        i18n::registered(self.locale, "app-state.076"),
                        path.display()
                    )
                }
                Err(error) => {
                    self.status = format!(
                        "{}: {}",
                        i18n::registered(self.locale, "app-state.077"),
                        error.localized(self.locale)
                    )
                }
            }
        }
    }

    /// Reassign the next set's dots to the current performers so total travel is
    /// minimized, keeping the target formation shape but reducing crossings.
    fn auto_assign_next(&mut self) {
        let next_index = self.current_set + 1;
        let Some(next) = self.document.sets.get(next_index) else {
            self.status = i18n::registered(self.locale, "app-state.145").into();
            return;
        };
        let from = &self.document.sets[self.current_set].positions;
        let before = next.positions.clone();
        let assignment = pathing::optimal_assignment(from, &before);
        let after = assignment.iter().map(|&j| before[j]).collect::<Vec<_>>();
        if after == before {
            self.status = i18n::registered(self.locale, "app-state.146").into();
            return;
        }
        let set_id = self.document.sets[next_index].id;
        let performer_ids = self.document.performers.iter().map(|p| p.id).collect();
        if self.execute_edit(
            Edit::MovePerformers {
                set_id,
                performer_ids,
                positions: after,
            },
            i18n::registered(self.locale, "app-state.078"),
        ) {
            self.status = i18n::registered(self.locale, "app-state.079").into();
        }
    }

    /// Read-only 3D stadium visualization of the current frame. Editing stays in 2D.
    fn save_dialog(&mut self) {
        let path = self
            .current_path
            .clone()
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .or_else(|| {
                rfd::FileDialog::new()
                    .add_filter("DrillForge", &["drill.json"])
                    .set_file_name("untitled.drill.json")
                    .save_file()
            });
        if let Some(path) = path {
            self.project_state
                .save_legacy_json(path, self.document.clone());
        }
    }

    /// Always ask for a new JSON destination.  This deliberately differs from
    /// Save, which reuses the current legacy JSON path when there is one.
    fn save_as_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("DrillForge", &["drill.json"])
            .set_file_name("untitled.drill.json")
            .save_file()
        {
            self.project_state
                .save_legacy_json(path, self.document.clone());
        }
    }

    fn save_project_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("DrillForge Project", &["drillproj"])
            .set_file_name("untitled.drillproj")
            .save_file()
        {
            self.project_state.save_project(
                path,
                self.document.clone(),
                self.embed_audio_in_project,
                self.underlay_state.asset_bytes.clone(),
            );
        }
    }

    fn request_open_document(&mut self, kind: DocumentOpenKind) {
        self.request_open_target(DocumentOpenTarget::Dialog(kind));
    }

    fn request_open_recent(&mut self, path: PathBuf) {
        self.request_open_target(DocumentOpenTarget::Recent(path));
    }

    fn request_open_target(&mut self, target: DocumentOpenTarget) {
        if self.dirty {
            self.document_open_guard = DocumentOpenGuard::Prompt(target);
        } else {
            self.begin_open_target(target);
        }
    }

    fn begin_open_document(&mut self, kind: DocumentOpenKind) {
        match kind {
            DocumentOpenKind::LegacyJson => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("DrillForge", &["json"])
                    .pick_file()
                {
                    self.project_state.load_legacy_json(path);
                }
            }
            DocumentOpenKind::Project => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("DrillForge Project", &["drillproj"])
                    .pick_file()
                {
                    self.project_state.load_project(path);
                }
            }
        }
    }

    fn begin_open_target(&mut self, target: DocumentOpenTarget) {
        match target {
            DocumentOpenTarget::Dialog(kind) => self.begin_open_document(kind),
            DocumentOpenTarget::Recent(path) => {
                if !path.is_file() {
                    self.recent_projects.remove(&path);
                    self.status = i18n::registered(self.locale, "recent-projects.005").into();
                } else if path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("drillproj"))
                {
                    self.project_state.load_project(path);
                } else {
                    self.project_state.load_legacy_json(path);
                }
            }
        }
    }
}

impl DrillApp {
    fn import_coordinates_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Coordinate table", &["csv", "tsv", "txt", "xlsx"])
            .pick_file()
        {
            self.import_state.choose(path);
        }
    }

    fn import_musical_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Music timeline", &["musicxml", "mxl", "xml", "mid", "midi"])
            .pick_file()
        {
            self.import_state.choose_musical(path);
        }
    }

    fn load_underlay_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Image", &["png", "jpg", "jpeg"])
            .pick_file()
        {
            self.underlay_state.load(path);
        }
    }

    fn show_musical_import_window(&mut self, context: &egui::Context) {
        if !self.import_state.musical_open {
            return;
        }
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new(i18n::registered(self.locale, "app-state.080"))
            .collapsible(false)
            .show(context, |ui| {
                ui.label(self.import_state.status.text(self.locale));
                if let Some((source, result)) = self.import_state.musical_review() {
                    ui.heading(source);
                    ui.label(format!(
                        "{}: {}",
                        i18n::registered(self.locale, "app-state.081"),
                        result.timeline.tempo.events().len()
                    ));
                    ui.label(format!(
                        "{}: {} / {}: {}",
                        i18n::registered(self.locale, "app-state.082"),
                        result.timeline.meters.len(),
                        i18n::registered(self.locale, "app-state.083"),
                        result.timeline.total_counts
                    ));
                    ui.label(format!(
                        "{}: {} / {}: {}",
                        i18n::registered(self.locale, "app-state.084"),
                        result.timeline.measures.len(),
                        i18n::registered(self.locale, "app-state.085"),
                        result.timeline.marks.len()
                    ));
                    ui.small(i18n::registered(self.locale, "app-state.086"));
                    for event in result.timeline.tempo.events().iter().take(12) {
                        ui.monospace(format!(
                            "Count {:>7.2}  {:>7.2} BPM",
                            event.count, event.bpm
                        ));
                    }
                    if result.timeline.tempo.events().len() > 12 {
                        ui.small("…");
                    }
                    if !result.warnings.is_empty() {
                        ui.colored_label(
                            Color32::YELLOW,
                            format!(
                                "{}: {:?}",
                                i18n::registered(self.locale, "app-state.087"),
                                result.warnings
                            ),
                        );
                    }
                    ui.horizontal(|ui| {
                        if ui
                            .button(i18n::registered(self.locale, "app-state.088"))
                            .clicked()
                        {
                            apply = true;
                        }
                        if ui
                            .button(i18n::registered(self.locale, "app-state.089"))
                            .clicked()
                        {
                            cancel = true;
                        }
                    });
                } else if ui
                    .button(i18n::registered(self.locale, "app-state.090"))
                    .clicked()
                {
                    cancel = true;
                }
            });
        if apply && let Some(timeline) = self.import_state.take_musical_timeline() {
            let mut document = self.document.clone();
            document.tempo = timeline.tempo;
            let starts = (0..document.sets.len())
                .map(|i| document.global_count(i, 0.0))
                .collect::<Vec<_>>();
            for (i, set) in document.sets.iter_mut().enumerate() {
                set.annotation.tempo_bpm = Some(document.tempo.bpm_at(starts[i]));
            }
            for mark in timeline.marks {
                if let Some(index) = starts
                    .iter()
                    .enumerate()
                    .min_by(|(_, a), (_, b)| {
                        (mark.count - **a)
                            .abs()
                            .total_cmp(&(mark.count - **b).abs())
                    })
                    .map(|(i, _)| i)
                {
                    document.sets[index].annotation.rehearsal_mark = mark.text;
                }
            }
            match self.history.execute(
                &mut self.document,
                Edit::ReplaceDocument {
                    document: Box::new(document),
                },
            ) {
                Ok(()) => {
                    self.tempo_bpm = self.document.tempo.bpm_at(0.0);
                    self.tempo_draft = Some(self.document.tempo.clone());
                    self.tempo_draft_dirty = false;
                    self.dirty = true;
                    self.status = i18n::registered(self.locale, "app-state.091").into();
                }
                Err(error) => {
                    self.status = format!(
                        "{}: {error}",
                        i18n::registered(self.locale, "app-state.092")
                    )
                }
            }
        }
        if cancel {
            self.import_state.cancel_musical();
        }
    }

    fn show_import_window(&mut self, context: &egui::Context) {
        if !self.import_state.open {
            return;
        }
        let mut open = true;
        let preview = self.import_state.preview().cloned();
        let xlsx_names = self.import_state.xlsx_sheet_names();
        let selected_xlsx = self.import_state.selected_xlsx_sheet();
        let phrase_diagnostics = self.import_state.phrase_preview(&self.document.grid);
        let mut choose_xlsx = None;
        let locale = self.locale;
        let mut start_review = false;
        let mut request_confirmation = false;
        let mut apply = false;
        egui::Window::new(i18n::registered(self.locale, "app-state.093"))
            .open(&mut open)
            .resizable(true)
            .default_width(760.0)
            .show(context, |ui| {
                ui.heading(
                    self.import_state
                        .source_name()
                        .unwrap_or_else(|| "座標表".into()),
                );
                ui.label(i18n::registered(self.locale, "app-state.094"));
                ui.label(self.import_state.status.text(self.locale));
                if let Some(preview) = &preview {
                    ui.separator();
                    if let Some(selected) = selected_xlsx {
                        ui.horizontal(|ui| {
                            ui.label(i18n::registered(locale, "app-state.095"));
                            egui::ComboBox::from_id_salt("xlsx-sheet")
                                .selected_text(xlsx_names.get(selected).map_or("?", String::as_str))
                                .show_ui(ui, |ui| {
                                    for (i, name) in xlsx_names.iter().enumerate() {
                                        if ui.selectable_label(i == selected, name).clicked() {
                                            choose_xlsx = Some(i);
                                        }
                                    }
                                });
                        });
                    }
                    ui.label(format!(
                        "{}: {} · {}: {}{}",
                        i18n::registered(self.locale, "app-state.097"),
                        preview.total_rows,
                        i18n::registered(self.locale, "app-state.098"),
                        match preview.delimiter {
                            drill_interop::Delimiter::Comma => "CSV (,)",
                            drill_interop::Delimiter::Tab => "TSV (Tab)",
                            drill_interop::Delimiter::Semicolon => "CSV (;)",
                        },
                        if preview.truncated {
                            i18n::registered(self.locale, "app-state.096")
                        } else {
                            ""
                        }
                    ));
                    if preview.replacement_characters {
                        ui.colored_label(
                            Color32::YELLOW,
                            i18n::registered(self.locale, "app-state.099"),
                        );
                    }
                    if let Some(mapping) = self.import_state.mapping_mut() {
                        egui::Grid::new("import-column-map")
                            .num_columns(2)
                            .striped(true)
                            .show(ui, |ui| {
                                for (label, index) in [
                                    ("Performer / 演者", &mut mapping.performer),
                                    ("Set / セット", &mut mapping.set),
                                    ("X / 横・左右座標文", &mut mapping.x),
                                    ("Y / 縦・前後座標文", &mut mapping.y),
                                ] {
                                    ui.label(label);
                                    egui::ComboBox::from_id_salt(label)
                                        .selected_text(
                                            preview.headers.get(*index).map_or("?", String::as_str),
                                        )
                                        .show_ui(ui, |ui| {
                                            for (i, header) in preview.headers.iter().enumerate() {
                                                ui.selectable_value(index, i, header);
                                            }
                                        });
                                    ui.end_row();
                                }
                                ui.label(i18n::registered(self.locale, "app-state.147"));
                                optional_column_combo(
                                    ui,
                                    "import-counts",
                                    &preview.headers,
                                    &mut mapping.counts,
                                );
                                ui.end_row();
                                ui.label(i18n::registered(self.locale, "app-state.148"));
                                optional_column_combo(
                                    ui,
                                    "import-section",
                                    &preview.headers,
                                    &mut mapping.section,
                                );
                                ui.end_row();
                            });
                    }
                    if !phrase_diagnostics.is_empty() {
                        ui.label(i18n::registered(locale, "app-state.100"));
                        for diagnostic in phrase_diagnostics.iter().take(8) {
                            if let Some(point) = diagnostic.point {
                                ui.colored_label(
                                    Color32::LIGHT_GREEN,
                                    format!(
                                        "✓ {} {} → ({:.2}, {:.2})",
                                        i18n::registered(locale, "app-state.101"),
                                        diagnostic.line,
                                        point.x,
                                        point.y
                                    ),
                                );
                            } else if let Some(error) = &diagnostic.error {
                                ui.colored_label(
                                    Color32::YELLOW,
                                    format!(
                                        "⚠ {} {}: {error}",
                                        i18n::registered(locale, "app-state.102"),
                                        diagnostic.line
                                    ),
                                );
                            }
                        }
                    }
                    ui.separator();
                    ui.label(i18n::registered(self.locale, "app-state.103"));
                    egui::ScrollArea::both().max_height(230.0).show(ui, |ui| {
                        egui::Grid::new("import-preview-grid")
                            .striped(true)
                            .show(ui, |ui| {
                                for header in &preview.headers {
                                    ui.strong(header);
                                }
                                ui.end_row();
                                for row in &preview.rows {
                                    for i in 0..preview.headers.len() {
                                        ui.label(row.get(i).map_or("", String::as_str));
                                    }
                                    ui.end_row();
                                }
                            });
                    });
                    ui.label(i18n::registered(self.locale, "app-state.104"));
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                !self.import_state.busy(),
                                egui::Button::new(i18n::registered(self.locale, "app-state.105")),
                            )
                            .clicked()
                        {
                            start_review = true;
                        }
                        if ui
                            .button(i18n::registered(self.locale, "app-state.106"))
                            .clicked()
                        {
                            self.import_state.cancel();
                        }
                    });
                } else if let Some(review) = self.import_state.review_mut() {
                    let diff = &review.diff;
                    ui.separator();
                    ui.heading(i18n::registered(locale, "app-state.107"));
                    ui.label(format!(
                        "{} +{}  ·  {} +{} / Δ{}  ·  {} {}",
                        i18n::registered(locale, "app-state.108"),
                        diff.performers_added,
                        i18n::registered(locale, "app-state.109"),
                        diff.sets_added,
                        diff.sets_changed,
                        diff.coordinates_moved,
                        i18n::registered(locale, "app-state.110")
                    ));
                    if diff.performers_changed > 0 {
                        ui.label(format!(
                            "{} Δ{}",
                            i18n::registered(locale, "app-state.111"),
                            diff.performers_changed
                        ));
                    }
                    for warning in &diff.warnings {
                        ui.colored_label(Color32::YELLOW, format!("⚠ {warning}"));
                    }
                    for warning in
                        review
                            .outcome
                            .report
                            .warnings
                            .iter()
                            .map(|warning| match warning {
                                drill_interop::ImportWarning::CountsDefaulted { .. } => {
                                    "Counts defaulted for one or more sets"
                                }
                                drill_interop::ImportWarning::HeldPreviousSet { .. } => {
                                    "Missing rows use a neighboring set position"
                                }
                                drill_interop::ImportWarning::SectionCreated(_) => {
                                    "Imported sections apply with selected performer rows"
                                }
                                drill_interop::ImportWarning::ReplacementCharacters => {
                                    "Replacement characters were detected"
                                }
                            })
                    {
                        ui.colored_label(Color32::YELLOW, format!("⚠ {warning}"));
                    }
                    ui.horizontal(|ui| {
                        if ui
                            .small_button(i18n::registered(locale, "app-state.112"))
                            .clicked()
                        {
                            review.selection = drill_interop::DiffSelection::all(diff);
                        }
                        if ui
                            .small_button(i18n::registered(locale, "app-state.113"))
                            .clicked()
                        {
                            review.selection = drill_interop::DiffSelection::none(diff);
                        }
                    });
                    ui.label(i18n::registered(locale, "app-state.114"));
                    egui::ScrollArea::horizontal()
                        .max_height(70.0)
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                for (index, set) in review.outcome.document.sets.iter().enumerate()
                                {
                                    ui.checkbox(&mut review.selection.sets[index], &set.name);
                                }
                            });
                        });
                    ui.label(i18n::registered(locale, "app-state.115"));
                    let row_height = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
                    egui::ScrollArea::vertical().max_height(260.0).show_rows(
                        ui,
                        row_height,
                        diff.rows.len(),
                        |ui, range| {
                            for index in range {
                                let row = diff.rows[index];
                                let set = &review.outcome.document.sets[row.imported_set].name;
                                let performer = &review.outcome.document.performers
                                    [row.imported_performer]
                                    .label;
                                ui.horizontal(|ui| {
                                    ui.add_enabled_ui(
                                        !review.selection.sets[row.imported_set],
                                        |ui| {
                                            ui.checkbox(&mut review.selection.rows[index], "");
                                        },
                                    );
                                    ui.monospace(format!("{set} · {performer}"));
                                    match row.before {
                                        Some(before) => ui.label(format!(
                                            "({:.2}, {:.2}) → ({:.2}, {:.2})",
                                            before.x, before.y, row.after.x, row.after.y
                                        )),
                                        None => ui.label(format!(
                                            "{} ({:.2}, {:.2})",
                                            i18n::registered(locale, "app-state.116"),
                                            row.after.x,
                                            row.after.y
                                        )),
                                    };
                                });
                            }
                        },
                    );
                    let selected_rows = diff
                        .rows
                        .iter()
                        .enumerate()
                        .filter(|(i, row)| {
                            review.selection.sets[row.imported_set] || review.selection.rows[*i]
                        })
                        .count();
                    let selected_set_changes =
                        review.selection.sets.iter().any(|selected| *selected)
                            && (diff.sets_added > 0
                                || diff.sets_changed > 0
                                || diff.performers_changed > 0);
                    ui.label(format!(
                        "{}: {}",
                        i18n::registered(locale, "app-state.117"),
                        selected_rows
                    ));
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                (selected_rows > 0 || selected_set_changes)
                                    && !self.import_state.busy(),
                                egui::Button::new(i18n::registered(locale, "app-state.118")),
                            )
                            .clicked()
                        {
                            request_confirmation = true;
                        }
                        if ui
                            .button(i18n::registered(locale, "app-state.119"))
                            .clicked()
                        {
                            self.import_state.cancel();
                        }
                    });
                }
                if self.import_state.confirming {
                    ui.separator();
                    ui.colored_label(
                        Color32::from_rgb(255, 210, 90),
                        i18n::registered(locale, "app-state.120"),
                    );
                    ui.horizontal(|ui| {
                        if ui
                            .button(i18n::registered(locale, "app-state.121"))
                            .clicked()
                        {
                            apply = true;
                        }
                        if ui
                            .button(i18n::registered(locale, "app-state.122"))
                            .clicked()
                        {
                            self.import_state.confirming = false;
                        }
                    });
                }
            });
        if let Some(index) = choose_xlsx
            && let Err(error) = self.import_state.select_xlsx_sheet(index)
        {
            self.status = format!(
                "{}: {}",
                i18n::registered(locale, "app-state.123"),
                error.localized(locale)
            );
        }
        if start_review {
            self.import_state
                .start_review(self.document.grid.clone(), self.document.clone());
        }
        if request_confirmation {
            self.import_state.confirming = true;
        }
        if apply && let Err(error) = self.import_state.apply_selected(&self.document) {
            self.status = error.into();
        }
        if !open {
            self.import_state.cancel();
        }
    }
}

fn optional_column_combo(
    ui: &mut egui::Ui,
    id: &str,
    headers: &[String],
    value: &mut Option<usize>,
) {
    let selected = value
        .and_then(|i| headers.get(i))
        .map_or("—", String::as_str);
    egui::ComboBox::from_id_salt(id)
        .selected_text(selected)
        .show_ui(ui, |ui| {
            ui.selectable_value(value, None, "—");
            for (i, header) in headers.iter().enumerate() {
                ui.selectable_value(value, Some(i), header);
            }
        });
}

// Kept during the staged renderer migration as a visual parity reference.
#[allow(dead_code)]
fn draw_field(painter: &egui::Painter, rect: Rect, grid: &GridConfig) {
    painter.rect_filled(rect, 4.0, Color32::from_rgb(25, 71, 45));
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(2.0, Color32::from_gray(210)),
        StrokeKind::Inside,
    );
    let to_x = |x: f32| rect.left() + x / grid.width * rect.width();
    let to_y = |y: f32| rect.top() + y / grid.height * rect.height();
    let mut unit = 0.0;
    while unit <= grid.width + 0.001 {
        let x = to_x(unit);
        let major = (unit / (grid.major_line_interval * 2.0)).fract().abs() < 0.001;
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(
                if major { 1.2 } else { 0.5 },
                Color32::from_white_alpha(if major { 120 } else { 55 }),
            ),
        );
        if major && unit > 0.0 && unit < grid.width {
            painter.text(
                Pos2::new(x, rect.top() + 8.0),
                egui::Align2::CENTER_TOP,
                format!("{unit:.0}"),
                egui::FontId::proportional(13.0),
                Color32::from_white_alpha(170),
            );
        }
        unit += grid.major_line_interval.max(0.25);
    }

    if grid.show_step_grid {
        let dx = grid.horizontal_units / grid.horizontal_steps.max(1) as f32;
        let dy = grid.vertical_units / grid.vertical_steps.max(1) as f32;
        match grid.style {
            GridStyle::Lines => {
                let mut x = dx;
                while x < grid.width {
                    painter.line_segment(
                        [
                            Pos2::new(to_x(x), rect.top()),
                            Pos2::new(to_x(x), rect.bottom()),
                        ],
                        Stroke::new(0.35, Color32::from_white_alpha(28)),
                    );
                    x += dx;
                }
                let mut y = dy;
                while y < grid.height {
                    painter.line_segment(
                        [
                            Pos2::new(rect.left(), to_y(y)),
                            Pos2::new(rect.right(), to_y(y)),
                        ],
                        Stroke::new(0.35, Color32::from_white_alpha(28)),
                    );
                    y += dy;
                }
            }
            GridStyle::Dots => {
                let mut y = dy;
                while y < grid.height {
                    let mut x = dx;
                    while x < grid.width {
                        painter.circle_filled(
                            Pos2::new(to_x(x), to_y(y)),
                            0.8,
                            Color32::from_white_alpha(55),
                        );
                        x += dx;
                    }
                    y += dy;
                }
            }
        }
    }

    for hash in &grid.hashes {
        let y = to_y(hash.position.clamp(0.0, grid.height));
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            Stroke::new(hash.weight.clamp(0.25, 4.0), Color32::from_white_alpha(120)),
        );
        painter.text(
            Pos2::new(rect.left() + 5.0, y - 3.0),
            egui::Align2::LEFT_BOTTOM,
            &hash.label,
            egui::FontId::proportional(10.0),
            Color32::from_white_alpha(150),
        );
    }
}

fn utc_month_day() -> (u8, u8) {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 86_400;
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    (month as u8, day as u8)
}

#[cfg(test)]
mod locale_regression_tests {
    use super::*;

    fn contains_japanese(value: &str) -> bool {
        value.chars().any(|ch| {
            matches!(ch,
                '\u{3040}'..='\u{30ff}' |
                '\u{3400}'..='\u{4dbf}' |
                '\u{4e00}'..='\u{9fff}')
        })
    }

    #[test]
    fn english_primary_workflow_copy_has_no_japanese_glyphs() {
        let labels = [
            i18n::registered(Locale::En, "app-state.124"),
            i18n::registered(Locale::En, "app-state.125"),
            i18n::registered(Locale::En, "app-state.126"),
            i18n::registered(Locale::En, "app-state.127"),
            i18n::registered(Locale::En, "app-state.128"),
            i18n::registered(Locale::En, "app-state.129"),
            i18n::registered(Locale::En, "app-state.130"),
            i18n::registered(Locale::En, "app-state.131"),
            i18n::registered(Locale::En, "app-state.132"),
            i18n::registered(Locale::En, "app-state.133"),
            i18n::registered(Locale::En, "app-state.134"),
            i18n::registered(Locale::En, "app-state.135"),
        ];
        for label in labels {
            assert!(
                !contains_japanese(label),
                "Japanese leaked into English UI: {label}"
            );
        }
    }
}
