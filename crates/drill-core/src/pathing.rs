//! Per-performer transition path metrics and set-to-set assignment optimization.
//! Pure logic, no UI. Complements `analyze_transition` in the crate root.

use crate::{Document, GridConfig, Point, Set, clinic};
use serde::{Deserialize, Serialize};

/// Straight-line (Euclidean) distance between two points, in field units.
pub fn path_length(a: Point, b: Point) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

/// A single performer's travel for a transition.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct PerformerMove {
    pub performer_index: usize,
    /// Straight-line distance travelled, in field units.
    pub distance: f32,
    /// Field units per count (`distance / counts`).
    pub step_size: f32,
}

/// Per-performer moves for the transition from `set_index` to `set_index + 1`.
///
/// Returns one entry per performer, index-aligned with `Document::performers`.
/// Uses the *source* set's `counts` for step sizing. Returns an empty vector if
/// either set is missing. When `counts == 0`, `step_size` is reported as `0.0`.
pub fn transition_moves(document: &Document, set_index: usize) -> Vec<PerformerMove> {
    let Some(from) = document.sets.get(set_index) else {
        return Vec::new();
    };
    let Some(to) = document.sets.get(set_index + 1) else {
        return Vec::new();
    };
    let counts = f32::from(from.counts);
    from.positions
        .iter()
        .zip(&to.positions)
        .enumerate()
        .map(|(performer_index, (&a, &b))| {
            let distance = path_length(a, b);
            let step_size = if counts > 0.0 { distance / counts } else { 0.0 };
            PerformerMove {
                performer_index,
                distance,
                step_size,
            }
        })
        .collect()
}

/// Aggregate statistics for a transition.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TransitionStats {
    /// Largest per-count step size across all performers.
    pub max_step: f32,
    /// Mean per-count step size across all performers.
    pub mean_step: f32,
    /// Sum of all straight-line travel distances.
    pub total_distance: f32,
    /// Performer index with the greatest distance, if any performers exist.
    pub longest_mover: Option<usize>,
}

/// Summarize the transition from `set_index` to `set_index + 1`.
pub fn transition_stats(document: &Document, set_index: usize) -> TransitionStats {
    let moves = transition_moves(document, set_index);
    if moves.is_empty() {
        return TransitionStats::default();
    }
    let mut max_step = 0.0f32;
    let mut total_step = 0.0f32;
    let mut total_distance = 0.0f32;
    let mut longest_mover = 0usize;
    let mut longest_distance = f32::NEG_INFINITY;
    for m in &moves {
        max_step = max_step.max(m.step_size);
        total_step += m.step_size;
        total_distance += m.distance;
        if m.distance > longest_distance {
            longest_distance = m.distance;
            longest_mover = m.performer_index;
        }
    }
    TransitionStats {
        max_step,
        mean_step: total_step / moves.len() as f32,
        total_distance,
        longest_mover: Some(longest_mover),
    }
}

/// The minimum pairwise distance within a formation (its tightest interval).
///
/// Returns `f32::INFINITY` when fewer than two points are supplied.
pub fn nearest_neighbor_interval(positions: &[Point]) -> f32 {
    let mut min = f32::INFINITY;
    for i in 0..positions.len() {
        for j in (i + 1)..positions.len() {
            min = min.min(path_length(positions[i], positions[j]));
        }
    }
    min
}

/// Total travel cost of an assignment mapping `from[i]` to `to[assignment[i]]`.
///
/// Entries of `assignment` that fall outside `to` are skipped (contribute 0).
pub fn assignment_cost(from: &[Point], to: &[Point], assignment: &[usize]) -> f32 {
    from.iter()
        .zip(assignment)
        .filter_map(|(&a, &j)| to.get(j).map(|&b| path_length(a, b)))
        .sum()
}

