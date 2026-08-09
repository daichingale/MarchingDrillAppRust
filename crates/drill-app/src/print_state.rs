use drill_core::{Document, Locale};
use drill_export::page::{Orientation, PageSize, PrintSettings, paginate};
use drill_export::pdf::{PdfExportRequest, PdfSummary, spawn_pdf_export};
use drill_export::report::{self, ReportKind};
use drill_jobs::{Job, JobErrorCode, JobMsg};
use std::path::PathBuf;

pub(crate) struct PrintState {
    pub open: bool,
    pub kind: ReportKind,
    pub settings: PrintSettings,
    pub status: String,
    pub completed: Option<(PathBuf, PdfSummary)>,
    job: Option<Job<PdfSummary>>,
    output: Option<PathBuf>,
}

impl Default for PrintState {
    fn default() -> Self {
        Self {
            open: false,
            kind: ReportKind::SetChart,
            settings: PrintSettings::default(),
            status: String::new(),
            completed: None,
            job: None,
            output: None,
        }
    }
}

impl PrintState {
    pub fn report(
        &self,
        document: &Document,
        beats_per_measure: u16,
        selected: &[usize],
    ) -> drill_export::page::ReportDocument {
        match self.kind {
            ReportKind::SetChart => report::set_charts(document, &[]),
            ReportKind::PerformerDrillBook => report::performer_drill_book(document, selected),
            ReportKind::CountSheet => report::count_sheet(document, beats_per_measure),
            ReportKind::ProductionSheet => report::production_sheet(document, Locale::En),
        }
    }
    pub fn page_count(
        &self,
        document: &Document,
        beats_per_measure: u16,
        selected: &[usize],
    ) -> usize {
        paginate(
            &self.report(document, beats_per_measure, selected),
            &self.settings,
        )
        .len()
    }
    pub fn start(
        &mut self,
        document: &Document,
        beats_per_measure: u16,
        selected: &[usize],
        output: PathBuf,
        locale: Locale,
        underlay: Option<Vec<u8>>,
    ) {
        let report = match self.kind {
            ReportKind::ProductionSheet => report::production_sheet(document, locale),
            ReportKind::PerformerDrillBook => {
                report::performer_drill_book_localized(document, selected, locale)
            }
            _ => self.report(document, beats_per_measure, selected),
        };
        self.settings.title = report.title.clone();
        self.output = Some(output.clone());
        self.completed = None;
        self.status = match locale {
            Locale::Ja => "PDFを生成しています…",
            Locale::En => "Generating PDF…",
        }
        .into();
        self.job = Some(spawn_pdf_export(PdfExportRequest {
            document: document.clone(),
            report,
            settings: self.settings.clone(),
            output,
            underlay,
        }));
    }
    pub fn progress(&self) -> Option<f32> {
        self.job.as_ref().map(Job::progress)
    }
    pub fn cancel(&self) {
        if let Some(job) = &self.job {
            job.cancel();
        }
    }
    pub fn poll(&mut self, locale: Locale) {
        let message = self.job.as_mut().and_then(Job::poll);
        if let Some(message) = message {
            self.job = None;
            match message {
                JobMsg::Done(summary) => {
                    let path = self.output.take().unwrap_or_default();
                    self.status = match locale {
                        Locale::Ja => format!(
                            "PDFを保存しました（{}ページ、{} KB）",
                            summary.pages,
                            summary.bytes.div_ceil(1024)
                        ),
                        Locale::En => format!(
                            "PDF saved ({} pages, {} KB)",
                            summary.pages,
                            summary.bytes.div_ceil(1024)
                        ),
                    };
                    self.completed = Some((path, summary));
                }
                JobMsg::Cancelled => {
                    self.status = match locale {
                        Locale::Ja => "PDF作成をキャンセルしました",
                        Locale::En => "PDF creation cancelled",
                    }
                    .into()
                }
                JobMsg::Failed(reason) => {
                    self.status = if reason.code == JobErrorCode::Busy {
                        match locale {
                            Locale::Ja => "同名PDFが存在します。別の名前を指定してください",
                            Locale::En => "A PDF with that name exists. Choose another name.",
                        }
                        .into()
                    } else {
                        match locale {
                            Locale::Ja => format!("PDF作成に失敗しました: {reason}"),
                            Locale::En => format!("PDF creation failed: {reason}"),
                        }
                    }
                }
            }
        }
    }
    pub fn page_size_label(&self) -> &'static str {
        match self.settings.page_size {
            PageSize::A4 => "A4",
            PageSize::Letter => "Letter",
            PageSize::Tabloid => "Tabloid",
            PageSize::Custom { .. } => "Custom",
        }
    }
    pub fn orientation_label(&self, locale: Locale) -> &'static str {
        match self.settings.orientation {
            Orientation::Portrait => match locale {
                Locale::Ja => "縦",
                Locale::En => "Portrait",
            },
            Orientation::Landscape => match locale {
                Locale::Ja => "横",
                Locale::En => "Landscape",
            },
        }
    }
}
