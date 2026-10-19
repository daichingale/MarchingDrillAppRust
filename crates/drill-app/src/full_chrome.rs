//! Light overlay chrome for the full editor.
//!
//! The field is laid out edge to edge. Menus, tools, the selection bar, the
//! inspector, and the timeline float on top of it so they do not push the
//! ground into a column.

use super::simple_mode::{PhoneIcon, PictogramTile};
use super::*;

fn paper(margin: i8) -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::WHITE)
        .stroke(Stroke::new(1.0, super::app_theme::HAIRLINE))
        .corner_radius(super::app_theme::CORNER_SM)
        .inner_margin(egui::Margin::same(margin))
        .shadow(egui::Shadow::NONE)
}

impl DrillApp {
    pub(super) fn show_full_chrome(&mut self, ui: &mut egui::Ui, set_counts: f32) {
        self.show_full_top_bar(ui);
        self.show_full_tool_dock(ui);
        self.show_full_context_bar(ui);
        self.show_full_inspector(ui, set_counts);
        self.show_full_timeline(ui);
    }

    fn full_project_label(&self) -> String {
        if let Some(path) = &self.current_path
            && let Some(name) = path.file_stem().and_then(|name| name.to_str())
            && !name.is_empty()
        {
            return name.to_string();
        }
        let title = self.document.title.trim();
        if title.is_empty() {
            "DrillForge".to_string()
        } else {
            title.to_string()
        }
    }

    fn full_tile(
        &self,
        ui: &mut egui::Ui,
        icon: PhoneIcon,
        label: &str,
        tip: &str,
        enabled: bool,
        active: bool,
    ) -> bool {
        let (_, clicked) = self.simple_pictogram_tile(
            ui,
            PictogramTile {
                icon,
                label,
                tip,
                width: 62.0,
                enabled,
                active,
                pulse: false,
                filled: false,
                compact: false,
                time: ui.input(|input| input.time as f32),
            },
        );
        clicked && enabled
    }

