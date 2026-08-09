use drill_core::{Document, Point};
use drill_render::set_svg;

#[test]
fn persisted_document_is_byte_deterministic_across_round_trips() {
    let original = Document::demo(16, 8);
    let first = original.to_json().expect("serialize fixture");
    let loaded = Document::from_json(&first).expect("load fixture");
    let second = loaded.to_json().expect("serialize loaded fixture");
    assert_eq!(first, second);
}

#[test]
fn display_list_svg_is_byte_deterministic() {
    let document = Document::demo(16, 3);
    let first = set_svg(&document, 1);
    let second = set_svg(&document, 1);
    assert_eq!(first.as_bytes(), second.as_bytes());
    // Performer dots are present in addition to optional dotted-grid geometry.
    assert!(first.matches("<circle").count() >= 16);
}

#[test]
fn interpolation_is_repeatable_for_dense_fraction_grid() {
    let document = Document::demo(64, 4);
    let mut first = Vec::<Point>::new();
    let mut second = Vec::<Point>::new();
    for numerator in 0..=256 {
        let fraction = numerator as f32 / 256.0;
        document.positions_at(1, fraction, &mut first);
        document.positions_at(1, fraction, &mut second);
        assert_eq!(first, second, "fraction {fraction}");
        assert!(
            first
                .iter()
                .all(|point| point.x.is_finite() && point.y.is_finite())
        );
    }
}
