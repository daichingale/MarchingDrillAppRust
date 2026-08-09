# DrillForge requirement evidence matrix

Last audited: 2026-08-10. Normative sources are `PRODUCT_QUALITY.md` and
`docs/design/00-conventions.md` through `90-integration-roadmap.md`. This file records
current evidence; it does not weaken those requirements.

Image underlay persistence (schema v4): `ImageUnderlay` and
`UnderlayPlacement` are validated and Undoable; optional `.drillproj`
`AssetKind::Image` entries are BLAKE3 content-addressed and may be embedded or
external. Missing bytes degrade non-fatally. The 2D editor supports placement,
scale, rotation, opacity and visibility; the default policy explicitly excludes
the reference image from exports, while the opt-in `EditorAnd2dExport` policy
composites it deterministically into SVG, PDF and 2D video. Stadium/3D export
always excludes it. See
`docs/adr/0002-image-underlay-assets.md`.

Status meanings:

- **Proved**: implementation exists and a directly relevant automated check passes.
- **Partial**: useful implementation exists, but the normative scope or proof is incomplete.
- **Missing**: the required implementation is absent.
- **External proof**: the implementation can be ready locally, but certification, credentials,
  hardware coverage, deployment, or human evaluation cannot be proved by repository tests.

## Product quality gates

| Requirement | Status | Current implementation evidence | Direct verification / missing proof |
|---|---|---|---|
| Exact count playback, set boundaries, variable BPM, partial loops | Proved | `drill-core/src/playback.rs`, `tempo.rs` | Unit/property tests cover snapping, boundaries and f64 round trips. |
| Schema validation, v1 migration, future/corrupt rejection | Proved | `Document::from_json`, schema v2 migration | Core fixtures, mutation corpus and cross-crate conformance tests. |
| Stable-ID Undo/Redo and document invariants | Proved for in-document actions | `Edit`, `History`, stable IDs in `crates/drill-core/src/lib.rs`; `crates/drill-app/src/ui_qa.rs` source guard and atomic stale-edit tests | 10,000-edit algebra and Undo/Redo round-trip tests pass. Project loading intentionally resets history; non-document UI state is not represented as `Edit`. |
| One coordinate/render source for screen, SVG, PDF and video | Proved | `drill-render::DisplayList`; egui/SVG/PDF/raster consumers | Render/export parity and deterministic tests. Legacy text/HTML serializers are compatibility output, not rendering backends. |
| 1,000-performer interpolation without steady allocation | Proved | reusable `TransitionPlan`, render scratch buffers | Allocation-counting release benches. |
| 1,000 performers within 16.6 ms | Proved on development baseline | core/render/GPU benches | Bench gates pass locally; representative GPU/driver/OS coverage remains external proof. |
| Two-hour playback without continual memory growth | Partial | bounded ring buffers and reusable rate/output buffers in `drill-audio` | Two-hour device-independent hot path has zero steady allocation. Multi-hour real-device RSS/device recovery matrix remains external proof. |
| Save/autosave/analysis do not stall rendering | Proved for local worker/caller boundary | `drill-jobs`; project, audio, import and export jobs; `crates/drill-jobs/benches/max_project_save_latency.rs` | Release gate concurrently performs atomic project save and recovery autosave for 4,000 performers × 240 sets (960,000 coordinates, 61.8 MB JSON) plus 3,500 embedded entries. Caller-thread identity guard and 600-frame poll require p95 ≤2 ms, p99 ≤4 ms, worst ≤8 ms. Latest run: 100 ns / 100 ns / 4.6 µs and 208,229,217-byte peak heap growth against a 768 MiB ceiling. OneDrive/network filesystem behaviour remains external proof. |
| Backup, autosave, crash and missing-asset recovery | Proved | `drill-project`, `project_state.rs` | Atomic replacement, recovery, container-limit and crash tests. |
| Discoverable first-run workflow | Partial | labelled toolbar, menus, onboarding, command palette, timeline guidance | Headless onboarding/edit/play smoke exists. Observational usability testing is external proof. |
| Keyboard, menu, non-colour status and platform accessibility bridge | Partial | command table; `eframe` `accesskit` feature; semantic UI tests | AccessKit tree/keyboard tests pass. Screen-reader and Windows UI Automation certification are external proof. |
| Windows 100–200% and Japanese/English presentation | Partial | scale-aware timeline/layout tests and QA screenshots; complete keyed JA/EN UI catalog with raw-literal source guard | Logical-scale tests are not true multi-monitor DPI transitions; physical multi-monitor transitions remain external QA. |