/// Optimize the mapping of `from` performers onto `to` target slots to minimize
/// total travel distance.
///
/// Returns a permutation of `0..N` where index `i` holds the `to` slot assigned
/// to `from[i]`. Uses a greedy nearest-available seed followed by 2-opt swap
/// refinement. This computes a mapping only; it does not mutate any Document.
///
/// If the two slices differ in length (or are empty) an identity mapping over
/// the `from` length is returned, so callers always receive a valid `Vec<usize>`.
pub fn optimal_assignment(from: &[Point], to: &[Point]) -> Vec<usize> {
    let n = from.len();
    if n != to.len() || n == 0 {
        return (0..n).collect();
    }

    // Greedy seed: assign each source (in order) to its nearest unused target.
    let mut assignment = vec![0usize; n];
    let mut used = vec![false; n];
    for i in 0..n {
        let mut best_j = usize::MAX;
        let mut best_d = f32::INFINITY;
        for j in 0..n {
            if used[j] {
                continue;
            }
            let d = path_length(from[i], to[j]);
            if d < best_d {
                best_d = d;
                best_j = j;
            }
        }
        assignment[i] = best_j;
        used[best_j] = true;
    }

    // 2-opt: swap target assignments between two sources whenever it lowers the
    // combined cost of that pair. Repeat until stable or the iteration cap hits.
    let max_passes = 4 * n + 8;
    for _ in 0..max_passes {
        let mut improved = false;
        for i in 0..n {
            for k in (i + 1)..n {
                let (ji, jk) = (assignment[i], assignment[k]);
                let current = path_length(from[i], to[ji]) + path_length(from[k], to[jk]);
                let swapped = path_length(from[i], to[jk]) + path_length(from[k], to[ji]);
                if swapped + f32::EPSILON < current {
                    assignment[i] = jk;
                    assignment[k] = ji;
                    improved = true;
                }
            }
        }
        if !improved {
            break;
        }
    }

    assignment
}

/// Maximum number of accepted target swaps in a collision-aware refinement.
///
/// Every accepted swap strictly lowers the pair `(collision count, total
/// travel)` in lexicographic order, so the loop can neither cycle nor stall --
/// the cap only bounds worst-case work. Two dozen is well past the point of
/// diminishing returns for real drill: mid-flight collisions arrive in small,
/// locally separable clusters, and a transition still colliding after 24
/// re-seats is telling the designer the two formations are wrong for each
/// other, not that the solver needs more attempts.
const MAX_REFINE_PASSES: usize = 24;

/// Candidate swaps evaluated per pass, worst (closest approach) first.
///
/// Each candidate costs one full swept scan, so the whole refinement is capped
/// at `MAX_REFINE_PASSES * MAX_TRIALS_PER_PASS` = 192 scans. That is a bounded,
/// predictable budget even for a 1,000-performer show, which is why this needs
/// no progress-driven early exit when run on a job thread.
const MAX_TRIALS_PER_PASS: usize = 8;

/// Tie-break tolerance for total travel, in field units.
///
/// `f32::EPSILON` is orders of magnitude below any coordinate a designer can
/// actually enter, so using it here would accept swaps that are numerically
/// but not meaningfully cheaper.
const DISTANCE_EPSILON: f32 = 1e-4;

/// Result of refining a distance-optimal assignment against mid-flight
/// collisions.
///
/// `*_before` describe the plain [`optimal_assignment`] baseline; `*_after`
/// describe `assignment`. Refinement is best-effort: `collisions_after` may
/// still be non-zero when no swap within the pass budget improved matters.
#[derive(Clone, Debug, PartialEq)]
pub struct CollisionAwareAssignment {
    /// Permutation of `0..N`: `from[i]` walks to `to[assignment[i]]`.
    pub assignment: Vec<usize>,
    pub collisions_before: usize,
    pub collisions_after: usize,
    pub distance_before: f32,
    pub distance_after: f32,
    /// Accepted target swaps. Zero means the distance-optimal assignment was
    /// already the best this refinement could find.
    pub swaps: usize,
}

impl CollisionAwareAssignment {
    /// Whether the refined assignment reaches the transition with no reported
    /// collision left.
    #[must_use]
    pub fn resolved(&self) -> bool {
        self.collisions_after == 0
    }

    /// Extra travel relative to the distance-optimal baseline (`0.04` = +4%).
    ///
    /// Zero when the baseline itself is degenerate (nobody moves), which keeps
    /// the UI summary from reporting an infinite percentage.
    #[must_use]
    pub fn distance_delta_ratio(&self) -> f32 {
        if self.distance_before > DISTANCE_EPSILON {
            (self.distance_after - self.distance_before) / self.distance_before
        } else {
            0.0
        }
    }
}

