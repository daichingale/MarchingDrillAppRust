//! Small, dependency-free background jobs for DrillForge.
//!
//! Progress and cancellation are lock-free. A job sends exactly one terminal
//! message, and worker panics are contained at the thread boundary.

use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Integer resolution used by the lock-free progress counter.
pub const PROGRESS_MAX: u32 = 10_000;

/// Frame-thread latency budget used by the desktop UI quality gate.
///
/// The budget is deliberately far below one 60 Hz frame (16.67 ms), leaving
/// time for layout, painting and GPU submission after background-job polling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiLatencyBudget {
    pub p95: Duration,
    pub p99: Duration,
    pub worst: Duration,
}

impl Default for UiLatencyBudget {
    fn default() -> Self {
        Self {
            p95: Duration::from_millis(2),
            p99: Duration::from_millis(4),
            worst: Duration::from_millis(8),
        }
    }
}

/// Distribution produced by a frame-thread latency run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiLatencyReport {
    pub frames: usize,
    pub p95: Duration,
    pub p99: Duration,
    pub worst: Duration,
}

impl UiLatencyReport {
    #[must_use]
    pub fn meets(self, budget: UiLatencyBudget) -> bool {
        self.p95 <= budget.p95 && self.p99 <= budget.p99 && self.worst <= budget.worst
    }
}

/// Allocation-free collector for durations measured on the egui/update thread.
///
/// `record` itself performs only a clock read and one array write. Sorting is
/// deferred until `report`, after the simulated frame loop has completed.
pub struct UiLatencyGate<const FRAMES: usize> {
    samples: [Duration; FRAMES],
    len: usize,
}

impl<const FRAMES: usize> Default for UiLatencyGate<FRAMES> {
    fn default() -> Self {
        Self {
            samples: [Duration::ZERO; FRAMES],
            len: 0,
        }
    }
}

impl<const FRAMES: usize> UiLatencyGate<FRAMES> {
    /// Measures exactly the caller-thread portion of one frame.
    pub fn record<R>(&mut self, frame: impl FnOnce() -> R) -> R {
        assert!(self.len < FRAMES, "latency gate sample capacity exceeded");
        let started = Instant::now();
        let result = frame();
        self.samples[self.len] = started.elapsed();
        self.len += 1;
        result
    }

    #[must_use]
    pub fn report(&self) -> UiLatencyReport {
        assert!(self.len > 0, "latency gate requires at least one frame");
        let mut sorted = self.samples;
        sorted[..self.len].sort_unstable();
        UiLatencyReport {
            frames: self.len,
            p95: percentile(&sorted[..self.len], 95),
            p99: percentile(&sorted[..self.len], 99),
            worst: sorted[self.len - 1],
        }
    }
}

fn percentile(samples: &[Duration], percentile: usize) -> Duration {
    let index = (samples.len() * percentile).div_ceil(100).saturating_sub(1);
    samples[index.min(samples.len() - 1)]
}

/// Worker-side access to progress reporting and cooperative cancellation.
#[derive(Clone)]
pub struct ProgressHandle {
    value: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
}

impl ProgressHandle {
    /// Stores a progress fraction clamped to `0.0..=1.0`.
    ///
    /// Non-finite input is treated as zero so it cannot produce a misleading
    /// completed state through float-to-integer casts.
    pub fn set(&self, fraction: f32) {
        let fraction = if fraction.is_finite() {
            fraction.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let value = (fraction * PROGRESS_MAX as f32).round() as u32;
        self.value.store(value.min(PROGRESS_MAX), Ordering::Relaxed);
    }

    /// Returns whether the owner has requested cooperative cancellation.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobErrorCode {
    Io,
    InvalidInput,
    Busy,
    Stale,
    Cancelled,
    External,
    TooLarge,
    Decode,
    Validation,
    NotFound,
    Unsupported,
    Internal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobFailure {
    pub code: JobErrorCode,
}

impl JobFailure {
    pub fn new(code: JobErrorCode) -> Self {
        Self { code }
    }
}

impl std::fmt::Display for JobFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.code)
    }
}

