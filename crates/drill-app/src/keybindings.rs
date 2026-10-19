//! One registry for every keyboard command.
//!
//! Menus, toolbar tips, and the key poll read the same chords. Overrides live
//! in the app settings file (`keybindings.json`), not in the drill. A missing
//! action keeps its default, so a newer build can add keys without throwing
//! away the ones a person already changed.

use super::FieldTool;
use super::commands::{self, Command};
use super::i18n;
use drill_core::{Edit, Locale};
use eframe::egui::{self, Color32};
use std::collections::BTreeMap;

const INK: Color32 = Color32::from_rgb(28, 32, 40);
const PAPER: Color32 = Color32::from_rgb(248, 250, 252);
const WARN: Color32 = Color32::from_rgb(232, 140, 48);
const RESET_RED: Color32 = Color32::from_rgb(196, 48, 40);
const NUDGE_LARGE: i32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Surface {
    Simple,
    Full,
}

#[derive(Clone, Copy)]
pub(crate) struct DispatchGate {
    pub surface: Surface,
    pub commands: commands::Context,
    pub locale: Locale,
    pub can_nudge: bool,
    pub symmetry_ok: bool,
    pub ghost_ok: bool,
    pub song_import_ok: bool,
    pub song_sync_ok: bool,
    pub song_remove_ok: bool,
}

