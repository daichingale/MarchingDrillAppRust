use crate::page::{PageItem, ReportDocument};
use drill_core::{Document, Locale, continuity, coordinates, countsheet, production};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportKind {
    PerformerDrillBook,
    SetChart,
    CountSheet,
    ProductionSheet,
}

pub fn production_sheet(doc: &Document, locale: Locale) -> ReportDocument {
    use drill_core::coordinates::ProductionTemplatePreset;
    let preset = doc.grid.coordinate_notation.production_template;
    let rows = production::production_sheet(doc)
        .into_iter()
        .map(|r| {
            let standard = vec![
                r.set_number.to_string(),
                r.mark,
                r.title,
                r.counts.to_string(),
                format!("{:.1}", r.tempo_bpm),
                format!("{:.3}s", r.sync_time_seconds),
                format!("{:.3}s", r.transition_duration_seconds),
                r.notes,
            ];
            match preset {
                ProductionTemplatePreset::Standard => standard,
                ProductionTemplatePreset::Compact => vec![
                    standard[0].clone(),
                    standard[1].clone(),
                    standard[2].clone(),
                    standard[3].clone(),
                    standard[7].clone(),
                ],
                ProductionTemplatePreset::Rehearsal => vec![
                    standard[1].clone(),
                    standard[2].clone(),
                    standard[3].clone(),
                    standard[4].clone(),
                    standard[7].clone(),
                ],
            }
        })
        .collect();
    let (suffix, standard_columns) = match locale {
        Locale::Ja => (
            "プロダクションシート",
            [
                "#",
                "マーク",
                "タイトル",
                "拍数",
                "テンポ",
                "同期",
                "尺",
                "備考",
            ],
        ),
        Locale::En => (
            "Production Sheet",
            [
                "#", "Mark", "Title", "Counts", "Tempo", "Sync", "Duration", "Notes",
            ],
        ),
    };
    let columns = match preset {
        ProductionTemplatePreset::Standard => standard_columns.map(str::to_owned).to_vec(),
        ProductionTemplatePreset::Compact => match locale {
            Locale::Ja => ["#", "マーク", "タイトル", "拍数", "備考"],
            Locale::En => ["#", "Mark", "Title", "Counts", "Notes"],
        }
        .map(str::to_owned)
        .to_vec(),
        ProductionTemplatePreset::Rehearsal => match locale {
            Locale::Ja => ["マーク", "タイトル", "拍数", "テンポ", "備考"],
            Locale::En => ["Mark", "Title", "Counts", "Tempo", "Notes"],
        }
        .map(str::to_owned)
        .to_vec(),
    };
    ReportDocument {
        title: format!("{} - {}", doc.title, suffix),
        items: vec![
            PageItem::Heading(format!("{} - {}", doc.title, suffix)),
            PageItem::Table { columns, rows },
        ],
    }
}

pub fn count_sheet(doc: &Document, beats_per_measure: u16) -> ReportDocument {
    let rows = countsheet::count_sheet(doc, beats_per_measure)
        .into_iter()
        .map(|r| {
            vec![
                r.rehearsal_mark,
                r.name,
                r.start_count.to_string(),
                r.counts.to_string(),
                format!("{}:{:.1}", r.start_measure, r.start_beat),
                format!("{:.2}s", r.start_seconds),
            ]
        })
        .collect();
    ReportDocument {
        title: format!("{} - Count Sheet", doc.title),
        items: vec![
            PageItem::Heading(doc.title.clone()),
            PageItem::Table {
                columns: ["Mark", "Set", "Start", "Counts", "Measure:Beat", "Time"]
                    .map(str::to_owned)
                    .to_vec(),
                rows,
            },
        ],
    }
}

pub fn performer_drill_book(doc: &Document, performer_indices: &[usize]) -> ReportDocument {
    performer_drill_book_localized(doc, performer_indices, Locale::En)
}

