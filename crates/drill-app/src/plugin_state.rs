use super::i18n::StatusMessage;
use drill_core::Locale;
use drill_plugin::{Capability, HostCommand, PluginManifest};
use eframe::egui;
use std::{collections::BTreeSet, fs, path::PathBuf};

#[derive(Default)]
pub struct PluginUiState {
    pub open: bool,
    entries: Vec<Entry>,
    selected: Option<usize>,
    status: StatusMessage,
}
struct Entry {
    manifest: PluginManifest,
    executable: PathBuf,
    hash_matches: bool,
    trusted: bool,
    approved: BTreeSet<Capability>,
}
impl PluginUiState {
    fn add(&mut self, manifest_path: PathBuf, executable: PathBuf) {
        let result = (|| {
            let manifest: PluginManifest = serde_json::from_slice(&fs::read(manifest_path)?)?;
            let component = fs::read(&executable)?;
            let hash_matches =
                blake3::hash(&component).to_hex().as_str() == manifest.integrity.component_blake3;
            Ok::<_, Box<dyn std::error::Error>>(Entry {
                manifest,
                executable,
                hash_matches,
                trusted: false,
                approved: BTreeSet::new(),
            })
        })();
        match result {
            Ok(entry) => {
                self.entries.push(entry);
                self.selected = Some(self.entries.len() - 1);
                self.status = StatusMessage::new("plugin-status.001");
            }
            Err(error) => self.status = StatusMessage::new("plugin-status.002").arg(0, error),
        }
    }
    pub fn show(&mut self, context: &egui::Context, locale: Locale) {
        if !self.open {
            return;
        }
        let mut open = self.open;
        egui::Window::new(super::i18n::registered(locale, "plugin-state.001"))
            .open(&mut open)
            .default_width(680.0)
            .show(context, |ui| {
                ui.label(super::i18n::registered(locale, "plugin-state.002"));
                if ui
                    .button(super::i18n::registered(locale, "plugin-state.003"))
                    .clicked()
                    && let Some(manifest) = rfd::FileDialog::new()
                        .add_filter("Plugin manifest", &["json"])
                        .pick_file()
                    && let Some(executable) = rfd::FileDialog::new().pick_file()
                {
                    self.add(manifest, executable);
                }
                ui.separator();
                ui.columns(2, |columns| {
                    columns[0].heading(super::i18n::registered(locale, "plugin-state.004"));
                    for (index, entry) in self.entries.iter().enumerate() {
                        columns[0].selectable_value(
                            &mut self.selected,
                            Some(index),
                            format!("{}  {}", entry.manifest.id, entry.manifest.version),
                        );
                    }
                    columns[1].heading(super::i18n::registered(locale, "plugin-state.005"));
                    let Some(entry) = self.selected.and_then(|i| self.entries.get_mut(i)) else {
                        columns[1].label(super::i18n::registered(locale, "plugin-state.006"));
                        return;
                    };
                    columns[1].label(format!("Publisher: {}", entry.manifest.publisher));
                    columns[1].label(format!("Executable: {}", entry.executable.display()));
                    columns[1].colored_label(
                        if entry.hash_matches {
                            egui::Color32::LIGHT_GREEN
                        } else {
                            egui::Color32::LIGHT_RED
                        },
                        if entry.hash_matches {
                            "BLAKE3: verified"
                        } else {
                            "BLAKE3: MISMATCH — execution blocked"
                        },
                    );
                    columns[1].checkbox(
                        &mut entry.trusted,
                        super::i18n::registered(locale, "plugin-state.007"),
                    );
                    columns[1].separator();
                    for cap in &entry.manifest.requested {
                        let mut approved = entry.approved.contains(cap);
                        if columns[1]
                            .checkbox(&mut approved, format!("{cap:?}"))
                            .changed()
                        {
                            if approved {
                                entry.approved.insert(*cap);
                            } else {
                                entry.approved.remove(cap);
                            }
                        }
                    }
                    columns[1].separator();
                    columns[1].label(super::i18n::registered(locale, "plugin-state.008"));
                    let mut preview =
                        serde_json::to_string_pretty(&HostCommand::Describe).unwrap_or_default();
                    columns[1].add(
                        egui::TextEdit::multiline(&mut preview)
                            .code_editor()
                            .interactive(false)
                            .desired_rows(4),
                    );
                    let ready = entry.hash_matches
                        && entry.trusted
                        && entry.approved == entry.manifest.requested;
                    if columns[1]
                        .add_enabled(
                            ready,
                            egui::Button::new(super::i18n::registered(locale, "plugin-state.010")),
                        )
                        .on_disabled_hover_text(super::i18n::registered(locale, "plugin-state.009"))
                        .clicked()
                    {
                        self.status = StatusMessage::new("plugin-state.011");
                    }
                });
                if !self.status.is_empty() {
                    ui.separator();
                    ui.small(self.status.text(locale));
                }
            });
        self.open = open;
    }
}
