# ADR-0003: Transactional editing, time-aware motion and collaboration boundaries
**Status:** PROPOSED (implementation not authorized until reviewed)
**Date:** 2026-10-23
**Related:** [Master Plan v2](../MASTER_PLAN_NEXTGEN_2026.md), [UX](../product/INTERACTION_SPEC_NEXTGEN_2026.md), [Feature Backlog](../product/FEATURE_BACKLOG_NEXTGEN_2026.md)  
**Normative owners:** [Document/Edit model](../design/10-document-model.md), [transition model](../design/11-transition-model.md), [security](../design/51-security.md), [interop](../design/52-interop-plugins.md)

## Context

DrillForge's current engine correctly stores performers with stable IDs and transitions with \`RouteTable\`, \`Gate\`, \`Easing\` and \`SetCounts\`. However the UX is moving from simple position edits toward complex, multi-performer tools, AI proposals, stage visuals and coediting. A remote replica currently merges per-dot LWW values. If an operation modifies many points, accepting only some of its dot updates is **not the same operation** the designer approved. A visual preview can also become stale while the document changes.

The design set (documents 10/11/15/43) includes old current-state sections that predate the implemented engine. This ADR must be reconciled with the live \`crates/\` before any schema/API code is changed.

## Decision requested

### 1. Separate proposals from mutations

Introduce an **application-level** \`EditProposal\` / \`PreviewResult\` contract (conceptual names only), with:
- immutable source document revision/hash and relevant set/performer IDs;
- exact time range and unit;
- proposed typed edits and declared unchanged parts;
- bounded validation (ranges, unknown IDs, finite coordinates, geometry, obstacle/collision options);
- warnings distinguished from blocking errors;
- computed visual diff, before/after metrics, and cancellation token.

All entry points—drag, transform, import, solver, AI—feed that contract. Only the existing trusted Edit/History implementation applies the accepted \`EditBatch\`. On stale revisions, **reject and re-preview**, never force overwrite without confirmation. Store a stable change summary, not arbitrary model instructions. A cancel means zero document mutation and zero history changes.

### 2. Preserve existing document model and prove a new time axis before adding fields

\`ShowTime\` is a conceptual pair \`{count: u32, subdivision: u8/ratio}\` with bounded resolution (initial candidate: eighth counts). Avoid float equality in interval ordering. \`TempoMap\` maps musical counts to seconds via f64; \`Route\` stays f32 for local geometry as specified by doc 11.

**Do not immediately change \`Document\` schema.** First run a prototype derived view of:
- physical position and active route from current \`Set\`;
- facing/orientation track independent of heading;
- optional equipment/gesture track attached to stable \`PerformerId\`;
- props, lights, camera, annotation tied to explicit time windows.

Then propose dedicated backward-compatible fields, \`Edit\` variants, \`validate\`, JSON migrations, export and mobile behavior. A missing future track should mean \`None\`, not "invent a movement".

### 3. Distinguish stream updates and transactional coediting

Current \`drill-sync\` Lamport+LWW is enough for independent atomic individual dot moves and good for cheap previews. Preserve it for those scopes, and **do not represent a multi-performer formation transformation as unrelated accepted dot writes**.

A candidate \`RoomEditBatch\` includes:
- \`base_revision\`, \`actor\`, \`affected_ids\`, \`set_ids\`, \`time_range\`, \`operation_id\`, \`serialized_intent\`;
- all-or-nothing validation at the authoritative merge boundary;
- conflict reporting when another accepted operation changed an overlapping scope;
- idempotency when the network retries;
- local undo as a new **compensating** operation, not erasure of someone else's edits.

A relay can remain lightweight but must not claim full merge semantics until this behavior is proven. A more complex CRDT is **not adopted by default**; adopt only if experiments show simpler atomic batches are insufficient.

### 4. AI, external import and plugins do not acquire write authority

- AI's output is untrusted typed candidate geometry, not a runnable script and not an edit applied directly.
- Importers parse bounded, public/user-exported formats; map columns and surface ambiguity instead of silently filling unknown data.
- Plugins remain sandboxed, require declared capabilities and budgets, and cannot access files/network or data not explicitly granted.
- All these routes use the same \`PreviewResult → validate → EditBatch\` gateway.
- Privacy defaults to local and pseudonymous. Online connection and external AI requests are opt-in.
- No proprietary Pyware binary decoding or asset copying is authorized by this ADR.

## Alternatives considered

| Alternative | Pros | Why not now |
|---|---|---|
| Each tool writes directly to Document as drag proceeds | Easy initial implementation | Partial results, hard Undo, stale async candidates |
| Put AI/solver commands directly into \`Edit\` | Fewer layers | Untrusted or nondeterministic mutations cannot be reviewed |
| Use CRDT for every entity immediately | Generic collaboration | Large dependency and complexity for single-designer-first workflow |
| Only use current per-dot LWW for whole tools | Smallest patch | Team can receive half a formation transformation |
| Replace all existing \`Set\`/Route with a new scene graph | Flexible stage semantics | Breaks saves, undo and current rendering proof |
| Store positions/route/facing/equipment in one giant per-count frame | Simple playback | Explodes project size, destroys editability, expensive sync |
| Keep all changes private until full v3 schema | Zero migration work today | Blocks safe prototyping and useful UX improvement |

## Consequences / compatibility budget

1. No change to \`SCHEMA_VERSION\` until a separate migration ADR and old-version fixtures are approved.
2. \`PerformerId\` and \`SetId\` remain stable; **UI selection indices must not become persistent references**.
3. Every candidate may be canceled; every accepted candidate has one complete Undo/Redo meaning.
4. Sync must eventually serialize validated commands with conflict reporting; old clients may stay view-only if unable to understand a new operation.
5. Projection/coordinate units are never stored implicitly in new tracks; refer to document grid and typed unit conversions.
6. Asset, PDF, video, mobile and collaboration consumers must not guess semantics for a future track; strict fallback is explicit/optional.
7. A feature is not released merely because CI compiles; domain and real-device checks remain mandatory.

## Verification gates before adoption

- **T-01**: EditProposal cancels without changing Document hash or History length.
- **T-02**: Two concurrent proposals against same dot cause explicit rebase/conflict rather than silent overwrite.
- **T-03**: A 24-person morph is all-or-nothing across two clients; after reconnect it cannot appear as 12 old + 12 new coordinates.
- **T-04**: Copy/paste across a move+hold boundary is reversible and does not drop a post-hold animation.
- **T-05**: An outdated AI proposal cannot be applied after changing the underlying set.
- **T-06**: A user without edit privilege cannot issue a document mutation through a relay or viewer.
- **T-07**: An eighth-count facing event is stable through \`TempoMap\`, persistence and 2D/3D/CSV export; no off-by-one at set boundary.
- **T-08**: A v5 \`.drillproj\` opens unchanged after a future schema migration, or fails with a loss-preventing explicit error.
- **T-09**: 1000-performer preview/commit/Undo stays within published latency/memory budgets.
- **T-10**: Windows school PC offline use still works without optional AI/sync/3D assets.

## Incremental rollout

1. Audit current per-tool transient mutation patterns and ID handling; no schema change.
2. Extract an app-level preview gateway for **one** reversible transform.
3. Add a source-revision conflict test, cancellation, and UI before/after overlay.
4. Extend to existing path suggestion and coordinate table import.
5. Implement a proof-of-concept \`RoomEditBatch\` behind a feature flag and model-based concurrency tests.
6. Only after owner review add Facing and equipment tracks with schema migration and export/reader fallback.
7. Mark this ADR **ACCEPTED** only when the decisions and gates are measured, otherwise revise it and keep status PROPOSED.

## Links / existing work

- Existing default branch \`main\` as of 2026-10-21: \`9a173f10\`.
- [PR #62](https://github.com/daichingale/MarchingDrillAppRust/pull/62) is a **separate draft** (count-accurate trajectory CSV); no implicit merge.
- [Issue #63](https://github.com/daichingale/MarchingDrillAppRust/issues/63) tracks a separate supply-chain blocker.
