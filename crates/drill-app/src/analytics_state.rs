use drill_core::aesthetics::{self, AestheticScore};
use drill_core::rhythm_sync::{self, RhythmSyncReport};
use drill_core::show_heatmap::{self, FieldOccupancy, HeatmapParams};
use drill_core::{Document, Revision};
use drill_jobs::{Job, JobErrorCode, JobFailure, JobKind, JobMsg};
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AnalyticsKey {
    pub revision: Revision,
    pub set_index: usize,
    pub beats_per_measure: u16,
    pub heatmap: bool,
}

struct AnalyticsResult {
    key: AnalyticsKey,
    rhythm: RhythmSyncReport,
    aesthetics: Option<AestheticScore>,
    heatmap: Option<FieldOccupancy>,
}

pub(crate) struct AnalyticsState {
    job: Option<Job<AnalyticsResult>>,
    running_key: Option<AnalyticsKey>,
    cache_key: Option<AnalyticsKey>,
    rhythm: Option<RhythmSyncReport>,
    aesthetics: Option<AestheticScore>,
    heatmap: Option<FieldOccupancy>,
    last_poll: Instant,
    stale_discards: u64,
}

impl Default for AnalyticsState {
    fn default() -> Self {
        Self {
            job: None,
            running_key: None,
            cache_key: None,
            rhythm: None,
            aesthetics: None,
            heatmap: None,
            last_poll: Instant::now() - POLL_INTERVAL,
            stale_discards: 0,
        }
    }
}

impl AnalyticsState {
    /// Starts analysis from an owned immutable snapshot. The clone is made
    /// exactly once per requested revision; all O(P*S), O(P^2), and whole-show
    /// sampling work runs on the named analytics worker.
    pub(crate) fn ensure(&mut self, document: &Document, key: AnalyticsKey) {
        if self.cache_key == Some(key) || self.running_key == Some(key) {
            return;
        }
        if let Some(job) = self.job.take() {
            job.cancel();
        }
        let snapshot = document.clone();
        self.running_key = Some(key);
        self.job = Some(Job::spawn_typed(JobKind::Analytics, move |progress| {
            if progress.is_cancelled() {
                return Err(JobFailure::new(JobErrorCode::Cancelled));
            }
            let rhythm_params = rhythm_sync::RhythmSyncParams {
                beats_per_measure: key.beats_per_measure,
                ..rhythm_sync::RhythmSyncParams::default()
            };
            let rhythm = rhythm_sync::analyze_show(&snapshot, &rhythm_params);
            progress.set(0.34);
            if progress.is_cancelled() {
                return Err(JobFailure::new(JobErrorCode::Cancelled));
            }
            let aesthetics = aesthetics::analyze_set(
                &snapshot,
                key.set_index,
                &aesthetics::AestheticParams::default(),
            );
            progress.set(0.67);
            if progress.is_cancelled() {
                return Err(JobFailure::new(JobErrorCode::Cancelled));
            }
            let heatmap = key.heatmap.then(|| {
                show_heatmap::analyze_show_occupancy(&snapshot, &HeatmapParams::default())
            });
            progress.set(1.0);
            Ok(AnalyticsResult {
                key,
                rhythm,
                aesthetics,
                heatmap,
            })
        }));
    }

    /// At most one lock-free channel poll every 50 ms. A result is accepted
    /// only when all revision and parameter fields still match.
    pub(crate) fn poll(&mut self, current: AnalyticsKey) {
        if self.last_poll.elapsed() < POLL_INTERVAL {
            return;
        }
        self.last_poll = Instant::now();
        let Some(message) = self.job.as_mut().and_then(Job::poll) else {
            return;
        };
        self.job = None;
        self.running_key = None;
        if let JobMsg::Done(result) = message {
            if result.key == current {
                self.cache_key = Some(result.key);
                self.rhythm = Some(result.rhythm);
                self.aesthetics = result.aesthetics;
                self.heatmap = result.heatmap;
            } else {
                self.stale_discards = self.stale_discards.saturating_add(1);
            }
        }
    }

    pub(crate) fn cancel(&mut self) {
        if let Some(job) = self.job.take() {
            job.cancel();
        }
        self.running_key = None;
    }

    pub(crate) fn progress(&self) -> Option<f32> {
        self.job.as_ref().map(Job::progress)
    }

    pub(crate) fn rhythm(&self, key: AnalyticsKey) -> Option<&RhythmSyncReport> {
        (self.cache_key == Some(key))
            .then_some(self.rhythm.as_ref())
            .flatten()
    }