impl DispatchGate {
    pub(crate) fn allows(self, action: Action) -> bool {
        match action {
            Action::Command(command) => command.enabled(self.commands, self.locale).is_ok(),
            Action::ToolSelect | Action::ToolMove | Action::ToolPlace => {
                self.surface == Surface::Full
            }
            Action::NudgeLeft
            | Action::NudgeRight
            | Action::NudgeUp
            | Action::NudgeDown
            | Action::NudgeLeftLarge
            | Action::NudgeRightLarge
            | Action::NudgeUpLarge
            | Action::NudgeDownLarge => self.can_nudge,
            Action::ToggleSymmetry => self.symmetry_ok,
            Action::TogglePreviousGhost => self.ghost_ok,
            Action::ImportSong => self.song_import_ok,
            Action::MarkDownbeat | Action::NudgeSongEarlier | Action::NudgeSongLater => {
                self.song_sync_ok
            }
            Action::RemoveSong => self.song_remove_ok,
            Action::OpenSetNavigator => self.surface == Surface::Full,
            Action::FirstSet
            | Action::LastSet
            | Action::PreviousCount
            | Action::NextCount
            | Action::ToggleGrid
            | Action::FitField
            | Action::ZoomIn
            | Action::ZoomOut
            | Action::ToggleToolbar
            | Action::OpenPalette
            | Action::ShowCheatSheet
            | Action::OpenKeySettings => true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Command(Command),
    ToolSelect,
    ToolMove,
    ToolPlace,
    FirstSet,
    LastSet,
    PreviousCount,
    NextCount,
    NudgeLeft,
    NudgeRight,
    NudgeUp,
    NudgeDown,
    NudgeLeftLarge,
    NudgeRightLarge,
    NudgeUpLarge,
    NudgeDownLarge,
    ToggleGrid,
    FitField,
    ZoomIn,
    ZoomOut,
    ToggleToolbar,
    ToggleSymmetry,
    TogglePreviousGhost,
    OpenPalette,
    OpenSetNavigator,
    ShowCheatSheet,
    OpenKeySettings,
    ImportSong,
    MarkDownbeat,
    NudgeSongEarlier,
    NudgeSongLater,
    RemoveSong,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Category {
    Tools,
    Playback,
    Sets,
    Edit,
    View,
    File,
    Song,
}

/// Frequent work first. Sets sit with playback (next set, duplicate).
/// File is last because new/open/save are rare next to moving people.
const CATEGORIES: [Category; 7] = [
    Category::Tools,
    Category::Playback,
    Category::Sets,
    Category::Edit,
    Category::View,
    Category::Song,
    Category::File,
];

const EXTRAS: [Action; 31] = [
    Action::ToolSelect,
    Action::ToolMove,
    Action::ToolPlace,
    Action::FirstSet,
    Action::LastSet,
    Action::PreviousCount,
    Action::NextCount,
    Action::NudgeLeft,
    Action::NudgeRight,
    Action::NudgeUp,
    Action::NudgeDown,
    Action::NudgeLeftLarge,
    Action::NudgeRightLarge,
    Action::NudgeUpLarge,
    Action::NudgeDownLarge,
    Action::ToggleGrid,
    Action::FitField,
    Action::ZoomIn,
    Action::ZoomOut,
    Action::ToggleToolbar,
    Action::ToggleSymmetry,
    Action::TogglePreviousGhost,
    Action::OpenPalette,
    Action::OpenSetNavigator,
    Action::ShowCheatSheet,
    Action::OpenKeySettings,
    Action::ImportSong,
    Action::MarkDownbeat,
    Action::NudgeSongEarlier,
    Action::NudgeSongLater,
    Action::RemoveSong,
];

fn all_actions() -> Vec<Action> {
    let mut actions: Vec<Action> = commands::SPECS
        .iter()
        .map(|spec| Action::Command(spec.command))
        .collect();
    actions.extend(EXTRAS);
    actions
}

impl Action {
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Command(command) => command_id(command),
            Self::ToolSelect => "tool.select",
            Self::ToolMove => "tool.move",
            Self::ToolPlace => "tool.place",
            Self::FirstSet => "set.first",
            Self::LastSet => "set.last",
            Self::PreviousCount => "playback.count_prev",
            Self::NextCount => "playback.count_next",
            Self::NudgeLeft => "edit.nudge_left",
            Self::NudgeRight => "edit.nudge_right",
            Self::NudgeUp => "edit.nudge_up",
            Self::NudgeDown => "edit.nudge_down",
            Self::NudgeLeftLarge => "edit.nudge_left_lg",
            Self::NudgeRightLarge => "edit.nudge_right_lg",
            Self::NudgeUpLarge => "edit.nudge_up_lg",
            Self::NudgeDownLarge => "edit.nudge_down_lg",
            Self::ToggleGrid => "view.grid",
            Self::FitField => "view.fit",
            Self::ZoomIn => "view.zoom_in",
            Self::ZoomOut => "view.zoom_out",
            Self::ToggleToolbar => "view.toolbar",
            Self::ToggleSymmetry => "view.symmetry",
            Self::TogglePreviousGhost => "view.previous_ghost",
            Self::OpenPalette => "nav.palette",
            Self::OpenSetNavigator => "nav.sets",
            Self::ShowCheatSheet => "view.cheat_sheet",
            Self::OpenKeySettings => "view.key_settings",
            Self::ImportSong => "song.import",
            Self::MarkDownbeat => "song.downbeat",
            Self::NudgeSongEarlier => "song.nudge_earlier",
            Self::NudgeSongLater => "song.nudge_later",
            Self::RemoveSong => "song.remove",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        all_actions().into_iter().find(|action| action.id() == id)
    }
}

fn command_id(command: Command) -> &'static str {
    use Command::*;
    match command {
        NewDocument => "file.new",
        OpenDocument => "file.open",
        OpenProject => "file.open_project",
        OpenRecent => "file.recent",
        Save => "file.save",
        SaveAs => "file.save_as",
        SaveProjectAs => "file.save_project",
        ImportCoordinates => "file.import_coordinates",
        ImportMusicalTimeline => "file.import_timeline",
        LoadImageUnderlay => "file.underlay",
        Undo => "edit.undo",
        Redo => "edit.redo",
        SelectAll => "edit.select_all",
        AddPerformer => "edit.add",
        RemoveSelectedPerformers => "edit.remove",
        ClearSelection => "edit.clear",
        RestoreRecentSelection => "edit.restore",
        CopyFormation => "arrange.copy",
        PasteFormation => "arrange.paste",
        AlignHorizontal => "arrange.align_h",
        AlignVertical => "arrange.align_v",
        DistributeHorizontal => "arrange.distribute_h",
        DistributeVertical => "arrange.distribute_v",
        FlipHorizontal => "arrange.flip_h",
        FlipVertical => "arrange.flip_v",
        MakeLine => "arrange.line",
        LockSelection => "arrange.lock",
        HideSelection => "arrange.hide",
        DuplicateSet => "set.duplicate",
        DeleteSet => "set.delete",
        ManageSections => "set.sections",
        PlayPause => "playback.play",
        RangeStart => "playback.range_start",
        RangeCurrentSet => "playback.range_set",
        RangeWholeShow => "playback.range_all",
        MarkRangeStart => "playback.mark_in",
        MarkRangeEnd => "playback.mark_out",
        PreviousProductionMarker => "playback.marker_prev",
        NextProductionMarker => "playback.marker_next",
        PreviousSet => "playback.set_prev",
        NextSet => "playback.set_next",
        GoToGlobalCount => "playback.go_count",
        FocusPerformerTools => "workspace.performers",
        FocusClinic => "workspace.clinic",
        FocusGrid => "workspace.grid",
        FocusTempo => "workspace.tempo",
        FocusVideo => "workspace.video",
        OpenPrint => "workspace.print",
        FocusAudio => "workspace.audio",
        OpenProductionSheet => "workspace.production",
        View2d => "view.2d",
        View3d => "view.3d",
        ToggleFocusField => "view.focus",
        WorkspaceDesign => "view.design",
        WorkspaceReview => "view.review",
        WorkspacePresent => "view.present",
        ToggleGuidance => "view.guidance",
        TogglePerfHud => "view.perf",
        ToggleCollaborators => "view.collaborators",
        GettingStarted => "help.start",
        LegalNotices => "help.legal",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Chord {
    pub command: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: egui::Key,
}

impl Chord {
    const fn plain(key: egui::Key) -> Self {
        Self {
            command: false,
            shift: false,
            alt: false,
            key,
        }
    }

    const fn command(key: egui::Key) -> Self {
        Self {
            command: true,
            shift: false,
            alt: false,
            key,
        }
    }

    const fn shift(key: egui::Key) -> Self {
        Self {
            command: false,
            shift: true,
            alt: false,
            key,
        }
    }

    fn from_shortcut(shortcut: commands::Shortcut) -> Self {
        match shortcut {
            commands::Shortcut::Command(key) => Self::command(key),
            commands::Shortcut::CommandShift(key) => Self {
                command: true,
                shift: true,
                alt: false,
                key,
            },
            commands::Shortcut::CommandAlt(key) => Self {
                command: true,
                shift: false,
                alt: true,
                key,
            },
            commands::Shortcut::Alt(key) => Self {
                command: false,
                shift: false,
                alt: true,
                key,
            },
            commands::Shortcut::Plain(key) => Self::plain(key),
        }
    }

    fn rank(self) -> u8 {
        u8::from(self.command) * 4 + u8::from(self.alt) * 2 + u8::from(self.shift)
    }

    fn egui(self) -> egui::KeyboardShortcut {
        let modifiers = match (self.command, self.shift, self.alt) {
            (true, true, true) => {
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT | egui::Modifiers::ALT
            }
            (true, true, false) => egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            (true, false, true) => egui::Modifiers::COMMAND | egui::Modifiers::ALT,
            (true, false, false) => egui::Modifiers::COMMAND,
            (false, true, true) => egui::Modifiers::SHIFT | egui::Modifiers::ALT,
            (false, true, false) => egui::Modifiers::SHIFT,
            (false, false, true) => egui::Modifiers::ALT,
            (false, false, false) => egui::Modifiers::NONE,
        };
        egui::KeyboardShortcut::new(modifiers, self.key)
    }
}

fn default_chords(action: Action) -> Vec<Chord> {
    let mut chords = Vec::new();
    if let Action::Command(command) = action {
        if let Some(spec) = commands::SPECS.iter().find(|spec| spec.command == command)
            && let Some(shortcut) = spec.shortcut
        {
            chords.push(Chord::from_shortcut(shortcut));
        }
        match command {
            Command::Redo => chords.push(Chord::command(egui::Key::Y)),
            Command::DuplicateSet => chords.push(Chord::plain(egui::Key::N)),
            Command::RemoveSelectedPerformers => chords.push(Chord::plain(egui::Key::Backspace)),
            Command::PreviousSet => chords.push(Chord::plain(egui::Key::OpenBracket)),
            Command::NextSet => chords.push(Chord::plain(egui::Key::CloseBracket)),
            _ => {}
        }
        return chords;
    }
    push_extra(action, &mut chords);
    chords
}

fn push_extra(action: Action, chords: &mut Vec<Chord>) {
    use egui::Key::*;
    match action {
        Action::ToolSelect => chords.push(Chord::plain(V)),
        Action::ToolMove => chords.push(Chord::plain(M)),
        Action::ToolPlace => chords.push(Chord::plain(P)),
        Action::FirstSet => chords.push(Chord::command(Home)),
        Action::LastSet => chords.push(Chord::plain(End)),
        Action::PreviousCount => chords.push(Chord::plain(Comma)),
        Action::NextCount => chords.push(Chord::plain(Period)),
        Action::NudgeLeft => chords.push(Chord::plain(ArrowLeft)),
        Action::NudgeRight => chords.push(Chord::plain(ArrowRight)),
        Action::NudgeUp => chords.push(Chord::plain(ArrowUp)),
        Action::NudgeDown => chords.push(Chord::plain(ArrowDown)),
        Action::NudgeLeftLarge => chords.push(Chord::shift(ArrowLeft)),
        Action::NudgeRightLarge => chords.push(Chord::shift(ArrowRight)),
        Action::NudgeUpLarge => chords.push(Chord::shift(ArrowUp)),
        Action::NudgeDownLarge => chords.push(Chord::shift(ArrowDown)),
        Action::ToggleGrid => chords.push(Chord::plain(G)),
        Action::FitField => chords.push(Chord::plain(F)),
        Action::ZoomIn => {
            chords.push(Chord::plain(Plus));
            chords.push(Chord::plain(Equals));
        }
        Action::ZoomOut => chords.push(Chord::plain(Minus)),
        Action::ToggleToolbar => chords.push(Chord::plain(T)),
        Action::ToggleSymmetry => chords.push(Chord::plain(B)),
        Action::TogglePreviousGhost => chords.push(Chord::plain(H)),
        Action::OpenPalette => chords.push(Chord::command(K)),
        Action::OpenSetNavigator => chords.push(Chord::command(J)),
        Action::ShowCheatSheet => {
            chords.push(Chord::command(Slash));
            chords.push(Chord::shift(Slash));
        }
        Action::OpenKeySettings => chords.push(Chord::command(Comma)),
        Action::MarkDownbeat => chords.push(Chord::plain(Num1)),
        Action::NudgeSongEarlier => chords.push(Chord::plain(Semicolon)),
        Action::NudgeSongLater => chords.push(Chord::plain(Quote)),
        Action::ImportSong | Action::RemoveSong | Action::Command(_) => {}
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct KeyMap {
    overrides: BTreeMap<String, Vec<Chord>>,
}

impl KeyMap {
    pub(crate) fn chords(&self, action: Action) -> Vec<Chord> {
        self.overrides
            .get(action.id())
            .cloned()
            .unwrap_or_else(|| default_chords(action))
    }

    pub(crate) fn resolve(&self, chord: Chord) -> Option<Action> {
        all_actions()
            .into_iter()
            .find(|action| self.chords(*action).contains(&chord))
    }

    #[cfg(test)]
    pub(crate) fn resolve_gated(&self, chord: Chord, gate: &DispatchGate) -> Option<Action> {
        self.resolve(chord).filter(|action| gate.allows(*action))
    }

    pub(crate) fn set(&mut self, action: Action, chords: Vec<Chord>) {
        self.overrides
            .insert(action.id().to_string(), dedup(chords));
    }

    pub(crate) fn reset_action(&mut self, action: Action) {
        self.overrides.remove(action.id());
    }

    pub(crate) fn reset_all(&mut self) {
        self.overrides.clear();
    }

    pub(crate) fn conflict_owner(&self, action: Action, chord: Chord) -> Option<Action> {
        self.resolve(chord).filter(|owner| *owner != action)
    }

    /// Exchange `chord` onto `action`. The other action receives `action`'s
    /// previous chord when it has one, so two commands can trade places.
    pub(crate) fn swap_chords(&mut self, action: Action, chord: Chord, other: Action) {
        let previous: Vec<Chord> = self
            .chords(action)
            .into_iter()
            .filter(|item| *item != chord)
            .collect();
        let mut given: Vec<Chord> = self
            .chords(other)
            .into_iter()
            .filter(|item| *item != chord)
            .collect();
        if let Some(old) = previous.first().copied() {
            given.insert(0, old);
        }
        self.overrides.insert(action.id().to_string(), Vec::new());
        self.overrides.insert(other.id().to_string(), Vec::new());
        self.overrides.insert(other.id().to_string(), dedup(given));
        self.overrides.insert(action.id().to_string(), vec![chord]);
    }

    pub(crate) fn primary_text(&self, ctx: &egui::Context, action: Action) -> String {
        self.chords(action)
            .first()
            .map(|chord| ctx.format_shortcut(&chord.egui()))
            .unwrap_or_default()
    }

    pub(crate) fn command_labels(&self, ctx: &egui::Context) -> Vec<(Command, String)> {
        commands::SPECS
            .iter()
            .map(|spec| {
                (
                    spec.command,
                    self.primary_text(ctx, Action::Command(spec.command)),
                )
            })
            .collect()
    }

    pub(crate) fn poll(&self, ctx: &egui::Context, gate: &DispatchGate) -> Option<Action> {
        if ctx.egui_wants_keyboard_input() || ctx.text_edit_focused() {
            return None;
        }
        let mut ranked: Vec<(u8, Action, Chord)> = Vec::new();
        for action in all_actions() {
            for chord in self.chords(action) {
                ranked.push((chord.rank(), action, chord));
            }
        }
        ranked.sort_by_key(|row| std::cmp::Reverse(row.0));
        ctx.input_mut(|input| {
            for (_, action, chord) in &ranked {
                if !gate.allows(*action) {
                    continue;
                }
                if input.consume_shortcut(&chord.egui()) {
                    return Some(*action);
                }
            }
            None
        })
    }

    pub(crate) fn parse(text: &str) -> Self {
        let Ok(root) = serde_json::from_str::<serde_json::Value>(text) else {
            return Self::default();
        };
        let Some(bindings) = root.get("bindings").and_then(|value| value.as_object()) else {
            return Self::default();
        };
        let mut staged = Vec::new();
        for (id, value) in bindings {
            let Some(action) = Action::from_id(id) else {
                continue;
            };
            let Some(list) = value.as_array() else {
                continue;
            };
            if list.is_empty() {
                staged.push((action, Vec::new()));
                continue;
            }
            let chords: Vec<Chord> = list.iter().filter_map(parse_chord).collect();
            if chords.is_empty() {
                continue;
            }
            staged.push((action, dedup(chords)));
        }
        let mut map = Self::default();
        for (action, _) in &staged {
            map.overrides.insert(action.id().to_string(), Vec::new());
        }
        for (action, chords) in staged {
            let blocked = chords
                .iter()
                .any(|chord| map.resolve(*chord).is_some_and(|owner| owner != action));
            if blocked {
                map.overrides.remove(action.id());
            } else {
                map.overrides.insert(action.id().to_string(), chords);
            }
        }
        map
    }

    fn to_json(&self) -> String {
        let mut bindings = serde_json::Map::new();
        for (id, chords) in &self.overrides {
            let list = chords
                .iter()
                .filter_map(|chord| {
                    let key = key_name(chord.key)?;
                    Some(serde_json::json!({
                        "key": key,
                        "command": chord.command,
                        "shift": chord.shift,
                        "alt": chord.alt,
                    }))
                })
                .collect();
            bindings.insert(id.clone(), serde_json::Value::Array(list));
        }
        serde_json::json!({ "version": 1, "bindings": bindings }).to_string()
    }

    fn load() -> Self {
        match std::fs::read_to_string(settings_path()) {
            Ok(text) => Self::parse(&text),
            Err(_) => Self::default(),
        }
    }
}

fn dedup(chords: Vec<Chord>) -> Vec<Chord> {
    let mut unique = Vec::new();
    for chord in chords {
        if !unique.contains(&chord) {
            unique.push(chord);
        }
    }
    unique
}

fn parse_chord(value: &serde_json::Value) -> Option<Chord> {
    let key = key_from_name(value.get("key")?.as_str()?)?;
    let flag = |name: &str| {
        value
            .get(name)
            .and_then(|item| item.as_bool())
            .unwrap_or(false)
    };
    Some(Chord {
        command: flag("command"),
        shift: flag("shift"),
        alt: flag("alt"),
        key,
    })
}

fn settings_path() -> std::path::PathBuf {
    super::project_state::app_data_dir().join("keybindings.json")
}

fn key_name(key: egui::Key) -> Option<&'static str> {
    named_keys()
        .iter()
        .find(|(_, candidate)| *candidate == key)
        .map(|(name, _)| *name)
}

fn key_from_name(name: &str) -> Option<egui::Key> {
    named_keys()
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, key)| *key)
}

fn named_keys() -> &'static [(&'static str, egui::Key)] {
    use egui::Key::*;
    &[
        ("A", A),
        ("B", B),
        ("C", C),
        ("D", D),
        ("E", E),
        ("F", F),
        ("G", G),
        ("H", H),
        ("I", I),
        ("J", J),
        ("K", K),
        ("L", L),
        ("M", M),
        ("N", N),
        ("O", O),
        ("P", P),
        ("Q", Q),
        ("R", R),
        ("S", S),
        ("T", T),
        ("U", U),
        ("V", V),
        ("W", W),
        ("X", X),
        ("Y", Y),
        ("Z", Z),
        ("Num0", Num0),
        ("Num1", Num1),
        ("Num2", Num2),
        ("Num3", Num3),
        ("Num4", Num4),
        ("Num5", Num5),
        ("Num6", Num6),
        ("Num7", Num7),
        ("Num8", Num8),
        ("Num9", Num9),
        ("F1", F1),
        ("ArrowLeft", ArrowLeft),
        ("ArrowRight", ArrowRight),
        ("ArrowUp", ArrowUp),
        ("ArrowDown", ArrowDown),
        ("Escape", Escape),
        ("Tab", Tab),
        ("Backspace", Backspace),
        ("Enter", Enter),
        ("Space", Space),
        ("Delete", Delete),
        ("Home", Home),
        ("End", End),
        ("PageUp", PageUp),
        ("PageDown", PageDown),
        ("Comma", Comma),
        ("Period", Period),
        ("Slash", Slash),
        ("Plus", Plus),
        ("Minus", Minus),
        ("Equals", Equals),
        ("OpenBracket", OpenBracket),
        ("CloseBracket", CloseBracket),
        ("Semicolon", Semicolon),
        ("Quote", Quote),
    ]
}

