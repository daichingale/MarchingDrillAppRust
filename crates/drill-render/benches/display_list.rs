use drill_core::Document;
use drill_render::{
    BuildScratch, DisplayList, RenderOptions, Scene, Theme, Vec2, Viewport, build_field_2d,
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
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

fn main() {
    let doc = Document::demo(25, 40);
    let options = RenderOptions::default();
    let scene = Scene {
        document: &doc,
        positions: &doc.sets[0].positions,
        viewport: Viewport {
            size: Vec2 {
                x: 1920.0,
                y: 1080.0,
            },
            ui_scale: 1.5,
        },
        options: &options,
        theme: &Theme::SCREEN_DARK,
    };
    let mut out = DisplayList::new();
    let mut scratch = BuildScratch;
    build_field_2d(&scene, &mut scratch, &mut out);
    let capacities = out.capacities();
    let allocations_before = ALLOCATIONS.load(Ordering::Relaxed);
    let start = Instant::now();
    for _ in 0..10_000 {
        build_field_2d(black_box(&scene), &mut scratch, black_box(&mut out));
    }
    let elapsed = start.elapsed();
    let allocations = ALLOCATIONS.load(Ordering::Relaxed) - allocations_before;
    eprintln!(
        "display_list_1000x10000: {elapsed:?} ({:?}/frame), allocations={allocations}",
        elapsed / 10_000
    );
    assert_eq!(
        out.capacities(),
        capacities,
        "display-list buffers grew after warm-up"
    );
    assert_eq!(
        allocations, 0,
        "display-list hot path allocated after warm-up"
    );
    assert!(
        elapsed / 10_000 < std::time::Duration::from_micros(16_600),
        "display-list build exceeded the 16.6ms frame budget"
    );

    // End-to-end CPU frame preparation: interpolate all 1,000 performers and
    // immediately turn the result into the backend-neutral display list.
    let mut positions = Vec::with_capacity(1_000);
    doc.positions_at(0, 0.5, &mut positions);
    let mut frame_out = DisplayList::new();
    let frame_scene = Scene {
        document: &doc,
        positions: &positions,
        viewport: Viewport {
            size: Vec2 {
                x: 1920.0,
                y: 1080.0,
            },
            ui_scale: 1.5,
        },
        options: &options,
        theme: &Theme::SCREEN_DARK,
    };
    build_field_2d(&frame_scene, &mut scratch, &mut frame_out);
    let frame_capacities = frame_out.capacities();
    let allocations_before = ALLOCATIONS.load(Ordering::Relaxed);
    let started = Instant::now();
    for frame in 0..10_000 {
        doc.positions_at(0, (frame % 1_000) as f32 / 999.0, &mut positions);
        let frame_scene = Scene {
            document: &doc,
            positions: &positions,
            viewport: Viewport {
                size: Vec2 {
                    x: 1920.0,
                    y: 1080.0,
                },
                ui_scale: 1.5,
            },
            options: &options,
            theme: &Theme::SCREEN_DARK,
        };
        build_field_2d(&frame_scene, &mut scratch, &mut frame_out);
        black_box(&frame_out);
    }
    let frame_elapsed = started.elapsed();
    let frame_allocations = ALLOCATIONS.load(Ordering::Relaxed) - allocations_before;
    eprintln!(
        "interpolate+display_list_1000x10000: {frame_elapsed:?} ({:?}/frame), allocations={frame_allocations}",
        frame_elapsed / 10_000
    );
    assert_eq!(frame_out.capacities(), frame_capacities);
    assert_eq!(frame_allocations, 0, "combined frame preparation allocated");
    assert!(
        frame_elapsed / 10_000 < std::time::Duration::from_micros(16_600),
        "1,000-performer CPU frame preparation exceeded 16.6ms"
    );
}
