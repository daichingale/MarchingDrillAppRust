//! Deterministic hostile-input/property runner.
//!
//! This is intentionally dependency-free and runs in ordinary stable CI.  The
//! same seed always produces the same failing case; `DRILLFORGE_MUTATION_CASES`
//! raises the bounded nightly workload without slowing the edit loop.

use drill_core::audio::{AnchorMap, SyncAnchor};
use drill_core::tempo::{TempoChange, TempoMap};
use drill_core::{ChordPoint, Document, Easing, Gate, PathVia, Point, Route, RouteShape};
use drill_interop::{ImportLimits, sniff};
use drill_project::container::{SaveProject, encode, load};
use std::collections::BTreeMap;
use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

const DEFAULT_CASES: usize = 256;
const MAX_CASES: usize = 65_536;
const MAX_MUTATED_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy)]
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn index(&mut self, upper: usize) -> usize {
        if upper == 0 {
            0
        } else {
            self.next() as usize % upper
        }
    }
}

fn case_count() -> usize {
    std::env::var("DRILLFORGE_MUTATION_CASES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_CASES)
        .clamp(1, MAX_CASES)
}

fn mutated(seed: &[u8], ordinal: usize) -> Vec<u8> {
    let mut rng = Rng(0xd1_11_f0_12_9e_37_79_b9 ^ ordinal as u64);
    let mut out = seed.to_vec();
    for _ in 0..(1 + rng.index(12)) {
        match rng.index(5) {
            0 if !out.is_empty() => {
                let at = rng.index(out.len());
                out[at] ^= (rng.next() | 1) as u8;
            }
            1 if !out.is_empty() => {
                out.remove(rng.index(out.len()));
            }
            2 if out.len() < MAX_MUTATED_BYTES => {
                let at = rng.index(out.len() + 1);
                out.insert(at, rng.next() as u8);
            }
            3 if !out.is_empty() && out.len() < MAX_MUTATED_BYTES => {
                let start = rng.index(out.len());
                let take = (1 + rng.index(32)).min(out.len() - start);
                let room = MAX_MUTATED_BYTES - out.len();
                let copy = out[start..start + take.min(room)].to_vec();
                out.extend_from_slice(&copy);
            }
            _ => out.truncate(rng.index(out.len() + 1)),
        }
    }
    out
}

fn assert_original_unchanged(actual: &[u8], expected: &[u8], domain: &str, case: usize) {
    assert_eq!(
        actual, expected,
        "{domain} case {case} mutated its source corpus"
    );
}

#[test]
fn json_mutation_corpus_is_bounded_and_never_panics() {
    let seed = Document::demo(16, 4).to_json().unwrap().into_bytes();
    let pristine = seed.clone();
    for case in 0..case_count() {
        let input = mutated(&seed, case);
        assert!(input.len() <= MAX_MUTATED_BYTES);
        let result = catch_unwind(AssertUnwindSafe(|| {
            if let Ok(text) = std::str::from_utf8(&input)
                && let Ok(document) = Document::from_json(text)
            {
                document.validate().expect("accepted JSON must validate");
                let encoded = document.to_json().expect("accepted JSON must serialize");
                Document::from_json(&encoded).expect("accepted JSON must round-trip");
            }
        }));
        assert!(
            result.is_ok(),
            "JSON parser panicked for deterministic case {case}"
        );
        assert_original_unchanged(&seed, &pristine, "JSON", case);
    }
}

fn temporary_project_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "drillforge-mutation-{}-{}.drillproj",
        std::process::id(),
        std::thread::current().name().unwrap_or("worker")
    ))
}

#[test]
fn container_mutation_corpus_is_bounded_and_never_panics() {
    let document = Document::demo(8, 3);
    let seed = encode(&SaveProject {
        document: &document,
        app_version: "mutation-test",
        created_utc: "2000-01-01T00:00:00Z",
        modified_utc: "2000-01-01T00:00:00Z",
        embedded: &BTreeMap::new(),
        assets: &[],
    })
    .unwrap();
    let pristine = seed.clone();
    let path = temporary_project_path();
    for case in 0..case_count().min(2_048) {
        let input = mutated(&seed, case);
        assert!(input.len() <= MAX_MUTATED_BYTES);
        fs::write(&path, &input).unwrap();
        let result = catch_unwind(AssertUnwindSafe(|| load(&path)));
        assert!(
            result.is_ok(),
            "container loader panicked for deterministic case {case}"
        );
        if let Ok(loaded) = result.unwrap() {
            loaded
                .document
                .validate()
                .expect("accepted container must validate");
        }
        assert_original_unchanged(&seed, &pristine, "container", case);
    }
    let _ = fs::remove_file(path);
}