/// The single terminal result emitted by a job.
#[derive(Debug, PartialEq, Eq)]
pub enum JobMsg<T> {
    Done(T),
    Failed(JobFailure),
    Cancelled,
}

/// Identifies work for UI presentation and concurrency policy.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum JobKind {
    Save,
    AutoSave,
    ExportText,
    ExportVideo,
    CollisionScan,
    AssignmentOptimize,
    AudioDecode,
    UpdateCheck,
    Import,
}

impl JobKind {
    const fn thread_name(self) -> &'static str {
        match self {
            Self::Save => "drill-job-save",
            Self::AutoSave => "drill-job-autosave",
            Self::ExportText => "drill-job-export-text",
            Self::ExportVideo => "drill-job-export-video",
            Self::CollisionScan => "drill-job-scan",
            Self::AssignmentOptimize => "drill-job-assign",
            Self::AudioDecode => "drill-job-audio-decode",
            Self::UpdateCheck => "drill-job-update-check",
            Self::Import => "drill-job-import",
        }
    }
}

/// One running, or completed but not yet collected, background operation.
pub struct Job<T> {
    kind: JobKind,
    progress: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    rx: Receiver<JobMsg<T>>,
    finished: bool,
    handle: Option<JoinHandle<()>>,
}

impl<T: Send + 'static> Job<T> {
    /// Starts `body` on a named OS thread.
    ///
    /// Cancellation is cooperative: the body must periodically inspect its
    /// [`ProgressHandle`]. If cancellation was requested before the body
    /// returns, its result is converted to [`JobMsg::Cancelled`].
    #[must_use]
    pub fn spawn_typed<F>(kind: JobKind, body: F) -> Self
    where
        F: FnOnce(&ProgressHandle) -> Result<T, JobFailure> + Send + 'static,
    {
        let progress = Arc::new(AtomicU32::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let worker_progress = ProgressHandle {
            value: Arc::clone(&progress),
            cancel: Arc::clone(&cancel),
        };
        let handle = std::thread::Builder::new()
            .name(kind.thread_name().into())
            .spawn(move || {
                let outcome = panic::catch_unwind(AssertUnwindSafe(|| body(&worker_progress)));
                let message = match outcome {
                    Err(_) => JobMsg::Failed(JobFailure::new(JobErrorCode::Internal)),
                    Ok(_) if worker_progress.is_cancelled() => JobMsg::Cancelled,
                    Ok(Ok(value)) => JobMsg::Done(value),
                    Ok(Err(reason)) => JobMsg::Failed(reason),
                };
                let _ = tx.send(message);
            })
            .expect("failed to spawn job thread");

        Self {
            kind,
            progress,
            cancel,
            rx,
            finished: false,
            handle: Some(handle),
        }
    }

    #[must_use]
    pub const fn kind(&self) -> JobKind {
        self.kind
    }

    /// Returns a lock-free progress snapshot in `0.0..=1.0`.
    #[must_use]
    pub fn progress(&self) -> f32 {
        self.progress.load(Ordering::Relaxed) as f32 / PROGRESS_MAX as f32
    }

    /// Requests cooperative cancellation. This operation is idempotent.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Polls for the terminal result without blocking.
    ///
    /// After returning a message once, all later calls return `None`.
    pub fn poll(&mut self) -> Option<JobMsg<T>> {
        if self.finished {
            return None;
        }
        match self.rx.try_recv() {
            Ok(message) => {
                self.finished = true;
                // Receiving the terminal message proves the worker no longer
                // needs anything owned by Job. Dropping the handle detaches it
                // without ever blocking the polling thread.
                self.handle.take();
                Some(message)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.finished = true;
                self.handle.take();
                Some(JobMsg::Failed(JobFailure::new(JobErrorCode::Internal)))
            }
        }
    }

    #[must_use]
    pub const fn is_finished(&self) -> bool {
        self.finished
    }
}