## Roadmap implementation matrix

| Normative scope | Status | Evidence | Remaining internal work |
|---|---|---|---|
| Wave 0: errors, stable IDs, Edit algebra, revisions, schema, atomic save, CI | Complete internally | `crates/drill-core`, `crates/drill-project`, `crates/drill-jobs/src/lib.rs`, `crates/drill-jobs/tests/ui_latency_gate.rs`, `crates/drill-app/src/i18n.rs`, `crates/drill-app/src/i18n_generated.rs`, `scripts/test-message-catalog.ps1`, `.gitattributes`, CI; typed `SnapshotStateError`, `UnderlayFailure`, `TextExportError`, `VideoConfigError`, `JobFailure`, `StatusMessage` | Worker jobs expose only `spawn_typed` and stable `JobErrorCode` values. UI state retains catalog IDs plus bounded arguments and resolves them at the locale-aware display boundary. Catalog CI rejects missing JA/EN text, Japanese glyphs in English entries, duplicate/missing IDs, reviewed multi-pair semantic swaps, and unregistered raw UI literals; the locale-neutral allowlist is explicit and reasoned. |
| Routes, gates, holds, easing and per-performer overrides | Proved | `drill-core/src/transition.rs`; persisted `RouteTable`; allocation-free `TransitionPlan` | — |
| Step style, swept collision, stride/turn/arrival clinic | Proved for implemented clinic model | `clinic.rs`, `continuity.rs`, route suggestions | Field validation by professional drill designers remains external proof. |
| Sections, subsets, snapshots and three-way branches | Proved for local desktop model | roster/snapshot core modules and app workflow | Multi-user synchronization is explicitly outside current desktop scope. |
| DisplayList, CPU raster, PDF, SVG and deterministic video | Proved | `drill-render`, `drill-export`; FFmpeg E2E harness | Tagged CI requires real FFmpeg tools. FFmpeg is intentionally not bundled pending licensing decision. |
| Audio decode, peaks, device output, click, anchors | Proved for supported code paths | `drill-audio`, app audio state | Hardware/driver endurance matrix is external proof. |
| GPU 2D and 3D with true depth | Proved for renderer core | `drill-gpu`; offscreen colour + `Depth32Float`; CPU fallback; `renderer_parity.rs` | CPU reference/prepared-data/WGSL policy parity is covered. Cross-vendor physical pixel evidence remains external. |
| Camera program and Real View | Proved for repository-defined desktop model | persisted camera tracks/cuts; `crates/drill-core/src/camera.rs`, `stadium.rs`, `visibility.rs`; `crates/drill-app/src/stadium_inspector.rs`; CPU/GPU 3D renderers | Seat occlusion, stable performer IDs, row straight/arc flatness, stands/press box, weather lighting, shadows/logo data and three-level LOD have automated tests. Cross-vendor pixels and professional venue validation remain external proof. |
| Formation designer | Proved for adopted doc-14 tool set | persisted `ShapeSpec` and sampling/fitting/morph/assignment in `crates/drill-core/src/shapes.rs`; preview/apply/free-draw/text-outline/UI in `crates/drill-app/src/app_state.rs`, `workspace_inspector.rs`, `ui_qa.rs` | Line/arc/block/circle/spiral/Bézier/polyline/ellipse/parabola/sine/star/polygon/cross/text/free-path, fit-line/circle, radial symmetry, section-contiguous assignment and single-Undo apply are covered. Font-outline appearance and expert workflow acceptance remain external proof. |
| Coordinate notation and production reports | Proved for repository-defined variants; professional print acceptance is external | persisted typed notation for 1/8–whole-step precision, 5/10/custom yard-line intervals, nearest/fixed custom references, explicit/short and 8-to-5/6-to-5 styles; Standard/Compact/Rehearsal Production presets; shared screen/CSV/TSV/HTML/PDF formatting and Undo UI | JSON legacy-default/roundtrip, locale, shared-output and preset-table tests cover the internal contract. Professional organizations' house templates and print-shop acceptance remain external validation. |
| Constraint route rewrite | Proved for declared deterministic constraints | route suggestions plus bounded projection solver in `crates/drill-core/src/constraint_solver.rs`; app preview/apply in `crates/drill-app/src/app_state.rs` | Fixed point, field bounds, minimum spacing, maximum step and shape-follow constraints have validation, cancellation, iteration/work limits and deterministic tests. It does not claim global optimality or solve arbitrary nonlinear constraints. |
| Interop and public plugin API | Proved for the documented open formats and isolated plugin boundary | CSV/TSV/TXT numeric and bounded human-coordinate mapping/diff (`coordinate_phrase.rs`), with explicit ambiguity diagnostics; MIDI/`.musicxml`/`.mxl` timeline import in `crates/drill-interop/src/musical.rs`; XLSX sheet selection in `xlsx.rs`; bounded PNG/JPEG decode in `underlay.rs`; schema-v4 content-addressed `.drillproj` underlay assets; deterministic isolated plugin protocol | Human phrases cover the documented Japanese/English yard-line, hash and sideline grammar and never guess a missing field side. Underlays support embedded/external recovery, placement Undo and opt-in 2D SVG/PDF/video composition; 3D exclusion is explicit. Proprietary Pyware parsing remains intentionally excluded without a lawful published specification. |
| Updater | Partial | HTTPS manifest validation and update UI | A configured, deployed, signed manifest endpoint is required. A hostname literal without deployed DNS/content is not release evidence. |
| Windows distribution | Partial | portable ZIP/MSIX scripts, identity/signing guards, static MSIX preflight | Real publisher identity, signature trust, WACK result and Store validation are external proof. |
| macOS distribution | Partial | bundle/package script, plist and entitlements | macOS runner artifact proof plus Apple signing/notarization/stapling are required; credentials and notarization are external proof. |
| Security and supply-chain | Proved for repository/offline policy; live advisory state is external | bounded parsers, deterministic complexity/DoS conformance for JSON/CSV/MIDI/MusicXML/XLSX/container/underlay, reviewed native-interop unsafe allowlist, locked builds, `deny.toml`, cargo-deny CI, deterministic 465-package license inventory, release provenance and packaged license files | `cargo test -p drill-conformance --test complexity_dos --locked`; `pwsh ./scripts/test-supply-chain.ps1`; CI `supply-chain` job; `THIRD_PARTY_LICENSES.md`. Local offline validation passes; a current RustSec database requires the networked CI run. |
| CPU/GPU rendering parity | Proved for CPU/prepared-data/WGSL contract; physical adapters are external | table-driven 2D/3D parity across DPI/viewport, performer kinds, all camera presets, exact LOD boundaries and lighting presets; WGSL syntax and policy contract | `cargo test -p drill-gpu --test renderer_parity --locked`; CI and nightly parity steps. CPU reference and GPU-prepared data are compared within 0.002 px; physical driver pixels remain a release-lab gate. |
| Test/CI matrix | Partial | Windows/Linux/macOS CI, mutation corpus, conformance, UI smoke, benches, deterministic complexity gate, maximum-project save/UI latency, CPU/GPU parity and reviewed product goldens | Golden SVG/CSV/HTML/Production TSV plus PDF/RGBA/catalog digests are check-only in `golden_approval.rs`; updates require `scripts/update-goldens.ps1 -Approve reviewed`, are forbidden in CI and produce bounded first-difference output. Logical-operation DoS regression is covered by `complexity_dos.rs`; maximum save/autosave latency and heap by `max_project_save_latency.rs`; shader/data parity by `renderer_parity.rs`. Coverage-guided fuzzing, real DPI/AT and broader hardware matrices remain incomplete. |

