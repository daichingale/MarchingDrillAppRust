//! Dockable production-sheet workspace.  It owns only ephemeral UI state;
//! annotation edits are one explicit `Edit` transaction on Save.
use super::*;
use drill_core::{Edit, SetAnnotation, SetId, production};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sort {
    Set,
    Count,
    Mark,
    Title,
    Tempo,
}

pub(super) struct ProductionSheetWorkspace {
    pub open: bool,
    /// When enabled, keep the inspector on the set under the playhead.  This
    /// never replaces an annotation draft that has local unsaved changes.
    pub follow_playhead: bool,
    filter: String,
    sort: Sort,
    descending: bool,
    selected: Option<SetId>,
    draft: Option<SetAnnotation>,
}

impl Default for ProductionSheetWorkspace {
    fn default() -> Self {
        Self {
            open: false,
            follow_playhead: false,
            filter: String::new(),
            sort: Sort::Set,
            descending: false,
            selected: None,
            draft: None,
        }
    }
}

impl ProductionSheetWorkspace {
    fn seek(app: &mut DrillApp, set_index: usize) {
        app.navigate_to_set(set_index); // exact integer count, pause, preserve selection
    }

    fn has_unsaved_draft(&self, document: &drill_core::Document) -> bool {
        let Some(selected) = self.selected else {
            return false;
        };
        let Some(set) = document.sets.iter().find(|set| set.id == selected) else {
            return self.draft.is_some();
        };
        self.draft
            .as_ref()
            .is_some_and(|draft| draft != &set.annotation)
    }

    /// Synchronize selection only when doing so cannot replace work that the
    /// user has not explicitly saved or cancelled.
    fn follow_current_set(&mut self, app: &DrillApp) {
        if !self.follow_playhead || self.has_unsaved_draft(&app.document) {
            return;
        }
        let Some(set) = app.document.sets.get(app.current_set) else {
            return;
        };
        if self.selected != Some(set.id) {
            self.selected = Some(set.id);
            self.draft = Some(set.annotation.clone());
        }
    }
    fn sort_button(
        ui: &mut egui::Ui,
        current: &mut Sort,
        descending: &mut bool,
        value: Sort,
        label: &str,
    ) {
        let suffix = if *current == value {
            if *descending { " ↓" } else { " ↑" }
        } else {
            ""
        };
        if ui.button(format!("{label}{suffix}")).clicked() {
            if *current == value {
                *descending = !*descending;
            } else {
                *current = value;
                *descending = false;
            }
        }
    }

    /// Returns the index into the filtered/sorted row list. Keeping this
    /// separate from egui input makes the familiar Finder-style navigation
    /// predictable even while a filter changes the visible rows.
    fn keyboard_target(row_count: usize, selected: Option<usize>, key: egui::Key) -> Option<usize> {
        if row_count == 0 {
            return None;
        }
        let Some(selected) = selected else {
            return match key {
                egui::Key::ArrowUp | egui::Key::End => Some(row_count - 1),
                egui::Key::ArrowDown | egui::Key::Home => Some(0),
                _ => None,
            };
        };
        let selected = selected.min(row_count - 1);
        match key {
            egui::Key::ArrowDown => Some((selected + 1).min(row_count - 1)),
            egui::Key::ArrowUp => Some(selected.saturating_sub(1)),
            egui::Key::Home => Some(0),
            egui::Key::End => Some(row_count - 1),
            _ => None,
        }
    }