#[derive(Clone, Copy)]
struct Pending {
    action: Action,
    chord: Chord,
    other: Action,
}

#[derive(Default)]
pub(crate) struct KeyBindings {
    pub(crate) map: KeyMap,
    pub(crate) settings_open: bool,
    pub(crate) cheat_open: bool,
    query: String,
    recording: Option<Action>,
    pending: Option<Pending>,
    confirm_reset: bool,
    /// Bit per [`Category`] index. Set means the group is folded.
    collapsed: u8,
}

fn category_mask(category: Category) -> u8 {
    1 << match category {
        Category::Tools => 0,
        Category::Playback => 1,
        Category::Sets => 2,
        Category::Edit => 3,
        Category::View => 4,
        Category::Song => 5,
        Category::File => 6,
    }
}

impl KeyBindings {
    pub(crate) fn load(&mut self) {
        self.map = KeyMap::load();
    }

    fn save(&self) {
        let path = settings_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, self.map.to_json());
    }

    pub(crate) fn stage(&mut self, action: Action, chord: Chord) -> Option<Action> {
        if let Some(other) = self.map.conflict_owner(action, chord) {
            self.pending = Some(Pending {
                action,
                chord,
                other,
            });
            self.recording = None;
            Some(other)
        } else {
            self.map.set(action, vec![chord]);
            self.pending = None;
            self.recording = None;
            None
        }
    }

    pub(crate) fn swap_pending(&mut self) -> bool {
        let Some(pending) = self.pending.take() else {
            return false;
        };
        self.map
            .swap_chords(pending.action, pending.chord, pending.other);
        true
    }

    pub(crate) fn cancel_pending(&mut self) {
        self.pending = None;
        self.recording = None;
    }

    fn category_collapsed(&self, category: Category) -> bool {
        self.collapsed & category_mask(category) != 0
    }

    fn toggle_category(&mut self, category: Category) {
        self.collapsed ^= category_mask(category);
    }
}

enum Recorded {
    Cancel,
    Chord(Chord),
}

