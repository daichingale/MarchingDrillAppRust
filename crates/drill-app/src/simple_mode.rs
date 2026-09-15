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
enum MetronomeFailure {
    Output,
    Schedule,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum SimpleScreen {
    #[default]
    Wizard,
    Metronome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum SimpleStep {
    #[default]
    ChooseFormation,
    SelectPerformers,
    MovePerformers,
    Playback,
}

impl SimpleStep {
    const ALL: [Self; 4] = [
        Self::ChooseFormation,
        Self::SelectPerformers,
        Self::MovePerformers,
        Self::Playback,
    ];

    fn index(self) -> usize {
        Self::ALL.iter().position(|&step| step == self).unwrap_or(0)
    }

    fn next(self) -> Self {
        Self::ALL[(self.index() + 1).min(Self::ALL.len() - 1)]
    }

    fn previous(self) -> Self {
        Self::ALL[self.index().saturating_sub(1)]
    }

    fn title(self, locale: Locale) -> &'static str {
        let id = match self {
            Self::ChooseFormation => "simple-mode.001",
            Self::SelectPerformers => "simple-mode.002",
            Self::MovePerformers => "simple-mode.003",
            Self::Playback => "simple-mode.004",
        };
        i18n::registered(locale, id)
    }

    fn guidance(self, locale: Locale) -> &'static str {
        let id = match self {
            Self::ChooseFormation => "simple-mode.005",
            Self::SelectPerformers => "simple-mode.006",
            Self::MovePerformers => "simple-mode.007",
            Self::Playback => "simple-mode.008",
        };
        i18n::registered(locale, id)
    }
}

/// Standalone metronome, independent of the open `Document`. Reuses the
/// existing `drill_audio` click-generation/output primitives (the same ones
/// `AudioState::configure_click` drives) rather than inventing a new audio
/// pipeline; this module only orchestrates them.
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

    fn error(&self) -> Option<MetronomeFailure> {
        self.error
    }

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

/// State for the drag-to-pan, 5m-square editing viewport used only by the
/// "move performers" step. Kept separate from the full-field 2D view used by
/// every other simple-mode screen and by the normal desktop UI.
#[derive(Default)]
struct EditViewport {
    center: Option<Point>,
}

#[derive(Default)]
pub(crate) struct SimpleModeState {
    pub enabled: bool,
    pub step: SimpleStep,
    pub screen: SimpleScreen,
    viewport: EditViewport,
    drag_start_pointer: Option<Pos2>,
    drag_start_points: Vec<Point>,
    pan_drag_pointer: Option<Pos2>,
    pub metronome: MetronomeState,
}

/// Half-width/height (in document units) of the fixed 5m-square editing
/// viewport used by the "move performers" step, so small screens only ever
/// need to point at a small area precisely.
fn viewport_half_extent(grid: &GridConfig) -> f32 {
    let five_meters = match grid.unit {
        Unit::Meters => 5.0,
        Unit::Yards => 5.0 * 1.093_613,
    };
    five_meters * 0.5
}

impl DrillApp {
    /// Entry point called instead of the full desktop UI while Simple Mode is
    /// enabled. Completely separate code path from the rest of `app_ui.rs`;
    /// every state change still flows through `execute_edit`/`commit_layout`/
    /// `commit_shape`, so undo, autosave and validation all keep working.
    pub(crate) fn simple_ui(&mut self, ui: &mut egui::Ui) {
        if !matches!(self.simple_mode.step, SimpleStep::Playback) {
            // Editing steps always look at the exact start of the current
            // set, never a mid-transition interpolation, so what's on screen
            // always matches what a drag or shape button would commit.
            self.count_position = 0.0;
            self.document
                .positions_at(self.current_set, 0.0, &mut self.frame_positions);
        }
        self.simple_header_ui(ui);
        ui.separator();
        match self.simple_mode.screen {
            SimpleScreen::Metronome => self.simple_metronome_ui(ui),
            SimpleScreen::Wizard => self.simple_wizard_ui(ui),
        }
        self.show_update_notice(ui.ctx());
        self.onboarding.help_ui(ui.ctx(), self.locale);
        self.onboarding.persist_if_changed();
    }

