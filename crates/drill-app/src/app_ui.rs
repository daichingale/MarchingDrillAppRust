use super::*;

impl eframe::App for DrillApp {
    /// This app never wraps its content in `egui::CentralPanel`, so the
    /// canvas behind every widget is exactly this clear color. eframe's
    /// default implementation ignores `visuals` and hardcodes a near-black
    /// gray, which happened to look plausible under the dark app themes but
    /// left the whole window canvas black under the Daylight theme (all
    /// widgets themed correctly, but painted over a background that never
    /// changed). `visuals` here is `egui_ctx.global_style().visuals`, i.e.
    /// exactly the currently active `AppTheme`, so this keeps the canvas in
    /// sync with whatever the user picked from the Color Theme menu.
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        // Native close requests are advisory for this frame.  Cancel first so
        // the confirmation sheet always has a chance to be painted.
        self.guard_close_request(ui.ctx());
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
        // Theming lives entirely in app_theme.rs / DrillApp::new now: this
        // used to re-hardcode the Studio theme's colors here every frame,
        // which silently clobbered any other theme the user picked from the
        // Color Theme menu before this widget tree ever got a chance to
        // paint with it.
        ui.visuals_mut().override_text_color = None;
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
                    if self.close_guard == CloseGuard::Saving {
                        self.close_guard = CloseGuard::Idle;
                        self.status =
                            super::i18n::registered(self.locale, "close-guard.007").into();
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if let DocumentOpenGuard::Saving(kind) = self.document_open_guard.clone() {
                        self.document_open_guard = DocumentOpenGuard::Idle;
                        self.begin_open_target(kind);
                    }
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
                    self.recent_projects.remember(path.clone());
                    self.current_path = Some(path);
                    self.current_set = 0;
                    self.count_position = 0.0;
                    self.reset_selection_for_document();
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
                    // A failed save must return the close sheet to its choice
                    // state. Leaving it as `Saving` would trap the user behind
                    // a spinner even though the worker has already finished.
                    if self.close_guard == CloseGuard::Saving {
                        self.close_guard = CloseGuard::Prompt;
                    }
                    if matches!(self.document_open_guard, DocumentOpenGuard::Saving(_)) {
                        self.document_open_guard = match self.document_open_guard.clone() {
                            DocumentOpenGuard::Saving(kind) => DocumentOpenGuard::Prompt(kind),
                            state => state,
                        };
                    }
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
                    self.reset_selection_for_document();
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
        {
            let mut production_sheet_workspace =
                std::mem::take(&mut self.production_sheet_workspace);
            if self.workspace_focus == Some(WorkspaceFocus::ProductionSheet) {
                production_sheet_workspace.open = true;
                self.workspace_focus = None;
            }
            production_sheet_workspace.show(self, ui.ctx());
            self.production_sheet_workspace = production_sheet_workspace;
        }
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
        self.command_palette.open_if_requested(ui.ctx());
        if !self.simple_mode.enabled {
            self.set_navigator.open_if_requested(ui.ctx());
        }
        let palette_open = self.command_palette.is_open();
        // Preview cancellation has priority over the normal Escape command
        // (which clears selection). A visible proposed formation is a pending,
        // non-destructive action, so Escape must dismiss that proposal first.
        let cancel_preview = !palette_open
            && (self.formation_preview_spec.is_some()
                || self.free_draw_active
                || self.clipboard_paste_preview.is_some())
            && !ui.ctx().egui_wants_keyboard_input()
            && ui.input_mut(|input| {
                input.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::NONE,
                    egui::Key::Escape,
                ))
            });
        if cancel_preview {
            if self.clipboard_paste_preview.is_some() {
                self.cancel_clipboard_paste_preview();
            } else {
                self.cancel_shape_preview();
            }
        }
        // Escape closes a non-destructive A/B reference before it is allowed
        // to fall through to Edit > Clear Selection. This mirrors native
        // macOS transient inspectors and avoids unexpectedly losing a working
        // group while simply leaving comparison mode.
        let close_comparison = !palette_open
            && !cancel_preview
            && self.set_comparison.is_some()
            && !ui.ctx().egui_wants_keyboard_input()
            && ui.input_mut(|input| {
                input.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::NONE,
                    egui::Key::Escape,
                ))
            });
        if close_comparison {
            self.set_comparison = None;
            self.status = super::i18n::registered(self.locale, "comparison.009").into();
        }
        let exit_focus_field = !palette_open
            && !cancel_preview
            && !close_comparison
            && self.focus_field
            && !ui.ctx().egui_wants_keyboard_input()
            && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if exit_focus_field {
            self.execute_command(UiCommand::ToggleFocusField, ui.ctx());
        }
        if !palette_open
            && !exit_focus_field
            && let Some(command) =
                commands::consume_shortcut(ui, self.command_context(), self.locale)
        {
            self.execute_command(command, ui.ctx());
        }

        if self.simple_mode.enabled {
            self.simple_ui(ui);
            return;
        }

        if !self.focus_field {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button(text(self.locale, Text::File), |ui| {
                    self.command_menu(ui, CommandMenu::File);
                    ui.separator();
                    self.recent_projects.prune_missing();
                    ui.menu_button(
                        super::i18n::registered(self.locale, "recent-projects.007"),
                        |ui| {
                            let paths = self.recent_projects.paths().to_vec();
                            if paths.is_empty() {
                                ui.add_enabled(
                                    false,
                                    egui::Button::new(super::i18n::registered(
                                        self.locale,
                                        "recent-projects.008",
                                    )),
                                );
                            }
                            for path in paths {
                                let name = path
                                    .file_name()
                                    .and_then(|name| name.to_str())
                                    .map(str::to_owned)
                                    .unwrap_or_else(|| path.to_string_lossy().into_owned());
                                if ui
                                    .button(name)
                                    .on_hover_text(path.display().to_string())
                                    .clicked()
                                {
                                    self.request_open_recent(path);
                                    ui.close();
                                }
                            }
                            if !self.recent_projects.paths().is_empty() {
                                ui.separator();
                                if ui
                                    .button(super::i18n::registered(
                                        self.locale,
                                        "recent-projects.009",
                                    ))
                                    .clicked()
                                {
                                    self.recent_projects.clear();
                                    ui.close();
                                }
                            }
                        },
                    );
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
                            ui.collapsing(
                                super::i18n::registered(self.locale, "app-ui.011"),
                                |ui| {
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
                                                .text(super::i18n::registered(
                                                    self.locale,
                                                    "app-ui.012",
                                                )),
                                            )
                                            .changed();
                                        placement_changed |= ui
                                            .add(
                                                egui::Slider::new(
                                                    &mut underlay.placement.scale_y,
                                                    0.05..=4.0,
                                                )
                                                .text(super::i18n::registered(
                                                    self.locale,
                                                    "app-ui.013",
                                                )),
                                            )
                                            .changed();
                                        let mut degrees =
                                            underlay.placement.rotation_radians.to_degrees();
                                        if ui
                                            .add(
                                                egui::Slider::new(&mut degrees, -180.0..=180.0)
                                                    .text(super::i18n::registered(
                                                        self.locale,
                                                        "app-ui.014",
                                                    )),
                                            )
                                            .changed()
                                        {
                                            underlay.placement.rotation_radians =
                                                degrees.to_radians();
                                            placement_changed = true;
                                        }
                                    });
                                    ui.small(super::i18n::registered(self.locale, "app-ui.015"));
                                },
                            );
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
                    ui.checkbox(
                        &mut self.embed_audio_in_project,
                        "音源をプロジェクトへ埋め込む",
                    );
                });
                ui.menu_button(text(self.locale, Text::Edit), |ui| {
                    self.command_menu(ui, CommandMenu::Edit);
                });
                ui.menu_button(super::i18n::registered(self.locale, "commands.135"), |ui| {
                    self.command_menu(ui, CommandMenu::Arrange)
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
                        self.seek_to_count(self.playback_start);
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
                    if ui
                        .button(super::i18n::registered(self.locale, "set-navigator.009"))
                        .clicked()
                    {
                        self.set_navigator.open();
                        ui.close();
                    }
                    ui.separator();
                    ui.label(super::i18n::registered(self.locale, "workspace-preset.007"));
                    for command in [
                        UiCommand::WorkspaceDesign,
                        UiCommand::WorkspaceReview,
                        UiCommand::WorkspacePresent,
                    ] {
                        if ui.button(command.label(self.locale)).clicked() {
                            self.execute_command(command, ui.ctx());
                            ui.close();
                        }
                    }
                    ui.separator();
                    let simple_mode_label = match self.locale {
                        Locale::Ja => "簡単モード",
                        Locale::En => "Simple Mode",
                    };
                    if ui
                        .checkbox(&mut self.simple_mode.enabled, simple_mode_label)
                        .changed()
                    {
                        ui.close();
                    }
                    ui.checkbox(&mut self.show_guidance, text(self.locale, Text::Guidance));
                    ui.checkbox(
                        &mut self.show_inspector,
                        super::i18n::registered(self.locale, "app-ui.074"),
                    );
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
                    ui.separator();
                    // App chrome only: the field view keeps its own
                    // print-styled drill_render::Theme regardless of this
                    // choice (see app_theme.rs's module doc comment).
                    ui.menu_button(super::i18n::registered(self.locale, "app-ui.162"), |ui| {
                        for candidate in app_theme::AppTheme::ALL {
                            if ui
                                .radio_value(
                                    &mut self.app_theme,
                                    candidate,
                                    candidate.label(self.locale),
                                )
                                .changed()
                            {
                                self.app_theme.apply(ui.ctx());
                                self.app_theme.persist();
                                ui.close();
                            }
                        }
                    });
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
        }

        if self.focus_field {
            // Keep one small, predictable escape hatch rather than making a
            // canvas-first workspace feel modal. Playback stays here because
            // checking motion is still part of hands-on field work.
            egui::Frame::new()
                .fill(ui.visuals().faint_bg_color)
                .inner_margin(6)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .button(super::i18n::registered(self.locale, "focus-field.004"))
                            .on_hover_text(super::i18n::registered(self.locale, "focus-field.005"))
                            .clicked()
                        {
                            self.execute_command(UiCommand::ToggleFocusField, ui.ctx());
                        }
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
                        ui.small(format!(
                            "{} · {:.0}",
                            self.document
                                .sets
                                .get(self.current_set)
                                .map(|set| set.name.as_str())
                                .unwrap_or("—"),
                            self.count_position
                        ));
                    });
                });
        } else {
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
                // Keep the identity of the open document in the same place as
                // the app identity.  This follows the macOS convention of a
                // quiet, persistent title rather than making the writer hunt
                // for a transient save notification.
                ui.separator();
                let document_name = self
                    .current_path
                    .as_deref()
                    .and_then(|path| path.file_name())
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| {
                        super::i18n::registered(self.locale, "document-feedback.001").into()
                    });
                let identity = ui.label(
                    egui::RichText::new(document_name)
                        .strong()
                        .color(Color32::from_rgb(232, 237, 245)),
                );
                if let Some(path) = self.current_path.as_deref() {
                    identity.on_hover_text(format!(
                        "{}: {}",
                        super::i18n::registered(self.locale, "document-feedback.005"),
                        path.display()
                    ));
                }
                let (document_state, document_color) = if self.project_state.is_saving() {
                    ("document-feedback.002", Color32::from_rgb(110, 183, 255))
                } else if self.dirty {
                    ("document-feedback.004", Color32::from_rgb(255, 190, 82))
                } else {
                    ("document-feedback.003", Color32::from_rgb(112, 210, 150))
                };
                ui.colored_label(
                    document_color,
                    super::i18n::registered(self.locale, document_state),
                );
                if ui
                    .button(text(self.locale, Text::Open))
                    .on_hover_text(super::i18n::registered(self.locale, "app-ui.025"))
                    .clicked()
                {
                    self.request_open_document(DocumentOpenKind::LegacyJson);
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
                    .button(super::i18n::registered(self.locale, "app-ui.075"))
                    .on_hover_text(super::i18n::registered(self.locale, "app-ui.076"))
                    .clicked()
                {
                    self.show_inspector = !self.show_inspector;
                }
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
                    self.seek_to_count(self.playback_start);
                    self.playing = false;
                }
                if ui
                    .add_enabled(self.history.can_undo(), egui::Button::new("← Undo"))
                    .on_hover_text(super::i18n::registered(self.locale, "app-ui.160"))
                    .clicked()
                {
                    self.execute_command(UiCommand::Undo, ui.ctx());
                }
                if ui
                    .add_enabled(self.history.can_redo(), egui::Button::new("→ Redo"))
                    .on_hover_text(super::i18n::registered(self.locale, "app-ui.161"))
                    .clicked()
                {
                    self.execute_command(UiCommand::Redo, ui.ctx());
                }
                ui.add(
                    egui::Slider::new(&mut self.speed, 0.25..=4.0)
                        .text(text(self.locale, Text::Speed)),
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
        }
        if !self.selected.is_empty() {
            let selected_count = self.selected.len();
            egui::Frame::new()
                .fill(ui.visuals().faint_bg_color)
                .inner_margin(8)
                .corner_radius(6)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "{}: {selected_count}",
                                super::i18n::registered(self.locale, "app-ui.077")
                            ))
                            .strong(),
                        );
                        ui.small(super::i18n::registered(self.locale, "app-ui.119"))
                            .on_hover_text(super::i18n::registered(self.locale, "app-ui.120"));
                        if ui
                            .button(super::i18n::registered(self.locale, "app-ui.078"))
                            .clicked()
                        {
                            let points = self.selected_points();
                            self.commit_layout(editing::align_horizontal(&points));
                        }
                        if ui
                            .button(super::i18n::registered(self.locale, "app-ui.079"))
                            .clicked()
                        {
                            let points = self.selected_points();
                            self.commit_layout(editing::distribute_horizontal(&points));
                        }
                        ui.menu_button(super::i18n::registered(self.locale, "app-ui.085"), |ui| {
                            if ui
                                .button(super::i18n::registered(self.locale, "app-ui.086"))
                                .clicked()
                            {
                                let points = self.selected_points();
                                self.commit_layout(editing::align_vertical(&points));
                                ui.close();
                            }
                            if ui
                                .button(super::i18n::registered(self.locale, "app-ui.087"))
                                .clicked()
                            {
                                let points = self.selected_points();
                                self.commit_layout(editing::distribute_vertical(&points));
                                ui.close();
                            }
                            ui.separator();
                            if ui
                                .button(super::i18n::registered(self.locale, "app-ui.088"))
                                .clicked()
                            {
                                let points = self.selected_points();
                                self.commit_layout(editing::flip_horizontal(&points));
                                ui.close();
                            }
                            if ui
                                .button(super::i18n::registered(self.locale, "app-ui.089"))
                                .clicked()
                            {
                                let points = self.selected_points();
                                self.commit_layout(editing::flip_vertical(&points));
                                ui.close();
                            }
                        });
                        ui.separator();
                        if ui
                            .button(super::i18n::registered(self.locale, "clipboard.018"))
                            .clicked()
                        {
                            self.copy_selected_formation();
                        }
                        if ui
                            .button(super::i18n::registered(self.locale, "clipboard.019"))
                            .clicked()
                        {
                            self.begin_clipboard_paste_preview();
                        }
                        ui.separator();
                        if ui
                            .button(super::i18n::registered(self.locale, "app-ui.156"))
                            .on_hover_text(super::i18n::registered(self.locale, "app-ui.157"))
                            .clicked()
                        {
                            self.lock_selected_performers();
                            self.show_inspector = true;
                            self.workspace_focus = Some(WorkspaceFocus::Performer);
                        }
                        if ui
                            .button(super::i18n::registered(self.locale, "app-ui.158"))
                            .on_hover_text(super::i18n::registered(self.locale, "app-ui.159"))
                            .clicked()
                        {
                            self.hide_selected_performers();
                            self.show_inspector = true;
                            self.workspace_focus = Some(WorkspaceFocus::Performer);
                        }
                        if ui
                            .button(super::i18n::registered(self.locale, "app-ui.080"))
                            .clicked()
                            && let Some((min, max)) = self.selection_bounds()
                        {
                            let y = (min.y + max.y) * 0.5;
                            self.preview_shape(shapes::ShapeSpec::Line {
                                start: Point { x: min.x, y },
                                end: Point { x: max.x, y },
                            });
                        }
                        if self.formation_preview_spec.is_some() {
                            ui.separator();
                            if ui
                                .button(super::i18n::registered(self.locale, "app-ui.082"))
                                .clicked()
                            {
                                self.apply_shape_preview();
                            }
                            if ui
                                .button(super::i18n::registered(self.locale, "app-ui.083"))
                                .on_hover_text(super::i18n::registered(self.locale, "app-ui.084"))
                                .clicked()
                            {
                                self.cancel_shape_preview();
                            }
                        }
                        if ui
                            .button(super::i18n::registered(self.locale, "app-ui.081"))
                            .clicked()
                        {
                            self.show_inspector = true;
                            self.workspace_focus = Some(WorkspaceFocus::Performer);
                        }
                    });
                });
        }
        if self.show_guidance {
            egui::Frame::new()
                .fill(ui.visuals().faint_bg_color)
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
                .fill(ui.visuals().faint_bg_color)
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
                .fill(ui.visuals().faint_bg_color)
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
                .fill(ui.visuals().faint_bg_color)
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
            .fill(ui.visuals().faint_bg_color)
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
                        .on_hover_text(super::i18n::registered(self.locale, "app-ui.111"))
                        .clicked()
                    {
                        self.playback_start =
                            current_global.min(self.playback_end.saturating_sub(1));
                    }
                    if ui
                        .button(super::i18n::registered(self.locale, "app-ui.053"))
                        .on_hover_text(super::i18n::registered(self.locale, "app-ui.112"))
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
                });
                let range = super::playback_range_summary(
                    &self.document,
                    self.playback_start,
                    self.playback_end,
                );
                let count_label = super::i18n::registered(self.locale, "app-ui.114");
                ui.horizontal_wrapped(|ui| {
                    ui.small(format!(
                        "{}: {} · {} {}",
                        super::i18n::registered(self.locale, "app-ui.113"),
                        range.start_set,
                        count_label,
                        range.start_count,
                    ));
                    ui.separator();
                    ui.small(format!(
                        "{}: {} · {} {}",
                        super::i18n::registered(self.locale, "app-ui.115"),
                        range.end_set,
                        count_label,
                        range.end_count,
                    ));
                    ui.separator();
                    ui.small(format!(
                        "{}: {} {}",
                        super::i18n::registered(self.locale, "app-ui.116"),
                        range.length,
                        super::i18n::registered(self.locale, "app-ui.117"),
                    ));
                });
                ui.small(super::i18n::registered(self.locale, "app-ui.118"));
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
        if let Some(edit) = timeline_change.marker_edit {
            self.execute_edit(edit, super::i18n::registered(self.locale, "app-ui.109"));
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
            // Performer identity is stable across sets, so seeking the count
            // track keeps the current working group intact.
        }
        // A quiet, persistent desktop status strip. It is rendered before the
        // canvas claims the remaining space, so it is available even when the
        // inspector is hidden or the field fills the window.
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            let saved = if self.project_state.is_saving() {
                "document-feedback.002"
            } else if self.dirty {
                "document-feedback.004"
            } else {
                "document-feedback.003"
            };
            let color = if self.project_state.is_saving() {
                Color32::from_rgb(110, 183, 255)
            } else if self.dirty {
                Color32::from_rgb(255, 190, 82)
            } else {
                Color32::from_rgb(112, 210, 150)
            };
            ui.colored_label(color, super::i18n::registered(self.locale, saved));
            ui.separator();
            let set_name = self
                .document
                .sets
                .get(self.current_set)
                .map(|set| set.name.as_str())
                .unwrap_or("—");
            ui.label(format!(
                "{}: {}",
                super::i18n::registered(self.locale, "app-ui.105"),
                set_name
            ));
            ui.label(format!(
                "{} {:.2}",
                super::i18n::registered(self.locale, "app-ui.106"),
                self.count_position
            ));
            ui.label(format!(
                "{} {}",
                self.selected.len(),
                super::i18n::registered(self.locale, "app-ui.107")
            ));
            if !self.locked_performers.is_empty() || !self.hidden_performers.is_empty() {
                let filter_label = format!(
                    "{}: {} {} · {} {}",
                    super::i18n::registered(self.locale, "app-ui.147"),
                    self.locked_performers.len(),
                    super::i18n::registered(self.locale, "app-ui.148"),
                    self.hidden_performers.len(),
                    super::i18n::registered(self.locale, "app-ui.149"),
                );
                if ui
                    .small_button(filter_label)
                    .on_hover_text(super::i18n::registered(self.locale, "app-ui.150"))
                    .clicked()
                {
                    self.show_inspector = true;
                    self.workspace_focus = Some(WorkspaceFocus::Performer);
                }
                if !self.last_filtered_performers.is_empty()
                    && ui
                        .small_button(super::i18n::registered(self.locale, "app-ui.153"))
                        .on_hover_text(super::i18n::registered(self.locale, "app-ui.154"))
                        .clicked()
                {
                    let restored = self.restore_last_filtered_performers();
                    self.status = format!(
                        "{} {}",
                        restored,
                        super::i18n::registered(self.locale, "app-ui.155")
                    );
                }
            }
            ui.separator();
            ui.small(format!(
                "{}: {}",
                super::i18n::registered(self.locale, "app-ui.108"),
                self.status
            ));
        });
        ui.horizontal_top(|ui| {
            if self.show_inspector && !self.focus_field {
                self.show_workspace_inspector(ui, set_counts);
            }
            let available = ui.available_size();
            let (response, painter) = ui.allocate_painter(available, Sense::click_and_drag());
            if !self.is_editable_set_start() {
                let current_set_name = self
                    .document
                    .sets
                    .get(self.current_set)
                    .map(|set| set.name.clone())
                    .unwrap_or_else(|| "—".to_owned());
                egui::Area::new("return-to-set-start".into())
                    .order(egui::Order::Foreground)
                    .fixed_pos(response.rect.left_top() + Vec2::new(22.0, 22.0))
                    .show(ui.ctx(), |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(super::i18n::registered(
                                    self.locale,
                                    "app-ui.121",
                                ))
                                .strong(),
                            );
                            ui.small(format!("{} · {:.2}", current_set_name, self.count_position));
                            if ui
                                .button(super::i18n::registered(self.locale, "app-ui.122"))
                                .on_hover_text(super::i18n::registered(self.locale, "app-ui.123"))
                                .clicked()
                            {
                                self.return_to_editable_set_start();
                            }
                        });
                    });
            }
            // The field is a first-class desktop editing surface: clicking it
            // gives arrow keys to the canvas, while text fields and palettes
            // retain their native keyboard behavior.
            if response.clicked() {
                response.request_focus();
            }
            let canvas_owns_keyboard = response.has_focus()
                && !ui.ctx().egui_wants_keyboard_input()
                && !self.command_palette.is_open();
            if canvas_owns_keyboard {
                let reset_view =
                    ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F));
                let focus_selection = ui.input_mut(|input| {
                    input.consume_key(egui::Modifiers::COMMAND, egui::Key::Num1)
                        || input.consume_key(egui::Modifiers::CTRL, egui::Key::Num1)
                });
                if reset_view && self.view_mode == ViewMode::Field2D {
                    self.field_viewport.reset(&self.document.grid);
                }
                if focus_selection
                    && self.view_mode == ViewMode::Field2D
                    && !self.selected.is_empty()
                {
                    let (sum_x, sum_y, count) = self.selected.iter().fold(
                        (0.0_f32, 0.0_f32, 0_u32),
                        |(x, y, count), &index| {
                            let point = self.frame_positions[index];
                            (x + point.x, y + point.y, count + 1)
                        },
                    );
                    if count > 0 {
                        self.field_viewport.center = Point {
                            x: sum_x / count as f32,
                            y: sum_y / count as f32,
                        };
                    }
                }
                let nudge = ui.input_mut(|input| {
                    let modifiers = input.modifiers;
                    if modifiers.command || modifiers.alt || modifiers.ctrl {
                        return None;
                    }
                    let scale = if modifiers.shift { 4 } else { 1 };
                    let expected = if modifiers.shift {
                        egui::Modifiers::SHIFT
                    } else {
                        egui::Modifiers::NONE
                    };
                    if input.consume_key(expected, egui::Key::ArrowUp) {
                        Some((0, scale))
                    } else if input.consume_key(expected, egui::Key::ArrowDown) {
                        Some((0, -scale))
                    } else if input.consume_key(expected, egui::Key::ArrowLeft) {
                        Some((-scale, 0))
                    } else if input.consume_key(expected, egui::Key::ArrowRight) {
                        Some((scale, 0))
                    } else {
                        None
                    }
                });
                if let Some((x, y)) = nudge {
                    self.nudge_selected(x, y);
                }
            }
            let rect = response.rect.shrink(18.0);
            if self.view_mode == ViewMode::Stadium3D {
                self.draw_stadium(ui, &response, &painter, rect);
            } else {
                // Navigation is intentionally processed before editing. This
                // makes middle-drag / Space-drag a true canvas pan rather
                // than a selection gesture, while leaving document/history
                // untouched.
                let viewport_size = rect.size();
                self.field_viewport
                    .clamp_center(&self.document.grid, viewport_size);
                let space_held = ui.input(|input| input.key_down(egui::Key::Space));
                let begin_pan = response.drag_started_by(egui::PointerButton::Middle)
                    || (space_held && response.drag_started_by(egui::PointerButton::Primary));
                if begin_pan {
                    self.field_viewport.pan_last_pointer = response.interact_pointer_pos();
                }
                let was_panning = self.field_viewport.pan_last_pointer.is_some();
                if let (Some(previous), Some(pointer)) = (
                    self.field_viewport.pan_last_pointer,
                    response.interact_pointer_pos(),
                ) {
                    let middle_down = ui.input(|input| input.pointer.middle_down());
                    if middle_down || space_held {
                        self.field_viewport.pan_pixels(
                            pointer - previous,
                            &self.document.grid,
                            viewport_size,
                        );
                        self.field_viewport.pan_last_pointer = Some(pointer);
                    } else {
                        self.field_viewport.pan_last_pointer = None;
                    }
                }
                if response.drag_stopped() {
                    self.field_viewport.pan_last_pointer = None;
                }
                if response.hovered() {
                    let (scroll, modifiers, pointer) = ui.input(|input| {
                        (
                            input.smooth_scroll_delta.y,
                            input.modifiers,
                            input.pointer.hover_pos(),
                        )
                    });
                    if scroll != 0.0
                        && (modifiers.command || modifiers.ctrl)
                        && let Some(pointer) = pointer
                    {
                        self.field_viewport.zoom_at(
                            (scroll * 0.0025).exp(),
                            pointer,
                            rect,
                            &self.document.grid,
                        );
                    }
                }
                let panning = was_panning || self.field_viewport.pan_last_pointer.is_some();
                egui::Area::new("field-navigation-controls".into())
                    .order(egui::Order::Foreground)
                    .fixed_pos(rect.left_top() + Vec2::new(12.0, 10.0))
                    .show(ui.ctx(), |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                if ui
                                    .small_button(super::i18n::registered(
                                        self.locale,
                                        "app-ui.124",
                                    ))
                                    .on_hover_text(super::i18n::registered(
                                        self.locale,
                                        "app-ui.125",
                                    ))
                                    .clicked()
                                {
                                    self.field_viewport.reset(&self.document.grid);
                                }
                                if ui
                                    .add_enabled(
                                        !self.selected.is_empty(),
                                        egui::Button::new(super::i18n::registered(
                                            self.locale,
                                            "app-ui.126",
                                        ))
                                        .small(),
                                    )
                                    .on_hover_text(super::i18n::registered(
                                        self.locale,
                                        "app-ui.127",
                                    ))
                                    .clicked()
                                {
                                    let (x, y, count) = self.selected.iter().fold(
                                        (0.0, 0.0, 0_u32),
                                        |(x, y, n), &index| {
                                            let point = self.frame_positions[index];
                                            (x + point.x, y + point.y, n + 1)
                                        },
                                    );
                                    if count > 0 {
                                        self.field_viewport.center = Point {
                                            x: x / count as f32,
                                            y: y / count as f32,
                                        };
                                    }
                                }
                            });
                            ui.small(super::i18n::registered(self.locale, "app-ui.128"));
                        });
                    });
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
                drill_render::build_field_2d(
                    &scene,
                    &mut self.render_scratch,
                    &mut self.display_list,
                );
                // Must stay in lockstep with the mapping `build_field_2d` used to
                // place the dots this frame (same grid, viewport, and margin) —
                // otherwise the heatmap overlay, trails, selection ring, and
                // hit-testing all drift away from where the performer is
                // actually drawn whenever the viewport's aspect ratio doesn't
                // match the field's. See FieldMap's docs.
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
                // Show DNA heatmap overlay is revision-gated and built on the
                // analytics worker, only while its toggle is on.
                if self.heatmap_enabled {
                    let revision = self.history.revision();
                    let analytics_key = super::analytics_state::AnalyticsKey {
                        revision,
                        set_index: self.current_set,
                        beats_per_measure: self.beats_per_measure,
                        heatmap: true,
                    };
                    self.analytics_state.poll(analytics_key);
                    self.analytics_state.ensure(&self.document, analytics_key);
                    if let Some(occupancy) = self.analytics_state.heatmap(analytics_key) {
                        drill_render::append_heatmap(occupancy, &field_map, &mut self.display_list);
                    }
                }
                // Movement trails for the transition leaving the current set,
                // scoped by the Analytics panel's "Show for" selector. Cheap
                // enough (warm thread-local scratch in `drill_render`) to
                // resample every visible frame rather than cache.
                if self.trail_selection != drill_render::TrailSelection::None
                    && let Some(set) = self.document.sets.get(self.current_set)
                {
                    let trail_performer_ids: Vec<PerformerId> = match self.trail_selection {
                        drill_render::TrailSelection::All => {
                            self.document.performers.iter().map(|p| p.id).collect()
                        }
                        drill_render::TrailSelection::Selected => self
                            .selected
                            .iter()
                            .filter_map(|&index| self.document.performers.get(index).map(|p| p.id))
                            .collect(),
                        drill_render::TrailSelection::None => Vec::new(),
                    };
                    if !trail_performer_ids.is_empty() {
                        drill_render::append_trails(
                            &self.document,
                            set.id,
                            &trail_performer_ids,
                            &field_map,
                            24,
                            &mut self.display_list,
                        );
                    }
                }
                if let Some(gpu) = self.gpu.as_ref().filter(|gpu| gpu.active()) {
                    gpu.update(&self.display_list);
                    egui_backend::paint_gpu_background(&painter, rect.min, &self.display_list);
                    painter.add(gpu.callback(rect));
                    egui_backend::paint_gpu_foreground(&painter, rect.min, &self.display_list);
                } else {
                    egui_backend::paint(&painter, rect.min, &self.display_list);
                }
                let field_rect = Rect::from_min_size(
                    Pos2::new(
                        rect.left() + field_map.origin.x,
                        rect.top() + field_map.origin.y,
                    ),
                    Vec2::new(
                        self.document.grid.width * field_map.scale,
                        self.document.grid.height * field_map.scale,
                    ),
                );
                let comparison_to_screen = |point: Point| {
                    let v = field_map.map(point);
                    Pos2::new(rect.left() + v.x, rect.top() + v.y)
                };
                self.underlay_state.paint(
                    &painter,
                    field_rect,
                    self.document
                        .underlay
                        .as_ref()
                        .map(|value| &value.placement),
                );
                // A/B comparison is a deliberately session-only visual aid:
                // amber dots are the selected reference set, while cyan lines
                // make the displacement from that form immediately legible.
                // It is painted in the same FieldMap as the live frame so it
                // remains perfectly registered through pan and zoom.
                if let Some(comparison) = self.set_comparison
                    && let Some(reference) = self.document.sets.get(comparison.reference_set)
                {
                    for (index, &point) in reference.positions.iter().enumerate() {
                        let reference_pos = comparison_to_screen(point);
                        if comparison.show_paths
                            && let Some(&current) = self.frame_positions.get(index)
                        {
                            let current_pos = comparison_to_screen(current);
                            if reference_pos.distance(current_pos) > 1.5 {
                                painter.line_segment(
                                    [reference_pos, current_pos],
                                    Stroke::new(
                                        1.25,
                                        Color32::from_rgba_unmultiplied(80, 220, 255, 150),
                                    ),
                                );
                            }
                        }
                        painter.circle_stroke(
                            reference_pos,
                            7.0,
                            Stroke::new(1.5, Color32::from_rgba_unmultiplied(255, 190, 75, 220)),
                        );
                    }
                }
                egui::Area::new("set-comparison-controls".into())
                    .order(egui::Order::Foreground)
                    .fixed_pos(rect.right_top() + Vec2::new(-286.0, 10.0))
                    .show(ui.ctx(), |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            if let Some(mut comparison) = self.set_comparison {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(super::i18n::registered(
                                            self.locale,
                                            "comparison.001",
                                        ))
                                        .strong(),
                                    );
                                    if ui
                                        .small_button(super::i18n::registered(
                                            self.locale,
                                            "comparison.002",
                                        ))
                                        .clicked()
                                    {
                                        self.set_comparison = None;
                                        self.status =
                                            super::i18n::registered(self.locale, "comparison.003")
                                                .into();
                                    }
                                });
                                let reference_name = self
                                    .document
                                    .sets
                                    .get(comparison.reference_set)
                                    .map(|set| {
                                        format!("{} · {}", comparison.reference_set + 1, set.name)
                                    })
                                    .unwrap_or_default();
                                egui::ComboBox::from_id_salt("comparison-reference-set")
                                    .selected_text(reference_name)
                                    .show_ui(ui, |ui| {
                                        for (index, set) in self.document.sets.iter().enumerate() {
                                            if index != self.current_set {
                                                ui.selectable_value(
                                                    &mut comparison.reference_set,
                                                    index,
                                                    format!("{} · {}", index + 1, set.name),
                                                );
                                            }
                                        }
                                    });
                                ui.checkbox(
                                    &mut comparison.show_paths,
                                    super::i18n::registered(self.locale, "comparison.004"),
                                );
                                ui.small(super::i18n::registered(self.locale, "comparison.005"));
                                self.set_comparison = Some(comparison);
                            } else if self.document.sets.len() > 1
                                && ui
                                    .button(super::i18n::registered(self.locale, "comparison.006"))
                                    .on_hover_text(super::i18n::registered(
                                        self.locale,
                                        "comparison.007",
                                    ))
                                    .clicked()
                            {
                                let reference_set = self.current_set.saturating_sub(1);
                                self.set_comparison = Some(super::SetComparison {
                                    reference_set: if reference_set == self.current_set {
                                        1
                                    } else {
                                        reference_set
                                    },
                                    show_paths: true,
                                });
                                self.status =
                                    super::i18n::registered(self.locale, "comparison.008").into();
                            }
                        });
                    });
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
                if !self.formation_preview_points.is_empty() || self.free_draw_active {
                    for pair in self.formation_preview_points.windows(2) {
                        painter.line_segment(
                            [to_screen(pair[0]), to_screen(pair[1])],
                            Stroke::new(2.0, Color32::from_rgb(80, 220, 255)),
                        );
                    }
                    if self.formation_preview_is_closed()
                        && let (Some(&first), Some(&last)) = (
                            self.formation_preview_points.first(),
                            self.formation_preview_points.last(),
                        )
                    {
                        painter.line_segment(
                            [to_screen(last), to_screen(first)],
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
                    // The inspector can be intentionally hidden while someone
                    // works on the field. Keep the pending edit actionable at
                    // its point of effect instead of leaving unexplained cyan
                    // geometry on the canvas.
                    let preview_count = self.selected.len();
                    egui::Area::new("field-preview-actions".into())
                        .order(egui::Order::Foreground)
                        .fixed_pos(rect.left_top() + Vec2::new(18.0, 130.0))
                        .show(ui.ctx(), |ui| {
                            egui::Frame::popup(ui.style()).show(ui, |ui| {
                                let ready = self.formation_preview_spec.is_some();
                                ui.label(
                                    egui::RichText::new(super::i18n::registered(
                                        self.locale,
                                        if ready { "app-ui.090" } else { "app-ui.095" },
                                    ))
                                    .strong(),
                                );
                                if ready {
                                    ui.small(format!(
                                        "{} {}",
                                        preview_count,
                                        super::i18n::registered(self.locale, "app-ui.091")
                                    ));
                                }
                                ui.horizontal(|ui| {
                                    if ui
                                        .add_enabled(
                                            ready,
                                            egui::Button::new(super::i18n::registered(
                                                self.locale,
                                                "app-ui.093",
                                            )),
                                        )
                                        .clicked()
                                    {
                                        self.apply_shape_preview();
                                    }
                                    if ui
                                        .button(super::i18n::registered(self.locale, "app-ui.094"))
                                        .on_hover_text(super::i18n::registered(
                                            self.locale,
                                            "app-ui.084",
                                        ))
                                        .clicked()
                                    {
                                        self.cancel_shape_preview();
                                    }
                                });
                                ui.small(super::i18n::registered(
                                    self.locale,
                                    if ready { "app-ui.092" } else { "app-ui.096" },
                                ));
                            });
                        });
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
                if let Some(preview) = &self.clipboard_paste_preview {
                    for &(id, point) in preview {
                        let pos = to_screen(point);
                        painter.circle_filled(
                            pos,
                            10.0,
                            Color32::from_rgba_unmultiplied(76, 203, 255, 70),
                        );
                        painter.circle_stroke(
                            pos,
                            8.0,
                            Stroke::new(2.0, Color32::from_rgb(76, 203, 255)),
                        );
                        painter.text(
                            pos + Vec2::new(11.0, -11.0),
                            egui::Align2::LEFT_BOTTOM,
                            "PASTE",
                            egui::FontId::proportional(10.0),
                            Color32::from_rgb(160, 235, 255),
                        );
                        let _ = id;
                    }
                    let preview_count = preview.len();
                    egui::Area::new("clipboard-paste-preview-actions".into())
                        .order(egui::Order::Foreground)
                        .fixed_pos(rect.left_top() + Vec2::new(18.0, 130.0))
                        .show(ui.ctx(), |ui| {
                            egui::Frame::popup(ui.style()).show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(super::i18n::registered(
                                        self.locale,
                                        "clipboard.013",
                                    ))
                                    .strong(),
                                );
                                ui.small(format!(
                                    "{preview_count} {}",
                                    super::i18n::registered(
                                        self.locale,
                                        if self.clipboard_paste_targets_selection {
                                            "clipboard.023"
                                        } else {
                                            "clipboard.014"
                                        },
                                    )
                                ));
                                ui.horizontal(|ui| {
                                    if ui
                                        .button(super::i18n::registered(
                                            self.locale,
                                            "clipboard.015",
                                        ))
                                        .clicked()
                                    {
                                        self.apply_clipboard_paste_preview();
                                    }
                                    if ui
                                        .button(super::i18n::registered(
                                            self.locale,
                                            "clipboard.016",
                                        ))
                                        .clicked()
                                    {
                                        self.cancel_clipboard_paste_preview();
                                    }
                                });
                                ui.small(super::i18n::registered(
                                    self.locale,
                                    if self.clipboard_paste_targets_selection {
                                        "clipboard.024"
                                    } else {
                                        "clipboard.017"
                                    },
                                ));
                            });
                        });
                }
                for (index, &point) in self.frame_positions.iter().enumerate() {
                    let pos = to_screen(point);
                    let selected = self.selected.contains(&index);
                    if self.is_hidden_index(index) {
                        // A translucent veil plus slash is intentionally not
                        // color-only: hidden dots remain findable and can be
                        // restored without touching the document.
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
                        painter.circle_stroke(pos, 11.0, Stroke::new(2.0, Color32::WHITE));
                        if self.selected.len() >= 2 {
                            self.paint_selection_rank_badge(&painter, pos, index);
                        }
                    }
                }
                if let Some(pointer) = response.interact_pointer_pos() {
                    // A preview was sampled for the current selection. Lock
                    // selection and direct-manipulation until Apply or
                    // Discard so the visible proposal cannot silently target
                    // a different group of performers.
                    let interaction_locked = self.formation_preview_spec.is_some()
                        || self.clipboard_paste_preview.is_some();
                    if !panning
                        && self.free_draw_active
                        && (response.drag_started() || response.dragged())
                    {
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
                    if !panning && self.free_draw_active && response.drag_stopped() {
                        self.finish_free_draw_preview();
                    }
                    let nearest = || {
                        self.frame_positions
                            .iter()
                            .enumerate()
                            .filter(|(index, _)| self.is_selectable_index(*index))
                            .min_by(|(_, a), (_, b)| {
                                to_screen(**a)
                                    .distance(pointer)
                                    .total_cmp(&to_screen(**b).distance(pointer))
                            })
                            .filter(|(_, p)| to_screen(**p).distance(pointer) < 18.0)
                            .map(|(i, _)| i)
                    };
                    let nearest_index = nearest();
                    if !panning
                        && !self.free_draw_active
                        && !interaction_locked
                        && response.clicked()
                    {
                        if let Some(index) = nearest_index {
                            // Command/Ctrl follows the native desktop
                            // convention; Shift mirrors the established drill
                            // design workflow, so users can extend a group
                            // without changing tools.
                            let additive = ui.input(|input| {
                                input.modifiers.command
                                    || input.modifiers.ctrl
                                    || input.modifiers.shift
                            });
                            if additive {
                                let mut next = self.selected.clone();
                                if !next.insert(index) {
                                    next.remove(&index);
                                }
                                self.replace_selection(next);
                            } else {
                                self.replace_selection([index].into_iter().collect());
                            }
                        } else {
                            self.clear_selection();
                        }
                    }
                    if !self.free_draw_active
                        && !interaction_locked
                        && response.drag_started()
                        && !panning
                        && self.is_editable_set_start()
                        && let Some(index) = nearest_index
                    {
                        if !self.selected.contains(&index) {
                            self.replace_selection([index].into_iter().collect());
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
                    if !self.free_draw_active
                        && !interaction_locked
                        && response.drag_started()
                        && !panning
                        && nearest_index.is_none()
                    {
                        self.marquee_origin = Some(pointer);
                    }
                    if !self.free_draw_active
                        && !interaction_locked
                        && response.dragged()
                        && !panning
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
                        && !interaction_locked
                        && response.dragged()
                        && !panning
                        && self.is_editable_set_start()
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
                        && !interaction_locked
                        && response.drag_stopped()
                        && !panning
                        && let Some(before) = self.drag_before.take()
                    {
                        let after = self.drag_preview.take().unwrap_or_else(|| before.clone());
                        if before != after && self.ensure_editable_set_start() {
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
                        && !interaction_locked
                        && response.drag_stopped()
                        && !panning
                        && let Some(origin) = self.marquee_origin.take()
                    {
                        let marquee = Rect::from_two_pos(origin, pointer);
                        let additive = ui.input(|input| {
                            input.modifiers.command || input.modifiers.ctrl || input.modifiers.shift
                        });
                        let mut next = if additive {
                            self.selected.clone()
                        } else {
                            BTreeSet::new()
                        };
                        for (index, &point) in self.frame_positions.iter().enumerate() {
                            if self.is_selectable_index(index) && marquee.contains(to_screen(point))
                            {
                                next.insert(index);
                            }
                        }
                        self.replace_selection(next);
                    }
                }
                // Keep frequent selection operations at the field as well as
                // in the toolbar and inspector. This is intentionally a
                // short contextual menu: it accelerates repetition without
                // becoming a second, hidden control surface.
                if !self.selected.is_empty()
                    && self.formation_preview_spec.is_none()
                    && !self.free_draw_active
                {
                    response.context_menu(|ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "{} {}",
                                self.selected.len(),
                                super::i18n::registered(self.locale, "app-ui.097")
                            ))
                            .strong(),
                        );
                        ui.separator();
                        let points = self.selected_points();
                        if ui
                            .button(super::i18n::registered(self.locale, "app-ui.098"))
                            .clicked()
                        {
                            self.commit_layout(editing::align_horizontal(&points));
                            ui.close();
                        }
                        if ui
                            .button(super::i18n::registered(self.locale, "app-ui.099"))
                            .clicked()
                        {
                            self.commit_layout(editing::align_vertical(&points));
                            ui.close();
                        }
                        if self.selected.len() >= 2 {
                            if ui
                                .button(super::i18n::registered(self.locale, "app-ui.100"))
                                .clicked()
                            {
                                self.commit_layout(editing::distribute_horizontal(&points));
                                ui.close();
                            }
                            if ui
                                .button(super::i18n::registered(self.locale, "app-ui.101"))
                                .clicked()
                            {
                                self.commit_layout(editing::distribute_vertical(&points));
                                ui.close();
                            }
                        }
                        if ui
                            .button(super::i18n::registered(self.locale, "app-ui.102"))
                            .clicked()
                        {
                            if let Some((min, max)) = self.selection_bounds() {
                                let y = (min.y + max.y) * 0.5;
                                self.preview_shape(shapes::ShapeSpec::Line {
                                    start: Point { x: min.x, y },
                                    end: Point { x: max.x, y },
                                });
                            }
                            ui.close();
                        }
                        ui.separator();
                        if ui
                            .button(super::i18n::registered(self.locale, "clipboard.020"))
                            .clicked()
                        {
                            self.copy_selected_formation();
                            ui.close();
                        }
                        if ui
                            .button(super::i18n::registered(self.locale, "clipboard.021"))
                            .clicked()
                        {
                            self.begin_clipboard_paste_preview();
                            ui.close();
                        }
                        ui.separator();
                        if ui
                            .button(super::i18n::registered(self.locale, "app-ui.129"))
                            .on_hover_text(super::i18n::registered(self.locale, "app-ui.130"))
                            .clicked()
                        {
                            self.lock_selected_performers();
                            ui.close();
                        }
                        if ui
                            .button(super::i18n::registered(self.locale, "app-ui.131"))
                            .on_hover_text(super::i18n::registered(self.locale, "app-ui.132"))
                            .clicked()
                        {
                            self.hide_selected_performers();
                            ui.close();
                        }
                    });
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
            Some(onboarding::WelcomeAction::OpenJson) => {
                self.request_open_document(DocumentOpenKind::LegacyJson)
            }
            Some(onboarding::WelcomeAction::OpenProject) => {
                self.request_open_document(DocumentOpenKind::Project)
            }
            None => {}
        }
        self.onboarding.persist_if_changed();
        if let Some(command) =
            self.command_palette
                .show(ui.ctx(), self.command_context(), self.locale)
        {
            self.execute_command(command, ui.ctx());
        }
        if let Some(set_index) =
            self.set_navigator
                .show(ui.ctx(), &self.document, self.current_set, self.locale)
        {
            self.navigate_to_set(set_index);
        }
        if let Some(count) =
            self.go_to_count
                .show(ui.ctx(), self.document.timeline_counts(), self.locale)
        {
            self.navigate_to_global_count(count);
        }
        self.show_close_guard(ui.ctx());
        self.show_document_open_guard(ui.ctx());
        self.show_recent_projects(ui.ctx());
    }
}

