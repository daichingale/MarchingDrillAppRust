use super::*;

impl eframe::App for DrillApp {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        self.update_state.poll();
        if let Some(event) = self.underlay_state.poll(ui.ctx()) {
            self.status = match event {
                underlay_state::UnderlayEvent::Ready { name, model } => {
                    let status = format!(
                        "{}: {name}",
                        super::i18n::registered(self.locale, "app-ui.001")
                    );
                    if let Some(model) = model {
                        let _ = self.history.execute(
                            &mut self.document,
                            Edit::SetImageUnderlay {
                                underlay: Some(model),
                            },
                        );
                        self.dirty = true;
                    }
                    status
                }
                underlay_state::UnderlayEvent::Failed(error) => format!(
                    "{}: {error}",
                    super::i18n::registered(self.locale, "app-ui.002"),
                    error = error.localized(self.locale),
                ),
            };
        }
        if self.underlay_state.busy() {
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
        if let Some(event) = self.text_export_state.poll() {
            self.status = match event {
                text_export_state::TextExportEvent::Written(path) => {
                    format!("書き出しました: {}", path.display())
                }
                text_export_state::TextExportEvent::Failed(error) => {
                    format!(
                        "{}: {}",
                        super::i18n::registered(self.locale, "app-ui.003"),
                        error.localized(self.locale)
                    )
                }
                text_export_state::TextExportEvent::Cancelled => {
                    "書き出しをキャンセルしました".into()
                }
            };
        }
        if self.text_export_state.is_running() {
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
        let visuals = ui.visuals_mut();
        visuals.override_text_color = None;
        visuals.widgets.noninteractive.fg_stroke.color = Color32::from_gray(225);
        // `inactive` is egui's normal enabled widget state, not the disabled state.
        // Keep it readable against the dark button fill; disabled widgets are
        // dimmed separately by egui's opacity handling.
        visuals.widgets.inactive.fg_stroke.color = Color32::from_gray(225);
        visuals.widgets.hovered.fg_stroke.color = Color32::WHITE;
        visuals.widgets.active.fg_stroke.color = Color32::WHITE;
        visuals.widgets.open.fg_stroke.color = Color32::WHITE;
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(36, 45, 58);
        visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(36, 45, 58);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(52, 66, 84);
        visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(52, 66, 84);
        visuals.widgets.active.bg_fill = Color32::from_rgb(67, 88, 112);
        visuals.widgets.active.weak_bg_fill = Color32::from_rgb(67, 88, 112);
        visuals.widgets.open.bg_fill = Color32::from_rgb(45, 58, 74);
        visuals.widgets.open.weak_bg_fill = Color32::from_rgb(45, 58, 74);
        visuals.selection.bg_fill = Color32::from_rgb(42, 112, 163);
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
        if let Some(event) = self.project_state.poll() {
            match event {
                project_state::ProjectEvent::Saved(path) => {
                    self.current_path = Some(path.clone());
                    self.dirty = false;
                    self.status = format!(
                        "{}: {}",
                        super::i18n::registered(self.locale, "app-ui.068"),
                        path.display()
                    );
                }
                project_state::ProjectEvent::Loaded { path, project } => {
                    let mut project = *project;
                    let embedded_audio = project.manifest.assets.iter().find_map(|entry| {
                        if entry.kind == drill_project::container::AssetKind::Audio {
                            project
                                .embedded
                                .remove(&entry.id)
                                .map(|bytes| (bytes, entry.original_name.clone()))
                        } else {
                            None
                        }
                    });
                    let embedded_image = project.manifest.assets.iter().find_map(|entry| {
                        (entry.kind == drill_project::container::AssetKind::Image)
                            .then(|| {
                                project
                                    .embedded
                                    .remove(&entry.id)
                                    .map(|bytes| (bytes, entry.original_name.clone()))
                            })
                            .flatten()
                    });
                    self.document = project.document;
                    self.tempo_bpm = self.document.tempo.bpm_at(0.0);
                    self.camera = Camera::press_box(&self.document.grid);
                    self.camera_program_preview = true;
                    self.section_manager.clear_drafts();
                    self.project_warnings = project.warnings;
                    self.current_path = Some(path);
                    self.current_set = 0;
                    self.count_position = 0.0;
                    self.selected.clear();
                    self.playback_start = 0;
                    self.playback_end = self.document.timeline_counts();
                    self.history = History::with_limit(500);
                    self.dirty = false;
                    self.status = super::i18n::registered(self.locale, "app-ui.069").into();
                    if let Some((bytes, name)) = embedded_audio {
                        self.audio_state.start_decode_bytes(bytes, &name);
                        self.status =
                            format!("プロジェクトを開きました · 埋込音源 {name} を準備中…");
                    }
                    if let Some((bytes, name)) = embedded_image {
                        self.underlay_state.load_bytes(bytes, name);
                    } else if self.document.underlay.is_some() {
                        self.underlay_state.remove();
                        self.status = super::i18n::registered(self.locale, "app-ui.004").into();
                    }
                }
                project_state::ProjectEvent::Failed(error) => {
                    self.status = format!(
                        "{}: {error}",
                        super::i18n::registered(self.locale, "app-ui.070")
                    );
                }
            }
        }
        if let Some(event) = self.import_state.poll() {
            match event {
                import_state::ImportEvent::Applied { document, report } => {
                    if let Err(error) = self
                        .history
                        .execute(&mut self.document, Edit::ReplaceDocument { document })
                    {
                        self.status = format!(
                            "{}: {error}",
                            super::i18n::registered(self.locale, "app-ui.071")
                        );
                        return;
                    }
                    self.current_set = 0;
                    self.count_position = 0.0;
                    self.playback_start = 0;
                    self.playback_end = self.document.timeline_counts();
                    self.timeline_view = TimelineViewport::fit(self.playback_end);
                    self.selected.clear();
                    self.dirty = true;
                    self.status = format!(
                        "インポート完了: {}行採用、{}行スキップ、演者{}名、セット{}個",
                        report.rows_accepted,
                        report.rows_skipped,
                        report.performers_created,
                        report.sets_created
                    );
                }
                import_state::ImportEvent::Failed(error) => {
                    self.status = format!(
                        "{}: {}",
                        super::i18n::registered(self.locale, "app-ui.005"),
                        error.localized(self.locale)
                    );
                }
            }
        }
        self.show_import_window(ui.ctx());
        self.show_musical_import_window(ui.ctx());
        self.plugin_state.show(ui.ctx(), self.locale);
        if self.import_state.busy() {
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
        self.project_state.heartbeat_if_due();
        if self.project_state.busy() {
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
        if let Some(event) = self.audio_state.poll() {
            match event {
                audio_state::DecodeEvent::Ready { duration } => {
                    if let Some(mut track) = self.document.audio.clone() {
                        track.duration_seconds = duration;
                        if self
                            .history
                            .execute(
                                &mut self.document,
                                Edit::SetAudioTrack {
                                    audio: Some(track.clone()),
                                },
                            )
                            .is_ok()
                        {
                            self.audio_draft = Some(track);
                            self.audio_draft_dirty = false;
                            self.dirty = true;
                        }
                    }
                    self.status = self.audio_state.status.text(self.locale);
                }
                audio_state::DecodeEvent::Failed(error) => {
                    self.status = format!(
                        "{}: {error}",
                        super::i18n::registered(self.locale, "app-ui.072")
                    )
                }
                audio_state::DecodeEvent::Cancelled => {
                    self.status = super::i18n::registered(self.locale, "app-ui.073").into()
                }
            }
        }
        if self.audio_state.is_loading() {
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
        self.export_state.poll();
        if self.export_state.is_running() || self.export_state.is_inspecting() {
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
        if let Some(track) = &self.document.audio {
            self.audio_state.set_mix(track.gain_linear(), track.muted);
        }
        if self.dirty && self.last_autosave.elapsed().as_secs() >= 30 {
            self.project_state
                .autosave(&self.document, self.current_path.as_deref());
            self.last_autosave = Instant::now();
        }
        if self.playing {
            let audio_count = if let (Some(seconds), Some(track)) = (
                self.audio_state.clock_seconds(),
                self.document.audio.as_ref(),
            ) {
                Some(drill_core::audio::audio_time_to_count(
                    track,
                    &self.document.tempo,
                    seconds,
                ))
            } else {
                None
            };
            let decision = controller::playback_decision(
                &self.document,
                controller::PlaybackInput {
                    global_count: self
                        .document
                        .global_count(self.current_set, self.count_position),
                    audio_count,
                    dt_seconds: dt,
                    speed: self.speed,
                    range_start: self.playback_start,
                    range_end: self.playback_end,
                    loop_enabled: self.loop_playback,
                },
            );
            match decision {
                controller::PlaybackDecision::Seek(count) => self.seek_global(count),
                controller::PlaybackDecision::LoopTo(count) => {
                    self.seek_global(count);
                    if audio_count.is_some()
                        && let Some(track) = &self.document.audio
                    {
                        let seconds = drill_core::audio::count_to_audio_time(
                            track,
                            &self.document.tempo,
                            count,
                        );
                        self.audio_state.seek_seconds(seconds);
                    }
                }
                controller::PlaybackDecision::StopAt(count) => {
                    self.seek_global(count);
                    self.playing = false;
                    self.audio_state.pause();
                }
            }
            ui.ctx().request_repaint_after(Duration::from_millis(16));
        }
        let set_counts = self.document.sets[self.current_set].counts.max(1) as f32;
        self.document.positions_at(
            self.current_set,
            self.count_position / set_counts,
            &mut self.frame_positions,
        );
        let save = ui.input_mut(|input| {
            input.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                egui::Key::S,
            ))
        });
        if save {
            self.save_dialog();
        }
        if let Some(command) = commands::consume_shortcut(ui, self.command_context(), self.locale) {
            self.execute_command(command, ui.ctx());
        }

        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(text(self.locale, Text::File), |ui| {
                if ui.button(text(self.locale, Text::OpenJson)).clicked() {
                    self.open_dialog();
                    ui.close();
                }
                if ui.button(text(self.locale, Text::OpenProject)).clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("DrillForge Project", &["drillproj"])
                        .pick_file()
                    {
                        self.project_state.load_project(path);
                    }
                    ui.close();
                }
                if ui
                    .button(super::i18n::registered(self.locale, "app-ui.006"))
                    .clicked()
                {
                    self.import_coordinates_dialog();
                    ui.close();
                }
                if ui
                    .button(super::i18n::registered(self.locale, "app-ui.007"))
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Music timeline", &["musicxml", "mxl", "xml", "mid", "midi"])
                        .pick_file()
                    {
                        self.import_state.choose_musical(path);
                    }
                    ui.close();
                }
                if ui
                    .button(super::i18n::registered(self.locale, "app-ui.008"))
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Image", &["png", "jpg", "jpeg"])
                        .pick_file()
                    {
                        self.underlay_state.load(path);
                    }
                    ui.close();
                }
                if self.underlay_state.texture.is_some() {
                    let visibility_changed = ui
                        .checkbox(
                            &mut self.underlay_state.visible,
                            super::i18n::registered(self.locale, "app-ui.009"),
                        )
                        .changed();
                    let opacity_changed = ui
                        .add(
                            egui::Slider::new(&mut self.underlay_state.opacity, 0.05..=0.85)
                                .text(super::i18n::registered(self.locale, "app-ui.010")),
                        )
                        .changed();
                    if (visibility_changed || opacity_changed)
                        && let Some(mut underlay) = self.document.underlay.clone()
                    {
                        underlay.placement.visible = self.underlay_state.visible;
                        underlay.placement.opacity = self.underlay_state.opacity;
                        let _ = self.history.execute(
                            &mut self.document,
                            Edit::SetImageUnderlay {
                                underlay: Some(underlay),
                            },
                        );
                        self.dirty = true;
                    }
                    if let Some(mut underlay) = self.document.underlay.clone() {
                        let mut placement_changed = false;
                        ui.collapsing(super::i18n::registered(self.locale, "app-ui.011"), |ui| {
                            ui.horizontal_wrapped(|ui| {
                                placement_changed |= ui
                                    .add(
                                        egui::DragValue::new(&mut underlay.placement.x)
                                            .speed(0.25)
                                            .prefix("X "),
                                    )
                                    .changed();
                                placement_changed |= ui
                                    .add(
                                        egui::DragValue::new(&mut underlay.placement.y)
                                            .speed(0.25)
                                            .prefix("Y "),
                                    )
                                    .changed();
                                placement_changed |= ui
                                    .add(
                                        egui::Slider::new(
                                            &mut underlay.placement.scale_x,
                                            0.05..=4.0,
                                        )
                                        .text(super::i18n::registered(self.locale, "app-ui.012")),
                                    )
                                    .changed();
                                placement_changed |= ui
                                    .add(
                                        egui::Slider::new(
                                            &mut underlay.placement.scale_y,
                                            0.05..=4.0,
                                        )
                                        .text(super::i18n::registered(self.locale, "app-ui.013")),
                                    )
                                    .changed();
                                let mut degrees = underlay.placement.rotation_radians.to_degrees();
                                if ui
                                    .add(
                                        egui::Slider::new(&mut degrees, -180.0..=180.0).text(
                                            super::i18n::registered(self.locale, "app-ui.014"),
                                        ),
                                    )
                                    .changed()
                                {
                                    underlay.placement.rotation_radians = degrees.to_radians();
                                    placement_changed = true;
                                }
                            });
                            ui.small(super::i18n::registered(self.locale, "app-ui.015"));
                        });
                        if placement_changed {
                            let _ = self.history.execute(
                                &mut self.document,
                                Edit::SetImageUnderlay {
                                    underlay: Some(underlay),
                                },
                            );
                            self.dirty = true;
                        }
                    }
                    if ui
                        .button(super::i18n::registered(self.locale, "app-ui.016"))
                        .clicked()
                    {
                        self.underlay_state.remove();
                        let _ = self.history.execute(
                            &mut self.document,
                            Edit::SetImageUnderlay { underlay: None },
                        );
                        self.dirty = true;
                        ui.close();
                    }
                }
                if ui
                    .button(super::i18n::registered(self.locale, "app-ui.017"))
                    .clicked()
                {
                    if self.underlay_state.undo() {
                        self.status = super::i18n::registered(self.locale, "app-ui.018").into();
                    }
                    ui.close();
                }
                ui.separator();
                if ui.button(text(self.locale, Text::SaveJson)).clicked() {
                    self.save_dialog();
                    ui.close();
                }
                ui.checkbox(
                    &mut self.embed_audio_in_project,
                    "音源をプロジェクトへ埋め込む",
                );
                if ui.button(text(self.locale, Text::SaveProject)).clicked() {
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
                    ui.close();
                }
            });
            ui.menu_button(text(self.locale, Text::Edit), |ui| {
                if ui
                    .add_enabled(
                        self.history.can_undo(),
                        egui::Button::new(text(self.locale, Text::Undo)),
                    )
                    .clicked()
                {
                    self.history.undo(&mut self.document);
                    self.dirty = true;
                    ui.close();
                }
                if ui
                    .add_enabled(
                        self.history.can_redo(),
                        egui::Button::new(text(self.locale, Text::Redo)),
                    )
                    .clicked()
                {
                    self.history.redo(&mut self.document);
                    self.dirty = true;
                    ui.close();
                }
                ui.separator();
                if ui.button(text(self.locale, Text::SelectAll)).clicked() {
                    self.selected = (0..self.document.performers.len()).collect();
                    ui.close();
                }
                if ui.button(text(self.locale, Text::ClearSelection)).clicked() {
                    self.selected.clear();
                    ui.close();
                }
            });
            ui.menu_button(super::i18n::registered(self.locale, "app-ui.019"), |ui| {
                self.command_menu(ui, CommandMenu::Set);
            });
            ui.menu_button(text(self.locale, Text::Playback), |ui| {
                if ui
                    .button(if self.playing {
                        text(self.locale, Text::Pause)
                    } else {
                        text(self.locale, Text::Play)
                    })
                    .clicked()
                {
                    self.toggle_playback(ui.ctx());
                    ui.close();
                }
                if ui.button(text(self.locale, Text::RangeStart)).clicked() {
                    self.seek_global(self.playback_start as f32);
                    self.playing = false;
                    ui.close();
                }
                ui.separator();
                if ui
                    .button(super::i18n::registered(self.locale, "app-ui.064"))
                    .clicked()
                {
                    let start = self.document.global_count(self.current_set, 0.0) as u32;
                    self.playback_start = start;
                    self.playback_end = (start
                        + u32::from(self.document.sets[self.current_set].counts))
                    .min(self.document.timeline_counts());
                    ui.close();
                }
                if ui.button(text(self.locale, Text::WholeShow)).clicked() {
                    self.playback_start = 0;
                    self.playback_end = self.document.timeline_counts();
                    ui.close();
                }
                ui.checkbox(&mut self.loop_playback, text(self.locale, Text::Loop));
            });
            ui.menu_button(text(self.locale, Text::View), |ui| {
                ui.checkbox(&mut self.show_guidance, text(self.locale, Text::Guidance));
                let mut show_grid = self.document.grid.show_step_grid;
                if ui
                    .checkbox(&mut show_grid, text(self.locale, Text::StepGrid))
                    .changed()
                {
                    let mut grid = self.document.grid.clone();
                    grid.show_step_grid = show_grid;
                    if self
                        .history
                        .execute(
                            &mut self.document,
                            Edit::ReplaceGrid {
                                grid,
                                scale_positions: false,
                            },
                        )
                        .is_ok()
                    {
                        self.dirty = true;
                    }
                }
            });
            ui.menu_button(super::i18n::registered(self.locale, "app-ui.020"), |ui| {
                self.command_menu(ui, CommandMenu::Workspace);
                ui.separator();
                if ui
                    .button(super::i18n::registered(self.locale, "app-ui.021"))
                    .clicked()
                {
                    self.plugin_state.open = true;
                    ui.close();
                }
                if ui
                    .button(super::i18n::registered(self.locale, "app-ui.022"))
                    .clicked()
                {
                    self.subset_snapshot_state.open = true;
                    ui.close();
                }
            });
            ui.menu_button(text(self.locale, Text::Help), |ui| {
                ui.menu_button(text(self.locale, Text::Language), |ui| {
                    ui.selectable_value(
                        &mut self.locale,
                        Locale::Ja,
                        text(Locale::Ja, Text::Japanese),
                    );
                    ui.selectable_value(
                        &mut self.locale,
                        Locale::En,
                        text(Locale::En, Text::English),
                    );
                });
                ui.separator();
                if ui.button(text(self.locale, Text::GettingStarted)).clicked() {
                    self.onboarding.show_help = true;
                    ui.close();
                }
                if ui
                    .button(super::i18n::registered(self.locale, "app-ui.065"))
                    .clicked()
                {
                    self.onboarding.show_welcome = true;
                    ui.close();
                }
                ui.separator();
                let mut beta =
                    self.update_state.preferences.channel == drill_updater::Channel::Beta;
                if ui
                    .checkbox(
                        &mut beta,
                        super::i18n::registered(self.locale, "app-ui.023"),
                    )
                    .changed()
                {
                    self.update_state.set_beta(beta);
                }
                if ui
                    .button(super::i18n::registered(self.locale, "app-ui.024"))
                    .clicked()
                {
                    // A network transport is intentionally not present yet. The
                    // background task reports offline without disturbing work.
                    let (month, day) = utc_month_day();
                    self.update_state.check(None, month, day);
                    ui.close();
                }
                let update_status = self.update_state.status.text(self.locale);
                if !update_status.is_empty() {
                    ui.label(update_status);
                }
                if ui
                    .button(UiCommand::LegalNotices.label(self.locale))
                    .clicked()
                {
                    self.execute_command(UiCommand::LegalNotices, ui.ctx());
                    ui.close();
                }
                ui.separator();
                ui.label(super::i18n::registered(self.locale, "app-ui.066"));
                ui.label(super::i18n::registered(self.locale, "app-ui.067"));
            });
        });

        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new("DRILLFORGE")
                    .size(24.0)
                    .strong()
                    .color(Color32::from_rgb(245, 197, 66)),
            );
            ui.label(
                egui::RichText::new("Marching Design Studio")
                    .italics()
                    .color(Color32::from_gray(160)),
            );
            if ui
                .button(text(self.locale, Text::Open))
                .on_hover_text(super::i18n::registered(self.locale, "app-ui.025"))
                .clicked()
            {
                self.open_dialog();
            }
            if ui
                .button(text(self.locale, Text::Save))
                .on_hover_text(super::i18n::registered(self.locale, "app-ui.026"))
                .clicked()
            {
                self.save_dialog();
            }
            ui.label(if self.dirty {
                format!("● {}", text(self.locale, Text::Unsaved))
            } else {
                format!("✓ {}", text(self.locale, Text::Saved))
            });
            ui.separator();
            if ui
                .button(if self.playing {
                    text(self.locale, Text::Pause)
                } else {
                    text(self.locale, Text::Play)
                })
                .clicked()
            {
                self.toggle_playback(ui.ctx());
            }
            if ui
                .button(super::i18n::registered(self.locale, "app-ui.027"))
                .on_hover_text(super::i18n::registered(self.locale, "app-ui.028"))
                .clicked()
            {
                self.seek_global(self.playback_start as f32);
                self.playing = false;
            }
            if ui
                .add_enabled(self.history.can_undo(), egui::Button::new("↶ Undo"))
                .clicked()
            {
                self.history.undo(&mut self.document);
                self.dirty = true;
            }
            if ui
                .add_enabled(self.history.can_redo(), egui::Button::new("↷ Redo"))
                .clicked()
            {
                self.history.redo(&mut self.document);
                self.dirty = true;
            }
            ui.add(
                egui::Slider::new(&mut self.speed, 0.25..=4.0).text(text(self.locale, Text::Speed)),
            );
            if ui
                .add(
                    egui::DragValue::new(&mut self.tempo_bpm)
                        .range(20.0..=300.0)
                        .suffix(" BPM"),
                )
                .on_hover_text(super::i18n::registered(self.locale, "app-ui.029"))
                .changed()
            {
                let mut tempo = self.document.tempo.clone();
                tempo.set(0.0, self.tempo_bpm);
                if self
                    .history
                    .execute(&mut self.document, Edit::SetTempoMap { tempo })
                    .is_ok()
                {
                    self.dirty = true;
                }
            }
            ui.separator();
            ui.label(format!(
                "{} {}",
                text(self.locale, Text::Performers),
                self.document.performers.len()
            ));
            ui.separator();
            ui.selectable_value(&mut self.view_mode, ViewMode::Field2D, "2D");
            ui.selectable_value(&mut self.view_mode, ViewMode::Stadium3D, "3D");
            if let Some(gpu) = &mut self.gpu {
                let mut enabled = gpu.enabled();
                if ui
                    .checkbox(&mut enabled, "GPU")
                    .on_hover_text(super::i18n::registered(self.locale, "app-ui.030"))
                    .changed()
                {
                    gpu.set_enabled(enabled);
                }
                ui.small(if gpu.active() {
                    "GPU instancing"
                } else if gpu.available() {
                    "CPU fallback (manual)"
                } else {
                    "CPU fallback (device error)"
                });
            } else {
                ui.small("CPU fallback");
            }
            if self.view_mode == ViewMode::Stadium3D {
                ui.toggle_value(
                    &mut self.camera_program_preview,
                    super::i18n::registered(self.locale, "app-ui.031"),
                )
                .on_hover_text(super::i18n::registered(self.locale, "app-ui.032"));
                if ui
                    .small_button(super::i18n::registered(self.locale, "app-ui.033"))
                    .clicked()
                {
                    self.camera = Camera::audience_view(&self.document.grid);
                    self.camera_program_preview = false;
                }
                if ui
                    .small_button(super::i18n::registered(self.locale, "app-ui.034"))
                    .clicked()
                {
                    self.camera = Camera::press_box(&self.document.grid);
                    self.camera_program_preview = false;
                }
                if ui
                    .small_button(super::i18n::registered(self.locale, "app-ui.035"))
                    .clicked()
                {
                    self.camera = Camera::overhead(&self.document.grid);
                    self.camera_program_preview = false;
                }
                if ui
                    .small_button(super::i18n::registered(self.locale, "app-ui.036"))
                    .clicked()
                {
                    self.camera = Camera::end_zone(&self.document.grid, true);
                    self.camera_program_preview = false;
                }
                if ui
                    .small_button(super::i18n::registered(self.locale, "app-ui.037"))
                    .on_hover_text(super::i18n::registered(self.locale, "app-ui.038"))
                    .clicked()
                    && let Some(camera_id) = self
                        .document
                        .camera_program
                        .tracks
                        .first()
                        .map(|track| track.id)
                {
                    let frame = drill_core::camera::CameraKeyframe::from_camera(
                        self.count_position,
                        self.camera,
                    );
                    if self
                        .history
                        .execute(
                            &mut self.document,
                            Edit::InsertCameraKeyframe {
                                camera_id,
                                keyframe: frame,
                            },
                        )
                        .is_ok()
                    {
                        self.camera_program_preview = true;
                        self.dirty = true;
                        self.status = format!(
                            "カメラキーフレームを Count {:.2} に保存しました",
                            self.count_position
                        );
                    }
                }
                let active_camera = self
                    .document
                    .camera_program
                    .active_track(self.count_position)
                    .map(|track| track.id);
                let exact_keyframe = active_camera.and_then(|camera_id| {
                    self.document
                        .camera_program
                        .tracks
                        .iter()
                        .find(|track| track.id == camera_id)
                        .and_then(|track| {
                            track
                                .keyframes()
                                .iter()
                                .find(|key| (key.count - self.count_position).abs() < 0.01)
                        })
                        .copied()
                        .map(|key| (camera_id, key))
                });
                if ui
                    .add_enabled(
                        exact_keyframe.is_some(),
                        egui::Button::new(super::i18n::registered(self.locale, "app-ui.039")),
                    )
                    .clicked()
                    && let Some((camera_id, key)) = exact_keyframe
                    && self
                        .history
                        .execute(
                            &mut self.document,
                            Edit::RemoveCameraKeyframe {
                                camera_id,
                                count: key.count,
                            },
                        )
                        .is_ok()
                {
                    self.dirty = true;
                }
                if let Some((camera_id, mut key)) = exact_keyframe {
                    ui.label(super::i18n::registered(self.locale, "app-ui.040"));
                    for (label, mode) in [
                        ("Smooth", drill_core::camera::CameraInterpolation::Smooth),
                        ("Linear", drill_core::camera::CameraInterpolation::Linear),
                        ("Hold", drill_core::camera::CameraInterpolation::Hold),
                    ] {
                        if ui
                            .selectable_label(key.interpolation == mode, label)
                            .clicked()
                        {
                            key.interpolation = mode;
                            if self
                                .history
                                .execute(
                                    &mut self.document,
                                    Edit::InsertCameraKeyframe {
                                        camera_id,
                                        keyframe: key,
                                    },
                                )
                                .is_ok()
                            {
                                self.dirty = true;
                            }
                        }
                    }
                }
                let exact_cut = self
                    .document
                    .camera_program
                    .cuts
                    .iter()
                    .find(|cut| (cut.count - self.count_position).abs() < 0.01)
                    .copied();
                if let Some(cut) = exact_cut {
                    if ui
                        .small_button(super::i18n::registered(self.locale, "app-ui.041"))
                        .clicked()
                        && self
                            .history
                            .execute(
                                &mut self.document,
                                Edit::RemoveCameraCut { count: cut.count },
                            )
                            .is_ok()
                    {
                        self.dirty = true;
                    }
                } else if ui
                    .small_button(super::i18n::registered(self.locale, "app-ui.042"))
                    .clicked()
                    && let Some(camera) = active_camera
                    && self
                        .history
                        .execute(
                            &mut self.document,
                            Edit::InsertCameraCut {
                                cut: drill_core::camera::CameraCut {
                                    count: self.count_position,
                                    camera,
                                },
                            },
                        )
                        .is_ok()
                {
                    self.dirty = true;
                }
            }
        });
        if self.show_guidance {
            egui::Frame::new()
                .fill(Color32::from_rgb(29, 38, 51))
                .inner_margin(8)
                .corner_radius(5)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            egui::RichText::new(super::i18n::registered(self.locale, "app-ui.043"))
                                .strong(),
                        );
                        ui.label("→");
                        ui.label(
                            egui::RichText::new(super::i18n::registered(self.locale, "app-ui.044"))
                                .strong(),
                        );
                        ui.label("→");
                        ui.label(
                            egui::RichText::new(super::i18n::registered(self.locale, "app-ui.045"))
                                .strong(),
                        );
                        ui.separator();
                        ui.label(super::i18n::registered(self.locale, "app-ui.046"));
                    });
                });
        }
        self.onboarding
            .observe(!self.selected.is_empty(), self.dirty, self.ever_played);
        if let Some((step, message)) = self.onboarding.coach_message(self.locale) {
            egui::Frame::new()
                .fill(Color32::from_rgb(38, 55, 72))
                .inner_margin(8)
                .corner_radius(5)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            egui::RichText::new(step)
                                .strong()
                                .color(Color32::from_rgb(245, 197, 66)),
                        );
                        ui.label(message);
                        if ui
                            .small_button(super::i18n::registered(self.locale, "app-ui.047"))
                            .clicked()
                        {
                            self.onboarding.coach_dismissed = true;
                        }
                    });
                });
        }
        if !self.project_state.crashes.is_empty() && !self.crash_notice_dismissed {
            egui::Frame::new()
                .fill(Color32::from_rgb(63, 31, 35))
                .inner_margin(8)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new(text(self.locale, Text::CrashTitle)).strong());
                        ui.label(text(self.locale, Text::CrashHint));
                    });
                    ui.horizontal(|ui| {
                        if ui.button(text(self.locale, Text::CopyReportPath)).clicked()
                            && let Some(report) = self.project_state.crashes.first()
                        {
                            ui.ctx().copy_text(report.path.display().to_string());
                        }
                        if ui.button(text(self.locale, Text::Ignore)).clicked() {
                            self.crash_notice_dismissed = true;
                        }
                    });
                });
        }
        if !self.project_state.recoveries.is_empty() {
            let mut open = None;
            let mut ignore = None;
            egui::Frame::new()
                .fill(Color32::from_rgb(54, 42, 20))
                .inner_margin(8)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(text(self.locale, Text::RecoveryTitle)).strong());
                    ui.small(text(self.locale, Text::RecoveryHint));
                    for (index, candidate) in self.project_state.recoveries.iter().enumerate() {
                        ui.horizontal_wrapped(|ui| {
                            let title = if candidate.meta.document_title.is_empty() {
                                super::i18n::registered(self.locale, "app-ui.048")
                            } else {
                                &candidate.meta.document_title
                            };
                            ui.label(match self.locale {
                                Locale::Ja => format!(
                                    "{} · {} KB · {}秒前",
                                    title,
                                    candidate.autosave_bytes / 1024,
                                    candidate.stale_for.as_secs()
                                ),
                                Locale::En => format!(
                                    "{} · {} KB · {}s ago",
                                    title,
                                    candidate.autosave_bytes / 1024,
                                    candidate.stale_for.as_secs()
                                ),
                            });
                            if ui.button(text(self.locale, Text::Open)).clicked() {
                                open = Some(index);
                            }
                            if ui.button(text(self.locale, Text::Ignore)).clicked() {
                                ignore = Some(index);
                            }
                        });
                    }
                });
            if let Some(index) = open {
                self.project_state.load_recovery(index);
            }
            if let Some(index) = ignore {
                self.project_state.ignore_recovery(index);
            }
        }
        if !self.project_warnings.is_empty() {
            ui.colored_label(
                Color32::from_rgb(255, 184, 77),
                match self.locale {
                    Locale::Ja => format!(
                        "⚠ 外部アセットの問題 {} 件。音源パネルから再リンクしてください。",
                        self.project_warnings.len()
                    ),
                    Locale::En => format!(
                        "⚠ {} external asset issue(s). Relink them in the Audio panel.",
                        self.project_warnings.len()
                    ),
                },
            );
        }
        let total_counts = self.document.timeline_counts();
        egui::Frame::new()
            .fill(Color32::from_rgb(20, 27, 36))
            .inner_margin(8)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        egui::RichText::new(super::i18n::registered(self.locale, "app-ui.049"))
                            .strong(),
                    );
                    if ui
                        .button(super::i18n::registered(self.locale, "app-ui.050"))
                        .clicked()
                    {
                        self.playback_start =
                            self.document.global_count(self.current_set, 0.0) as u32;
                        self.playback_end = (self.playback_start
                            + u32::from(self.document.sets[self.current_set].counts))
                        .min(total_counts);
                    }
                    if ui
                        .button(super::i18n::registered(self.locale, "app-ui.051"))
                        .clicked()
                    {
                        self.playback_start = 0;
                        self.playback_end = total_counts;
                    }
                    let current_global = self
                        .document
                        .global_count(self.current_set, self.count_position)
                        .round() as u32;
                    if ui
                        .button(super::i18n::registered(self.locale, "app-ui.052"))
                        .clicked()
                    {
                        self.playback_start =
                            current_global.min(self.playback_end.saturating_sub(1));
                    }
                    if ui
                        .button(super::i18n::registered(self.locale, "app-ui.053"))
                        .clicked()
                    {
                        self.playback_end = current_global
                            .max(self.playback_start + 1)
                            .min(total_counts);
                    }
                    ui.checkbox(
                        &mut self.loop_playback,
                        super::i18n::registered(self.locale, "app-ui.054"),
                    );
                    ui.label(format!(
                        "COUNT {} → {}",
                        self.playback_start, self.playback_end
                    ));
                });
            });
        let current_global = self
            .document
            .global_count(self.current_set, self.count_position);
        let accepts_timeline_shortcut = !ui.ctx().egui_wants_keyboard_input();
        if accepts_timeline_shortcut && ui.input(|input| input.key_pressed(egui::Key::I)) {
            self.playback_start =
                (current_global.round() as u32).min(self.playback_end.saturating_sub(1));
        }
        if accepts_timeline_shortcut && ui.input(|input| input.key_pressed(egui::Key::O)) {
            self.playback_end = (current_global.round() as u32)
                .max(self.playback_start + 1)
                .min(total_counts);
        }
        if self.timeline_follow
            && self.playing
            && !self
                .timeline_view
                .contains_with_margin(current_global, 0.08)
        {
            self.timeline_view.center_on(current_global, total_counts);
        }
        ui.horizontal(|ui| {
            ui.strong(super::i18n::registered(self.locale, "app-ui.055"));
            let zoom_out = ui.button("−");
            zoom_out.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    super::i18n::registered(self.locale, "app-ui.056"),
                )
            });
            if zoom_out
                .on_hover_text(super::i18n::registered(self.locale, "app-ui.057"))
                .clicked()
            {
                self.timeline_view
                    .zoom_at(1.5, current_global, total_counts);
            }
            let zoom_in = ui.button("＋");
            zoom_in.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    super::i18n::registered(self.locale, "app-ui.058"),
                )
            });
            if zoom_in
                .on_hover_text(super::i18n::registered(self.locale, "app-ui.059"))
                .clicked()
            {
                self.timeline_view
                    .zoom_at(2.0 / 3.0, current_global, total_counts);
            }
            if ui
                .button(super::i18n::registered(self.locale, "app-ui.060"))
                .clicked()
            {
                self.timeline_view = TimelineViewport::fit(total_counts);
            }
            if ui
                .button(super::i18n::registered(self.locale, "app-ui.061"))
                .clicked()
            {
                self.timeline_view = TimelineViewport {
                    start: self.playback_start as f32,
                    span: self.playback_end.saturating_sub(self.playback_start).max(1) as f32,
                };
                self.timeline_view.normalize(total_counts);
            }
            ui.checkbox(
                &mut self.timeline_follow,
                super::i18n::registered(self.locale, "app-ui.062"),
            );
            ui.small(format!(
                "表示 {:.0}–{:.0} / {} · Ctrl+ホイール: 拡大縮小 · ホイール/中ドラッグ: 移動",
                self.timeline_view.start,
                self.timeline_view.start + self.timeline_view.span,
                total_counts
            ));
        });
        let timeline_change = draw_count_track(
            ui,
            &self.document,
            self.current_set,
            self.count_position,
            self.playback_start,
            self.playback_end,
            &mut self.timeline_view,
            self.locale,
        );
        if let Some((start, end)) = timeline_change.range {
            self.playback_start = start;
            self.playback_end = end;
        }
        if timeline_change.viewport_interacted {
            self.timeline_follow = false;
        }
        if let Some((set_index, local_count)) = timeline_change.seek {
            self.current_set = set_index;
            self.count_position = local_count;
            self.playing = false;
            self.audio_state.pause();
            if let Some(track) = &self.document.audio {
                let global = self.document.global_count(set_index, local_count);
                self.audio_state
                    .seek_seconds(drill_core::audio::count_to_audio_time(
                        track,
                        &self.document.tempo,
                        global,
                    ));
            }
            self.selected.clear();
        }
        ui.horizontal_top(|ui| {
            self.show_workspace_inspector(ui, set_counts);
            let available = ui.available_size();
            let (response, painter) = ui.allocate_painter(available, Sense::click_and_drag());
            let rect = response.rect.shrink(18.0);
            if self.view_mode == ViewMode::Stadium3D {
                self.draw_stadium(ui, &response, &painter, rect);
            } else {
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
                drill_render::build_field_2d(
                    &scene,
                    &mut self.render_scratch,
                    &mut self.display_list,
                );
                if let Some(gpu) = self.gpu.as_ref().filter(|gpu| gpu.active()) {
                    gpu.update(&self.display_list);
                    egui_backend::paint_gpu_background(&painter, rect.min, &self.display_list);
                    painter.add(gpu.callback(rect));
                    egui_backend::paint_gpu_foreground(&painter, rect.min, &self.display_list);
                } else {
                    egui_backend::paint(&painter, rect.min, &self.display_list);
                }
                self.underlay_state.paint(
                    &painter,
                    rect,
                    self.document
                        .underlay
                        .as_ref()
                        .map(|value| &value.placement),
                );
                if self.selected.is_empty() {
                    let card = Rect::from_min_size(
                        rect.left_top() + Vec2::new(18.0, 42.0),
                        Vec2::new(310.0, 76.0),
                    );
                    painter.rect_filled(card, 8.0, Color32::from_black_alpha(190));
                    painter.rect_stroke(
                        card,
                        8.0,
                        Stroke::new(1.0, Color32::from_rgb(245, 197, 66)),
                        StrokeKind::Inside,
                    );
                    painter.text(
                        card.left_top() + Vec2::new(14.0, 12.0),
                        egui::Align2::LEFT_TOP,
                        "編集を始めましょう",
                        egui::FontId::proportional(18.0),
                        Color32::WHITE,
                    );
                    painter.text(
                        card.left_top() + Vec2::new(14.0, 42.0),
                        egui::Align2::LEFT_TOP,
                        "黄色い演者をクリック → ドラッグで移動",
                        egui::FontId::proportional(13.0),
                        Color32::from_gray(205),
                    );
                }
                let grid_width = self.document.grid.width;
                let grid_height = self.document.grid.height;
                // Must stay in lockstep with the mapping `build_field_2d` used to
                // place the dots this frame (same grid, viewport, and margin) —
                // otherwise the selection ring and hit-testing drift away from
                // where the performer is actually drawn whenever the viewport's
                // aspect ratio doesn't match the field's. See FieldMap's docs.
                let field_map = drill_render::FieldMap::new(
                    grid_width,
                    grid_height,
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
                if !self.formation_preview_points.is_empty() {
                    for pair in self.formation_preview_points.windows(2) {
                        painter.line_segment(
                            [to_screen(pair[0]), to_screen(pair[1])],
                            Stroke::new(2.0, Color32::from_rgb(80, 220, 255)),
                        );
                    }
                    for &point in &self.formation_preview_points {
                        painter.circle_stroke(
                            to_screen(point),
                            6.0,
                            Stroke::new(2.0, Color32::from_rgb(100, 235, 255)),
                        );
                    }
                }
                if self.free_draw_raw.len() >= 2 {
                    for pair in self.free_draw_raw.windows(2) {
                        painter.line_segment(
                            [to_screen(pair[0]), to_screen(pair[1])],
                            Stroke::new(2.5, Color32::from_rgb(100, 235, 255)),
                        );
                    }
                }
                if let Some(preview) = &self.drag_preview {
                    for &point in preview {
                        painter.circle_filled(
                            to_screen(point),
                            7.0,
                            Color32::from_rgb(100, 235, 255),
                        );
                    }
                }
                for (index, &point) in self.frame_positions.iter().enumerate() {
                    let pos = to_screen(point);
                    let selected = self.selected.contains(&index);
                    if selected {
                        painter.circle_stroke(pos, 11.0, Stroke::new(2.0, Color32::WHITE));
                    }
                }
                if let Some(pointer) = response.interact_pointer_pos() {
                    if self.free_draw_active && (response.drag_started() || response.dragged()) {
                        let unclamped = from_screen(pointer);
                        let point = Point {
                            x: unclamped.x.clamp(0.0, grid_width),
                            y: unclamped.y.clamp(0.0, grid_height),
                        };
                        let sufficiently_far = self
                            .free_draw_raw
                            .last()
                            .is_none_or(|last| to_screen(*last).distance(pointer) >= 2.0);
                        if sufficiently_far
                            && self.free_draw_raw.len() < shapes::MAX_RAW_PATH_POINTS
                            && point.x.is_finite()
                            && point.y.is_finite()
                        {
                            self.free_draw_raw.push(point);
                        }
                    }
                    if self.free_draw_active && response.drag_stopped() {
                        self.finish_free_draw_preview();
                    }
                    let nearest = || {
                        self.frame_positions
                            .iter()
                            .enumerate()
                            .min_by(|(_, a), (_, b)| {
                                to_screen(**a)
                                    .distance(pointer)
                                    .total_cmp(&to_screen(**b).distance(pointer))
                            })
                            .filter(|(_, p)| to_screen(**p).distance(pointer) < 18.0)
                            .map(|(i, _)| i)
                    };
                    if !self.free_draw_active && response.clicked() {
                        if let Some(index) = nearest() {
                            let additive =
                                ui.input(|input| input.modifiers.command || input.modifiers.ctrl);
                            if additive {
                                if !self.selected.insert(index) {
                                    self.selected.remove(&index);
                                }
                            } else {
                                self.selected.clear();
                                self.selected.insert(index);
                            }
                        } else {
                            self.selected.clear();
                        }
                    }
                    if !self.free_draw_active
                        && response.drag_started()
                        && self.count_position == 0.0
                        && let Some(index) = nearest()
                    {
                        if !self.selected.contains(&index) {
                            self.selected.clear();
                            self.selected.insert(index);
                        }
                        self.drag_before = Some(
                            self.selected
                                .iter()
                                .map(|&i| self.document.sets[self.current_set].positions[i])
                                .collect(),
                        );
                        self.drag_preview = self.drag_before.clone();
                        self.drag_origin = Some(pointer);
                    }
                    if !self.free_draw_active && response.drag_started() && nearest().is_none() {
                        self.marquee_origin = Some(pointer);
                    }
                    if !self.free_draw_active
                        && response.dragged()
                        && let Some(origin) = self.marquee_origin
                    {
                        let marquee = Rect::from_two_pos(origin, pointer).intersect(rect);
                        painter.rect_filled(
                            marquee,
                            0.0,
                            Color32::from_rgba_unmultiplied(70, 160, 255, 35),
                        );
                        painter.rect_stroke(
                            marquee,
                            0.0,
                            Stroke::new(1.5, Color32::from_rgb(95, 180, 255)),
                            StrokeKind::Inside,
                        );
                    }
                    if !self.free_draw_active
                        && response.dragged()
                        && self.count_position == 0.0
                        && let (Some(before), Some(origin)) = (&self.drag_before, self.drag_origin)
                    {
                        let preview = self.drag_preview.get_or_insert_with(Vec::new);
                        preview.clear();
                        for &start in before {
                            preview.push(controller::drag_point(
                                start,
                                (pointer.x - origin.x, pointer.y - origin.y),
                                field_map.scale,
                                &self.document,
                            ));
                        }
                    }
                    if !self.free_draw_active
                        && response.drag_stopped()
                        && let Some(before) = self.drag_before.take()
                    {
                        let after = self.drag_preview.take().unwrap_or_else(|| before.clone());
                        if before != after {
                            let set_id = self.document.sets[self.current_set].id;
                            let performer_ids = self
                                .selected
                                .iter()
                                .filter_map(|&index| {
                                    self.document.performers.get(index).map(|p| p.id)
                                })
                                .collect();
                            self.execute_edit(
                                Edit::MovePerformers {
                                    set_id,
                                    performer_ids,
                                    positions: after,
                                },
                                super::i18n::registered(self.locale, "app-ui.063"),
                            );
                        }
                        self.drag_origin = None;
                    }
                    if !self.free_draw_active
                        && response.drag_stopped()
                        && let Some(origin) = self.marquee_origin.take()
                    {
                        let marquee = Rect::from_two_pos(origin, pointer);
                        let additive =
                            ui.input(|input| input.modifiers.command || input.modifiers.ctrl);
                        if !additive {
                            self.selected.clear();
                        }
                        for (index, &point) in self.frame_positions.iter().enumerate() {
                            if marquee.contains(to_screen(point)) {
                                self.selected.insert(index);
                            }
                        }
                    }
                }
            }
        });
        self.show_section_manager(ui.ctx());
        self.show_subset_snapshot_workspace(ui.ctx());
        self.show_print_workspace(ui.ctx());
        legal_notices::show(ui.ctx(), &mut self.show_legal_notices, self.locale);
        self.show_update_notice(ui.ctx());
        self.onboarding.help_ui(ui.ctx(), self.locale);
        match self.onboarding.welcome_ui(ui.ctx(), self.locale) {
            Some(onboarding::WelcomeAction::OpenJson) => self.open_dialog(),
            Some(onboarding::WelcomeAction::OpenProject) => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("DrillForge Project", &["drillproj"])
                    .pick_file()
                {
                    self.project_state.load_project(path);
                }
            }
            None => {}
        }
        self.onboarding.persist_if_changed();
    }
}
