//! Per-performer transition path metrics and set-to-set assignment optimization.
//! Pure logic, no UI. Complements `analyze_transition` in the crate root.

use crate::{Document, Point};
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
}
