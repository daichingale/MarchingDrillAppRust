//! Light overlay chrome for the full editor.
//!
//! The field is laid out edge to edge. Menus, tools, the selection bar, the
//! inspector, and the timeline float on top of it so they do not push the
//! ground into a column.

use super::simple_mode::{PhoneIcon, PictogramTile};
use super::*;

const CONTEXT_BAR_H: f32 = 46.0;
const CONTEXT_BAR_GAP: f32 = 20.0;
const CONTEXT_BAR_HALF: f32 = 360.0;
const CONTEXT_DOT_PAD: f32 = 16.0;

fn context_bar_hits(top: f32, bottom: f32, center_x: f32, performers: &[egui::Pos2]) -> bool {
    performers.iter().any(|at| {
        at.y >= top - CONTEXT_DOT_PAD
            && at.y <= bottom + CONTEXT_DOT_PAD
            && (at.x - center_x).abs() <= CONTEXT_BAR_HALF + CONTEXT_DOT_PAD
    })
}

/// Above the selection when that band is empty, otherwise below, otherwise
/// the top edge of the open field.
fn context_bar_anchor(
    bounds: egui::Rect,
    visible: egui::Rect,
    performers: &[egui::Pos2],
) -> (egui::Align2, egui::Pos2) {
    let center_x = bounds.center().x;
    let above_top = bounds.min.y - CONTEXT_BAR_GAP - CONTEXT_BAR_H;
    let above_bottom = bounds.min.y - CONTEXT_BAR_GAP;
    let below_top = bounds.max.y + CONTEXT_BAR_GAP;
    let below_bottom = bounds.max.y + CONTEXT_BAR_GAP + CONTEXT_BAR_H;
    let above_fits = above_top >= visible.top()
        && !context_bar_hits(above_top, above_bottom, center_x, performers);
    if above_fits {
        return (
            egui::Align2::CENTER_BOTTOM,
            egui::pos2(center_x, above_bottom),
        );
    }
    let below_fits = below_bottom <= visible.bottom()
        && !context_bar_hits(below_top, below_bottom, center_x, performers);
    if below_fits {
        return (egui::Align2::CENTER_TOP, egui::pos2(center_x, below_top));
    }
    (
        egui::Align2::CENTER_TOP,
        egui::pos2(visible.center().x, visible.top() + 8.0),
    )
}

fn paper(margin: i8) -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::WHITE)
        .stroke(Stroke::new(1.0, super::app_theme::HAIRLINE))
        .corner_radius(super::app_theme::CORNER_SM)
        .inner_margin(egui::Margin::same(margin))
        .shadow(egui::Shadow::NONE)
}

