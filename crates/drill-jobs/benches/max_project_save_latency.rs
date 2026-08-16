use drill_core::{Document, SetId};
use drill_jobs::{Job, JobErrorCode, JobFailure, JobKind, JobMsg, UiLatencyBudget, UiLatencyGate};
use drill_project::container::{AssetEntry, AssetId, AssetKind, AssetLocation, SaveProject};
use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Barrier;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

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
        let next = unsafe { System.realloc(pointer, old, new_size) };
        if !next.is_null() {
            if new_size >= old.size() {
                track_add(new_size - old.size());
            } else {
                LIVE.fetch_sub(old.size() - new_size, Ordering::Relaxed);
            }
        }
        next
    }
}
fn track_add(bytes: usize) {
    let live = LIVE
        .fetch_add(bytes, Ordering::Relaxed)
        .saturating_add(bytes);
    PEAK.fetch_max(live, Ordering::Relaxed);
}

fn near_limit_document() -> Document {
    let mut document = Document::demo(40, 100);
    let template = document.sets[0].clone();
    for raw in 3..=240u32 {
        let mut set = template.clone();
        set.id = SetId::new(raw).unwrap();
        set.name = format!("Set {raw}");
        document.sets.push(set);
    }
    document.validate().expect("near-limit fixture");
    document
}

fn assets() -> (Vec<AssetEntry>, BTreeMap<AssetId, Vec<u8>>) {
    let mut entries = Vec::with_capacity(3_500);
    let mut bytes = BTreeMap::new();
    for raw in 1..=3_500u32 {
        let id = AssetId(raw);
        let payload = vec![(raw % 251) as u8];
        entries.push(AssetEntry {
            id,
            kind: if raw % 2 == 0 {
                AssetKind::Audio
            } else {
                AssetKind::Image
            },
            location: AssetLocation::Embedded {
                entry: format!("assets/{raw:04}.bin"),
            },
            original_name: format!("asset-{raw:04}.bin"),
            byte_len: payload.len() as u64,
            blake3_hex: blake3::hash(&payload).to_hex().to_string(),
        });
        bytes.insert(id, payload);
    }
    (entries, bytes)
}

fn main() {
    const FRAMES: usize = 600;
    const MAX_HEAP_GROWTH: usize = 768 * 1024 * 1024;
    let root = std::env::temp_dir().join(format!("drillforge-max-save-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let document = near_limit_document();
    let json_bytes = document.to_json().unwrap().len();
    assert!(json_bytes > 16 << 20, "fixture is not production-scale");
    assert!(json_bytes < drill_project::container::MAX_DOCUMENT_BYTES as usize);
    let (entries, embedded) = assets();
    let baseline = LIVE.load(Ordering::Relaxed);
    PEAK.store(baseline, Ordering::Relaxed);
    let caller = std::thread::current().id();

    let start = Arc::new(Barrier::new(3));
    let save_start = Arc::clone(&start);
    let save_path = root.join("maximum.drillproj");
    let save_doc = document.clone();
    let mut save = Job::spawn_typed(JobKind::Save, move |_| {
        assert_ne!(
            std::thread::current().id(),
            caller,
            "save I/O reached caller thread"
        );
        save_start.wait();
        drill_project::container::save(
            &save_path,
            &SaveProject {
                document: &save_doc,
                app_version: "latency-gate",
                created_utc: "0",
                modified_utc: "0",
                embedded: &embedded,
                assets: &entries,
            },
        )
        .map_err(|_| JobFailure::new(JobErrorCode::Io))?;
        Ok(())
    });
    let autosave_start = Arc::clone(&start);
    let autosave_doc = document;
    let autosave_root = root.clone();
    let mut autosave = Job::spawn_typed(JobKind::AutoSave, move |_| {
        assert_ne!(
            std::thread::current().id(),
            caller,
            "autosave I/O reached caller thread"
        );
        autosave_start.wait();
        let mut session =
            drill_project::recovery::Session::open_new(&autosave_root, "latency-gate")
                .map_err(|_| JobFailure::new(JobErrorCode::Io))?;
        session
            .write_autosave(&autosave_doc)
            .map_err(|_| JobFailure::new(JobErrorCode::Io))?;
        Ok(())
    });
    start.wait();

    let mut gate = UiLatencyGate::<FRAMES>::default();
    for _ in 0..FRAMES {
        gate.record(|| {
            std::hint::black_box(save.poll());
            std::hint::black_box(save.progress());
            std::hint::black_box(autosave.poll());
            std::hint::black_box(autosave.progress());
        });
    }
    let report = gate.report();
    assert!(
        report.meets(UiLatencyBudget::default()),
        "maximum-save UI latency regression: {report:?}"
    );

    let deadline = Instant::now() + Duration::from_secs(60);
    let mut save_done = save.is_finished();
    let mut autosave_done = autosave.is_finished();
    while !(save_done && autosave_done) {
        if let Some(message) = save.poll() {
            assert!(
                matches!(message, JobMsg::Done(())),
                "save failed: {message:?}"
            );
            save_done = true;
        }
        if let Some(message) = autosave.poll() {
            assert!(
                matches!(message, JobMsg::Done(())),
                "autosave failed: {message:?}"
            );
            autosave_done = true;
        }
        assert!(Instant::now() < deadline, "maximum save workers timed out");
        std::thread::yield_now();
    }
    let heap_growth = PEAK.load(Ordering::Relaxed).saturating_sub(baseline);
    assert!(
        heap_growth <= MAX_HEAP_GROWTH,
        "heap growth {heap_growth} exceeds {MAX_HEAP_GROWTH}"
    );
    println!(
        "max_project_save_ui: {report:?}; json={json_bytes}; assets=3500; peak_heap_growth={heap_growth}"
    );
    let _ = std::fs::remove_dir_all(root);
}