pub fn performer_drill_book_localized(
    doc: &Document,
    performer_indices: &[usize],
    locale: Locale,
) -> ReportDocument {
    let indices: Vec<usize> = if performer_indices.is_empty() {
        (0..doc.performers.len()).collect()
    } else {
        performer_indices.to_vec()
    };
    let mut items = Vec::new();
    for (book, index) in indices.into_iter().enumerate() {
        let Some(performer) = doc.performers.get(index) else {
            continue;
        };
        if book > 0 {
            items.push(PageItem::PageBreak);
        }
        items.push(PageItem::Heading(format!(
            "{} - {}",
            doc.title, performer.label
        )));
        let moves = continuity::performer_continuity(doc, index);
        let rows = doc
            .sets
            .iter()
            .enumerate()
            .map(|(set_index, set)| {
                let coordinate = set
                    .positions
                    .get(index)
                    .map(|&p| coordinates::readable_localized(p, &doc.grid, locale))
                    .unwrap_or_default();
                let movement = set_index
                    .checked_sub(1)
                    .and_then(|i| moves.get(i))
                    .map(|m| continuity::format_segment(m, locale))
                    .unwrap_or_default();
                vec![
                    countsheet::rehearsal_mark(set_index),
                    set.name.clone(),
                    set.counts.to_string(),
                    coordinate,
                    movement,
                ]
            })
            .collect();
        let columns = match locale {
            Locale::Ja => ["マーク", "セット", "拍数", "座標", "コンティニュイティ"],
            Locale::En => ["Mark", "Set", "Counts", "Coordinate", "Continuity"],
        };
        items.push(PageItem::Table {
            columns: columns.map(str::to_owned).to_vec(),
            rows,
        });
    }
    ReportDocument {
        title: format!(
            "{} - {}",
            doc.title,
            if locale == Locale::Ja {
                "ドリルブック"
            } else {
                "Drill Book"
            }
        ),
        items,
    }
}

pub fn set_charts(doc: &Document, set_indices: &[usize]) -> ReportDocument {
    let indices: Vec<usize> = if set_indices.is_empty() {
        (0..doc.sets.len()).collect()
    } else {
        set_indices.to_vec()
    };
    let mut items = Vec::new();
    for (page, index) in indices
        .into_iter()
        .filter(|&i| i < doc.sets.len())
        .enumerate()
    {
        if page > 0 {
            items.push(PageItem::PageBreak);
        }
        items.push(PageItem::Heading(format!(
            "{} - {}",
            countsheet::rehearsal_mark(index),
            doc.sets[index].name
        )));
        items.push(PageItem::FieldDiagram { set_index: index });
    }
    ReportDocument {
        title: format!("{} - Set Charts", doc.title),
        items,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drill_book_has_one_table_per_performer() {
        let d = Document::demo(2, 2);
        let r = performer_drill_book(&d, &[]);
        assert_eq!(
            r.items
                .iter()
                .filter(|i| matches!(i, PageItem::Table { .. }))
                .count(),
            4
        );
    }
    #[test]
    fn charts_reference_valid_sets() {
        let d = Document::demo(1, 2);
        let r = set_charts(&d, &[]);
        assert_eq!(
            r.items
                .iter()
                .filter(|i| matches!(i, PageItem::FieldDiagram { .. }))
                .count(),
            d.sets.len()
        );
    }
    #[test]
    fn production_report_contains_annotations() {
        let mut d = Document::demo(1, 1);
        d.sets[0].annotation.notes = "Lighting cue".into();
        let r = production_sheet(&d, Locale::En);
        let PageItem::Table { columns, rows } = &r.items[1] else {
            panic!()
        };
        assert_eq!(columns.len(), 8);
        assert_eq!(rows[0][7], "Lighting cue");
    }
    #[test]
    fn production_template_presets_change_pdf_table_deterministically() {
        let mut d = Document::demo(1, 1);
        d.grid.coordinate_notation.production_template =
            drill_core::coordinates::ProductionTemplatePreset::Compact;
        let r = production_sheet(&d, Locale::Ja);
        let PageItem::Table { columns, rows } = &r.items[1] else {
            panic!()
        };
        assert_eq!(columns, &["#", "マーク", "タイトル", "拍数", "備考"]);
        assert!(rows.iter().all(|row| row.len() == columns.len()));
    }
}
