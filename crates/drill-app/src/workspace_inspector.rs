//! Editing workspace inspector: audio sync, sets, performers, clinic, grid and tempo.
//!
//! This module owns no document state. Mutations still flow through DrillApp and History.

use super::*;
use drill_core::clinic;
use drill_core::rhythm_sync;

impl DrillApp {
    pub(super) fn show_workspace_inspector(&mut self, ui: &mut egui::Ui, set_counts: f32) {
        let global_count = self
            .document
            .global_count(self.current_set, self.count_position);
        let waveform_action = self.audio_state.show_waveform(
            ui,
            self.locale,
            self.document.audio.as_ref(),
            &self.document.tempo,
            global_count,
            self.timeline_view.visible_range(),
        );
        if let Some(action) = waveform_action {
            let edit = match action {
                audio_state::WaveformAction::Add(anchor) => {
                    Edit::AddSyncAnchor { id: None, anchor }
                }
                audio_state::WaveformAction::Move { id, anchor } => {
                    Edit::MoveSyncAnchor { id, anchor }
                }
                audio_state::WaveformAction::Remove(id) => Edit::RemoveSyncAnchor { id },
            };
            if self.history.execute(&mut self.document, edit).is_ok() {
                self.dirty = true;
            } else {
                self.status =
                    super::i18n::registered(self.locale, "workspace-inspector.102").into();
            }
        }
        if let Some(track) = &self.document.audio {
            let proposal = track.anchors.tempo_proposal(&self.document.tempo);
            if let Some(worst) = proposal
                .residuals
                .iter()
                .max_by(|a, b| a.milliseconds.abs().total_cmp(&b.milliseconds.abs()))
            {
                ui.small(format!(
                    "{} {:+.1} ms",
                    super::i18n::registered(self.locale, "workspace-inspector.074"),
                    worst.milliseconds
                ));
            }
            let mut mismatches = Vec::new();
            track
                .anchors
                .mismatches(&self.document.tempo, &mut mismatches);
            if !mismatches.is_empty() {
                ui.colored_label(
                    Color32::from_rgb(255, 184, 77),
                    format!("⚠ テンポ不一致 {} 区間", mismatches.len()),
                );
            }
            ui.small(super::i18n::registered(
                self.locale,
                "workspace-inspector.075",
            ));
        }
        ui.separator();

        ui.allocate_ui_with_layout(
        Vec2::new(300.0, ui.available_height()),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            egui::ScrollArea::vertical()
                .id_salt("workspace-inspector-scroll")
                .show(ui, |ui| {
            ui.heading(super::i18n::registered(self.locale, "workspace-inspector.001"));
            ui.small(super::i18n::registered(self.locale, "workspace-inspector.002"));
            for (index, set) in self.document.sets.iter().enumerate() {
                let text = format!("{}  ·  {} counts", set.name, set.counts);
                if ui
                    .selectable_label(index == self.current_set, text)
                    .clicked()
                {
                    self.current_set = index;
                    self.count_position = 0.0;
                    self.playing = false;
                    // Performer identity is stable across sets; preserve the
                    // working group while the designer compares or adjusts a
                    // transition.
                }
            }
            if ui.button(super::i18n::registered(self.locale, "workspace-inspector.003")).clicked() {
                self.duplicate_current_set();
            }
            ui.collapsing(super::i18n::registered(self.locale, "count-adjust.001"), |ui| {
                ui.small(super::i18n::registered(self.locale, "count-adjust.002"));
                let set_id = self.document.sets[self.current_set].id;
                let existing_moves = self.document.sets[self.current_set].counts;
                let active_for_set = self
                    .set_count_draft
                    .is_some_and(|draft| draft.set_id == set_id);
                if !active_for_set && ui.button(super::i18n::registered(self.locale, "count-adjust.003")).clicked() {
                    self.begin_set_count_draft();
                }
                if active_for_set {
                    let mut moves = self.set_count_draft.expect("active draft").moves;
                    ui.add(egui::DragValue::new(&mut moves).range(1..=512).suffix(" c"));
                    if let Some(draft) = &mut self.set_count_draft {
                        draft.moves = moves;
                    }
                    let delta = i32::from(moves) - i32::from(existing_moves);
                    ui.label(format!(
                        "{} {:+} c",
                        super::i18n::registered(self.locale, "count-adjust.004"),
                        delta
                    ));
                    ui.small(super::i18n::registered(self.locale, "count-adjust.005"));
                    ui.horizontal(|ui| {
                        if ui.button(super::i18n::registered(self.locale, "count-adjust.006")).clicked() {
                            self.apply_set_count_draft();
                        }
                        if ui.button(super::i18n::registered(self.locale, "count-adjust.007")).clicked() {
                            self.discard_set_count_draft();
                        }
                    });
                }
            });
            // Keep the marker browser close to the set list: both are the
            // writer's fast ways to orient themselves without abandoning the
            // field. Taking the small session state out avoids aliasing the
            // app while it seeks, edits, or adjusts the playback range.
            let mut production_markers_panel = std::mem::take(&mut self.production_markers_panel);
            production_markers_panel.show(self, ui);
            self.production_markers_panel = production_markers_panel;
            ui.collapsing(super::i18n::registered(self.locale, "workspace-inspector.004"), |ui| {
                ui.small(super::i18n::registered(self.locale, "workspace-inspector.005"));
                let set_id = self.document.sets[self.current_set].id;
                let mut value = self.document.sets[self.current_set].annotation.clone();
                let before = value.clone();
                ui.label(super::i18n::registered(self.locale, "workspace-inspector.006"));
                ui.text_edit_singleline(&mut value.title);
                ui.label(super::i18n::registered(self.locale, "workspace-inspector.007"));
                ui.text_edit_singleline(&mut value.rehearsal_mark);
                ui.label(super::i18n::registered(self.locale, "workspace-inspector.008"));
                ui.add(egui::TextEdit::multiline(&mut value.notes).desired_rows(3));
                let mut custom_tempo = value.tempo_bpm.is_some();
                if ui.checkbox(&mut custom_tempo, super::i18n::registered(self.locale, "workspace-inspector.009")).changed() { value.tempo_bpm = custom_tempo.then_some(self.document.tempo.bpm_at(self.document.global_count(self.current_set, 0.0))); }
                if let Some(v) = &mut value.tempo_bpm { ui.add(egui::DragValue::new(v).range(1.0..=999.0).suffix(" BPM")); }
                let mut custom_sync = value.sync_time_seconds.is_some();
                if ui.checkbox(&mut custom_sync, super::i18n::registered(self.locale, "workspace-inspector.010")).changed() { value.sync_time_seconds = custom_sync.then_some(f64::from(self.document.tempo.seconds_at(self.document.global_count(self.current_set, 0.0)))); }
                if let Some(v) = &mut value.sync_time_seconds { ui.add(egui::DragValue::new(v).range(0.0..=86_400.0).speed(0.01).suffix(" s")); }
                let mut custom_duration = value.transition_duration_seconds.is_some();
                if ui.checkbox(&mut custom_duration, super::i18n::registered(self.locale, "workspace-inspector.011")).changed() {
                    let start = self.document.global_count(self.current_set, 0.0);
                    let end = start + f32::from(self.document.sets[self.current_set].counts);
                    let derived = f64::from(self.document.tempo.seconds_at(end) - self.document.tempo.seconds_at(start));
                    value.transition_duration_seconds = custom_duration.then_some(derived.max(0.0));
                }
                if let Some(v) = &mut value.transition_duration_seconds { ui.add(egui::DragValue::new(v).range(0.0..=86_400.0).speed(0.01).suffix(" s")); }
                if value != before {
                    self.execute_edit(Edit::SetAnnotation { set_id, annotation: value }, super::i18n::registered(self.locale, "workspace-inspector.012"));
                }
            });
            ui.separator();
            ui.label(format!(
                "COUNT  {:02} / {:02}",
                self.count_position.round() as u16,
                self.document.sets[self.current_set].counts
            ));
            let mut editor_count = self.count_position.round().clamp(0.0, set_counts) as u32;
            let seek = ui.add(
                egui::Slider::new(&mut editor_count, 0..=set_counts as u32)
                    .show_value(false),
            );
            if seek.changed() {
                self.count_position = editor_count as f32;
            }
            ui.separator();
            ui.heading(super::i18n::registered(self.locale, "workspace-inspector.013"));
            ui.small(super::i18n::registered(self.locale, "workspace-inspector.014"));
            if self.document.performers.is_empty() {
                egui::Frame::new()
                    .fill(ui.visuals().faint_bg_color)
                    .inner_margin(10)
                    .corner_radius(5)
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new(super::i18n::registered(self.locale, "workspace-inspector.015")).strong());
                        ui.small(super::i18n::registered(self.locale, "workspace-inspector.016"));
                        if ui.button(super::i18n::registered(self.locale, "workspace-inspector.017")).clicked()
                            && self.execute_edit(
                                Edit::ReplaceDocument { document: Box::new(Document::demo(8, 10)) },
                                super::i18n::registered(self.locale, "workspace-inspector.018"),
                            )
                        {
                            self.current_set = 0;
                            self.count_position = 0.0;
                            self.playback_start = 0;
                            self.playback_end = self.document.timeline_counts();
                            self.reset_selection_for_document();
                            self.status = super::i18n::registered(self.locale, "workspace-inspector.019").into();
                        }
                    });
            }
            ui.horizontal(|ui| {
                if ui.button(super::i18n::registered(self.locale, "workspace-inspector.020")).clicked() {
                    self.replace_selection((0..self.document.performers.len()).collect());
                }
                if ui.button(super::i18n::registered(self.locale, "workspace-inspector.021")).clicked() {
                    self.clear_selection();
                }
                if ui
                    .add_enabled(
                        self.can_restore_selection(),
                        egui::Button::new(super::i18n::registered(
                            self.locale,
                            "workspace-inspector.121",
                        )),
                    )
                    .on_hover_text(super::i18n::registered(
                        self.locale,
                        "workspace-inspector.122",
                    ))
                    .clicked()
                {
                    self.restore_recent_selection();
                }
                ui.menu_button(
                    super::i18n::registered(self.locale, "workspace-inspector.123"),
                    |ui| {
                        ui.small(super::i18n::registered(
                            self.locale,
                            "workspace-inspector.124",
                        ));
                        let entries: Vec<_> = self
                            .selection_stack
                            .iter()
                            .rev()
                            .cloned()
                            .collect();
                        for (recency, selection) in entries.into_iter().enumerate() {
                            let label = self.selection_history_label(&selection);
                            if ui.button(label).clicked() {
                                self.restore_selection_history_at(recency);
                                ui.close();
                            }
                        }
                    },
                );
                if ui.button(super::i18n::registered(self.locale, "workspace-inspector.076")).on_hover_text(super::i18n::registered(self.locale, "workspace-inspector.077")).clicked() {
                    self.section_manager.open = true;
                }
            });
            if !self.locked_performers.is_empty() || !self.hidden_performers.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!(
                        "{} {} · {} {}",
                        self.locked_performers.len(),
                        super::i18n::registered(self.locale, "app-ui.133"),
                        self.hidden_performers.len(),
                        super::i18n::registered(self.locale, "app-ui.134"),
                    ));
                    if ui
                        .small_button(super::i18n::registered(self.locale, "app-ui.135"))
                        .on_hover_text(super::i18n::registered(self.locale, "app-ui.136"))
                        .clicked()
                    {
                        self.clear_performer_filters();
                    }
                });
                let filtered_sections: Vec<_> = self
                    .document
                    .sections
                    .iter()
                    .filter_map(|section| {
                        let count = self
                            .document
                            .performers
                            .iter()
                            .filter(|performer| {
                                performer.section == section.id
                                    && (self.locked_performers.contains(&performer.id)
                                        || self.hidden_performers.contains(&performer.id))
                            })
                            .count();
                        (count > 0).then(|| (section.id, section.name.clone(), count))
                    })
                    .collect();
                let filtered_performers: Vec<_> = self
                    .document
                    .performers
                    .iter()
                    .filter_map(|performer| {
                        let locked = self.locked_performers.contains(&performer.id);
                        let hidden = self.hidden_performers.contains(&performer.id);
                        (locked || hidden).then(|| {
                            (
                                performer.id,
                                performer.label.clone(),
                                locked,
                                hidden,
                            )
                        })
                    })
                    .collect();
                ui.collapsing(
                    super::i18n::registered(self.locale, "app-ui.137"),
                    |ui| {
                        ui.small(super::i18n::registered(self.locale, "app-ui.138"));
                        ui.horizontal_wrapped(|ui| {
                            for (section, name, count) in &filtered_sections {
                                let label = format!("{name} ({count})");
                                if ui
                                    .small_button(label)
                                    .on_hover_text(super::i18n::registered(
                                        self.locale,
                                        "app-ui.139",
                                    ))
                                    .clicked()
                                {
                                    let restored = self.restore_filtered_section(*section);
                                    self.status = format!(
                                        "{} {restored}",
                                        super::i18n::registered(self.locale, "app-ui.140")
                                    );
                                }
                            }
                        });
                        ui.separator();
                        for (id, label, locked, hidden) in filtered_performers {
                            ui.horizontal(|ui| {
                                let state = match (locked, hidden) {
                                    (true, true) => super::i18n::registered(self.locale, "app-ui.143"),
                                    (true, false) => super::i18n::registered(self.locale, "app-ui.145"),
                                    (false, true) => super::i18n::registered(self.locale, "app-ui.146"),
                                    (false, false) => unreachable!("filtered performer must have a filter"),
                                };
                                ui.label(format!("{label} · {state}"));
                                if ui
                                    .small_button(super::i18n::registered(self.locale, "app-ui.141"))
                                    .on_hover_text(super::i18n::registered(
                                        self.locale,
                                        "app-ui.142",
                                    ))
                                    .clicked()
                                {
                                    self.restore_filtered_performer(id);
                                    self.status = super::i18n::registered(self.locale, "app-ui.144")
                                        .to_owned();
                                }
                            });
                        }
                    },
                );
            }
            if self.count_position != 0.0 {
                ui.colored_label(
                    Color32::from_rgb(255, 184, 77),
                    super::i18n::registered(self.locale, "workspace-inspector.078"),
                );
            }
            ui.separator();
            ui.heading(if self.selected.is_empty() {
                super::i18n::registered(self.locale, "workspace-inspector.103").to_owned()
            } else {
                format!(
                    "{} {}",
                    super::i18n::registered(self.locale, "workspace-inspector.104"),
                    self.selected.len()
                )
            });
            if !self.selected.is_empty() {
                if self.workspace_focus == Some(WorkspaceFocus::Performer) {
                    ui.scroll_to_cursor(Some(egui::Align::Center));
                    self.workspace_focus = None;
                }
                if let Some(&first) = self.selected.iter().next() {
                    let pos = self.document.sets[self.current_set].positions[first];
                    ui.label(
                        egui::RichText::new(format!(
                            "{} {}",
                            self.document.performers[first].label
                            ,super::i18n::registered(self.locale, "workspace-inspector.105")
                        ))
                        .strong(),
                    );
                    ui.small(coordinates::readable_localized(pos, &self.document.grid, self.locale));
                    let segments =
                        continuity::performer_continuity(&self.document, first);
                    if let Some(seg) =
                        segments.iter().find(|s| s.from_set == self.current_set)
                    {
                        ui.small(format!("→ {}", continuity::format_segment(seg, self.locale)));
                    }
                }
                if let Some((min, max)) = self.selection_bounds() {
                    ui.label(egui::RichText::new(super::i18n::registered(self.locale, "workspace-inspector.079")).strong());
                    ui.horizontal_wrapped(|ui| {
                        if ui.small_button(super::i18n::registered(self.locale, "workspace-inspector.080")).clicked() {
                            let y = (min.y + max.y) * 0.5;
                            self.commit_layout(evenly_spaced_line(
                                Point { x: min.x, y },
                                Point { x: max.x, y },
                                self.selected.len(),
                            ));
                        }
                        if ui.small_button(super::i18n::registered(self.locale, "workspace-inspector.081")).clicked() {
                            let x = (min.x + max.x) * 0.5;
                            self.commit_layout(evenly_spaced_line(
                                Point { x, y: min.y },
                                Point { x, y: max.y },
                                self.selected.len(),
                            ));
                        }
                        if ui.small_button(super::i18n::registered(self.locale, "workspace-inspector.082")).clicked() {
                            self.commit_layout(evenly_spaced_line(
                                min,
                                max,
                                self.selected.len(),
                            ));
                        }
                        if ui.small_button(super::i18n::registered(self.locale, "workspace-inspector.083")).clicked() {
                            let center = Point {
                                x: (min.x + max.x) * 0.5,
                                y: max.y,
                            };
                            let radius =
                                ((max.x - min.x) * 0.5).max((max.y - min.y) * 0.5).max(2.5);
                            self.commit_layout(evenly_spaced_arc(
                                center,
                                radius,
                                std::f32::consts::PI,
                                std::f32::consts::TAU,
                                self.selected.len(),
                            ));
                        }
                        if ui.small_button(super::i18n::registered(self.locale, "workspace-inspector.084")).clicked() {
                            let center = Point {
                                x: (min.x + max.x) * 0.5,
                                y: (min.y + max.y) * 0.5,
                            };
                            let radius =
                                ((max.x - min.x) * 0.5).max((max.y - min.y) * 0.5).max(2.5);
                            self.commit_layout(shapes::circle(
                                center,
                                radius,
                                self.selected.len(),
                            ));
                        }
                        if ui.small_button(super::i18n::registered(self.locale, "workspace-inspector.085")).clicked() {
                            let n = self.selected.len();
                            let cols = (n as f32).sqrt().ceil() as usize;
                            let rows = n.div_ceil(cols.max(1));
                            let mut pts = shapes::block_fit(min, max, cols, rows);
                            pts.truncate(n);
                            self.commit_layout(pts);
                        }
                        if ui.small_button(super::i18n::registered(self.locale, "workspace-inspector.086")).clicked() {
                            let center = Point {
                                x: (min.x + max.x) * 0.5,
                                y: (min.y + max.y) * 0.5,
                            };
                            let radius =
                                ((max.x - min.x) * 0.5).max((max.y - min.y) * 0.5).max(3.0);
                            self.commit_layout(shapes::spiral(
                                center,
                                radius * 0.15,
                                radius,
                                2.0,
                                self.selected.len(),
                            ));
                        }
                    });
                    ui.collapsing(
                        super::i18n::registered(self.locale, "workspace-inspector.022"),
                        |ui| {
                            ui.small(super::i18n::registered(self.locale, "workspace-inspector.023"));
                            let center = Point { x: (min.x + max.x) * 0.5, y: (min.y + max.y) * 0.5 };
                            let rx = ((max.x - min.x) * 0.5).max(2.5);
                            let ry = ((max.y - min.y) * 0.5).max(2.5);
                            ui.horizontal_wrapped(|ui| {
                                if ui.button(super::i18n::registered(self.locale, "workspace-inspector.024")).clicked() {
                                    self.preview_shape(shapes::ShapeSpec::Ellipse { center, radius_x: rx, radius_y: ry, rotation: 0.0 });
                                }
                                if ui.button(super::i18n::registered(self.locale, "workspace-inspector.025")).clicked() {
                                    self.begin_free_draw();
                                }
                                let ready = self.formation_preview_spec.is_some();
                                if ui.add_enabled(ready, egui::Button::new(super::i18n::registered(self.locale, "workspace-inspector.026"))).clicked() {
                                    self.apply_shape_preview();
                                }
                                if ui.add_enabled(ready || self.free_draw_active, egui::Button::new(super::i18n::registered(self.locale, "workspace-inspector.027"))).clicked() {
                                    self.cancel_shape_preview();
                                }
                            });
                            ui.horizontal_wrapped(|ui| {
                                ui.label(super::i18n::registered(self.locale, "workspace-inspector.028"));
                                let changed=ui.add(egui::TextEdit::singleline(&mut self.formation_text).char_limit(32).desired_width(180.0).hint_text(super::i18n::registered(self.locale, "workspace-inspector.029"))).changed();
                                if changed || ui.button(super::i18n::registered(self.locale, "workspace-inspector.030")).clicked(){self.preview_formation_text();}
                            });
                            if self.free_draw_active {
                                ui.colored_label(
                                    Color32::from_rgb(100, 220, 255),
                                    super::i18n::registered(self.locale, "workspace-inspector.031"),
                                );
                            }
                            ui.horizontal_wrapped(|ui| {
                                if ui.button(super::i18n::registered(self.locale, "workspace-inspector.111")).clicked() {
                                    self.preview_shape(shapes::ShapeSpec::Ellipse { center, radius_x: rx, radius_y: ry, rotation: 0.0 });
                                }
                                if ui.button(super::i18n::registered(self.locale, "workspace-inspector.112")).clicked() {
                                    self.preview_shape(shapes::ShapeSpec::Parabola { vertex: Point { x: center.x, y: min.y }, curvature: 0.06, half_width: rx, rotation: 0.0 });
                                }
                                if ui.button(super::i18n::registered(self.locale, "workspace-inspector.113")).clicked() {
                                    self.preview_shape(shapes::ShapeSpec::SineWave { start: Point { x: min.x, y: center.y }, end: Point { x: max.x, y: center.y }, amplitude: ry * 0.65, cycles: 2.0, phase: 0.0 });
                                }
                                if ui.button(super::i18n::registered(self.locale, "workspace-inspector.114")).clicked() {
                                    self.preview_shape(shapes::ShapeSpec::Star { center, outer_radius: rx.max(ry), inner_radius: rx.max(ry) * 0.45, points: 5, rotation: 0.0 });
                                }
                                if ui.button(super::i18n::registered(self.locale, "workspace-inspector.115")).clicked() {
                                    self.preview_shape(shapes::ShapeSpec::Polygon { center, radius: rx.max(ry), sides: 6, rotation: 0.0 });
                                }
                                if ui.button(super::i18n::registered(self.locale, "workspace-inspector.116")).clicked() {
                                    self.preview_shape(shapes::ShapeSpec::Cross { center, arm_length: rx.max(ry), arm_width: rx.min(ry) * 0.7 });
                                }
                                if ui.button(super::i18n::registered(self.locale, "workspace-inspector.128")).clicked() {
                                    // Parade block: an evenly spaced rank/file grid on the
                                    // document's own grid step, unlike show blocks (BlockFit)
                                    // which stretch to fit an arbitrary bounding box.
                                    let n = self.selected.len().max(1);
                                    let cols = (n as f32).sqrt().ceil() as usize;
                                    let rows = n.div_ceil(cols.max(1));
                                    let grid = &self.document.grid;
                                    let dx = grid.horizontal_units / grid.horizontal_steps.max(1) as f32;
                                    let dy = grid.vertical_units / grid.vertical_steps.max(1) as f32;
                                    let width = cols.saturating_sub(1) as f32 * dx;
                                    let height = rows.saturating_sub(1) as f32 * dy;
                                    let top_left = Point { x: center.x - width * 0.5, y: center.y - height * 0.5 };
                                    self.preview_shape(shapes::ShapeSpec::Block { top_left, cols, rows, dx, dy });
                                }
                                if ui.button(super::i18n::registered(self.locale, "workspace-inspector.129")).clicked() {
                                    // U-turn: outbound leg along the bounding box's long axis,
                                    // turnaround radius sized from the short axis, starting at
                                    // one end of the box.
                                    let width = (max.x - min.x).max(1.0);
                                    let height = (max.y - min.y).max(1.0);
                                    let along_x = width >= height;
                                    let (direction, leg_length, short_dim) = if along_x {
                                        (0.0, width, height)
                                    } else {
                                        (std::f32::consts::FRAC_PI_2, height, width)
                                    };
                                    let turn_radius = (short_dim * 0.5).max(1.5);
                                    let start = if along_x {
                                        Point { x: min.x, y: center.y }
                                    } else {
                                        Point { x: center.x, y: min.y }
                                    };
                                    self.preview_shape(shapes::ShapeSpec::UTurn {
                                        start,
                                        direction,
                                        leg_length,
                                        turn_radius,
                                        lane_spacing: turn_radius * 2.0,
                                    });
                                }
                            });
                            ui.horizontal_wrapped(|ui| {
                                if ui.button(super::i18n::registered(self.locale, "workspace-inspector.038")).clicked()
                                    && let Some(fit) = shapes::fit_line(&self.selected_points())
                                {
                                    self.commit_layout(shapes::snap_to_line(&self.selected_points(), &fit));
                                }
                                if ui.button(super::i18n::registered(self.locale, "workspace-inspector.039")).clicked()
                                    && let Some(fit) = shapes::fit_circle(&self.selected_points())
                                {
                                    self.commit_layout(shapes::snap_to_circle(&self.selected_points(), &fit));
                                }
                            });
                            ui.separator();
                            ui.small(super::i18n::registered(self.locale, "workspace-inspector.040"));
                            ui.horizontal_wrapped(|ui| {
                                let has_next = self.current_set + 1 < self.document.sets.len();
                                if ui.add_enabled(has_next, egui::Button::new(super::i18n::registered(self.locale, "workspace-inspector.041"))).clicked() {
                                    self.apply_morph_preview(0.25);
                                }
                                if ui.add_enabled(has_next, egui::Button::new(super::i18n::registered(self.locale, "workspace-inspector.042"))).clicked() {
                                    self.apply_morph_preview(0.5);
                                }
                                if ui.button(super::i18n::registered(self.locale, "workspace-inspector.043")).clicked() {
                                    self.apply_radial_selection(2);
                                }
                                if ui.button(super::i18n::registered(self.locale, "workspace-inspector.044")).clicked() {
                                    self.apply_radial_selection(4);
                                }
                                let has_shape = self.document.sets[self.current_set].shape.is_some();
                                if ui.add_enabled(has_shape, egui::Button::new(super::i18n::registered(self.locale, "workspace-inspector.045"))).clicked() {
                                    self.apply_section_shape_assignment();
                                }
                                if ui.button(super::i18n::registered(self.locale, "workspace-inspector.046")).on_hover_text(super::i18n::registered(self.locale, "workspace-inspector.047")).clicked(){self.apply_constraint_cleanup();}
                            });
                            if let Some(spec) = self.document.sets[self.current_set].shape.as_ref() {
                                ui.small(format!("{}: {:?}", super::i18n::registered(self.locale, "workspace-inspector.048"), spec));
                            }
                        },
                    );
                    ui.label(egui::RichText::new(super::i18n::registered(self.locale, "workspace-inspector.087")).strong());
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .small_button("－15°")
                            .on_hover_text(super::i18n::registered(self.locale, "workspace-inspector.125"))
                            .clicked()
                        {
                            self.transform_selection(1.0, -15.0_f32.to_radians());
                        }
                        if ui
                            .small_button("＋15°")
                            .on_hover_text(super::i18n::registered(self.locale, "workspace-inspector.126"))
                            .clicked()
                        {
                            self.transform_selection(1.0, 15.0_f32.to_radians());
                        }
                        if ui.small_button("＋ 10%").clicked() {
                            self.transform_selection(1.1, 0.0);
                        }
                        if ui.small_button("－ 10%").clicked() {
                            self.transform_selection(0.9, 0.0);
                        }
                    });
                    ui.label(egui::RichText::new(super::i18n::registered(self.locale, "workspace-inspector.088")).strong());
                    ui.horizontal_wrapped(|ui| {
                        let pts = self.selected_points();
                        if ui.small_button(super::i18n::registered(self.locale, "workspace-inspector.089")).clicked() {
                            self.commit_layout(editing::align_horizontal(&pts));
                        }
                        if ui.small_button(super::i18n::registered(self.locale, "workspace-inspector.090")).clicked() {
                            self.commit_layout(editing::align_vertical(&pts));
                        }
                        if ui.small_button(super::i18n::registered(self.locale, "workspace-inspector.091")).clicked() {
                            self.commit_layout(editing::distribute_horizontal(&pts));
                        }
                        if ui.small_button(super::i18n::registered(self.locale, "workspace-inspector.092")).clicked() {
                            self.commit_layout(editing::distribute_vertical(&pts));
                        }
                        if ui.small_button(super::i18n::registered(self.locale, "workspace-inspector.093")).clicked() {
                            self.commit_layout(editing::flip_horizontal(&pts));
                        }
                        if ui.small_button(super::i18n::registered(self.locale, "workspace-inspector.094")).clicked() {
                            self.commit_layout(editing::flip_vertical(&pts));
                        }
                    });
                }
            } else if !self.document.performers.is_empty() {
                ui.group(|ui| {
                    ui.label(
                        egui::RichText::new(super::i18n::registered(
                            self.locale,
                            "workspace-inspector.117",
                        ))
                        .strong(),
                    );
                    ui.small(super::i18n::registered(
                        self.locale,
                        "workspace-inspector.118",
                    ));
                    if ui
                        .button(super::i18n::registered(
                            self.locale,
                            "workspace-inspector.119",
                        ))
                        .on_hover_text(super::i18n::registered(
                            self.locale,
                            "workspace-inspector.120",
                        ))
                        .clicked()
                    {
                        self.begin_free_draw();
                    }
                });
            }
            // Calls the spatial-hash clinic directly (instead of the
            // `analyze_transition` compatibility wrapper) so the reusable
            // `ScanScratch` on `DrillApp` carries its allocation across
            // frames. `analyze_transition` allocates a fresh `ScanScratch`
            // per call, which is correct for occasional callers but would
            // reallocate every frame here.
            let report = clinic::scan_transition(
                &self.document,
                self.current_set,
                clinic::ClinicParams {
                    style: clinic::StepStyle::Custom { units_per_step: 1.0 },
                    collision_radius: 0.75,
                    danger_radius: 0.75,
                    crowded_radius: 0.75,
                    aggressive_above: 1.0,
                    impossible_above: f32::MAX,
                    ..clinic::ClinicParams::default()
                },
                &mut self.clinic_scratch,
            );
            let collisions = report.collisions.len();
            let excessive_strides = report
                .strides
                .iter()
                .filter(|stride| stride.rating > clinic::StrideRating::Comfortable)
                .count();
            ui.separator();
            if self.workspace_focus == Some(WorkspaceFocus::Clinic) {
                ui.scroll_to_cursor(Some(egui::Align::Center));
                self.workspace_focus = None;
            }
            ui.heading(super::i18n::registered(self.locale, "workspace-inspector.095"));
            ui.small(super::i18n::registered(self.locale, "workspace-inspector.049"));
            let collision_color = if collisions == 0 {
                Color32::from_rgb(99, 210, 151)
            } else {
                Color32::from_rgb(255, 92, 92)
            };
            ui.colored_label(
                collision_color,
                format!("● {}: {}", super::i18n::registered(self.locale, "workspace-inspector.106"), collisions),
            );
            let stride_color = if excessive_strides == 0 {
                Color32::from_rgb(99, 210, 151)
            } else {
                Color32::from_rgb(255, 184, 77)
            };
            ui.colored_label(
                stride_color,
                format!("● {}: {}", super::i18n::registered(self.locale, "workspace-inspector.107"), excessive_strides),
            );
            // A clinic report is useful only when it leads directly to the
            // people that need attention.  Keep this session-only: focusing
            // a warning changes neither the drill nor its undo history.
            let collision_focus = report
                .collisions
                .iter()
                .take(8)
                .map(|event| (event.a, event.b, event.count, event.distance))
                .collect::<Vec<_>>();
            let stride_focus = report
                .strides
                .iter()
                .filter(|event| event.rating > clinic::StrideRating::Comfortable)
                .take(8)
                .map(|event| (event.performer, event.units_per_count, event.rating))
                .collect::<Vec<_>>();
            if !collision_focus.is_empty() || !stride_focus.is_empty() {
                egui::CollapsingHeader::new(super::i18n::registered(
                    self.locale,
                    "clinic-ui.001",
                ))
                .default_open(false)
                .show(ui, |ui| {
                    for (a, b, count, distance) in collision_focus {
                        let label = format!(
                            "{} · {:.0} / {:.2}",
                            super::i18n::registered(self.locale, "clinic-ui.002"),
                            count,
                            distance
                        );
                        if ui
                            .button(label)
                            .on_hover_text(super::i18n::registered(
                                self.locale,
                                "clinic-ui.003",
                            ))
                            .clicked()
                        {
                            let next = self
                                .document
                                .performers
                                .iter()
                                .enumerate()
                                .filter_map(|(index, performer)| {
                                    (performer.id == a || performer.id == b).then_some(index)
                                })
                                .collect();
                            self.replace_selection(next);
                            self.field_viewport.center = editing::centroid(&self.selected_points());
                            self.field_viewport.zoom = self.field_viewport.zoom.max(1.8);
                            self.status = super::i18n::registered(self.locale, "clinic-ui.004")
                                .into();
                        }
                    }
                    for (performer_id, stride, rating) in stride_focus {
                        let label = format!(
                            "{} · {:.2} ({rating:?})",
                            super::i18n::registered(self.locale, "clinic-ui.005"),
                            stride
                        );
                        if ui
                            .button(label)
                            .on_hover_text(super::i18n::registered(
                                self.locale,
                                "clinic-ui.003",
                            ))
                            .clicked()
                        {
                            let next = self
                                .document
                                .performers
                                .iter()
                                .enumerate()
                                .filter_map(|(index, performer)| {
                                    (performer.id == performer_id).then_some(index)
                                })
                                .collect();
                            self.replace_selection(next);
                            self.field_viewport.center = editing::centroid(&self.selected_points());
                            self.field_viewport.zoom = self.field_viewport.zoom.max(1.8);
                            self.status = super::i18n::registered(self.locale, "clinic-ui.006")
                                .into();
                        }
                    }
                    ui.small(super::i18n::registered(self.locale, "clinic-ui.007"));
                });
            }
            let stats = pathing::transition_stats(&self.document, self.current_set);
            let unit_label = match self.document.grid.unit {
                Unit::Yards => "yd",
                Unit::Meters => "m",
            };
            ui.small(format!(
                "最大歩幅 {:.2}/count ・ 総移動 {:.0}{}",
                stats.max_step, stats.total_distance, unit_label
            ));
            ui.add_space(4.0);
            if ui
                .button(super::i18n::registered(self.locale, "workspace-inspector.050"))
                .on_hover_text(super::i18n::registered(self.locale, "workspace-inspector.051"))
                .clicked()
            {
                self.route_suggestions = suggest_routes(
                    &self.document,
                    self.current_set,
                    SuggestionConstraints::default(),
                    SuggestionLimits::default(),
                );
                self.route_suggestion_selected = 0;
                self.status = if self.route_suggestions.is_empty() {
                    super::i18n::registered(self.locale, "workspace-inspector.052").into()
                } else {
                    super::i18n::registered(self.locale, "workspace-inspector.053").into()
                };
            }
            if !self.route_suggestions.is_empty() {
                egui::CollapsingHeader::new(if self.locale == Locale::Ja {
                    format!("改善案プレビュー（{}件）", self.route_suggestions.len())
                } else {
                    format!("Fix preview ({})", self.route_suggestions.len())
                })
                .default_open(true)
                .show(ui, |ui| {
                    for (index, candidate) in self.route_suggestions.iter().enumerate() {
                        let reasons = candidate.reasons.iter().map(|reason| match (self.locale, reason) {
                            (Locale::Ja, SuggestionReason::AvoidCollision) => "衝突回避",
                            (Locale::Ja, SuggestionReason::ReduceStride) => "歩幅軽減",
                            (Locale::Ja, SuggestionReason::SoftenArrivalTurn) => "到着ターン緩和",
                            (Locale::Ja, SuggestionReason::MeetArrival) => "到着時刻",
                            (_, SuggestionReason::AvoidCollision) => "collision",
                            (_, SuggestionReason::ReduceStride) => "stride",
                            (_, SuggestionReason::SoftenArrivalTurn) => "arrival turn",
                            (_, SuggestionReason::MeetArrival) => "arrival time",
                        }).collect::<Vec<_>>().join(" + ");
                        let label = format!(
                            "{}  {}  · {} → {} pts · {} performer(s)",
                            index + 1,
                            reasons,
                            candidate.before.penalty(),
                            candidate.after.penalty(),
                            candidate.affected_performers.len()
                        );
                        ui.selectable_value(&mut self.route_suggestion_selected, index, label);
                    }
                    if let Some(candidate) = self.route_suggestions.get(self.route_suggestion_selected) {
                        ui.small(if self.locale == Locale::Ja {
                            format!("適用内容: {} moves / {} hold、経路変更 {}人。まだ設計には反映されていません。", candidate.counts.moves, candidate.counts.hold, candidate.affected_performers.len())
                        } else {
                            format!("Preview: {} moves / {} hold, {} route change(s). The design is still unchanged.", candidate.counts.moves, candidate.counts.hold, candidate.affected_performers.len())
                        });
                    }
                    if ui.button(super::i18n::registered(self.locale, "workspace-inspector.054")).clicked() {
                        let edit = self.route_suggestions.get(self.route_suggestion_selected)
                            .and_then(|candidate| candidate.to_edit(&self.document, self.current_set));
                        if let Some(edit) = edit {
                            if self.execute_edit(edit, super::i18n::registered(self.locale, "workspace-inspector.055")) {
                                self.status = super::i18n::registered(self.locale, "workspace-inspector.056").into();
                                self.route_suggestions.clear();
                            }
                        } else {
                            self.status = super::i18n::registered(self.locale, "workspace-inspector.057").into();
                            self.route_suggestions.clear();
                        }
                    }
                });
            }
            ui.separator();
            self.show_analytics_panel(ui);
            ui.separator();
            let focus_grid = self.workspace_focus == Some(WorkspaceFocus::Grid);
            egui::CollapsingHeader::new(super::i18n::registered(self.locale, "workspace-inspector.058")).open(focus_grid.then_some(true)).show(ui, |ui| {
                if focus_grid { ui.scroll_to_cursor(Some(egui::Align::Center)); }
                if !self.grid_draft_dirty && self.grid_draft.as_ref() != Some(&self.document.grid) {
                    self.grid_draft = Some(self.document.grid.clone());
                }
                let mut grid = self.grid_draft.clone().unwrap_or_else(|| self.document.grid.clone());
                let mut changed = false;
                let mut commit = false;
                let mut preset = None;
                ui.label(super::i18n::registered(self.locale, "workspace-inspector.059"));
                ui.horizontal_wrapped(|ui| {
                    if ui.small_button(super::i18n::registered(self.locale, "workspace-inspector.096")).clicked() {
                        preset = Some(GridConfig::default());
                    }
                    if ui.small_button(super::i18n::registered(self.locale, "workspace-inspector.097")).clicked() {
                        preset = Some(GridConfig::indoor());
                    }
                    if ui.small_button(super::i18n::registered(self.locale, "workspace-inspector.098")).clicked() {
                        preset = Some(GridConfig::soccer());
                    }
                    if ui.small_button(super::i18n::registered(self.locale, "workspace-inspector.127")).clicked() {
                        preset = Some(GridConfig::japan_floor());
                    }
                });
                ui.horizontal(|ui| {
                    changed |= ui.selectable_value(&mut grid.unit, Unit::Yards, super::i18n::registered(self.locale, "workspace-inspector.099")).changed();
                    changed |= ui.selectable_value(&mut grid.unit, Unit::Meters, super::i18n::registered(self.locale, "workspace-inspector.100")).changed();
                    commit |= changed;
                });
                ui.label(super::i18n::registered(self.locale, "workspace-inspector.060"));
                ui.horizontal(|ui| {
                    let width = ui.add(
                        egui::DragValue::new(&mut grid.width)
                            .range(10.0..=300.0)
                            .suffix(" W"),
                    );
                    let height = ui.add(
                        egui::DragValue::new(&mut grid.height)
                            .range(10.0..=300.0)
                            .suffix(" H"),
                    );
                    changed |= width.changed() || height.changed();
                    commit |= width.drag_stopped() || height.drag_stopped()
                        || (width.changed() && !width.dragged()) || (height.changed() && !height.dragged());
                });
                ui.label(super::i18n::registered(self.locale, "workspace-inspector.061"));
                ui.horizontal(|ui| {
                    let steps = ui.add(egui::DragValue::new(&mut grid.horizontal_steps).range(1..=32));
                    let units = ui.add(
                        egui::DragValue::new(&mut grid.horizontal_units).range(0.5..=20.0),
                    );
                    changed |= steps.changed() || units.changed();
                    commit |= steps.drag_stopped() || units.drag_stopped()
                        || (steps.changed() && !steps.dragged()) || (units.changed() && !units.dragged());
                });
                ui.label(super::i18n::registered(self.locale, "workspace-inspector.062"));
                ui.horizontal(|ui| {
                    let steps = ui.add(egui::DragValue::new(&mut grid.vertical_steps).range(1..=32));
                    let units = ui.add(
                        egui::DragValue::new(&mut grid.vertical_units).range(0.5..=20.0),
                    );
                    changed |= steps.changed() || units.changed();
                    commit |= steps.drag_stopped() || units.drag_stopped()
                        || (steps.changed() && !steps.dragged()) || (units.changed() && !units.dragged());
                });
                let major = ui.add(
                    egui::Slider::new(&mut grid.major_line_interval, 1.0..=20.0)
                        .text(super::i18n::registered(self.locale, "workspace-inspector.063")),
                );
                let resolution = ui.add(egui::Slider::new(&mut grid.resolution, 1..=8).text(super::i18n::registered(self.locale, "workspace-inspector.064")));
                changed |= major.changed() || resolution.changed();
                commit |= major.drag_stopped() || resolution.drag_stopped()
                    || (major.changed() && !major.dragged()) || (resolution.changed() && !resolution.dragged());
                ui.horizontal(|ui| {
                    let lines = ui.selectable_value(&mut grid.style, GridStyle::Lines, super::i18n::registered(self.locale, "workspace-inspector.065"));
                    let dots = ui.selectable_value(&mut grid.style, GridStyle::Dots, super::i18n::registered(self.locale, "workspace-inspector.066"));
                    changed |= lines.changed() || dots.changed();
                    commit |= lines.changed() || dots.changed();
                });
                let show = ui.checkbox(&mut grid.show_step_grid, super::i18n::registered(self.locale, "workspace-inspector.067"));
                let snap = ui.checkbox(&mut grid.snap_enabled, super::i18n::registered(self.locale, "workspace-inspector.068"));
                changed |= show.changed() || snap.changed();
                commit |= show.changed() || snap.changed();
                ui.label(super::i18n::registered(self.locale, "workspace-inspector.069"));
                let grid_height = grid.height;
                let mut remove_hash = None;
                for (index, hash) in grid.hashes.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        let label = ui.text_edit_singleline(&mut hash.label);
                        let position = ui.add(
                            egui::DragValue::new(&mut hash.position)
                                .range(0.0..=grid_height),
                        );
                        let weight = ui.add(
                            egui::DragValue::new(&mut hash.weight)
                                .range(0.25..=4.0)
                                .speed(0.1),
                        );
                        changed |= label.changed() || position.changed() || weight.changed();
                        commit |= label.lost_focus() || position.drag_stopped() || weight.drag_stopped()
                            || (position.changed() && !position.dragged()) || (weight.changed() && !weight.dragged());
                        if ui.small_button("×").clicked() {
                            remove_hash = Some(index);
                        }
                    });
                }
                if let Some(index) = remove_hash {
                    grid.hashes.remove(index);
                    changed = true;
                    commit = true;
                }
                if ui.small_button(super::i18n::registered(self.locale, "workspace-inspector.070")).clicked() {
                    grid.hashes.push(GridLine {
                        position: grid.height / 2.0,
                        label: "新しい線".into(),
                        weight: 1.0,
                    });
                    changed = true;
                    commit = true;
                }
                if let Some(grid) = preset {
                    if self
                        .history
                        .execute(
                            &mut self.document,
                            Edit::ReplaceGrid {
                                grid,
                                scale_positions: true,
                            },
                        )
                        .is_ok()
                    {
                        self.grid_draft = Some(self.document.grid.clone());
                        self.grid_draft_dirty = false;
                        self.dirty = true;
                    }
                } else {
                    if changed {
                        self.grid_draft = Some(grid.clone());
                        self.grid_draft_dirty = true;
                    }
                    if commit
                        && self.grid_draft_dirty
                        && grid != self.document.grid
                        && self
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
                        self.grid_draft_dirty = false;
                        self.grid_draft = Some(self.document.grid.clone());
                        self.dirty = true;
                    }
                }
            });
            if focus_grid { self.workspace_focus = None; }
            ui.separator();
            let focus_tempo = self.workspace_focus == Some(WorkspaceFocus::Tempo);
            egui::CollapsingHeader::new(super::i18n::registered(self.locale, "workspace-inspector.071")).open(focus_tempo.then_some(true)).show(ui, |ui| {
                if focus_tempo { ui.scroll_to_cursor(Some(egui::Align::Center)); }
                if !self.tempo_draft_dirty
                    && self.tempo_draft.as_ref().map(|tempo| tempo.events())
                        != Some(self.document.tempo.events())
                {
                    self.tempo_draft = Some(self.document.tempo.clone());
                }
                let mut tempo = self.tempo_draft.clone().unwrap_or_else(|| self.document.tempo.clone());
                let mut changed = false;
                let mut commit = false;
                ui.small(super::i18n::registered(self.locale, "workspace-inspector.072"));
                let total = self.document.timeline_counts() as f32;
                let events = tempo.events().to_vec();
                let mut edit: Option<(f32, f32, f32)> = None;
                let mut remove: Option<f32> = None;
                let removable = events.len() > 1;
                for ev in &events {
                    ui.horizontal(|ui| {
                        let mut count = ev.count;
                        let mut bpm = ev.bpm;
                        ui.label(super::i18n::registered(self.locale, "workspace-inspector.101"));
                        let c = ui.add(
                            egui::DragValue::new(&mut count).range(0.0..=total.max(0.0)),
                        );
                        let b = ui.add(
                            egui::DragValue::new(&mut bpm)
                                .range(20.0..=300.0)
                                .suffix(" BPM"),
                        );
                        if c.changed() || b.changed() {
                            edit = Some((ev.count, count, bpm));
                            changed = true;
                        }
                        commit |= c.drag_stopped() || b.drag_stopped()
                            || (c.changed() && !c.dragged()) || (b.changed() && !b.dragged());
                        if removable && ui.small_button("×").clicked() {
                            remove = Some(ev.count);
                        }
                    });
                }
                if let Some((old_count, new_count, new_bpm)) = edit {
                    if (new_count - old_count).abs() > f32::EPSILON {
                        tempo.remove(old_count);
                    }
                    tempo.set(new_count, new_bpm);
                }
                if let Some(count) = remove {
                    tempo.remove(count);
                    changed = true;
                    commit = true;
                }
                if ui.button(super::i18n::registered(self.locale, "workspace-inspector.073")).clicked() {
                    let global = self
                        .document
                        .global_count(self.current_set, self.count_position)
                        .round();
                    tempo.set(global, self.tempo_bpm);
                    changed = true;
                    commit = true;
                }
                if changed {
                    self.tempo_draft = Some(tempo.clone());
                    self.tempo_draft_dirty = true;
                }
                if commit && self.tempo_draft_dirty
                    && tempo.events() != self.document.tempo.events()
                    && self
                        .history
                        .execute(&mut self.document, Edit::SetTempoMap { tempo })
                        .is_ok()
                {
                    self.tempo_bpm = self.document.tempo.bpm_at(0.0);
                    self.tempo_draft_dirty = false;
                    self.tempo_draft = Some(self.document.tempo.clone());
                    self.dirty = true;
                }
                let global = self
                    .document
                    .global_count(self.current_set, self.count_position);
                ui.small(format!(
                    "現在位置 {:.2} 秒 ・ 曲全体 {:.1} 秒",
                    self.document.tempo.seconds_at(global),
                    self.document.tempo.seconds_at(total)
                ));
            });
            if focus_tempo { self.workspace_focus = None; }
            ui.separator();
            self.show_export_inspector(ui);
            ui.separator();
            self.show_audio_inspector(ui);
            ui.separator();
            self.show_text_export_progress(ui);
            ui.small(&self.status);
                });
        },
    );
        ui.separator();
    }

    /// "Analytics" panel: rhythm-sync, aesthetic/symmetry scoring, and the
    /// Show DNA heatmap toggle. Unlike the clinic scan above (cheap, reused
    /// `ScanScratch`, safe to run every frame), all three analyses here are
    /// heavier at 1,000-performer scale, so each is cached against
    /// `history.revision()` (and, for the per-set aesthetic score, the
    /// current set index too) and only recomputed when that key changes --
    /// never unconditionally every repaint. See the cache fields'
    /// doc comments on `DrillApp` for the exact invalidation rule.
    fn show_analytics_panel(&mut self, ui: &mut egui::Ui) {
        let revision = self.history.revision();
        let analytics_key = super::analytics_state::AnalyticsKey {
            revision,
            set_index: self.current_set,
            beats_per_measure: self.beats_per_measure,
            heatmap: self.heatmap_enabled,
        };
        self.analytics_state.poll(analytics_key);
        self.analytics_state.ensure(&self.document, analytics_key);
        egui::CollapsingHeader::new(super::i18n::registered(self.locale, "analytics.001")).show(
            ui,
            |ui| {
                ui.small(super::i18n::registered(self.locale, "analytics.002"));

                // --- Rhythm sync -------------------------------------------------
                ui.separator();
                ui.heading(super::i18n::registered(self.locale, "analytics.003"));
                ui.small(super::i18n::registered(self.locale, "analytics.004"));
                if let Some(report) = self.analytics_state.rhythm(analytics_key) {
                    if report.events.is_empty() {
                        ui.small(super::i18n::registered(self.locale, "analytics.010"));
                    } else {
                        let total = report.events.len() as f32;
                        let on_downbeat = report
                            .events
                            .iter()
                            .filter(|event| event.intent == rhythm_sync::RhythmicIntent::OnDownbeat)
                            .count();
                        let on_backbeat = report
                            .events
                            .iter()
                            .filter(|event| event.intent == rhythm_sync::RhythmicIntent::OnBackbeat)
                            .count();
                        let syncopated = report
                            .events
                            .iter()
                            .filter(|event| event.intent == rhythm_sync::RhythmicIntent::Syncopated)
                            .count();
                        let color = if report.show_score >= 70.0 {
                            Color32::from_rgb(99, 210, 151)
                        } else if report.show_score >= 40.0 {
                            Color32::from_rgb(255, 184, 77)
                        } else {
                            Color32::from_rgb(255, 92, 92)
                        };
                        ui.colored_label(
                            color,
                            format!(
                                "{} {:.0}%",
                                super::i18n::registered(self.locale, "analytics.005"),
                                report.on_beat_ratio * 100.0
                            ),
                        );
                        ui.small(format!(
                            "{}: {} {:.0}%  ・  {} {:.0}%  ・  {} {:.0}%",
                            super::i18n::registered(self.locale, "analytics.006"),
                            super::i18n::registered(self.locale, "analytics.007"),
                            on_downbeat as f32 / total * 100.0,
                            super::i18n::registered(self.locale, "analytics.008"),
                            on_backbeat as f32 / total * 100.0,
                            super::i18n::registered(self.locale, "analytics.009"),
                            syncopated as f32 / total * 100.0,
                        ));
                    }
                }

                // --- Aesthetics / symmetry ---------------------------------------
                ui.separator();
                ui.heading(super::i18n::registered(self.locale, "analytics.011"));
                ui.small(super::i18n::registered(self.locale, "analytics.012"));
                if let Some(score) = self.analytics_state.aesthetics(analytics_key) {
                    ui.small(format!(
                        "{}: {:.0}  ・  {}: {:.0}  ・  {}: {:.0}",
                        super::i18n::registered(self.locale, "analytics.013"),
                        score.overall,
                        super::i18n::registered(self.locale, "analytics.014"),
                        score.symmetry.score,
                        super::i18n::registered(self.locale, "analytics.015"),
                        score.density_uniformity,
                    ));
                    if !score.symmetry.worst_offenders.is_empty() {
                        ui.small(super::i18n::registered(self.locale, "analytics.016"));
                        for &(performer_id, distance) in &score.symmetry.worst_offenders {
                            let label = self
                                .document
                                .performers
                                .iter()
                                .find(|performer| performer.id == performer_id)
                                .map(|performer| performer.label.as_str())
                                .unwrap_or("?");
                            ui.small(format!("    {label}  ·  {distance:.2}"));
                        }
                    }
                } else {
                    ui.small(super::i18n::registered(self.locale, "analytics.017"));
                }

                // --- Show DNA heatmap ---------------------------------------------
                ui.separator();
                ui.heading(super::i18n::registered(self.locale, "analytics.018"));
                ui.small(super::i18n::registered(self.locale, "analytics.019"));
                ui.checkbox(
                    &mut self.heatmap_enabled,
                    super::i18n::registered(self.locale, "analytics.020"),
                );
                if !self.heatmap_enabled {
                    self.analytics_state.cancel();
                }
                if let Some(progress) = self.analytics_state.progress() {
                    ui.horizontal(|ui| {
                        ui.add(egui::ProgressBar::new(progress).show_percentage());
                        if ui
                            .button(if self.locale == drill_core::Locale::Ja {
                                "解析を中止"
                            } else {
                                "Cancel analysis"
                            })
                            .clicked()
                        {
                            self.analytics_state.cancel();
                        }
                    });
                }

                // --- Trails ---------------------------------------------------------
                ui.separator();
                ui.heading(super::i18n::registered(self.locale, "analytics.021"));
                ui.small(super::i18n::registered(self.locale, "analytics.022"));
                ui.horizontal_wrapped(|ui| {
                    ui.label(super::i18n::registered(self.locale, "analytics.023"));
                    ui.selectable_value(
                        &mut self.trail_selection,
                        drill_render::TrailSelection::None,
                        super::i18n::registered(self.locale, "analytics.024"),
                    );
                    ui.selectable_value(
                        &mut self.trail_selection,
                        drill_render::TrailSelection::Selected,
                        super::i18n::registered(self.locale, "analytics.025"),
                    );
                    ui.selectable_value(
                        &mut self.trail_selection,
                        drill_render::TrailSelection::All,
                        super::i18n::registered(self.locale, "analytics.026"),
                    );
                });
            },
        );
    }
}
