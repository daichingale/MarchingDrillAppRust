//! Production-facing annotations and derived production sheets.
use crate::{Document, Locale, MAX_TEXT_BYTES, ProductionMarkerId};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SetAnnotation {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub rehearsal_mark: String,
    #[serde(default)]
    pub tempo_bpm: Option<f32>,
    #[serde(default)]
    pub sync_time_seconds: Option<f64>,
    #[serde(default)]
    pub transition_duration_seconds: Option<f64>,
}

/// A user-authored landmark on the count timeline. Unlike set annotations,
/// markers may be placed at any whole count, including inside a transition.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProductionMarkerKind {
    #[default]
    Hit,
    Rehearsal,
    Note,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProductionMarker {
    pub id: ProductionMarkerId,
    pub count: u32,
    #[serde(default)]
    pub kind: ProductionMarkerKind,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub detail: String,
}

impl ProductionMarker {
    pub fn validate(&self, total_counts: u32) -> bool {
        self.count <= total_counts
            && self.label.len() <= MAX_TEXT_BYTES
            && self.detail.len() <= MAX_TEXT_BYTES
    }
}

impl SetAnnotation {
    pub fn validate(&self) -> bool {
        self.title.len() <= MAX_TEXT_BYTES
            && self.notes.len() <= MAX_TEXT_BYTES
            && self.rehearsal_mark.len() <= MAX_TEXT_BYTES
            && self
                .tempo_bpm
                .is_none_or(|v| v.is_finite() && (1.0..=999.0).contains(&v))
            && self
                .sync_time_seconds
                .is_none_or(|v| v.is_finite() && v >= 0.0)
            && self
                .transition_duration_seconds
                .is_none_or(|v| v.is_finite() && v >= 0.0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProductionRow {
    pub set_number: usize,
    pub mark: String,
    pub title: String,
    pub counts: u16,
    pub tempo_bpm: f32,
    pub sync_time_seconds: f64,
    pub transition_duration_seconds: f64,
    pub notes: String,
}

pub fn production_sheet(document: &Document) -> Vec<ProductionRow> {
    let mut count = 0_u32;
    document
        .sets
        .iter()
        .enumerate()
        .map(|(index, set)| {
            let start = document.tempo.seconds_at(count as f32) as f64;
            let end = document.tempo.seconds_at(count as f32 + set.counts as f32) as f64;
            let row = ProductionRow {
                set_number: index + 1,
                mark: if set.annotation.rehearsal_mark.trim().is_empty() {
                    crate::countsheet::rehearsal_mark(index)
                } else {
                    set.annotation.rehearsal_mark.clone()
                },
                title: if set.annotation.title.trim().is_empty() {
                    set.name.clone()
                } else {
                    set.annotation.title.clone()
                },
                counts: set.counts,
                tempo_bpm: set
                    .annotation
                    .tempo_bpm
                    .unwrap_or_else(|| document.tempo.bpm_at(count as f32)),
                sync_time_seconds: set.annotation.sync_time_seconds.unwrap_or(start),
                transition_duration_seconds: set
                    .annotation
                    .transition_duration_seconds
                    .unwrap_or(end - start),
                notes: set.annotation.notes.clone(),
            };
            count = count.saturating_add(u32::from(set.counts) + u32::from(set.hold));
            row
        })
        .collect()
}

pub fn production_sheet_text(document: &Document, locale: Locale) -> String {
    use crate::coordinates::ProductionTemplatePreset;
    let mut out = String::new();
    let standard_headers = match locale {
        Locale::Ja => [
            "#",
            "マーク",
            "タイトル",
            "拍数",
            "テンポ",
            "同期時刻",
            "尺",
            "備考",
        ],
        Locale::En => [
            "#", "Mark", "Title", "Counts", "Tempo", "Sync", "Duration", "Notes",
        ],
    };
    let preset = document.grid.coordinate_notation.production_template;
    let headers = match preset {
        ProductionTemplatePreset::Standard => standard_headers.to_vec(),
        ProductionTemplatePreset::Compact => match locale {
            Locale::Ja => vec!["#", "マーク", "タイトル", "拍数", "備考"],
            Locale::En => vec!["#", "Mark", "Title", "Counts", "Notes"],
        },
        ProductionTemplatePreset::Rehearsal => match locale {
            Locale::Ja => vec!["マーク", "タイトル", "拍数", "テンポ", "備考"],
            Locale::En => vec!["Mark", "Title", "Counts", "Tempo", "Notes"],
        },
    };
    let _ = writeln!(out, "{}", headers.join("\t"));
    for row in production_sheet(document) {
        let notes = row.notes.replace(['\r', '\n', '\t'], " ");
        match preset {
            ProductionTemplatePreset::Standard => {
                let _ = writeln!(
                    out,
                    "{}\t{}\t{}\t{}\t{:.1}\t{:.3}\t{:.3}\t{}",
                    row.set_number,
                    row.mark,
                    row.title,
                    row.counts,
                    row.tempo_bpm,
                    row.sync_time_seconds,
                    row.transition_duration_seconds,
                    notes
                );
            }
            ProductionTemplatePreset::Compact => {
                let _ = writeln!(
                    out,
                    "{}\t{}\t{}\t{}\t{}",
                    row.set_number, row.mark, row.title, row.counts, notes
                );
            }
            ProductionTemplatePreset::Rehearsal => {
                let _ = writeln!(
                    out,
                    "{}\t{}\t{}\t{:.1}\t{}",
                    row.mark, row.title, row.counts, row.tempo_bpm, notes
                );
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn derives_defaults_and_honors_overrides() {
        let mut doc = Document::demo(1, 1);
        doc.sets[0].annotation.rehearsal_mark = "Intro".into();
        doc.sets[0].annotation.tempo_bpm = Some(144.0);
        let rows = production_sheet(&doc);
        assert_eq!(rows[0].mark, "Intro");
        assert_eq!(rows[0].tempo_bpm, 144.0);
        assert_eq!(rows[1].mark, "B");
        assert!(rows[0].transition_duration_seconds > 0.0);
    }
    #[test]
    fn template_presets_apply_to_tsv_in_both_locales() {
        let mut doc = Document::demo(1, 1);
        doc.grid.coordinate_notation.production_template =
            crate::coordinates::ProductionTemplatePreset::Compact;
        assert!(
            production_sheet_text(&doc, Locale::Ja)
                .starts_with("#\tマーク\tタイトル\t拍数\t備考\n")
        );
        assert!(
            production_sheet_text(&doc, Locale::En).starts_with("#\tMark\tTitle\tCounts\tNotes\n")
        );
    }
    #[test]
    fn localized_text_has_one_line_per_set() {
        let doc = Document::demo(1, 1);
        let text = production_sheet_text(&doc, Locale::Ja);
        assert!(text.starts_with("#\tマーク"));
        assert_eq!(text.lines().count(), doc.sets.len() + 1);
    }
}
