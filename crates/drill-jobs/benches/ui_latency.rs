use drill_jobs::{Job, JobKind, UiLatencyBudget, UiLatencyGate};
use std::sync::{Arc, Barrier};

fn main() {
    const FRAMES: usize = 120;
    let kinds = [
        JobKind::Save,
        JobKind::AutoSave,
        JobKind::Import,
        JobKind::CollisionScan,
        JobKind::AudioDecode,
        JobKind::ExportVideo,
    ];
    let entered = Arc::new(Barrier::new(kinds.len() + 1));
    let release = Arc::new(Barrier::new(kinds.len() + 1));
    let mut jobs: Vec<_> = kinds
        .into_iter()
        .map(|kind| {
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            Job::spawn_typed(kind, move |progress| {
                entered.wait();
                progress.set(0.5);
                release.wait();
                Ok(())
            })
        })
        .collect();
    entered.wait();
    let mut gate = UiLatencyGate::<FRAMES>::default();
    for _ in 0..FRAMES {
        gate.record(|| {
            for job in &mut jobs {
                std::hint::black_box(job.poll());
                std::hint::black_box(job.progress());
            }
        });
    }
    let report = gate.report();
    println!("ui_latency_6_jobs_120_frames: {report:?}");
    release.wait();
    assert!(report.meets(UiLatencyBudget::default()));
}
