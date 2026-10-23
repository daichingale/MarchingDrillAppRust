# NextGen Geometry & Motion Contract 2026
**Status:** Design proposal, not implementation proof.  
**Owners:** \`drill-core\` (geometry/time), \`drill-render\` (projections), \`drill-export\` (reports), \`drill-app\` (interaction).  
**References:** [Master plan](../MASTER_PLAN_NEXTGEN_2026.md), [ADR-0003](../adr/0003-nextgen-editing-and-time-model.md), [existing transition model](../design/11-transition-model.md), [coordinate system](../design/16-coordinates-notation.md).

## 1. Problem to solve

A marching drill is a time-dependent identity-preserving system: dots are not merely shape samples, performers may hold and turn independently, and visual props may extend the body. The same work needs to produce **editable geometry**, **reproducible motion**, **coachable warnings**, **paper coordinates**, and **low-latency 3D**.

The next generation must **not** rewrite the already implemented route engine wholesale. This document defines behavior and conformance tests before extending storage models.

## 2. Canonical coordinate domains

| Domain | Definition | Invariants |
|---|---|---|
| \`FieldPoint\` (existing \`Point\`) | \`(x,y)\` in \`GridConfig.unit\`, increasing \`x\` to the right and increasing \`y\` away from the audience/front edge | Finite numeric values, validated bounds, never mixes steps and units |
| \`WorldPoint3D\` | Presentation only, converts field coordinates via shared projection | Does not become an alternate source of performer locations |
| \`StepVector\` | Field displacement divided by axis-specific \`horizontal_units / horizontal_steps\` and \`vertical_units / vertical_steps\` | Carries a unit/step profile, no silent assumption of 8-to-5 |
| \`MetricDistance\` | Physical separation in meters for clearance/obstacle rules | Yards convert with exactly 0.9144 m/yd |
| \`StageBoundary\` | Physical polygon and obstacles; distinct from larger editing canvas and printed lines | Can be edited only with an explicit venue-changing operation |

**Typical cases:**
- Standard 8-to-5: 5 yards / 8 steps = **0.625 yd/step = 0.5715 m/step**.
- Japan indoor reference: 30m square competition/reference frame plus optional exterior staging canvas. Do not assume every Japanese event uses identical markings; venue profiles are selectable and verified.
- A 5-meter gridline and a 5-yard gridline **must not** map to the same physical separation.

## 3. Identity and assignment

The canonical mapping is \`Document.performers[performer_index].id\` ↔ \`Set.positions[performer_index]\` for the same **current immutable document snapshot**. Long-lived selections, route keys, imported data and collaborative commands must use \`PerformerId\`, never a persisted array index.

**Shape sampling** creates target points and proposed assignment only. Changing the sample spacing (e.g. circle radius) must not silently permute performers. When assigning slots automatically:
1. fixed IDs remain fixed;
2. constrained section order remains valid;
3. each source maps to one valid destination exactly once;
4. a proposed permutation is shown in visual comparison and a cost report;
5. accepted result is one Undoable Edit;
6. if no valid assignment exists, return an explicit error and preserve the document.

The existing greedy+2-opt assignment is a fast candidate, **not a guarantee of global optimality**. For an optional Hungarian/min-cost-flow solver, define a hard workload cap and deterministic tie-breaking first.

## 4. Time interval and movement semantics

For a source set \`S_i\`, define:
- \`moves = S_i.counts\`,
- \`hold = S_i.hold\`,
- \`total = moves + hold\`,
- \`C_i = Σ_{j<i}(S_j.counts + S_j.hold)\`.

During \`[0, moves)\`, an evaluator resolves a performer's gate/easing/route and outputs \`Point\`. From \`moves\` through \`total\`, the performer is on the next set's target point. A missing following set is a **terminal snapshot**, not an invented transition.

At a shared boundary \`C_(i+1)\`, the rendered location must be identical whether evaluated as the end of set \`i\` or start of \`i+1\` (up to documented floating-point tolerance). Never sample the same boundary twice in a flattened full-show export unless it is explicitly tagged as a duplicate visual event.

**Three time levels:**
1. integer musical counts for ordinary chart positions and print;
2. bounded rational subcounts for facing, flags, prop/visual events (initial proposal: eighth count, adjustable after test);
3. \`f64\` seconds/sample indices for audio and high precision video scheduling.

An event exactly at \`moves\` takes effect in the **arrival** phase, not an ambiguous half-open prior set; holds are an explicit phase. Route departs/arrives remain typed \`Gate\` fields with runtime validation.

## 5. Facing: physical body and travel are independent

Define \`Facing\` as *orientation around the vertical axis*, relative to a documented field/front convention (zero baseline to be fixed in an ADR). It is distinct from:
- path tangent (direction of travel);
- bell direction (instrument pointing);
- eye/gaze direction (optional visual);
- prop equipment orientation.

Candidate keyframe modes:
- Fixed angle / Front / Back / Side;
- Follow tangent with angular offset;
- Slide (face audience while moving sideways);
- Backward march (face opposite direction of travel);
- Interpolated turn over a count/subcount range;
- Explicit zero-duration pose event (needs boundary semantics).

On a zero-velocity hold, \`FollowTangent\` cannot infer a unique orientation. **Fallback must be the last explicitly defined facing or a visible unassigned state**, never an arbitrary angle from floating noise. Treat 0→360/0→2π wrap consistently; interpolation chooses the shorter turn unless the author requests a directed multi-turn.

**Proposed tests:** hold→turn, backward marching, at count boundary, camera/3D facing consistency, undo after keyframe edit, deletion of performer keeping IDs stable.

## 6. Step size and tempo

The straight-line distance between dots is only a **lower bound** for a curved path. Let a performer travel \`L\` field units between its gate departure and arrival with \`Δc>0\` available counts. Then:
- physical travel = \`L * meters_per_unit\`;
- average physical distance per count = \`physical_travel / Δc\`;
- average movement speed can be estimated using tempo-integrated elapsed seconds \`Δt\` (variable BPM supported);
- optional march-style stride = route length / chosen style step length;
- rapid turns/crossing and body/equipment geometry are **additional independent difficulty dimensions**, not converted to one magic number.

The user can configure different thresholds for marching style, section, equipment type, and backward/sideways movement. Defaults require expert review. A threshold exceedance is a coaching warning; it is **not** a definitive physical safety verdict.

Curved paths: use the already defined arc-length representation or adaptive numerical integration with an error budget (initial geometry tolerance to be set with fixtures; do not promise exact true analytic arclength). Compare computed \`L\` with the current renderer's route samples to ensure UI/coaching parity.

## 7. Collision and clearance model

**Definitions**
- \`At(t,p)\`: performer \`p\`'s canonical field position at musical time \`t\`.
- \`radius(p,t)\`: optional conservative physical clearance radius from body/instrument and configuration.
- \`clearance(i,j,t)\`: physical distance between \`At(t,i)\` and \`At(t,j)\`, minus both radii.
- A true **numeric overlap** is \`clearance < 0\`; a **near pass** is \`0 <= clearance < user_margin\`. Safety certification is *not claimed*.

**Tier 1: broad-phase temporal pruning**
- For every transition, compute conservative motion bounds for each performer over small time intervals. Include the full curve/arc envelope **and** any holds; a bound consisting only of the endpoints is insufficient for a curve.
- Spatial grid/sweep/BVH based on the maximum conservative radius. Eliminate impossible pairs before expensive narrow-phase checks.
- Handle 0, 1, 1,000 and max 4,000 performers; keep hot-path scratch reusable, and don't loop over O(N²) full show every UI frame.

**Tier 2: narrow-phase distance**
- For linear motion inside a common time interval, let relative displacement be \`r(t)=r0+t*v\`; the candidate minimizer on \`[0,1]\` is \`t* = clamp(-dot(r0,v)/dot(v,v),0,1)\` when \`dot(v,v)>0\`. Check endpoints and \`t*\` for distance.
- For gated/eased/curved routes, adaptively subdivide in time or evaluate conservative geometry bounds until the minimum is bracketed within a documented tolerance. If computation budget is exhausted, report **unresolved at current precision** rather than \`safe\`.
- Compare simultaneous-time positions. Two performers crossing the same place **at different counts** do not collide.
- Include each instrument/prop as a separate clearance policy when a physically meaningful object is available, but do not pretend a cylinder/circle models actual performer orientation perfectly.

**Report contract**
\`\`\`text
CollisionFinding {
  set_id,
  performer_ids: (a,b),
  local_time_interval,
  closest_count_estimate,
  closest_distance_m,
  required_clearance_m,
  confidence: exact_linear | conservative_bound | sampled_estimate,
  outcome: overlap | near_pass | unresolved,
  reproducer_seed,
}
\`\`\`
Concept only; not implemented struct. Findings should be deterministic, de-duplicated into count runs, show the offending pair, and link to a one-click timeline jump. A user can turn off a **suggestion**, not the recorded true geometry in the source project.

## 8. Print/export/interop parity

All exporters read the same \`Document\` snapshot and field/tempo semantics.
- Single-set coordinates print the target dots, not sampled positions mistaken as final.
- Transition trajectory CSV (separate draft PR #62) samples the actual route evaluator count by count and labels start/arrival/hold; unit is always included.
- Drillbooks should make \`before\` / \`after\` / \`hold\` and stage orientation visible. A page may be 30m even if the edit canvas is larger.
- Import from CSV/XLSX detects decimal separator, delimiter, field orientation and unit. If ambiguous, **stop at preview** and ask for a mapping; never rotate/flip without an explicit declaration.
- Peer/device sync passes IDs and physical units explicitly; no client-specific calculated positions in persistent source data.

## 9. Reference test cases (developer must build as independent data)

| ID | Geometry and timing | Expected diagnostic |
|---|---|---|
| G-01 | Two dots cross at midpoint between integer counts | Collision found despite neither endpoint overlapping |
| G-02 | Same crossing location visited 2 counts apart | No same-time collision |
| G-03 | 8 people on arc, leader at one end, all follow | Reordering preserves ID and no route discontinuity |
| G-04 | One performer starts at count 4, arrives at count 12, holds 4 | Positions agree with both UI and count CSV |
| G-05 | Bezier bulges outside bounding box of its endpoints | Broad-phase cannot falsely prune it |
| G-06 | 30m indoor with 5m grid and standard 8-to-5 | Physical step = 0.5715m, no mistaken 5m/8 equivalence |
| G-07 | Zero-move hold with facing turn 270 degrees | Position constant, orientation changes with directed/shorter-turn rule |
| G-08 | 1000 people, narrow-collision broad-phase | Clinic response within budget, no fresh allocation on hot path |
| G-09 | Misshapen imported CSV, reversed front/back | Preview identifies mismatch; no edited document |
| G-10 | Undo a mixed 24-person curved transform and gate change | Every ID/route/count and saved show hash restored |
| G-11 | Curve at shared set boundary and tempo change | Identical starting/arrival location across render/print/export |
| G-12 | Two peers modify different dots vs same formation atomically | Disjoint edits converge; overlapping edits produce an explicit conflict |

## 10. Incremental implementation path

1. Audit and document current \`pathing.rs\`, \`clinic.rs\`, \`transition.rs\`, \`continuity.rs\` formulas, including which use straight line vs actual arclength.
2. Add the reference tests G-01/G-02/G-04/G-05/G-06 without changing the public schema.
3. Extract shared typed \`PhysicalDistance\` / \`StepRate\` helpers without changing persistence.
4. Bring analytics and drillbook calculations into agreement with existing route evaluator; review numerical tolerances.
5. Prototype Facing as an **optional transient derived view** with zero persisted fields.
6. Propose an ADR + migration for Facing/subcount storage when that prototype is accepted.
7. Extend clinic with equipment clearance policies and profiled precision/uncertainty.
8. Expand field study with percussion, brass and guard experts on full rehearsal drills.

**Release gate:** After any path algorithm change, the entire golden/test suite, performance budget, Undo/Redo and old-file fixtures must still pass. Warnings may improve, but silently editing someone into a new position is forbidden.
