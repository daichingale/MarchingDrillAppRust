use std::path::{Path, PathBuf};

const TARGETS: &[&str] = &[
    "document_json",
    "drillproj_container",
    "csv",
    "xlsx",
    "midi",
    "mxl",
    "underlay",
    "plugin_protocol",
];

fn fuzz_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz")
}

#[test]
fn fuzz_workspace_inventory_is_complete_and_isolated() {
    let root = fuzz_root();
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert!(manifest.contains("[workspace]"));
    assert!(manifest.contains("cargo-fuzz = true"));
    assert!(root.join("dictionaries/drillforge.dict").is_file());
    for target in TARGETS {
        assert!(
            root.join(format!("fuzz_targets/{target}.rs")).is_file(),
            "missing target {target}"
        );
        let corpus = root.join("corpus").join(target);
        assert!(
            corpus.read_dir().unwrap().next().is_some(),
            "empty corpus {target}"
        );
    }
}

#[test]
fn seed_corpus_replays_through_bounded_production_parsers() {
    let root = fuzz_root().join("corpus");
    for target in TARGETS {
        for entry in std::fs::read_dir(root.join(target)).unwrap() {
            let bytes = std::fs::read(entry.unwrap().path()).unwrap();
            replay(target, &bytes);
        }
    }
}

fn replay(target: &str, bytes: &[u8]) {
    match target {
        "document_json" => {
            if let Ok(text) = std::str::from_utf8(bytes) {
                let _ = drill_core::Document::from_json(text);
            }
        }
        "drillproj_container" => {
            let path = std::env::temp_dir()
                .join(format!("drillforge-seed-{}.drillproj", std::process::id()));
            std::fs::write(&path, bytes).unwrap();
            let _ = drill_project::container::load(&path);
            let _ = std::fs::remove_file(path);
        }
        "csv" => {
            let _ = drill_interop::sniff(
                bytes,
                &drill_interop::ImportLimits {
                    max_bytes: 1_048_576,
                    ..Default::default()
                },
            );
        }
        "xlsx" => {
            let _ = drill_interop::xlsx::inspect_xlsx(
                bytes,
                &drill_interop::ImportLimits {
                    max_bytes: 2_097_152,
                    ..Default::default()
                },
            );
        }
        "midi" => {
            let _ = drill_interop::musical::import_midi(bytes, &Default::default());
        }
        "mxl" => {
            let _ = drill_interop::musical::import_musicxml(bytes, &Default::default());
        }
        "underlay" => {
            let _ = drill_interop::underlay::decode_underlay(bytes, Default::default());
        }
        "plugin_protocol" => {
            let _ = drill_plugin::decode_command(bytes, drill_plugin::PluginLimits::DEFAULT);
        }
        _ => unreachable!(),
    }
}