impl DrillApp {
    /// Draws the small numbered badge marking a selected performer's rank in
    /// the current click order (1 = first clicked). Callers gate on
    /// `self.selected.len() >= 2` first — a badge on a single selected dot
    /// is just noise. Positioned at the dot's bottom-right, a deliberately
    /// different corner from the locked-badge "L" at top-right
    /// (`pos + Vec2::new(7.0, -8.0)`), so a performer that is both locked
    /// and selected shows both badges without overlap. The radius grows
    /// with digit count so 10+ ranks aren't clipped.
    pub(crate) fn paint_selection_rank_badge(
        &self,
        painter: &egui::Painter,
        pos: Pos2,
        index: usize,
    ) {
        let Some(rank) = self.selected.iter().position(|&i| i == index).map(|p| p + 1) else {
            return;
        };
        let mut digits: u32 = 1;
        let mut remaining = rank;
        while remaining >= 10 {
            remaining /= 10;
            digits += 1;
        }
        let radius = 7.5 + 2.3 * (digits - 1) as f32;
        let center = pos + Vec2::new(8.0, 9.0);
        painter.circle_filled(center, radius, Color32::from_rgb(245, 197, 66));
        painter.circle_stroke(center, radius, Stroke::new(1.0, Color32::from_black_alpha(200)));
        painter.text(
            center,
            egui::Align2::CENTER_CENTER,
            rank.to_string(),
            egui::FontId::proportional(10.0),
            Color32::BLACK,
        );
    }