/// Optimize the `from` -> `to` mapping for travel distance *and* then refine it
/// against collisions that happen mid-flight, not just at rest.
///
/// Starts from [`optimal_assignment`] (distance-optimal, collision-blind) and
/// repeatedly asks [`clinic::scan_transition`] which performers come too close
/// while walking. For the worst reported pair -- and, if that one cannot be
/// improved, the next worst, up to [`MAX_TRIALS_PER_PASS`] per pass -- it tries
/// exchanging the two performers' target slots. A swap is kept only when it
/// lowers the collision count, or leaves the count unchanged while strictly
/// lowering total travel, so refinement never regresses what
/// [`optimal_assignment`] already optimized without buying a collision for it.
///
/// `counts` and `grid` describe the transition the assignment will live in:
/// `counts` is the source set's duration (it drives the clinic's stride
/// ratings and the reported collision beat) and `grid` sizes the clinic's
/// spatial hash. `params` selects the collision radii and the severity floor,
/// so callers get exactly the collisions their own clinic surface reports.
///
/// This computes a mapping only; no `Document` is mutated. Mismatched or empty
/// inputs fall through to the [`optimal_assignment`] identity result.
#[must_use]
pub fn optimal_assignment_collision_aware(
    from: &[Point],
    to: &[Point],
    counts: u16,
    grid: &GridConfig,
    params: clinic::ClinicParams,
) -> CollisionAwareAssignment {
    let n = from.len();
    let mut assignment = optimal_assignment(from, to);
    let distance_before = assignment_cost(from, to, &assignment);
    if n != to.len() || n == 0 {
        return CollisionAwareAssignment {
            assignment,
            collisions_before: 0,
            collisions_after: 0,
            distance_before,
            distance_after: distance_before,
            swaps: 0,
        };
    }

    // `clinic::scan_transition` is this project's only swept-path collision
    // test and it is document-shaped, so rather than duplicate its geometry we
    // build one throwaway two-set document here and rewrite only the arrival
    // positions between passes. Both the document and the `ScanScratch`
    // spatial hash are therefore allocated once for the whole refinement.
    // `Document::demo` numbers performers 1..=N in index order, which is what
    // `collect_collisions` relies on to map an event back to a `from` index.
    let mut probe = Document::demo(1, n);
    probe.grid = grid.clone();
    probe.sets[0].counts = counts.max(1);
    probe.sets[0].positions.copy_from_slice(from);
    debug_assert!(
        probe
            .performers
            .iter()
            .enumerate()
            .all(|(index, performer)| performer.id.get() as usize == index + 1),
        "probe document must number performers 1..=N in index order"
    );

    let mut scratch = clinic::ScanScratch::default();
    let mut events = Vec::new();
    let mut trial = Vec::new();

    seat_arrivals(&mut probe, to, &assignment);
    collect_collisions(&probe, params, &mut scratch, &mut events);
    let collisions_before = events.len();
    let mut collisions_after = collisions_before;
    let mut distance_after = distance_before;
    let mut swaps = 0usize;

    for _ in 0..MAX_REFINE_PASSES {
        if collisions_after == 0 {
            break;
        }
        // Worst (closest approach) first: the tightest pair is both the most
        // urgent to fix and the most likely to be a genuine head-on crossing,
        // which a target swap resolves outright.
        events.sort_unstable_by(|a, b| a.2.total_cmp(&b.2));
        let candidates = events.len().min(MAX_TRIALS_PER_PASS);
        let mut accepted = false;
        for candidate in 0..candidates {
            let (i, j, _) = events[candidate];
            if i == j || i >= n || j >= n {
                continue;
            }
            assignment.swap(i, j);
            let distance = assignment_cost(from, to, &assignment);
            seat_arrivals(&mut probe, to, &assignment);
            collect_collisions(&probe, params, &mut scratch, &mut trial);
            let improves = trial.len() < collisions_after
                || (trial.len() == collisions_after
                    && distance + DISTANCE_EPSILON < distance_after);
            if improves {
                collisions_after = trial.len();
                distance_after = distance;
                std::mem::swap(&mut events, &mut trial);
                swaps += 1;
                accepted = true;
                break;
            }
            assignment.swap(i, j);
        }
        if !accepted {
            // Best effort: no swap among the worst offenders helped, so the
            // caller keeps the improvement found so far rather than a panic,
            // an unbounded search, or a silent regression.
            break;
        }
    }

    CollisionAwareAssignment {
        assignment,
        collisions_before,
        collisions_after,
        distance_before,
        distance_after,
        swaps,
    }
}

