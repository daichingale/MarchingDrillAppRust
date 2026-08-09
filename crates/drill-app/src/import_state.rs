use super::i18n::StatusMessage;
use drill_core::{Document, GridConfig};
use drill_interop::{
    ColumnMapping, DiffSelection, ImportDiff, ImportLimits, ImportOutcome, ImportReport,
    TabularPreview,
};
use drill_jobs::{Job, JobErrorCode, JobFailure, JobKind, JobMsg};
use std::path::PathBuf;

struct MusicalPrepared {
    source: String,
    result: drill_interop::musical::MusicalImport,
}

#[cfg(test)]
mod typed_error_tests {
    use super::*;
    #[test]
    fn import_failures_are_machine_readable_and_fully_localized() {
        for error in [
            ImportFailure::Io,
            ImportFailure::TooLarge,
            ImportFailure::Invalid,
            ImportFailure::Stale,
            ImportFailure::Cancelled,
            ImportFailure::Internal,
        ] {
            let ja = error.localized(drill_core::Locale::Ja);
            let en = error.localized(drill_core::Locale::En);
            assert_ne!(ja, en);
            assert!(
                !en.chars()
                    .any(|ch| matches!(ch, '\u{3040}'..='\u{30ff}' | '\u{4e00}'..='\u{9fff}'))
            );
        }
        let failure = JobFailure::new(JobErrorCode::Stale);
        assert_eq!(ImportFailure::from_job(&failure), ImportFailure::Stale);
    }
}

struct Prepared {
    path: PathBuf,
    bytes: Vec<u8>,
    preview: TabularPreview,
    mapping: ColumnMapping,
    xlsx_sheets: Vec<drill_interop::xlsx::XlsxSheet>,
    selected_sheet: usize,
}

pub(crate) struct ImportReview {
    pub outcome: ImportOutcome,
    pub diff: ImportDiff,
    pub selection: DiffSelection,
    baseline: Document,
}

struct AppliedImport {
    document: Document,
    report: ImportReport,
}

pub(crate) enum ImportEvent {
    Applied {
        document: Box<Document>,
        report: ImportReport,
    },
    Failed(ImportFailure),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImportFailure {
    Io,
    TooLarge,
    Invalid,
    Stale,
    Cancelled,
    Internal,
}

impl ImportFailure {
    fn from_job(error: &JobFailure) -> Self {
        match error.code {
            JobErrorCode::Io => Self::Io,
            JobErrorCode::Stale => Self::Stale,
            JobErrorCode::Cancelled => Self::Cancelled,
            JobErrorCode::Internal => Self::Internal,
            JobErrorCode::TooLarge => Self::TooLarge,
            JobErrorCode::Decode | JobErrorCode::Validation | JobErrorCode::InvalidInput => {
                Self::Invalid
            }
            _ => Self::Internal,
        }
    }
    pub(crate) fn localized(self, locale: drill_core::Locale) -> &'static str {
        use drill_core::Locale::{En, Ja};
        match (self, locale) {
            (Self::Io, Ja) => "ファイルを読み込めません",
            (Self::Io, En) => "The file could not be read",
            (Self::TooLarge, Ja) => "ファイルが安全上限を超えています",
            (Self::TooLarge, En) => "The file exceeds the safety limit",
            (Self::Invalid, Ja) => "ファイル形式または内容を確認してください",
            (Self::Invalid, En) => "Check the file format and contents",
            (Self::Stale, Ja) => "比較後にドキュメントが変更されました。再比較してください",
            (Self::Stale, En) => "The document changed after review; review it again",
            (Self::Cancelled, Ja) => "インポートをキャンセルしました",
            (Self::Cancelled, En) => "Import was cancelled",
            (Self::Internal, Ja) => "インポート処理で内部エラーが発生しました",
            (Self::Internal, En) => "An internal import error occurred",
        }
    }
}

#[derive(Default)]
pub(crate) struct ImportState {
    prepare: Option<Job<Prepared>>,
    import: Option<Job<ImportReview>>,
    apply: Option<Job<AppliedImport>>,
    musical: Option<Job<MusicalPrepared>>,
    musical_review: Option<MusicalPrepared>,
    prepared: Option<Prepared>,
    review: Option<ImportReview>,
    pub open: bool,
    pub confirming: bool,
    pub status: StatusMessage,
    pub musical_open: bool,
}

