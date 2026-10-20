use drill_core::{Document, Locale};
use drill_export::blender::{self, BlenderOptions};
use drill_export::page::{Orientation, PageItem, PageSize, PrintSettings, paginate};
use drill_export::pdf::{PdfExportRequest, PdfSummary, spawn_pdf_export};
use drill_export::report::{self, ReportKind};
use drill_jobs::{Job, JobErrorCode, JobMsg};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

pub(crate) struct PrintState {
    pub open: bool,
    pub kind: ReportKind,
    pub settings: PrintSettings,
    /// Limit set charts and per-set tables to the set open in the editor.
    pub current_set_only: bool,
    /// Editor's open set. The sheet copies this in before counting pages.
    pub scope_set: usize,
    pub status: String,
    pub completed: Option<(PathBuf, PdfSummary)>,
    /// The sheet is showing the 3D video bundle instead of a PDF report.
    pub video: bool,
    pub video_options: BlenderOptions,
    pub blender_path: String,
    pub open_when_done: bool,
    pub video_done: Option<PathBuf>,
    job: Option<Job<PdfSummary>>,
    output: Option<PathBuf>,
    video_job: Option<BlenderJob>,
}

impl Default for PrintState {
    fn default() -> Self {
        Self {
            open: false,
            kind: ReportKind::SetChart,
            settings: PrintSettings::default(),
            current_set_only: false,
            scope_set: 0,
            status: String::new(),
            completed: None,
            video: false,
            video_options: BlenderOptions::default(),
            blender_path: load_blender_path(),
            open_when_done: true,
            video_done: None,
            job: None,
            output: None,
            video_job: None,
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
        let set_index = self.scope_set;
        let mut report = match self.kind {
            ReportKind::SetChart => {
                if self.current_set_only {
                    report::set_charts(document, &[set_index])
                } else {
                    report::set_charts(document, &[])
                }
            }
            ReportKind::PerformerDrillBook => report::performer_drill_book(document, selected),
            ReportKind::CountSheet => report::count_sheet(document, beats_per_measure),
            ReportKind::ProductionSheet => report::production_sheet(document, Locale::En),
        };
        if self.current_set_only && self.kind != ReportKind::SetChart {
            keep_set_rows(&mut report, set_index);
        }
        report
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
        let mut report = match self.kind {
            ReportKind::ProductionSheet => report::production_sheet(document, locale),
            ReportKind::PerformerDrillBook => {
                report::performer_drill_book_localized(document, selected, locale)
            }
            _ => self.report(document, beats_per_measure, selected),
        };
        if self.current_set_only && self.kind != ReportKind::SetChart {
            keep_set_rows(&mut report, self.scope_set);
        }
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
    pub fn scene_options(&self) -> BlenderOptions {
        let mut options = self.video_options.clone();
        options.set_index = if self.current_set_only {
            Some(self.scope_set)
        } else {
            None
        };
        options
    }

    pub fn video_running(&self) -> bool {
        self.video_job.is_some()
    }

    pub fn video_fraction(&self) -> Option<f32> {
        self.video_job.as_ref().map(BlenderJob::fraction)
    }

    pub fn remember_blender_path(&mut self) {
        save_blender_path(&self.blender_path);
    }

    pub fn poll_video(&mut self, locale: Locale) {
        let (done, end) = {
            let Some(job) = self.video_job.as_ref() else {
                return;
            };
            (job.frame_done(), job.frame_end.max(1))
        };
        self.status = match locale {
            Locale::Ja => format!("レンダリング中… {done}/{end}"),
            Locale::En => format!("Rendering… {done}/{end}"),
        };
        let outcome = self.video_job.as_mut().and_then(BlenderJob::poll);
        let Some(outcome) = outcome else {
            return;
        };
        let path = self
            .video_job
            .as_ref()
            .map(|job| job.output.clone())
            .unwrap_or_default();
        let open = self.open_when_done;
        self.video_job = None;
        match outcome {
            VideoEnd::Done => {
                self.video_done = Some(path.clone());
                self.status = match locale {
                    Locale::Ja => "動画を保存しました".into(),
                    Locale::En => "Video saved".into(),
                };
                if open {
                    reveal_path(&path);
                }
            }
            VideoEnd::Cancelled => {
                self.status = match locale {
                    Locale::Ja => "レンダリングを止めました".into(),
                    Locale::En => "Render stopped".into(),
                };
            }
            VideoEnd::Failed(reason) => {
                self.status = match locale {
                    Locale::Ja => format!("レンダリングに失敗しました: {reason}"),
                    Locale::En => format!("Render failed: {reason}"),
                };
            }
        }
    }

    pub fn cancel_video(&mut self) {
        if let Some(job) = self.video_job.as_mut() {
            job.cancel();
        }
    }

    /// Writes the bundle beside `output` and, when Blender is installed, starts it.
    pub fn start_video(
        &mut self,
        document: &Document,
        output: PathBuf,
        locale: Locale,
    ) -> Result<(), String> {
        if self.video_job.is_some() {
            return Ok(());
        }
        let scene = blender::build_scene(document, &self.scene_options());
        let stem = output
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "drill".into());
        let directory = output.with_file_name(format!("{stem}.blender"));
        let (script, data) = match blender::write_bundle(&directory, &scene) {
            Ok(paths) => paths,
            Err(err) => {
                self.status = match locale {
                    Locale::Ja => format!("ファイルを書けませんでした: {err}"),
                    Locale::En => format!("Could not write the files: {err}"),
                };
                return Err(self.status.clone());
            }
        };
        let Some(blender_bin) = locate_blender(&self.blender_path) else {
            self.video_done = None;
            self.status = match locale {
                Locale::Ja => "Blender用のファイルを保存しました".into(),
                Locale::En => "Saved the Blender files".into(),
            };
            return Err(self.status.clone());
        };
        let frame_end = blender::last_frame(&scene);
        let job = match BlenderJob::start(&blender_bin, &script, &data, output, frame_end) {
            Ok(job) => job,
            Err(err) => {
                self.status = match locale {
                    Locale::Ja => format!("Blenderを起動できませんでした: {err}"),
                    Locale::En => format!("Could not start Blender: {err}"),
                };
                return Err(self.status.clone());
            }
        };
        self.video_done = None;
        self.status = match locale {
            Locale::Ja => "レンダリング中…".into(),
            Locale::En => "Rendering…".into(),
        };
        self.video_job = Some(job);
        Ok(())
    }

    pub fn save_bundle(&mut self, document: &Document, directory: PathBuf, locale: Locale) {
        let scene = blender::build_scene(document, &self.scene_options());
        match blender::write_bundle(&directory, &scene) {
            Ok(_) => {
                self.status = match locale {
                    Locale::Ja => "Blender用のファイルを保存しました".into(),
                    Locale::En => "Saved the Blender files".into(),
                };
            }
            Err(err) => {
                self.status = match locale {
                    Locale::Ja => format!("ファイルを書けませんでした: {err}"),
                    Locale::En => format!("Could not write the files: {err}"),
                };
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

pub(crate) fn blender_settings_path() -> PathBuf {
    super::project_state::app_data_dir().join("blender_path.txt")
}

pub(crate) fn load_blender_path() -> String {
    std::fs::read_to_string(blender_settings_path())
        .unwrap_or_default()
        .trim()
        .to_owned()
}

pub(crate) fn save_blender_path(path: &str) {
    let file = blender_settings_path();
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(file, path.trim());
}

pub(crate) fn locate_blender(configured: &str) -> Option<PathBuf> {
    let configured = configured.trim();
    if !configured.is_empty() {
        let path = PathBuf::from(configured);
        if path.is_file() {
            return Some(path);
        }
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&paths) {
            for name in ["blender", "blender.exe"] {
                let candidate = directory.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    for candidate in ["/usr/bin/blender", "/usr/local/bin/blender"] {
        let path = PathBuf::from(candidate);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

pub(crate) fn reveal_path(path: &Path) {
    let mut command = if cfg!(target_os = "windows") {
        let mut command = Command::new("explorer");
        command.arg(path);
        command
    } else if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command.arg(path);
        command
    } else {
        let mut command = Command::new("xdg-open");
        command.arg(path);
        command
    };
    let _ = command.spawn();
}

enum VideoEnd {
    Done,
    Cancelled,
    Failed(String),
}

struct BlenderJob {
    child: Option<Child>,
    progress: Arc<AtomicU32>,
    frame_end: u32,
    output: PathBuf,
    log: Arc<Mutex<String>>,
    cancelled: bool,
}

impl BlenderJob {
    fn start(
        blender_bin: &Path,
        script: &Path,
        data: &Path,
        output: PathBuf,
        frame_end: u32,
    ) -> std::io::Result<Self> {
        let mut command = Command::new(blender_bin);
        command
            .arg("-b")
            .arg("-P")
            .arg(script)
            .arg("--")
            .arg("--data")
            .arg(data)
            .arg("--render")
            .arg(&output)
            .env("PYTHONUNBUFFERED", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        hide_console(&mut command);
        let mut child = command.spawn()?;
        let progress = Arc::new(AtomicU32::new(0));
        let log = Arc::new(Mutex::new(String::new()));
        if let Some(stdout) = child.stdout.take() {
            spawn_pump(stdout, progress.clone(), log.clone());
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_pump(stderr, progress.clone(), log.clone());
        }
        Ok(Self {
            child: Some(child),
            progress,
            frame_end,
            output,
            log,
            cancelled: false,
        })
    }

    fn frame_done(&self) -> u32 {
        self.progress.load(Ordering::Relaxed)
    }

    fn fraction(&self) -> f32 {
        self.frame_done() as f32 / self.frame_end.max(1) as f32
    }

    fn cancel(&mut self) {
        self.cancelled = true;
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
        }
    }

    fn poll(&mut self) -> Option<VideoEnd> {
        let child = self.child.as_mut()?;
        match child.try_wait() {
            Ok(None) => None,
            Ok(Some(status)) => {
                self.child = None;
                if self.cancelled {
                    Some(VideoEnd::Cancelled)
                } else if status.success() {
                    Some(VideoEnd::Done)
                } else {
                    Some(VideoEnd::Failed(log_tail(&self.log)))
                }
            }
            Err(err) => {
                self.child = None;
                Some(VideoEnd::Failed(err.to_string()))
            }
        }
    }
}

impl Drop for BlenderJob {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn spawn_pump(
    mut reader: impl Read + Send + 'static,
    progress: Arc<AtomicU32>,
    log: Arc<Mutex<String>>,
) {
    std::thread::spawn(move || {
        let mut buffer = [0_u8; 512];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(size) => note_blender_output(&buffer[..size], &progress, &log),
            }
        }
    });
}

fn note_blender_output(bytes: &[u8], progress: &AtomicU32, log: &Mutex<String>) {
    let chunk = String::from_utf8_lossy(bytes);
    let mut held = String::new();
    if let Ok(mut guard) = log.lock() {
        guard.push_str(&chunk);
        if guard.len() > 2000 {
            let start = guard.len() - 1500;
            let trimmed = guard[start..].to_owned();
            *guard = trimmed;
        }
        held.clone_from(&guard);
    }
    if let Some(index) = held.rfind("Fra:") {
        let digits: String = held[index + 4..]
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect();
        if let Ok(frame) = digits.parse::<u32>() {
            progress.fetch_max(frame, Ordering::Relaxed);
        }
    }
}

fn log_tail(log: &Mutex<String>) -> String {
    let Ok(guard) = log.lock() else {
        return String::new();
    };
    let trimmed = guard.trim();
    let tail = trimmed.chars().rev().take(180).collect::<String>();
    tail.chars().rev().collect()
}

#[cfg(windows)]
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_command: &mut Command) {}

fn keep_set_rows(report: &mut drill_export::page::ReportDocument, set_index: usize) {
    for item in &mut report.items {
        if let PageItem::Table { rows, .. } = item
            && let Some(row) = rows.get(set_index).cloned()
        {
            rows.clear();
            rows.push(row);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn video_range_follows_the_open_set() {
        let state = PrintState {
            scope_set: 3,
            ..PrintState::default()
        };
        assert!(state.scene_options().set_index.is_none());
        let state = PrintState {
            scope_set: 3,
            current_set_only: true,
            ..PrintState::default()
        };
        assert_eq!(state.scene_options().set_index, Some(3));
    }

    #[test]
    fn locate_uses_path_or_a_known_install() {
        if std::path::Path::new("/usr/bin/blender").is_file() {
            assert!(locate_blender("").is_some());
        }
        assert!(locate_blender("/no/such/blender").is_none() || locate_blender("").is_some());
    }

    #[test]
    fn frame_marker_advances_progress() {
        let progress = AtomicU32::new(0);
        let log = Mutex::new(String::new());
        note_blender_output(b"Fra:12 Mem:100", &progress, &log);
        assert_eq!(progress.load(Ordering::Relaxed), 12);
        note_blender_output(b"noise Fra:4", &progress, &log);
        assert_eq!(progress.load(Ordering::Relaxed), 12);
    }
}