/// Rewrite the probe document's arrival set to match `assignment`.
fn seat_arrivals(probe: &mut Document, to: &[Point], assignment: &[usize]) {
    let positions = &mut probe.sets[1].positions;
    for (index, &slot) in assignment.iter().enumerate() {
        positions[index] = to[slot];
    }
}

/// Run the swept clinic scan and flatten it to `(from index, from index,
/// closest approach)` triples that outlive the report's borrow of `scratch`.
fn collect_collisions(
    probe: &Document,
    params: clinic::ClinicParams,
    scratch: &mut clinic::ScanScratch,
    out: &mut Vec<(usize, usize, f32)>,
) {
    let report = clinic::scan_transition(probe, 0, params, scratch);
    out.clear();
    out.extend(report.collisions.iter().map(|event| {
        (
            event.a.get() as usize - 1,
            event.b.get() as usize - 1,
            event.distance,
        )
    }));
}

/// Re-derive a generated set's formation for the current roster and re-seat
/// every performer into it with the least total travel.
///
/// This is the roster-change ("reweave") counterpart to committing a shape:
/// when performers are added or removed, a set built from an editable
/// `ShapeSpec` should keep its *shape* and spread the new cast evenly across
/// it, instead of leaving a hole where the departed performer stood or
/// stranding a newcomer off-form. Slot matching uses plain
/// [`optimal_assignment`]; a re-seat happens inside one formation rather than
/// across a transition, so there is no mid-flight path to keep clear.
///
/// Returns the full replacement position vector for `set_index`, or `None`
/// when the set is missing, holds no cast, or was not generated from a shape
/// (in which case there is no slot geometry to re-derive).
#[must_use]
pub fn rebalance_roster(document: &Document, set_index: usize) -> Option<Vec<Point>> {
    let set = document.sets.get(set_index)?;
    let spec = set.shape.as_ref()?;
    let cast = document.performers.len();
    if cast == 0 {
        return None;
    }
    let mut slots = Vec::new();
    spec.sample(cast, &mut slots);
    if slots.len() != cast {
        return None;
    }
    let current = roster_positions(set, cast);
    let assignment = optimal_assignment(&current, &slots);
    Some(
        assignment
            .iter()
            .map(|&slot| slots.get(slot).copied().unwrap_or_default())
            .collect(),
    )
}

/// The set's stored positions resized to the current cast.
///
/// A performer added since the set was written has no prior coordinate. Seed
/// those at the surviving formation's centroid so the assignment treats them
/// as roughly equidistant from every free slot, rather than dragging them in
/// from the field origin and distorting the whole match.
fn roster_positions(set: &Set, cast: usize) -> Vec<Point> {
    let mut points = set.positions.clone();
    points.truncate(cast);
    if points.len() < cast {
        let fill = centroid(&points);
        points.resize(cast, fill);
    }
    points
}