    fn show_full_top_bar(&mut self, ui: &mut egui::Ui) {
        let screen = ui.ctx().viewport_rect();
        egui::Area::new("full-top-bar".into())
            .order(egui::Order::Foreground)
            .fixed_pos(screen.left_top())
            .show(ui.ctx(), |ui| {
                let frame = egui::Frame::new()
                    .fill(Color32::WHITE)
                    .stroke(Stroke::new(1.0, super::app_theme::HAIRLINE))
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.set_min_width(screen.width() - 16.0);
                        ui.horizontal(|ui| {
                            self.fill_editor_menus(ui);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
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
                                        .selectable_label(
                                            false,
                                            text(self.locale, Text::SimpleMode),
                                        )
                                        .clicked()
                                    {
                                        self.set_simple_mode(true);
                                    }
                                    let _ = ui.selectable_label(
                                        true,
                                        text(self.locale, Text::DetailedMode),
                                    );
                                    ui.label(if self.dirty {
                                        text(self.locale, Text::Unsaved)
                                    } else {
                                        text(self.locale, Text::Saved)
                                    });
                                    ui.label(
                                        egui::RichText::new(self.full_project_label()).strong(),
                                    );
                                },
                            );
                        });
                    });
                let _ = frame;
            });
    }

    fn show_full_tool_dock(&mut self, ui: &mut egui::Ui) {
        let screen = ui.ctx().viewport_rect();
        egui::Area::new("full-tool-dock".into())
            .order(egui::Order::Foreground)
            .fixed_pos(screen.left_top() + egui::vec2(12.0, 46.0))
            .show(ui.ctx(), |ui| {
                paper(6).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        self.full_tool_tiles(ui);
                    });
                });
            });
    }

    fn full_tool_tiles(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        for (tool, icon, action) in [
            (
                FieldTool::Select,
                PhoneIcon::Person,
                super::keybindings::Action::ToolSelect,
            ),
            (
                FieldTool::Move,
                PhoneIcon::PersonMove,
                super::keybindings::Action::ToolMove,
            ),
            (
                FieldTool::Place,
                PhoneIcon::PersonPlus,
                super::keybindings::Action::ToolPlace,
            ),
        ] {
            let label = tool.label(self.locale);
            let tip = self.key_tip(&ctx, label, action);
            if self.full_tile(ui, icon, label, &tip, true, self.field_tool == tool) {
                self.set_field_tool(tool);
            }
        }
        let undo = UiCommand::Undo.label(self.locale);
        let undo_tip = self.key_tip(
            &ctx,
            undo,
            super::keybindings::Action::Command(UiCommand::Undo),
        );
        if self.full_tile(
            ui,
            PhoneIcon::Undo,
            undo,
            &undo_tip,
            self.history.can_undo(),
            false,
        ) {
            self.execute_command(UiCommand::Undo, &ctx);
        }
        let redo = UiCommand::Redo.label(self.locale);
        let redo_tip = self.key_tip(
            &ctx,
            redo,
            super::keybindings::Action::Command(UiCommand::Redo),
        );
        if self.full_tile(
            ui,
            PhoneIcon::Redo,
            redo,
            &redo_tip,
            self.history.can_redo(),
            false,
        ) {
            self.execute_command(UiCommand::Redo, &ctx);
        }
        let add = UiCommand::AddPerformer.label(self.locale);
        let add_tip = self.key_tip(
            &ctx,
            add,
            super::keybindings::Action::Command(UiCommand::AddPerformer),
        );
        if self.full_tile(ui, PhoneIcon::Plus, add, &add_tip, true, false) {
            self.execute_command(UiCommand::AddPerformer, &ctx);
        }
        let remove = UiCommand::RemoveSelectedPerformers.label(self.locale);
        let remove_tip = self.key_tip(
            &ctx,
            remove,
            super::keybindings::Action::Command(UiCommand::RemoveSelectedPerformers),
        );
        if self.full_tile(
            ui,
            PhoneIcon::Minus,
            remove,
            &remove_tip,
            !self.selected.is_empty(),
            false,
        ) {
            self.execute_command(UiCommand::RemoveSelectedPerformers, &ctx);
        }
        let mirror = text(self.locale, Text::Symmetry);
        let mirror_tip = self.key_tip(&ctx, mirror, super::keybindings::Action::ToggleSymmetry);
        if self.full_tile(
            ui,
            PhoneIcon::Mirror,
            mirror,
            &mirror_tip,
            !self.playing,
            self.mirror_moves_on(),
        ) {
            self.simple_toggle_mirror();
        }
        let ghost = text(self.locale, Text::PreviousSet);
        let ghost_tip = self.key_tip(&ctx, ghost, super::keybindings::Action::TogglePreviousGhost);
        if self.full_tile(
            ui,
            PhoneIcon::SheetsNext,
            ghost,
            &ghost_tip,
            self.current_set > 0,
            self.previous_ghost_shown(),
        ) {
            self.toggle_previous_ghost();
        }
        let song = text(self.locale, Text::Song);
        let song_tip = self.key_tip(&ctx, song, super::keybindings::Action::ImportSong);
        if self.full_tile(
            ui,
            PhoneIcon::Note,
            song,
            &song_tip,
            true,
            self.document.audio.is_some(),
        ) {
            self.simple_pick_and_import_song();
        }
        let help = text(self.locale, Text::Help);
        let help_tip = self.key_tip(&ctx, help, super::keybindings::Action::ShowCheatSheet);
        if self.full_tile(ui, PhoneIcon::Tools, help, &help_tip, true, false) {
            self.keybindings.cheat_open = true;
        }
    }

    fn show_full_context_bar(&mut self, ui: &mut egui::Ui) {
        if self.selected.is_empty() {
            return;
        }
        let screen = ui.ctx().viewport_rect();
        let anchor = self
            .selection_screen_anchor(screen)
            .unwrap_or(screen.center());
        let mut pos = anchor + egui::vec2(-24.0, -72.0);
        pos.x = pos.x.clamp(screen.left() + 8.0, screen.right() - 280.0);
        pos.y = pos.y.clamp(screen.top() + 140.0, screen.bottom() - 180.0);
        egui::Area::new("full-context-bar".into())
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ui.ctx(), |ui| {
                paper(6).show(ui, |ui| {
                    ui.set_max_width(screen.width() - 24.0);
                    egui::ScrollArea::horizontal()
                        .id_salt("full-context-scroll")
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(super::i18n::people_selected(
                                        self.locale,
                                        self.selected.len(),
                                    ))
                                    .strong(),
                                );
                                self.context_command(ui, UiCommand::AlignHorizontal);
                                if ui.button(text(self.locale, Text::OnGrid)).clicked() {
                                    self.snap_selected_to_grid();
                                }
                                if ui.button(text(self.locale, Text::FlipCenterLine)).clicked() {
                                    self.mirror_selection_across_field();
                                }
                                self.context_command(ui, UiCommand::DistributeHorizontal);
                                ui.menu_button(text(self.locale, Text::Arrange), |ui| {
                                    for command in [
                                        UiCommand::AlignVertical,
                                        UiCommand::DistributeVertical,
                                        UiCommand::FlipHorizontal,
                                        UiCommand::FlipVertical,
                                        UiCommand::MakeLine,
                                    ] {
                                        self.context_command(ui, command);
                                    }
                                });
                                ui.menu_button(text(self.locale, Text::Pinwheel), |ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        for degrees in [-90.0_f32, -45.0, -15.0, 15.0, 45.0, 90.0] {
                                            let label = if degrees > 0.0 {
                                                format!("+{degrees:.0}°")
                                            } else {
                                                format!("{degrees:.0}°")
                                            };
                                            if ui.button(label).clicked() {
                                                let points = self.selected_points();
                                                self.commit_layout(editing::rotate_about_centroid(
                                                    &points,
                                                    degrees.to_radians(),
                                                ));
                                                ui.close();
                                            }
                                        }
                                    });
                                });
                                self.context_command(ui, UiCommand::CopyFormation);
                                self.context_command(ui, UiCommand::PasteFormation);
                                self.context_command(ui, UiCommand::LockSelection);
                                self.context_command(ui, UiCommand::HideSelection);
                            });
                        });
                });
            });
    }

    fn context_command(&mut self, ui: &mut egui::Ui, command: UiCommand) {
        let enabled = command.enabled(self.command_context(), self.locale);
        let response = ui.add_enabled(
            enabled.is_ok(),
            egui::Button::new(command.label(self.locale)),
        );
        let response = match enabled {
            Ok(()) => response,
            Err(reason) => response.on_disabled_hover_text(reason),
        };
        if response.clicked() {
            self.execute_command(command, ui.ctx());
        }
    }

    fn selection_screen_anchor(&self, screen: egui::Rect) -> Option<Pos2> {
        if self.frame_positions.is_empty() {
            return None;
        }
        let (mut x, mut y, mut count) = (0.0_f32, 0.0_f32, 0.0_f32);
        for &index in &self.selected {
            let Some(point) = self.frame_positions.get(index) else {
                continue;
            };
            x += point.x;
            y += point.y;
            count += 1.0;
        }
        if count == 0.0 {
            return None;
        }
        let grid = &self.document.grid;
        let map = drill_render::FieldMap::with_view(
            grid.width,
            grid.height,
            drill_render::Vec2 {
                x: screen.width(),
                y: screen.height(),
            },
            0.0,
            Some(self.field_viewport.center),
            self.field_viewport.zoom,
        );
        let local = map.map(Point {
            x: x / count,
            y: y / count,
        });
        Some(screen.min + egui::vec2(local.x, local.y))
    }

    fn show_full_inspector(&mut self, ui: &mut egui::Ui, set_counts: f32) {
        let screen = ui.ctx().viewport_rect();
        let top = 46.0;
        let bottom = if self.full_timeline_expanded {
            260.0
        } else {
            112.0
        };
        if self.full_inspector_collapsed || !self.show_inspector {
            egui::Area::new("full-inspector-tab".into())
                .order(egui::Order::Foreground)
                .fixed_pos(egui::pos2(screen.right() - 72.0, screen.top() + top + 4.0))
                .show(ui.ctx(), |ui| {
                    paper(4).show(ui, |ui| {
                        let label = text(self.locale, Text::Inspector);
                        if self.full_tile(ui, PhoneIcon::Person, label, label, true, false) {
                            self.full_inspector_collapsed = false;
                            self.show_inspector = true;
                        }
                    });
                });
            return;
        }
        let height = (screen.height() - top - bottom).max(180.0);
        egui::Area::new("full-inspector".into())
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(screen.right() - 316.0, screen.top() + top))
            .show(ui.ctx(), |ui| {
                paper(8).show(ui, |ui| {
                    ui.set_width(300.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(text(self.locale, Text::Inspector)).strong());
                        if ui
                            .small_button("−")
                            .on_hover_text(text(self.locale, Text::Inspector))
                            .clicked()
                        {
                            self.full_inspector_collapsed = true;
                        }
                    });
                    let body = (height - 36.0).max(120.0);
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(300.0, body), Sense::hover());
                    let mut child = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(rect)
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                    );
                    child.set_clip_rect(rect);
                    self.show_workspace_inspector(&mut child, set_counts);
                });
            });
    }

    fn show_full_timeline(&mut self, ui: &mut egui::Ui) {
        let screen = ui.ctx().viewport_rect();
        egui::Area::new("full-timeline".into())
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::LEFT_BOTTOM, egui::Vec2::ZERO)
            .show(ui.ctx(), |ui| {
                let frame = egui::Frame::new()
                    .fill(Color32::WHITE)
                    .stroke(Stroke::new(1.0, super::app_theme::HAIRLINE))
                    .inner_margin(egui::Margin::symmetric(8, 6))
                    .show(ui, |ui| {
                        ui.set_min_width(screen.width() - 16.0);
                        ui.set_max_width(screen.width() - 16.0);
                        if self.full_timeline_expanded {
                            ui.set_max_height(240.0);
                        }
                        ui.horizontal(|ui| {
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
                            if ui.button(text(self.locale, Text::RangeStart)).clicked() {
                                self.jump_to_show_start();
                            }
                            let total = self.document.timeline_counts().max(1) as f32;
                            let mut global = self
                                .document
                                .global_count(self.current_set, self.count_position)
                                .clamp(0.0, total);
                            if ui
                                .add_sized(
                                    [220.0, 18.0],
                                    egui::Slider::new(&mut global, 0.0..=total).show_value(false),
                                )
                                .changed()
                            {
                                self.seek_global(global);
                            }
                            self.show_playback_speed_presets(ui);
                            if ui
                                .selectable_label(
                                    self.full_timeline_expanded,
                                    text(self.locale, Text::Timeline),
                                )
                                .clicked()
                            {
                                self.full_timeline_expanded = !self.full_timeline_expanded;
                            }
                        });
                        self.show_set_card_strip(ui);
                        if self.full_timeline_expanded {
                            egui::ScrollArea::vertical()
                                .id_salt("full-timeline-extra")
                                .max_height(150.0)
                                .show(ui, |ui| {
                                    self.show_timeline_playback_bar(ui);
                                    let (playhead_set, playhead_count) = self.render_playhead();
                                    let change = super::timeline::draw_count_track(
                                        ui,
                                        &self.document,
                                        playhead_set,
                                        playhead_count,
                                        self.playback_start,
                                        self.playback_end,
                                        &mut self.timeline_view,
                                        self.locale,
                                        self.presence.set_marks(),
                                    );
                                    self.apply_full_timeline_change(change);
                                });
                        }
                    });
                let _ = frame;
            });
    }

    fn apply_full_timeline_change(&mut self, change: super::timeline::TimelineChange) {
        if let Some((start, end)) = change.range {
            self.playback_start = start;
            self.playback_end = end;
        }
        if change.viewport_interacted {
            self.timeline_follow = false;
        }
        if let Some(edit) = change.marker_edit {
            self.execute_edit(edit, text(self.locale, Text::Timeline));
        }
        if let Some((set_index, local_count)) = change.seek {
            self.scrub_to(set_index, local_count);
        }
    }
}
