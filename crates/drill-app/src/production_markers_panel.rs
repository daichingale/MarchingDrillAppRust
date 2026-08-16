//! A compact, keyboard-friendly production-marker browser.
//!
//! This intentionally keeps text edits in an in-memory draft. A marker is
//! committed only from Save, keeping the undo stack meaningful while a writer
//! names a rehearsal event.

use super::*;
use drill_core::{Edit, ProductionMarker, ProductionMarkerId, ProductionMarkerKind};

#[derive(Default)]
pub(super) struct ProductionMarkersPanel {
    filter: String,
    selected: Option<ProductionMarkerId>,
    range_start: Option<ProductionMarkerId>,
    draft: Option<ProductionMarker>,
}

impl ProductionMarkersPanel {
    fn kind_name(kind: ProductionMarkerKind, locale: Locale) -> &'static str {
        match kind {
            ProductionMarkerKind::Hit => i18n::registered(locale, "production-markers.005"),
            ProductionMarkerKind::Rehearsal => i18n::registered(locale, "production-markers.006"),
            ProductionMarkerKind::Note => i18n::registered(locale, "production-markers.007"),
        }
    }

    fn marker_color(kind: ProductionMarkerKind) -> Color32 {
        match kind {
            ProductionMarkerKind::Hit => Color32::from_rgb(255, 198, 72),
            ProductionMarkerKind::Rehearsal => Color32::from_rgb(126, 216, 172),
            ProductionMarkerKind::Note => Color32::from_rgb(181, 160, 255),
        }
    }

    fn seek(app: &mut DrillApp, count: u32) {
        let (set, local) = app.document.locate_count(count as f32);
        app.current_set = set;
        app.count_position = local;
        app.playing = false;
        app.audio_state.pause();
        if let Some(track) = &app.document.audio {
            app.audio_state
                .seek_seconds(drill_core::audio::count_to_audio_time(
                    track,
                    &app.document.tempo,
                    count as f32,
                ));
        }
    }

    pub(super) fn show(&mut self, app: &mut DrillApp, ui: &mut egui::Ui) {
        ui.separator();
        ui.heading(i18n::registered(app.locale, "production-markers.001"));
        ui.small(i18n::registered(app.locale, "production-markers.002"));
        // Rehearsal navigation needs to be available without first finding a
        // row in a long marker list. These mirror the menu/palette commands
        // and deliberately seek whole counts, just like the shortcuts.
        let current_count = app
            .document
            .global_count(app.current_set, app.count_position)
            .round()
            .clamp(0.0, app.document.timeline_counts() as f32) as u32;
        let has_previous = app
            .document
            .production_markers
            .iter()
            .any(|marker| marker.count < current_count);
        let has_next = app
            .document
            .production_markers
            .iter()
            .any(|marker| marker.count > current_count);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    has_previous,
                    egui::Button::new(i18n::registered(app.locale, "production-markers.021")),
                )
                .on_hover_text(i18n::registered(app.locale, "production-markers.022"))
                .clicked()
            {
                app.navigate_production_marker(false);
            }
            if ui
                .add_enabled(
                    has_next,
                    egui::Button::new(i18n::registered(app.locale, "production-markers.023")),
                )
                .on_hover_text(i18n::registered(app.locale, "production-markers.024"))
                .clicked()
            {
                app.navigate_production_marker(true);
            }
            if let Some(marker) = app
                .document
                .production_markers
                .iter()
                .find(|marker| marker.count == current_count)
            {
                ui.small(format!(
                    "{} {} · {}",
                    i18n::registered(app.locale, "production-markers.025"),
                    marker.count,
                    marker.label
                ));
            }
        });
        let filter_response = ui.add(
            egui::TextEdit::singleline(&mut self.filter)
                .hint_text(i18n::registered(app.locale, "production-markers.003")),
        );
        filter_response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::TextEdit,
                true,
                i18n::registered(app.locale, "production-markers.004"),
            )
        });

        let needle = self.filter.trim().to_lowercase();
        // Own the display rows so a click can seek (and therefore mutably
        // touch the app) without holding an immutable document borrow.
        let mut visible = app.document.production_markers.clone();
        visible.sort_by_key(|marker| marker.count);
        visible.retain(|marker| {
            needle.is_empty()
                || marker.label.to_lowercase().contains(&needle)
                || marker.detail.to_lowercase().contains(&needle)
                || Self::kind_name(marker.kind, app.locale)
                    .to_lowercase()
                    .contains(&needle)
                || marker.count.to_string().contains(&needle)
        });

        if visible.is_empty() {
            ui.small(i18n::registered(app.locale, "production-markers.008"));
        } else {
            let mut seek_to = None;
            egui::ScrollArea::vertical()
                .id_salt("production-marker-list")
                .max_height(180.0)
                .show(ui, |ui| {
                    for marker in &visible {
                        let label = format!(
                            "{} · {} {}",
                            marker.count,
                            Self::kind_name(marker.kind, app.locale),
                            marker.label
                        );
                        let response = ui
                            .selectable_label(self.selected == Some(marker.id), label)
                            .on_hover_text(if marker.detail.trim().is_empty() {
                                i18n::registered(app.locale, "production-markers.009")
                            } else {
                                marker.detail.as_str()
                            });
                        if response.clicked() {
                            self.selected = Some(marker.id);
                            self.draft = Some(marker.clone());
                            seek_to = Some(marker.count);
                        }
                        if self.range_start == Some(marker.id) {
                            ui.painter().rect_stroke(
                                response.rect.expand2(egui::vec2(1.0, 0.0)),
                                3.0,
                                egui::Stroke::new(1.0, Self::marker_color(marker.kind)),
                                egui::StrokeKind::Outside,
                            );
                        }
                        let dot = egui::Rect::from_center_size(
                            response.rect.left_center() + egui::vec2(5.0, 0.0),
                            egui::vec2(5.0, 5.0),
                        );
                        ui.painter().circle_filled(
                            dot.center(),
                            2.5,
                            Self::marker_color(marker.kind),
                        );
                    }
                });
            if let Some(count) = seek_to {
                Self::seek(app, count);
            }
        }

        let selected = self.selected.and_then(|id| {
            app.document
                .production_markers
                .iter()
                .find(|marker| marker.id == id)
                .cloned()
        });
        if self.selected.is_some() && selected.is_none() {
            self.selected = None;
            self.draft = None;
        }
        let Some(selected) = selected else { return };

        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .button(i18n::registered(app.locale, "production-markers.010"))
                .clicked()
            {
                self.range_start = Some(selected.id);
            }
            if ui
                .add_enabled(
                    self.range_start.is_some(),
                    egui::Button::new(i18n::registered(app.locale, "production-markers.026")),
                )
                .clicked()
            {
                self.range_start = None;
                app.status = i18n::registered(app.locale, "production-markers.027").into();
            }
            let range_start = self
                .range_start
                .and_then(|id| app.document.production_markers.iter().find(|m| m.id == id));
            let can_make_range = range_start.is_some_and(|start| start.count < selected.count);
            let set_range = ui
                .add_enabled(
                    can_make_range,
                    egui::Button::new(i18n::registered(app.locale, "production-markers.011")),
                )
                .on_disabled_hover_text(i18n::registered(app.locale, "production-markers.012"))
                .clicked();
            let applied_start = set_range.then_some(range_start).flatten();
            if let Some(start) = applied_start {
                app.playback_start = start.count;
                app.playback_end = selected.count;
                app.timeline_view = TimelineViewport {
                    start: start.count as f32,
                    span: selected.count.saturating_sub(start.count).max(1) as f32,
                };
                app.timeline_view.normalize(app.document.timeline_counts());
                app.status = i18n::registered(app.locale, "production-markers.013").into();
            }
        });
        if let Some(start) = self
            .range_start
            .and_then(|id| app.document.production_markers.iter().find(|m| m.id == id))
        {
            ui.small(format!(
                "{} {}",
                i18n::registered(app.locale, "production-markers.014"),
                start.count
            ));
        }

        let mut draft = self.draft.clone().unwrap_or_else(|| selected.clone());
        if draft.id != selected.id {
            draft = selected.clone();
        }
        ui.label(i18n::registered(app.locale, "production-markers.015"));
        ui.horizontal(|ui| {
            for kind in [
                ProductionMarkerKind::Hit,
                ProductionMarkerKind::Rehearsal,
                ProductionMarkerKind::Note,
            ] {
                if ui
                    .selectable_label(draft.kind == kind, Self::kind_name(kind, app.locale))
                    .clicked()
                {
                    draft.kind = kind;
                }
            }
        });
        ui.label(i18n::registered(app.locale, "production-markers.016"));
        ui.text_edit_singleline(&mut draft.label);
        ui.label(i18n::registered(app.locale, "production-markers.017"));
        ui.add(egui::TextEdit::multiline(&mut draft.detail).desired_rows(2));
        let mut save = false;
        let mut cancel = false;
        ui.horizontal(|ui| {
            if ui
                .button(i18n::registered(app.locale, "production-markers.018"))
                .clicked()
            {
                save = true;
            }
            if ui
                .button(i18n::registered(app.locale, "production-markers.019"))
                .clicked()
            {
                cancel = true;
            }
        });
        if save && draft != selected {
            app.execute_edit(
                Edit::SetProductionMarker {
                    marker: draft.clone(),
                },
                i18n::registered(app.locale, "production-markers.020"),
            );
            self.draft = Some(draft);
        } else if cancel {
            self.draft = Some(selected);
        } else {
            self.draft = Some(draft);
        }
    }
}