impl super::DrillApp {
    pub(crate) fn apply_qa_keys(&mut self) {
        let Ok(stage) = std::env::var("DRILLFORGE_QA_KEYS") else {
            return;
        };
        self.onboarding.show_welcome = false;
        self.simple_mode.enabled = true;
        match stage.as_str() {
            "settings" => self.keybindings.settings_open = true,
            "sheet" => self.keybindings.cheat_open = true,
            // V is already Select. Stage that onto Move, then keep the
            // recorder armed so the shot shows the warning during recording.
            "conflict" => {
                self.keybindings.settings_open = true;
                let _ = self
                    .keybindings
                    .stage(Action::ToolMove, Chord::plain(egui::Key::V));
                self.keybindings.recording = Some(Action::ToolMove);
            }
            _ => {}
        }
    }

    pub(crate) fn handle_global_keys(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let text_focus = ctx.egui_wants_keyboard_input() || ctx.text_edit_focused();
        if self.keybindings.recording.is_some() {
            if !text_focus {
                self.capture_recording(&ctx);
            }
            return;
        }
        if self.keybindings.cheat_open && !text_focus {
            let close =
                ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            if close {
                self.keybindings.cheat_open = false;
                return;
            }
        }
        if text_focus || self.command_palette.is_open() || self.set_navigator.is_open() {
            return;
        }
        let gate = self.key_gate();
        if let Some(action) = self.keybindings.map.poll(&ctx, &gate) {
            self.perform_key_action(action, &ctx);
        }
    }

    pub(crate) fn show_key_surfaces(&mut self, ctx: &egui::Context) {
        self.show_key_settings(ctx);
        self.show_cheat_sheet(ctx);
    }

    pub(crate) fn show_command_surfaces(&mut self, ctx: &egui::Context) {
        let labels = self.keybindings.map.command_labels(ctx);
        let command_context = self.command_context();
        let locale = self.locale;
        if let Some(command) = self
            .command_palette
            .show(ctx, command_context, locale, &|command| {
                labels
                    .iter()
                    .find(|(candidate, _)| *candidate == command)
                    .map(|(_, text)| text.clone())
                    .unwrap_or_default()
            })
        {
            self.execute_command(command, ctx);
        }
        if !self.simple_mode.enabled
            && let Some(index) =
                self.set_navigator
                    .show(ctx, &self.document, self.current_set, self.locale)
        {
            self.navigate_to_set(index);
        }
        let total = self.document.timeline_counts();
        let locale = self.locale;
        if let Some(count) = self.go_to_count.show(ctx, total, locale) {
            self.navigate_to_global_count(count);
        }
    }

    pub(crate) fn key_tip(&self, ctx: &egui::Context, label: &str, action: Action) -> String {
        let shortcut = self.keybindings.map.primary_text(ctx, action);
        if shortcut.is_empty() {
            label.to_string()
        } else {
            format!("{label}\n{shortcut}")
        }
    }

