use drill_core::{Document, Locale, Point, coordinates, production, svg};
use drill_export::{RasterSurface, page::PrintSettings, pdf, report};
use drill_render::{
    BuildScratch, DisplayList, RenderOptions, Scene, Theme, Vec2, Viewport, build_field_2d,
};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const UPDATE_TOKEN: &str = "reviewed";

fn fixture() -> Document {
    let mut doc = Document::demo(2, 3);
    doc.title = "Golden Parade / ゴールデン".into();
    doc.sets[0].name = "Opening".into();
    doc.sets[0].annotation.rehearsal_mark = "A".into();
    doc.sets[0].annotation.notes = "Lights: warm".into();
    doc.grid.coordinate_notation = coordinates::CoordinateNotation::dci();
    if let Some(position) = doc.sets.get_mut(1).and_then(|set| set.positions.get_mut(0)) {
        *position = Point { x: 46.25, y: 22.5 };
    }
    doc
}

fn fnv64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

fn byte_tag(bytes: &[u8]) -> String {
    format!("len={} fnv64={:016x}\n", bytes.len(), fnv64(bytes))
}

fn normalize_catalog(source: &str) -> String {
    let mut out = String::new();
    for line in source.lines().filter(|line| line.starts_with("| legacy.")) {
        let cells = line.split('|').map(str::trim).collect::<Vec<_>>();
        if cells.len() >= 7 {
            let _ = writeln!(
                out,
                "{} | {} | {} | {}",
                cells[1], cells[2], cells[5], cells[6]
            );
        }
    }
    out
}

fn artifacts() -> Vec<(&'static str, Vec<u8>)> {
    let doc = fixture();
    let positions = doc.sets[0].positions.clone();
    let options = RenderOptions::default();
    let scene = Scene {
        document: &doc,
        positions: &positions,
        viewport: Viewport {
            size: Vec2 { x: 320.0, y: 180.0 },
            ui_scale: 1.0,
        },
        options: &options,
        theme: &Theme::SCREEN_DARK,
    };
    let mut list = DisplayList::new();
    build_field_2d(&scene, &mut BuildScratch, &mut list);
    let mut raster = RasterSurface::default();
    raster.resize(320, 180).expect("bounded golden surface");
    let stats = raster.render(&list);
    let rgba = format!(
        "{}commands={} text_skipped={}\n",
        byte_tag(raster.pixels()),
        stats.commands,
        stats.text_skipped
    );
    let production_pdf = pdf::render_pdf(
        &doc,
        &report::production_sheet(&doc, Locale::En),
        &PrintSettings::default(),
    );
    let catalog = normalize_catalog(include_str!("../../../docs/MESSAGE_CATALOG.md"));
    vec![
        (
            "field.svg",
            svg::field_svg(&doc, 0, 640.0, 360.0).into_bytes(),
        ),
        (
            "coordinates.csv",
            coordinates::coordinates_csv_localized(&doc, Locale::En).into_bytes(),
        ),
        (
            "coordinate-sheet.html",
            svg::coordinate_sheet_html_localized(&doc, Locale::En).into_bytes(),
        ),
        (
            "production.tsv",
            production::production_sheet_text(&doc, Locale::En).into_bytes(),
        ),
        (
            "production-pdf.digest",
            byte_tag(&production_pdf).into_bytes(),
        ),
        ("render-rgba.digest", rgba.into_bytes()),
        (
            "message-catalog.digest",
            byte_tag(catalog.as_bytes()).into_bytes(),
        ),
    ]
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn first_difference(expected: &[u8], actual: &[u8]) -> String {
    let offset = expected
        .iter()
        .zip(actual)
        .position(|(a, b)| a != b)
        .unwrap_or(expected.len().min(actual.len()));
    let line = actual[..offset.min(actual.len())]
        .iter()
        .filter(|b| **b == b'\n')
        .count()
        + 1;
    let start = offset.saturating_sub(48);
    let expected_end = (offset + 96).min(expected.len());
    let actual_end = (offset + 96).min(actual.len());
    format!(
        "first difference at byte {offset}, line {line}\nexpected: {:?}\nactual:   {:?}",
        String::from_utf8_lossy(&expected[start.min(expected.len())..expected_end]),
        String::from_utf8_lossy(&actual[start.min(actual.len())..actual_end]),
    )
}

#[test]
fn approved_product_goldens_are_current() {
    let update = std::env::var("UPDATE_GOLDENS").ok();
    assert!(
        update.as_deref().is_none_or(|value| value == UPDATE_TOKEN),
        "refusing unknown UPDATE_GOLDENS value; use UPDATE_GOLDENS={UPDATE_TOKEN} only"
    );
    if update.is_some() {
        assert!(
            std::env::var_os("CI").is_none(),
            "goldens cannot be updated in CI"
        );
        std::fs::create_dir_all(golden_dir()).expect("create committed golden directory");
    }
    let mut failures = Vec::new();
    for (name, actual) in artifacts() {
        let path = golden_dir().join(name);
        if update.is_some() {
            std::fs::write(&path, actual)
                .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
            continue;
        }
        match std::fs::read(&path) {
            Ok(expected) if expected == actual => {}
            Ok(expected) => failures.push(format!(
                "{}\n{}",
                path.display(),
                first_difference(&expected, &actual)
            )),
            Err(error) => failures.push(format!("{} is missing ({error})", path.display())),
        }
    }
    assert!(
        failures.is_empty(),
        "golden approval check failed:\n\n{}\n\nReview the change, then run scripts/update-goldens.ps1 -Approve reviewed",
        failures.join("\n\n")
    );
}

#[test]
fn mismatch_diagnostic_is_bounded_and_locates_first_line() {
    let diagnostic = first_difference(b"same\nexpected\ntail", b"same\nactual\ntail");
    assert!(diagnostic.contains("line 2"));
    assert!(diagnostic.contains("expected"));
    assert!(diagnostic.contains("actual"));
    assert!(diagnostic.len() < 512);
}
