use drill_core::Locale;
use eframe::egui;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Menu {
    File,
    Edit,
    Arrange,
    Set,
    Playback,
    Workspace,
    View,
    Help,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    OpenDocument,
    OpenProject,
    OpenRecent,
    Save,
    SaveAs,
    SaveProjectAs,
    ImportCoordinates,
    ImportMusicalTimeline,
    LoadImageUnderlay,
    Undo,
    Redo,
    SelectAll,
    ClearSelection,
    RestoreRecentSelection,
    CopyFormation,
    PasteFormation,
    AlignHorizontal,
    AlignVertical,
    DistributeHorizontal,
    DistributeVertical,
    FlipHorizontal,
    FlipVertical,
    MakeLine,
    LockSelection,
    HideSelection,
    DuplicateSet,
    ManageSections,
    PlayPause,
    RangeStart,
    RangeCurrentSet,
    RangeWholeShow,
    MarkRangeStart,
    MarkRangeEnd,
    PreviousProductionMarker,
    NextProductionMarker,
    PreviousSet,
    NextSet,
    GoToGlobalCount,
    FocusPerformerTools,
    FocusClinic,
    FocusGrid,
    FocusTempo,
    FocusVideo,
    OpenPrint,
    FocusAudio,
    OpenProductionSheet,
    View2d,
    View3d,
    ToggleFocusField,
    WorkspaceDesign,
    WorkspaceReview,
    WorkspacePresent,
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
    pub has_recent_selection: bool,
    pub has_formation_clipboard: bool,
    pub has_multiple_selection: bool,
    pub can_edit_selection: bool,
    pub has_sets: bool,
    pub has_previous_production_marker: bool,
    pub has_next_production_marker: bool,
    pub has_previous_set: bool,
    pub has_next_set: bool,
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
    CommandShift(egui::Key),
    CommandAlt(egui::Key),
    Alt(egui::Key),
    Plain(egui::Key),
}