    fn simple_header_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading(i18n::registered(self.locale, "simple-mode.009"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(i18n::registered(self.locale, "simple-mode.010"))
                    .on_hover_text(i18n::registered(self.locale, "simple-mode.011"))
                    .clicked()
                {
                    self.simple_mode.enabled = false;
                }
                ui.add_space(6.0);
                let metronome_selected = self.simple_mode.screen == SimpleScreen::Metronome;
                if ui
                    .selectable_label(
                        metronome_selected,
                        i18n::registered(self.locale, "simple-mode.012"),
                    )
                    .clicked()
                {
                    self.simple_mode.screen = if metronome_selected {
                        SimpleScreen::Wizard
                    } else {
                        SimpleScreen::Metronome
                    };
                }
                let dirty_label = if self.dirty {
                    text(self.locale, Text::Unsaved)
                } else {
                    text(self.locale, Text::Saved)
                };
                ui.label(dirty_label);
            });
        });
        if self.simple_mode.screen == SimpleScreen::Wizard {
            ui.horizontal(|ui| {
                for step in SimpleStep::ALL {
                    let selected = self.simple_mode.step == step;
                    if ui
                        .selectable_label(selected, step.title(self.locale))
                        .clicked()
                    {
                        self.simple_mode.step = step;
                    }
                }
            });
            ui.add_space(4.0);
            egui::Frame::new()
                .fill(ui.visuals().faint_bg_color)
                .inner_margin(10)
                .corner_radius(6)
                .show(ui, |ui| {
                    ui.label(self.simple_mode.step.guidance(self.locale));
                });
        }
    }

    fn simple_wizard_ui(&mut self, ui: &mut egui::Ui) {
        match self.simple_mode.step {
            SimpleStep::ChooseFormation => self.simple_step_choose_formation(ui),
            SimpleStep::SelectPerformers => self.simple_step_select_performers(ui),
            SimpleStep::MovePerformers => self.simple_step_move_performers(ui),
            SimpleStep::Playback => self.simple_step_playback(ui),
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.simple_mode.step != SimpleStep::ChooseFormation,
                    egui::Button::new(i18n::registered(self.locale, "simple-mode.013")),
                )
                .clicked()
            {
                self.simple_mode.step = self.simple_mode.step.previous();
            }
            if ui
                .add_enabled(
                    self.simple_mode.step != SimpleStep::Playback,
                    egui::Button::new(i18n::registered(self.locale, "simple-mode.014")),
                )
                .clicked()
            {
                self.simple_mode.step = self.simple_mode.step.next();
            }
        });
    }

    fn simple_step_choose_formation(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.current_set > 0,
                    egui::Button::new(i18n::registered(self.locale, "simple-mode.015")),
                )
                .clicked()
            {
                self.current_set -= 1;
                self.count_position = 0.0;
            }
            ui.label(format!(
                "{} {} / {}",
                i18n::registered(self.locale, "simple-mode.016"),
                self.current_set + 1,
                self.document.sets.len()
            ));
            if ui
                .add_enabled(
                    self.current_set + 1 < self.document.sets.len(),
                    egui::Button::new(i18n::registered(self.locale, "simple-mode.017")),
                )
                .clicked()
            {
                self.current_set += 1;
                self.count_position = 0.0;
            }
            if ui
                .button(i18n::registered(self.locale, "simple-mode.018"))
                .clicked()
            {
                self.duplicate_current_set();
            }
            let delete_set = UiCommand::DeleteSet.enabled(self.command_context(), self.locale);
            if ui
                .add_enabled(
                    delete_set.is_ok(),
                    egui::Button::new(UiCommand::DeleteSet.label(self.locale)),
                )
                .clicked()
            {
                self.delete_current_set();
            }
        });
        ui.add_space(6.0);
        self.simple_field_full_ui(ui, false);
    }

    fn simple_step_select_performers(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button(text(self.locale, Text::SelectAll)).clicked() {
                self.replace_selection((0..self.document.performers.len()).collect());
            }
            if ui.button(text(self.locale, Text::ClearSelection)).clicked() {
                self.clear_selection();
            }
            ui.label(format!(
                "{}: {}",
                i18n::registered(self.locale, "simple-mode.019"),
                self.selected.len()
            ));
            if ui
                .button(UiCommand::AddPerformer.label(self.locale))
                .clicked()
            {
                self.add_performer();
            }
            let remove =
                UiCommand::RemoveSelectedPerformers.enabled(self.command_context(), self.locale);
            if ui
                .add_enabled(
                    remove.is_ok(),
                    egui::Button::new(UiCommand::RemoveSelectedPerformers.label(self.locale)),
                )
                .clicked()
            {
                self.remove_selected_performers();
            }
        });
        ui.add_space(6.0);
        self.simple_field_full_ui(ui, true);
    }

    fn simple_step_move_performers(&mut self, ui: &mut egui::Ui) {
        if self.selected.is_empty() {
            ui.colored_label(
                Color32::from_rgb(245, 197, 66),
                i18n::registered(self.locale, "simple-mode.020"),
            );
        }
        ui.horizontal(|ui| {
            let enabled = !self.selected.is_empty();
            let grid = self.document.grid.clone();
            if ui
                .add_enabled(
                    enabled,
                    egui::Button::new(i18n::registered(self.locale, "simple-mode.021")),
                )
                .clicked()
            {
                self.commit_shape(shapes::ShapeSpec::Line {
                    start: Point {
                        x: grid.width * 0.25,
                        y: grid.height * 0.5,
                    },
                    end: Point {
                        x: grid.width * 0.75,
                        y: grid.height * 0.5,
                    },
                });
            }
            if ui
                .add_enabled(
                    enabled,
                    egui::Button::new(i18n::registered(self.locale, "simple-mode.022")),
                )
                .clicked()
            {
                self.commit_shape(shapes::ShapeSpec::Circle {
                    center: Point {
                        x: grid.width * 0.5,
                        y: grid.height * 0.5,
                    },
                    radius: grid.width.min(grid.height * 2.0) * 0.15,
                });
            }
            if ui
                .add_enabled(
                    enabled,
                    egui::Button::new(i18n::registered(self.locale, "simple-mode.023")),
                )
                .clicked()
            {
                let count = self.selected.len().max(1);
                let cols = (count as f32).sqrt().ceil() as usize;
                let rows = count.div_ceil(cols.max(1));
                self.commit_shape(shapes::ShapeSpec::BlockFit {
                    rect_min: Point {
                        x: grid.width * 0.35,
                        y: grid.height * 0.3,
                    },
                    rect_max: Point {
                        x: grid.width * 0.65,
                        y: grid.height * 0.7,
                    },
                    cols: cols.max(1),
                    rows: rows.max(1),
                });
            }
            if ui
                .button(i18n::registered(self.locale, "simple-mode.024"))
                .clicked()
            {
                self.simple_mode.viewport.center = Some(self.simple_selection_anchor());
            }
        });
        ui.add_space(6.0);
        self.simple_field_zoom_ui(ui);
    }

    fn simple_step_playback(&mut self, ui: &mut egui::Ui) {
        // Keep range selection beside the primary Play control. In the full
        // editor these controls live above the timeline, but a learner in
        // Simple Mode should never have to leave the playback step just to
        // answer the essential question: "which part will play?".
        let total_counts = self.document.timeline_counts().max(1);
        let current_global = self
            .document
            .global_count(self.current_set, self.count_position)
            .round()
            .clamp(0.0, total_counts as f32) as u32;
        egui::Frame::new()
            .fill(ui.visuals().faint_bg_color)
            .inner_margin(8)
            .corner_radius(6)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        egui::RichText::new(i18n::registered(self.locale, "simple-mode.039"))
                            .strong(),
                    );
                    if ui
                        .button(i18n::registered(self.locale, "simple-mode.040"))
                        .clicked()
                    {
                        let start = self.document.global_count(self.current_set, 0.0) as u32;
                        self.playback_start = start;
                        self.playback_end = (start
                            + u32::from(self.document.sets[self.current_set].counts))
                        .min(total_counts);
                    }
                    if ui
                        .button(i18n::registered(self.locale, "simple-mode.041"))
                        .clicked()
                    {
                        self.playback_start = 0;
                        self.playback_end = total_counts;
                    }
                    if ui
                        .button(i18n::registered(self.locale, "simple-mode.042"))
                        .on_hover_text(i18n::registered(self.locale, "simple-mode.046"))
                        .clicked()
                    {
                        self.playback_start =
                            current_global.min(self.playback_end.saturating_sub(1));
                    }
                    if ui
                        .button(i18n::registered(self.locale, "simple-mode.043"))
                        .on_hover_text(i18n::registered(self.locale, "simple-mode.047"))
                        .clicked()
                    {
                        self.playback_end = current_global
                            .max(self.playback_start + 1)
                            .min(total_counts);
                    }
                    ui.checkbox(
                        &mut self.loop_playback,
                        i18n::registered(self.locale, "simple-mode.044"),
                    );
                });
                let range =
                    playback_range_summary(&self.document, self.playback_start, self.playback_end);
                let count_label = i18n::registered(self.locale, "simple-mode.049");
                ui.horizontal_wrapped(|ui| {
                    ui.small(format!(
                        "{}: {} · {} {}",
                        i18n::registered(self.locale, "simple-mode.048"),
                        range.start_set,
                        count_label,
                        range.start_count,
                    ));
                    ui.separator();
                    ui.small(format!(
                        "{}: {} · {} {}",
                        i18n::registered(self.locale, "simple-mode.050"),
                        range.end_set,
                        count_label,
                        range.end_count,
                    ));
                    ui.separator();
                    ui.small(format!(
                        "{}: {} {}",
                        i18n::registered(self.locale, "simple-mode.051"),
                        range.length,
                        i18n::registered(self.locale, "simple-mode.052"),
                    ));
                });
                ui.small(i18n::registered(self.locale, "simple-mode.053"));
            });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let label = if self.playing {
                text(self.locale, Text::Pause)
            } else {
                text(self.locale, Text::Play)
            };
            if ui
                .add_sized(
                    [160.0, 46.0],
                    egui::Button::new(egui::RichText::new(label).size(20.0)),
                )
                .clicked()
            {
                self.toggle_playback(ui.ctx());
            }
            if ui
                .button(i18n::registered(self.locale, "simple-mode.025"))
                .clicked()
            {
                self.navigate_to_global_count(self.playback_start);
            }
            if ui
                .add_sized(
                    [140.0, 46.0],
                    egui::Button::new(i18n::registered(self.locale, "simple-mode.026")),
                )
                .clicked()
            {
                self.save_dialog();
            }
        });
        let total = total_counts as f32;
        let position = self
            .document
            .global_count(self.current_set, self.count_position)
            .clamp(0.0, total);
        ui.add(egui::ProgressBar::new(position / total).text(format!(
            "{}: {:.0} / {:.0}",
            i18n::registered(self.locale, "simple-mode.027"),
            position,
            total
        )));
        ui.add_space(6.0);
        self.simple_field_full_ui(ui, false);
    }

    /// Full-field 2D view shared by every simple-mode screen except "move
    /// performers". Reuses the exact same rendering pipeline as the normal
    /// desktop view (`drill_render::build_field_2d` + `egui_backend::paint`),
    /// only the interaction is simplified. `selectable` gates click-to-select;
    /// dragging performers is intentionally not offered here (that belongs
    /// to the zoomed step, per the precision-on-small-screens requirement).
    fn simple_field_full_ui(&mut self, ui: &mut egui::Ui, selectable: bool) {
        let available = ui.available_size();
        let sense = if selectable {
            Sense::click()
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
        for (index, &point) in self.frame_positions.iter().enumerate() {
            if self.selected.contains(&index) {
                let pos = to_screen(point);
                painter.circle_stroke(pos, 11.0, Stroke::new(2.0, Color32::WHITE));
                if self.selected.len() >= 2 {
                    self.paint_selection_rank_badge(&painter, pos, index);
                }
            }
        }
        if !selectable {
            return;
        }
        let Some(pointer) = response.interact_pointer_pos() else {
            return;
        };
        if !response.clicked() {
            return;
        }
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
            .filter(|(_, p)| to_screen(**p).distance(pointer) < 22.0)
            .map(|(index, _)| index);
        let additive = ui.input(|input| {
            input.modifiers.command || input.modifiers.ctrl || input.modifiers.shift
        });
        match nearest {
            Some(index) if additive => {
                let mut next = self.selected.clone();
                if !next.insert(index) {
                    next.remove(&index);
                }
                self.replace_selection(next);
            }
            Some(index) => self.replace_selection(std::iter::once(index).collect()),
            None if !additive => self.clear_selection(),
            None => {}
        }
    }

    fn simple_selection_anchor(&self) -> Point {
        let points = self.selected_points();
        if points.is_empty() {
            return self.frame_positions.first().copied().unwrap_or(Point {
                x: self.document.grid.width * 0.5,
                y: self.document.grid.height * 0.5,
            });
        }
        let count = points.len() as f32;
        Point {
            x: points.iter().map(|p| p.x).sum::<f32>() / count,
            y: points.iter().map(|p| p.y).sum::<f32>() / count,
        }
    }

    /// Zoomed, pannable 5m-square editing viewport for the "move performers"
    /// step. Performers outside the visible square simply aren't drawn; the
    /// arrow buttons (and arrow keys) pan the square to reach them. Dragging
    /// a performer here still ends in `commit_layout`, which snaps to the
    /// grid and writes absolute field coordinates via `Edit::MovePerformers`
    /// exactly like the desktop field view — only the on-screen mapping is
    /// local to this module.
    fn simple_field_zoom_ui(&mut self, ui: &mut egui::Ui) {
        let grid = self.document.grid.clone();
        let half = viewport_half_extent(&grid);
        if self.simple_mode.viewport.center.is_none() {
            self.simple_mode.viewport.center = Some(self.simple_selection_anchor());
        }
        let center = {
            let center = self
                .simple_mode
                .viewport
                .center
                .as_mut()
                .expect("just populated above");
            center.x = center.x.clamp(half, (grid.width - half).max(half));
            center.y = center.y.clamp(half, (grid.height - half).max(half));
            *center
        };

        let step = if grid.unit == Unit::Meters {
            1.0
        } else {
            1.093_613
        };
        let mut pan = Vec2::ZERO;
        ui.input(|input| {
            if input.key_pressed(egui::Key::ArrowUp) {
                pan.y += step;
            }
            if input.key_pressed(egui::Key::ArrowDown) {
                pan.y -= step;
            }
            if input.key_pressed(egui::Key::ArrowLeft) {
                pan.x -= step;
            }
            if input.key_pressed(egui::Key::ArrowRight) {
                pan.x += step;
            }
        });

        ui.horizontal(|ui| {
            ui.label(i18n::registered(self.locale, "simple-mode.028"));
        });
        ui.horizontal(|ui| {
            ui.add_space((ui.available_width() * 0.5 - 24.0).max(0.0));
            if ui
                .add_sized(
                    [48.0, 32.0],
                    egui::Button::new(i18n::registered(self.locale, "simple-mode.033")),
                )
                .clicked()
            {
                pan.y += step;
            }
        });

        let available = ui.available_size() - Vec2::new(0.0, 40.0);
        ui.horizontal(|ui| {
            if ui
                .add_sized(
                    [32.0, 48.0],
                    egui::Button::new(i18n::registered(self.locale, "simple-mode.034")),
                )
                .clicked()
            {
                pan.x -= step;
            }
            let (response, painter) = ui.allocate_painter(
                available.max(Vec2::new(80.0, 80.0)),
                Sense::click_and_drag(),
            );
            let rect = response.rect;
            self.paint_zoom_viewport(ui, &painter, rect, center, half);
            self.handle_zoom_interaction(ui, &response, rect, center, half);
            if ui
                .add_sized(
                    [32.0, 48.0],
                    egui::Button::new(i18n::registered(self.locale, "simple-mode.035")),
                )
                .clicked()
            {
                pan.x += step;
            }
        });
        ui.horizontal(|ui| {
            ui.add_space((ui.available_width() * 0.5 - 24.0).max(0.0));
            if ui
                .add_sized(
                    [48.0, 32.0],
                    egui::Button::new(i18n::registered(self.locale, "simple-mode.036")),
                )
                .clicked()
            {
                pan.y -= step;
            }
        });

        if pan != Vec2::ZERO
            && let Some(center) = &mut self.simple_mode.viewport.center
        {
            center.x = (center.x + pan.x).clamp(half, (grid.width - half).max(half));
            center.y = (center.y + pan.y).clamp(half, (grid.height - half).max(half));
        }
    }

    fn paint_zoom_viewport(
        &self,
        _ui: &egui::Ui,
        painter: &egui::Painter,
        rect: Rect,
        center: Point,
        half: f32,
    ) {
        painter.rect_filled(rect, 4.0, Color32::from_rgb(25, 71, 45));
        let size = half * 2.0;
        let scale = (rect.width() / size).min(rect.height() / size).max(0.001);
        let drawn = size * scale;
        let offset = Vec2::new((rect.width() - drawn) * 0.5, (rect.height() - drawn) * 0.5);
        let vp_min_x = center.x - half;
        let vp_max_y = center.y + half;
        let to_screen = |p: Point| -> Pos2 {
            Pos2::new(
                rect.left() + offset.x + (p.x - vp_min_x) * scale,
                rect.top() + offset.y + (vp_max_y - p.y) * scale,
            )
        };
        // Reference lines every yard/meter so a small square still reads as
        // a field, not an abstract canvas.
        let grid = &self.document.grid;
        let minor = if grid.unit == Unit::Meters {
            1.0
        } else {
            1.093_613
        };
        if minor > 0.01 {
            let mut x = (vp_min_x / minor).floor() * minor;
            while x <= center.x + half {
                if x >= vp_min_x {
                    let a = to_screen(Point {
                        x,
                        y: center.y - half,
                    });
                    let b = to_screen(Point {
                        x,
                        y: center.y + half,
                    });
                    painter.line_segment([a, b], Stroke::new(1.0, Color32::from_white_alpha(35)));
                }
                x += minor;
            }
            let mut y = ((center.y - half) / minor).floor() * minor;
            while y <= center.y + half {
                if y >= center.y - half {
                    let a = to_screen(Point {
                        x: center.x - half,
                        y,
                    });
                    let b = to_screen(Point {
                        x: center.x + half,
                        y,
                    });
                    painter.line_segment([a, b], Stroke::new(1.0, Color32::from_white_alpha(35)));
                }
                y += minor;
            }
        }
        for hash in &grid.hashes {
            if hash.position < center.y - half || hash.position > center.y + half {
                continue;
            }
            let a = to_screen(Point {
                x: center.x - half,
                y: hash.position,
            });
            let b = to_screen(Point {
                x: center.x + half,
                y: hash.position,
            });
            painter.line_segment([a, b], Stroke::new(1.5, Color32::from_white_alpha(110)));
        }
        for (index, performer) in self.document.performers.iter().enumerate() {
            let Some(&point) = self.frame_positions.get(index) else {
                continue;
            };
            if (point.x - center.x).abs() > half + 0.5 || (point.y - center.y).abs() > half + 0.5 {
                continue;
            }
            let color = performer.resolved_color(&self.document.sections);
            let selected = self.selected.contains(&index);
            // While a performer in the current drag is being moved, its
            // on-field dot stays put and a live preview dot is drawn at the
            // pointer-following position instead, mirroring the desktop
            // field view's drag preview.
            let pos = if self.simple_mode.drag_start_pointer.is_some()
                && let Some(preview) = self.drag_preview_point(index)
            {
                to_screen(preview)
            } else {
                to_screen(point)
            };
            painter.circle_filled(pos, 9.0, Color32::from_rgb(color[0], color[1], color[2]));
            if self.is_hidden_index(index) {
                painter.circle_filled(pos, 9.0, Color32::from_black_alpha(185));
                painter.line_segment(
                    [pos + Vec2::new(-6.0, 6.0), pos + Vec2::new(6.0, -6.0)],
                    Stroke::new(1.5, Color32::WHITE),
                );
            } else if self.is_locked_index(index) {
                painter.circle_stroke(pos, 8.5, Stroke::new(1.5, Color32::WHITE));
                painter.text(
                    pos + Vec2::new(7.0, -8.0),
                    egui::Align2::CENTER_CENTER,
                    "L",
                    egui::FontId::proportional(10.0),
                    Color32::WHITE,
                );
            }
            if selected {
                painter.circle_stroke(pos, 12.0, Stroke::new(2.5, Color32::WHITE));
                if self.selected.len() >= 2 {
                    self.paint_selection_rank_badge(painter, pos, index);
                }
            }
        }
    }

    /// While a drag is in flight, the live preview position for `index`
    /// (screen-independent, in field units), or `None` if it isn't part of
    /// the current drag.
    fn drag_preview_point(&self, index: usize) -> Option<Point> {
        if !self.selected.contains(&index) {
            return None;
        }
        let selected_order: Vec<usize> = self.selected.iter().copied().collect();
        let slot = selected_order.iter().position(|&i| i == index)?;
        self.simple_mode.drag_start_points.get(slot).copied()
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_zoom_interaction(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: Rect,
        center: Point,
        half: f32,
    ) {
        let size = half * 2.0;
        let scale = (rect.width() / size).min(rect.height() / size).max(0.001);
        let drawn = size * scale;
        let offset = Vec2::new((rect.width() - drawn) * 0.5, (rect.height() - drawn) * 0.5);
        let vp_min_x = center.x - half;
        let vp_max_y = center.y + half;
        let to_screen = |p: Point| -> Pos2 {
            Pos2::new(
                rect.left() + offset.x + (p.x - vp_min_x) * scale,
                rect.top() + offset.y + (vp_max_y - p.y) * scale,
            )
        };
        let Some(pointer) = response.interact_pointer_pos() else {
            return;
        };
        // Resolve the hit before mutating `self`. Keeping this as a value (not
        // a closure that borrows frame_positions) makes click and drag share
        // one stable target and avoids a stale borrow across selection changes.
        let nearest = self
            .frame_positions
            .iter()
            .enumerate()
            .filter(|(index, _)| self.is_selectable_index(*index))
            .filter(|&(_, &p)| (p.x - center.x).abs() <= half && (p.y - center.y).abs() <= half)
            .min_by(|(_, a), (_, b)| {
                to_screen(**a)
                    .distance(pointer)
                    .total_cmp(&to_screen(**b).distance(pointer))
            })
            .filter(|&(_, &p)| to_screen(p).distance(pointer) < 22.0)
            .map(|(index, _)| index);
        let additive = ui.input(|input| {
            input.modifiers.command || input.modifiers.ctrl || input.modifiers.shift
        });
        if response.clicked() {
            match nearest {
                Some(index) if additive => {
                    let mut next = self.selected.clone();
                    if !next.insert(index) {
                        next.remove(&index);
                    }
                    self.replace_selection(next);
                }
                Some(index) => self.replace_selection(std::iter::once(index).collect()),
                None if !additive => self.clear_selection(),
                None => {}
            }
        }
        if response.drag_started() {
            if let Some(index) = nearest {
                if !self.is_editable_set_start() {
                    self.ensure_editable_set_start();
                    return;
                }
                if !self.selected.contains(&index) {
                    if additive {
                        let mut next = self.selected.clone();
                        next.insert(index);
                        self.replace_selection(next);
                    } else {
                        self.replace_selection(std::iter::once(index).collect());
                    }
                }
                self.simple_mode.drag_start_points = self.selected_points();
                self.simple_mode.drag_start_pointer = Some(pointer);
            } else if self.selected.is_empty() {
                // Panning by dragging empty background, but only while
                // nothing is selected, so a move-drag never gets stolen.
                self.simple_mode.pan_drag_pointer = Some(pointer);
            }
        }
        if response.dragged()
            && !self.simple_mode.drag_start_points.is_empty()
            && let Some(last_pointer) = self.simple_mode.drag_start_pointer
        {
            // Integrate frame-to-frame screen delta into the live preview
            // positions (in field units); `commit_layout` snaps and writes
            // the final absolute field coordinates once the drag ends.
            let delta = pointer - last_pointer;
            for point in &mut self.simple_mode.drag_start_points {
                point.x = (point.x + delta.x / scale).clamp(0.0, self.document.grid.width);
                point.y = (point.y - delta.y / scale).clamp(0.0, self.document.grid.height);
            }
            self.simple_mode.drag_start_pointer = Some(pointer);
        }
        if response.dragged()
            && let Some(start_pointer) = self.simple_mode.pan_drag_pointer
        {
            let delta = pointer - start_pointer;
            if let Some(center) = &mut self.simple_mode.viewport.center {
                center.x = (center.x - delta.x / scale)
                    .clamp(half, (self.document.grid.width - half).max(half));
                center.y = (center.y + delta.y / scale)
                    .clamp(half, (self.document.grid.height - half).max(half));
            }
            self.simple_mode.pan_drag_pointer = Some(pointer);
        }
        if response.drag_stopped() {
            if !self.simple_mode.drag_start_points.is_empty() {
                let final_points = std::mem::take(&mut self.simple_mode.drag_start_points);
                self.commit_layout(final_points);
            }
            self.simple_mode.drag_start_pointer = None;
            self.simple_mode.pan_drag_pointer = None;
        }
        let _ = ui;
    }

    fn simple_metronome_ui(&mut self, ui: &mut egui::Ui) {
        self.simple_mode.metronome.poll();
        ui.vertical_centered(|ui| {
            ui.add_space(10.0);
            ui.label(i18n::registered(self.locale, "simple-mode.029"));
            ui.add_space(16.0);
            ui.label(
                egui::RichText::new(format!("{:.0}", self.simple_mode.metronome.bpm))
                    .size(72.0)
                    .strong(),
            );
            ui.label(i18n::registered(self.locale, "simple-mode.032"));
            ui.add_space(12.0);
            let mut bpm = self.simple_mode.metronome.bpm;
            if ui
                .add(egui::Slider::new(&mut bpm, 30.0..=300.0).show_value(false))
                .changed()
            {
                self.simple_mode.metronome.set_bpm(bpm);
            }
            ui.add_space(18.0);
            let running = self.simple_mode.metronome.is_running();
            let phase = self.simple_mode.metronome.beat_phase();
            let pulse_radius = if running { 34.0 - phase * 10.0 } else { 30.0 };
            let pulse_color = if running {
                let brightness = (1.0 - phase) * 200.0 + 55.0;
                Color32::from_rgb(80, brightness as u8, 235)
            } else {
                Color32::from_gray(90)
            };
            let (response, painter) = ui.allocate_painter(Vec2::new(90.0, 90.0), Sense::hover());
            painter.circle_filled(response.rect.center(), pulse_radius, pulse_color);
            painter.circle_stroke(
                response.rect.center(),
                36.0,
                Stroke::new(2.0, Color32::from_gray(200)),
            );
            ui.add_space(18.0);
            let toggle_label = if running {
                i18n::registered(self.locale, "simple-mode.030")
            } else {
                i18n::registered(self.locale, "simple-mode.031")
            };
            if ui
                .add_sized(
                    [200.0, 56.0],
                    egui::Button::new(egui::RichText::new(toggle_label).size(22.0)),
                )
                .clicked()
            {
                self.simple_mode.metronome.toggle();
            }
            if let Some(error) = self.simple_mode.metronome.error() {
                ui.add_space(8.0);
                let id = match error {
                    MetronomeFailure::Output => "simple-mode.037",
                    MetronomeFailure::Schedule => "simple-mode.038",
                };
                ui.colored_label(
                    Color32::from_rgb(235, 120, 120),
                    i18n::registered(self.locale, id),
                );
            }
        });
        if self.simple_mode.metronome.is_running() {
            ui.ctx().request_repaint_after(Duration::from_millis(33));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_half_extent_is_small_and_finite() {
        let grid = GridConfig::default();
        let half = viewport_half_extent(&grid);
        assert!(half.is_finite() && half > 0.0 && half < grid.width);
    }

    #[test]
    fn simple_step_order_is_linear_and_bidirectional() {
        assert_eq!(
            SimpleStep::ChooseFormation.next(),
            SimpleStep::SelectPerformers
        );
        assert_eq!(SimpleStep::Playback.next(), SimpleStep::Playback);
        assert_eq!(
            SimpleStep::ChooseFormation.previous(),
            SimpleStep::ChooseFormation
        );
        assert_eq!(SimpleStep::Playback.previous(), SimpleStep::MovePerformers);
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
    fn guided_flow_select_apply_shape_and_play_is_edit_routed() {
        let mut app = DrillApp {
            simple_mode: SimpleModeState {
                enabled: true,
                ..SimpleModeState::default()
            },
            ..DrillApp::default()
        };
        let before = app.document.clone();

        // Step 2: select performers, mirroring the "Select All" button.
        app.simple_mode.step = SimpleStep::SelectPerformers;
        app.selected = (0..app.document.performers.len()).collect();
        assert!(!app.selected.is_empty());

        // Step 3: apply a basic shape, mirroring the "Line Up" button. This
        // must go through `commit_shape` -> `execute_edit`, so it is a single
        // undoable transaction and never touches `document` directly.
        app.simple_mode.step = SimpleStep::MovePerformers;
        let grid = app.document.grid.clone();
        app.commit_shape(shapes::ShapeSpec::Line {
            start: Point {
                x: grid.width * 0.25,
                y: grid.height * 0.5,
            },
            end: Point {
                x: grid.width * 0.75,
                y: grid.height * 0.5,
            },
        });
        assert_ne!(app.document, before);
        assert!(app.history.can_undo());
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document, before);
        assert!(app.history.redo(&mut app.document));
        assert_ne!(app.document, before);

        // Step 4: start playback, mirroring the big Play button.
        app.simple_mode.step = SimpleStep::Playback;
        let context = egui::Context::default();
        app.execute_command(commands::Command::RangeWholeShow, &context);
        app.toggle_playback(&context);
        assert!(app.playing);
        app.toggle_playback(&context);
        assert!(!app.playing);
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
}
