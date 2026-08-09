use drill_core::Document;
use std::hint::black_box;
use std::time::Instant;

fn main() {
    let document = Document::demo(100, 10);
    let mut positions = Vec::with_capacity(1_000);
    let started = Instant::now();
    for frame in 0..60_000 {
        document.positions_at(0, (frame % 1_000) as f32 / 999.0, &mut positions);
        black_box(&positions);
    }
    let interpolation = started.elapsed();

    let started = Instant::now();
    for _ in 0..100 {
        black_box(
            document
                .to_json()
                .expect("benchmark document must serialize"),
        );
    }
    let serialization = started.elapsed();

    println!("1,000 performers × 60,000 interpolation frames: {interpolation:?}");
    println!("1,000 performers × 100 JSON serializations: {serialization:?}");
    assert!(positions.capacity() >= 1_000);
}
