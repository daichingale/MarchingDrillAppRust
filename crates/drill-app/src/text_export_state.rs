use drill_jobs::{Job, JobErrorCode, JobFailure, JobKind, JobMsg};
use std::path::PathBuf;

/// Owns text-based exports so filesystem latency never stalls an egui frame.
#[derive(Default)]
pub struct TextExportState {
    job: Option<Job<PathBuf>>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TextExportEvent {
    Written(PathBuf),
    Failed(TextExportError),
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextExportError {
    Busy,
    Io,
    Worker,
}

impl TextExportError {
    pub fn localized(self, locale: drill_core::Locale) -> &'static str {
        match (self, locale) {
            (Self::Busy, drill_core::Locale::Ja) => "別のテキスト書き出しを実行中です",
            (Self::Busy, drill_core::Locale::En) => "Another text export is already running",
            (Self::Io, drill_core::Locale::Ja) => "出力ファイルを書き込めません",
            (Self::Io, drill_core::Locale::En) => "The output file could not be written",
            (Self::Worker, drill_core::Locale::Ja) => "書き出し処理で内部エラーが発生しました",
            (Self::Worker, drill_core::Locale::En) => "An internal export error occurred",
        }
    }
}

impl TextExportState {
    pub fn start(&mut self, path: PathBuf, contents: String) -> Result<(), TextExportError> {
        if self.job.is_some() {
            return Err(TextExportError::Busy);
        }
        self.job = Some(Job::spawn_typed(JobKind::ExportText, move |progress| {
            progress.set(0.1);
            if progress.is_cancelled() {
                return Ok(path);
            }
            drill_project::atomic_write(&path, contents.as_bytes(), None)
                .map_err(|_| JobFailure::new(JobErrorCode::Io))?;
            progress.set(1.0);
            Ok(path)
        }));
        Ok(())
    }

    pub fn poll(&mut self) -> Option<TextExportEvent> {
        let message = self.job.as_mut()?.poll()?;
        self.job = None;
        Some(match message {
            JobMsg::Done(path) => TextExportEvent::Written(path),
            JobMsg::Failed(failure) => {
                TextExportEvent::Failed(if failure.code == JobErrorCode::Io {
                    TextExportError::Io
                } else {
                    TextExportError::Worker
                })
            }
            JobMsg::Cancelled => TextExportEvent::Cancelled,
        })
    }

    #[must_use]
    pub fn progress(&self) -> Option<f32> {
        self.job.as_ref().map(Job::progress)
    }

    #[must_use]
    pub fn is_running(&self) -> bool {
        self.job.is_some()
    }

    pub fn cancel(&self) {
        if let Some(job) = &self.job {
            job.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn unique_path() -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "drillforge-text-export-{}-{}.txt",
            std::process::id(),
            nonce
        ))
    }

    #[test]
    fn writes_contents_and_reports_destination() {
        let path = unique_path();
        let _ = std::fs::remove_file(&path);
        let mut state = TextExportState::default();
        state.start(path.clone(), "日本語,123\n".into()).unwrap();
        assert!(state.is_running());
        let deadline = Instant::now() + Duration::from_secs(3);
        let event = loop {
            if let Some(event) = state.poll() {
                break event;
            }
            assert!(Instant::now() < deadline, "text export timed out");
            std::thread::yield_now();
        };
        assert_eq!(event, TextExportEvent::Written(path.clone()));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "日本語,123\n");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn refuses_to_replace_a_running_job() {
        let path = unique_path();
        let mut state = TextExportState::default();
        state.start(path.clone(), "first".into()).unwrap();
        assert_eq!(
            state.start(path, "second".into()),
            Err(TextExportError::Busy)
        );
    }

    #[test]
    fn errors_are_machine_readable_and_localized() {
        for error in [
            TextExportError::Busy,
            TextExportError::Io,
            TextExportError::Worker,
        ] {
            let ja = error.localized(drill_core::Locale::Ja);
            let en = error.localized(drill_core::Locale::En);
            assert_ne!(ja, en);
            assert!(
                !en.chars()
                    .any(|ch| matches!(ch, '\u{3040}'..='\u{30ff}' | '\u{4e00}'..='\u{9fff}'))
            );
        }
    }
}