    pub(crate) fn toolbar_toggle_label(&self) -> &'static str {
        i18n::registered(self.locale, "shortcuts.026")
    }

    fn key_gate(&self) -> DispatchGate {
        let simple = self.simple_mode.enabled;
        DispatchGate {
            surface: if simple {
                Surface::Simple
            } else {
                Surface::Full
            },
            commands: self.command_context(),
            locale: self.locale,
            can_nudge: self.key_nudge_allowed(),
            symmetry_ok: !self.playing,
            ghost_ok: self.current_set > 0,
            song_import_ok: simple,
            song_sync_ok: simple && self.document.audio.is_some() && !self.simple_mode.song_missing,
            song_remove_ok: simple && self.document.audio.is_some(),
        }
    }

    fn perform_key_action(&mut self, action: Action, ctx: &egui::Context) {
        if !self.key_gate().allows(action) {
            return;
        }
        match action {
            Action::Command(command) => self.execute_command(command, ctx),
            Action::ToolSelect => self.set_field_tool(FieldTool::Select),
            Action::ToolMove => self.set_field_tool(FieldTool::Move),
            Action::ToolPlace => self.set_field_tool(FieldTool::Place),
            Action::FirstSet => self.navigate_to_set(0),
            Action::LastSet => {
                let last = self.document.sets.len().saturating_sub(1);
                self.navigate_to_set(last);
            }
            Action::PreviousCount => self.key_step_count(-1),
            Action::NextCount => self.key_step_count(1),
            Action::NudgeLeft => self.key_nudge(-1, 0),
            Action::NudgeRight => self.key_nudge(1, 0),
            Action::NudgeUp => self.key_nudge(0, 1),
            Action::NudgeDown => self.key_nudge(0, -1),
            Action::NudgeLeftLarge => self.key_nudge(-NUDGE_LARGE, 0),
            Action::NudgeRightLarge => self.key_nudge(NUDGE_LARGE, 0),
            Action::NudgeUpLarge => self.key_nudge(0, NUDGE_LARGE),
            Action::NudgeDownLarge => self.key_nudge(0, -NUDGE_LARGE),
            Action::ToggleGrid => self.key_toggle_grid(),
            Action::FitField => {
                let grid = self.document.grid.clone();
                self.field_viewport.reset(&grid);
            }
            Action::ZoomIn => self
                .field_viewport
                .set_zoom(self.field_viewport.zoom * 1.25),
            Action::ZoomOut => self.field_viewport.set_zoom(self.field_viewport.zoom * 0.8),
            Action::ToggleToolbar => {
                if self.simple_mode.enabled {
                    self.onboarding.simple_toolbar_collapsed =
                        !self.onboarding.simple_toolbar_collapsed;
                } else {
                    self.full_toolbar_collapsed = !self.full_toolbar_collapsed;
                }
            }
            Action::ToggleSymmetry => self.simple_toggle_mirror(),
            Action::TogglePreviousGhost => self.toggle_previous_ghost(),
            Action::OpenPalette => self.command_palette.open(),
            Action::OpenSetNavigator => self.set_navigator.open(),
            Action::ShowCheatSheet => self.keybindings.cheat_open = true,
            Action::OpenKeySettings => self.keybindings.settings_open = true,
            Action::ImportSong => self.simple_pick_and_import_song(),
            Action::MarkDownbeat => self.simple_mark_downbeat(),
            Action::NudgeSongEarlier => self.simple_nudge_song(-1),
            Action::NudgeSongLater => self.simple_nudge_song(1),
            Action::RemoveSong => self.simple_remove_song(),
        }
    }

    fn key_nudge(&mut self, horizontal: i32, vertical: i32) {
        if self.simple_mode.enabled {
            self.simple_nudge(horizontal, vertical);
        } else {
            self.nudge_selected(horizontal, vertical);
        }
    }

    fn key_step_count(&mut self, delta: i32) {
        if self.playing || delta == 0 {
            return;
        }
        if self.simple_mode.enabled {
            self.simple_step_beat(delta);
            return;
        }
        let total = self.document.timeline_counts();
        let current = self
            .document
            .global_count(self.current_set, self.count_position)
            .round()
            .clamp(0.0, total as f32) as u32;
        let next = if delta > 0 {
            current.saturating_add(delta as u32).min(total)
        } else {
            current.saturating_sub(delta.unsigned_abs())
        };
        if next != current {
            self.navigate_to_global_count(next);
        }
    }

    fn key_toggle_grid(&mut self) {
        let mut grid = self.document.grid.clone();
        grid.show_step_grid = !grid.show_step_grid;
        let show = grid.show_step_grid;
        let failure = i18n::registered(self.locale, "shortcuts.023");
        if self.execute_edit(
            Edit::ReplaceGrid {
                grid,
                scale_positions: false,
            },
            failure,
        ) && let Some(draft) = &mut self.grid_draft
        {
            draft.show_step_grid = show;
        }
    }

    fn capture_recording(&mut self, ctx: &egui::Context) {
        let Some(action) = self.keybindings.recording else {
            return;
        };
        let Some(recorded) = recorded_chord(ctx) else {
            return;
        };
        self.keybindings.recording = None;
        match recorded {
            Recorded::Cancel => self.keybindings.pending = None,
            Recorded::Chord(chord) => {
                if self.keybindings.stage(action, chord).is_none() {
                    self.keybindings.save();
                }
            }
        }
    }

    fn show_key_settings(&mut self, ctx: &egui::Context) {
        if !self.keybindings.settings_open {
            self.keybindings.recording = None;
            self.keybindings.pending = None;
            self.keybindings.confirm_reset = false;
            return;
        }
        let mut open = true;
        let title = self.settings_title();
        let bounds = ctx.viewport_rect().shrink(16.0);
        super::app_theme::modal_window(title, ctx)
            .id(egui::Id::new("key-settings"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .title_bar(false)
            .vscroll(false)
            .default_size(egui::vec2(
                700.0_f32.min(bounds.width()),
                (bounds.height() - 8.0).min(640.0),
            ))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .frame(paper_frame())
            .show(ctx, |ui| self.settings_contents(ui));
        self.keybindings.settings_open = self.keybindings.settings_open && open;
    }

    fn settings_contents(&mut self, ui: &mut egui::Ui) {
        light_paper(ui);
        ui.set_min_width(560.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(self.settings_title())
                    .size(22.0)
                    .strong()
                    .color(INK),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(self.close_word()).clicked() {
                    self.keybindings.settings_open = false;
                }
            });
        });
        let search_id = ui.make_persistent_id("keybinding-search");
        ui.add(
            egui::TextEdit::singleline(&mut self.keybindings.query)
                .id(search_id)
                .hint_text(i18n::registered(self.locale, "shortcuts.002"))
                .desired_width(f32::INFINITY),
        );
        if let Some(action) = self.keybindings.recording {
            ui.label(
                egui::RichText::new(i18n::registered(self.locale, "shortcuts.003"))
                    .color(super::app_theme::ACCENT)
                    .strong(),
            );
            ui.label(self.action_label(action));
            if ui
                .button(i18n::registered(self.locale, "shortcuts.022"))
                .clicked()
            {
                self.keybindings.recording = None;
            }
        }
        if let Some(pending) = self.keybindings.pending {
            let message = i18n::registered(self.locale, "shortcuts.004")
                .replace("{0}", self.action_label(pending.other));
            ui.label(egui::RichText::new(message).color(WARN).strong());
            ui.horizontal(|ui| {
                if ui
                    .button(i18n::registered(self.locale, "shortcuts.005"))
                    .clicked()
                    && self.keybindings.swap_pending()
                {
                    self.keybindings.save();
                }
                if ui.button(self.cancel_word()).clicked() {
                    self.keybindings.cancel_pending();
                }
            });
        }
        let query = self.keybindings.query.clone();
        let rows = self.visible_actions(ui.ctx(), &query);
        let bounds_h = ui.ctx().viewport_rect().shrink(16.0).height();
        let mut chrome = 168.0;
        if self.keybindings.recording.is_some() {
            chrome += 72.0;
        }
        if self.keybindings.pending.is_some() {
            chrome += 60.0;
        }
        if self.keybindings.confirm_reset {
            chrome += 48.0;
        }
        let scroll_height = (bounds_h - chrome).max(120.0);
        egui::ScrollArea::vertical()
            .max_height(scroll_height)
            .auto_shrink([false, false])
            .show(ui, |ui| self.settings_rows(ui, &rows, search_id, &query));
        ui.separator();
        self.reset_all_row(ui);
    }

    fn settings_rows(
        &mut self,
        ui: &mut egui::Ui,
        rows: &[Action],
        search_id: egui::Id,
        query: &str,
    ) {
        let reset = i18n::registered(self.locale, "shortcuts.007");
        let unbound = self.unbound_word();
        let searching = !query.trim().is_empty();
        for category in CATEGORIES {
            let group: Vec<Action> = rows
                .iter()
                .copied()
                .filter(|action| category_of(*action) == category)
                .collect();
            if group.is_empty() {
                continue;
            }
            let open = searching || !self.keybindings.category_collapsed(category);
            if self.category_header(ui, category, open).clicked() && !searching {
                self.keybindings.toggle_category(category);
            }
            if !open {
                continue;
            }
            for action in group {
                let name = self.action_label(action);
                let chords = self.keybindings.map.chords(action);
                let armed = self.keybindings.recording == Some(action);
                let changed = chords != default_chords(action);
                ui.horizontal(|ui| {
                    ui.label(name);
                    if chords.is_empty() {
                        if key_chip(ui, unbound, armed).clicked() {
                            self.arm(action, search_id, ui.ctx());
                        }
                    } else {
                        for chord in &chords {
                            let text = ui.ctx().format_shortcut(&chord.egui());
                            if key_chip(ui, &text, armed).clicked() {
                                self.arm(action, search_id, ui.ctx());
                            }
                        }
                    }
                    if changed && ui.button(reset).clicked() {
                        self.keybindings.map.reset_action(action);
                        self.keybindings.recording = None;
                        self.keybindings.pending = None;
                        self.keybindings.save();
                    }
                });
            }
        }
    }

    fn category_header(&self, ui: &mut egui::Ui, category: Category, open: bool) -> egui::Response {
        ui.add_space(8.0);
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 28.0), egui::Sense::click());
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, 8.0, super::app_theme::ACCENT_SOFT);
        }
        paint_chevron(
            ui.painter(),
            rect.left_center() + egui::vec2(10.0, 0.0),
            open,
        );
        paint_category_icon_in(
            ui.painter(),
            egui::Rect::from_center_size(
                rect.left_center() + egui::vec2(32.0, 0.0),
                egui::vec2(18.0, 18.0),
            ),
            category,
        );
        ui.painter().text(
            rect.left_center() + egui::vec2(48.0, 0.0),
            egui::Align2::LEFT_CENTER,
            self.category_label(category),
            egui::FontId::proportional(16.0),
            INK,
        );
        response
    }

    fn reset_all_row(&mut self, ui: &mut egui::Ui) {
        if self.keybindings.confirm_reset {
            ui.label(
                egui::RichText::new(i18n::registered(self.locale, "shortcuts.009"))
                    .color(RESET_RED),
            );
            ui.horizontal(|ui| {
                let button = egui::Button::new(
                    egui::RichText::new(self.reset_confirm_word()).color(Color32::WHITE),
                )
                .fill(RESET_RED);
                if ui.add(button).clicked() {
                    self.keybindings.map.reset_all();
                    self.keybindings.confirm_reset = false;
                    self.keybindings.recording = None;
                    self.keybindings.pending = None;
                    self.keybindings.save();
                }
                if ui.button(self.cancel_word()).clicked() {
                    self.keybindings.confirm_reset = false;
                }
            });
        } else if ui
            .button(i18n::registered(self.locale, "shortcuts.008"))
            .clicked()
        {
            self.keybindings.confirm_reset = true;
        }
    }

    fn show_cheat_sheet(&mut self, ctx: &egui::Context) {
        if !self.keybindings.cheat_open {
            return;
        }
        let screen = ctx.viewport_rect();
        let bounds = screen.shrink(16.0);
        let mut close = false;
        egui::Area::new(egui::Id::new("key-chart-dim"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (rect, response) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
                ui.painter()
                    .rect_filled(rect, 0.0, Color32::from_black_alpha(96));
                if response.clicked() {
                    close = true;
                }
            });
        let card_w = 780.0_f32.min(bounds.width());
        let card_h = bounds.height().min(720.0);
        egui::Area::new(egui::Id::new("key-chart"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                paper_frame().show(ui, |ui| {
                    light_paper(ui);
                    ui.set_min_width(card_w.min(560.0));
                    ui.set_max_size(egui::vec2(card_w, card_h));
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(self.cheat_title())
                                .size(22.0)
                                .strong()
                                .color(INK),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button(self.close_word()).clicked() {
                                close = true;
                            }
                        });
                    });
                    ui.label(
                        egui::RichText::new(i18n::registered(self.locale, "shortcuts.021"))
                            .size(15.0)
                            .color(INK),
                    );
                    self.cheat_rows(ui, (card_h - 96.0).max(160.0));
                });
            });
        if close {
            self.keybindings.cheat_open = false;
        }
    }

    fn cheat_rows(&self, ui: &mut egui::Ui, scroll_height: f32) {
        let unbound = self.unbound_word();
        egui::ScrollArea::vertical()
            .max_height(scroll_height)
            .show(ui, |ui| {
                for category in CATEGORIES {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        paint_category_icon(ui, category);
                        ui.label(
                            egui::RichText::new(self.category_label(category))
                                .strong()
                                .size(16.0)
                                .color(INK),
                        );
                    });
                    for action in all_actions()
                        .into_iter()
                        .filter(|action| category_of(*action) == category)
                    {
                        ui.horizontal(|ui| {
                            ui.label(self.action_label(action));
                            let chords = self.keybindings.map.chords(action);
                            if chords.is_empty() {
                                keycap(ui, unbound);
                            } else {
                                for chord in chords {
                                    keycap(ui, &ui.ctx().format_shortcut(&chord.egui()));
                                }
                            }
                        });
                    }
                }
            });
    }

    fn visible_actions(&self, ctx: &egui::Context, query: &str) -> Vec<Action> {
        all_actions()
            .into_iter()
            .filter(|action| self.matches_query(ctx, *action, query))
            .collect()
    }

    fn matches_query(&self, ctx: &egui::Context, action: Action, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return true;
        }
        if self.action_label(action).to_lowercase().contains(&query) {
            return true;
        }
        self.keybindings.map.chords(action).iter().any(|chord| {
            ctx.format_shortcut(&chord.egui())
                .to_lowercase()
                .contains(&query)
        })
    }

    fn arm(&mut self, action: Action, search_id: egui::Id, ctx: &egui::Context) {
        self.keybindings.recording = Some(action);
        self.keybindings.pending = None;
        self.keybindings.confirm_reset = false;
        ctx.memory_mut(|memory| memory.surrender_focus(search_id));
    }

    fn settings_title(&self) -> &'static str {
        i18n::registered(self.locale, "shortcuts.001")
    }

    fn cheat_title(&self) -> &'static str {
        i18n::registered(self.locale, "shortcuts.018")
    }

    fn close_word(&self) -> &'static str {
        i18n::registered(self.locale, "shortcuts.019")
    }

    fn cancel_word(&self) -> &'static str {
        i18n::registered(self.locale, "shortcuts.006")
    }

    fn unbound_word(&self) -> &'static str {
        i18n::registered(self.locale, "shortcuts.011")
    }

    fn reset_confirm_word(&self) -> &'static str {
        i18n::registered(self.locale, "shortcuts.010")
    }

    fn action_label(&self, action: Action) -> &'static str {
        let locale = self.locale;
        match action {
            Action::Command(command) => command.label(locale),
            Action::ToolSelect => i18n::registered(locale, "shortcuts.030"),
            Action::ToolMove => i18n::registered(locale, "shortcuts.031"),
            Action::ToolPlace => i18n::registered(locale, "shortcuts.032"),
            Action::FirstSet => i18n::registered(locale, "shortcuts.033"),
            Action::LastSet => i18n::registered(locale, "shortcuts.034"),
            Action::PreviousCount => i18n::registered(locale, "shortcuts.035"),
            Action::NextCount => i18n::registered(locale, "shortcuts.036"),
            Action::NudgeLeft => i18n::registered(locale, "shortcuts.037"),
            Action::NudgeRight => i18n::registered(locale, "shortcuts.038"),
            Action::NudgeUp => i18n::registered(locale, "shortcuts.039"),
            Action::NudgeDown => i18n::registered(locale, "shortcuts.040"),
            Action::NudgeLeftLarge => i18n::registered(locale, "shortcuts.041"),
            Action::NudgeRightLarge => i18n::registered(locale, "shortcuts.042"),
            Action::NudgeUpLarge => i18n::registered(locale, "shortcuts.043"),
            Action::NudgeDownLarge => i18n::registered(locale, "shortcuts.044"),
            Action::ToggleGrid => i18n::registered(locale, "shortcuts.045"),
            Action::FitField => i18n::registered(locale, "shortcuts.046"),
            Action::ZoomIn => i18n::registered(locale, "shortcuts.047"),
            Action::ZoomOut => i18n::registered(locale, "shortcuts.048"),
            Action::ToggleToolbar => i18n::registered(locale, "shortcuts.049"),
            Action::ToggleSymmetry => i18n::registered(locale, "shortcuts.050"),
            Action::OpenPalette => i18n::registered(locale, "shortcuts.051"),
            Action::OpenSetNavigator => i18n::registered(locale, "shortcuts.052"),
            Action::TogglePreviousGhost => i18n::registered(locale, "shortcuts.053"),
            Action::OpenKeySettings => i18n::registered(locale, "shortcuts.054"),
            Action::ShowCheatSheet => i18n::registered(locale, "shortcuts.055"),
            Action::ImportSong => i18n::registered(locale, "shortcuts.057"),
            Action::MarkDownbeat => i18n::registered(locale, "shortcuts.058"),
            Action::NudgeSongEarlier => i18n::registered(locale, "shortcuts.059"),
            Action::NudgeSongLater => i18n::registered(locale, "shortcuts.060"),
            Action::RemoveSong => i18n::registered(locale, "shortcuts.061"),
        }
    }

    fn category_label(&self, category: Category) -> &'static str {
        let locale = self.locale;
        match category {
            Category::Tools => i18n::registered(locale, "shortcuts.012"),
            Category::Playback => i18n::registered(locale, "shortcuts.013"),
            Category::Sets => i18n::registered(locale, "shortcuts.014"),
            Category::Edit => i18n::registered(locale, "shortcuts.015"),
            Category::View => i18n::registered(locale, "shortcuts.016"),
            Category::File => i18n::registered(locale, "shortcuts.017"),
            Category::Song => i18n::registered(locale, "shortcuts.056"),
        }
    }
}

