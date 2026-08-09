use drill_core::Locale;
use eframe::egui;

pub fn show(context: &egui::Context, open: &mut bool, locale: Locale) {
    if !*open {
        return;
    }

    egui::Window::new(super::i18n::registered(locale, "legal-notices.001"))
        .open(open)
        .default_width(760.0)
        .default_height(560.0)
        .show(context, |ui| {
            ui.label(super::i18n::registered(locale, "legal-notices.002"));
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.collapsing("DrillForge — MIT OR Apache-2.0", |ui| {
                    ui.monospace(include_str!("../../../LICENSE"));
                });
                ui.collapsing("MIT License", |ui| {
                    ui.monospace(include_str!("../../../LICENSE-MIT"));
                });
                ui.collapsing("Apache License 2.0", |ui| {
                    ui.monospace(include_str!("../../../LICENSE-APACHE"));
                });
                ui.collapsing("Third-party notices", |ui| {
                    ui.monospace(include_str!("../../../THIRD_PARTY_NOTICES.md"));
                });
                ui.collapsing("Noto Sans JP — SIL OFL 1.1", |ui| {
                    ui.monospace(include_str!("../../../assets/OFL-NotoSansJP.txt"));
                });
            });
        });
}