impl ImportState {
    pub fn choose_musical(&mut self, path: PathBuf) {
        self.musical_open = true;
        self.musical_review = None;
        self.status = StatusMessage::new("import-status.001");
        self.musical = Some(Job::spawn_typed(JobKind::Import, move |progress| {
            let opts = drill_interop::musical::MusicalImportOptions::default();
            let metadata =
                std::fs::metadata(&path).map_err(|_| JobFailure::new(JobErrorCode::Io))?;
            if metadata.len() > opts.max_bytes {
                return Err(JobFailure::new(JobErrorCode::TooLarge));
            }
            let bytes = std::fs::read(&path).map_err(|_| JobFailure::new(JobErrorCode::Io))?;
            if progress.is_cancelled() {
                return Err(JobFailure::new(JobErrorCode::Cancelled));
            }
            progress.set(0.4);
            let extension = path
                .extension()
                .and_then(|v| v.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let result = if extension == "mid" || extension == "midi" {
                drill_interop::musical::import_midi(&bytes, &opts)
            } else {
                drill_interop::musical::import_musicxml(&bytes, &opts)
            }
            .map_err(|_| JobFailure::new(JobErrorCode::Decode))?;
            progress.set(1.0);
            Ok(MusicalPrepared {
                source: path
                    .file_name()
                    .map_or_else(|| "music".into(), |v| v.to_string_lossy().into_owned()),
                result,
            })
        }));
    }

    pub fn musical_review(&self) -> Option<(&str, &drill_interop::musical::MusicalImport)> {
        self.musical_review
            .as_ref()
            .map(|v| (v.source.as_str(), &v.result))
    }
    pub fn take_musical_timeline(&mut self) -> Option<drill_interop::musical::MusicalTimeline> {
        self.musical_open = false;
        self.musical_review.take().map(|v| v.result.timeline)
    }
    pub fn cancel_musical(&mut self) {
        if let Some(job) = &self.musical {
            job.cancel();
        }
        self.musical_open = false;
        self.musical_review = None;
    }
    pub fn choose(&mut self, path: PathBuf) {
        self.open = true;
        self.confirming = false;
        self.status = StatusMessage::new("import-status.002");
        self.prepared = None;
        self.review = None;
        self.prepare = Some(Job::spawn_typed(JobKind::Import, move |progress| {
            let metadata =
                std::fs::metadata(&path).map_err(|_| JobFailure::new(JobErrorCode::Io))?;
            let limits = ImportLimits::default();
            if metadata.len() > limits.max_bytes as u64 {
                return Err(JobFailure::new(JobErrorCode::TooLarge));
            }
            let bytes = std::fs::read(&path).map_err(|_| JobFailure::new(JobErrorCode::Io))?;
            progress.set(0.5);
            let is_xlsx = path
                .extension()
                .and_then(|v| v.to_str())
                .is_some_and(|v| v.eq_ignore_ascii_case("xlsx"));
            let (bytes, preview, xlsx_sheets) = if is_xlsx {
                let workbook = drill_interop::xlsx::inspect_xlsx(&bytes, &limits)
                    .map_err(|_| JobFailure::new(JobErrorCode::Decode))?;
                let first = workbook
                    .sheets
                    .first()
                    .ok_or_else(|| JobFailure::new(JobErrorCode::InvalidInput))?;
                (first.csv.clone(), first.preview.clone(), workbook.sheets)
            } else {
                let preview = drill_interop::sniff(&bytes, &limits)
                    .map_err(|_| JobFailure::new(JobErrorCode::Decode))?;
                (bytes, preview, Vec::new())
            };
            let mapping = drill_interop::suggest_mapping(&preview.headers)
                .map_err(|_| JobFailure::new(JobErrorCode::InvalidInput))?;
            progress.set(1.0);
            Ok(Prepared {
                path,
                bytes,
                preview,
                mapping,
                xlsx_sheets,
                selected_sheet: 0,
            })
        }));
    }

    pub fn start_review(&mut self, grid: GridConfig, baseline: Document) {
        let Some(prepared) = self.prepared.take() else {
            return;
        };
        let preview = prepared.preview.clone();
        let mapping = prepared.mapping.clone();
        let bytes = prepared.bytes;
        let limits = ImportLimits::default();
        self.status = StatusMessage::new("import-status.003");
        self.import = Some(Job::spawn_typed(JobKind::Import, move |progress| {
            let plan = drill_interop::plan_import(&preview, mapping, grid, limits)
                .map_err(|_| JobFailure::new(JobErrorCode::InvalidInput))?;
            let outcome = drill_interop::import_tabular_as_document(&bytes, &plan, |fraction| {
                progress.set(fraction * 0.8);
                !progress.is_cancelled()
            })
            .map_err(|error| {
                JobFailure::new(if matches!(error, drill_interop::ImportError::Cancelled) {
                    JobErrorCode::Cancelled
                } else {
                    JobErrorCode::Validation
                })
            })?;
            if progress.is_cancelled() {
                return Err(JobFailure::new(JobErrorCode::Cancelled));
            }
            let diff = drill_interop::diff_documents(&baseline, &outcome.document);
            let selection = DiffSelection::all(&diff);
            progress.set(1.0);
            Ok(ImportReview {
                outcome,
                diff,
                selection,
                baseline,
            })
        }));
    }

    /// Starts the potentially large merge away from the UI thread. A stale
    /// preview is rejected instead of overwriting edits made since comparison.
    pub fn apply_selected(&mut self, current: &Document) -> Result<(), &'static str> {
        let Some(review) = self.review.take() else {
            return Err("レビュー結果がありません");
        };
        if review.baseline != *current {
            self.review = Some(review);
            return Err("比較後にドキュメントが変更されました。再比較してください");
        }
        self.confirming = false;
        self.status = StatusMessage::new("import-status.004");
        self.apply = Some(Job::spawn_typed(JobKind::Import, move |progress| {
            let document = drill_interop::merge_selected(
                &review.baseline,
                &review.outcome.document,
                &review.diff,
                &review.selection,
            )
            .map_err(|_| JobFailure::new(JobErrorCode::Validation))?;
            progress.set(1.0);
            Ok(AppliedImport {
                document,
                report: review.outcome.report,
            })
        }));
        Ok(())
    }

