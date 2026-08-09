//! Deterministic complexity/DoS gate for every untrusted document import surface.
//!
//! This intentionally measures logical operations and declared sizes, never wall time.

use drill_core::{Document, MAX_PROJECT_JSON_BYTES};
use drill_interop::musical::{MusicalError, MusicalImportOptions, import_midi, import_musicxml};
use drill_interop::underlay::{UnderlayError, UnderlayLimits, decode_underlay};
use drill_interop::xlsx::inspect_xlsx;
use drill_interop::{ImportError, ImportLimits, sniff};
use drill_project::container::{
    AssetEntry, AssetId, AssetKind, AssetLocation, MAX_ENTRIES, ProjectError, SaveProject, encode,
};
use image::ImageEncoder;
use std::collections::BTreeMap;
use std::io::Cursor;
use zip::write::SimpleFileOptions;

#[derive(Default)]
struct Operations(u64);
impl Operations {
    fn charge(&mut self, units: usize) {
        self.0 = self.0.checked_add(units as u64).expect("operation count");
    }
    fn assert_linear(&self, input_bytes: usize, multiplier: u64) {
        assert!(self.0 <= (input_bytes as u64).saturating_mul(multiplier));
    }
}

#[test]
fn json_and_csv_reject_before_superlinear_domain_work() {
    let oversized = " ".repeat(MAX_PROJECT_JSON_BYTES + 1);
    assert!(Document::from_json(&oversized).is_err());

    for rows in [1usize, 17, 257, 4096] {
        let mut csv = String::from("performer,set,x,y\n");
        let mut ops = Operations::default();
        for i in 0..rows {
            let row = format!("P{i},S{},1,2\n", i / 8);
            ops.charge(row.len());
            csv.push_str(&row);
        }
        let limits = ImportLimits {
            max_rows: rows,
            max_bytes: csv.len(),
            ..Default::default()
        };
        let preview = sniff(csv.as_bytes(), &limits).expect("bounded CSV");
        assert_eq!(preview.total_rows, rows);
        ops.assert_linear(csv.len(), 1);
    }
    let limits = ImportLimits {
        max_field_bytes: 16,
        ..Default::default()
    };
    assert!(matches!(
        sniff(format!("x\n{}", "q".repeat(17)).as_bytes(), &limits),
        Err(ImportError::Limit { .. })
    ));
}

fn midi_with_events(events: usize) -> Vec<u8> {
    let mut track = Vec::with_capacity(events * 4 + 4);
    for _ in 0..events {
        track.extend_from_slice(&[0, 0x90, 60, 64]);
    }
    track.extend_from_slice(&[0, 0xff, 0x2f, 0]);
    let mut out = b"MThd\0\0\0\x06\0\0\0\x01\x01\xe0MTrk".to_vec();
    out.extend_from_slice(&(track.len() as u32).to_be_bytes());
    out.extend_from_slice(&track);
    out
}

#[test]
fn midi_and_musicxml_have_byte_and_token_envelopes() {
    for events in [1usize, 128, 4096, 32_768] {
        let midi = midi_with_events(events);
        let mut ops = Operations::default();
        ops.charge(events + midi.len());
        import_midi(&midi, &MusicalImportOptions::default()).expect("bounded MIDI");
        ops.assert_linear(midi.len(), 2);
    }
    let tiny = MusicalImportOptions {
        max_bytes: 8,
        ..Default::default()
    };
    assert!(matches!(
        import_midi(&midi_with_events(2), &tiny),
        Err(MusicalError::TooLarge { .. })
    ));

    let xml = format!(
        "<score-timewise>{}</score-timewise>",
        "<x></x>".repeat(250_001)
    );
    let mut ops = Operations::default();
    ops.charge(xml.bytes().filter(|b| *b == b'<').count());
    assert!(matches!(
        import_musicxml(xml.as_bytes(), &Default::default()),
        Err(MusicalError::Limit("XML nodes"))
    ));
    ops.assert_linear(xml.len(), 1);
}

fn zip_with_entries(entries: usize) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default();
    for i in 0..entries {
        writer.start_file(format!("junk/{i}"), options).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

#[test]
fn xlsx_container_and_underlay_reject_declared_complexity() {
    let hostile_xlsx = zip_with_entries(257);
    assert!(matches!(
        inspect_xlsx(&hostile_xlsx, &Default::default()),
        Err(ImportError::Limit {
            what: "xlsx entries",
            limit: 256
        })
    ));

    let assets: Vec<_> = (0..MAX_ENTRIES - 2)
        .map(|i| AssetEntry {
            id: AssetId(i as u32),
            kind: AssetKind::Audio,
            location: AssetLocation::External {
                relative: None,
                absolute_hint: None,
            },
            original_name: format!("asset-{i}"),
            byte_len: 0,
            blake3_hex: "0".repeat(64),
        })
        .collect();
    let document = Document::demo(1, 1);
    let request = SaveProject {
        document: &document,
        app_version: "test",
        created_utc: "0",
        modified_utc: "0",
        embedded: &BTreeMap::new(),
        assets: &assets,
    };
    assert!(matches!(
        encode(&request),
        Err(ProjectError::LimitExceeded("assets"))
    ));

    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(&[0; 16], 2, 2, image::ExtendedColorType::Rgba8)
        .unwrap();
    let limits = UnderlayLimits {
        max_width: 1,
        max_height: 1,
        max_pixels: 1,
        ..Default::default()
    };
    assert_eq!(
        decode_underlay(&png, limits).unwrap_err(),
        UnderlayError::Dimensions
    );
}