    fn show_recent_projects(&mut self, ctx: &egui::Context) {
        if !self.show_recent_projects {
            return;
        }
        self.recent_projects.prune_missing();
        let mut open = true;
        let mut chosen = None;
        egui::Window::new(super::i18n::registered(self.locale, "recent-projects.010"))
            .id(egui::Id::new("recent-projects"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(560.0)
            .show(ctx, |ui| {
                ui.label(super::i18n::registered(self.locale, "recent-projects.004"));
                ui.add_space(8.0);
                let paths = self.recent_projects.paths().to_vec();
                if paths.is_empty() {
                    ui.label(super::i18n::registered(self.locale, "recent-projects.011"));
                }
                for path in paths {
                    ui.horizontal(|ui| {
                        let label = path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .map(str::to_owned)
                            .unwrap_or_else(|| path.to_string_lossy().into_owned());
                        if ui
                            .button(label)
                            .on_hover_text(path.display().to_string())
                            .clicked()
                        {
                            chosen = Some(path.clone());
                        }
                        if ui
                            .small_button("×")
                            .on_hover_text(super::i18n::registered(
                                self.locale,
                                "recent-projects.006",
                            ))
                            .clicked()
                        {
                            self.recent_projects.remove(&path);
                        }
                    });
                }
                if !self.recent_projects.paths().is_empty() {
                    ui.add_space(8.0);
                    if ui
                        .button(super::i18n::registered(self.locale, "recent-projects.012"))
                        .clicked()
                    {
                        self.recent_projects.clear();
                    }
                }
            });
        self.show_recent_projects = open;
        if let Some(path) = chosen {
            self.show_recent_projects = false;
            self.request_open_recent(path);
        }
    }

    /// Keep the native window alive while the user decides, including while a
    /// save job owns a snapshot of the document on its worker thread.
    fn guard_close_request(&mut self, ctx: &egui::Context) {
        if !ctx.input(|input| input.viewport().close_requested()) {
            return;
        }
        if self.dirty || self.close_guard != CloseGuard::Idle {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.close_guard == CloseGuard::Idle {
                self.close_guard = CloseGuard::Prompt;
            }
        }
    }

    fn show_close_guard(&mut self, ctx: &egui::Context) {
        if self.close_guard == CloseGuard::Idle {
            return;
        }

        if self.close_guard == CloseGuard::Prompt
            && ctx.input(|input| input.key_pressed(egui::Key::Escape))
        {
            self.close_guard = CloseGuard::Idle;
            self.status = super::i18n::registered(self.locale, "close-guard.008").into();
            return;
        }

        egui::Window::new(super::i18n::registered(self.locale, "close-guard.001"))
            .id(egui::Id::new("unsaved-close-guard"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.set_min_width(390.0);
                ui.label(super::i18n::registered(self.locale, "close-guard.002"));
                ui.add_space(10.0);
                if self.close_guard == CloseGuard::Saving {
                    ui.spinner();
                    ui.label(super::i18n::registered(self.locale, "close-guard.006"));
                    ctx.request_repaint_after(Duration::from_millis(50));
                    return;
                }
                ui.horizontal(|ui| {
                    if ui
                        .button(super::i18n::registered(self.locale, "close-guard.003"))
                        .clicked()
                    {
                        self.save_dialog();
                        // A file picker may have been cancelled. Only enter
                        // the waiting state after the save worker really owns
                        // a request; `ProjectEvent::Saved` is the sole path
                        // that eventually sends the final Close command.
                        if self.project_state.is_saving() {
                            self.close_guard = CloseGuard::Saving;
                        }
                    }
                    if ui
                        .button(
                            egui::RichText::new(super::i18n::registered(
                                self.locale,
                                "close-guard.004",
                            ))
                            .color(Color32::from_rgb(255, 170, 170)),
                        )
                        .clicked()
                    {
                        self.dirty = false;
                        self.close_guard = CloseGuard::Idle;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if ui
                        .button(super::i18n::registered(self.locale, "close-guard.005"))
                        .clicked()
                    {
                        self.close_guard = CloseGuard::Idle;
                        self.status =
                            super::i18n::registered(self.locale, "close-guard.009").into();
                    }
                });
            });
    }

    /// Opening is delayed until Save has completed, or the writer explicitly
    /// chooses to discard. The picker itself is not shown before that choice.
    fn show_document_open_guard(&mut self, ctx: &egui::Context) {
        let state = self.document_open_guard.clone();
        if state == DocumentOpenGuard::Idle {
            return;
        }
        if matches!(state, DocumentOpenGuard::Prompt(_))
            && ctx.input(|input| input.key_pressed(egui::Key::Escape))
        {
            self.document_open_guard = DocumentOpenGuard::Idle;
            self.status = super::i18n::registered(self.locale, "document-open-guard.009").into();
            return;
        }
        egui::Window::new(super::i18n::registered(
            self.locale,
            "document-open-guard.001",
        ))
        .id(egui::Id::new("unsaved-document-open-guard"))
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.set_min_width(410.0);
            ui.label(super::i18n::registered(
                self.locale,
                "document-open-guard.002",
            ));
            ui.add_space(10.0);
            let kind = match &state {
                DocumentOpenGuard::Prompt(kind) | DocumentOpenGuard::Saving(kind) => kind.clone(),
                DocumentOpenGuard::Idle => return,
            };
            if matches!(state, DocumentOpenGuard::Saving(_)) {
                ui.spinner();
                ui.label(super::i18n::registered(
                    self.locale,
                    "document-open-guard.006",
                ));
                ctx.request_repaint_after(Duration::from_millis(50));
                return;
            }
            ui.horizontal(|ui| {
                if ui
                    .button(super::i18n::registered(
                        self.locale,
                        "document-open-guard.003",
                    ))
                    .clicked()
                {
                    self.save_dialog();
                    if self.project_state.is_saving() {
                        self.document_open_guard = DocumentOpenGuard::Saving(kind.clone());
                    }
                }
                if ui
                    .button(
                        egui::RichText::new(super::i18n::registered(
                            self.locale,
                            "document-open-guard.004",
                        ))
                        .color(Color32::from_rgb(255, 170, 170)),
                    )
                    .clicked()
                {
                    self.document_open_guard = DocumentOpenGuard::Idle;
                    self.begin_open_target(kind);
                }
                if ui
                    .button(super::i18n::registered(
                        self.locale,
                        "document-open-guard.005",
                    ))
                    .clicked()
                {
                    self.document_open_guard = DocumentOpenGuard::Idle;
                    self.status =
                        super::i18n::registered(self.locale, "document-open-guard.008").into();
                }
            });
        });
    }
}
