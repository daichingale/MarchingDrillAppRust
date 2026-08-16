use drill_core::{Document, Locale, SectionId};
use eframe::egui::{self, Color32};
use std::collections::{BTreeMap, BTreeSet};

pub enum Action {
    Add { name: String, short: String },
    Rename(SectionId, String, String),
    Assign(SectionId),
    Remove(SectionId, SectionId),
}

#[derive(Default)]
pub struct SectionManager {
    pub open: bool,
    drafts: BTreeMap<SectionId, (String, String)>,
    new_name: String,
    new_short: String,
}

impl SectionManager {
    pub fn clear_drafts(&mut self) {
        self.drafts.clear();
    }

    pub fn section_removed(&mut self, id: SectionId) {
        self.drafts.remove(&id);
    }

    pub fn section_added(&mut self) {
        self.new_name.clear();
        self.new_short.clear();
    }

    pub fn show(
        &mut self,
        context: &egui::Context,
        locale: Locale,
        document: &Document,
        selected: &BTreeSet<usize>,
    ) -> Option<Action> {
        if !self.open {
            return None;
        }
        let mut action = None;
        egui::Window::new(super::i18n::registered(locale, "section-manager.001"))
            .open(&mut self.open)
            .default_width(520.0)
            .show(context, |ui| {
                ui.label(super::i18n::registered(locale, "section-manager.002"));
                ui.small(super::i18n::registered(locale, "section-manager.003"));
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        for section in &document.sections {
                            let draft = self
                                .drafts
                                .entry(section.id)
                                .or_insert_with(|| (section.name.clone(), section.short.clone()));
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    ui.colored_label(
                                        Color32::from_rgb(
                                            section.color[0],
                                            section.color[1],
                                            section.color[2],
                                        ),
                                        "●",
                                    );
                                    let count = document
                                        .performers
                                        .iter()
                                        .filter(|p| p.section == section.id)
                                        .count();
                                    ui.label(if locale == Locale::Ja {
                                        format!("{count}人")
                                    } else {
                                        format!("{count} performers")
                                    });
                                    ui.label(super::i18n::registered(
                                        locale,
                                        "section-manager.004",
                                    ));
                                    ui.text_edit_singleline(&mut draft.0);
                                    ui.label(super::i18n::registered(
                                        locale,
                                        "section-manager.005",
                                    ));
                                    ui.add(
                                        egui::TextEdit::singleline(&mut draft.1)
                                            .desired_width(70.0),
                                    );
                                });
                                ui.horizontal(|ui| {
                                    let valid = !draft.0.trim().is_empty()
                                        && !draft.1.trim().is_empty()
                                        && (draft.0 != section.name || draft.1 != section.short);
                                    if ui
                                        .add_enabled(
                                            valid,
                                            egui::Button::new(super::i18n::registered(
                                                locale,
                                                "section-manager.006",
                                            )),
                                        )
                                        .clicked()
                                    {
                                        action = Some(Action::Rename(
                                            section.id,
                                            draft.0.trim().to_owned(),
                                            draft.1.trim().to_owned(),
                                        ));
                                    }
                                    if ui
                                        .add_enabled(
                                            !selected.is_empty(),
                                            egui::Button::new(super::i18n::registered(
                                                locale,
                                                "section-manager.007",
                                            )),
                                        )
                                        .clicked()
                                    {
                                        action = Some(Action::Assign(section.id));
                                    }
                                    if document.sections.len() > 1 {
                                        let fallback = document
                                            .sections
                                            .iter()
                                            .find(|candidate| candidate.id != section.id)
                                            .map(|candidate| candidate.id)
                                            .expect("another section exists");
                                        if ui
                                            .button(super::i18n::registered(
                                                locale,
                                                "section-manager.008",
                                            ))
                                            .on_hover_text(super::i18n::registered(
                                                locale,
                                                "section-manager.009",
                                            ))
                                            .clicked()
                                        {
                                            action = Some(Action::Remove(section.id, fallback));
                                        }
                                    }
                                });
                            });
                        }
                    });
                ui.separator();
                ui.strong(super::i18n::registered(locale, "section-manager.010"));
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_name)
                            .hint_text(super::i18n::registered(locale, "section-manager.011")),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_short)
                            .desired_width(80.0)
                            .hint_text("CG"),
                    );
                    let valid =
                        !self.new_name.trim().is_empty() && !self.new_short.trim().is_empty();
                    if ui
                        .add_enabled(
                            valid,
                            egui::Button::new(super::i18n::registered(
                                locale,
                                "section-manager.012",
                            )),
                        )
                        .clicked()
                    {
                        action = Some(Action::Add {
                            name: self.new_name.trim().to_owned(),
                            short: self.new_short.trim().to_owned(),
                        });
                    }
                });
            });
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successful_add_clears_inputs_but_keeps_window_open() {
        let mut manager = SectionManager {
            open: true,
            new_name: "Brass".into(),
            new_short: "BR".into(),
            ..SectionManager::default()
        };
        manager.section_added();
        assert!(manager.new_name.is_empty());
        assert!(manager.new_short.is_empty());
        assert!(manager.open);
    }

    #[test]
    fn removing_a_section_invalidates_only_its_draft() {
        let first = SectionId::new(1).unwrap();
        let second = SectionId::new(2).unwrap();
        let mut manager = SectionManager::default();
        manager.drafts.insert(first, ("A".into(), "A".into()));
        manager.drafts.insert(second, ("B".into(), "B".into()));
        manager.section_removed(first);
        assert!(!manager.drafts.contains_key(&first));
        assert!(manager.drafts.contains_key(&second));
    }
}