## External proof ledger

These are not internal coding gaps and must not be marked complete from mocks, static package
inspection, or headless unit tests.

| Proof required | Repository readiness | Evidence still required outside this workspace |
|---|---|---|
| Windows distribution trust | MSIX identity/version guards, PFX signing path and static package validation exist in `scripts/package-msix.ps1`, `sign-msix.ps1`, `test-msix.ps1` | Production publisher certificate/HSM signing, trusted Authenticode result, WACK pass and Store ingestion. |
| macOS distribution trust | Universal bundle/DMG, hardened-runtime signing and notarization scripts exist under `scripts/package-macos.sh` and `scripts/notarize-macos.sh` | Apple Developer ID credentials, successful notarization/stapling and install/launch proof on supported Intel/Apple Silicon systems. |
| Update delivery | Compile-time HTTPS endpoint configuration and signed-manifest verification exist in `crates/drill-updater` and tagged release guards | Deployed DNS/TLS endpoint, published signed manifests/artifacts, rollback drill and production update telemetry. |
| GPU rendering | Deterministic CPU/prepared-data/WGSL parity is a CI gate | Captured output on supported Intel/AMD/NVIDIA/Apple driver matrix, including device-loss recovery and centroid/colour comparison. |
| Audio endurance | Device-independent two-hour hot-path allocation gate passes | Multi-hour RSS, underrun, unplug/replug and sample-rate/device matrix on real audio hardware. |
| Accessibility and DPI | AccessKit semantics, keyboard flows and logical 100/150/200% tests pass | NVDA/Narrator/VoiceOver audit, Windows UIA inspection and mixed-DPI multi-monitor transitions. |
| Product usability and domain accuracy | Onboarding, menus, shortcuts, formation/clinic/visibility workflows and localized guidance exist | Observed first-run and expert drill-designer studies; validation of coordinate/report/venue conventions against production use. |
| Supply-chain freshness | Offline lock/inventory/unsafe/source checks and cargo-deny CI configuration exist | A successful networked cargo-deny run against a current RustSec advisory database for the release commit. |

