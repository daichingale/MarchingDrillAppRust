use drill_core::Document;
use drill_export::RasterSurface;
use drill_render::{
    BuildScratch, DisplayList, RenderOptions, Scene, Theme, Vec2, Viewport, build_field_2d,
};
use std::hint::black_box;
use std::time::Instant;

fn main() {
    let document = Document::demo(25, 40);
    let mut list = DisplayList::new();
    let mut scratch = BuildScratch;
    build_field_2d(
        &Scene {
            document: &document,
            positions: &document.sets[0].positions,
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
        &mut scratch,
        &mut list,
    );
    let mut surface = RasterSurface::default();
    surface.resize(1920, 1080).unwrap();
    let start = Instant::now();
    for _ in 0..100 {
        black_box(surface.render(&list));
    }
    println!(
        "1000 performers × 100 1080p CPU frames: {:?}",
        start.elapsed()
    );
}
