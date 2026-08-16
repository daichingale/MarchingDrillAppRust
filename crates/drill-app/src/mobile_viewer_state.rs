use drill_core::{Document, Locale, PerformerId, Revision};
use drill_jobs::{Job, JobErrorCode, JobFailure, JobKind, JobMsg};
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(50);

struct GeneratedViewer {
    revision: Revision,
    performer_count: usize,
    html: String,
}

pub(crate) enum MobileViewerEvent {
    Ready {
        html: String,
        performer_count: usize,
    },
    Failed,
    Cancelled,
    Stale,
}

pub(crate) struct MobileViewerState {
    pub last_performer_count: usize,
    job: Option<Job<GeneratedViewer>>,
    last_poll: Instant,
}

impl Default for MobileViewerState {
    fn default() -> Self {
        Self {
            last_performer_count: 0,
            job: None,
            last_poll: Instant::now() - POLL_INTERVAL,
        }
    }
}

impl MobileViewerState {
    pub(crate) fn start(
        &mut self,
        document: &Document,
        performer_ids: Vec<PerformerId>,
        locale: Locale,
        revision: Revision,
    ) {
        if let Some(job) = self.job.take() {
            job.cancel();
        }
        let snapshot = document.clone();
        let performer_count = if performer_ids.is_empty() {
            snapshot.performers.len()
        } else {
            performer_ids.len()
        };
        self.job = Some(Job::spawn_typed(JobKind::ExportText, move |progress| {
            progress.set(0.1);
            if progress.is_cancelled() {
                return Err(JobFailure::new(JobErrorCode::Cancelled));
            }
            let html =
                drill_mobile_viewer::build_practice_viewer(&snapshot, &performer_ids, locale)
                    .map_err(|_| JobFailure::new(JobErrorCode::Validation))?;
            progress.set(1.0);
            Ok(GeneratedViewer {
                revision,
                performer_count,
                html,
            })
        }));
    }

    pub(crate) fn poll(&mut self, revision: Revision) -> Option<MobileViewerEvent> {
        if self.last_poll.elapsed() < POLL_INTERVAL {
            return None;
        }
        self.last_poll = Instant::now();
        let message = self.job.as_mut()?.poll()?;
        self.job = None;
        Some(match message {
            JobMsg::Done(result) if result.revision == revision => MobileViewerEvent::Ready {
                html: result.html,
                performer_count: result.performer_count,
            },
            JobMsg::Done(_) => MobileViewerEvent::Stale,
            JobMsg::Failed(_) => MobileViewerEvent::Failed,
            JobMsg::Cancelled => MobileViewerEvent::Cancelled,
        })
    }

    pub(crate) fn progress(&self) -> Option<f32> {
        self.job.as_ref().map(Job::progress)
    }
    pub(crate) fn cancel(&self) {
        if let Some(job) = &self.job {
            job.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousand_performer_poll_is_bounded_and_stale_is_discarded() {
        let document = Document::demo(25, 40);
        let mut state = MobileViewerState::default();
        state.start(&document, Vec::new(), Locale::Ja, Revision(4));
        let deadline = Instant::now() + Duration::from_secs(3);
        let event = loop {
            state.last_poll = Instant::now() - POLL_INTERVAL;
            let started = Instant::now();
            let event = state.poll(Revision(5));
            assert!(started.elapsed() < Duration::from_millis(2));
            if let Some(event) = event {
                break event;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        };
        assert!(matches!(event, MobileViewerEvent::Stale));
    }

    #[test]
    fn inspector_never_generates_viewer_inline() {
        assert!(
            !include_str!("inspector_media.rs")
                .contains("drill_mobile_viewer::build_practice_viewer(")
        );
    }
}