impl Shortcut {
    pub(crate) fn value(self) -> egui::KeyboardShortcut {
        match self {
            Self::Command(key) => egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, key),
            Self::CommandShift(key) => {
                egui::KeyboardShortcut::new(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, key)
            }
            Self::CommandAlt(key) => {
                egui::KeyboardShortcut::new(egui::Modifiers::COMMAND | egui::Modifiers::ALT, key)
            }
            Self::Alt(key) => egui::KeyboardShortcut::new(egui::Modifiers::ALT, key),
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
                Self::CommandShift(_) => modifiers.command && modifiers.shift && !modifiers.alt,
                Self::CommandAlt(_) => modifiers.command && modifiers.alt && !modifiers.shift,
                Self::Alt(_) => modifiers.alt && !modifiers.command && !modifiers.shift,
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
        command: Command::OpenDocument,
        menu: Menu::File,
        shortcut: Some(Shortcut::Command(egui::Key::O)),
    },
    Spec {
        command: Command::OpenProject,
        menu: Menu::File,
        shortcut: None,
    },
    Spec {
        command: Command::OpenRecent,
        menu: Menu::File,
        shortcut: None,
    },
    Spec {
        command: Command::Save,
        menu: Menu::File,
        shortcut: Some(Shortcut::Command(egui::Key::S)),
    },
    Spec {
        command: Command::SaveAs,
        menu: Menu::File,
        shortcut: Some(Shortcut::CommandShift(egui::Key::S)),
    },
    Spec {
        command: Command::SaveProjectAs,
        menu: Menu::File,
        shortcut: None,
    },
    Spec {
        command: Command::ImportCoordinates,
        menu: Menu::File,
        shortcut: None,
    },
    Spec {
        command: Command::ImportMusicalTimeline,
        menu: Menu::File,
        shortcut: None,
    },
    Spec {
        command: Command::LoadImageUnderlay,
        menu: Menu::File,
        shortcut: None,
    },
    Spec {
        command: Command::Undo,
        menu: Menu::Edit,
        shortcut: Some(Shortcut::Command(egui::Key::Z)),
    },
    Spec {
        command: Command::Redo,
        menu: Menu::Edit,
        // Cmd+Shift+Z is the native macOS redo chord. `COMMAND` maps to Ctrl
        // on Windows/Linux, so the same discoverable gesture remains portable.
        shortcut: Some(Shortcut::CommandShift(egui::Key::Z)),
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
        command: Command::RestoreRecentSelection,
        menu: Menu::Edit,
        shortcut: None,
    },
    Spec {
        command: Command::CopyFormation,
        menu: Menu::Arrange,
        shortcut: Some(Shortcut::Command(egui::Key::C)),
    },
    Spec {
        command: Command::PasteFormation,
        menu: Menu::Arrange,
        shortcut: Some(Shortcut::Command(egui::Key::V)),
    },
    Spec {
        command: Command::AlignHorizontal,
        menu: Menu::Arrange,
        shortcut: None,
    },
    Spec {
        command: Command::AlignVertical,
        menu: Menu::Arrange,
        shortcut: None,
    },
    Spec {
        command: Command::DistributeHorizontal,
        menu: Menu::Arrange,
        shortcut: None,
    },
    Spec {
        command: Command::DistributeVertical,
        menu: Menu::Arrange,
        shortcut: None,
    },
    Spec {
        command: Command::FlipHorizontal,
        menu: Menu::Arrange,
        shortcut: None,
    },
    Spec {
        command: Command::FlipVertical,
        menu: Menu::Arrange,
        shortcut: None,
    },
    Spec {
        command: Command::MakeLine,
        menu: Menu::Arrange,
        shortcut: None,
    },
    Spec {
        command: Command::LockSelection,
        menu: Menu::Arrange,
        shortcut: None,
    },
    Spec {
        command: Command::HideSelection,
        menu: Menu::Arrange,
        shortcut: None,
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
        command: Command::MarkRangeStart,
        menu: Menu::Playback,
        shortcut: Some(Shortcut::Plain(egui::Key::I)),
    },
    Spec {
        command: Command::MarkRangeEnd,
        menu: Menu::Playback,
        shortcut: Some(Shortcut::Plain(egui::Key::O)),
    },
    Spec {
        command: Command::PreviousProductionMarker,
        menu: Menu::Playback,
        shortcut: Some(Shortcut::Alt(egui::Key::ArrowLeft)),
    },
    Spec {
        command: Command::NextProductionMarker,
        menu: Menu::Playback,
        shortcut: Some(Shortcut::Alt(egui::Key::ArrowRight)),
    },
    Spec {
        command: Command::PreviousSet,
        menu: Menu::Playback,
        shortcut: Some(Shortcut::CommandAlt(egui::Key::ArrowLeft)),
    },
    Spec {
        command: Command::NextSet,
        menu: Menu::Playback,
        shortcut: Some(Shortcut::CommandAlt(egui::Key::ArrowRight)),
    },
    Spec {
        command: Command::GoToGlobalCount,
        menu: Menu::Playback,
        shortcut: Some(Shortcut::Command(egui::Key::G)),
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
        command: Command::OpenProductionSheet,
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
        command: Command::ToggleFocusField,
        menu: Menu::View,
        shortcut: Some(Shortcut::CommandShift(egui::Key::F)),
    },
    Spec {
        command: Command::WorkspaceDesign,
        menu: Menu::View,
        shortcut: None,
    },
    Spec {
        command: Command::WorkspaceReview,
        menu: Menu::View,
        shortcut: None,
    },
    Spec {
        command: Command::WorkspacePresent,
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
            (_, OpenDocument) => super::i18n::registered(locale, "commands.115"),
            (_, OpenProject) => super::i18n::registered(locale, "commands.116"),
            (_, OpenRecent) => super::i18n::registered(locale, "recent-projects.013"),
            (_, Save) => super::i18n::registered(locale, "commands.117"),
            (_, SaveAs) => super::i18n::registered(locale, "commands.118"),
            (_, SaveProjectAs) => super::i18n::registered(locale, "commands.119"),
            (_, ImportCoordinates) => super::i18n::registered(locale, "commands.120"),
            (_, ImportMusicalTimeline) => super::i18n::registered(locale, "commands.121"),
            (_, LoadImageUnderlay) => super::i18n::registered(locale, "commands.122"),
            (Locale::Ja, Undo) => "元に戻す",
            (Locale::En, Undo) => "Undo",
            (Locale::Ja, Redo) => "やり直す",
            (Locale::En, Redo) => "Redo",
            (Locale::Ja, SelectAll) => "全員を選択",
            (Locale::En, SelectAll) => "Select All",
            (Locale::Ja, ClearSelection) => "選択を解除",
            (Locale::En, ClearSelection) => "Clear Selection",
            (Locale::Ja, RestoreRecentSelection) => "直前の選択を復元",
            (Locale::En, RestoreRecentSelection) => "Restore Previous Selection",
            (_, CopyFormation) => super::i18n::registered(locale, "clipboard.009"),
            (_, PasteFormation) => super::i18n::registered(locale, "clipboard.010"),
            (_, AlignHorizontal) => super::i18n::registered(locale, "commands.123"),
            (_, AlignVertical) => super::i18n::registered(locale, "commands.124"),
            (_, DistributeHorizontal) => super::i18n::registered(locale, "commands.125"),
            (_, DistributeVertical) => super::i18n::registered(locale, "commands.126"),
            (_, FlipHorizontal) => super::i18n::registered(locale, "commands.127"),
            (_, FlipVertical) => super::i18n::registered(locale, "commands.128"),
            (_, MakeLine) => super::i18n::registered(locale, "commands.129"),
            (_, LockSelection) => super::i18n::registered(locale, "commands.130"),
            (_, HideSelection) => super::i18n::registered(locale, "commands.131"),
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
            (Locale::Ja, MarkRangeStart) => "現在位置を再生範囲の開始に設定",
            (Locale::En, MarkRangeStart) => "Mark Range Start (In)",
            (Locale::Ja, MarkRangeEnd) => "現在位置を再生範囲の終了に設定",
            (Locale::En, MarkRangeEnd) => "Mark Range End (Out)",
            (_, PreviousProductionMarker) => super::i18n::registered(locale, "commands.101"),
            (_, NextProductionMarker) => super::i18n::registered(locale, "commands.102"),
            (_, PreviousSet) => super::i18n::registered(locale, "commands.106"),
            (_, NextSet) => super::i18n::registered(locale, "commands.107"),
            (_, GoToGlobalCount) => super::i18n::registered(locale, "commands.108"),
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
            (_, OpenProductionSheet) => super::i18n::registered(locale, "commands.105"),
            (Locale::Ja, View2d) => "2Dフィールド",
            (Locale::En, View2d) => "2D Field",
            (Locale::Ja, View3d) => "3Dスタジアム",
            (Locale::En, View3d) => "3D Stadium",
            (_, ToggleFocusField) => super::i18n::registered(locale, "focus-field.001"),
            (_, WorkspaceDesign) => super::i18n::registered(locale, "workspace-preset.001"),
            (_, WorkspaceReview) => super::i18n::registered(locale, "workspace-preset.002"),
            (_, WorkspacePresent) => super::i18n::registered(locale, "workspace-preset.003"),
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
            RestoreRecentSelection if !context.has_recent_selection => Err(match locale {
                Locale::Ja => "復元できる選択がありません",
                Locale::En => "No previous selection to restore",
            }),
            CopyFormation if !context.has_selection => {
                Err(super::i18n::registered(locale, "clipboard.011"))
            }
            PasteFormation if !context.has_formation_clipboard => {
                Err(super::i18n::registered(locale, "clipboard.012"))
            }
            DistributeHorizontal | DistributeVertical | MakeLine
                if !context.has_multiple_selection =>
            {
                Err(super::i18n::registered(locale, "commands.132"))
            }
            AlignHorizontal | AlignVertical | DistributeHorizontal | DistributeVertical
            | FlipHorizontal | FlipVertical | MakeLine
                if !context.has_selection =>
            {
                Err(super::i18n::registered(locale, "commands.133"))
            }
            AlignHorizontal | AlignVertical | DistributeHorizontal | DistributeVertical
            | FlipHorizontal | FlipVertical | MakeLine
                if !context.can_edit_selection =>
            {
                Err(super::i18n::registered(locale, "commands.134"))
            }
            LockSelection | HideSelection if !context.has_selection => {
                Err(super::i18n::registered(locale, "commands.136"))
            }
            DuplicateSet | RangeCurrentSet if !context.has_sets => Err(match locale {
                Locale::Ja => "セットがありません",
                Locale::En => "No sets",
            }),
            PreviousProductionMarker if !context.has_previous_production_marker => {
                Err(super::i18n::registered(locale, "commands.103"))
            }
            NextProductionMarker if !context.has_next_production_marker => {
                Err(super::i18n::registered(locale, "commands.104"))
            }
            PreviousSet if !context.has_previous_set => {
                Err(super::i18n::registered(locale, "commands.109"))
            }
            NextSet if !context.has_next_set => {
                Err(super::i18n::registered(locale, "commands.110"))
            }
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
            Command::OpenDocument,
            Command::Save,
            Command::SaveAs,
            Command::ImportCoordinates,
            Command::ImportMusicalTimeline,
            Command::FocusPerformerTools,
            Command::ManageSections,
            Command::FocusGrid,
            Command::FocusAudio,
            Command::OpenPrint,
            Command::OpenProductionSheet,
            Command::FocusVideo,
            Command::FocusClinic,
            Command::View2d,
            Command::View3d,
            Command::ToggleFocusField,
        ] {
            assert!(SPECS.iter().any(|spec| spec.command == command));
        }
    }

    #[test]
    fn arrange_surface_keeps_all_selection_actions_discoverable() {
        let arrange = [
            Command::AlignHorizontal,
            Command::AlignVertical,
            Command::DistributeHorizontal,
            Command::DistributeVertical,
            Command::FlipHorizontal,
            Command::FlipVertical,
            Command::MakeLine,
            Command::CopyFormation,
            Command::PasteFormation,
            Command::LockSelection,
            Command::HideSelection,
        ];
        for command in arrange {
            assert!(
                SPECS
                    .iter()
                    .any(|spec| { spec.command == command && spec.menu == Menu::Arrange })
            );
        }
        let no_selection = Context {
            has_formation_clipboard: true,
            can_edit_selection: true,
            ..Context::default()
        };
        assert!(
            Command::AlignHorizontal
                .enabled(no_selection, Locale::En)
                .is_err()
        );
        let not_at_set_start = Context {
            has_selection: true,
            has_multiple_selection: true,
            ..Context::default()
        };
        assert!(
            Command::MakeLine
                .enabled(not_at_set_start, Locale::En)
                .is_err()
        );
    }

    #[test]
    fn every_shortcut_round_trips_and_no_chord_is_ambiguous() {
        let mut chords = std::collections::HashSet::new();
        for spec in SPECS.iter().filter(|spec| spec.shortcut.is_some()) {
            let shortcut = spec.shortcut.unwrap();
            assert!(chords.insert(shortcut), "duplicate chord: {shortcut:?}");
            let modifiers = match shortcut {
                Shortcut::Command(_) => egui::Modifiers::COMMAND,
                Shortcut::CommandShift(_) => egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                Shortcut::CommandAlt(_) => egui::Modifiers::COMMAND | egui::Modifiers::ALT,
                Shortcut::Alt(_) => egui::Modifiers::ALT,
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
            has_recent_selection: true,
            has_formation_clipboard: true,
            has_multiple_selection: true,
            can_edit_selection: true,
            has_sets: true,
            has_previous_production_marker: true,
            has_next_production_marker: true,
            has_previous_set: true,
            has_next_set: true,
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