    pub fn cancel(&mut self) {
        if let Some(job) = &self.import {
            job.cancel();
        }
        if let Some(job) = &self.prepare {
            job.cancel();
        }
        if let Some(job) = &self.apply {
            job.cancel();
        }
        self.open = false;
        self.confirming = false;
    }
    pub fn busy(&self) -> bool {
        self.prepare.is_some()
            || self.import.is_some()
            || self.apply.is_some()
            || self.musical.is_some()
    }
    pub fn preview(&self) -> Option<&TabularPreview> {
        self.prepared.as_ref().map(|v| &v.preview)
    }
    pub fn mapping_mut(&mut self) -> Option<&mut ColumnMapping> {
        self.prepared.as_mut().map(|v| &mut v.mapping)
    }
    pub fn phrase_preview(
        &self,
        grid: &GridConfig,
    ) -> Vec<drill_interop::CoordinatePhraseDiagnostic> {
        self.prepared.as_ref().map_or_else(Vec::new, |prepared| {
            drill_interop::preview_coordinate_phrases(&prepared.preview, &prepared.mapping, grid)
        })
    }
    pub fn xlsx_sheet_names(&self) -> Vec<String> {
        self.prepared.as_ref().map_or_else(Vec::new, |p| {
            p.xlsx_sheets.iter().map(|s| s.name.clone()).collect()
        })
    }
    pub fn selected_xlsx_sheet(&self) -> Option<usize> {
        self.prepared
            .as_ref()
            .filter(|p| !p.xlsx_sheets.is_empty())
            .map(|p| p.selected_sheet)
    }
    pub fn select_xlsx_sheet(&mut self, index: usize) -> Result<(), ImportFailure> {
        let p = self.prepared.as_mut().ok_or(ImportFailure::Invalid)?;
        let sheet = p.xlsx_sheets.get(index).ok_or(ImportFailure::Invalid)?;
        p.bytes = sheet.csv.clone();
        p.preview = sheet.preview.clone();
        p.mapping = drill_interop::suggest_mapping(&p.preview.headers)
            .map_err(|_| ImportFailure::Invalid)?;
        p.selected_sheet = index;
        Ok(())
    }
    pub fn review_mut(&mut self) -> Option<&mut ImportReview> {
        self.review.as_mut()
    }
    pub fn source_name(&self) -> Option<String> {
        self.prepared
            .as_ref()
            .and_then(|v| v.path.file_name())
            .map(|v| v.to_string_lossy().into_owned())
    }

    pub fn poll(&mut self) -> Option<ImportEvent> {
        if let Some(message) = self.musical.as_mut().and_then(Job::poll) {
            self.musical = None;
            match message {
                JobMsg::Done(prepared) => {
                    self.status = StatusMessage::new("import-status.005");
                    self.musical_review = Some(prepared);
                }
                JobMsg::Failed(error) => {
                    return Some(ImportEvent::Failed(ImportFailure::from_job(&error)));
                }
                JobMsg::Cancelled => self.musical_open = false,
            }
        }
        if let Some(message) = self.prepare.as_mut().and_then(Job::poll) {
            self.prepare = None;
            match message {
                JobMsg::Done(prepared) => {
                    self.status =
                        StatusMessage::new("import-status.006").arg(0, prepared.preview.total_rows);
                    self.prepared = Some(prepared);
                }
                JobMsg::Failed(error) => {
                    return Some(ImportEvent::Failed(ImportFailure::from_job(&error)));
                }
                JobMsg::Cancelled => self.open = false,
            }
        }
        if let Some(message) = self.import.as_mut().and_then(Job::poll) {
            self.import = None;
            match message {
                JobMsg::Done(review) => {
                    self.status = StatusMessage::new("import-status.007");
                    self.review = Some(review);
                }
                JobMsg::Failed(error) => {
                    return Some(ImportEvent::Failed(ImportFailure::from_job(&error)));
                }
                JobMsg::Cancelled => {
                    return Some(ImportEvent::Failed(ImportFailure::Cancelled));
                }
            }
        }
        if let Some(message) = self.apply.as_mut().and_then(Job::poll) {
            self.apply = None;
            return Some(match message {
                JobMsg::Done(applied) => {
                    self.open = false;
                    ImportEvent::Applied {
                        document: Box::new(applied.document),
                        report: applied.report,
                    }
                }
                JobMsg::Failed(error) => ImportEvent::Failed(ImportFailure::from_job(&error)),
                JobMsg::Cancelled => ImportEvent::Failed(ImportFailure::Cancelled),
            });
        }
        None
    }
}
