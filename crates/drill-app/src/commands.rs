use drill_core::Locale;
use eframe::egui;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Menu {
    Edit,
    Set,
    Playback,
    Workspace,
    View,
    Help,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Undo,
    Redo,
    SelectAll,
    ClearSelection,
    DuplicateSet,
    ManageSections,
    PlayPause,
    RangeStart,
    RangeCurrentSet,
    RangeWholeShow,
    FocusPerformerTools,
    FocusClinic,
    FocusGrid,
    FocusTempo,
    FocusVideo,
    OpenPrint,
    FocusAudio,
    View2d,
    View3d,
    ToggleGuidance,
    GettingStarted,
    LegalNotices,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Context {
    pub can_undo: bool,
    pub can_redo: bool,
    pub has_performers: bool,
    pub has_selection: bool,
    pub has_sets: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct Spec {
    pub command: Command,
    pub menu: Menu,
    pub shortcut: Option<Shortcut>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Shortcut {
    Command(egui::Key),
    Plain(egui::Key),
}

impl Shortcut {
    pub(crate) fn value(self) -> egui::KeyboardShortcut {
        match self {
            Self::Command(key) => egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, key),
            Self::Plain(key) => egui::KeyboardShortcut::new(egui::Modifiers::NONE, key),
        }
    }

    /// Resolve a physical key chord without depending on egui frame state.
    /// This is the authoritative mapping used by both the UI and semantic QA.
    #[cfg(test)]
    pub(crate) fn matches(self, modifiers: egui::Modifiers, key: egui::Key) -> bool {
        let expected = self.value();
        expected.logical_key == key
            && match self {
                Self::Command(_) => modifiers.command && !modifiers.alt && !modifiers.shift,
                Self::Plain(_) => modifiers.is_none(),
            }
    }
}

/// Returns the command bound to a chord. Application shortcuts are deliberately
/// suspended while an editor owns the keyboard, so Space/Escape/Cmd+A retain
/// their native text-edit meanings.
#[cfg(test)]
pub(crate) fn command_for_chord(
    modifiers: egui::Modifiers,
    key: egui::Key,
    editor_owns_keyboard: bool,
) -> Option<Command> {
    if editor_owns_keyboard {
        return None;
    }
    SPECS.iter().find_map(|spec| {
        spec.shortcut
            .filter(|shortcut| shortcut.matches(modifiers, key))
            .map(|_| spec.command)
    })
}

/// Consume at most one enabled application command in deterministic spec order.
pub(crate) fn consume_shortcut(
    ui: &mut egui::Ui,
    context: Context,
    locale: Locale,
) -> Option<Command> {
    if ui.ctx().egui_wants_keyboard_input() || ui.ctx().text_edit_focused() {
        return None;
    }
    SPECS.iter().find_map(|spec| {
        let shortcut = spec.shortcut?;
        if spec.command.enabled(context, locale).is_ok()
            && ui.input_mut(|input| input.consume_shortcut(&shortcut.value()))
        {
            Some(spec.command)
        } else {
            None
        }
    })
}

pub(crate) const SPECS: &[Spec] = &[
    Spec {
        command: Command::Undo,
        menu: Menu::Edit,
        shortcut: Some(Shortcut::Command(egui::Key::Z)),
    },
    Spec {
        command: Command::Redo,
        menu: Menu::Edit,
        shortcut: Some(Shortcut::Command(egui::Key::Y)),
    },
    Spec {
        command: Command::SelectAll,
        menu: Menu::Edit,
        shortcut: Some(Shortcut::Command(egui::Key::A)),
    },
    Spec {
        command: Command::ClearSelection,
        menu: Menu::Edit,
        shortcut: Some(Shortcut::Plain(egui::Key::Escape)),
    },
    Spec {
        command: Command::FocusPerformerTools,
        menu: Menu::Edit,
        shortcut: None,
    },
    Spec {
        command: Command::DuplicateSet,
        menu: Menu::Set,
        shortcut: Some(Shortcut::Command(egui::Key::D)),
    },
    Spec {
        command: Command::ManageSections,
        menu: Menu::Set,
        shortcut: None,
    },
    Spec {
        command: Command::PlayPause,
        menu: Menu::Playback,
        shortcut: Some(Shortcut::Plain(egui::Key::Space)),
    },
    Spec {
        command: Command::RangeStart,
        menu: Menu::Playback,
        shortcut: Some(Shortcut::Plain(egui::Key::Home)),
    },
    Spec {
        command: Command::RangeCurrentSet,
        menu: Menu::Playback,
        shortcut: None,
    },
    Spec {
        command: Command::RangeWholeShow,
        menu: Menu::Playback,
        shortcut: None,
    },
    Spec {
        command: Command::FocusGrid,
        menu: Menu::Workspace,
        shortcut: None,
    },
    Spec {
        command: Command::FocusTempo,
        menu: Menu::Workspace,
        shortcut: None,
    },
    Spec {
        command: Command::FocusAudio,
        menu: Menu::Workspace,
        shortcut: None,
    },
    Spec {
        command: Command::FocusVideo,
        menu: Menu::Workspace,
        shortcut: None,
    },
    Spec {
        command: Command::OpenPrint,
        menu: Menu::Workspace,
        shortcut: Some(Shortcut::Command(egui::Key::P)),
    },
    Spec {
        command: Command::FocusClinic,
        menu: Menu::Workspace,
        shortcut: None,
    },
    Spec {
        command: Command::View2d,
        menu: Menu::View,
        shortcut: None,
    },
    Spec {
        command: Command::View3d,
        menu: Menu::View,
        shortcut: None,
    },
    Spec {
        command: Command::ToggleGuidance,
        menu: Menu::View,
        shortcut: None,
    },
    Spec {
        command: Command::GettingStarted,
        menu: Menu::Help,
        shortcut: Some(Shortcut::Plain(egui::Key::F1)),
    },
    Spec {
        command: Command::LegalNotices,
        menu: Menu::Help,
        shortcut: None,
    },
];

impl Command {
    pub(crate) fn label(self, locale: Locale) -> &'static str {
        use Command::*;
        match (locale, self) {
            (Locale::Ja, Undo) => "元に戻す",
            (Locale::En, Undo) => "Undo",
            (Locale::Ja, Redo) => "やり直す",
            (Locale::En, Redo) => "Redo",
            (Locale::Ja, SelectAll) => "全員を選択",
            (Locale::En, SelectAll) => "Select All",
            (Locale::Ja, ClearSelection) => "選択を解除",
            (Locale::En, ClearSelection) => "Clear Selection",
            (Locale::Ja, DuplicateSet) => "現在のセットを複製",
            (Locale::En, DuplicateSet) => "Duplicate Current Set",
            (Locale::Ja, ManageSections) => "セクション管理…",
            (Locale::En, ManageSections) => "Manage Sections…",
            (Locale::Ja, PlayPause) => "再生／一時停止",
            (Locale::En, PlayPause) => "Play / Pause",
            (Locale::Ja, RangeStart) => "再生範囲の先頭へ",
            (Locale::En, RangeStart) => "Go to Range Start",
            (Locale::Ja, RangeCurrentSet) => "現在のセットを再生範囲に",
            (Locale::En, RangeCurrentSet) => "Range: Current Set",
            (Locale::Ja, RangeWholeShow) => "曲全体を再生範囲に",
            (Locale::En, RangeWholeShow) => "Range: Whole Show",
            (Locale::Ja, FocusPerformerTools) => "隊形編集ツールを表示",
            (Locale::En, FocusPerformerTools) => "Show Formation Tools",
            (Locale::Ja, FocusClinic) => "LIVE CLINICを表示",
            (Locale::En, FocusClinic) => "Show Live Clinic",
            (Locale::Ja, FocusGrid) => "グリッドデザイナーを表示",
            (Locale::En, FocusGrid) => "Show Grid Designer",
            (Locale::Ja, FocusTempo) => "テンポマップを表示",
            (Locale::En, FocusTempo) => "Show Tempo Map",
            (Locale::Ja, FocusVideo) => "動画書き出しを表示",
            (Locale::En, FocusVideo) => "Show Video Export",
            (Locale::Ja, OpenPrint) => "印刷・PDFワークスペース…",
            (Locale::En, OpenPrint) => "Print & PDF Workspace…",
            (Locale::Ja, FocusAudio) => "音源・クリックを表示",
            (Locale::En, FocusAudio) => "Show Audio & Click",
            (Locale::Ja, View2d) => "2Dフィールド",
            (Locale::En, View2d) => "2D Field",
            (Locale::Ja, View3d) => "3Dスタジアム",
            (Locale::En, View3d) => "3D Stadium",
            (Locale::Ja, ToggleGuidance) => "操作ガイドを切替",
            (Locale::En, ToggleGuidance) => "Toggle Guidance",
            (Locale::Ja, GettingStarted) => "はじめかた・全ショートカット…",
            (Locale::En, GettingStarted) => "Getting Started & Shortcuts…",
            (Locale::Ja, LegalNotices) => "ライセンス・第三者通知…",
            (Locale::En, LegalNotices) => "Licenses & Third-party Notices…",
        }
    }

    pub(crate) fn enabled(self, context: Context, locale: Locale) -> Result<(), &'static str> {
        use Command::*;
        match self {
            Undo if !context.can_undo => Err(match locale {
                Locale::Ja => "元に戻せる操作がありません",
                Locale::En => "Nothing to undo",
            }),
            Redo if !context.can_redo => Err(match locale {
                Locale::Ja => "やり直せる操作がありません",
                Locale::En => "Nothing to redo",
            }),
            SelectAll if !context.has_performers => Err(match locale {
                Locale::Ja => "演者がいません",
                Locale::En => "No performers",
            }),
            ClearSelection | FocusPerformerTools if !context.has_selection => Err(match locale {
                Locale::Ja => "先に演者を選択してください",
                Locale::En => "Select performers first",
            }),
            DuplicateSet | RangeCurrentSet if !context.has_sets => Err(match locale {
                Locale::Ja => "セットがありません",
                Locale::En => "No sets",
            }),
            _ => Ok(()),
        }
    }
}