    fn select_row(&mut self, app: &DrillApp, index: usize) {
        // Selecting a row is review-only. It deliberately does not seek the
        // playhead, and it turns off Follow so the chosen row stays stable.
        self.follow_playhead = false;
        self.selected = Some(app.document.sets[index].id);
        self.draft = Some(app.document.sets[index].annotation.clone());
    }
    pub(super) fn show(&mut self, app: &mut DrillApp, context: &egui::Context) {
        if !self.open {
            return;
        }
        self.follow_current_set(app);
        let title = i18n::registered(app.locale, "production-sheet.001");
        let mut open = self.open;
        let mut close = false;
        egui::Window::new(title)
            .id(egui::Id::new("production-sheet-workspace"))
            .open(&mut open)
            .default_width(960.0)
            .default_height(560.0)
            .resizable(true)
            .collapsible(false)
            .show(context, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.strong(i18n::registered(app.locale, "production-sheet.002"));
                    let follow_response = ui
                        .checkbox(
                        &mut self.follow_playhead,
                        i18n::registered(app.locale, "production-sheet.025"),
                    )
                        .on_hover_text(i18n::registered(app.locale, "production-sheet.026"));
                    if follow_response.changed() && self.follow_playhead {
                        self.follow_current_set(app);
                    }
                    ui.small(i18n::registered(app.locale, "production-sheet.030"));
                    ui.small(i18n::registered(app.locale, "production-sheet.034"));
                    if ui
                        .button(i18n::registered(app.locale, "production-sheet.005"))
                        .clicked()
                    {
                        close = true;
                    }
                });
                ui.add(
                    egui::TextEdit::singleline(&mut self.filter)
                        .hint_text(i18n::registered(app.locale, "production-sheet.006"))
                        .desired_width(f32::INFINITY),
                );
                let needle = self.filter.trim().to_lowercase();
                let derived = production::production_sheet(&app.document);
                let mut rows: Vec<_> = derived
                    .into_iter()
                    .filter(|row| {
                        needle.is_empty()
                            || row.set_number.to_string().contains(&needle)
                            || row.mark.to_lowercase().contains(&needle)
                            || row.title.to_lowercase().contains(&needle)
                            || row.notes.to_lowercase().contains(&needle)
                            || format!("{:.1}", row.tempo_bpm).contains(&needle)
                    })
                    .collect();
                rows.sort_by(|a, b| {
                    let order = match self.sort {
                        Sort::Set => a.set_number.cmp(&b.set_number),
                        Sort::Count => a.counts.cmp(&b.counts),
                        Sort::Mark => a.mark.cmp(&b.mark),
                        Sort::Title => a.title.cmp(&b.title),
                        Sort::Tempo => a.tempo_bpm.total_cmp(&b.tempo_bpm),
                    };
                    if self.descending {
                        order.reverse()
                    } else {
                        order
                    }
                });
                // A table can be reviewed at speed without moving the show.
                // Text fields retain their normal keys; selection navigation
                // is active only while no text editor owns the keyboard.
                if !context.egui_wants_keyboard_input() && !context.text_edit_focused() {
                    let selected_position = self.selected.and_then(|id| {
                        rows.iter().position(|row| app.document.sets[row.set_number - 1].id == id)
                    });
                    let pressed = context.input_mut(|input| {
                        [egui::Key::ArrowDown, egui::Key::ArrowUp, egui::Key::Home, egui::Key::End]
                            .into_iter()
                            .find(|key| input.consume_key(egui::Modifiers::NONE, *key))
                    });
                    if let Some(key) = pressed
                        && !self.has_unsaved_draft(&app.document)
                        && let Some(position) = Self::keyboard_target(rows.len(), selected_position, key)
                    {
                        self.select_row(app, rows[position].set_number - 1);
                    }
                    let go_to_selected = context.input_mut(|input| {
                        input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                    });
                    if go_to_selected
                        && let Some(id) = self.selected
                        && let Some(index) = app.document.sets.iter().position(|set| set.id == id)
                    {
                        Self::seek(app, index);
                    }
                }
                ui.horizontal(|ui| {
                    Self::sort_button(ui, &mut self.sort, &mut self.descending, Sort::Set, "#");
                    Self::sort_button(
                        ui,
                        &mut self.sort,
                        &mut self.descending,
                        Sort::Count,
                        i18n::registered(app.locale, "production-sheet.007"),
                    );
                    Self::sort_button(
                        ui,
                        &mut self.sort,
                        &mut self.descending,
                        Sort::Mark,
                        i18n::registered(app.locale, "production-sheet.008"),
                    );
                    Self::sort_button(
                        ui,
                        &mut self.sort,
                        &mut self.descending,
                        Sort::Title,
                        i18n::registered(app.locale, "production-sheet.009"),
                    );
                    Self::sort_button(
                        ui,
                        &mut self.sort,
                        &mut self.descending,
                        Sort::Tempo,
                        i18n::registered(app.locale, "production-sheet.010"),
                    );
                    ui.label(i18n::registered(app.locale, "production-sheet.011"));
                });
                let draft_is_dirty = self.has_unsaved_draft(&app.document);
                // ScrollArea only lays out visible rows, keeping a 256-set show cheap.
                egui::ScrollArea::vertical()
                    .id_salt("production-sheet-rows")
                    .max_height(270.0)
                    .show_rows(ui, 26.0, rows.len(), |ui, range| {
                    for row in &rows[range] {
                        let index = row.set_number - 1;
                        let set_id = app.document.sets[index].id;
                        let global_count = app.document.global_count(index, 0.0).round() as u32;
                        let label = format!(
                            "{:>3}  {} {:>4}  {:<8}  {:<20}  {:>3}c  {:>6.1} BPM  {:>7.3}s  {:>6.3}s  {}{}",
                            row.set_number,
                            i18n::registered(app.locale, "production-sheet.024"),
                            global_count,
                            row.mark,
                                row.title,
                                row.counts,
                                row.tempo_bpm,
                                row.sync_time_seconds,
                                row.transition_duration_seconds,
                                row.notes,
                                if index == app.current_set {
                                    format!("  · {}", i18n::registered(app.locale, "production-sheet.031"))
                                } else {
                                    String::new()
                                }
                            );
                            let response = ui
                                .add_enabled(
                                    !draft_is_dirty || self.selected == Some(set_id),
                                    egui::Button::selectable(self.selected == Some(set_id), label),
                                );
                            let response = if draft_is_dirty && self.selected != Some(set_id) {
                                response.on_hover_text(i18n::registered(
                                    app.locale,
                                    "production-sheet.027",
                                ))
                            } else {
                                response.on_hover_text(i18n::registered(
                                    app.locale,
                                    "production-sheet.029",
                                ))
                            };
                            if response.clicked() {
                                self.select_row(app, index);
                            }
                        }
                    });
                let selected = self
                    .selected
                    .and_then(|id| app.document.sets.iter().position(|set| set.id == id));
                let Some(index) = selected else {
                    ui.small(i18n::registered(app.locale, "production-sheet.013"));
                    return;
                };
                let set_id = app.document.sets[index].id;
                let current = app.document.sets[index].annotation.clone();
                let mut draft = self.draft.clone().unwrap_or_else(|| current.clone());
                ui.separator();
                ui.strong(format!(
                    "{} {}",
                    i18n::registered(app.locale, "production-sheet.014"),
                    index + 1
                ));
                ui.horizontal(|ui| {
                    if ui
                        .button(i18n::registered(app.locale, "production-sheet.028"))
                        .clicked()
                    {
                        Self::seek(app, index);
                    }
                    if self.follow_playhead && draft_is_dirty {
                        ui.small(i18n::registered(app.locale, "production-sheet.027"));
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(i18n::registered(app.locale, "production-sheet.020"));
                    ui.text_edit_singleline(&mut draft.title);
                });
                ui.horizontal(|ui| {
                    ui.label(i18n::registered(app.locale, "production-sheet.021"));
                    ui.text_edit_singleline(&mut draft.rehearsal_mark);
                });
                ui.label(i18n::registered(app.locale, "production-sheet.022"));
                ui.add(egui::TextEdit::multiline(&mut draft.notes).desired_rows(3));
                ui.horizontal(|ui| {
                    let mut tempo_on = draft.tempo_bpm.is_some();
                    ui.checkbox(
                        &mut tempo_on,
                        i18n::registered(app.locale, "production-sheet.023"),
                    );
                    if tempo_on && draft.tempo_bpm.is_none() {
                        draft.tempo_bpm = Some(
                            app.document
                                .tempo
                                .bpm_at(app.document.global_count(index, 0.0)),
                        );
                    }
                    if !tempo_on {
                        draft.tempo_bpm = None;
                    }
                    if let Some(v) = &mut draft.tempo_bpm {
                        ui.add(egui::DragValue::new(v).range(1.0..=999.0).suffix(" BPM"));
                    }
                });
                ui.horizontal(|ui| {
                    let mut enabled = draft.sync_time_seconds.is_some();
                    ui.checkbox(
                        &mut enabled,
                        i18n::registered(app.locale, "production-sheet.018"),
                    );
                    if enabled && draft.sync_time_seconds.is_none() {
                        draft.sync_time_seconds = Some(f64::from(
                            app.document
                                .tempo
                                .seconds_at(app.document.global_count(index, 0.0)),
                        ));
                    }
                    if !enabled {
                        draft.sync_time_seconds = None;
                    }
                    if let Some(v) = &mut draft.sync_time_seconds {
                        ui.add(
                            egui::DragValue::new(v)
                                .range(0.0..=86_400.0)
                                .speed(0.01)
                                .suffix(" s"),
                        );
                    }
                });
                ui.horizontal(|ui| {
                    let mut enabled = draft.transition_duration_seconds.is_some();
                    ui.checkbox(
                        &mut enabled,
                        i18n::registered(app.locale, "production-sheet.019"),
                    );
                    if enabled && draft.transition_duration_seconds.is_none() {
                        let start = app.document.global_count(index, 0.0);
                        let end = start + f32::from(app.document.sets[index].counts);
                        draft.transition_duration_seconds = Some(
                            f64::from(
                                app.document.tempo.seconds_at(end)
                                    - app.document.tempo.seconds_at(start),
                            )
                            .max(0.0),
                        );
                    }
                    if !enabled {
                        draft.transition_duration_seconds = None;
                    }
                    if let Some(v) = &mut draft.transition_duration_seconds {
                        ui.add(
                            egui::DragValue::new(v)
                                .range(0.0..=86_400.0)
                                .speed(0.01)
                                .suffix(" s"),
                        );
                    }
                });
                ui.horizontal(|ui| {
                    if ui
                        .button(i18n::registered(app.locale, "production-sheet.015"))
                        .clicked()
                        && draft != current
                    {
                        app.execute_edit(
                            Edit::SetAnnotation {
                                set_id,
                                annotation: draft.clone(),
                            },
                            i18n::registered(app.locale, "production-sheet.017"),
                        );
                    }
                    if ui
                        .button(i18n::registered(app.locale, "production-sheet.016"))
                        .clicked()
                    {
                        draft = current;
                    }
                });
                self.draft = Some(draft);
            });
        self.open = open && !close;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workspace_defaults_to_a_visible_set_order_and_without_following() {
        let state = ProductionSheetWorkspace::default();
        assert!(!state.open);
        assert!(!state.follow_playhead);
        assert_eq!(state.sort, Sort::Set);
    }
    #[test]
    fn row_click_navigation_is_an_exact_paused_set_start() {
        let mut app = DrillApp::default();
        app.selected.insert(0);
        app.playing = true;
        ProductionSheetWorkspace::seek(&mut app, 1);
        assert_eq!(app.current_set, 1);
        assert_eq!(app.count_position, 0.0);
        assert!(!app.playing);
        assert_eq!(app.selected.len(), 1);
    }

    #[test]
    fn follow_playhead_selects_the_current_set_without_moving_the_playhead() {
        let app = DrillApp {
            current_set: 1,
            count_position: 3.0,
            ..DrillApp::default()
        };
        let mut workspace = ProductionSheetWorkspace {
            follow_playhead: true,
            ..Default::default()
        };
        workspace.follow_current_set(&app);
        assert_eq!(workspace.selected, Some(app.document.sets[1].id));
        assert_eq!(app.current_set, 1);
        assert_eq!(app.count_position, 3.0);
    }

    #[test]
    fn follow_playhead_never_replaces_an_unsaved_annotation_draft() {
        let mut app = DrillApp::default();
        let mut workspace = ProductionSheetWorkspace {
            follow_playhead: true,
            selected: Some(app.document.sets[0].id),
            draft: Some(SetAnnotation {
                title: "Unsaved cue".into(),
                ..app.document.sets[0].annotation.clone()
            }),
            ..Default::default()
        };
        app.current_set = 1;
        workspace.follow_current_set(&app);
        assert_eq!(workspace.selected, Some(app.document.sets[0].id));
        assert_eq!(workspace.draft.as_ref().unwrap().title, "Unsaved cue");
    }

    #[test]
    fn keyboard_navigation_stays_within_the_visible_row_list() {
        assert_eq!(
            ProductionSheetWorkspace::keyboard_target(0, None, egui::Key::ArrowDown),
            None
        );
        assert_eq!(
            ProductionSheetWorkspace::keyboard_target(3, None, egui::Key::ArrowDown),
            Some(0)
        );
        assert_eq!(
            ProductionSheetWorkspace::keyboard_target(3, Some(0), egui::Key::ArrowUp),
            Some(0)
        );
        assert_eq!(
            ProductionSheetWorkspace::keyboard_target(3, Some(1), egui::Key::End),
            Some(2)
        );
        assert_eq!(
            ProductionSheetWorkspace::keyboard_target(3, Some(2), egui::Key::Home),
            Some(0)
        );
    }
}
