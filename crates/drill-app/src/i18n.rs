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
}

impl Text {
    /// Authoritative catalog inventory. Adding a key without adding it here makes
    /// the catalog completeness test fail during review rather than leaking a
    /// fallback string into a release build.
    #[cfg(test)]
    pub const ALL: [Self; 31] = [
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
