//! One menu bar and one icon row, shared by simple mode and detailed mode.
//!
//! Formation commands, sheets, and the less common rehearsal actions live in
//! the menus. The icon row keeps the actions a rehearsal uses every minute.

use super::keybindings::Action;
use super::simple_mode::{
    PhoneIcon, PlainAction, SIMPLE_BLUE, SIMPLE_INK, SimpleGuide, paint_phone_icon_at,
};
use super::*;

const ICON_W: f32 = 58.0;
const ICON_H: f32 = 44.0;

impl DrillApp {
    /// Menu, icons, and scene tabs. Painted before the field so the picture
    /// gets whatever height is left.
    pub(crate) fn show_desktop_chrome(&mut self, ui: &mut egui::Ui) {
        let simple = self.simple_mode.enabled;
        let fill = if simple {
            Color32::from_rgb(246, 248, 252)
        } else {
            ui.visuals().panel_fill
        };
        let hairline = if simple {
            super::app_theme::HAIRLINE
        } else {
            ui.visuals().widgets.noninteractive.bg_stroke.color
        };
        let bar = egui::Frame::new()
            .fill(fill)
            .inner_margin(egui::Margin::symmetric(8, 3))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.style_mut()
                    .text_styles
                    .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
                ui.style_mut()
                    .text_styles
                    .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
                egui::MenuBar::new().ui(ui, |ui| {
                    ui.spacing_mut().button_padding = egui::vec2(10.0, 4.0);
                    ui.spacing_mut().item_spacing.x = 2.0;
                    self.fill_editor_menus(ui);
                });
                if self.icon_toolbar_shown() {
                    ui.separator();
                    self.show_icon_toolbar(ui);
                }
                ui.separator();
                self.show_scene_tabs(ui);
            });
        ui.painter().hline(
            bar.response.rect.x_range(),
            bar.response.rect.bottom(),
            Stroke::new(1.0, hairline),
        );
        self.desktop_chrome_px = bar.response.rect.height();
    }

    fn icon_toolbar_shown(&self) -> bool {
        if self.simple_mode.enabled {
            self.simple_toolbar_open()
        } else {
            !self.full_toolbar_collapsed
        }
    }

    fn show_scene_tabs(&mut self, ui: &mut egui::Ui) {
        let set_count = self.document.sets.len();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.label(
                egui::RichText::new(i18n::registered(self.locale, "desktop-chrome.001"))
                    .size(13.0)
                    .color(if self.simple_mode.enabled {
                        SIMPLE_INK
                    } else {
                        ui.visuals().text_color()
                    }),
            );
            let toggle_w = 168.0;
            let chips_w = (ui.available_width() - toggle_w).max(80.0);
            // 16px under the circle is reserved for the move-length number.
            ui.allocate_ui_with_layout(
                egui::vec2(chips_w, 64.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    egui::ScrollArea::horizontal()
                        .id_salt("desktop-scenes")
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                for index in 0..set_count {
                                    self.simple_scene_chip(ui, index);
                                }
                            });
                        });
                },
            );
            self.simple_previous_toggle(ui);
        });
    }

    fn show_icon_toolbar(&mut self, ui: &mut egui::Ui) {
        if self.simple_mode.enabled {
            self.simple_clear_hint_targets();
        }
        let hint = self
            .simple_mode
            .enabled
            .then(|| self.simple_active_hint())
            .flatten();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let status_w = 210.0;
            let icons_w = (ui.available_width() - status_w).max(160.0);
            ui.allocate_ui_with_layout(
                egui::vec2(icons_w, ICON_H),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    egui::ScrollArea::horizontal()
                        .id_salt("desktop-icons")
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                self.icon_commands(ui, hint);
                            });
                        });
                },
            );
            self.icon_status(ui);
        });
    }

    fn icon_commands(&mut self, ui: &mut egui::Ui, hint: Option<SimpleGuide>) {
        let ctx = ui.ctx().clone();
        let simple = self.simple_mode.enabled;
        if !simple {
            for (tool, icon, action) in [
                (FieldTool::Select, PhoneIcon::Person, Action::ToolSelect),
                (FieldTool::Move, PhoneIcon::PersonMove, Action::ToolMove),
                (FieldTool::Place, PhoneIcon::PersonPlus, Action::ToolPlace),
            ] {
                let label = tool.label(self.locale);
                let tip = self.key_tip(&ctx, label, action);
                if self.tool_icon(ui, icon, label, &tip, true, self.field_tool == tool, false) {
                    self.set_field_tool(tool);
                }
            }
            self.icon_gap(ui);
        }

        let undo_label = i18n::registered(self.locale, "desktop-chrome.019");
        let undo_tip = self.key_tip(&ctx, undo_label, Action::Command(UiCommand::Undo));
        if self.tool_icon(
            ui,
            PhoneIcon::Undo,
            undo_label,
            &undo_tip,
            self.history.can_undo(),
            false,
            false,
        ) {
            self.execute_command(UiCommand::Undo, &ctx);
        }
        let redo_label = i18n::registered(self.locale, "desktop-chrome.020");
        let redo_tip = self.key_tip(&ctx, redo_label, Action::Command(UiCommand::Redo));
        if self.tool_icon(
            ui,
            PhoneIcon::Redo,
            redo_label,
            &redo_tip,
            self.history.can_redo(),
            false,
            false,
        ) {
            self.execute_command(UiCommand::Redo, &ctx);
        }
        self.icon_gap(ui);

        let open_label = i18n::registered(self.locale, "desktop-chrome.021");
        let open_tip = self.key_tip(&ctx, open_label, Action::Command(UiCommand::OpenDocument));
        if self.tool_icon(
            ui,
            PhoneIcon::Open,
            open_label,
            &open_tip,
            true,
            false,
            false,
        ) {
            if simple {
                self.simple_open_shows();
            } else {
                self.request_open_document(DocumentOpenKind::LegacyJson);
            }
        }
        let save_label = text(self.locale, Text::Save);
        let save_tip = self.key_tip(&ctx, save_label, Action::Command(UiCommand::Save));
        if self.tool_icon(
            ui,
            PhoneIcon::Sheet,
            save_label,
            &save_tip,
            true,
            false,
            false,
        ) {
            self.execute_command(UiCommand::Save, &ctx);
        }
        self.icon_gap(ui);

        let playing = self.playing;
        let (play_icon, play_label) = if playing {
            (
                PhoneIcon::Stop,
                i18n::registered(self.locale, "desktop-chrome.022"),
            )
        } else {
            (
                PhoneIcon::Play,
                i18n::registered(self.locale, "desktop-chrome.023"),
            )
        };
        let play_tip = self.key_tip(&ctx, play_label, Action::Command(UiCommand::PlayPause));
        let (play_rect, play_click) = self.tool_icon_rect(
            ui,
            play_icon,
            play_label,
            &play_tip,
            true,
            true,
            hint == Some(SimpleGuide::Play),
        );
        if simple {
            self.simple_note_hint_target(SimpleGuide::Play, play_rect);
        }
        if play_click {
            if simple {
                self.simple_press_play(&ctx);
            } else {
                self.toggle_playback(&ctx);
            }
        }

        let back_label = i18n::registered(self.locale, "desktop-chrome.024");
        let back_tip = self.key_tip(&ctx, back_label, Action::PreviousCount);
        if self.tool_icon(
            ui,
            PhoneIcon::ChevronDown,
            i18n::registered(self.locale, "desktop-chrome.002"),
            &back_tip,
            !playing,
            false,
            false,
        ) {
            self.key_step_count(-1);
        }
        let next_count = i18n::registered(self.locale, "desktop-chrome.025");
        let next_count_tip = self.key_tip(&ctx, next_count, Action::NextCount);
        if self.tool_icon(
            ui,
            PhoneIcon::Next,
            i18n::registered(self.locale, "desktop-chrome.003"),
            &next_count_tip,
            !playing,
            false,
            false,
        ) {
            self.key_step_count(1);
        }
        if playing {
            let total = self
                .document
                .sets
                .get(self.current_set)
                .map(|set| set.counts)
                .unwrap_or(1);
            let now = (self.count_position.round() as i32).clamp(0, i32::from(total));
            ui.label(
                egui::RichText::new(format!("{now}/{total}"))
                    .size(14.0)
                    .strong()
                    .color(SIMPLE_BLUE),
            );
        }
        self.icon_gap(ui);

        let scene_label = i18n::registered(self.locale, "desktop-chrome.026");
        let scene_tip = self.key_tip(&ctx, scene_label, Action::Command(UiCommand::DuplicateSet));
        let can_scene =
            !playing && !self.document.performers.is_empty() && self.is_editable_set_start();
        let (scene_rect, scene_click) = self.tool_icon_rect(
            ui,
            PhoneIcon::SheetsNext,
            i18n::registered(self.locale, "desktop-chrome.004"),
            &scene_tip,
            can_scene,
            false,
            hint == Some(SimpleGuide::NextSet),
        );
        if simple {
            self.simple_note_hint_target(SimpleGuide::NextSet, scene_rect);
        }
        if scene_click {
            if simple {
                self.simple_press_add_scene();
            } else {
                self.execute_command(UiCommand::DuplicateSet, &ctx);
            }
        }

        let add_label = if simple {
            i18n::registered(self.locale, "desktop-chrome.027")
        } else {
            text(self.locale, Text::Add)
        };
        let add_tip = self.key_tip(&ctx, add_label, Action::Command(UiCommand::AddPerformer));
        let can_add = !playing && self.is_editable_set_start();
        let (add_rect, add_click) = self.tool_icon_rect(
            ui,
            PhoneIcon::PersonPlus,
            add_label,
            &add_tip,
            can_add,
            false,
            hint == Some(SimpleGuide::Place),
        );
        if simple {
            self.simple_note_hint_target(SimpleGuide::Place, add_rect);
        }
        if add_click {
            if simple {
                self.simple_press_place();
            } else {
                self.execute_command(UiCommand::AddPerformer, &ctx);
            }
        }

        if simple {
            self.simple_trash_icon(ui);
        } else {
            let remove = text(self.locale, Text::Delete);
            let remove_tip = self.key_tip(
                &ctx,
                remove,
                Action::Command(UiCommand::RemoveSelectedPerformers),
            );
            if self.tool_icon(
                ui,
                PhoneIcon::Trash,
                remove,
                &remove_tip,
                !self.selected.is_empty(),
                false,
                false,
            ) {
                self.execute_command(UiCommand::RemoveSelectedPerformers, &ctx);
            }
        }
        self.icon_gap(ui);

        let zoom_out = i18n::registered(self.locale, "desktop-chrome.028");
        let zoom_out_tip = self.key_tip(&ctx, zoom_out, Action::ZoomOut);
        if self.tool_icon(
            ui,
            PhoneIcon::Minus,
            zoom_out,
            &zoom_out_tip,
            true,
            false,
            false,
        ) {
            self.field_viewport.set_zoom(self.field_viewport.zoom * 0.8);
        }
        let zoom_in = i18n::registered(self.locale, "desktop-chrome.029");
        let zoom_in_tip = self.key_tip(&ctx, zoom_in, Action::ZoomIn);
        if self.tool_icon(
            ui,
            PhoneIcon::Plus,
            zoom_in,
            &zoom_in_tip,
            true,
            false,
            false,
        ) {
            self.field_viewport
                .set_zoom(self.field_viewport.zoom * 1.25);
        }
        let fit = text(self.locale, Text::FitPeople);
        let fit_tip = self.key_tip(&ctx, fit, Action::FitField);
        if self.tool_icon(
            ui,
            PhoneIcon::Chart,
            i18n::registered(self.locale, "desktop-chrome.005"),
            &fit_tip,
            true,
            false,
            false,
        ) {
            if self.document.performers.is_empty() {
                self.fit_whole_floor();
            } else {
                self.zoom_to_performers(ui.ctx().content_rect().size());
            }
        }
    }

    fn simple_trash_icon(&mut self, ui: &mut egui::Ui) {
        let enabled = self.simple_can_remove();
        let label = if self.selected.len() == 1 {
            i18n::registered(self.locale, "desktop-chrome.030")
        } else {
            i18n::registered(self.locale, "desktop-chrome.031")
        };
        let tip = self.key_tip(
            ui.ctx(),
            label,
            Action::Command(UiCommand::RemoveSelectedPerformers),
        );
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ICON_W, ICON_H), Sense::click_and_drag());
        let holding = enabled && response.is_pointer_button_down_on();
        let progress = self.simple_delete_hold_progress(holding);
        let caption = i18n::registered(self.locale, "desktop-chrome.006");
        self.paint_tool_icon(
            ui,
            rect,
            &response,
            PhoneIcon::Trash,
            caption,
            false,
            false,
            enabled,
        );
        response.clone().widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, caption.to_owned())
        });
        if progress > 0.0 {
            let painter = ui.painter();
            let center = rect.center();
            let radius = 16.0;
            let steps = ((progress * 24.0).ceil() as usize).max(2);
            let mut points = Vec::with_capacity(steps + 1);
            for i in 0..=steps {
                let t = progress * (i as f32 / steps as f32);
                let angle = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * t;
                points.push(center + egui::Vec2::angled(angle) * radius);
            }
            painter.add(egui::Shape::line(
                points,
                Stroke::new(3.0, Color32::from_rgb(196, 92, 42)),
            ));
        }
        response.on_hover_text(tip);
        if holding {
            ui.ctx().request_repaint_after(Duration::from_millis(16));
        }
        if enabled && progress >= 1.0 {
            self.simple_commit_held_delete();
        }
    }

    fn icon_status(&mut self, ui: &mut egui::Ui) {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let switch = if self.simple_mode.enabled {
                text(self.locale, Text::DetailedMode)
            } else {
                text(self.locale, Text::SimpleMode)
            };
            if ui.button(egui::RichText::new(switch).size(13.0)).clicked() {
                self.set_simple_mode(!self.simple_mode.enabled);
            }
            let (state, color) = if self.project_state.is_saving() {
                (
                    text(self.locale, Text::Saved),
                    Color32::from_rgb(20, 96, 200),
                )
            } else if self.dirty {
                (
                    text(self.locale, Text::Unsaved),
                    Color32::from_rgb(176, 96, 20),
                )
            } else {
                (
                    text(self.locale, Text::Saved),
                    Color32::from_rgb(46, 158, 92),
                )
            };
            ui.colored_label(color, egui::RichText::new(state).size(13.0));
            if !self.document.performers.is_empty() {
                ui.label(
                    egui::RichText::new(super::i18n::roster_count(
                        self.locale,
                        self.document.performers.len(),
                    ))
                    .size(13.0),
                );
            }
        });
    }

    fn icon_gap(&self, ui: &mut egui::Ui) {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, ICON_H), Sense::hover());
        let x = rect.center().x;
        ui.painter().vline(
            x,
            rect.shrink(10.0).y_range(),
            Stroke::new(1.0, super::app_theme::HAIRLINE),
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn tool_icon(
        &self,
        ui: &mut egui::Ui,
        icon: PhoneIcon,
        label: &str,
        tip: &str,
        enabled: bool,
        filled: bool,
        pulse: bool,
    ) -> bool {
        self.tool_icon_rect(ui, icon, label, tip, enabled, filled, pulse)
            .1
    }

    #[allow(clippy::too_many_arguments)]
    fn tool_icon_rect(
        &self,
        ui: &mut egui::Ui,
        icon: PhoneIcon,
        label: &str,
        tip: &str,
        enabled: bool,
        filled: bool,
        pulse: bool,
    ) -> (egui::Rect, bool) {
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(egui::vec2(ICON_W, ICON_H), sense);
        self.paint_tool_icon(ui, rect, &response, icon, label, filled, pulse, enabled);
        response.clone().widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label.to_owned())
        });
        response.clone().on_hover_text(tip);
        (rect, enabled && response.clicked())
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_tool_icon(
        &self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        response: &egui::Response,
        icon: PhoneIcon,
        label: &str,
        filled: bool,
        pulse: bool,
        enabled: bool,
    ) {
        let simple = self.simple_mode.enabled;
        let hovered = enabled && response.hovered();
        let fill = if filled {
            if hovered {
                Color32::from_rgb(16, 82, 176)
            } else if enabled {
                SIMPLE_BLUE
            } else {
                SIMPLE_BLUE.gamma_multiply(0.45)
            }
        } else if hovered {
            if simple {
                super::app_theme::ACCENT_SOFT
            } else {
                ui.visuals().widgets.hovered.bg_fill
            }
        } else if simple {
            Color32::WHITE
        } else {
            ui.visuals().widgets.inactive.bg_fill
        };
        ui.painter().rect_filled(rect, 8.0, fill);
        let stroke = if pulse {
            Stroke::new(2.0, SIMPLE_BLUE)
        } else {
            Stroke::new(1.0, super::app_theme::HAIRLINE)
        };
        ui.painter()
            .rect_stroke(rect, 8.0, stroke, StrokeKind::Inside);
        let icon_color = if filled {
            Color32::WHITE
        } else if enabled {
            if simple {
                SIMPLE_BLUE
            } else {
                ui.visuals().text_color()
            }
        } else {
            ui.visuals().weak_text_color()
        };
        let ink = if filled {
            Color32::WHITE
        } else if enabled {
            if simple {
                SIMPLE_INK
            } else {
                ui.visuals().text_color()
            }
        } else {
            ui.visuals().weak_text_color()
        };
        paint_phone_icon_at(
            ui.painter(),
            rect.center() + egui::vec2(0.0, -7.0),
            icon,
            icon_color,
            0.95,
        );
        let previous_clip = ui.clip_rect();
        ui.set_clip_rect(previous_clip.intersect(rect.shrink(2.0)));
        ui.painter().text(
            rect.center() + egui::vec2(0.0, 12.0),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(11.0),
            ink,
        );
        ui.set_clip_rect(previous_clip);
    }

    pub(crate) fn edit_plain_menu(&mut self, ui: &mut egui::Ui) {
        self.menu_heading(ui, i18n::registered(self.locale, "desktop-chrome.007"));
        self.plain_item(
            ui,
            "simple-mode.130",
            "simple-mode.140",
            PlainAction::Everyone,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.234",
            "simple-mode.235",
            PlainAction::PrevPerson,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.232",
            "simple-mode.233",
            PlainAction::NextPerson,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.305",
            "simple-mode.306",
            PlainAction::FocusMove,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.159",
            "simple-mode.160",
            PlainAction::AddBeside,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.211",
            "simple-mode.212",
            PlainAction::SwapPair,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.175",
            "simple-mode.176",
            PlainAction::Renumber,
            None,
        );
        ui.separator();
        self.menu_heading(ui, i18n::registered(self.locale, "desktop-chrome.008"));
        self.plain_item(
            ui,
            "simple-mode.154",
            "simple-mode.158",
            PlainAction::NudgeForward,
            Some(Action::NudgeUp),
        );
        self.plain_item(
            ui,
            "simple-mode.155",
            "simple-mode.158",
            PlainAction::NudgeBack,
            Some(Action::NudgeDown),
        );
        self.plain_item(
            ui,
            "simple-mode.156",
            "simple-mode.158",
            PlainAction::NudgeLeft,
            Some(Action::NudgeLeft),
        );
        self.plain_item(
            ui,
            "simple-mode.157",
            "simple-mode.158",
            PlainAction::NudgeRight,
            Some(Action::NudgeRight),
        );
        ui.separator();
        self.plain_item(
            ui,
            "simple-mode.213",
            "simple-mode.215",
            PlainAction::Restore,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.164",
            "simple-mode.165",
            PlainAction::ClearMove,
            None,
        );
    }

    pub(crate) fn arrange_plain_menu(&mut self, ui: &mut egui::Ui) {
        self.menu_heading(ui, i18n::registered(self.locale, "desktop-chrome.009"));
        self.plain_item(
            ui,
            "simple-mode.132",
            "simple-mode.141",
            PlainAction::Line,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.148",
            "simple-mode.149",
            PlainAction::File,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.150",
            "simple-mode.151",
            PlainAction::Circle,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.216",
            "simple-mode.217",
            PlainAction::Block,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.197",
            "simple-mode.198",
            PlainAction::Arc,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.254",
            "simple-mode.255",
            PlainAction::Chevron,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.248",
            "simple-mode.249",
            PlainAction::Diagonal,
            None,
        );
        ui.separator();
        self.menu_heading(ui, i18n::registered(self.locale, "desktop-chrome.010"));
        self.plain_item(
            ui,
            "simple-mode.152",
            "simple-mode.153",
            PlainAction::SwapSides,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.193",
            "simple-mode.194",
            PlainAction::SwapEnds,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.195",
            "simple-mode.196",
            PlainAction::Turn,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.199",
            "simple-mode.200",
            PlainAction::Spread,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.201",
            "simple-mode.202",
            PlainAction::Tighten,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.244",
            "simple-mode.245",
            PlainAction::Center,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.246",
            "simple-mode.247",
            PlainAction::DressFront,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.250",
            "simple-mode.251",
            PlainAction::DressBack,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.252",
            "simple-mode.253",
            PlainAction::EvenGaps,
            None,
        );
        ui.separator();
        ui.menu_button(i18n::registered(self.locale, "desktop-chrome.011"), |ui| {
            self.bounds_item(ui, "workspace-inspector.080", BoundShape::Horizontal);
            self.bounds_item(ui, "workspace-inspector.081", BoundShape::Vertical);
            self.bounds_item(ui, "workspace-inspector.082", BoundShape::Diagonal);
            self.bounds_item(ui, "workspace-inspector.083", BoundShape::Arc);
            self.bounds_item(ui, "workspace-inspector.084", BoundShape::Circle);
            self.bounds_item(ui, "workspace-inspector.085", BoundShape::Block);
            self.bounds_item(ui, "workspace-inspector.086", BoundShape::Spiral);
            ui.separator();
            self.menu_heading(ui, i18n::registered(self.locale, "desktop-chrome.012"));
            self.fit_item(ui, "workspace-inspector.038", true);
            self.fit_item(ui, "workspace-inspector.039", false);
            if ui
                .add_enabled(
                    !self.selected.is_empty() && self.is_editable_set_start(),
                    egui::Button::new(text(self.locale, Text::OnGrid)),
                )
                .clicked()
            {
                self.snap_selected_to_grid();
                ui.close();
            }
            if ui
                .add_enabled(
                    !self.selected.is_empty() && self.is_editable_set_start(),
                    egui::Button::new(text(self.locale, Text::FlipCenterLine)),
                )
                .clicked()
            {
                self.mirror_selection_across_field();
                ui.close();
            }
        });
        ui.menu_button(i18n::registered(self.locale, "desktop-chrome.013"), |ui| {
            ui.label(i18n::registered(self.locale, "desktop-chrome.032"));
            ui.add(
                egui::TextEdit::singleline(&mut self.formation_text)
                    .char_limit(32)
                    .desired_width(180.0)
                    .hint_text(i18n::registered(self.locale, "desktop-chrome.033")),
            );
            let ready = !self.formation_text.trim().is_empty() && !self.selected.is_empty();
            if ui
                .add_enabled(
                    ready,
                    egui::Button::new(i18n::registered(self.locale, "desktop-chrome.034")),
                )
                .clicked()
            {
                self.preview_formation_text();
                if self.formation_preview_spec.is_some() {
                    self.apply_shape_preview();
                }
                ui.close();
            }
        });
        if ui
            .button(i18n::registered(self.locale, "desktop-chrome.014"))
            .clicked()
        {
            if self.simple_mode.enabled {
                self.set_simple_mode(false);
            }
            self.show_inspector = true;
            self.full_inspector_collapsed = false;
            self.workspace_focus = Some(WorkspaceFocus::Performer);
            ui.close();
        }
    }

    pub(crate) fn scene_plain_menu(&mut self, ui: &mut egui::Ui) {
        self.scene_add_item(ui);
        self.plain_item(
            ui,
            "simple-mode.268",
            "simple-mode.269",
            PlainAction::ShiftEarlier,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.270",
            "simple-mode.271",
            PlainAction::ShiftLater,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.101",
            "simple-mode.101",
            PlainAction::DeleteScene,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.259",
            "simple-mode.260",
            PlainAction::ReturnScene,
            None,
        );
        ui.separator();
        let counts = self
            .document
            .sets
            .get(self.current_set)
            .map(|set| set.counts)
            .unwrap_or(0);
        ui.label(
            egui::RichText::new(format!(
                "{counts}{}",
                i18n::registered(self.locale, "desktop-chrome.035")
            ))
            .size(13.0),
        );
        self.plain_item(
            ui,
            "simple-mode.113",
            "simple-mode.113",
            PlainAction::FewerCounts,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.114",
            "simple-mode.114",
            PlainAction::MoreCounts,
            None,
        );
        ui.separator();
        self.simple_scene_name_row(ui);
    }

    fn scene_add_item(&mut self, ui: &mut egui::Ui) {
        let label = i18n::registered(self.locale, "desktop-chrome.036");
        let shortcut = self
            .keybindings
            .map
            .primary_text(self.locale, Action::Command(UiCommand::DuplicateSet));
        let text = if shortcut.is_empty() {
            label.to_owned()
        } else {
            format!("{label}    {shortcut}")
        };
        let enabled =
            !self.playing && !self.document.performers.is_empty() && self.is_editable_set_start();
        if ui.add_enabled(enabled, egui::Button::new(text)).clicked() {
            if self.simple_mode.enabled {
                self.simple_press_add_scene();
            } else {
                self.execute_command(UiCommand::DuplicateSet, ui.ctx());
            }
            ui.close();
        }
    }

    pub(crate) fn playback_plain_menu(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        self.plain_item(
            ui,
            "simple-mode.133",
            "simple-mode.142",
            PlainAction::StepBack,
            Some(Action::PreviousCount),
        );
        self.plain_item(
            ui,
            "simple-mode.134",
            "simple-mode.143",
            PlainAction::StepForward,
            Some(Action::NextCount),
        );
        self.plain_item(
            ui,
            "simple-mode.135",
            "simple-mode.144",
            PlainAction::SceneStart,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.137",
            "simple-mode.145",
            PlainAction::Slow,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.236",
            "simple-mode.237",
            PlainAction::SceneLoop,
            None,
        );
        ui.separator();
        self.menu_heading(ui, i18n::registered(self.locale, "desktop-chrome.037"));
        self.plain_item(
            ui,
            "simple-mode.220",
            "simple-mode.220",
            PlainAction::TempoDown,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.221",
            "simple-mode.221",
            PlainAction::TempoUp,
            None,
        );
        ui.separator();
        self.plain_item(
            ui,
            "simple-mode.340",
            "simple-mode.341",
            PlainAction::ImportSong,
            Some(Action::ImportSong),
        );
        self.plain_item(
            ui,
            "simple-mode.242",
            "simple-mode.243",
            PlainAction::SelectMovers,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.257",
            "simple-mode.258",
            PlainAction::SelectHolders,
            None,
        );
    }

    pub(crate) fn handout_plain_menu(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        self.menu_heading(ui, i18n::registered(self.locale, "desktop-chrome.015"));
        self.plain_item(
            ui,
            "simple-mode.177",
            "simple-mode.178",
            PlainAction::Memo,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.226",
            "simple-mode.227",
            PlainAction::PlaceSheet,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.274",
            "simple-mode.275",
            PlainAction::ShowSheet,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.291",
            "simple-mode.292",
            PlainAction::WalkSheet,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.280",
            "simple-mode.281",
            PlainAction::PathSheet,
            None,
        );
        ui.separator();
        self.plain_item(
            ui,
            "simple-mode.278",
            "simple-mode.279",
            PlainAction::FocusPerson,
            None,
        );
        self.plain_item(
            ui,
            "simple-mode.287",
            "simple-mode.288",
            PlainAction::Away,
            None,
        );
    }

    pub(crate) fn view_plain_menu(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        let mut toolbar = self.icon_toolbar_shown();
        if ui
            .checkbox(&mut toolbar, self.toolbar_toggle_label())
            .changed()
        {
            if self.simple_mode.enabled {
                self.onboarding.simple_toolbar_collapsed = !toolbar;
            } else {
                self.full_toolbar_collapsed = !toolbar;
            }
        }
        let mut mirror = self.mirror_moves_on();
        if ui
            .checkbox(&mut mirror, text(self.locale, Text::Symmetry))
            .changed()
        {
            self.simple_toggle_mirror();
        }
        let mut ghosts = self.previous_ghost_shown();
        if ui
            .add_enabled(
                self.current_set > 0,
                egui::Checkbox::new(&mut ghosts, text(self.locale, Text::PreviousSet)),
            )
            .changed()
        {
            self.toggle_previous_ghost();
        }
        if let Some(gpu) = &mut self.gpu {
            let mut enabled = gpu.enabled();
            if ui
                .checkbox(&mut enabled, "GPU")
                .on_hover_text(i18n::registered(self.locale, "desktop-chrome.038"))
                .changed()
            {
                gpu.set_enabled(enabled);
            }
        }
        if !self.collab.is_live() {
            ui.menu_button(i18n::registered(self.locale, "desktop-chrome.016"), |ui| {
                for (id, kind) in [
                    ("app-ui.033", CameraPreset::Audience),
                    ("app-ui.034", CameraPreset::PressBox),
                    ("app-ui.035", CameraPreset::Overhead),
                    ("app-ui.036", CameraPreset::EndZone),
                ] {
                    if ui.button(i18n::registered(self.locale, id)).clicked() {
                        self.view_mode = ViewMode::Stadium3D;
                        self.camera = match kind {
                            CameraPreset::Audience => Camera::audience_view(&self.document.grid),
                            CameraPreset::PressBox => Camera::press_box(&self.document.grid),
                            CameraPreset::Overhead => Camera::overhead(&self.document.grid),
                            CameraPreset::EndZone => Camera::end_zone(&self.document.grid, true),
                        };
                        self.camera_program_preview = false;
                        ui.close();
                    }
                }
            });
        }
        if self.simple_active_hint().is_some() {
            self.plain_item(
                ui,
                "simple-mode.313",
                "simple-mode.313",
                PlainAction::DismissHint,
                None,
            );
        }
    }

    fn bounds_item(&mut self, ui: &mut egui::Ui, id: &str, kind: BoundShape) {
        let enabled = self.selected.len() >= 2 && self.is_editable_set_start() && !self.playing;
        if ui
            .add_enabled(
                enabled,
                egui::Button::new(i18n::registered(self.locale, id)),
            )
            .clicked()
        {
            self.apply_bound_shape(kind);
            ui.close();
        }
    }

    fn apply_bound_shape(&mut self, kind: BoundShape) {
        let Some((min, max)) = self.selection_bounds() else {
            return;
        };
        let count = self.selected.len();
        if count < 2 {
            return;
        }
        let center = Point {
            x: (min.x + max.x) * 0.5,
            y: (min.y + max.y) * 0.5,
        };
        match kind {
            BoundShape::Horizontal => {
                let y = center.y;
                self.commit_layout(evenly_spaced_line(
                    Point { x: min.x, y },
                    Point { x: max.x, y },
                    count,
                ));
            }
            BoundShape::Vertical => {
                let x = center.x;
                self.commit_layout(evenly_spaced_line(
                    Point { x, y: min.y },
                    Point { x, y: max.y },
                    count,
                ));
            }
            BoundShape::Diagonal => {
                self.commit_layout(evenly_spaced_line(min, max, count));
            }
            BoundShape::Arc => self.arrange_selection_arc(),
            BoundShape::Circle => self.arrange_selection_circle(),
            BoundShape::Block => self.arrange_selection_block(),
            BoundShape::Spiral => {
                let radius = ((max.x - min.x) * 0.5).max((max.y - min.y) * 0.5).max(3.0);
                self.commit_layout(shapes::spiral(center, radius * 0.15, radius, 2.0, count));
            }
        }
    }

    fn fit_item(&mut self, ui: &mut egui::Ui, id: &str, line: bool) {
        let enabled = self.selected.len() >= 3 && self.is_editable_set_start() && !self.playing;
        if ui
            .add_enabled(
                enabled,
                egui::Button::new(i18n::registered(self.locale, id)),
            )
            .clicked()
        {
            let points = self.selected_points();
            if line {
                if let Some(fit) = shapes::fit_line(&points) {
                    self.commit_layout(shapes::snap_to_line(&points, &fit));
                }
            } else if let Some(fit) = shapes::fit_circle(&points) {
                self.commit_layout(shapes::snap_to_circle(&points, &fit));
            }
            ui.close();
        }
    }

    fn plain_item(
        &mut self,
        ui: &mut egui::Ui,
        id: &str,
        tip: &str,
        action: PlainAction,
        shortcut: Option<Action>,
    ) {
        let enabled = self.plain_action_enabled(action);
        let label = i18n::registered(self.locale, id);
        let shortcut = shortcut
            .map(|action| self.keybindings.map.primary_text(self.locale, action))
            .unwrap_or_default();
        let text = if shortcut.is_empty() {
            label.to_owned()
        } else {
            format!("{label}    {shortcut}")
        };
        let response = ui.add_enabled(enabled, egui::Button::new(text));
        let response = if enabled {
            response.on_hover_text(i18n::registered(self.locale, tip))
        } else {
            response.on_disabled_hover_text(i18n::registered(self.locale, tip))
        };
        if response.clicked() {
            self.run_plain_action(action);
            ui.close();
        }
    }

    fn menu_heading(&self, ui: &mut egui::Ui, title: &str) {
        ui.label(
            egui::RichText::new(title)
                .size(12.0)
                .color(ui.visuals().weak_text_color()),
        );
    }
}