impl DrillApp {
    pub(super) fn show_full_chrome(&mut self, ui: &mut egui::Ui) {
        self.show_full_tool_dock(ui);
        self.show_full_status_cluster(ui);
        self.show_full_context_bar(ui);
        self.show_full_inspector(ui);
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
                width: 64.0,
                enabled,
                active,
                pulse: false,
                filled: false,
                compact: false,
                time: ui.input(|input| input.time as f32),
                lock_width: true,
            },
        );
        clicked && enabled
    }

    /// One band: the menus fold into ☰ so they no longer sit on a second row
    /// above the tool tiles. The field keeps the height that row used to take.
    fn show_full_tool_dock(&mut self, ui: &mut egui::Ui) {
        let screen = ui.ctx().viewport_rect();
        egui::Area::new("full-tool-dock".into())
            .order(egui::Order::Foreground)
            .fade_in(false)
            .fixed_pos(screen.left_top() + egui::vec2(8.0, 8.0))
            .show(ui.ctx(), |ui| {
                paper(6).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.scope(|ui| {
                            ui.spacing_mut().button_padding = egui::vec2(12.0, 16.0);
                            ui.menu_button(
                                egui::RichText::new(text(self.locale, Text::Menu)).size(22.0),
                                |ui| self.fill_editor_menus(ui),
                            );
                        });
                        self.full_tool_tiles(ui);
                    });
                });
            });
    }

    fn show_full_status_cluster(&mut self, ui: &mut egui::Ui) {
        let screen = ui.ctx().viewport_rect();
        egui::Area::new("full-status-cluster".into())
            .order(egui::Order::Foreground)
            .fade_in(false)
            .pivot(egui::Align2::RIGHT_TOP)
            .fixed_pos(egui::pos2(screen.right() - 8.0, screen.top() + 8.0))
            .show(ui.ctx(), |ui| {
                paper(6).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        ui.allocate_ui_with_layout(
                            egui::vec2(120.0, 24.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(self.full_project_label())
                                            .color(Color32::from_rgb(32, 36, 44)),
                                    )
                                    .truncate(),
                                );
                            },
                        );
                        ui.label(if self.dirty {
                            text(self.locale, Text::Unsaved)
                        } else {
                            text(self.locale, Text::Saved)
                        });
                        egui::Frame::new()
                            .fill(super::simple_mode::SIMPLE_BLUE)
                            .corner_radius(6.0)
                            .inner_margin(egui::Margin::symmetric(8, 3))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(text(self.locale, Text::DetailedMode))
                                        .size(14.0)
                                        .color(Color32::WHITE),
                                );
                            });
                        if ui.button(text(self.locale, Text::SimpleMode)).clicked() {
                            self.set_simple_mode(true);
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
        if self.full_tile(
            ui,
            PhoneIcon::Plus,
            text(self.locale, Text::Add),
            &add_tip,
            true,
            false,
        ) {
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
            text(self.locale, Text::Delete),
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
        let visible = self.full_visible_field(screen);
        if visible.width() < 120.0 || visible.height() < 48.0 {
            return;
        }
        let Some(bounds) = self.selection_screen_bounds(screen) else {
            return;
        };
        let performers = self.performer_screen_points(screen);
        // Hug the buttons. A fixed width left an empty stretch on the left
        // because the buttons were centered inside a bar they did not fill.
        // The bar also stays off the dots: above the selection, then below,
        // then the top edge of the field when both sides are crowded.
        let (pivot, at) = context_bar_anchor(bounds, visible, &performers);
        let dragging = self.drag_preview.is_some();
        egui::Area::new("full-context-bar".into())
            .order(egui::Order::Foreground)
            .fade_in(false)
            .pivot(pivot)
            .fixed_pos(at)
            .constrain_to(visible)
            .show(ui.ctx(), |ui| {
                if dragging {
                    ui.set_opacity(0.45);
                }
                paper(6).show(ui, |ui| {
                    // `horizontal` reserves the full available width, so the
                    // paper stays wider than the buttons. A left-to-right
                    // scope keeps the bar as wide as its contents.
                    // `.strong()` uses the active-widget color, which is white
                    // on this theme, so the count vanished into the paper.
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            ui.label(
                                egui::RichText::new(super::i18n::people_selected(
                                    self.locale,
                                    self.selected.len(),
                                ))
                                .color(Color32::from_rgb(32, 36, 44)),
                            );
                            self.context_command_visible(ui, UiCommand::AlignHorizontal);
                            if ui.button(text(self.locale, Text::OnGrid)).clicked() {
                                self.snap_selected_to_grid();
                            }
                            if ui.button(text(self.locale, Text::FlipCenterLine)).clicked() {
                                self.mirror_selection_across_field();
                            }
                            self.context_command_visible(ui, UiCommand::DistributeHorizontal);
                            ui.menu_button(text(self.locale, Text::Arrange), |ui| {
                                for command in [
                                    UiCommand::AlignVertical,
                                    UiCommand::DistributeVertical,
                                    UiCommand::FlipHorizontal,
                                    UiCommand::FlipVertical,
                                    UiCommand::MakeLine,
                                ] {
                                    self.context_command_visible(ui, command);
                                }
                            });
                            ui.menu_button(text(self.locale, Text::More), |ui| {
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
                        },
                    );
                });
            });
    }

    /// Commands that do nothing for a single person stay off the bar.
    fn context_command_visible(&mut self, ui: &mut egui::Ui, command: UiCommand) {
        let needs_several = matches!(
            command,
            UiCommand::DistributeHorizontal | UiCommand::DistributeVertical | UiCommand::MakeLine
        );
        if needs_several && self.selected.len() < 2 {
            return;
        }
        self.context_command(ui, command);
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

    /// Field area that is not covered by the floating toolbar, inspector, or timeline.
    /// While the return-to-edit chip is up, the band it occupies is left out too.
    pub(super) fn full_visible_field(&self, screen: egui::Rect) -> egui::Rect {
        let hint = !self.is_editable_set_start();
        let top = if hint && self.selected.is_empty() {
            160.0
        } else {
            104.0
        };
        let mut bottom = if self.full_timeline_expanded {
            340.0
        } else {
            118.0
        };
        if hint && !self.selected.is_empty() {
            bottom += 64.0;
        }
        let right = if self.full_inspector_collapsed || !self.show_inspector {
            16.0
        } else {
            332.0
        };
        let min = screen.left_top() + egui::vec2(12.0, top);
        let max = screen.right_bottom() - egui::vec2(right, bottom);
        if max.x <= min.x || max.y <= min.y {
            return egui::Rect::from_min_size(min, egui::Vec2::ZERO);
        }
        egui::Rect::from_min_max(min, max)
    }

    /// Keeps dots and their numbers out of the floating timeline and inspector.
    ///
    /// egui's first area frame is a 600×400 default anchored to the bottom, so
    /// the stored rect is much taller than the panel we actually draw. Clip to
    /// the layout we use, not that placeholder.
    pub(super) fn full_field_paint_clip(
        &self,
        _ctx: &egui::Context,
        field: egui::Rect,
    ) -> egui::Rect {
        let mut clip = field;
        let bottom = if self.full_timeline_expanded {
            340.0
        } else {
            100.0
        };
        clip.max.y = (field.bottom() - bottom).max(field.top());
        if self.show_inspector && !self.full_inspector_collapsed {
            clip.max.x = (field.right() - 324.0).max(field.left());
        }
        clip
    }

    fn selection_screen_bounds(&self, screen: egui::Rect) -> Option<egui::Rect> {
        if self.selected.is_empty() || self.frame_positions.is_empty() {
            return None;
        }
        let grid = &self.document.grid;
        let map = self
            .field_viewport
            .field_map(grid, egui::vec2(screen.width(), screen.height()));
        let mut bounds = None;
        for &index in &self.selected {
            let Some(point) = self.frame_positions.get(index).copied() else {
                continue;
            };
            let local = map.map(point);
            let at = screen.min + egui::vec2(local.x, local.y);
            let dot = egui::Rect::from_center_size(at, egui::vec2(36.0, 36.0));
            bounds = Some(bounds.map_or(dot, |existing: egui::Rect| existing.union(dot)));
        }
        bounds
    }

    fn performer_screen_points(&self, screen: egui::Rect) -> Vec<egui::Pos2> {
        let grid = &self.document.grid;
        let map = self
            .field_viewport
            .field_map(grid, egui::vec2(screen.width(), screen.height()));
        self.frame_positions
            .iter()
            .map(|point| {
                let local = map.map(*point);
                screen.min + egui::vec2(local.x, local.y)
            })
            .collect()
    }

    fn show_full_inspector(&mut self, ui: &mut egui::Ui) {
        let screen = ui.ctx().viewport_rect();
        let top = 52.0;
        let bottom = if self.full_timeline_expanded {
            340.0
        } else {
            112.0
        };
        if self.full_inspector_collapsed || !self.show_inspector {
            egui::Area::new("full-inspector-tab".into())
                .order(egui::Order::Foreground)
                .fade_in(false)
                .fixed_pos(egui::pos2(screen.right() - 92.0, screen.top() + top + 4.0))
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
            .fade_in(false)
            .fixed_pos(egui::pos2(screen.right() - 316.0, screen.top() + top))
            .show(ui.ctx(), |ui| {
                paper(8).show(ui, |ui| {
                    ui.set_width(300.0);
                    self.show_full_inspector_header(ui);
                    let body = (height - 36.0).max(120.0);
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(300.0, body), Sense::hover());
                    let mut child = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(rect)
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                    );
                    child.set_clip_rect(rect);
                    egui::ScrollArea::vertical()
                        .id_salt("full-inspector-scroll")
                        .auto_shrink([false, false])
                        .show(&mut child, |ui| {
                            self.show_full_inspector_body(ui);
                        });
                });
            });
    }

    fn show_full_inspector_header(&mut self, ui: &mut egui::Ui) {
        let width = 284.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 32.0), Sense::hover());
        let title = if self.selected.len() > 1 {
            super::i18n::people_selected(self.locale, self.selected.len())
        } else {
            text(self.locale, Text::Inspector).to_string()
        };
        let title_rect = egui::Rect::from_min_max(
            rect.left_top(),
            egui::pos2(rect.left() + 148.0, rect.bottom()),
        );
        ui.painter().text(
            title_rect.left_center(),
            egui::Align2::LEFT_CENTER,
            &title,
            egui::FontId::proportional(16.0),
            Color32::from_rgb(32, 36, 44),
        );
        let button_rect = egui::Rect::from_min_max(
            egui::pos2(rect.left() + 148.0, rect.top()),
            rect.right_bottom(),
        );
        let mut buttons = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(button_rect)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        if self.full_collapse_button(&mut buttons) {
            self.full_inspector_collapsed = true;
        }
        if !self.selected.is_empty()
            && buttons
                .button(text(self.locale, Text::ClearSelection))
                .clicked()
        {
            self.clear_selection();
        }
    }

    fn full_collapse_button(&self, ui: &mut egui::Ui) -> bool {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), Sense::click());
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, 6.0, super::app_theme::ACCENT_SOFT);
        }
        let color = if response.hovered() {
            super::app_theme::ACCENT
        } else {
            super::app_theme::SECONDARY_TEXT
        };
        let center = rect.center();
        let stroke = Stroke::new(1.8, color);
        ui.painter().line_segment(
            [
                center + egui::vec2(-4.0, -5.0),
                center + egui::vec2(2.0, 0.0),
            ],
            stroke,
        );
        ui.painter().line_segment(
            [
                center + egui::vec2(2.0, 0.0),
                center + egui::vec2(-4.0, 5.0),
            ],
            stroke,
        );
        response
            .on_hover_text(text(self.locale, Text::Collapse))
            .clicked()
    }

    fn show_full_inspector_body(&mut self, ui: &mut egui::Ui) {
        if self.selected.is_empty() {
            self.show_full_roster(ui);
        } else {
            self.show_performer_properties(ui);
        }
    }

    fn show_full_roster(&mut self, ui: &mut egui::Ui) {
        let mut query = self.full_roster_query.clone();
        let search = ui.add(
            egui::TextEdit::singleline(&mut query)
                .hint_text(text(self.locale, Text::RosterSearch))
                .desired_width(ui.available_width().clamp(80.0, 284.0)),
        );
        ui.painter().rect_stroke(
            search.rect,
            6.0,
            Stroke::new(1.0, super::app_theme::HAIRLINE),
            egui::StrokeKind::Inside,
        );
        if search.changed() {
            self.full_roster_query = query.clone();
        }
        let sections: Vec<(drill_core::SectionId, String)> = self
            .document
            .sections
            .iter()
            .map(|section| (section.id, section.name.clone()))
            .collect();
        if !sections.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for (id, name) in &sections {
                    let on = self.full_section_filter.contains(id);
                    if full_filter_chip(ui, name, on) {
                        if on {
                            self.full_section_filter.retain(|section| section != id);
                        } else {
                            self.full_section_filter.push(*id);
                        }
                    }
                }
            });
        }
        if self.document.performers.is_empty() {
            ui.add_space(8.0);
            ui.label(text(self.locale, Text::NoPerformers));
            ui.label(
                egui::RichText::new(text(self.locale, Text::PlaceHint))
                    .color(super::app_theme::SECONDARY_TEXT),
            );
            if ui.button(FieldTool::Place.label(self.locale)).clicked() {
                self.set_field_tool(FieldTool::Place);
            }
            return;
        }
        let filter = self.full_section_filter.clone();
        let mut rows = Vec::new();
        for (index, performer) in self.document.performers.iter().enumerate() {
            if !self.is_selectable_index(index) {
                continue;
            }
            if !filter.is_empty() && !filter.contains(&performer.section) {
                continue;
            }
            let (number, name) = split_performer_label(&performer.label);
            let section_name = self
                .document
                .sections
                .iter()
                .find(|section| section.id == performer.section)
                .map(|section| section.name.clone())
                .unwrap_or_default();
            let haystack = format!("{number} {name} {section_name}");
            if !query.is_empty() && !haystack.to_lowercase().contains(&query.to_lowercase()) {
                continue;
            }
            rows.push((index, number, name, section_name));
        }
        rows.sort_by(|left, right| {
            roster_number_order(&left.1, &right.1).then_with(|| left.2.cmp(&right.2))
        });
        if rows.is_empty() {
            ui.label(text(self.locale, Text::NoMatches));
            return;
        }
        ui.label(
            egui::RichText::new(super::i18n::roster_count(self.locale, rows.len()))
                .color(super::app_theme::SECONDARY_TEXT),
        );
        for (index, number, name, section) in rows {
            self.full_roster_row(ui, index, &number, &name, &section);
        }
    }

    fn full_roster_row(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        number: &str,
        name: &str,
        section: &str,
    ) {
        let selected = self.selected.contains(&index);
        let width = ui.available_width().max(40.0);
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 32.0), Sense::click());
        let fill = if selected {
            super::app_theme::ACCENT_SOFT
        } else if response.hovered() {
            super::app_theme::ACCENT_HOVER
        } else {
            Color32::TRANSPARENT
        };
        if fill != Color32::TRANSPARENT {
            ui.painter().rect_filled(rect, 6.0, fill);
        }
        ui.painter().hline(
            rect.x_range(),
            rect.bottom(),
            Stroke::new(1.0, super::app_theme::HAIRLINE),
        );
        let ink = Color32::from_rgb(32, 36, 44);
        let clip = ui.clip_rect();
        let number_rect = egui::Rect::from_min_max(
            rect.left_top(),
            egui::pos2(rect.left() + 44.0, rect.bottom()),
        );
        let name_rect = egui::Rect::from_min_max(
            egui::pos2(rect.left() + 44.0, rect.top()),
            egui::pos2((rect.right() - 96.0).max(rect.left() + 44.0), rect.bottom()),
        );
        let section_rect = egui::Rect::from_min_max(
            egui::pos2((rect.right() - 92.0).max(rect.left()), rect.top()),
            rect.right_bottom(),
        );
        ui.set_clip_rect(clip.intersect(number_rect.shrink(4.0)));
        ui.painter().text(
            rect.left_center() + egui::vec2(8.0, 0.0),
            egui::Align2::LEFT_CENTER,
            number,
            egui::FontId::monospace(13.0),
            ink,
        );
        ui.set_clip_rect(clip.intersect(name_rect));
        // A number with no separate name already shows in the number column.
        // Repeating 名前なし on every row hides the few people who do have names.
        let unnamed = text(self.locale, Text::NoName);
        let (shown, name_ink) = if !name.is_empty() {
            (name, ink)
        } else if number.is_empty() {
            (unnamed, super::app_theme::SECONDARY_TEXT)
        } else {
            ("", ink)
        };
        if !shown.is_empty() {
            ui.painter().text(
                name_rect.left_center(),
                egui::Align2::LEFT_CENTER,
                shown,
                egui::FontId::proportional(13.0),
                name_ink,
            );
        }
        ui.set_clip_rect(clip.intersect(section_rect.shrink2(egui::vec2(4.0, 0.0))));
        ui.painter().text(
            section_rect.right_center() + egui::vec2(-8.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            section,
            egui::FontId::proportional(13.0),
            super::app_theme::SECONDARY_TEXT,
        );
        ui.set_clip_rect(clip);
        if response.clicked() {
            let additive = ui.input(|input| {
                input.modifiers.shift || input.modifiers.command || input.modifiers.ctrl
            });
            self.select_performer_from_list(index, additive);
        }
    }

    fn show_full_timeline(&mut self, ui: &mut egui::Ui) {
        let screen = ui.ctx().viewport_rect();
        egui::Area::new("full-timeline".into())
            .order(egui::Order::Foreground)
            .fade_in(false)
            .anchor(egui::Align2::LEFT_BOTTOM, egui::Vec2::ZERO)
            .show(ui.ctx(), |ui| {
                let frame = egui::Frame::new()
                    .fill(Color32::from_rgb(244, 246, 250))
                    .stroke(Stroke::new(1.0, super::app_theme::HAIRLINE))
                    .inner_margin(egui::Margin::symmetric(8, 6))
                    .show(ui, |ui| {
                        ui.set_min_width(screen.width() - 16.0);
                        ui.set_max_width(screen.width() - 16.0);
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
                            self.full_count_scrub(ui, 200.0);
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
                        ui.horizontal(|ui| {
                            self.show_music_sync_controls(ui);
                        });
                        self.show_set_card_strip(ui);
                        if self.full_timeline_expanded {
                            let (playhead_set, playhead_count) = self.render_playhead();
                            let wave = self.timeline_wave(ui.available_width());
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
                                &wave,
                            );
                            self.apply_full_timeline_change(change);
                            ui.collapsing(text(self.locale, Text::PlaybackOptions), |ui| {
                                self.show_timeline_grouped_options(ui);
                            });
                            ui.collapsing(text(self.locale, Text::Sets), |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    self.context_command(ui, UiCommand::DuplicateSet);
                                    self.context_command(ui, UiCommand::DeleteSet);
                                });
                            });
                            ui.collapsing(text(self.locale, Text::Parts), |ui| {
                                self.show_section_picker(ui);
                            });
                            ui.collapsing(text(self.locale, Text::Markers), |ui| {
                                let mut markers =
                                    std::mem::take(&mut self.production_markers_panel);
                                markers.show(self, ui);
                                self.production_markers_panel = markers;
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

    /// Count scrub with a visible rail. The stock slider paints a white
    /// handle on the white timeline, which reads as an empty oval.
    fn full_count_scrub(&mut self, ui: &mut egui::Ui, width: f32) {
        let total = self.document.timeline_counts().max(1) as f32;
        let global = self
            .document
            .global_count(self.current_set, self.count_position)
            .clamp(0.0, total);
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, 22.0), Sense::click_and_drag());
        let rail = egui::Rect::from_center_size(rect.center(), egui::vec2(width - 12.0, 6.0));
        ui.painter()
            .rect_filled(rail, 3.0, Color32::from_rgb(168, 176, 188));
        let t = (global / total).clamp(0.0, 1.0);
        let x = rail.left() + t * rail.width();
        let filled = egui::Rect::from_min_max(
            rail.left_top(),
            egui::pos2(x.max(rail.left()), rail.bottom()),
        );
        ui.painter()
            .rect_filled(filled, 2.0, super::app_theme::ACCENT);
        let handle = egui::pos2(x, rail.center().y);
        ui.painter().circle_filled(handle, 7.0, Color32::WHITE);
        ui.painter()
            .circle_stroke(handle, 7.0, Stroke::new(2.0, super::app_theme::ACCENT));
        if (response.dragged() || response.clicked())
            && let Some(pointer) = response.interact_pointer_pos()
        {
            let next = ((pointer.x - rail.left()) / rail.width().max(1.0)).clamp(0.0, 1.0);
            self.seek_global(next * total);
        }
    }
}

fn full_filter_chip(ui: &mut egui::Ui, label: &str, on: bool) -> bool {
    let font = egui::FontId::proportional(13.0);
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        font.clone(),
        Color32::from_rgb(32, 36, 44),
    );
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(galley.size().x + 22.0, 26.0), Sense::click());
    let fill = if on {
        super::app_theme::ACCENT_SOFT
    } else if response.hovered() {
        super::app_theme::ACCENT_HOVER
    } else {
        Color32::from_rgb(248, 249, 252)
    };
    ui.painter().rect_filled(rect.shrink(1.0), 12.0, fill);
    ui.painter().rect_stroke(
        rect,
        13.0,
        Stroke::new(
            1.0,
            if on {
                super::app_theme::ACCENT
            } else {
                Color32::from_rgb(176, 182, 194)
            },
        ),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        font,
        Color32::from_rgb(32, 36, 44),
    );
    response.clicked()
}

