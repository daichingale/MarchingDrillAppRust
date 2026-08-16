use drill_core::{
    ChordPoint, Document, Easing, Edit, Gate, History, PathVia, Point, Route, RouteShape,
    TransitionPlan,
    clinic::{ClinicParams, ScanScratch, scan_transition},
    eval_transition,
    tempo::{TempoChange, TempoMap},
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

struct CountingAllocator;
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the request is forwarded unchanged to the system allocator.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: the pointer/layout pair came from the system allocator above.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the pointer/layout pair came from the system allocator above.
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

const FRAME_BUDGET: std::time::Duration = std::time::Duration::from_micros(16_600);
const CLINIC_BUDGET: std::time::Duration = std::time::Duration::from_millis(2);

fn main() {
    let mut document = Document::demo(100, 10);
    document.sets[0].routes.default = Route {
        shape: RouteShape::Curve {
            control: ChordPoint {
                along: 0.5,
                lateral: 0.35,
            },
        },
        gate: Gate::FULL,
        easing: Easing::Smooth,
    };
    let mut positions = Vec::with_capacity(1_000);
    document.positions_at(0, 0.0, &mut positions);
    let positions_pointer = positions.as_ptr();
    let positions_capacity = positions.capacity();
    let allocations_before = ALLOCATIONS.load(Ordering::Relaxed);
    let started = Instant::now();
    for frame in 0..60_000 {
        document.positions_at(0, (frame % 1_000) as f32 / 999.0, &mut positions);
        black_box(&positions);
    }
    let interpolation = started.elapsed();
    let interpolation_allocations = ALLOCATIONS.load(Ordering::Relaxed) - allocations_before;

    let mut plan = TransitionPlan::default();
    document.plan_transition(document.sets[0].id, &mut plan);
    eval_transition(&plan, 0.0, &mut positions);
    let plan_allocations_before = ALLOCATIONS.load(Ordering::Relaxed);
    let plan_started = Instant::now();
    for frame in 0..60_000 {
        eval_transition(&plan, (frame % 1_000) as f32 * 16.0 / 999.0, &mut positions);
        black_box(&positions);
    }
    let planned_interpolation = plan_started.elapsed();
    let plan_allocations = ALLOCATIONS.load(Ordering::Relaxed) - plan_allocations_before;
    assert_shape_hot_loop_no_alloc(RouteShape::Path {
        via: PathVia::Relative(vec![
            ChordPoint {
                along: 0.3,
                lateral: 0.2,
            },
            ChordPoint {
                along: 0.7,
                lateral: -0.2,
            },
        ]),
    });
    assert_shape_hot_loop_no_alloc(RouteShape::Arc { bulge: 0.45 });

    let started = Instant::now();
    for _ in 0..100 {
        black_box(
            document
                .to_json()
                .expect("benchmark document must serialize"),
        );
    }
    let serialization = started.elapsed();

    let tempo = TempoMap::from_changes((0..65_536).map(|i| TempoChange {
        count: i as f32 * 4.0,
        bpm: 60.0 + (i % 180) as f32,
    }));
    let started = Instant::now();
    for i in 0..1_000_000 {
        let count = (i % 262_144) as f64;
        black_box(tempo.count_at_f64(tempo.seconds_at_f64(count)));
    }
    let tempo_lookup = started.elapsed();

    let mut clinic_document = document.clone();
    clinic_document.sets[0].counts = 16;
    clinic_document.sets[1].positions = clinic_document.sets[0]
        .positions
        .iter()
        .map(|point| drill_core::Point {
            x: point.x + 0.5,
            y: point.y + 0.25,
        })
        .collect();
    let mut scan_scratch = ScanScratch::default();
    scan_transition(
        &clinic_document,
        0,
        ClinicParams::default(),
        &mut scan_scratch,
    );
    let started = Instant::now();
    for _ in 0..100 {
        black_box(scan_transition(
            &clinic_document,
            0,
            ClinicParams::default(),
            &mut scan_scratch,
        ));
    }
    let clinic_scan = started.elapsed() / 100;

    // PRODUCT_QUALITY reliability gate: exercise the stable-ID command path on
    // a production-size document, then prove the entire prefix is reversible.
    let mut edit_document = Document::demo(100, 10);
    let set_id = edit_document.sets[0].id;
    let performer_id = edit_document.performers[0].id;
    let original = edit_document.sets[0].positions[0];
    let mut history = History::with_limit(10_000);
    let started = Instant::now();
    for index in 0..10_000 {
        history
            .execute(
                &mut edit_document,
                Edit::MovePerformers {
                    set_id,
                    performer_ids: vec![performer_id],
                    positions: vec![Point {
                        x: index as f32 * 0.001,
                        y: 1.0,
                    }],
                },
            )
            .expect("10,000-edit stress command must apply");
    }
    for _ in 0..10_000 {
        assert!(history.undo(&mut edit_document));
    }
    assert_eq!(edit_document.sets[0].positions[0], original);
    for _ in 0..10_000 {
        assert!(history.redo(&mut edit_document));
    }
    let edit_stress = started.elapsed();

    println!("1,000 performers × 60,000 interpolation frames: {interpolation:?}");
    println!("1,000 performers × 60,000 compiled curve frames: {planned_interpolation:?}");
    println!("1,000 performers × 100 JSON serializations: {serialization:?}");
    println!("65,536 tempo events × 1,000,000 round trips: {tempo_lookup:?}");
    println!("1,000 performers × 16 counts swept clinic average: {clinic_scan:?} (target <2ms)");
    println!("1,000 performers × 10,000 edits + undo + redo: {edit_stress:?}");
    println!("steady interpolation allocations: {interpolation_allocations} (target 0)");
    println!("compiled interpolation allocations: {plan_allocations} (target 0)");
    assert_eq!(
        positions.as_ptr(),
        positions_pointer,
        "interpolation reallocated its output"
    );
    assert_eq!(positions.capacity(), positions_capacity);
    assert_eq!(
        interpolation_allocations, 0,
        "interpolation hot path allocated"
    );
    assert_eq!(
        plan_allocations, 0,
        "compiled interpolation hot path allocated"
    );
    assert!(
        interpolation / 60_000 < FRAME_BUDGET,
        "interpolation exceeded the 16.6ms frame budget"
    );
    assert!(
        clinic_scan < CLINIC_BUDGET,
        "clinic scan exceeded its 2ms budget"
    );
}

fn assert_shape_hot_loop_no_alloc(shape: RouteShape) {
    let mut document = Document::demo(100, 10);
    document.sets[0].routes.default.shape = shape;
    let mut plan = TransitionPlan::default();
    document.plan_transition(document.sets[0].id, &mut plan);
    let mut positions = Vec::with_capacity(document.performers.len());
    eval_transition(&plan, 0.0, &mut positions);
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    for frame in 0..1_024 {
        eval_transition(&plan, (frame % 64) as f32 / 4.0, &mut positions);
        black_box(&positions);
    }
    assert_eq!(
        ALLOCATIONS.load(Ordering::Relaxed) - before,
        0,
        "compiled Path/Arc hot loop allocated"
    );
}