fn category_of(action: Action) -> Category {
    match action {
        Action::ToolSelect | Action::ToolMove | Action::ToolPlace => Category::Tools,
        Action::Command(command) => command_category(command),
        Action::PreviousCount | Action::NextCount => Category::Playback,
        Action::FirstSet | Action::LastSet => Category::Sets,
        Action::NudgeLeft
        | Action::NudgeRight
        | Action::NudgeUp
        | Action::NudgeDown
        | Action::NudgeLeftLarge
        | Action::NudgeRightLarge
        | Action::NudgeUpLarge
        | Action::NudgeDownLarge => Category::Edit,
        Action::ToggleGrid
        | Action::FitField
        | Action::ZoomIn
        | Action::ZoomOut
        | Action::ToggleToolbar
        | Action::ToggleSymmetry
        | Action::TogglePreviousGhost
        | Action::OpenPalette
        | Action::OpenSetNavigator
        | Action::ShowCheatSheet
        | Action::OpenKeySettings => Category::View,
        Action::ImportSong
        | Action::MarkDownbeat
        | Action::NudgeSongEarlier
        | Action::NudgeSongLater
        | Action::RemoveSong => Category::Song,
    }
}

fn command_category(command: Command) -> Category {
    use Command::*;
    match command {
        PlayPause
        | RangeStart
        | RangeCurrentSet
        | RangeWholeShow
        | MarkRangeStart
        | MarkRangeEnd
        | PreviousProductionMarker
        | NextProductionMarker
        | GoToGlobalCount => Category::Playback,
        PreviousSet | NextSet | DuplicateSet | DeleteSet | ManageSections => Category::Sets,
        NewDocument
        | OpenDocument
        | OpenProject
        | OpenRecent
        | Save
        | SaveAs
        | SaveProjectAs
        | ImportCoordinates
        | ImportMusicalTimeline
        | LoadImageUnderlay
        | GettingStarted
        | LegalNotices => Category::File,
        FocusPerformerTools | FocusClinic | FocusGrid | FocusTempo | FocusVideo | OpenPrint
        | FocusAudio | OpenProductionSheet | View2d | View3d | ToggleFocusField
        | WorkspaceDesign | WorkspaceReview | WorkspacePresent | ToggleGuidance | TogglePerfHud
        | ToggleCollaborators => Category::View,
        Undo
        | Redo
        | SelectAll
        | AddPerformer
        | RemoveSelectedPerformers
        | ClearSelection
        | RestoreRecentSelection
        | CopyFormation
        | PasteFormation
        | AlignHorizontal
        | AlignVertical
        | DistributeHorizontal
        | DistributeVertical
        | FlipHorizontal
        | FlipVertical
        | MakeLine
        | LockSelection
        | HideSelection => Category::Edit,
    }
}

fn paper_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(PAPER)
        .stroke(egui::Stroke::new(1.0, super::app_theme::HAIRLINE))
        .corner_radius(16.0)
        .inner_margin(egui::Margin::symmetric(16, 14))
}