fn centroid(points: &[Point]) -> Point {
    if points.is_empty() {
        return Point::default();
    }
    let divisor = points.len() as f32;
    Point {
        x: points.iter().map(|point| point.x).sum::<f32>() / divisor,
        y: points.iter().map(|point| point.y).sum::<f32>() / divisor,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: f32, y: f32) -> Point {
        Point { x, y }
    }

    fn two_set_doc(a: Vec<Point>, counts: u16, b: Vec<Point>) -> Document {
        let mut doc = Document::demo(1, a.len());
        doc.sets[0].counts = counts;
        doc.sets[0].positions = a;
        doc.sets[1].positions = b;
        doc
    }

    #[test]
    fn path_length_is_euclidean() {
        assert!((path_length(pt(0.0, 0.0), pt(3.0, 4.0)) - 5.0).abs() < 1e-6);
    }

    #[test]
    fn transition_moves_one_per_performer_with_step_math() {
        let doc = two_set_doc(
            vec![pt(0.0, 0.0), pt(0.0, 0.0)],
            8,
            vec![pt(8.0, 0.0), pt(0.0, 4.0)],
        );
        let moves = transition_moves(&doc, 0);
        assert_eq!(moves.len(), doc.performers.len());
        assert!((moves[0].distance - 8.0).abs() < 1e-6);
        assert!((moves[0].step_size - 1.0).abs() < 1e-6); // 8 / 8 counts
        assert!((moves[1].distance - 4.0).abs() < 1e-6);
        assert!((moves[1].step_size - 0.5).abs() < 1e-6); // 4 / 8 counts
    }

    #[test]
    fn transition_moves_guards_zero_counts() {
        let doc = two_set_doc(vec![pt(0.0, 0.0)], 0, vec![pt(3.0, 4.0)]);
        let moves = transition_moves(&doc, 0);
        assert_eq!(moves.len(), 1);
        assert!((moves[0].distance - 5.0).abs() < 1e-6);
        assert_eq!(moves[0].step_size, 0.0);
    }

    #[test]
    fn transition_moves_empty_when_no_next_set() {
        let doc = two_set_doc(vec![pt(0.0, 0.0)], 8, vec![pt(1.0, 1.0)]);
        assert!(transition_moves(&doc, 1).is_empty());
    }

    #[test]
    fn transition_stats_aggregates_correctly() {
        let doc = two_set_doc(
            vec![pt(0.0, 0.0), pt(0.0, 0.0)],
            8,
            vec![pt(8.0, 0.0), pt(0.0, 4.0)],
        );
        let stats = transition_stats(&doc, 0);
        assert!((stats.total_distance - 12.0).abs() < 1e-6);
        assert!((stats.max_step - 1.0).abs() < 1e-6);
        assert!((stats.mean_step - 0.75).abs() < 1e-6); // (1.0 + 0.5) / 2
        assert_eq!(stats.longest_mover, Some(0));
    }

    #[test]
    fn nearest_neighbor_interval_finds_tightest_pair() {
        let formation = vec![pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 2.0)];
        assert!((nearest_neighbor_interval(&formation) - 2.0).abs() < 1e-6);
    }

    #[test]
    fn nearest_neighbor_interval_infinite_for_small_sets() {
        assert_eq!(nearest_neighbor_interval(&[]), f32::INFINITY);
        assert_eq!(nearest_neighbor_interval(&[pt(1.0, 1.0)]), f32::INFINITY);
    }

    #[test]
    fn assignment_cost_matches_hand_calculation() {
        let from = vec![pt(0.0, 0.0), pt(0.0, 0.0)];
        let to = vec![pt(3.0, 4.0), pt(6.0, 8.0)];
        // identity: 5 + 10 = 15
        assert!((assignment_cost(&from, &to, &[0, 1]) - 15.0).abs() < 1e-6);
    }

    #[test]
    fn optimal_assignment_returns_valid_permutation() {
        let from = vec![pt(0.0, 0.0), pt(5.0, 5.0), pt(9.0, 1.0), pt(2.0, 8.0)];
        let to = vec![pt(1.0, 1.0), pt(8.0, 2.0), pt(4.0, 6.0), pt(0.0, 9.0)];
        let assignment = optimal_assignment(&from, &to);
        assert_eq!(assignment.len(), 4);
        let mut seen = assignment.clone();
        seen.sort_unstable();
        assert_eq!(seen, vec![0, 1, 2, 3]);
    }

    #[test]
    fn optimal_assignment_finds_the_swap() {
        // Two performers whose nearest targets are each other's slot.
        let from = vec![pt(0.0, 0.0), pt(10.0, 0.0)];
        let to = vec![pt(10.0, 0.0), pt(0.0, 0.0)];
        let assignment = optimal_assignment(&from, &to);
        // Optimal maps 0->slot1 (0,0) and 1->slot0 (10,0): total cost 0.
        assert_eq!(assignment, vec![1, 0]);
        assert!((assignment_cost(&from, &to, &assignment)).abs() < 1e-6);
    }

    #[test]
    fn two_opt_never_worse_than_greedy() {
        // A scattered case; recompute the greedy seed and compare to the result.
        let from = vec![
            pt(0.0, 0.0),
            pt(1.0, 5.0),
            pt(9.0, 9.0),
            pt(8.0, 1.0),
            pt(4.0, 4.0),
        ];
        let to = vec![
            pt(9.0, 8.0),
            pt(0.0, 1.0),
            pt(4.0, 5.0),
            pt(8.0, 2.0),
            pt(1.0, 4.0),
        ];

        // Reproduce the greedy-only seed.
        let n = from.len();
        let mut greedy = vec![0usize; n];
        let mut used = vec![false; n];
        for i in 0..n {
            let mut best_j = usize::MAX;
            let mut best_d = f32::INFINITY;
            for j in 0..n {
                if used[j] {
                    continue;
                }
                let d = path_length(from[i], to[j]);
                if d < best_d {
                    best_d = d;
                    best_j = j;
                }
            }
            greedy[i] = best_j;
            used[best_j] = true;
        }

        let optimized = optimal_assignment(&from, &to);
        assert!(
            assignment_cost(&from, &to, &optimized) <= assignment_cost(&from, &to, &greedy) + 1e-6
        );
    }

    #[test]
    fn optimal_assignment_handles_unequal_lengths() {
        let from = vec![pt(0.0, 0.0), pt(1.0, 1.0)];
        let to = vec![pt(0.0, 0.0)];
        assert_eq!(optimal_assignment(&from, &to), vec![0, 1]);
        assert!(optimal_assignment(&[], &[]).is_empty());
    }

    /// The same thresholds the editing workspace's clinic surface reports with,
    /// so these tests exercise the configuration the feature actually ships.
    fn strict_params() -> clinic::ClinicParams {
        clinic::ClinicParams {
            style: clinic::StepStyle::Custom {
                units_per_step: 1.0,
            },
            collision_radius: 0.75,
            danger_radius: 0.75,
            crowded_radius: 0.75,
            aggressive_above: 1.0,
            impossible_above: f32::MAX,
            ..clinic::ClinicParams::default()
        }
    }

    fn is_permutation(assignment: &[usize]) -> bool {
        let mut seen = assignment.to_vec();
        seen.sort_unstable();
        seen == (0..assignment.len()).collect::<Vec<_>>()
    }

    #[test]
    fn collision_aware_resolves_a_mid_flight_near_miss_the_distance_solver_accepts() {
        // A walks the length of the field; B barely moves, sitting 0.4 units
        // off A's line. The cheapest assignment (10.100) sends A straight
        // through B; the collision-free alternative costs 10.132, so plain
        // `optimal_assignment` will never find it on its own.
        let grid = GridConfig::default();
        let from = vec![pt(0.0, 0.0), pt(5.0, 0.4)];
        let to = vec![pt(10.0, 0.0), pt(5.1, 0.4)];

        let plain = optimal_assignment(&from, &to);
        assert_eq!(plain, vec![0, 1], "baseline must be the colliding mapping");

        let refined = optimal_assignment_collision_aware(&from, &to, 8, &grid, strict_params());
        assert!(is_permutation(&refined.assignment));
        assert_eq!(refined.collisions_before, 1);
        assert_eq!(refined.collisions_after, 0);
        assert!(refined.resolved());
        assert_eq!(refined.assignment, vec![1, 0], "targets must be exchanged");
        assert_eq!(refined.swaps, 1);
        // Buying the fix is allowed to cost travel, but only a little: the
        // whole point of seeding from `optimal_assignment` is not to regress
        // distance wholesale.
        assert!(
            refined.distance_after <= refined.distance_before * 1.05,
            "{} vs {}",
            refined.distance_after,
            refined.distance_before
        );
        assert!(refined.distance_delta_ratio() > 0.0);
        assert!(refined.distance_delta_ratio() < 0.05);
    }

    #[test]
    fn collision_aware_matches_the_plain_solver_when_nothing_collides() {
        let grid = GridConfig::default();
        let from = vec![pt(0.0, 0.0), pt(0.0, 20.0), pt(0.0, 40.0)];
        let to = vec![pt(30.0, 0.0), pt(30.0, 20.0), pt(30.0, 40.0)];
        let plain = optimal_assignment(&from, &to);
        let refined = optimal_assignment_collision_aware(&from, &to, 16, &grid, strict_params());
        assert_eq!(refined.collisions_before, 0);
        assert_eq!(refined.collisions_after, 0);
        assert_eq!(refined.swaps, 0);
        assert_eq!(refined.assignment, plain);
        assert!((refined.distance_after - refined.distance_before).abs() < 1e-6);
        assert_eq!(refined.distance_delta_ratio(), 0.0);
    }

    #[test]
    fn collision_aware_is_best_effort_when_the_target_form_is_unresolvable() {
        // Every arrival slot sits inside the collision radius of its
        // neighbours, so no permutation can clear the transition. The
        // contract is a bounded, non-panicking, never-worse result -- not a
        // solution.
        let grid = GridConfig::default();
        let from = (0..5).map(|i| pt(i as f32 * 5.0, 0.0)).collect::<Vec<_>>();
        let to = (0..5)
            .map(|i| pt(10.0 + i as f32 * 0.2, 0.0))
            .collect::<Vec<_>>();
        let refined = optimal_assignment_collision_aware(&from, &to, 8, &grid, strict_params());
        assert!(is_permutation(&refined.assignment));
        assert!(refined.collisions_before > 0);
        assert!(refined.collisions_after > 0, "the form cannot be cleared");
        assert!(refined.collisions_after <= refined.collisions_before);
        assert!(refined.swaps <= MAX_REFINE_PASSES);
        assert!(!refined.resolved());
    }

    #[test]
    fn collision_aware_tolerates_degenerate_input() {
        let grid = GridConfig::default();
        let empty = optimal_assignment_collision_aware(&[], &[], 8, &grid, strict_params());
        assert!(empty.assignment.is_empty());
        assert_eq!(empty.collisions_before, 0);
        assert_eq!(empty.distance_delta_ratio(), 0.0);

        let mismatched = optimal_assignment_collision_aware(
            &[pt(0.0, 0.0), pt(1.0, 1.0)],
            &[pt(0.0, 0.0)],
            8,
            &grid,
            strict_params(),
        );
        assert_eq!(mismatched.assignment, vec![0, 1]);
        assert_eq!(mismatched.collisions_after, 0);
    }

    #[test]
    fn collision_aware_never_mutates_a_caller_document() {
        // The probe document is internal; nothing the caller owns is touched.
        let document = Document::demo(3, 4);
        let before = document.clone();
        let refined = optimal_assignment_collision_aware(
            &document.sets[0].positions,
            &document.sets[1].positions,
            document.sets[0].counts,
            &document.grid,
            strict_params(),
        );
        assert_eq!(document, before);
        assert!(is_permutation(&refined.assignment));
    }

    #[test]
    fn rebalance_roster_spreads_a_shrunken_cast_across_the_whole_shape() {
        let mut document = Document::demo(1, 12);
        document.sets[0].shape = Some(crate::shapes::ShapeSpec::Line {
            start: pt(0.0, 10.0),
            end: pt(44.0, 10.0),
        });
        // Two performers leave; the document is already down to 10, but the
        // set still holds the 12-slot geometry.
        document.performers.truncate(10);
        let positions =
            rebalance_roster(&document, 0).expect("a shape-generated set can be rebalanced");
        assert_eq!(positions.len(), 10);
        // Re-derived for ten, the line's endpoints are still occupied and the
        // interval is even -- no hole where the departed performers stood.
        let min_x = positions.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
        let max_x = positions
            .iter()
            .map(|p| p.x)
            .fold(f32::NEG_INFINITY, f32::max);
        assert!((min_x - 0.0).abs() < 1e-3);
        assert!((max_x - 44.0).abs() < 1e-3);
        let interval = nearest_neighbor_interval(&positions);
        assert!((interval - 44.0 / 9.0).abs() < 1e-3, "interval {interval}");
    }

    #[test]
    fn rebalance_roster_seats_newcomers_without_dragging_them_from_the_origin() {
        let mut document = Document::demo(1, 4);
        document.sets[0].shape = Some(crate::shapes::ShapeSpec::Line {
            start: pt(60.0, 30.0),
            end: pt(66.0, 30.0),
        });
        document.sets[0].positions = vec![
            pt(60.0, 30.0),
            pt(62.0, 30.0),
            pt(64.0, 30.0),
            pt(66.0, 30.0),
        ];
        // One performer joins; the set has no coordinate for them yet.
        let newcomer = document.performers[0].clone();
        document.performers.push(newcomer);
        let positions = rebalance_roster(&document, 0).expect("shape re-samples for five");
        assert_eq!(positions.len(), 5);
        assert!(
            positions.iter().all(|p| p.x >= 59.9 && p.x <= 66.1),
            "every seat stays on the form: {positions:?}"
        );
    }

    #[test]
    fn rebalance_roster_declines_sets_without_editable_geometry() {
        let document = Document::demo(1, 4);
        assert!(document.sets[0].shape.is_none());
        assert!(rebalance_roster(&document, 0).is_none());
        assert!(rebalance_roster(&document, 99).is_none());
    }
}