#[test]
fn csv_mutation_corpus_respects_limits_and_never_panics() {
    let seed = b"performer,set,x,y,counts\nA,One,1,2,8\nB,One,3,4,8\n".to_vec();
    let pristine = seed.clone();
    let limits = ImportLimits {
        max_bytes: 4096,
        max_rows: 128,
        max_columns: 32,
        max_field_bytes: 256,
        ..ImportLimits::default()
    };
    for case in 0..case_count() {
        let input = mutated(&seed, case);
        let result = catch_unwind(AssertUnwindSafe(|| sniff(&input, &limits)));
        assert!(
            result.is_ok(),
            "CSV sniffer panicked for deterministic case {case}"
        );
        if let Ok(preview) = result.unwrap() {
            assert!(preview.total_rows <= limits.max_rows);
            assert!(preview.headers.len() <= limits.max_columns);
        }
        assert_original_unchanged(&seed, &pristine, "CSV", case);
    }
}

#[test]
fn tempo_route_and_anchor_properties_hold_for_generated_corpus() {
    let mut rng = Rng(0x50_51_c0_ff_ee);
    for case in 0..case_count() {
        let bpm = 30.0 + (rng.next() % 27000) as f32 / 100.0;
        let split = 1.0 + (rng.next() % 204_700) as f32 / 100.0;
        let tempo = TempoMap::from_changes([
            TempoChange { count: 0.0, bpm },
            TempoChange {
                count: split,
                bpm: bpm * 0.75 + 1.0,
            },
        ]);
        let count = (rng.next() % 204_800) as f64 / 100.0;
        let seconds = tempo.seconds_at_f64(count);
        let back = tempo.count_at_f64(seconds);
        assert!(seconds.is_finite() && back.is_finite());
        assert!(
            (back - count).abs() <= 1e-7 * count.max(1.0),
            "tempo case {case}"
        );

        let moves = 1 + (rng.next() % 512) as u16;
        let route = Route {
            gate: Gate {
                depart: (rng.next() % moves as u64) as f32,
                arrive: Some((rng.next() % (moves as u64 + 1)) as f32),
            },
            easing: match rng.index(4) {
                0 => Easing::Linear,
                1 => Easing::Smooth,
                2 => Easing::EaseIn,
                _ => Easing::EaseOut,
            },
            shape: match rng.index(4) {
                0 => RouteShape::Straight,
                1 => RouteShape::Curve {
                    control: ChordPoint {
                        along: 0.5,
                        lateral: (rng.next() % 200) as f32 / 100.0 - 1.0,
                    },
                },
                2 => RouteShape::Arc {
                    bulge: (rng.next() % 200) as f32 / 100.0 - 1.0,
                },
                _ => RouteShape::Path {
                    via: PathVia::Absolute(vec![Point { x: 10.0, y: 20.0 }]),
                },
            },
        };
        for local in [0.0, moves as f32 * 0.5, moves as f32] {
            let point = drill_core::transition::evaluate(
                &route,
                Point { x: 0.0, y: 0.0 },
                Point { x: 40.0, y: 80.0 },
                local,
                moves,
            );
            assert!(
                point.x.is_finite() && point.y.is_finite(),
                "route case {case}"
            );
        }

        let delta = 0.1 + (rng.next() % 100_000) as f64 / 1000.0;
        let anchors = AnchorMap::try_from_anchors([
            SyncAnchor {
                count: 0.0,
                seconds: 0.0,
            },
            SyncAnchor {
                count: delta,
                seconds: delta * 60.0 / bpm as f64,
            },
        ])
        .expect("monotonic generated anchors");
        anchors.validate().expect("accepted anchors remain valid");
        assert_eq!(anchors.anchors().len(), 2);
    }
}
