# Pyware 3D / 3DX workflow comparison (independent-product audit)

This audit is a research/checklist for **DrillForge**, not a reverse engineered
file parser, trademark affiliation, or a declaration of feature parity.

Evidence checked on 2026-10-23:

- Official Pyware 3D feature overview: https://www.pyware.com/3d/
- Official 3DX capabilities and editions: https://www.pyware.com/3d-pricing/
- Pyware 3DX product description (mobile/desktop): https://apps.apple.com/jp/app/pyware-3dx/id6449265513
- DrillForge source audit: `crates/drill-core`, `crates/drill-app`,
  `crates/drill-interop`, `crates/drill-mobile-viewer`, plus
  `docs/IMPLEMENTATION_EVIDENCE.md` (historical 2026-08-10 snapshot).

Product claims below are an **implementation inventory**, not empirical proof
that DrillForge produces the same motion as Pyware. Each discrepancy requires
an independently created reference drill and designer review.

| Workflow requirement | Pyware evidence | DrillForge implementation evidence | Gap / verification needed |
| --- | --- | --- | --- |
| Line, block, arc, freehand, spline formations | 3D/3DX official tools | `drill-core/shapes.rs`; formation designer UI | Compare point-order consistency, interval and curve handles |
| Morph, mirror, resize, warp formations | 3DX product description | Shape morph/assignment, editing tools | Verify adjacent-set continuity, pivot choices, reshape ergonomics |
| Count track, range edits, audio sync | 3D and 3DX product overview | `drill-core/playback.rs`, tempo/audio modules and timeline | Test partial-range edits, tempo-change boundaries and seek behavior |
| Path and gate editing, sequenced forms | 3DX feature description | `drill-core/transition.rs`, `pathing.rs` | Test one-performer and multi-shape sequence, gates/holds and IDs |
| Stride, collision, direction clinic | 3D overview | `clinic.rs`, constraint solver, route suggestions | Validate real performer clearances, instrument constraints and defaults |
| Interactive reference / dress points | 3D and 3DX descriptions | Coordinate and field-marking tools | Add ergonomic live multi-performer distance/reference tools |
| 3D, venue, uniforms, props | 3D/3DX features | Stadium/camera/visibility and 2D/3D renderers | Costume/prop authoring, flag cloth and real equipment are NOT parity-proved |
| Production sheets, coordinate charts, PDF | 3D/3DX features | `production.rs`, `coordinates.rs`, `drill-export` | Compare organization-specific print conventions and accuracy |
| Cast list import, merge and transfer | 3DX description | Section/roster models, CSV/XLSX import | Build detailed merge/diff workflow and duplicate-ID handling |
| Mobile viewing and shared work | 3DX mobile support | Mobile viewer / sync / relay crates | Cross-device acceptance, offline reconciliation, access controls |
| Per-count trajectory export for selected performers | DrillForge differentiation | `trajectory_export.rs` + timeline export | Confirm field-unit precision, phase names and user workflows |
| Proprietary Pyware project read/write | No published parsing specification accepted here | Deliberately NOT supported | Seek lawful supported interchange; no binary format assumptions |

## Acceptance-drill suite to build from scratch

1. **Simple set transition.** 8 people; 16 counts; 8-to-5 reference;
   compare starting positions, end positions, labels and exact count boundary.
2. **Split movement.** One section holds four counts, another departs on count
   one, both arrive by count 12. Check playback samples and printable routes.
3. **Curve and crossing.** 24 people exchange ranks via curved paths and
   changed assignments; verify no unwanted performer identity swaps.
4. **Indoor gym.** 30m x 30m boundary in a meter-based canvas; evaluate
   correct separation of `5 meters` and `5 yards` step conventions.
5. **Document survival.** Save, reload, undo, redo and print a complex project;
   verify all positions and count/route metadata are retained.
6. **Large show.** 300 performers / 128 sets with waveform, fast scrubbing,
   print, video and mobile preview; measure on real Windows/AMD hardware.

Each comparison records input conditions, screenshots created from original
test data, counts/coordinates, desired behavior, actual behavior, and outcome
(PASS/FAIL/INCONCLUSIVE). Do not use vendor-provided assets in DrillForge or
copy Pyware's source or proprietary UI artwork.

## Roadmap for actual compatibility

1. Treat official user-exported **cast lists, production sheets and coordinate
   tables** as imports and use the existing `drill-interop` adapters.
2. Offer column mapping, locale/decimal separator detection and field-unit
   sanity checks. Never silently guess missing side/hash orientation.
3. Show a diff/preview before applying imported data with a single Undoable
   `Edit` batch.
4. Support direct proprietary project files only if documented permissions or
   legitimate public format details establish that this is lawful and practical.
   Maintain read-only originals and lossless round-trip tests wherever possible.

Priority is precision and **operator time saved**, not recreating UI chrome.