#[derive(Clone, Copy)]
enum BoundShape {
    Horizontal,
    Vertical,
    Diagonal,
    Arc,
    Circle,
    Block,
    Spiral,
}

#[derive(Clone, Copy)]
enum CameraPreset {
    Audience,
    PressBox,
    Overhead,
    EndZone,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_top_bar_names_the_menus_and_keeps_a_short_icon_row() {
        let mut app = DrillApp::default();
        app.onboarding.show_welcome = false;
        app.simple_mode.enabled = true;
        app.begin_simple_show();
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        };
        let output = ctx.run_ui(raw, |ui| {
            app.show_desktop_chrome(ui);
        });
        let tree = output
            .platform_output
            .accesskit_update
            .expect("the menu bar is part of the accessible tree");
        let labels: Vec<String> = tree
            .nodes
            .iter()
            .filter_map(|(_, node)| node.label().map(|label| label.to_string()))
            .collect();
        for word in [
            "ファイル",
            "編集",
            "並べる",
            "場面",
            "再生",
            "表示",
            "書き出し",
            "ヘルプ",
            "戻す",
        ] {
            assert!(
                labels.iter().any(|label| label.contains(word)),
                "{word} is missing from {labels:?}"
            );
        }
        assert!(
            app.desktop_chrome_px > 40.0 && app.desktop_chrome_px < 220.0,
            "menu, icons, and scene tabs should stay in a short band, got {}",
            app.desktop_chrome_px
        );
    }
}
