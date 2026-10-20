use drill_core::Locale;

#[path = "i18n_generated.rs"]
mod generated;
pub(crate) use generated::registered;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatusMessage {
    id: Option<&'static str>,
    args: [Option<String>; 2],
}

impl StatusMessage {
    pub fn new(id: &'static str) -> Self {
        Self {
            id: Some(id),
            args: Default::default(),
        }
    }

    pub fn arg(mut self, index: usize, value: impl ToString) -> Self {
        if let Some(slot) = self.args.get_mut(index) {
            let mut value = value.to_string();
            value.truncate(512);
            *slot = Some(value);
        }
        self
    }

    pub fn is_empty(&self) -> bool {
        self.id.is_none()
    }

    pub fn text(&self, locale: Locale) -> String {
        let Some(id) = self.id else {
            return String::new();
        };
        let mut rendered = registered(locale, id).to_owned();
        for (index, value) in self.args.iter().enumerate() {
            if let Some(value) = value {
                rendered = rendered.replace(&format!("{{{index}}}"), value);
            }
        }
        rendered
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Text {
    File,
    Edit,
    Playback,
    View,
    Help,
    #[allow(dead_code)]
    SelectAll,
    #[allow(dead_code)]
    ClearSelection,
    Play,
    Pause,
    RangeStart,
    WholeShow,
    Loop,
    Guidance,
    StepGrid,
    GettingStarted,
    Open,
    Save,
    Saved,
    Unsaved,
    Speed,
    Performers,
    Ready,
    Language,
    Japanese,
    English,
    RecoveryTitle,
    RecoveryHint,
    Ignore,
    CrashTitle,
    CrashHint,
    CopyReportPath,
    SimpleMode,
    DetailedMode,
    OnGrid,
    FlipCenterLine,
    Arrange,
    Pinwheel,
    Inspector,
    Timeline,
    Song,
    Symmetry,
    PreviousSet,
    Delete,
    Add,
    More,
    NoPerformers,
    PlaceHint,
    RosterSearch,
    NoMatches,
    Markers,
    Parts,
    Sets,
    NoName,
    PlaybackOptions,
    Collapse,
    Menu,
    CrossMarks,
    Export,
    Compare,
    Paper,
    Orientation,
    Range,
    ThisSet,
    AllSets,
    ShowSheet,
    ExportTitle,
    FitPeople,
    Video3d,
    RenderVideo,
    SaveBundle,
    Draft,
    Fine,
    DayWord,
    NightWord,
    Figures,
    Capsule,
    PersonFigure,
    PressBox,
    EndZone,
    OrbitCam,
    FollowCam,
    BlenderMissing,
    OpenVideo,
    SongOn,
    Quality,
    CameraWord,
    BlenderApp,
    StopRender,
    Pick,
    OpenWhenDone,
    TimeWord,
    TitleCard,
    CameraCuts,
    BlenderFound,
    BlenderAbsent,
    BlenderInstall,
    BlenderAuto,
    EeveeName,
    CyclesName,
}

impl Text {
    /// Authoritative catalog inventory. Adding a key without adding it here makes
    /// the catalog completeness test fail during review rather than leaking a
    /// fallback string into a release build.
    #[cfg(test)]
    pub const ALL: [Self; 99] = [
        Self::File,
        Self::Edit,
        Self::Playback,
        Self::View,
        Self::Help,
        Self::SelectAll,
        Self::ClearSelection,
        Self::Play,
        Self::Pause,
        Self::RangeStart,
        Self::WholeShow,
        Self::Loop,
        Self::Guidance,
        Self::StepGrid,
        Self::GettingStarted,
        Self::Open,
        Self::Save,
        Self::Saved,
        Self::Unsaved,
        Self::Speed,
        Self::Performers,
        Self::Ready,
        Self::Language,
        Self::Japanese,
        Self::English,
        Self::RecoveryTitle,
        Self::RecoveryHint,
        Self::Ignore,
        Self::CrashTitle,
        Self::CrashHint,
        Self::CopyReportPath,
        Self::SimpleMode,
        Self::DetailedMode,
        Self::OnGrid,
        Self::FlipCenterLine,
        Self::Arrange,
        Self::Pinwheel,
        Self::Inspector,
        Self::Timeline,
        Self::Song,
        Self::Symmetry,
        Self::PreviousSet,
        Self::Delete,
        Self::Add,
        Self::More,
        Self::NoPerformers,
        Self::PlaceHint,
        Self::RosterSearch,
        Self::NoMatches,
        Self::Markers,
        Self::Parts,
        Self::Sets,
        Self::NoName,
        Self::PlaybackOptions,
        Self::Collapse,
        Self::Menu,
        Self::CrossMarks,
        Self::Export,
        Self::Compare,
        Self::Paper,
        Self::Orientation,
        Self::Range,
        Self::ThisSet,
        Self::AllSets,
        Self::ShowSheet,
        Self::ExportTitle,
        Self::FitPeople,
        Self::Video3d,
        Self::RenderVideo,
        Self::SaveBundle,
        Self::Draft,
        Self::Fine,
        Self::DayWord,
        Self::NightWord,
        Self::Figures,
        Self::Capsule,
        Self::PersonFigure,
        Self::PressBox,
        Self::EndZone,
        Self::OrbitCam,
        Self::FollowCam,
        Self::BlenderMissing,
        Self::OpenVideo,
        Self::SongOn,
        Self::Quality,
        Self::CameraWord,
        Self::BlenderApp,
        Self::StopRender,
        Self::Pick,
        Self::OpenWhenDone,
        Self::TimeWord,
        Self::TitleCard,
        Self::CameraCuts,
        Self::BlenderFound,
        Self::BlenderAbsent,
        Self::BlenderInstall,
        Self::BlenderAuto,
        Self::EeveeName,
        Self::CyclesName,
    ];
}

pub fn text(locale: Locale, key: Text) -> &'static str {
    match (locale, key) {
        (Locale::Ja, Text::File) => "ファイル",
        (Locale::En, Text::File) => "File",
        (Locale::Ja, Text::Edit) => "編集",
        (Locale::En, Text::Edit) => "Edit",
        (Locale::Ja, Text::Playback) => "再生",
        (Locale::En, Text::Playback) => "Playback",
        (Locale::Ja, Text::View) => "表示",
        (Locale::En, Text::View) => "View",
        (Locale::Ja, Text::Help) => "ヘルプ",
        (Locale::En, Text::Help) => "Help",
        (Locale::Ja, Text::SelectAll) => "全員を選択",
        (Locale::En, Text::SelectAll) => "Select All",
        (Locale::Ja, Text::ClearSelection) => "選択解除",
        (Locale::En, Text::ClearSelection) => "Clear Selection",
        (Locale::Ja, Text::Play) => "▶ 再生",
        (Locale::En, Text::Play) => "▶ Play",
        (Locale::Ja, Text::Pause) => "‖ 一時停止",
        (Locale::En, Text::Pause) => "‖ Pause",
        (Locale::Ja, Text::RangeStart) => "範囲の先頭へ",
        (Locale::En, Text::RangeStart) => "Go to Range Start",
        (Locale::Ja, Text::WholeShow) => "曲全体を範囲にする",
        (Locale::En, Text::WholeShow) => "Set Range to Whole Show",
        (Locale::Ja, Text::Loop) => "ループ再生",
        (Locale::En, Text::Loop) => "Loop Playback",
        (Locale::Ja, Text::Guidance) => "操作ガイド",
        (Locale::En, Text::Guidance) => "Guidance",
        (Locale::Ja, Text::StepGrid) => "ステップグリッド",
        (Locale::En, Text::StepGrid) => "Step Grid",
        (Locale::Ja, Text::GettingStarted) => "はじめかた・操作ガイド    F1",
        (Locale::En, Text::GettingStarted) => "Getting Started & Help    F1",
        (Locale::Ja, Text::Open) => "ファイルを開く",
        (Locale::En, Text::Open) => "Open File",
        (Locale::Ja, Text::Save) => "保存",
        (Locale::En, Text::Save) => "Save",
        (Locale::Ja, Text::Saved) => "✓ 保存済み",
        (Locale::En, Text::Saved) => "✓ Saved",
        (Locale::Ja, Text::Unsaved) => "● 未保存",
        (Locale::En, Text::Unsaved) => "● Unsaved",
        (Locale::Ja, Text::Speed) => "速度",
        (Locale::En, Text::Speed) => "Speed",
        (Locale::Ja, Text::Performers) => "演者",
        (Locale::En, Text::Performers) => "Performers",
        (Locale::Ja, Text::Ready) => "準備完了",
        (Locale::En, Text::Ready) => "Ready",
        (Locale::Ja, Text::Language) => "言語 / Language",
        (Locale::En, Text::Language) => "Language / 言語",
        (_, Text::Japanese) => "日本語",
        (_, Text::English) => "English",
        (Locale::Ja, Text::RecoveryTitle) => "前回の未終了セッションを復旧できます",
        (Locale::En, Text::RecoveryTitle) => "An unfinished session can be recovered",
        (Locale::Ja, Text::RecoveryHint) => {
            "復旧データは元のプロジェクトを変更せず、別の作業コピーとして開きます。"
        }
        (Locale::En, Text::RecoveryHint) => {
            "Recovery opens a separate working copy and never changes the original project."
        }
        (Locale::Ja, Text::Ignore) => "無視",
        (Locale::En, Text::Ignore) => "Dismiss",
        (Locale::Ja, Text::CrashTitle) => "前回のクラッシュレポートがあります",
        (Locale::En, Text::CrashTitle) => "A crash report from a previous run is available",
        (Locale::Ja, Text::CrashHint) => {
            "元ファイルは変更されていません。下の復旧候補から作業を再開できます。"
        }
        (Locale::En, Text::CrashHint) => {
            "Your original file was not changed. Resume from a recovery candidate below."
        }
        (Locale::Ja, Text::CopyReportPath) => "レポートの場所をコピー",
        (Locale::En, Text::CopyReportPath) => "Copy Report Location",
        (Locale::Ja, Text::SimpleMode) => "かんたん",
        (Locale::En, Text::SimpleMode) => "Simple",
        (Locale::Ja, Text::DetailedMode) => "くわしく",
        (Locale::En, Text::DetailedMode) => "Detailed",
        (Locale::Ja, Text::OnGrid) => "マス目へ",
        (Locale::En, Text::OnGrid) => "On the grid",
        (Locale::Ja, Text::FlipCenterLine) => "中央線で反転",
        (Locale::En, Text::FlipCenterLine) => "Flip on the center line",
        (Locale::Ja, Text::Arrange) => "整列",
        (Locale::En, Text::Arrange) => "Arrange",
        (Locale::Ja, Text::Pinwheel) => "ピンウィール",
        (Locale::En, Text::Pinwheel) => "Pinwheel",
        (Locale::Ja, Text::Inspector) => "詳細",
        (Locale::En, Text::Inspector) => "Details",
        (Locale::Ja, Text::Timeline) => "タイムライン",
        (Locale::En, Text::Timeline) => "Timeline",
        (Locale::Ja, Text::Song) => "曲",
        (Locale::En, Text::Song) => "Song",
        (Locale::Ja, Text::Symmetry) => "左右対称",
        (Locale::En, Text::Symmetry) => "Symmetry",
        (Locale::Ja, Text::PreviousSet) => "前の場面",
        (Locale::En, Text::PreviousSet) => "Previous set",
        (Locale::Ja, Text::Delete) => "削除",
        (Locale::En, Text::Delete) => "Delete",
        (Locale::Ja, Text::Add) => "追加",
        (Locale::En, Text::Add) => "Add",
        (Locale::Ja, Text::More) => "その他",
        (Locale::En, Text::More) => "More",
        (Locale::Ja, Text::NoPerformers) => "演者がまだいません",
        (Locale::En, Text::NoPerformers) => "No performers yet",
        (Locale::Ja, Text::PlaceHint) => "配置でフィールドをタップ",
        (Locale::En, Text::PlaceHint) => "Tap the field with Place",
        (Locale::Ja, Text::RosterSearch) => "名前か番号",
        (Locale::En, Text::RosterSearch) => "Name or number",
        (Locale::Ja, Text::NoMatches) => "該当する演者はいません",
        (Locale::En, Text::NoMatches) => "No matching performers",
        (Locale::Ja, Text::Markers) => "本番マーカー",
        (Locale::En, Text::Markers) => "Production markers",
        (Locale::Ja, Text::Parts) => "パート",
        (Locale::En, Text::Parts) => "Parts",
        (Locale::Ja, Text::Sets) => "セット",
        (Locale::En, Text::Sets) => "Sets",
        (Locale::Ja, Text::NoName) => "名前なし",
        (Locale::En, Text::NoName) => "No name",
        (Locale::Ja, Text::PlaybackOptions) => "再生の設定",
        (Locale::En, Text::PlaybackOptions) => "Playback options",
        (Locale::Ja, Text::Collapse) => "閉じる",
        (Locale::En, Text::Collapse) => "Collapse",
        (_, Text::Menu) => "☰",
        (Locale::Ja, Text::CrossMarks) => "×印",
        (Locale::En, Text::CrossMarks) => "Cross marks",
        (Locale::Ja, Text::Export) => "書き出す",
        (Locale::En, Text::Export) => "Export",
        (Locale::Ja, Text::Compare) => "A/B比較",
        (Locale::En, Text::Compare) => "A/B compare",
        (Locale::Ja, Text::Paper) => "用紙",
        (Locale::En, Text::Paper) => "Paper",
        (Locale::Ja, Text::Orientation) => "向き",
        (Locale::En, Text::Orientation) => "Orientation",
        (Locale::Ja, Text::Range) => "範囲",
        (Locale::En, Text::Range) => "Range",
        (Locale::Ja, Text::ThisSet) => "今のセット",
        (Locale::En, Text::ThisSet) => "This set",
        (Locale::Ja, Text::AllSets) => "全部",
        (Locale::En, Text::AllSets) => "All sets",
        (Locale::Ja, Text::ShowSheet) => "進行表",
        (Locale::En, Text::ShowSheet) => "Run sheet",
        (Locale::Ja, Text::ExportTitle) => "書き出し",
        (Locale::En, Text::ExportTitle) => "Export",
        (Locale::Ja, Text::FitPeople) => "人に合わせる",
        (Locale::En, Text::FitPeople) => "Fit performers",
        (Locale::Ja, Text::Video3d) => "3D動画",
        (Locale::En, Text::Video3d) => "3D video",
        (Locale::Ja, Text::RenderVideo) => "書き出してレンダリング",
        (Locale::En, Text::RenderVideo) => "Render video",
        (Locale::Ja, Text::SaveBundle) => "ファイルだけ保存",
        (Locale::En, Text::SaveBundle) => "Save files",
        (Locale::Ja, Text::Draft) => "下書き",
        (Locale::En, Text::Draft) => "Draft",
        (Locale::Ja, Text::Fine) => "きれい",
        (Locale::En, Text::Fine) => "Fine",
        (Locale::Ja, Text::DayWord) => "昼",
        (Locale::En, Text::DayWord) => "Day",
        (Locale::Ja, Text::NightWord) => "夜",
        (Locale::En, Text::NightWord) => "Night",
        (Locale::Ja, Text::Figures) => "人形",
        (Locale::En, Text::Figures) => "Figures",
        (Locale::Ja, Text::Capsule) => "棒",
        (Locale::En, Text::Capsule) => "Stick",
        (Locale::Ja, Text::PersonFigure) => "人",
        (Locale::En, Text::PersonFigure) => "Person",
        (Locale::Ja, Text::PressBox) => "正面",
        (Locale::En, Text::PressBox) => "Front",
        (Locale::Ja, Text::EndZone) => "端",
        (Locale::En, Text::EndZone) => "End",
        (Locale::Ja, Text::OrbitCam) => "回り",
        (Locale::En, Text::OrbitCam) => "Orbit",
        (Locale::Ja, Text::FollowCam) => "追う",
        (Locale::En, Text::FollowCam) => "Follow",
        (Locale::Ja, Text::BlenderMissing) => "Blenderを選ぶと、ここで動画を作れます。",
        (Locale::En, Text::BlenderMissing) => "Choose Blender to render the video here.",
        (Locale::Ja, Text::OpenVideo) => "開く",
        (Locale::En, Text::OpenVideo) => "Open",
        (Locale::Ja, Text::SongOn) => "曲あり",
        (Locale::En, Text::SongOn) => "With song",
        (Locale::Ja, Text::Quality) => "画質",
        (Locale::En, Text::Quality) => "Quality",
        (Locale::Ja, Text::CameraWord) => "カメラ",
        (Locale::En, Text::CameraWord) => "Camera",
        (Locale::Ja, Text::BlenderApp) => "Blender",
        (Locale::En, Text::BlenderApp) => "Blender",
        (Locale::Ja, Text::StopRender) => "止める",
        (Locale::En, Text::StopRender) => "Stop",
        (Locale::Ja, Text::Pick) => "選ぶ",
        (Locale::En, Text::Pick) => "Choose",
        (Locale::Ja, Text::OpenWhenDone) => "終わったら開く",
        (Locale::En, Text::OpenWhenDone) => "Open when finished",
        (Locale::Ja, Text::TimeWord) => "時間",
        (Locale::En, Text::TimeWord) => "Time",
        (Locale::Ja, Text::TitleCard) => "題名",
        (Locale::En, Text::TitleCard) => "Title",
        (Locale::Ja, Text::CameraCuts) => "切替",
        (Locale::En, Text::CameraCuts) => "Cuts",
        (Locale::Ja, Text::BlenderFound) => "見つかった",
        (Locale::En, Text::BlenderFound) => "Found",
        (Locale::Ja, Text::BlenderAbsent) => "見つからない",
        (Locale::En, Text::BlenderAbsent) => "Not found",
        (Locale::Ja, Text::BlenderInstall) => "入手",
        (Locale::En, Text::BlenderInstall) => "Get Blender",
        (Locale::Ja, Text::BlenderAuto) => "自動",
        (Locale::En, Text::BlenderAuto) => "Auto",
        (Locale::Ja, Text::EeveeName) => "Eevee",
        (Locale::En, Text::EeveeName) => "Eevee",
        (Locale::Ja, Text::CyclesName) => "Cycles",
        (Locale::En, Text::CyclesName) => "Cycles",
    }
}

/// How many people are in the working group, for the floating selection bar.
pub fn people_selected(locale: Locale, count: usize) -> String {
    match locale {
        Locale::Ja => format!("{count}人選択中"),
        Locale::En => format!("{count} selected"),
    }
}

/// How many people are in the roster list, without saying they are selected.
pub fn roster_count(locale: Locale, count: usize) -> String {
    match locale {
        Locale::Ja => format!("{count}人"),
        Locale::En => format!("{count} people"),
    }
}

/// Page count beside the paper name, for the export sheet.
pub fn page_summary(locale: Locale, pages: usize, size: &str) -> String {
    match locale {
        Locale::Ja => format!("{pages}ページ · {size}"),
        Locale::En => format!("{pages} pages · {size}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_key_has_both_locales() {
        for key in Text::ALL {
            assert!(!text(Locale::Ja, key).is_empty());
            assert!(!text(Locale::En, key).is_empty());
        }
    }
    #[test]
    fn locales_are_distinct_for_translated_text() {
        assert_ne!(text(Locale::Ja, Text::Play), text(Locale::En, Text::Play));
    }
    #[test]
    fn page_summary_names_the_count_and_the_paper() {
        assert_eq!(page_summary(Locale::Ja, 6, "A4"), "6ページ · A4");
        assert_eq!(page_summary(Locale::En, 6, "A4"), "6 pages · A4");
    }

    #[test]
    fn english_catalog_does_not_leak_japanese_glyphs() {
        fn has_japanese(value: &str) -> bool {
            value.chars().any(|ch| {
                matches!(ch,
                    '\u{3040}'..='\u{30ff}' |
                    '\u{3400}'..='\u{4dbf}' |
                    '\u{4e00}'..='\u{9fff}')
            })
        }
        for key in Text::ALL {
            // The language selector intentionally presents each language in its
            // native script; all other English UI entries must be English-only.
            if !matches!(key, Text::Language | Text::Japanese) {
                let value = text(Locale::En, key);
                assert!(
                    !has_japanese(value),
                    "Japanese leaked into English {key:?}: {value}"
                );
            }
        }
    }
}