    pub(crate) fn aesthetics(&self, key: AnalyticsKey) -> Option<&AestheticScore> {
        (self.cache_key == Some(key))
            .then_some(self.aesthetics.as_ref())
            .flatten()
    }

    pub(crate) fn heatmap(&self, key: AnalyticsKey) -> Option<&FieldOccupancy> {
        (self.cache_key == Some(key))
            .then_some(self.heatmap.as_ref())
            .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{self, Sender};

    /// Unblocks a parked analytics worker when the test finishes, including
    /// on assertion failure, so the worker cannot outlive the test.
    struct Release(Option<Sender<()>>);

    impl Release {
        fn signal(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }

    impl Drop for Release {
        fn drop(&mut self) {
            self.signal();
        }
    }

    fn poll_until_idle(state: &mut AnalyticsState, current: AnalyticsKey) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while state.job.is_some() {
            assert!(Instant::now() < deadline, "analytics job did not finish");
            state.last_poll = Instant::now() - POLL_INTERVAL;
            state.poll(current);
            std::thread::yield_now();
        }
    }

    #[test]
    fn stale_result_never_replaces_current_revision_and_poll_is_bounded() {
        let old = AnalyticsKey {
            revision: Revision(0),
            set_index: 0,
            beats_per_measure: 4,
            heatmap: true,
        };
        let current = AnalyticsKey {
            revision: Revision(1),
            ..old
        };

        // The worker stays parked until `release` is signaled, so a poll that
        // joined it or ran the analysis inline could not return. No wall-clock
        // budget: a 2ms limit flakes when the shared runner is preempted.
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let mut release = Release(Some(release_tx));
        let mut state = AnalyticsState {
            running_key: Some(old),
            job: Some(Job::spawn_typed(JobKind::Analytics, move |_| {
                let _ = entered_tx.send(());
                let _ = release_rx.recv();
                Ok(AnalyticsResult {
                    key: old,
                    rhythm: RhythmSyncReport {
                        events: Vec::new(),
                        on_beat_ratio: 0.0,
                        show_score: 0.0,
                    },
                    aesthetics: None,
                    heatmap: None,
                })
            })),
            ..AnalyticsState::default()
        };
        entered_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("analytics worker did not start");

        for _ in 0..16 {
            state.last_poll = Instant::now() - POLL_INTERVAL;
            state.poll(current);
            assert!(
                state.job.is_some(),
                "poll must return while the analytics worker is still parked"
            );
            assert!(state.rhythm(current).is_none());
            assert_eq!(state.stale_discards, 0);
        }

        release.signal();
        poll_until_idle(&mut state, current);
        assert!(state.rhythm(current).is_none());
        assert!(state.cache_key.is_none());
        assert_eq!(state.stale_discards, 1);

        // A real analysis result for the previous revision is discarded the
        // same way, and still does not become the current revision's report.
        let document = Document::demo(10, 10);
        let mut state = AnalyticsState::default();
        state.ensure(&document, old);
        poll_until_idle(&mut state, current);
        assert!(state.rhythm(current).is_none());
        assert!(state.cache_key.is_none());
        assert_eq!(state.stale_discards, 1);
    }

    #[test]
    fn analysis_is_deterministic_and_does_not_mutate_snapshot() {
        let document = Document::demo(8, 10);
        let before = document.clone();
        let params = rhythm_sync::RhythmSyncParams::default();
        let first = rhythm_sync::analyze_show(&document, &params);
        let second = rhythm_sync::analyze_show(&document, &params);
        assert_eq!(first, second);
        assert_eq!(document, before);
    }

    #[test]
    fn ui_call_sites_do_not_run_heavy_analytics_inline() {
        let ui = include_str!("app_ui.rs");
        let inspector = include_str!("workspace_inspector.rs");
        for forbidden in [
            "rhythm_sync::analyze_show(",
            "aesthetics::analyze_set(",
            "show_heatmap::analyze_show_occupancy(",
            // Collision-aware assignment runs up to 192 swept scans; it
            // belongs on the AssignmentOptimize job thread, never inline.
            "pathing::optimal_assignment_collision_aware(",
        ] {
            assert!(!ui.contains(forbidden), "inline UI analysis: {forbidden}");
            assert!(
                !inspector.contains(forbidden),
                "inline UI analysis: {forbidden}"
            );
        }
    }
}