## Historical defect ledger (`DESIGN_GAPS.md` section 0)

The original 16 defects are retained there as history. Current disposition:

| Items | Disposition and evidence |
|---|---|
| #1, #2, #5 | Resolved by stable-ID `Edit`/`History` using `VecDeque`; duplicate-set and history tests. |
| #3, #4 | Resolved by revision-gated swept clinic and reusable scratch; clinic benchmark. |
| #6, #7 | Resolved structurally by shared `DisplayList`/field mapping across screen and exports. |
| #8 | Resolved for Bézier and spiral by bounded dense sampling plus polyline arc-length redistribution; direct spacing tests cover both. |
| #9, #10 | Resolved by background atomic project save/replacement; OneDrive behavior still needs an OS integration test. |
| #11, #12 | Resolved by reusable transition buffers and guarded empty/invalid document paths. |
| #13 | Resolved by discovered absolute FFmpeg/ffprobe paths and restricted child environment. |
| #14, #15 | Resolved by repository `.gitignore`/`.gitattributes`. |
| #16 | Resolved in build configuration (`eframe/accesskit`) and semantic-tree tests; real assistive-technology proof remains external. |
| L1, L2 | Resolved by in-app notices plus OFL/MIT/Apache/license files in release packaging. |
| L3 | FFmpeg remains external and is not bundled. Any future bundling requires a fresh license determination. |

## Strict internal closure order

1. Finish the remaining interop scope: human coordinate-phrase import and persisted underlay transform/visibility policy across project, 2D/3D and exports.
2. Expand coverage-guided fuzzing and maintain the reviewed catalog/neutral-token manifests as UI surfaces grow.
3. Run the deterministic renderer parity suite on the supported physical GPU/driver matrix and archive image/centroid artifacts; retain the CPU/WGSL contract gate in ordinary CI.
4. Configure and deploy the signed update manifest endpoint; the build deliberately reports “not configured” without release endpoint settings.
5. Collect external real-device audio endurance, multi-monitor DPI, screen-reader/UIA, expert workflow, Windows signing/WACK/Store and Apple signing/notarization evidence.

The project is **not a proved general release** until every Partial/Missing row above is either
completed or explicitly removed from the normative designs by an owner decision. External proof
must never be replaced by a unit test or a generated-but-unsigned package.
