//! Safe, deterministic import of public tabular interchange formats.
//!
//! This crate deliberately does not parse proprietary drill files. Export
//! those files to CSV in their originating application first.

use drill_core::{
    Document, GridConfig, OptionalColor, Performer, PerformerId, PerformerKind, Point,
    SCHEMA_VERSION, Section, SectionId, Set, SetId, Symbol,
};
use std::collections::{BTreeMap, BTreeSet};

pub mod coordinate_phrase;
pub mod font_outline;
pub mod musical;
pub mod underlay;
pub mod xlsx;

type CoordinateKey = (String, String);
type CoordinateCell = (Point, Option<String>, Option<u16>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delimiter {
    Comma,
    Tab,
    Semicolon,
}

impl Delimiter {
    const fn byte(self) -> u8 {
        match self {
            Self::Comma => b',',
            Self::Tab => b'\t',
            Self::Semicolon => b';',
        }
    }
}

#[derive(Clone, Debug)]
pub struct ImportLimits {
    pub max_bytes: usize,
    pub max_rows: usize,
    pub max_columns: usize,
    pub max_field_bytes: usize,
    pub max_preview_rows: usize,
    pub max_reported_rows: usize,
    pub max_performers: usize,
    pub max_sets: usize,
}

impl Default for ImportLimits {
    fn default() -> Self {
        Self {
            max_bytes: 64 << 20,
            max_rows: 1_000_000,
            max_columns: 256,
            max_field_bytes: 64 << 10,
            max_preview_rows: 30,
            max_reported_rows: 200,
            max_performers: drill_core::MAX_PERFORMERS,
            max_sets: drill_core::MAX_SETS,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportError {
    Limit { what: &'static str, limit: usize },
    InvalidEncoding,
    Malformed { line: usize, message: &'static str },
    MissingColumn(&'static str),
    DuplicateColumn(&'static str),
    NoUsableRows,
    InvalidDocument(String),
    Cancelled,
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Limit { what, limit } => write!(f, "{what} exceeds safety limit ({limit})"),
            Self::InvalidEncoding => {
                f.write_str("input is not UTF-8 (save the table as UTF-8 CSV)")
            }
            Self::Malformed { line, message } => write!(f, "line {line}: {message}"),
            Self::MissingColumn(c) => write!(f, "required column is not mapped: {c}"),
            Self::DuplicateColumn(c) => write!(f, "column is mapped more than once: {c}"),
            Self::NoUsableRows => f.write_str("no usable coordinate rows were found"),
            Self::InvalidDocument(e) => write!(f, "imported document is invalid: {e}"),
            Self::Cancelled => f.write_str("import cancelled"),
        }
    }
}

impl std::error::Error for ImportError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnMapping {
    pub performer: usize,
    pub set: usize,
    pub x: usize,
    pub y: usize,
    pub counts: Option<usize>,
    pub section: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct TabularPreview {
    pub delimiter: Delimiter,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub total_rows: usize,
    pub truncated: bool,
    pub replacement_characters: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CoordinatePhraseDiagnostic {
    pub line: usize,
    pub point: Option<Point>,
    pub error: Option<coordinate_phrase::CoordinatePhraseError>,
}

/// Parses only preview rows whose mapped coordinate cells are not both numeric.
/// This is bounded by `TabularPreview::rows` and never mutates a document.
pub fn preview_coordinate_phrases(
    preview: &TabularPreview,
    mapping: &ColumnMapping,
    grid: &GridConfig,
) -> Vec<CoordinatePhraseDiagnostic> {
    preview
        .rows
        .iter()
        .enumerate()
        .filter_map(|(offset, row)| {
            let lateral = row.get(mapping.x)?.trim();
            let depth = row.get(mapping.y)?.trim();
            if lateral.parse::<f32>().is_ok() && depth.parse::<f32>().is_ok() {
                return None;
            }
            match coordinate_phrase::parse_coordinate_phrases(lateral, depth, grid) {
                Ok((_, point)) => Some(CoordinatePhraseDiagnostic {
                    line: offset + 2,
                    point: Some(point),
                    error: None,
                }),
                Err(error) => Some(CoordinatePhraseDiagnostic {
                    line: offset + 2,
                    point: None,
                    error: Some(error),
                }),
            }
        })
        .collect()
}

#[derive(Clone, Debug)]
pub struct ImportPlan {
    pub mapping: ColumnMapping,
    pub delimiter: Delimiter,
    pub grid: GridConfig,
    pub limits: ImportLimits,
    pub ignored_columns: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SkipReason {
    Empty,
    MissingValue(&'static str),
    InvalidNumber(&'static str),
    InvalidCoordinatePhrase { axis: coordinate_phrase::PhraseAxis },
    AmbiguousCoordinatePhrase { yard_line: f32 },
    NonFinite,
    OutOfField { x: f32, y: f32 },
    DuplicateKey,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SkippedRow {
    pub line: usize,
    pub reason: SkipReason,
    pub excerpt: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ImportWarning {
    CountsDefaulted { set: String, value: u16 },
    HeldPreviousSet { performer: String, set: String },
    SectionCreated(String),
    ReplacementCharacters,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportReport {
    pub rows_read: usize,
    pub rows_accepted: usize,
    pub rows_skipped: usize,
    pub skipped_detail: Vec<SkippedRow>,
    pub performers_created: usize,
    pub sets_created: usize,
    pub sections_created: usize,
    pub warnings: Vec<ImportWarning>,
    pub ignored_columns: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ImportOutcome {
    pub document: Document,
    pub report: ImportReport,
}

/// A bounded, index-based preview of how an imported table differs from the
/// open document. Indices refer to `imported`, so labels can be duplicated in
/// the UI without becoming identity keys during application.
#[derive(Clone, Debug)]
pub struct ImportDiff {
    pub performer_existing: Vec<Option<usize>>,
    pub set_existing: Vec<Option<usize>>,
    pub rows: Vec<CoordinateDiff>,
    pub performers_added: usize,
    pub performers_changed: usize,
    pub sets_added: usize,
    pub sets_changed: usize,
    pub coordinates_moved: usize,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoordinateDiff {
    pub imported_set: usize,
    pub imported_performer: usize,
    pub before: Option<Point>,
    pub after: Point,
}

#[derive(Clone, Debug)]
pub struct DiffSelection {
    /// Selecting a set applies every coordinate and its count value.
    pub sets: Vec<bool>,
    /// Individual coordinate selections; a selected set takes precedence.
    pub rows: Vec<bool>,
}

impl DiffSelection {
    pub fn all(diff: &ImportDiff) -> Self {
        Self {
            sets: vec![true; diff.set_existing.len()],
            rows: vec![true; diff.rows.len()],
        }
    }

    pub fn none(diff: &ImportDiff) -> Self {
        Self {
            sets: vec![false; diff.set_existing.len()],
            rows: vec![false; diff.rows.len()],
        }
    }
}

/// Computes an O(performers * sets) diff. The existing document is borrowed
/// and remains untouched until `merge_selected` has produced and validated a
/// separate replacement document.
pub fn diff_documents(current: &Document, imported: &Document) -> ImportDiff {
    let performer_by_label = current
        .performers
        .iter()
        .enumerate()
        .map(|(index, performer)| (performer.label.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    let set_by_name = current
        .sets
        .iter()
        .enumerate()
        .map(|(index, set)| (set.name.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    let performer_existing = imported
        .performers
        .iter()
        .map(|performer| performer_by_label.get(performer.label.as_str()).copied())
        .collect::<Vec<_>>();
    let set_existing = imported
        .sets
        .iter()
        .map(|set| set_by_name.get(set.name.as_str()).copied())
        .collect::<Vec<_>>();
    let mut rows = Vec::new();
    let mut sets_changed = 0;
    for (imported_set, set) in imported.sets.iter().enumerate() {
        let existing_set = set_existing[imported_set];
        let count_changed = existing_set.is_some_and(|i| current.sets[i].counts != set.counts);
        let before_len = rows.len();
        for (imported_performer, &after) in set.positions.iter().enumerate() {
            let before = existing_set.and_then(|si| {
                performer_existing[imported_performer]
                    .and_then(|pi| current.sets[si].positions.get(pi).copied())
            });
            if before != Some(after) {
                rows.push(CoordinateDiff {
                    imported_set,
                    imported_performer,
                    before,
                    after,
                });
            }
        }
        if count_changed || before_len != rows.len() {
            sets_changed += 1;
        }
    }
    let performers_added = performer_existing.iter().filter(|v| v.is_none()).count();
    let performers_changed = imported
        .performers
        .iter()
        .enumerate()
        .filter(|(ii, performer)| {
            performer_existing[*ii].is_some_and(|ci| {
                let imported_section = imported
                    .sections
                    .iter()
                    .find(|section| section.id == performer.section)
                    .map(|section| section.name.as_str());
                let current_performer = &current.performers[ci];
                let current_section = current
                    .sections
                    .iter()
                    .find(|section| section.id == current_performer.section)
                    .map(|section| section.name.as_str());
                imported_section != current_section
            })
        })
        .count();
    let sets_added = set_existing.iter().filter(|v| v.is_none()).count();
    let coordinates_moved = rows.iter().filter(|row| row.before.is_some()).count();
    let mut warnings = Vec::new();
    if current.grid != imported.grid {
        warnings.push("Imported grid differs; current grid will be preserved".into());
    }
    if imported
        .performers
        .iter()
        .map(|p| &p.label)
        .collect::<BTreeSet<_>>()
        .len()
        != imported.performers.len()
    {
        warnings.push("Duplicate performer labels were matched by first occurrence".into());
    }
    ImportDiff {
        performer_existing,
        set_existing,
        rows,
        performers_added,
        performers_changed,
        sets_added,
        sets_changed,
        coordinates_moved,
        warnings,
    }
}

/// Builds a new document from the selection without mutating `current`.
/// Existing audio, tempo, cameras, grid and document identity are preserved.
pub fn merge_selected(
    current: &Document,
    imported: &Document,
    diff: &ImportDiff,
    selection: &DiffSelection,
) -> Result<Document, ImportError> {
    if selection.sets.len() != imported.sets.len() || selection.rows.len() != diff.rows.len() {
        return Err(ImportError::InvalidDocument(
            "invalid diff selection".into(),
        ));
    }
    let selected_row = |index: usize, row: &CoordinateDiff| {
        selection.sets[row.imported_set] || selection.rows[index]
    };
    let mut needed_performers = vec![false; imported.performers.len()];
    let mut needed_sets = vec![false; imported.sets.len()];
    for (index, row) in diff.rows.iter().enumerate() {
        if selected_row(index, row) {
            needed_performers[row.imported_performer] = true;
            needed_sets[row.imported_set] = true;
        }
    }
    for (index, selected) in selection.sets.iter().copied().enumerate() {
        if selected {
            needed_sets[index] = true;
            needed_performers.fill(true);
        }
    }

    let mut merged = current.clone();
    let mut imported_to_merged_performer = diff.performer_existing.clone();
    let mut imported_to_merged_section = vec![None; imported.sections.len()];
    let mut next_section_id = merged
        .sections
        .iter()
        .map(|section| section.id.get())
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    for (ii, section) in imported.sections.iter().enumerate() {
        if let Some(existing) = merged.sections.iter().position(|v| v.name == section.name) {
            imported_to_merged_section[ii] = Some(existing);
            continue;
        }
        let used = imported
            .performers
            .iter()
            .enumerate()
            .any(|(pi, performer)| needed_performers[pi] && performer.section == section.id);
        if used {
            let mut section = section.clone();
            section.id = SectionId::new(next_section_id)
                .ok_or_else(|| ImportError::InvalidDocument("section id space exhausted".into()))?;
            section.order = merged.sections.len() as u16;
            next_section_id = next_section_id.saturating_add(1);
            imported_to_merged_section[ii] = Some(merged.sections.len());
            merged.sections.push(section);
        }
    }
    let mut next_performer_id = merged
        .performers
        .iter()
        .map(|p| p.id.get())
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    for (ii, performer) in imported.performers.iter().enumerate() {
        let imported_section_index = imported
            .sections
            .iter()
            .position(|section| section.id == performer.section);
        let merged_section = imported_section_index
            .and_then(|index| imported_to_merged_section[index])
            .and_then(|index| merged.sections.get(index).map(|section| section.id));
        if imported_to_merged_performer[ii].is_none() && needed_performers[ii] {
            let mut performer = performer.clone();
            performer.id = PerformerId::new(next_performer_id).ok_or_else(|| {
                ImportError::InvalidDocument("performer id space exhausted".into())
            })?;
            next_performer_id = next_performer_id.saturating_add(1);
            // Imported section IDs are not meaningful in the current document.
            performer.section = merged_section
                .or_else(|| merged.sections.first().map(|s| s.id))
                .ok_or_else(|| {
                    ImportError::InvalidDocument("current document has no section".into())
                })?;
            let new_index = merged.performers.len();
            merged.performers.push(performer);
            for set in &mut merged.sets {
                set.positions.push(Point::default());
            }
            imported_to_merged_performer[ii] = Some(new_index);
        } else if needed_performers[ii]
            && let (Some(pi), Some(section)) = (imported_to_merged_performer[ii], merged_section)
        {
            merged.performers[pi].section = section;
        }
    }

    let mut imported_to_merged_set = diff.set_existing.clone();
    let mut next_set_id = merged
        .sets
        .iter()
        .map(|s| s.id.get())
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    for (ii, set) in imported.sets.iter().enumerate() {
        if imported_to_merged_set[ii].is_none() && needed_sets[ii] {
            let new_index = merged.sets.len();
            merged.sets.push(Set {
                id: SetId::new(next_set_id)
                    .ok_or_else(|| ImportError::InvalidDocument("set id space exhausted".into()))?,
                name: set.name.clone(),
                annotation: set.annotation.clone(),
                counts: set.counts,
                hold: set.hold,
                routes: set.routes.clone(),
                shape: set.shape.clone(),
                positions: vec![Point::default(); merged.performers.len()],
            });
            next_set_id = next_set_id.saturating_add(1);
            imported_to_merged_set[ii] = Some(new_index);
        }
        if selection.sets[ii]
            && let Some(mi) = imported_to_merged_set[ii]
        {
            merged.sets[mi].counts = set.counts;
        }
    }
    for (index, row) in diff.rows.iter().enumerate() {
        if !selected_row(index, row) {
            continue;
        }
        let Some(si) = imported_to_merged_set[row.imported_set] else {
            continue;
        };
        let Some(pi) = imported_to_merged_performer[row.imported_performer] else {
            continue;
        };
        merged.sets[si].positions[pi] = row.after;
    }
    merged
        .validate()
        .map_err(|e| ImportError::InvalidDocument(e.to_string()))?;
    Ok(merged)
}

pub fn sniff(bytes: &[u8], limits: &ImportLimits) -> Result<TabularPreview, ImportError> {
    if bytes.len() > limits.max_bytes {
        return Err(ImportError::Limit {
            what: "input bytes",
            limit: limits.max_bytes,
        });
    }
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let text = std::str::from_utf8(bytes).map_err(|_| ImportError::InvalidEncoding)?;
    let delimiter = [Delimiter::Comma, Delimiter::Tab, Delimiter::Semicolon]
        .into_iter()
        .max_by_key(|d| delimiter_score(text, d.byte()))
        .unwrap();
    let records = parse_records(text, delimiter.byte(), limits)?;
    let Some(headers) = records.first().cloned() else {
        return Err(ImportError::NoUsableRows);
    };
    if headers.is_empty() {
        return Err(ImportError::NoUsableRows);
    }
    let total_rows = records.len().saturating_sub(1);
    Ok(TabularPreview {
        delimiter,
        headers,
        rows: records
            .into_iter()
            .skip(1)
            .take(limits.max_preview_rows)
            .collect(),
        total_rows,
        truncated: total_rows > limits.max_preview_rows,
        replacement_characters: text.contains('\u{fffd}'),
    })
}

pub fn suggest_mapping(headers: &[String]) -> Result<ColumnMapping, ImportError> {
    fn find(headers: &[String], names: &[&str], role: &'static str) -> Result<usize, ImportError> {
        let hits = headers
            .iter()
            .enumerate()
            .filter(|(_, h)| names.iter().any(|n| normalize(h) == *n))
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        match hits.as_slice() {
            [one] => Ok(*one),
            [] => Err(ImportError::MissingColumn(role)),
            _ => Err(ImportError::DuplicateColumn(role)),
        }
    }
    fn optional(headers: &[String], names: &[&str]) -> Option<usize> {
        headers
            .iter()
            .position(|h| names.iter().any(|n| normalize(h) == *n))
    }
    Ok(ColumnMapping {
        performer: find(
            headers,
            &[
                "performer",
                "label",
                "performerid",
                "performerlabel",
                "marcher",
                "marcherid",
                "dot",
                "dotnumber",
                "drillnumber",
                "演者",
                "演者名",
                "番号",
                "ドリル番号",
                "ラベル",
            ],
            "performer",
        )?,
        set: find(
            headers,
            &[
                "set",
                "setname",
                "setnumber",
                "page",
                "chart",
                "セット",
                "セット名",
                "ページ",
                "場面",
            ],
            "set",
        )?,
        x: find(
            headers,
            &[
                "x",
                "xposition",
                "sidetoside",
                "sideline",
                "lateral",
                "horizontal",
                "横",
                "左右",
                "サイド",
                "横位置",
            ],
            "x",
        )?,
        y: find(
            headers,
            &[
                "y",
                "yposition",
                "fronttoback",
                "depth",
                "vertical",
                "縦",
                "前後",
                "深さ",
                "縦位置",
            ],
            "y",
        )?,
        counts: optional(
            headers,
            &[
                "counts",
                "count",
                "duration",
                "beats",
                "カウント",
                "拍",
                "拍数",
            ],
        ),
        section: optional(
            headers,
            &[
                "section",
                "part",
                "instrument",
                "セクション",
                "パート",
                "楽器",
            ],
        ),
    })
}

pub fn plan_import(
    preview: &TabularPreview,
    mapping: ColumnMapping,
    grid: GridConfig,
    limits: ImportLimits,
) -> Result<ImportPlan, ImportError> {
    let required = [
        (mapping.performer, "performer"),
        (mapping.set, "set"),
        (mapping.x, "x"),
        (mapping.y, "y"),
    ];
    if let Some((_, role)) = required.iter().find(|(i, _)| *i >= preview.headers.len()) {
        return Err(ImportError::MissingColumn(role));
    }
    let distinct = required.iter().map(|v| v.0).collect::<BTreeSet<_>>();
    if distinct.len() != required.len() {
        return Err(ImportError::DuplicateColumn("required role"));
    }
    for index in [mapping.counts, mapping.section].into_iter().flatten() {
        if index >= preview.headers.len() {
            return Err(ImportError::MissingColumn("optional role"));
        }
    }
    let used = required
        .iter()
        .map(|v| v.0)
        .chain([mapping.counts, mapping.section].into_iter().flatten())
        .collect::<BTreeSet<_>>();
    let ignored_columns = preview
        .headers
        .iter()
        .enumerate()
        .filter(|(i, _)| !used.contains(i))
        .map(|(_, h)| h.clone())
        .collect();
    Ok(ImportPlan {
        mapping,
        delimiter: preview.delimiter,
        grid,
        limits,
        ignored_columns,
    })
}

pub fn import_tabular_as_document(
    bytes: &[u8],
    plan: &ImportPlan,
    mut progress: impl FnMut(f32) -> bool,
) -> Result<ImportOutcome, ImportError> {
    if bytes.len() > plan.limits.max_bytes {
        return Err(ImportError::Limit {
            what: "input bytes",
            limit: plan.limits.max_bytes,
        });
    }
    let text = std::str::from_utf8(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes))
        .map_err(|_| ImportError::InvalidEncoding)?;
    let records = parse_records(text, plan.delimiter.byte(), &plan.limits)?;
    let mut points: BTreeMap<CoordinateKey, CoordinateCell> = BTreeMap::new();
    let mut performer_order = Vec::new();
    let mut performer_seen = BTreeSet::new();
    let mut set_order = Vec::new();
    let mut set_seen = BTreeSet::new();
    let mut report = ImportReport {
        ignored_columns: plan.ignored_columns.clone(),
        ..Default::default()
    };
    if text.contains('\u{fffd}') {
        report.warnings.push(ImportWarning::ReplacementCharacters);
    }
    let total = records.len().saturating_sub(1).max(1);
    for (offset, row) in records.iter().skip(1).enumerate() {
        if !progress(offset as f32 / total as f32) {
            return Err(ImportError::Cancelled);
        }
        report.rows_read += 1;
        let line = offset + 2;
        let get = |i: usize| row.get(i).map(|s| s.trim()).unwrap_or("");
        let performer = get(plan.mapping.performer);
        let set = get(plan.mapping.set);
        let reason = if performer.is_empty() || set.is_empty() {
            Some(SkipReason::MissingValue(if performer.is_empty() {
                "performer"
            } else {
                "set"
            }))
        } else {
            let lateral = get(plan.mapping.x);
            let depth = get(plan.mapping.y);
            let parsed = match (lateral.parse::<f32>(), depth.parse::<f32>()) {
                (Ok(x), Ok(y)) => Ok((x, y)),
                _ => coordinate_phrase::parse_coordinate_phrases(lateral, depth, &plan.grid)
                    .map(|(_, point)| (point.x, point.y)),
            };
            match parsed {
                Ok((x, y)) if !x.is_finite() || !y.is_finite() => Some(SkipReason::NonFinite),
                Ok((x, y)) if x < 0.0 || y < 0.0 || x > plan.grid.width || y > plan.grid.height => {
                    Some(SkipReason::OutOfField { x, y })
                }
                Ok((x, y)) => {
                    let key = (performer.to_owned(), set.to_owned());
                    if let std::collections::btree_map::Entry::Vacant(entry) = points.entry(key) {
                        let section = plan
                            .mapping
                            .section
                            .map(|i| get(i).to_owned())
                            .filter(|s| !s.is_empty());
                        let counts = plan
                            .mapping
                            .counts
                            .and_then(|i| get(i).parse::<u16>().ok())
                            .filter(|v| *v > 0);
                        entry.insert((Point { x, y }, section, counts));
                        if performer_seen.insert(performer.to_owned()) {
                            performer_order.push(performer.to_owned());
                        }
                        if set_seen.insert(set.to_owned()) {
                            set_order.push(set.to_owned());
                        }
                        report.rows_accepted += 1;
                        None
                    } else {
                        Some(SkipReason::DuplicateKey)
                    }
                }
                Err(coordinate_phrase::CoordinatePhraseError::AmbiguousSide { yard_line }) => {
                    Some(SkipReason::AmbiguousCoordinatePhrase { yard_line })
                }
                Err(coordinate_phrase::CoordinatePhraseError::Unsupported { axis }) => {
                    Some(SkipReason::InvalidCoordinatePhrase { axis })
                }
                Err(coordinate_phrase::CoordinatePhraseError::UnknownReference) => {
                    Some(SkipReason::InvalidCoordinatePhrase {
                        axis: coordinate_phrase::PhraseAxis::Depth,
                    })
                }
                Err(_) => Some(SkipReason::InvalidNumber("x/y or coordinate phrase")),
            }
        };
        if let Some(reason) = reason {
            report.rows_skipped += 1;
            if report.skipped_detail.len() < plan.limits.max_reported_rows {
                report.skipped_detail.push(SkippedRow {
                    line,
                    reason,
                    excerpt: row.join(" | ").chars().take(120).collect(),
                });
            }
        }
    }
    if points.is_empty() {
        return Err(ImportError::NoUsableRows);
    }
    if performer_order.len() > plan.limits.max_performers {
        return Err(ImportError::Limit {
            what: "performers",
            limit: plan.limits.max_performers,
        });
    }
    if set_order.len() > plan.limits.max_sets {
        return Err(ImportError::Limit {
            what: "sets",
            limit: plan.limits.max_sets,
        });
    }
    let mut section_names = Vec::new();
    let mut section_seen = BTreeSet::new();
    for performer in &performer_order {
        if let Some(name) = set_order.iter().find_map(|set| {
            points
                .get(&(performer.clone(), set.clone()))
                .and_then(|v| v.1.clone())
        }) && section_seen.insert(name.clone())
        {
            section_names.push(name);
        }
    }
    if section_names.is_empty() {
        section_names.push("Ensemble".into());
    }
    let sections = section_names
        .iter()
        .enumerate()
        .map(|(i, name)| Section {
            id: SectionId::new(i as u32 + 1).unwrap(),
            name: name.clone(),
            short: name.chars().take(4).collect(),
            color: [128, 160, 210],
            order: i as u16,
        })
        .collect::<Vec<_>>();
    let performers = performer_order
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let section_name = set_order
                .iter()
                .find_map(|set| {
                    points
                        .get(&(label.clone(), set.clone()))
                        .and_then(|v| v.1.as_ref())
                })
                .map(String::as_str)
                .unwrap_or(&section_names[0]);
            let section_index = section_names
                .iter()
                .position(|v| v == section_name)
                .unwrap_or(0);
            Performer {
                id: PerformerId::new(i as u32 + 1).unwrap(),
                label: label.clone(),
                section: sections[section_index].id,
                symbol: Symbol::Circle,
                color: OptionalColor(None),
                height_m: 1.7,
                kind: PerformerKind::Wind,
            }
        })
        .collect::<Vec<_>>();
    let mut previous = vec![None; performers.len()];
    let sets = set_order
        .iter()
        .enumerate()
        .map(|(si, name)| {
            let mut positions = Vec::with_capacity(performers.len());
            let mut counts = None;
            for (pi, performer) in performer_order.iter().enumerate() {
                if let Some((point, _, row_counts)) = points.get(&(performer.clone(), name.clone()))
                {
                    positions.push(*point);
                    previous[pi] = Some(*point);
                    counts = counts.or(*row_counts);
                } else {
                    let held = previous[pi]
                        .or_else(|| {
                            set_order.iter().skip(si + 1).find_map(|future| {
                                points
                                    .get(&(performer.clone(), future.clone()))
                                    .map(|v| v.0)
                            })
                        })
                        .unwrap_or_default();
                    positions.push(held);
                    report.warnings.push(ImportWarning::HeldPreviousSet {
                        performer: performer.clone(),
                        set: name.clone(),
                    });
                }
            }
            let counts = counts.unwrap_or_else(|| {
                report.warnings.push(ImportWarning::CountsDefaulted {
                    set: name.clone(),
                    value: 8,
                });
                8
            });
            Set {
                id: SetId::new(si as u32 + 1).unwrap(),
                name: name.clone(),
                annotation: drill_core::SetAnnotation::default(),
                counts,
                hold: 0,
                routes: drill_core::RouteTable::default(),
                shape: None,
                positions,
            }
        })
        .collect();
    report.performers_created = performers.len();
    report.sets_created = set_order.len();
    report.sections_created = sections.len();
    for name in &section_names {
        report
            .warnings
            .push(ImportWarning::SectionCreated(name.clone()));
    }
    let document = Document {
        schema_version: SCHEMA_VERSION,
        title: "Imported coordinates".into(),
        grid: plan.grid.clone(),
        tempo: Default::default(),
        audio: None,
        camera_program: drill_core::camera::CameraProgram::default_for_grid(&plan.grid),
        underlay: None,
        sections,
        subsets: Vec::new(),
        performers,
        sets,
    };
    document
        .validate()
        .map_err(|e| ImportError::InvalidDocument(e.to_string()))?;
    progress(1.0);
    Ok(ImportOutcome { document, report })
}

fn normalize(value: &str) -> String {
    value
        .trim()
        .trim_start_matches('\u{feff}')
        .to_lowercase()
        .chars()
        .filter_map(|c| match c {
            '\u{ff10}'..='\u{ff19}' => char::from_u32(c as u32 - 0xff10 + 0x30),
            '\u{ff21}'..='\u{ff3a}' => char::from_u32(c as u32 - 0xff21 + 0x61),
            '\u{ff41}'..='\u{ff5a}' => char::from_u32(c as u32 - 0xff41 + 0x61),
            c if c.is_alphanumeric() => Some(c),
            _ => None,
        })
        .collect()
}
fn delimiter_score(text: &str, delimiter: u8) -> usize {
    text.lines()
        .take(8)
        .map(|line| line.as_bytes().iter().filter(|b| **b == delimiter).count())
        .sum()
}

fn parse_records(
    text: &str,
    delimiter: u8,
    limits: &ImportLimits,
) -> Result<Vec<Vec<String>>, ImportError> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    let mut line = 1;
    while let Some(ch) = chars.next() {
        if quoted {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                }
            } else {
                if ch == '\n' {
                    line += 1;
                }
                field.push(ch);
            }
        } else if ch == '"' && field.is_empty() {
            quoted = true;
        } else if ch as u32 == delimiter as u32 {
            push_field(&mut row, &mut field, limits)?;
        } else if ch == '\n' {
            push_field(&mut row, &mut field, limits)?;
            if row.len() > limits.max_columns {
                return Err(ImportError::Limit {
                    what: "columns",
                    limit: limits.max_columns,
                });
            }
            rows.push(std::mem::take(&mut row));
            line += 1;
            if rows.len() > limits.max_rows + 1 {
                return Err(ImportError::Limit {
                    what: "rows",
                    limit: limits.max_rows,
                });
            }
        } else if ch != '\r' {
            field.push(ch);
            if field.len() > limits.max_field_bytes {
                return Err(ImportError::Limit {
                    what: "field bytes",
                    limit: limits.max_field_bytes,
                });
            }
        }
    }
    if quoted {
        return Err(ImportError::Malformed {
            line,
            message: "unterminated quoted field",
        });
    }
    if !field.is_empty() || !row.is_empty() {
        push_field(&mut row, &mut field, limits)?;
        rows.push(row);
    }
    Ok(rows)
}
fn push_field(
    row: &mut Vec<String>,
    field: &mut String,
    limits: &ImportLimits,
) -> Result<(), ImportError> {
    if field.len() > limits.max_field_bytes {
        return Err(ImportError::Limit {
            what: "field bytes",
            limit: limits.max_field_bytes,
        });
    }
    row.push(std::mem::take(field));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(input: &str) -> ImportOutcome {
        let limits = ImportLimits::default();
        let preview = sniff(input.as_bytes(), &limits).unwrap();
        let mapping = suggest_mapping(&preview.headers).unwrap();
        let plan = plan_import(&preview, mapping, GridConfig::default(), limits).unwrap();
        import_tabular_as_document(input.as_bytes(), &plan, |_| true).unwrap()
    }
    #[test]
    fn imports_long_csv_deterministically() {
        let input = "performer,set,x,y,counts,section\r\nA1,Set 1,10,20,8,Brass\r\nA2,Set 1,11,21,8,Brass\r\nA1,Set 2,12,22,16,Brass\r\nA2,Set 2,13,23,16,Brass\r\n";
        let a = run(input);
        let b = run(input);
        assert_eq!(a.document.to_json().unwrap(), b.document.to_json().unwrap());
        assert_eq!(a.report.rows_accepted, 4);
        assert_eq!(a.document.sets[1].counts, 16);
    }
    #[test]
    fn handles_rfc4180_quotes_and_tab_tables() {
        let out = run("label\tset\tx\ty\n\"A\t1\"\tOne\t1\t2\n");
        assert_eq!(out.document.performers[0].label, "A\t1");
    }

    #[test]
    fn mapping_accepts_real_world_aliases_and_fullwidth_headers() {
        let headers = [
            "\u{feff}Ｄｒｉｌｌ Ｎｕｍｂｅｒ",
            "ページ",
            "左右",
            "深さ",
            "拍数",
            "楽器",
        ]
        .map(str::to_owned);
        let mapping = suggest_mapping(&headers).unwrap();
        assert_eq!(
            (mapping.performer, mapping.set, mapping.x, mapping.y),
            (0, 1, 2, 3)
        );
        assert_eq!(mapping.counts, Some(4));
        assert_eq!(mapping.section, Some(5));
    }
    #[test]
    fn rejects_hostile_limits_and_non_utf8() {
        let l = ImportLimits {
            max_bytes: 2,
            ..ImportLimits::default()
        };
        assert!(matches!(sniff(b"a,b", &l), Err(ImportError::Limit { .. })));
        assert_eq!(
            sniff(&[0xff], &ImportLimits::default()).unwrap_err(),
            ImportError::InvalidEncoding
        );
    }
    #[test]
    fn dry_run_reports_bad_rows_without_mutation() {
        let input = "performer,set,x,y\nA,One,1,2\nA,One,3,4\nB,One,nan,4\nC,One,999,4\n";
        let out = run(input);
        assert_eq!(out.report.rows_accepted, 1);
        assert_eq!(out.report.rows_skipped, 3);
        assert_eq!(out.document.performers.len(), 1);
    }
    #[test]
    fn incomplete_later_set_holds_previous_position_and_reports() {
        let out = run("performer,set,x,y\nA,One,1,2\nB,One,3,4\nA,Two,5,6\n");
        assert_eq!(out.document.sets[1].positions[1], Point { x: 3.0, y: 4.0 });
        assert!(
            out.report.warnings.iter().any(
                |w| matches!(w,ImportWarning::HeldPreviousSet{performer,..} if performer=="B")
            )
        );
    }

    #[test]
    fn leading_gap_uses_first_known_position_instead_of_field_origin() {
        let out = run("performer,set,x,y\nA,One,1,2\nA,Two,5,6\nB,Two,7,8\n");
        assert_eq!(out.document.sets[0].positions[1], Point { x: 7.0, y: 8.0 });
    }
    #[test]
    fn malformed_quote_is_rejected() {
        assert!(matches!(
            sniff(b"a,b\n\"oops", &ImportLimits::default()),
            Err(ImportError::Malformed { .. })
        ));
    }

    #[test]
    fn diff_and_selective_merge_preserve_open_project_state() {
        let current = run("performer,set,x,y,counts\nA,One,1,2,8\nB,One,3,4,8\n").document;
        let mut current = current;
        current.title = "Keep me".into();
        current.tempo = drill_core::tempo::TempoMap::constant(144.0);
        let imported = run("performer,set,x,y,counts\nA,One,9,9,16\nB,One,3,4,16\nC,One,7,8,16\nA,Two,5,6,12\nB,Two,6,7,12\nC,Two,7,8,12\n").document;
        let diff = diff_documents(&current, &imported);
        assert_eq!(diff.performers_added, 1);
        assert_eq!(diff.sets_added, 1);
        assert_eq!(diff.coordinates_moved, 1);
        let mut selection = DiffSelection::none(&diff);
        let moved = diff
            .rows
            .iter()
            .position(|row| {
                imported.performers[row.imported_performer].label == "A"
                    && imported.sets[row.imported_set].name == "One"
            })
            .unwrap();
        selection.rows[moved] = true;
        let merged = merge_selected(&current, &imported, &diff, &selection).unwrap();
        assert_eq!(merged.title, "Keep me");
        assert_eq!(merged.tempo.bpm_at(0.0), 144.0);
        assert_eq!(merged.performers.len(), 2);
        assert_eq!(merged.sets.len(), 1);
        assert_eq!(merged.sets[0].positions[0], Point { x: 9.0, y: 9.0 });
        assert_eq!(merged.sets[0].counts, 8);
    }

    #[test]
    fn selecting_new_set_adds_required_performers_and_valid_document() {
        let current = run("performer,set,x,y\nA,One,1,2\n").document;
        let imported = run("performer,set,x,y\nA,One,1,2\nA,Two,3,4\nC,Two,5,6\n").document;
        let diff = diff_documents(&current, &imported);
        let mut selection = DiffSelection::none(&diff);
        selection.sets[1] = true;
        let merged = merge_selected(&current, &imported, &diff, &selection).unwrap();
        assert_eq!(merged.performers.len(), 2);
        assert_eq!(merged.sets.len(), 2);
        assert_eq!(
            merged.sets[1].positions,
            vec![Point { x: 3.0, y: 4.0 }, Point { x: 5.0, y: 6.0 }]
        );
        merged.validate().unwrap();
    }

    #[test]
    fn imports_human_coordinate_columns_and_reports_ambiguity() {
        let bytes = b"performer,set,side_to_side,front_to_back\nA,One,Side 1: 2 steps inside the 45 yard line,4 steps behind the front hash\n";
        let limits = ImportLimits::default();
        let preview = sniff(bytes, &limits).unwrap();
        let mapping = suggest_mapping(&preview.headers).unwrap();
        let diagnostics = preview_coordinate_phrases(&preview, &mapping, &GridConfig::default());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].point, Some(Point { x: 46.25, y: 22.5 }));
        let plan = plan_import(&preview, mapping, GridConfig::default(), limits).unwrap();
        let outcome = import_tabular_as_document(bytes, &plan, |_| true).unwrap();
        assert_eq!(
            outcome.document.sets[0].positions[0],
            Point { x: 46.25, y: 22.5 }
        );

        let ambiguous = b"performer,set,x,y\nA,One,2 inside the 45,front sideline\n";
        let preview = sniff(ambiguous, &ImportLimits::default()).unwrap();
        let mapping = suggest_mapping(&preview.headers).unwrap();
        let diagnostic = preview_coordinate_phrases(&preview, &mapping, &GridConfig::default());
        assert!(matches!(
            diagnostic[0].error,
            Some(coordinate_phrase::CoordinatePhraseError::AmbiguousSide { .. })
        ));
    }
}
