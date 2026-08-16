use drill_jobs::{Job, JobKind, UiLatencyBudget, UiLatencyGate};
use std::sync::{Arc, Barrier};
use std::time::Duration;

const FRAMES: usize = 120;

/// Represents save, autosave, import, clinic, audio decode and export
/// preflight all being slow at once. Workers remain blocked for the whole
/// frame loop, proving the caller only performs lock-free snapshots/try_recv.
#[test]
fn six_slow_product_jobs_do_not_block_120_ui_frames() {
    let kinds = [
        JobKind::Save,
        JobKind::AutoSave,
        JobKind::Import,
        JobKind::CollisionScan,
        JobKind::AudioDecode,
        JobKind::ExportVideo,
    ];
    let caller = std::thread::current().id();
    let entered = Arc::new(Barrier::new(kinds.len() + 1));
    let release = Arc::new(Barrier::new(kinds.len() + 1));
    let mut jobs: Vec<_> = kinds
        .into_iter()
        .map(|kind| {
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            Job::spawn_typed(kind, move |progress| {
                assert_ne!(std::thread::current().id(), caller);
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
                assert!(job.poll().is_none());
                std::hint::black_box(job.progress());
            }
        });
    }
    let report = gate.report();
    assert!(
        report.meets(UiLatencyBudget::default()),
        "UI latency regression with slow jobs: {report:?}"
    );

    release.wait();
    // Do not leave detached workers running into the next test process phase.
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while jobs.iter().any(|job| !job.is_finished()) {
        for job in &mut jobs {
            let _ = job.poll();
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
}

/// Source-level guard for accidental synchronous filesystem work in the main
/// egui frame function. Concrete state modules may mention fs only inside a
/// `Job::spawn` worker and are covered dynamically above.
#[test]
fn egui_frame_has_no_direct_filesystem_io() {
    let frame_source = include_str!("../../drill-app/src/app_ui.rs");
    for forbidden in ["std::fs::", "File::open", "File::create", "read_to_string("] {
        assert!(
            !frame_source.contains(forbidden),
            "caller-thread I/O entered app_ui.rs: {forbidden}"
        );
    }
}

/// The generic string-error boundary was intentionally removed. Keep every
/// product callsite on stable, typed error codes so UI copy can be localized
/// without parsing worker-provided text.
#[test]
fn workspace_jobs_use_only_the_typed_failure_boundary() {
    let sources = [
        ("drill-jobs", include_str!("../src/lib.rs")),
        ("audio", include_str!("../../drill-app/src/audio_state.rs")),
        (
            "export-ui",
            include_str!("../../drill-app/src/export_state.rs"),
        ),
        (
            "import",
            include_str!("../../drill-app/src/import_state.rs"),
        ),
        (
            "project",
            include_str!("../../drill-app/src/project_state.rs"),
        ),
        (
            "underlay",
            include_str!("../../drill-app/src/underlay_state.rs"),
        ),
        (
            "update",
            include_str!("../../drill-app/src/update_state.rs"),
        ),
        (
            "text-export",
            include_str!("../../drill-app/src/text_export_state.rs"),
        ),
        ("export", include_str!("../../drill-export/src/lib.rs")),
        ("pdf", include_str!("../../drill-export/src/pdf.rs")),
    ];

    for (name, source) in sources {
        for forbidden in [
            "Job::spawn(",
            "JobErrorCode::Legacy",
            "with_detail(",
            ".detail",
            "Result<T, String>",
        ] {
            assert!(
                !source.contains(forbidden),
                "legacy job failure boundary re-entered {name}: {forbidden}"
            );
        }
    }
}
