use drill_core::aesthetics::{self, AestheticParams};
use drill_core::rhythm_sync::{self, RhythmSyncParams};
use drill_core::show_heatmap::{self, HeatmapParams};
use drill_core::{Document, SetId};
use drill_jobs::{Job, JobErrorCode, JobFailure, JobKind, JobMsg, UiLatencyBudget, UiLatencyGate};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Arc;
use std::sync::Barrier;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const MAX_WORKER_TIME: Duration = Duration::from_secs(30);
const MAX_HEAP_GROWTH: usize = 768 * 1024 * 1024;

struct TrackingAllocator;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
#[global_allocator]
static ALLOCATOR: TrackingAllocator = TrackingAllocator;

unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            track_add(layout.size());
        }
        pointer
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(pointer, layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, old: Layout, new_size: usize) -> *mut u8 {
        let pointer = unsafe { System.realloc(pointer, old, new_size) };
        if !pointer.is_null() {
            if new_size >= old.size() {
                track_add(new_size - old.size());
            } else {
                LIVE.fetch_sub(old.size() - new_size, Ordering::Relaxed);
            }
        }
        pointer
    }
}

fn track_add(bytes: usize) {
    let live = LIVE
        .fetch_add(bytes, Ordering::Relaxed)
        .saturating_add(bytes);
    PEAK.fetch_max(live, Ordering::Relaxed);
}

fn maximum_document() -> Document {
    let mut document = Document::demo(40, 100);
    let template = document.sets[0].clone();
    for raw in 3..=240_u32 {
        let mut set = template.clone();
        set.id = SetId::new(raw).unwrap();
        set.name = format!("Set {raw}");
        document.sets.push(set);
    }
    document.validate().expect("maximum analytics fixture");
    document
}

fn main() {
    const UI_FRAMES: usize = 600;
    let document = maximum_document();
    assert_eq!(document.performers.len(), 4_000);
    assert_eq!(document.sets.len(), 240);
    let caller = std::thread::current().id();
    let baseline = LIVE.load(Ordering::Relaxed);
    PEAK.store(baseline, Ordering::Relaxed);
    let started = Instant::now();
    let mut worker = Job::spawn_typed(JobKind::Analytics, move |progress| {
        assert_ne!(
            std::thread::current().id(),
            caller,
            "analytics reached UI thread"
        );
        let rhythm = rhythm_sync::analyze_show(&document, &RhythmSyncParams::default());
        progress.set(0.34);
        if progress.is_cancelled() {
            return Err(JobFailure::new(JobErrorCode::Cancelled));
        }
        let aesthetics = aesthetics::analyze_set(&document, 120, &AestheticParams::default());
        progress.set(0.67);
        if progress.is_cancelled() {
            return Err(JobFailure::new(JobErrorCode::Cancelled));
        }
        let heatmap = show_heatmap::analyze_show_occupancy(&document, &HeatmapParams::default());
        progress.set(1.0);
        Ok((
            rhythm.events.len(),
            aesthetics.map(|v| v.overall),
            heatmap.max(),
        ))
    });

    let mut gate = UiLatencyGate::<UI_FRAMES>::default();
    for _ in 0..UI_FRAMES {
        gate.record(|| {
            std::hint::black_box(worker.progress());
            std::hint::black_box(worker.poll());
        });
    }
    let ui = gate.report();
    assert!(
        ui.meets(UiLatencyBudget::default()),
        "analytics UI poll regression: {ui:?}"
    );
    let deadline = Instant::now() + MAX_WORKER_TIME;
    let result = loop {
        if let Some(message) = worker.poll() {
            break message;
        }
        assert!(
            Instant::now() < deadline,
            "maximum analytics worker timed out"
        );
        std::thread::yield_now();
    };
    assert!(
        matches!(result, JobMsg::Done(_)),
        "analytics failed: {result:?}"
    );
    let compute = started.elapsed();
    assert!(
        compute <= MAX_WORKER_TIME,
        "analytics exceeded {MAX_WORKER_TIME:?}: {compute:?}"
    );

    let barrier = Arc::new(Barrier::new(2));
    let worker_barrier = Arc::clone(&barrier);
    let mut cancelled = Job::spawn_typed(JobKind::Analytics, move |progress| {
        worker_barrier.wait();
        if progress.is_cancelled() {
            return Err(JobFailure::new(JobErrorCode::Cancelled));
        }
        Ok(())
    });
    cancelled.cancel();
    barrier.wait();
    let cancel_deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(message) = cancelled.poll() {
            assert_eq!(message, JobMsg::Cancelled);
            break;
        }
        assert!(
            Instant::now() < cancel_deadline,
            "analytics cancellation timed out"
        );
        std::thread::yield_now();
    }

    let heap_growth = PEAK.load(Ordering::Relaxed).saturating_sub(baseline);
    assert!(
        heap_growth <= MAX_HEAP_GROWTH,
        "analytics heap {heap_growth} > {MAX_HEAP_GROWTH}"
    );
    println!(
        "analytics_maximum: compute={compute:?}; ui={ui:?}; peak_heap_growth={heap_growth}; performers=4000; sets=240; progress=bounded; cancel=pass"
    );
}