fn light_paper(ui: &mut egui::Ui) {
    let visuals = &mut ui.style_mut().visuals;
    visuals.override_text_color = Some(INK);
    visuals.panel_fill = PAPER;
    visuals.extreme_bg_color = Color32::WHITE;
    visuals.widgets.noninteractive.fg_stroke.color = INK;
    visuals.widgets.inactive.fg_stroke.color = INK;
    visuals.widgets.inactive.weak_bg_fill = Color32::WHITE;
    visuals.widgets.hovered.weak_bg_fill = super::app_theme::ACCENT_SOFT;
    visuals.widgets.hovered.fg_stroke.color = INK;
    visuals.selection.bg_fill = super::app_theme::ACCENT_SOFT;
}

fn paint_category_icon(ui: &mut egui::Ui, category: Category) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
    paint_category_icon_in(ui.painter(), rect, category);
}

fn paint_chevron(painter: &egui::Painter, center: egui::Pos2, open: bool) {
    let points = if open {
        vec![
            center + egui::vec2(-5.0, -2.0),
            center + egui::vec2(5.0, -2.0),
            center + egui::vec2(0.0, 4.0),
        ]
    } else {
        vec![
            center + egui::vec2(-3.0, -5.0),
            center + egui::vec2(4.0, 0.0),
            center + egui::vec2(-3.0, 5.0),
        ]
    };
    painter.add(egui::Shape::convex_polygon(points, INK, egui::Stroke::NONE));
}

fn paint_category_icon_in(painter: &egui::Painter, rect: egui::Rect, category: Category) {
    let center = rect.center();
    let color = super::app_theme::ACCENT;
    match category {
        Category::Tools => {
            painter.rect_filled(rect.shrink(3.0), 3.0, color);
        }
        Category::Playback => {
            // A transport button, not a second triangle beside the collapse chevron.
            painter.circle_stroke(center, 6.5, egui::Stroke::new(1.6, color));
            painter.rect_filled(
                egui::Rect::from_center_size(center, egui::vec2(4.5, 4.5)),
                1.0,
                color,
            );
        }
        Category::Sets => {
            painter.rect_stroke(
                rect.shrink(3.0),
                2.0,
                egui::Stroke::new(1.5, color),
                egui::StrokeKind::Inside,
            );
            painter.line_segment(
                [
                    rect.left_center() + egui::vec2(4.0, 0.0),
                    rect.right_center() + egui::vec2(-4.0, 0.0),
                ],
                egui::Stroke::new(1.5, color),
            );
        }
        Category::Edit => {
            painter.circle_stroke(center, 5.0, egui::Stroke::new(1.5, color));
        }
        Category::View => {
            painter.circle_filled(center, 3.0, color);
            painter.circle_stroke(center, 6.0, egui::Stroke::new(1.5, color));
        }
        Category::File => {
            painter.rect_filled(rect.shrink(4.0), 2.0, color);
        }
        Category::Song => {
            painter.circle_filled(center + egui::vec2(-2.0, 3.0), 3.0, color);
            painter.line_segment(
                [
                    center + egui::vec2(1.0, 3.0),
                    center + egui::vec2(1.0, -6.0),
                ],
                egui::Stroke::new(1.5, color),
            );
        }
    }
}

fn key_chip(ui: &mut egui::Ui, text: &str, armed: bool) -> egui::Response {
    let fill = if armed {
        super::app_theme::ACCENT
    } else {
        Color32::WHITE
    };
    let ink = if armed { Color32::WHITE } else { INK };
    ui.add(
        egui::Button::new(egui::RichText::new(text).color(ink))
            .fill(fill)
            .stroke(egui::Stroke::new(1.0, super::app_theme::HAIRLINE))
            .corner_radius(8.0),
    )
}

fn keycap(ui: &mut egui::Ui, text: &str) {
    egui::Frame::new()
        .fill(Color32::WHITE)
        .stroke(egui::Stroke::new(1.0, super::app_theme::HAIRLINE))
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(14.0).strong().color(INK));
        });
}

fn recorded_chord(ctx: &egui::Context) -> Option<Recorded> {
    let found = ctx.input(|input| {
        for event in &input.events {
            if let egui::Event::Key {
                key,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            } = event
            {
                if *key == egui::Key::Escape && modifiers.is_none() {
                    return Some(Recorded::Cancel);
                }
                return Some(Recorded::Chord(Chord {
                    command: modifiers.command,
                    shift: modifiers.shift,
                    alt: modifiers.alt,
                    key: *key,
                }));
            }
        }
        None
    });
    match found {
        Some(Recorded::Cancel) => {
            ctx.input_mut(|input| {
                let _ = input.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
            });
        }
        Some(Recorded::Chord(chord)) => {
            ctx.input_mut(|input| {
                let _ = input.consume_shortcut(&chord.egui());
            });
        }
        None => {}
    }
    found
}

#[cfg(test)]
mod tests {
    use super::super::DrillApp;
    use super::*;

    fn ready_gate(surface: Surface) -> DispatchGate {
        DispatchGate {
            surface,
            commands: commands::Context {
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
                has_multiple_sets: true,
                can_exit_field_tool: true,
            },
            locale: Locale::En,
            can_nudge: true,
            symmetry_ok: true,
            ghost_ok: true,
            song_import_ok: surface == Surface::Simple,
            song_sync_ok: surface == Surface::Simple,
            song_remove_ok: surface == Surface::Simple,
        }
    }

