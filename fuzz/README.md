# DrillForge fuzzing

This is an intentionally independent Cargo workspace, so ordinary workspace
checks and offline builds do not resolve `libfuzzer-sys`.

Install nightly Rust and `cargo-fuzz`, then run a bounded target from this
directory:

```text
cargo +nightly fuzz run document_json -- -max_len=1048576 -timeout=5
cargo +nightly fuzz run drillproj_container -- -max_len=2097152 -timeout=10
```

Targets cover Document JSON, `.drillproj`, CSV, XLSX, MIDI, MusicXML/MXL,
image underlays, and the plugin command protocol. Every target checks length
before entering the production parser and production parsers retain their own
row, entry, pixel, event, and allocation limits.

Seed files live in `corpus/<target>/`; syntax tokens live in
`dictionaries/drillforge.dict`. Crashes and timeouts are written by cargo-fuzz
under `artifacts/` and must be minimized before being promoted to a regression
test. Never commit user documents or media as corpus.

Without cargo-fuzz, `cargo test -p drill-conformance --test fuzz_layout` proves
the isolated workspace, complete target/corpus inventory, dictionary, and
replays every seed through the corresponding bounded production parser.
