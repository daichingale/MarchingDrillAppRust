//! A Finder-like set browser for quickly orienting in long productions.
//!
//! It deliberately derives all landmarks from the document's existing set
//! annotations; there is no parallel, UI-only list of rehearsal information.

use drill_core::{Document, Locale};
use eframe::egui;

#[derive(Clone, Debug, PartialEq, Eq)]
struct SetEntry {
    index: usize,
    start_count: u32,
    title: String,
    landmark: String,
    counts: u16,
}

#[derive(Default)]
pub(crate) struct SetNavigator {
    open: bool,
    query: String,
    selected: usize,
    focus_query: bool,
}

impl SetNavigator {
    pub(crate) fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
        self.focus_query = true;
    }

    /// Cmd/Ctrl+J is intentionally separate from the command palette: it is
    /// a direct navigation gesture, comparable to Finder's jump surfaces.
    pub(crate) fn open_if_requested(&mut self, context: &egui::Context) {
        if self.open || context.egui_wants_keyboard_input() || context.text_edit_focused() {
            return;
        }
        if context.input_mut(|input| {
            input.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                egui::Key::J,
            ))
        }) {
            self.open();
        }
    }

    pub(crate) fn show(
        &mut self,
        context: &egui::Context,
        document: &Document,
        current_set: usize,
        locale: Locale,
    ) -> Option<usize> {
        if !self.open {
            return None;
        }
        let entries = matching_entries(document, &self.query);
        self.selected = self.selected.min(entries.len().saturating_sub(1));
        let mut chosen = None;
        let mut close = false;
        context.input_mut(|input| {
            if input.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                close = true;
            }
            if !entries.is_empty() && input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)
            {
                self.selected = (self.selected + 1).min(entries.len() - 1);
            }
            if !entries.is_empty() && input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                self.selected = self.selected.saturating_sub(1);
            }
            if input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                && let Some(entry) = entries.get(self.selected)
            {
                chosen = Some(entry.index);
                close = true;
            }
        });

        let mut open = true;
        egui::Window::new(super::i18n::registered(locale, "set-navigator.001"))
            .id(egui::Id::new("set-navigator"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size(egui::vec2(560.0, 440.0))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(context, |ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .id(ui.make_persistent_id("set-navigator-query"))
                        .hint_text(super::i18n::registered(locale, "set-navigator.002"))
                        .desired_width(f32::INFINITY),
                );
                if self.focus_query {
                    response.request_focus();
                    self.focus_query = false;
                }
                if response.changed() {
                    self.selected = 0;
                }
                ui.add_space(6.0);
                if entries.is_empty() {
                    ui.centered_and_justified(|ui| {
                        ui.label(super::i18n::registered(locale, "set-navigator.003"));
                    });
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt("set-navigator-results")
                        .show(ui, |ui| {
                            for (position, entry) in entries.iter().enumerate() {
                                let is_current = entry.index == current_set;
                                let landmark = if entry.landmark.is_empty() {
                                    String::new()
                                } else {
                                    format!("  ·  {}", entry.landmark)
                                };
                                let label = format!(
                                    "{} {}  ·  {} {}  ·  {} {}{}",
                                    super::i18n::registered(locale, "set-navigator.004"),
                                    entry.index + 1,
                                    super::i18n::registered(locale, "set-navigator.005"),
                                    entry.start_count + 1,
                                    entry.counts,
                                    super::i18n::registered(locale, "set-navigator.006"),
                                    landmark,
                                );
                                let text = if is_current {
                                    egui::RichText::new(format!("{}  —  {}", entry.title, label))
                                        .strong()
                                } else {
                                    egui::RichText::new(format!("{}  —  {}", entry.title, label))
                                };
                                let response = ui.add(
                                    egui::Button::new(text)
                                        .selected(position == self.selected)
                                        .frame(false),
                                );
                                response.widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::WidgetType::Button,
                                        true,
                                        format!(
                                            "{} {}",
                                            super::i18n::registered(locale, "set-navigator.007"),
                                            entry.title
                                        ),
                                    )
                                });
                                if response.hovered() {
                                    self.selected = position;
                                }
                                if response.clicked() {
                                    chosen = Some(entry.index);
                                    close = true;
                                }
                            }
                        });
                }
                ui.separator();
                ui.small(super::i18n::registered(locale, "set-navigator.008"));
            });
        if !open || close {
            self.open = false;
            self.focus_query = false;
        }
        chosen
    }
}

fn matching_entries(document: &Document, query: &str) -> Vec<SetEntry> {
    let needle = query.trim().to_lowercase();
    let mut count = 0_u32;
    document
        .sets
        .iter()
        .enumerate()
        .filter_map(|(index, set)| {
            let start_count = count;
            count = count.saturating_add(u32::from(set.counts));
            let annotation = &set.annotation;
            let landmark = if annotation.rehearsal_mark.trim().is_empty() {
                annotation.title.trim().to_owned()
            } else {
                annotation.rehearsal_mark.trim().to_owned()
            };
            let haystack = format!(
                "{} {} {} {} {}",
                index + 1,
                set.name,
                start_count + 1,
                landmark,
                annotation.notes
            )
            .to_lowercase();
            (needle.is_empty() || haystack.contains(&needle)).then(|| SetEntry {
                index,
                start_count,
                title: set.name.clone(),
                landmark,
                counts: set.counts,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_includes_set_name_number_and_rehearsal_mark_with_global_count() {
        let mut document = Document::demo(1, 3);
        document.sets[0].name = "Opening".into();
        document.sets[0].counts = 8;
        document.sets[1].name = "Impact".into();
        document.sets[1].counts = 12;
        document.sets[1].annotation.rehearsal_mark = "B".into();

        let mark = matching_entries(&document, "b");
        assert_eq!(mark.len(), 1);
        assert_eq!(mark[0].index, 1);
        assert_eq!(mark[0].start_count, 8);
        assert_eq!(matching_entries(&document, "2")[0].index, 1);
        assert_eq!(matching_entries(&document, "impact")[0].counts, 12);
    }
}
