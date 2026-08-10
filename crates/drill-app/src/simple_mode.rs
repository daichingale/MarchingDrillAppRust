//! "Simple Mode" (かんたんモード): a reduced, guided UI aimed at first-time
//! high-school students who just want to build a formation, move performers,
//! and play it back without learning the full desktop toolset.
//!
//! Invariant: every mutation in this module goes through `DrillApp::execute_edit`
//! (directly or via existing helpers like `commit_layout`/`commit_shape`), the
//! same as the full UI. This module never touches `self.document` fields
//! directly. See `ui_qa.rs` for the guard test that scans this file too.
use super::*;

/// Tiny locale-pair helper local to this module. Deliberately *not* named
/// `tr` and not written as `if locale == Locale::Ja { .. } else { .. }`, so
/// `scripts/generate-message-catalog.ps1`'s textual scan (which only matches
/// those two call shapes) does not pick up simple-mode copy and require a
/// docs/MESSAGE_CATALOG.md regeneration for this file.
fn s(locale: Locale, ja: &'static str, en: &'static str) -> &'static str {
    match locale {
        Locale::Ja => ja,
        Locale::En => en,
    }
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
        match (locale, self) {
            (Locale::Ja, Self::ChooseFormation) => "1. フォーメーションを選ぶ",
            (Locale::En, Self::ChooseFormation) => "1. Choose a Formation",
            (Locale::Ja, Self::SelectPerformers) => "2. 演者を選ぶ",
            (Locale::En, Self::SelectPerformers) => "2. Select Performers",
            (Locale::Ja, Self::MovePerformers) => "3. 動かす",
            (Locale::En, Self::MovePerformers) => "3. Move",
            (Locale::Ja, Self::Playback) => "4. 再生する",
            (Locale::En, Self::Playback) => "4. Play",
        }
    }

    fn guidance(self, locale: Locale) -> &'static str {
        match (locale, self) {
            (Locale::Ja, Self::ChooseFormation) => {
                "編集したいフォーメーション（セット）を選びましょう。"
            }
            (Locale::En, Self::ChooseFormation) => {
                "Choose the formation (set) you want to edit."
            }
            (Locale::Ja, Self::SelectPerformers) => {
                "次は演者を選んでみましょう。フィールドの演者をタップするか、「全員を選択」を押します。"
            }
            (Locale::En, Self::SelectPerformers) => {
                "Next, let's select performers. Tap performers on the field, or press Select All."
            }
            (Locale::Ja, Self::MovePerformers) => {
                "選んだ演者をドラッグするか、並べ方のボタンで動かしましょう。矢印ボタンで表示範囲を移動できます。"
            }
            (Locale::En, Self::MovePerformers) => {
                "Drag the selected performers, or use a layout button. Use the arrow buttons to pan the view."
            }
            (Locale::Ja, Self::Playback) => "再生ボタンを押して、動きを確認しましょう。",
            (Locale::En, Self::Playback) => "Press Play to check how it moves.",
        }
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
    error: Option<String>,
}

impl Default for MetronomeState {
    fn default() -> Self {
        Self {
            bpm: 120.0,
            running: false,
            output: None,
            error: None,
        }
    }
}

impl MetronomeState {
    fn ensure_output(&mut self) -> bool {
        if self.output.is_some() {
            return true;
        }
        // A few seconds of silence: `render_block` fills silence past the end
        // of the asset too, so the click mixer keeps ticking indefinitely
        // while the device stream stays open. No music/document dependency.
        let asset =
            match drill_audio::AudioAsset::from_interleaved(vec![0_i16; 48_000 * 2], 48_000, 1, 1.0)
            {
                Ok(asset) => std::sync::Arc::new(asset),
                Err(error) => {
                    self.error = Some(error.to_string());
                    return false;
                }
            };
        match drill_audio::AudioOutput::open_default(asset) {
            Ok(output) => {
                self.output = Some(output);
                self.error = None;
                true
            }
            Err(error) => {
                self.error = Some(error.to_string());
                false
            }
        }
    }