    fn poll_key(modifiers: egui::Modifiers, key: egui::Key, gate: &DispatchGate) -> Option<Action> {
        let ctx = egui::Context::default();
        let mut raw = egui::RawInput {
            modifiers,
            ..egui::RawInput::default()
        };
        raw.events.push(egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers,
        });
        let map = KeyMap::default();
        let mut resolved = None;
        let _ = ctx.run_ui(raw, |ui| {
            resolved = map.poll(ui.ctx(), gate);
        });
        resolved
    }

    #[test]
    fn ids_are_unique_and_round_trip() {
        let mut seen = std::collections::BTreeSet::new();
        for action in all_actions() {
            assert!(seen.insert(action.id()), "duplicate id {}", action.id());
            assert_eq!(Action::from_id(action.id()), Some(action));
        }
    }

    #[test]
    fn defaults_cover_every_existing_shortcut_without_duplicate_chords() {
        let map = KeyMap::default();
        let mut seen = Vec::new();
        for action in all_actions() {
            for chord in map.chords(action) {
                assert!(
                    !seen.contains(&chord),
                    "chord {chord:?} is used twice ({action:?})"
                );
                seen.push(chord);
            }
        }
        for spec in commands::SPECS {
            let Some(shortcut) = spec.shortcut else {
                continue;
            };
            assert_eq!(
                map.resolve(Chord::from_shortcut(shortcut)),
                Some(Action::Command(spec.command))
            );
        }
    }

    #[test]
    fn dispatch_keeps_existing_chords_and_adds_the_new_ones() {
        let map = KeyMap::default();
        let full = ready_gate(Surface::Full);
        let simple = ready_gate(Surface::Simple);
        assert_eq!(
            map.resolve(Chord::plain(egui::Key::Space)),
            Some(Action::Command(Command::PlayPause))
        );
        assert_eq!(
            map.resolve(Chord::plain(egui::Key::Delete)),
            Some(Action::Command(Command::RemoveSelectedPerformers))
        );
        assert_eq!(
            map.resolve(Chord::plain(egui::Key::Backspace)),
            Some(Action::Command(Command::RemoveSelectedPerformers))
        );
        assert_eq!(
            map.resolve(Chord::command(egui::Key::Z)),
            Some(Action::Command(Command::Undo))
        );
        assert_eq!(
            map.resolve(Chord {
                command: true,
                shift: true,
                alt: false,
                key: egui::Key::Z,
            }),
            Some(Action::Command(Command::Redo))
        );
        assert_eq!(
            map.resolve(Chord::command(egui::Key::Y)),
            Some(Action::Command(Command::Redo))
        );
        assert_eq!(
            map.resolve(Chord::plain(egui::Key::PageUp)),
            Some(Action::Command(Command::PreviousSet))
        );
        assert_eq!(
            map.resolve(Chord::plain(egui::Key::Home)),
            Some(Action::Command(Command::RangeStart))
        );
        assert_eq!(
            map.resolve(Chord::command(egui::Key::Home)),
            Some(Action::FirstSet)
        );
        assert_eq!(
            map.resolve(Chord::plain(egui::Key::End)),
            Some(Action::LastSet)
        );
        assert_eq!(
            map.resolve(Chord::plain(egui::Key::N)),
            Some(Action::Command(Command::DuplicateSet))
        );
        assert_eq!(
            map.resolve(Chord::command(egui::Key::N)),
            Some(Action::Command(Command::NewDocument))
        );
        assert_eq!(
            map.resolve_gated(Chord::plain(egui::Key::V), &full),
            Some(Action::ToolSelect)
        );
        assert_eq!(map.resolve_gated(Chord::plain(egui::Key::V), &simple), None);
        assert_eq!(
            map.resolve_gated(Chord::plain(egui::Key::B), &simple),
            Some(Action::ToggleSymmetry)
        );
        assert_eq!(
            map.resolve_gated(Chord::plain(egui::Key::B), &full),
            Some(Action::ToggleSymmetry)
        );
        assert_eq!(
            map.resolve_gated(Chord::plain(egui::Key::H), &simple),
            Some(Action::TogglePreviousGhost)
        );
        assert_eq!(
            map.resolve_gated(Chord::plain(egui::Key::H), &full),
            Some(Action::TogglePreviousGhost)
        );
        assert_eq!(
            map.resolve(Chord::plain(egui::Key::Num1)),
            Some(Action::MarkDownbeat)
        );
        assert_eq!(
            map.resolve(Chord::plain(egui::Key::Semicolon)),
            Some(Action::NudgeSongEarlier)
        );
        assert_eq!(
            map.resolve(Chord::plain(egui::Key::Quote)),
            Some(Action::NudgeSongLater)
        );
        assert!(map.chords(Action::ImportSong).is_empty());
        assert!(map.chords(Action::RemoveSong).is_empty());
        assert_eq!(
            map.resolve_gated(Chord::plain(egui::Key::Num1), &full),
            None
        );
        assert_eq!(
            map.resolve_gated(Chord::plain(egui::Key::Num1), &simple),
            Some(Action::MarkDownbeat)
        );
        assert!(simple.allows(Action::ImportSong));
        assert!(!full.allows(Action::ImportSong));
        assert_eq!(
            poll_key(egui::Modifiers::NONE, egui::Key::Space, &full),
            Some(Action::Command(Command::PlayPause))
        );
        assert_eq!(
            poll_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::S,
                &full
            ),
            Some(Action::Command(Command::SaveAs))
        );
    }

    #[test]
    fn recording_reports_a_conflict_and_swap_or_cancel() {
        let mut keys = KeyBindings::default();
        assert_eq!(
            keys.stage(Action::ToolMove, Chord::plain(egui::Key::V)),
            Some(Action::ToolSelect)
        );
        keys.cancel_pending();
        assert_eq!(
            keys.map.resolve(Chord::plain(egui::Key::V)),
            Some(Action::ToolSelect)
        );
        assert_eq!(
            keys.map.resolve(Chord::plain(egui::Key::M)),
            Some(Action::ToolMove)
        );
        assert_eq!(
            keys.stage(Action::ToolMove, Chord::plain(egui::Key::V)),
            Some(Action::ToolSelect)
        );
        assert!(keys.swap_pending());
        assert_eq!(
            keys.map.resolve(Chord::plain(egui::Key::V)),
            Some(Action::ToolMove)
        );
        assert_eq!(
            keys.map.resolve(Chord::plain(egui::Key::M)),
            Some(Action::ToolSelect)
        );
    }

    #[test]
    fn persistence_merges_defaults_and_rejects_garbage() {
        let swapped = KeyMap::parse(
            r#"{"version":1,"bindings":{"edit.undo":[{"key":"Z","command":true,"shift":true}],"edit.redo":[{"key":"Z","command":true}]}}"#,
        );
        assert_eq!(
            swapped.resolve(Chord {
                command: true,
                shift: true,
                alt: false,
                key: egui::Key::Z,
            }),
            Some(Action::Command(Command::Undo))
        );
        assert_eq!(
            swapped.resolve(Chord::command(egui::Key::Z)),
            Some(Action::Command(Command::Redo))
        );
        assert_eq!(
            swapped.resolve(Chord::plain(egui::Key::Space)),
            Some(Action::Command(Command::PlayPause))
        );

        let unknown = KeyMap::parse(
            r#"{"version":4,"bindings":{"not.an.action":[{"key":"V"}],"edit.undo":[{"key":"Q","command":true}]}}"#,
        );
        assert_eq!(
            unknown.resolve(Chord::plain(egui::Key::V)),
            Some(Action::ToolSelect)
        );
        assert_eq!(
            unknown.resolve(Chord::command(egui::Key::Q)),
            Some(Action::Command(Command::Undo))
        );

        let invalid = KeyMap::parse(
            r#"{"version":1,"bindings":{"edit.undo":[{"key":"NotAKey","command":true}]}}"#,
        );
        assert_eq!(
            invalid.resolve(Chord::command(egui::Key::Z)),
            Some(Action::Command(Command::Undo))
        );

        let unbound = KeyMap::parse(r#"{"version":1,"bindings":{"playback.play":[]}}"#);
        assert_eq!(unbound.resolve(Chord::plain(egui::Key::Space)), None);
        assert_eq!(
            unbound.resolve(Chord::plain(egui::Key::Delete)),
            Some(Action::Command(Command::RemoveSelectedPerformers))
        );

        let outside = KeyMap::parse(r#"{"version":1,"bindings":{"edit.undo":[{"key":"V"}]}}"#);
        assert_eq!(
            outside.resolve(Chord::plain(egui::Key::V)),
            Some(Action::ToolSelect)
        );
        assert_eq!(
            outside.resolve(Chord::command(egui::Key::Z)),
            Some(Action::Command(Command::Undo))
        );

        assert_eq!(
            KeyMap::parse("not json").resolve(Chord::plain(egui::Key::Space)),
            Some(Action::Command(Command::PlayPause))
        );

        let map = KeyMap::default();
        let mut edited = map.clone();
        edited.set(
            Action::Command(Command::Undo),
            vec![Chord::command(egui::Key::Q)],
        );
        let round = KeyMap::parse(&edited.to_json());
        assert_eq!(
            round.resolve(Chord::command(egui::Key::Q)),
            Some(Action::Command(Command::Undo))
        );
        assert_eq!(round.resolve(Chord::command(egui::Key::Z)), None);
    }

    #[test]
    fn text_focus_suppresses_shortcuts() {
        let gate = ready_gate(Surface::Full);
        let ctx = egui::Context::default();
        let mut text = String::new();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let response =
                ui.add(egui::TextEdit::singleline(&mut text).id(egui::Id::new("keys-focus")));
            response.request_focus();
        });
        let mut raw = egui::RawInput::default();
        raw.events.push(egui::Event::Key {
            key: egui::Key::Space,
            physical_key: Some(egui::Key::Space),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        let map = KeyMap::default();
        let mut resolved = Some(Action::Command(Command::PlayPause));
        let _ = ctx.run_ui(raw, |ui| {
            let _ = ui.add(egui::TextEdit::singleline(&mut text).id(egui::Id::new("keys-focus")));
            assert!(ui.ctx().egui_wants_keyboard_input() || ui.ctx().text_edit_focused());
            resolved = map.poll(ui.ctx(), &gate);
        });
        assert_eq!(resolved, None);
    }

    #[test]
    fn performed_actions_respect_the_mode_gate() {
        let ctx = egui::Context::default();
        let mut app = DrillApp::default();
        app.simple_mode.enabled = true;
        app.field_tool = FieldTool::Move;
        app.perform_key_action(Action::ToggleSymmetry, &ctx);
        assert!(app.key_mirror_on());
        app.perform_key_action(Action::ToolSelect, &ctx);
        assert_eq!(app.field_tool, FieldTool::Move);
        app.simple_mode.enabled = false;
        app.field_tool = FieldTool::Move;
        app.perform_key_action(Action::ToolSelect, &ctx);
        assert_eq!(app.field_tool, FieldTool::Select);
        app.perform_key_action(Action::ToggleSymmetry, &ctx);
        assert!(!app.key_mirror_on());
        let shown = app.document.grid.show_step_grid;
        app.perform_key_action(Action::ToggleGrid, &ctx);
        assert_ne!(app.document.grid.show_step_grid, shown);
        assert!(app.history.can_undo());
    }
}