fn roster_number_order(left: &str, right: &str) -> std::cmp::Ordering {
    match (left.parse::<i64>(), right.parse::<i64>()) {
        (Ok(a), Ok(b)) => a.cmp(&b).then_with(|| left.cmp(right)),
        (Ok(_), Err(_)) => std::cmp::Ordering::Less,
        (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
        (Err(_), Err(_)) => left.cmp(right),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_bar_moves_below_when_the_row_above_is_occupied() {
        let visible = egui::Rect::from_min_max(egui::pos2(0.0, 104.0), egui::pos2(1280.0, 700.0));
        let bounds = egui::Rect::from_min_max(egui::pos2(400.0, 420.0), egui::pos2(700.0, 480.0));
        let row_above = egui::pos2(550.0, bounds.min.y - 28.0);
        let (pivot, at) = context_bar_anchor(bounds, visible, &[row_above]);
        assert_eq!(pivot, egui::Align2::CENTER_TOP);
        assert!(at.y >= bounds.max.y);
        let (clear_pivot, clear_at) = context_bar_anchor(bounds, visible, &[]);
        assert_eq!(clear_pivot, egui::Align2::CENTER_BOTTOM);
        assert!(clear_at.y <= bounds.min.y);
        let crowded = [
            egui::pos2(550.0, bounds.min.y - 28.0),
            egui::pos2(550.0, bounds.max.y + 36.0),
        ];
        let (top_pivot, top_at) = context_bar_anchor(bounds, visible, &crowded);
        assert_eq!(top_pivot, egui::Align2::CENTER_TOP);
        assert!(top_at.y <= visible.top() + 8.5);
    }
}