impl<T> Drop for Job<T> {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        // Dropping JoinHandle detaches the worker. UI code must never wait here.
        self.handle.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hint::spin_loop;
    use std::time::{Duration, Instant};

    fn wait_for<T: Send + 'static>(job: &mut Job<T>) -> JobMsg<T> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(message) = job.poll() {
                return message;
            }
            assert!(Instant::now() < deadline, "job did not terminate");
            std::thread::yield_now();
        }
    }

    #[test]
    fn poll_is_non_blocking_before_result_arrives() {
        let mut job = Job::<()>::spawn_typed(JobKind::ExportText, |_| {
            std::thread::sleep(Duration::from_millis(50));
            Ok(())
        });
        let start = Instant::now();
        assert_eq!(job.poll(), None);
        assert!(start.elapsed() < Duration::from_millis(10));
        assert_eq!(wait_for(&mut job), JobMsg::Done(()));
    }

    #[test]
    fn done_is_delivered_exactly_once() {
        let mut job = Job::spawn_typed(JobKind::ExportText, |_| Ok(42));
        assert_eq!(wait_for(&mut job), JobMsg::Done(42));
        assert!(job.is_finished());
        assert_eq!(job.poll(), None);
    }

    #[test]
    fn save_and_analysis_never_execute_on_the_caller_thread() {
        let caller = std::thread::current().id();
        for kind in [JobKind::Save, JobKind::AutoSave, JobKind::CollisionScan] {
            let mut job = Job::spawn_typed(kind, move |_| Ok(std::thread::current().id()));
            let JobMsg::Done(worker) = wait_for(&mut job) else {
                panic!("background job did not finish successfully");
            };
            assert_ne!(worker, caller, "{kind:?} executed on the UI/caller thread");
        }
    }

    #[test]
    fn error_is_failed() {
        let mut job =
            Job::<()>::spawn_typed(JobKind::Save, |_| Err(JobFailure::new(JobErrorCode::Io)));
        assert_eq!(
            wait_for(&mut job),
            JobMsg::Failed(JobFailure::new(JobErrorCode::Io))
        );
    }

    #[test]
    fn typed_error_code_survives_worker_boundary() {
        let mut job = Job::<()>::spawn_typed(JobKind::Import, |_| {
            Err(JobFailure::new(JobErrorCode::Stale))
        });
        let JobMsg::Failed(error) = wait_for(&mut job) else {
            panic!("expected failure")
        };
        assert_eq!(error.code, JobErrorCode::Stale);
    }

    #[test]
    fn panic_is_contained() {
        let mut job = Job::<()>::spawn_typed(JobKind::CollisionScan, |_| panic!("boom"));
        assert_eq!(
            wait_for(&mut job),
            JobMsg::Failed(JobFailure::new(JobErrorCode::Internal))
        );
    }

    #[test]
    fn progress_is_clamped_and_non_finite_is_zero() {
        for (input, expected) in [
            (-2.0, 0.0),
            (0.25, 0.25),
            (2.0, 1.0),
            (f32::NAN, 0.0),
            (f32::INFINITY, 0.0),
        ] {
            let (ready_tx, ready_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let mut job = Job::spawn_typed(JobKind::AudioDecode, move |progress| {
                progress.set(input);
                ready_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok(())
            });
            ready_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert!((job.progress() - expected).abs() <= 1.0 / PROGRESS_MAX as f32);
            release_tx.send(()).unwrap();
            assert_eq!(wait_for(&mut job), JobMsg::Done(()));
        }
    }

    #[test]
    fn cooperative_cancel_returns_cancelled() {
        let (started_tx, started_rx) = mpsc::channel();
        let mut job = Job::spawn_typed(JobKind::AssignmentOptimize, move |progress| {
            started_tx.send(()).unwrap();
            while !progress.is_cancelled() {
                spin_loop();
            }
            Ok(99)
        });
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        job.cancel();
        job.cancel();
        assert_eq!(wait_for(&mut job), JobMsg::Cancelled);
    }
}
