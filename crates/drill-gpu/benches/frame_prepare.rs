use drill_gpu::GpuFrame;
use drill_render::{
    BuildScratch, DisplayList, RenderOptions, Scene, Theme, Vec2, Viewport, build_field_2d,
};
use std::{hint::black_box, time::Instant};

fn main() {
    let document = drill_core::Document::demo(100, 10);
    let positions = document.sets[0].positions.clone();
    let mut list = DisplayList::new();
    build_field_2d(
        &Scene {
            document: &document,
            positions: &positions,
            viewport: Viewport {
                size: Vec2 {
                    x: 1920.0,
                    y: 1080.0,
                },
                ui_scale: 1.0,
            },
            options: &RenderOptions::default(),
            theme: &Theme::SCREEN_DARK,
        },
        &mut BuildScratch,
        &mut list,
    );
    let mut frame = GpuFrame::new();
    frame.update_from_display_list(&list);
    let iterations = 20_000;
    let start = Instant::now();
    for _ in 0..iterations {
        frame.update_from_display_list(black_box(&list));
    }
    println!(
        "gpu_frame_1000: {:.3} us/frame; capacity={}",
        start.elapsed().as_secs_f64() * 1e6 / f64::from(iterations),
        frame.capacity()
    );

    let start = Instant::now();
    for _ in 0..iterations {
        frame.update_stadium(black_box(&document), black_box(&positions));
    }
    println!(
        "gpu_stadium_1000: {:.3} us/frame; capacity={}",
        start.elapsed().as_secs_f64() * 1e6 / f64::from(iterations),
        frame.capacity()
    );
}
