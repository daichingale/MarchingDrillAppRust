//! Structured errors at the untrusted project boundary.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locale {
    Ja,
    En,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DrillError {
    UnsupportedSchema {
        found: u16,
        supported: u16,
    },
    InvalidJson(String),
    EmptySets,
    InvalidGrid,
    InvalidNumber {
        field: &'static str,
    },
    LimitExceeded {
        field: &'static str,
        limit: usize,
    },
    SetSizeMismatch {
        set_index: usize,
        expected: usize,
        found: usize,
    },
    DuplicatePerformerId,
    DuplicateSetId,
    DuplicateSubsetId,
    MissingSet,
    MissingPerformer,
    MissingSubset,
    InvalidEdit,
    InvalidTransition,
}

impl DrillError {
    pub fn message(&self, locale: Locale) -> String {
        match (self, locale) {
            (Self::UnsupportedSchema { found, supported }, Locale::Ja) => {
                format!("未対応のファイルバージョンです: {found}（対応: {supported}）")
            }
            (Self::UnsupportedSchema { found, supported }, Locale::En) => {
                format!("Unsupported file version: {found} (supported: {supported})")
            }
            (Self::InvalidJson(detail), Locale::Ja) => format!("JSONを読み込めません: {detail}"),
            (Self::InvalidJson(detail), Locale::En) => format!("Could not read JSON: {detail}"),
            (Self::EmptySets, Locale::Ja) => "セットがありません".into(),
            (Self::EmptySets, Locale::En) => "The drill has no sets".into(),
            (Self::InvalidGrid, Locale::Ja) => "グリッド設定が不正です".into(),
            (Self::InvalidGrid, Locale::En) => "The grid configuration is invalid".into(),
            (Self::InvalidNumber { field }, Locale::Ja) => {
                format!("{field} に不正な数値があります")
            }
            (Self::InvalidNumber { field }, Locale::En) => {
                format!("{field} contains an invalid number")
            }
            (Self::LimitExceeded { field, limit }, Locale::Ja) => {
                format!("{field} が上限 {limit} を超えています")
            }
            (Self::LimitExceeded { field, limit }, Locale::En) => {
                format!("{field} exceeds the limit of {limit}")
            }
            (Self::SetSizeMismatch { set_index, .. }, Locale::Ja) => {
                format!("セット {} の演者数が一致しません", set_index + 1)
            }
            (
                Self::SetSizeMismatch {
                    set_index,
                    expected,
                    found,
                },
                Locale::En,
            ) => format!(
                "Set {} has {found} positions; expected {expected}",
                set_index + 1
            ),
            (Self::DuplicatePerformerId, Locale::Ja) => "演者IDが重複しています".into(),
            (Self::DuplicatePerformerId, Locale::En) => "Performer IDs are duplicated".into(),
            (Self::DuplicateSetId, Locale::Ja) => "セットIDが重複しています".into(),
            (Self::DuplicateSetId, Locale::En) => "Set IDs are duplicated".into(),
            (Self::DuplicateSubsetId, Locale::Ja) => "サブセットIDが重複しています".into(),
            (Self::DuplicateSubsetId, Locale::En) => "Subset IDs are duplicated".into(),
            (Self::MissingSet, Locale::Ja) => "編集対象のセットが見つかりません".into(),
            (Self::MissingSet, Locale::En) => "The edited set no longer exists".into(),
            (Self::MissingPerformer, Locale::Ja) => "編集対象の演者が見つかりません".into(),
            (Self::MissingPerformer, Locale::En) => "An edited performer no longer exists".into(),
            (Self::MissingSubset, Locale::Ja) => "編集対象のサブセットが見つかりません".into(),
            (Self::MissingSubset, Locale::En) => "The edited subset no longer exists".into(),
            (Self::InvalidEdit, Locale::Ja) => "編集内容が不正です".into(),
            (Self::InvalidEdit, Locale::En) => "The edit is invalid".into(),
            (Self::InvalidTransition, Locale::Ja) => {
                "移動ルートまたはカウント設定が不正です".into()
            }
            (Self::InvalidTransition, Locale::En) => {
                "The transition route or count settings are invalid".into()
            }
        }
    }
}

impl fmt::Display for DrillError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message(Locale::Ja))
    }
}

impl std::error::Error for DrillError {}
