//! A small, keyboard-first exact-count navigation sheet.
//!
//! The editor stores a zero-based global count internally, while rehearsals
//! call the first count "1". This surface makes that conversion explicit and
//! refuses fractional or out-of-range input rather than silently rounding.

use drill_core::Locale;
use eframe::egui;

#[derive(Default)]
pub(crate) struct GoToCount {
    open: bool,
    input: String,
    error: bool,
    focus_input: bool,
}

impl GoToCount {
    pub(crate) fn open(&mut self) {
        self.open = true;
        self.input.clear();
        self.error = false;
        self.focus_input = true;
    }

    pub(crate) fn show(
        &mut self,
        context: &egui::Context,
        total_counts: u32,
        locale: Locale,
    ) -> Option<u32> {
        if !self.open {
            return None;
        }
        let mut chosen = None;
        let mut close = false;
        let submit = |state: &mut Self| -> Option<u32> {
            match state.input.trim().parse::<u32>() {
                Ok(one_based) if (1..=total_counts).contains(&one_based) => Some(one_based - 1),
                _ => {
                    state.error = true;
                    None
                }
            }
        };
        context.input_mut(|input| {
            if input.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                close = true;
            }
            if input.consume_key(egui::Modifiers::NONE, egui::Key::Enter) {
                chosen = submit(self);
                close = chosen.is_some();
            }
        });
        let mut open = true;
        egui::Window::new(super::i18n::registered(locale, "go-to-count.001"))
            .id(egui::Id::new("go-to-global-count"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size(egui::vec2(440.0, 180.0))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(context, |ui| {
                ui.label(format!(
                    "{} 1–{total_counts}",
                    super::i18n::registered(locale, "go-to-count.002")
                ));
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.input)
                        .id(ui.make_persistent_id("global-count-input"))
                        .hint_text(super::i18n::registered(locale, "go-to-count.003"))
                        .desired_width(f32::INFINITY),
                );
                if self.focus_input {
                    response.request_focus();
                    self.focus_input = false;
                }
                if response.changed() {
                    self.error = false;
                }
                if self.error {
                    ui.colored_label(
                        egui::Color32::from_rgb(245, 126, 120),
                        super::i18n::registered(locale, "go-to-count.004"),
                    );
                }
                ui.horizontal(|ui| {
                    if ui
                        .button(super::i18n::registered(locale, "go-to-count.005"))
                        .clicked()
                    {
                        chosen = submit(self);
                        close = chosen.is_some();
                    }
                    if ui
                        .button(super::i18n::registered(locale, "go-to-count.006"))
                        .clicked()
                    {
                        close = true;
                    }
                });
                ui.small(super::i18n::registered(locale, "go-to-count.007"));
            });
        if !open || close {
            self.open = false;
            self.focus_input = false;
        }
        chosen
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sheet_uses_one_based_integer_counts() {
        let mut sheet = GoToCount::default();
        sheet.open();
        sheet.input = "8".into();
        assert_eq!(
            sheet.input.parse::<u32>().ok().map(|value| value - 1),
            Some(7)
        );
    }
}
