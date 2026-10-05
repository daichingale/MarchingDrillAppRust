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
use std::time::{Duration, Instant};

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
    toast: Option<(Instant, String)>,
    confirm_remove: bool,
}

impl SimpleModeState {
    pub(crate) fn with_enabled(enabled: bool) -> Self {
        Self {
            enabled,
            ..Self::default()
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

    pub(crate) fn push_simple_toast(&mut self, text: impl Into<String>) {
        self.simple_mode.toast = Some((Instant::now(), text.into()));
    }

    fn simple_toast_alive(&mut self) -> Option<String> {
        let expired = self
            .simple_mode
            .toast
            .as_ref()
            .is_some_and(|(shown, _)| shown.elapsed().as_secs_f32() > 2.8);
        if expired {
            self.simple_mode.toast = None;
        }
        self.simple_mode
            .toast
            .as_ref()
            .map(|(_, text)| text.clone())
    }

    fn simple_play_label(&self) -> &'static str {
        i18n::registered(self.locale, "simple-mode.061")
    }

    fn simple_place_label(&self) -> &'static str {
        i18n::registered(self.locale, "simple-mode.055")
    }

    fn simple_empty_field_hint(&self) -> &'static str {
        i18n::registered(self.locale, "simple-mode.058")
    }

    pub(crate) fn simple_new_work_label(&self) -> &'static str {
        i18n::registered(self.locale, "simple-mode.098")
    }

    fn simple_guide(&self) -> SimpleGuide {
        if self.document.performers.is_empty() {
            SimpleGuide::Place
        } else if !self.onboarding.simple_drag_tip_seen {
            SimpleGuide::Move
        } else if self.document.sets.len() < 2 {
            SimpleGuide::NextSet
        } else {
            SimpleGuide::Play
        }
    }

    /// Entry point called instead of the full desktop UI while Simple Mode is
    /// enabled. Completely separate code path from the rest of `app_ui.rs`;
    /// every state change still flows through `execute_edit`/`commit_layout`/
    /// `commit_shape`, so undo, autosave and validation all keep working.
    pub(crate) fn simple_ui(&mut self, ui: &mut egui::Ui) {
        if !self.playing {
            self.count_position = 0.0;
            self.document
                .positions_at(self.current_set, 0.0, &mut self.frame_positions);
        }
        if self.document.performers.is_empty() {
            self.field_tool = FieldTool::Place;
        } else if self.field_tool == FieldTool::Select {
            self.field_tool = FieldTool::Move;
        }
        if self.audio_state.is_loading() || self.project_state.busy() {
            ui.small(i18n::registered(self.locale, "simple-mode.099"));
        }
        super::app_theme::toolbar_frame(ui).show(ui, |ui| {
            self.simple_top_bar(ui);
        });
        ui.add_space(8.0);
        if !self.is_editable_set_start() {
            super::app_theme::surface_frame(ui).show(ui, |ui| {
                ui.label(
                    egui::RichText::new(i18n::registered(self.locale, "simple-mode.082"))
                        .size(14.0)
                        .color(super::app_theme::SECONDARY_TEXT),
                );
            });
            ui.add_space(6.0);
        }
        if !self.onboarding.simple_steps_dismissed && self.simple_guide() != SimpleGuide::Place {
            self.simple_cue_banner(ui);
            ui.add_space(6.0);
        }
        self.simple_tools_ui(ui);
        if let Some(toast) = self.simple_toast_alive() {
            ui.add_space(6.0);
            super::app_theme::surface_frame(ui)
                .fill(super::app_theme::ACCENT_SOFT)
                .stroke(egui::Stroke::new(1.0, super::app_theme::ACCENT))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(toast).size(15.0).strong());
                });
            ui.ctx().request_repaint_after(Duration::from_millis(200));
        }
        ui.add_space(8.0);
        ui.horizontal_top(|ui| {
            self.simple_field_full_ui(ui, true);
            self.simple_roster_panel(ui);
        });
        self.show_update_notice(ui.ctx());
        self.onboarding.help_ui(ui.ctx(), self.locale);
        self.onboarding.persist_if_changed();
    }

    fn simple_top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let undo_ok = self.history.can_undo();
            let undo_label = i18n::registered(self.locale, "simple-mode.064");
            if ui
                .add_enabled(
                    undo_ok,
                    super::app_theme::quiet_button(egui::RichText::new(undo_label).size(15.0)),
                )
                .on_hover_text(i18n::registered(self.locale, "simple-mode.092"))
                .clicked()
            {
                self.execute_command(UiCommand::Undo, ui.ctx());
            }
            let save_label = i18n::registered(self.locale, "simple-mode.026");
            if ui
                .add_sized(
                    [108.0, 36.0],
                    super::app_theme::quiet_button(egui::RichText::new(save_label).size(15.0)),
                )
                .clicked()
            {
                self.save_dialog();
            }
            let play_label = if self.playing {
                text(self.locale, Text::Pause)
            } else {
                self.simple_play_label()
            };
            let play_filled = self.simple_guide() == SimpleGuide::Play;
            let play_button = if play_filled {
                super::app_theme::primary_button(
                    egui::RichText::new(play_label)
                        .size(16.0)
                        .color(Color32::WHITE),
                )
            } else {
                super::app_theme::quiet_button(egui::RichText::new(play_label).size(16.0))
            };
            if ui.add_sized([120.0, 36.0], play_button).clicked() {
                self.toggle_playback(ui.ctx());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let full = i18n::registered(self.locale, "simple-mode.063");
                let simple = i18n::registered(self.locale, "simple-mode.062");
                if ui
                    .add_sized(
                        [88.0, 36.0],
                        super::app_theme::quiet_button(egui::RichText::new(full).size(14.0)),
                    )
                    .clicked()
                {
                    self.simple_mode.enabled = false;
                    self.onboarding.prefer_simple = false;
                }
                let _ = ui.add_sized(
                    [88.0, 36.0],
                    super::app_theme::primary_button(
                        egui::RichText::new(simple).size(14.0).color(Color32::WHITE),
                    ),
                );
            });
        });
    }

    fn simple_cue_banner(&mut self, ui: &mut egui::Ui) {
        let guide = self.simple_guide();
        let message = match guide {
            SimpleGuide::Place => self.simple_empty_field_hint(),
            SimpleGuide::Move => i18n::registered(self.locale, "simple-mode.059"),
            SimpleGuide::NextSet => i18n::registered(self.locale, "simple-mode.060"),
            SimpleGuide::Play => self.simple_play_label(),
        };
        egui::Frame::new()
            .fill(Color32::from_rgb(245, 248, 252))
            .stroke(egui::Stroke::new(1.0, super::app_theme::HAIRLINE))
            .corner_radius(super::app_theme::CORNER)
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(message)
                            .size(14.0)
                            .color(super::app_theme::SECONDARY_TEXT),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let close = i18n::registered(self.locale, "simple-mode.065");
                        if ui.add(super::app_theme::quiet_button(close)).clicked() {
                            self.onboarding.simple_steps_dismissed = true;
                            if guide == SimpleGuide::Move {
                                self.onboarding.simple_drag_tip_seen = true;
                            }
                        }
                    });
                });
            });
    }

    fn simple_tools_ui(&mut self, ui: &mut egui::Ui) {
        let guide = self.simple_guide();
        let editable = self.is_editable_set_start();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            let place_size = if guide == SimpleGuide::Place {
                egui::Vec2::new(188.0, 52.0)
            } else {
                egui::Vec2::new(132.0, 44.0)
            };
            let tool_size = egui::Vec2::new(132.0, 44.0);
            let place_label = self.simple_place_label();
            let move_label = i18n::registered(self.locale, "simple-mode.056");
            let next_label = i18n::registered(self.locale, "simple-mode.057");
            let highlight_place =
                guide == SimpleGuide::Place && self.document.performers.is_empty();
            if ui
                .add_sized(
                    place_size,
                    Self::simple_action_button(place_label, highlight_place),
                )
                .on_hover_text(i18n::registered(self.locale, "simple-mode.069"))
                .clicked()
                && editable
            {
                self.set_field_tool(FieldTool::Place);
            }
            if ui
                .add_sized(
                    tool_size,
                    Self::simple_action_button(move_label, guide == SimpleGuide::Move),
                )
                .on_hover_text(i18n::registered(self.locale, "simple-mode.070"))
                .clicked()
            {
                self.set_field_tool(FieldTool::Move);
            }
            if ui
                .add_sized(
                    tool_size,
                    Self::simple_action_button(next_label, guide == SimpleGuide::NextSet),
                )
                .on_hover_text(i18n::registered(self.locale, "simple-mode.071"))
                .clicked()
            {
                self.duplicate_current_set();
            }
            let set_name = self
                .document
                .sets
                .get(self.current_set)
                .map(|set| set.name.as_str())
                .unwrap_or("—");
            ui.add_space(8.0);
            super::app_theme::surface_frame(ui).show(ui, |ui| {
                ui.label(format!(
                    "{}  {} / {}",
                    i18n::registered(self.locale, "simple-mode.072"),
                    self.current_set + 1,
                    self.document.sets.len().max(1)
                ));
                ui.strong(set_name);
            });
        });
    }

    fn simple_action_button(label: &'static str, filled: bool) -> egui::Button<'static> {
        if filled {
            super::app_theme::primary_button(
                egui::RichText::new(label).size(16.0).color(Color32::WHITE),
            )
        } else {
            super::app_theme::quiet_button(egui::RichText::new(label).size(16.0))
        }
    }

    fn simple_roster_panel(&mut self, ui: &mut egui::Ui) {
        ui.allocate_ui_with_layout(
            egui::Vec2::new(240.0, ui.available_height()),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                super::app_theme::surface_frame(ui).show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(i18n::registered(self.locale, "simple-mode.073"))
                            .strong(),
                    );
                    ui.add_space(6.0);
                    let rows: Vec<(usize, String)> = self
                        .document
                        .performers
                        .iter()
                        .enumerate()
                        .map(|(index, performer)| (index, performer.label.clone()))
                        .collect();
                    if rows.is_empty() {
                        ui.label(
                            egui::RichText::new(i18n::registered(self.locale, "simple-mode.074"))
                                .color(super::app_theme::SECONDARY_TEXT),
                        );
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("simple-roster-list")
                        .max_height(ui.available_height() * 0.45)
                        .show(ui, |ui| {
                            for (index, label) in &rows {
                                let selected = self.selected.contains(index);
                                let fill = if selected {
                                    super::app_theme::ACCENT_SOFT
                                } else {
                                    ui.visuals().extreme_bg_color
                                };
                                let stroke = if selected {
                                    egui::Stroke::new(1.0, super::app_theme::ACCENT)
                                } else {
                                    egui::Stroke::new(1.0, super::app_theme::HAIRLINE)
                                };
                                let response = egui::Frame::new()
                                    .fill(fill)
                                    .stroke(stroke)
                                    .corner_radius(super::app_theme::CORNER_SM)
                                    .inner_margin(egui::Margin::symmetric(8, 6))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.label(label);
                                    })
                                    .response
                                    .interact(Sense::click());
                                if response.clicked() {
                                    self.replace_selection(std::iter::once(*index).collect());
                                }
                                ui.add_space(4.0);
                            }
                        });
                    ui.add_space(8.0);
                    self.simple_identity_fields(ui);
                    if !self.selected.is_empty()
                        && self.document.performers.len() > self.selected.len()
                    {
                        ui.add_space(10.0);
                        let remove = i18n::registered(self.locale, "simple-mode.075");
                        if ui.add(super::app_theme::quiet_button(remove)).clicked() {
                            self.simple_mode.confirm_remove = true;
                        }
                    }
                });
            },
        );
        self.simple_remove_confirm(ui.ctx());
    }

    fn simple_remove_confirm(&mut self, ctx: &egui::Context) {
        if !self.simple_mode.confirm_remove {
            return;
        }
        let mut open = true;
        egui::Modal::new(egui::Id::new("simple-remove-confirm")).show(ctx, |ui| {
            ui.set_max_width(360.0);
            ui.label(i18n::registered(self.locale, "simple-mode.076"));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let cancel = i18n::registered(self.locale, "simple-mode.078");
                if ui.add(super::app_theme::quiet_button(cancel)).clicked() {
                    self.simple_mode.confirm_remove = false;
                    open = false;
                }
                let remove = i18n::registered(self.locale, "simple-mode.077");
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new(remove).color(Color32::WHITE))
                            .fill(Color32::from_rgb(196, 80, 72))
                            .corner_radius(super::app_theme::CORNER),
                    )
                    .clicked()
                {
                    self.remove_selected_performers();
                    self.simple_mode.confirm_remove = false;
                    open = false;
                }
            });
        });
        if !open {
            self.simple_mode.confirm_remove = false;
        }
    }

    fn simple_empty_field_overlay(&mut self, ui: &mut egui::Ui, field: egui::Rect) {
        let pos = field.center() - egui::vec2(140.0, 52.0);
        egui::Area::new(egui::Id::new("simple-empty-field"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .movable(false)
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(255, 255, 255))
                    .stroke(egui::Stroke::new(1.0, super::app_theme::HAIRLINE))
                    .corner_radius(super::app_theme::CORNER)
                    .inner_margin(egui::Margin::symmetric(16, 12))
                    .show(ui, |ui| {
                        ui.set_width(260.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new(self.simple_empty_field_hint())
                                    .size(14.0)
                                    .color(super::app_theme::SECONDARY_TEXT),
                            );
                            ui.add_space(8.0);
                            let place = self.simple_place_label();
                            if ui
                                .add_sized([220.0, 48.0], Self::simple_action_button(place, true))
                                .clicked()
                            {
                                self.set_field_tool(FieldTool::Place);
                            }
                        });
                    });
            });
    }

    fn simple_identity_fields(&mut self, ui: &mut egui::Ui) {
        self.ensure_performer_draft();
        if self.selected.len() != 1 {
            return;
        }
        let editable = self.is_editable_set_start();
        let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
        let mut number = self
            .performer_draft
            .as_ref()
            .map(|draft| draft.number.clone())
            .unwrap_or_default();
        let mut name = self
            .performer_draft
            .as_ref()
            .map(|draft| draft.name.clone())
            .unwrap_or_default();
        ui.label(i18n::registered(self.locale, "simple-mode.067"));
        let number_response = ui.add_enabled(
            editable,
            egui::TextEdit::singleline(&mut number).desired_width(f32::INFINITY),
        );
        ui.label(i18n::registered(self.locale, "simple-mode.068"));
        let name_response = ui.add_enabled(
            editable,
            egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY),
        );
        if (number_response.changed() || name_response.changed())
            && let Some(draft) = self.performer_draft.as_mut()
        {
            draft.number = number;
            draft.name = name;
            draft.label_dirty = true;
        }
        if (number_response.lost_focus()
            || name_response.lost_focus()
            || ((number_response.has_focus() || name_response.has_focus()) && enter))
            && self
                .performer_draft
                .as_ref()
                .is_some_and(|draft| draft.label_dirty)
        {
            self.commit_performer_draft();
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
        const SELECTION_ACCENT: Color32 = Color32::from_rgb(76, 163, 255);
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
                ui.ctx().set_cursor_icon(match self.field_tool {
                    FieldTool::Place if !editable => egui::CursorIcon::NotAllowed,
                    FieldTool::Place => egui::CursorIcon::Crosshair,
                    FieldTool::Move if self.drag_before.is_some() => egui::CursorIcon::Grabbing,
                    FieldTool::Move if !editable && !self.selected.is_empty() => {
                        egui::CursorIcon::NotAllowed
                    }
                    FieldTool::Move if self.selected.is_empty() => egui::CursorIcon::Default,
                    FieldTool::Move => egui::CursorIcon::Grab,
                    FieldTool::Select if self.drag_before.is_some() => egui::CursorIcon::Grabbing,
                    FieldTool::Select if hover_on_dot && editable => egui::CursorIcon::Grab,
                    FieldTool::Select => egui::CursorIcon::Default,
                });
            }
            if self.field_tool == FieldTool::Place
                && self.is_editable_set_start()
                && self.drag_before.is_none()
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
        for (index, &point) in self.frame_positions.iter().enumerate() {
            if self.selected.contains(&index) && self.drag_preview.is_none() {
                let pos = to_screen(point);
                painter.circle_stroke(pos, 11.0, Stroke::new(2.0, SELECTION_ACCENT));
                if self.selected.len() >= 2 {
                    self.paint_selection_rank_badge(&painter, pos, index);
                }
            }
        }
        if !interactive {
            return;
        }
        if self.document.performers.is_empty() {
            self.simple_empty_field_overlay(ui, rect);
        }
        let overlay_hit = self.document.performers.is_empty()
            && response.interact_pointer_pos().is_some_and(|pos| {
                egui::Rect::from_center_size(rect.center(), egui::vec2(280.0, 120.0)).contains(pos)
            });
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
            if self.field_tool == FieldTool::Place && response.clicked() && !overlay_hit {
                self.place_performer_at(from_screen(pointer), snap_now);
            } else if self.field_tool != FieldTool::Place && response.clicked() {
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
                    None if self.field_tool != FieldTool::Move && !additive => {
                        self.clear_selection();
                    }
                    None => {}
                }
            }
            if self.field_tool != FieldTool::Place && response.drag_started() {
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
    fn empty_roster_recommends_place() {
        let app = empty_simple_app();
        assert!(app.document.performers.is_empty());
        assert_eq!(app.simple_guide(), SimpleGuide::Place);
    }

    #[test]
    fn first_place_recommends_move_until_drag_tip_seen() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        assert_eq!(app.document.performers.len(), 1);
        assert_eq!(app.simple_guide(), SimpleGuide::Move);
        assert_eq!(app.field_tool, FieldTool::Move);
        app.onboarding.simple_drag_tip_seen = true;
        assert_eq!(app.simple_guide(), SimpleGuide::NextSet);
    }

    #[test]
    fn two_sets_recommend_play() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        app.onboarding.simple_drag_tip_seen = true;
        app.duplicate_current_set();
        assert!(app.document.sets.len() >= 2);
        assert_eq!(app.simple_guide(), SimpleGuide::Play);
    }

    #[test]
    fn everyday_verbs_match_the_beginner_spec() {
        let app = empty_simple_app();
        assert_eq!(app.simple_place_label(), "人を置く");
        assert_eq!(
            app.simple_empty_field_hint(),
            "クリックしてメンバーを置きます"
        );
        assert_eq!(app.simple_play_label(), "再生して確認");
        assert_eq!(app.simple_new_work_label(), "新しい作品");
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
    fn place_and_next_shape_show_a_success_toast() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        assert_eq!(app.status, "置けました");
        assert!(app.simple_mode.toast.is_some());
        app.onboarding.simple_drag_tip_seen = true;
        app.duplicate_current_set();
        assert_eq!(app.status, "次のセットを作りました");
        assert!(app.document.sets[1].name.contains("セット"));
    }

    #[test]
    fn confirm_flag_does_not_remove_people_by_itself() {
        let mut app = empty_simple_app();
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        app.field_tool = FieldTool::Place;
        app.place_performer_at(Point { x: 10.0, y: 6.0 }, true);
        app.replace_selection([0].into_iter().collect());
        let count = app.document.performers.len();
        app.simple_mode.confirm_remove = true;
        assert_eq!(app.document.performers.len(), count);
        app.remove_selected_performers();
        assert_eq!(app.document.performers.len(), count - 1);
    }
}