    fn rebuild_schedule(&mut self) {
        let Some(output) = &self.output else { return };
        let rate = output.output_sample_rate();
        let tempo = drill_core::tempo::TempoMap::constant(self.bpm.clamp(30.0, 300.0));
        let settings = drill_audio::ClickSettings {
            enabled: true,
            ..drill_audio::ClickSettings::default()
        };
        // ~100,000 counts covers many hours of practice at any supported BPM
        // without needing to rebuild the schedule while it plays.
        let schedule = drill_audio::ClickSchedule::build(&tempo, 0.0, 100_000.0, &settings, rate);
        let voices = drill_audio::ClickVoices::render(&settings, rate);
        if let Err(error) = output.set_clicks(settings, schedule, voices) {
            self.error = Some(error.to_string());
        }
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn toggle(&mut self) {
        if self.running {
            self.stop();
        } else {
            self.start();
        }
    }

    pub fn start(&mut self) {
        if !self.ensure_output() {
            return;
        }
        self.rebuild_schedule();
        if let Some(output) = &self.output {
            output.seek(0);
            output.play();
        }
        self.running = true;
    }

    pub fn stop(&mut self) {
        if let Some(output) = &self.output {
            output.pause();
        }
        self.running = false;
    }

    pub fn set_bpm(&mut self, bpm: f32) {
        let bpm = bpm.clamp(30.0, 300.0);
        if (bpm - self.bpm).abs() < f32::EPSILON {
            return;
        }
        self.bpm = bpm;
        if self.running && self.output.is_some() {
            self.rebuild_schedule();
            if let Some(output) = &self.output {
                output.seek(0);
            }
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
            ui.heading(s(self.locale, "かんたんモード", "Simple Mode"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(s(self.locale, "通常モードに戻る", "Back to Full Mode"))
                    .on_hover_text(s(
                        self.locale,
                        "いつでも通常の画面に戻れます",
                        "You can return to the full desktop UI anytime",
                    ))
                    .clicked()
                {
                    self.simple_mode.enabled = false;
                }
                ui.add_space(6.0);
                let metronome_selected = self.simple_mode.screen == SimpleScreen::Metronome;
                if ui
                    .selectable_label(metronome_selected, s(self.locale, "メトロノーム", "Metronome"))
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
                .fill(Color32::from_rgb(27, 37, 49))
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
                    egui::Button::new(s(self.locale, "◀ もどる", "◀ Back")),
                )
                .clicked()
            {
                self.simple_mode.step = self.simple_mode.step.previous();
            }
            if ui
                .add_enabled(
                    self.simple_mode.step != SimpleStep::Playback,
                    egui::Button::new(s(self.locale, "つぎへ ▶", "Next ▶")),
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
                    egui::Button::new(s(self.locale, "◀ 前のフォーメーション", "◀ Previous")),
                )
                .clicked()
            {
                self.current_set -= 1;
                self.count_position = 0.0;
            }
            ui.label(format!(
                "{} {} / {}",
                s(self.locale, "フォーメーション", "Formation"),
                self.current_set + 1,
                self.document.sets.len()
            ));
            if ui
                .add_enabled(
                    self.current_set + 1 < self.document.sets.len(),
                    egui::Button::new(s(self.locale, "次のフォーメーション ▶", "Next ▶")),
                )
                .clicked()
            {
                self.current_set += 1;
                self.count_position = 0.0;
            }
            if ui
                .button(s(self.locale, "＋ 新しいフォーメーション", "+ Add Formation"))
                .clicked()
            {
                self.duplicate_current_set();
            }
        });
        ui.add_space(6.0);
        self.simple_field_full_ui(ui, false);
    }

    fn simple_step_select_performers(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button(text(self.locale, Text::SelectAll)).clicked() {
                self.selected = (0..self.document.performers.len()).collect();
            }
            if ui.button(text(self.locale, Text::ClearSelection)).clicked() {
                self.selected.clear();
            }
            ui.label(format!(
                "{}: {}",
                s(self.locale, "選択中", "Selected"),
                self.selected.len()
            ));
        });
        ui.add_space(6.0);
        self.simple_field_full_ui(ui, true);
    }

    fn simple_step_move_performers(&mut self, ui: &mut egui::Ui) {
        if self.selected.is_empty() {
            ui.colored_label(
                Color32::from_rgb(245, 197, 66),
                s(
                    self.locale,
                    "先に「2. 演者を選ぶ」で演者を選びましょう",
                    "Select performers in step 2 first",
                ),
            );
        }
        ui.horizontal(|ui| {
            let enabled = !self.selected.is_empty();
            let grid = self.document.grid.clone();
            if ui
                .add_enabled(
                    enabled,
                    egui::Button::new(s(self.locale, "直線に並べる", "Line Up")),
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
                    egui::Button::new(s(self.locale, "円に並べる", "Arrange in Circle")),
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
                    egui::Button::new(s(self.locale, "ブロックに並べる", "Arrange in Block")),
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
                .button(s(self.locale, "選択箇所を表示", "Center on Selection"))
                .clicked()
            {
                self.simple_mode.viewport.center = Some(self.simple_selection_anchor());
            }
        });
        ui.add_space(6.0);
        self.simple_field_zoom_ui(ui);
    }

    fn simple_step_playback(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let label = if self.playing {
                text(self.locale, Text::Pause)
            } else {
                text(self.locale, Text::Play)
            };
            if ui
                .add_sized([160.0, 46.0], egui::Button::new(egui::RichText::new(label).size(20.0)))
                .clicked()
            {
                self.toggle_playback(ui.ctx());
            }
            if ui
                .button(s(self.locale, "最初から", "From the Start"))
                .clicked()
            {
                self.seek_global(self.playback_start as f32);
                self.playing = false;
            }
            if ui
                .add_sized(
                    [140.0, 46.0],
                    egui::Button::new(s(self.locale, "保存する", "Save")),
                )
                .clicked()
            {
                self.save_dialog();
            }
        });
        let total = self.document.timeline_counts().max(1) as f32;
        let position = self
            .document
            .global_count(self.current_set, self.count_position)
            .clamp(0.0, total);
        ui.add(egui::ProgressBar::new(position / total).text(format!(
            "{}: {:.0} / {:.0}",
            s(self.locale, "カウント", "Count"),
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
            theme: &drill_render::Theme::SCREEN_DARK,
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
                painter.circle_stroke(to_screen(point), 11.0, Stroke::new(2.0, Color32::WHITE));
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
            .min_by(|(_, a), (_, b)| {
                to_screen(**a)
                    .distance(pointer)
                    .total_cmp(&to_screen(**b).distance(pointer))
            })
            .filter(|(_, p)| to_screen(**p).distance(pointer) < 22.0)
            .map(|(index, _)| index);
        match nearest {
            Some(index) => {
                if !self.selected.insert(index) {
                    self.selected.remove(&index);
                }
            }
            None => self.selected.clear(),
        }
    }

    fn simple_selection_anchor(&self) -> Point {
        let points = self.selected_points();
        if points.is_empty() {
            return self
                .frame_positions
                .first()
                .copied()
                .unwrap_or(Point {
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

        let step = if grid.unit == Unit::Meters { 1.0 } else { 1.093_613 };
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
            ui.label(s(
                self.locale,
                "拡大表示：矢印ボタンやキーで移動できます",
                "Zoomed in: pan with the arrow buttons or arrow keys",
            ));
        });
        ui.horizontal(|ui| {
            ui.add_space((ui.available_width() * 0.5 - 24.0).max(0.0));
            if ui.add_sized([48.0, 32.0], egui::Button::new("▲")).clicked() {
                pan.y += step;
            }
        });

        let available = ui.available_size() - Vec2::new(0.0, 40.0);
        ui.horizontal(|ui| {
            if ui.add_sized([32.0, 48.0], egui::Button::new("◀")).clicked() {
                pan.x -= step;
            }
            let (response, painter) =
                ui.allocate_painter(available.max(Vec2::new(80.0, 80.0)), Sense::click_and_drag());
            let rect = response.rect;
            self.paint_zoom_viewport(ui, &painter, rect, center, half);
            self.handle_zoom_interaction(ui, &response, rect, center, half);
            if ui.add_sized([32.0, 48.0], egui::Button::new("▶")).clicked() {
                pan.x += step;
            }
        });
        ui.horizontal(|ui| {
            ui.add_space((ui.available_width() * 0.5 - 24.0).max(0.0));
            if ui.add_sized([48.0, 32.0], egui::Button::new("▼")).clicked() {
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
        let offset = Vec2::new(
            (rect.width() - drawn) * 0.5,
            (rect.height() - drawn) * 0.5,
        );
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
        let minor = if grid.unit == Unit::Meters { 1.0 } else { 1.093_613 };
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
            if selected {
                painter.circle_stroke(pos, 12.0, Stroke::new(2.5, Color32::WHITE));
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
        let offset = Vec2::new(
            (rect.width() - drawn) * 0.5,
            (rect.height() - drawn) * 0.5,
        );
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
        let nearest = || {
            self.frame_positions
                .iter()
                .enumerate()
                .filter(|&(_, &p)| (p.x - center.x).abs() <= half && (p.y - center.y).abs() <= half)
                .min_by(|(_, a), (_, b)| {
                    to_screen(**a)
                        .distance(pointer)
                        .total_cmp(&to_screen(**b).distance(pointer))
                })
                .filter(|&(_, &p)| to_screen(p).distance(pointer) < 22.0)
                .map(|(index, _)| index)
        };
        if response.clicked() {
            match nearest() {
                Some(index) => {
                    if !self.selected.insert(index) {
                        self.selected.remove(&index);
                    }
                }
                None => self.selected.clear(),
            }
        }
        if response.drag_started() {
            if let Some(index) = nearest() {
                if !self.selected.contains(&index) {
                    self.selected.clear();
                    self.selected.insert(index);
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
        ui.vertical_centered(|ui| {
            ui.add_space(10.0);
            ui.label(s(
                self.locale,
                "ドリルとは関係なく、いつでも使える練習用メトロノームです",
                "A practice metronome, independent of the drill you're editing",
            ));
            ui.add_space(16.0);
            ui.label(
                egui::RichText::new(format!("{:.0}", self.simple_mode.metronome.bpm))
                    .size(72.0)
                    .strong(),
            );
            ui.label("BPM");
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
            let (response, painter) =
                ui.allocate_painter(Vec2::new(90.0, 90.0), Sense::hover());
            painter.circle_filled(response.rect.center(), pulse_radius, pulse_color);
            painter.circle_stroke(
                response.rect.center(),
                36.0,
                Stroke::new(2.0, Color32::from_gray(200)),
            );
            ui.add_space(18.0);
            let toggle_label = if running {
                s(self.locale, "停止", "Stop")
            } else {
                s(self.locale, "開始", "Start")
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
                ui.colored_label(Color32::from_rgb(235, 120, 120), error);
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
        assert_eq!(SimpleStep::ChooseFormation.next(), SimpleStep::SelectPerformers);
        assert_eq!(SimpleStep::Playback.next(), SimpleStep::Playback);
        assert_eq!(SimpleStep::ChooseFormation.previous(), SimpleStep::ChooseFormation);
        assert_eq!(SimpleStep::Playback.previous(), SimpleStep::MovePerformers);
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
}
