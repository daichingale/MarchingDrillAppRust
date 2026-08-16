//! A compact, keyboard-first command surface for the desktop editor.
//!
//! The palette intentionally reads the same [`commands::SPECS`] table as the
//! menu bar and shortcuts. This keeps every action discoverable without
//! creating a second, slowly-diverging command registry.

use super::commands::{self, Command, Context, Spec};
use super::i18n;
use drill_core::Locale;
use eframe::egui;

#[derive(Default)]
pub(crate) struct CommandPalette {
    open: bool,
    query: String,
    selected: usize,
    focus_query: bool,
}

impl CommandPalette {
    /// Opens only when the canvas owns the keyboard. Cmd/Ctrl+K must retain
    /// its native meaning while naming a set, editing a marker, or typing in
    /// any other text field.
    pub(crate) fn open_if_requested(&mut self, context: &egui::Context) {
        if self.open || context.egui_wants_keyboard_input() || context.text_edit_focused() {
            return;
        }
        let requested = context.input_mut(|input| {
            input.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                egui::Key::K,
            ))
        });
        if requested {
            self.open = true;
            self.query.clear();
            self.selected = 0;
            self.focus_query = true;
        }
    }

    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    pub(crate) fn show(
        &mut self,
        context: &egui::Context,
        command_context: Context,
        locale: Locale,
    ) -> Option<Command> {
        if !self.open {
            return None;
        }

        let candidates = matching_specs(&self.query, locale);
        self.selected = selected_enabled_index(&candidates, command_context, locale, self.selected);
        let mut chosen = None;
        let mut close = false;

        context.input_mut(|input| {
            if input.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                close = true;
            }
            if !candidates.is_empty()
                && input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)
            {
                self.selected =
                    next_enabled_index(&candidates, command_context, locale, self.selected, 1);
            }
            if !candidates.is_empty()
                && input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)
            {
                self.selected =
                    next_enabled_index(&candidates, command_context, locale, self.selected, -1);
            }
            if input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                && let Some(spec) = candidates.get(self.selected)
                && spec.command.enabled(command_context, locale).is_ok()
            {
                chosen = Some(spec.command);
                close = true;
            }
        });

        let mut open = true;
        egui::Window::new(i18n::registered(locale, "command-palette.001"))
            .id(egui::Id::new("command-palette"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size(egui::vec2(620.0, 430.0))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(context, |ui| {
                let query_id = ui.make_persistent_id("command-palette-query");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .id(query_id)
                        .hint_text(i18n::registered(locale, "command-palette.002"))
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

                if candidates.is_empty() {
                    ui.centered_and_justified(|ui| {
                        ui.label(i18n::registered(locale, "command-palette.003"));
                    });
                    return;
                }
                egui::ScrollArea::vertical()
                    .id_salt("command-palette-results")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (index, spec) in candidates.iter().enumerate() {
                            let enabled = spec.command.enabled(command_context, locale);
                            let selected = index == self.selected;
                            let shortcut = spec
                                .shortcut
                                .map(|shortcut| context.format_shortcut(&shortcut.value()))
                                .unwrap_or_default();
                            let label = if shortcut.is_empty() {
                                spec.command.label(locale).to_owned()
                            } else {
                                format!("{}    {shortcut}", spec.command.label(locale))
                            };
                            let response = ui.add_enabled(
                                enabled.is_ok(),
                                egui::Button::new(label).selected(selected).frame(false),
                            );
                            let response = match enabled {
                                Ok(()) => response,
                                Err(reason) => response.on_disabled_hover_text(reason),
                            };
                            if response.hovered() && enabled.is_ok() {
                                self.selected = index;
                            }
                            if response.clicked() {
                                chosen = Some(spec.command);
                                close = true;
                            }
                        }
                    });
                ui.separator();
                ui.small(i18n::registered(locale, "command-palette.004"));
            });

        if !open || close {
            self.open = false;
            self.focus_query = false;
        }
        chosen
    }
}

fn matching_specs(query: &str, locale: Locale) -> Vec<&'static Spec> {
    let needle = query.trim().to_lowercase();
    commands::SPECS
        .iter()
        .filter(|spec| {
            needle.is_empty() || spec.command.label(locale).to_lowercase().contains(&needle)
        })
        .collect()
}

fn selected_enabled_index(
    candidates: &[&Spec],
    context: Context,
    locale: Locale,
    selected: usize,
) -> usize {
    if candidates.is_empty() {
        return 0;
    }
    let selected = selected.min(candidates.len() - 1);
    if candidates[selected]
        .command
        .enabled(context, locale)
        .is_ok()
    {
        selected
    } else {
        next_enabled_index(candidates, context, locale, selected, 1)
    }
}

fn next_enabled_index(
    candidates: &[&Spec],
    context: Context,
    locale: Locale,
    current: usize,
    direction: isize,
) -> usize {
    if candidates.is_empty() {
        return 0;
    }
    let len = candidates.len() as isize;
    for offset in 1..=len {
        let index = (current as isize + direction * offset).rem_euclid(len) as usize;
        if candidates[index].command.enabled(context, locale).is_ok() {
            return index;
        }
    }
    current.min(candidates.len() - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_covers_creation_playback_and_export_workflows() {
        assert!(
            matching_specs("grid", Locale::En)
                .iter()
                .any(|spec| spec.command == Command::FocusGrid)
        );
        assert!(
            matching_specs("play", Locale::En)
                .iter()
                .any(|spec| spec.command == Command::PlayPause)
        );
        assert!(
            matching_specs("動画", Locale::Ja)
                .iter()
                .any(|spec| spec.command == Command::FocusVideo)
        );
    }

    #[test]
    fn keyboard_selection_skips_disabled_commands_and_wraps() {
        let context = Context {
            has_performers: true,
            has_sets: true,
            ..Context::default()
        };
        let candidates = matching_specs("", Locale::En);
        let undo = candidates
            .iter()
            .position(|spec| spec.command == Command::Undo)
            .unwrap();
        let next = next_enabled_index(&candidates, context, Locale::En, undo, 1);
        assert_ne!(candidates[next].command, Command::Undo);
        assert!(
            candidates[next]
                .command
                .enabled(context, Locale::En)
                .is_ok()
        );
    }
}