pub(crate) fn specs(menu: Menu) -> impl Iterator<Item = &'static Spec> {
    SPECS.iter().filter(move |spec| spec.menu == menu)
}

/// Renders one native desktop menu and returns the selected command.
/// Command execution deliberately remains in the application controller.
pub(crate) fn show_menu(
    ui: &mut egui::Ui,
    menu: Menu,
    context: Context,
    locale: Locale,
) -> Option<Command> {
    let mut chosen = None;
    for spec in specs(menu) {
        let label = if let Some(shortcut) = spec.shortcut {
            format!(
                "{}    {}",
                spec.command.label(locale),
                ui.ctx().format_shortcut(&shortcut.value())
            )
        } else {
            spec.command.label(locale).to_owned()
        };
        let enabled = spec.command.enabled(context, locale);
        let response = ui.add_enabled(enabled.is_ok(), egui::Button::new(&label));
        if let Err(reason) = enabled {
            // Tooltips help pointer users; the semantic label also explains to
            // screen-reader users why an otherwise discoverable command cannot run.
            let accessible_label = format!("{label}. {reason}");
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, false, accessible_label.clone())
            });
        }
        let response = if let Err(reason) = enabled {
            response.on_disabled_hover_text(reason)
        } else {
            response
        };
        if response.clicked() {
            chosen = Some(spec.command);
        }
    }
    if chosen.is_some() {
        ui.close();
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn command_table_is_unique_and_fully_translated() {
        let mut commands = std::collections::HashSet::new();
        for spec in SPECS {
            assert!(commands.insert(spec.command as u8));
            assert!(!spec.command.label(Locale::Ja).is_empty());
            assert!(!spec.command.label(Locale::En).is_empty());
            assert_ne!(
                spec.command.label(Locale::Ja),
                spec.command.label(Locale::En)
            );
        }
        let unavailable = Context::default();
        for spec in SPECS {
            if let Err(message) = spec.command.enabled(unavailable, Locale::En) {
                assert!(
                    message.is_ascii(),
                    "English command error leaked non-ASCII text: {message}"
                );
            }
        }
    }

    #[test]
    fn every_product_workspace_is_discoverable() {
        for command in [
            Command::FocusPerformerTools,
            Command::ManageSections,
            Command::FocusGrid,
            Command::FocusAudio,
            Command::OpenPrint,
            Command::FocusVideo,
            Command::FocusClinic,
            Command::View2d,
            Command::View3d,
        ] {
            assert!(SPECS.iter().any(|spec| spec.command == command));
        }
    }

    #[test]
    fn every_shortcut_round_trips_and_no_chord_is_ambiguous() {
        let mut chords = std::collections::HashSet::new();
        for spec in SPECS.iter().filter(|spec| spec.shortcut.is_some()) {
            let shortcut = spec.shortcut.unwrap();
            assert!(chords.insert(shortcut), "duplicate chord: {shortcut:?}");
            let modifiers = match shortcut {
                Shortcut::Command(_) => egui::Modifiers::COMMAND,
                Shortcut::Plain(_) => egui::Modifiers::NONE,
            };
            let key = shortcut.value().logical_key;
            assert_eq!(command_for_chord(modifiers, key, false), Some(spec.command));
            assert_eq!(command_for_chord(modifiers, key, true), None);
        }
    }

    #[test]
    fn enabled_contract_has_localized_reason_and_recovers() {
        let unavailable = Context::default();
        let available = Context {
            can_undo: true,
            can_redo: true,
            has_performers: true,
            has_selection: true,
            has_sets: true,
        };
        for spec in SPECS {
            assert!(spec.command.enabled(available, Locale::Ja).is_ok());
            assert!(spec.command.enabled(available, Locale::En).is_ok());
            if let Err(ja) = spec.command.enabled(unavailable, Locale::Ja) {
                let en = spec.command.enabled(unavailable, Locale::En).unwrap_err();
                assert!(!ja.is_empty() && !en.is_empty());
                assert_ne!(ja, en);
                assert!(en.is_ascii());
            }
        }
    }
}
