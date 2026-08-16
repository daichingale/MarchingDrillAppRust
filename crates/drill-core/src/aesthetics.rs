//! Aesthetic / symmetry scoring for a single formation (`Set`).
//!
//! Pyware 3D has no equivalent of this module: it only checks mechanical
//! constraints (spacing, step size). This module adds a *judgement*-oriented
//! signal — how visually balanced a formation looks — so a designer can get
//! live feedback on the same qualities an experienced staff member eyeballs.
//!
//! Two independent, deliberately simple proxies are combined:
//!
//! 1. **Left-right symmetry** about a vertical axis on the field
//!    ([`SymmetryDetail`]).
//! 2. **Density uniformity** — how evenly performers are spread across a
//!    coarse grid over the field ([`analyze_set`]'s
//!    `density_uniformity` field).
//!
//! Neither proxy is an absolute measure of beauty. A deliberately asymmetric
//! design (a wedge pushed toward one sideline) or a deliberately dense block
//! or circle are legitimate, often *more* striking choices than a symmetric
//! or evenly-spread formation. Both scores are reference indicators meant to
//! be shown as one input among many, never as a verdict — see the caveat on
//! `density_uniformity_score` below, which any UI surfacing this value should
//! carry forward in its wording (e.g. "spread evenness", not "beauty").
//!
//! Follows the `Params + pure analysis fn` shape used by `clinic.rs`, but
//! unlike swept collision scanning (which runs every frame while dragging),
//! this analysis is triggered on demand for a single static formation and
//! has no cross-call state worth persisting, so no `*Scratch` type is
//! introduced — every call here allocates once and returns.

use crate::{Document, Performer, PerformerId, Point};
use serde::{Deserialize, Serialize};

/// Detail for the left-right symmetry sub-score.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SymmetryDetail {
    /// Field x-coordinate of the mirror axis used for this score.
    pub axis_x: f32,
    /// 0-100, 100 = perfectly mirror-symmetric about `axis_x`.
    pub score: f32,
    /// Performers contributing the largest mirror-distance, worst (largest
    /// distance) first. Distance is in field units (`GridConfig::unit`).
    /// Capped at `AestheticParams::worst_offenders_len` entries.
    pub worst_offenders: Vec<(PerformerId, f32)>,
}

/// Combined aesthetic evaluation of one formation
/// (`Document::sets[set_index]`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AestheticScore {
    pub symmetry: SymmetryDetail,
    /// 0-100. **Reference indicator only** (see module docs): a low score
    /// does not mean the formation is ugly — an intentional block, wedge, or
    /// circle formation is legitimately non-uniform. Do not present this to
    /// users as an absolute judgement.
    pub density_uniformity: f32,
    /// 0-100, the weighted combination of `symmetry.score` and
    /// `density_uniformity` (weights from `AestheticParams`).
    pub overall: f32,
}

/// Tunable knobs for [`analyze_set`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AestheticParams {
    /// Density grid column count. Clamped to at least 1.
    pub density_grid_cells_x: u16,
    /// Density grid row count. Clamped to at least 1.
    pub density_grid_cells_y: u16,
    /// Weight of the symmetry sub-score in `overall`. Negative values are
    /// treated as 0.
    pub symmetry_weight: f32,
    /// Weight of the density-uniformity sub-score in `overall`. Negative
    /// values are treated as 0.
    pub density_weight: f32,
    /// Maximum number of entries kept in `SymmetryDetail::worst_offenders`.
    pub worst_offenders_len: usize,
}

impl Default for AestheticParams {
    /// `10x10` density grid is a reasonable default for a standard football
    /// field (`100 x 53.33` yd): each cell is roughly `10 x 5.3` yd, on the
    /// order of a few performers' marching intervals, fine enough to notice
    /// clumping without being so fine that ordinary spacing noise dominates.
    /// Symmetry is weighted slightly above density uniformity because
    /// mirror-symmetry is a much more commonly *intended* design property in
    /// marching drill than perfectly even spread (which is frequently
    /// violated on purpose).
    fn default() -> Self {
        Self {
            density_grid_cells_x: 10,
            density_grid_cells_y: 10,
            symmetry_weight: 0.6,
            density_weight: 0.4,
            worst_offenders_len: 5,
        }
    }
}

/// Falls back to `fallback` when `value` is non-finite or non-positive, so a
/// malformed `GridConfig` (should not happen post-`Document::validate`, but
/// this module must not panic even on documents nobody validated yet) can
/// never divide-by-zero or propagate NaN.
fn sanitized_dimension(value: f32, fallback: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        fallback
    }
}

/// Analyzes one set's formation for symmetry and density uniformity.
///
/// Returns `None` when `set_index` does not exist, the document has no
/// performers, or the target set has no positions — there is nothing
/// meaningful to score. Zero, one, or NaN-tainted performer positions never
/// panic: NaN-positioned performers are simply excluded from the relevant
/// sub-computation (mirroring the `Excluded`-style handling in
/// `clinic.rs`/doc 13, without needing a full exclusion report here since
/// this is a single summary score, not an event list).
///
/// ## Complexity
///
/// `O(n)` for density uniformity, `O(n^2)` for symmetry (`n` = performer
/// count in this set). At `n = 1,000` that is ~1e6 distance evaluations for
/// symmetry, sub-millisecond to a few ms in release builds — see the doc
/// comment on `symmetry_score` for why an O(n^2) nearest-neighbor scan was
/// chosen over reusing `pathing::optimal_assignment`'s bijective solver.
pub fn analyze_set(
    document: &Document,
    set_index: usize,
    params: &AestheticParams,
) -> Option<AestheticScore> {
    let set = document.sets.get(set_index)?;
    if set.positions.is_empty() || document.performers.is_empty() {
        return None;
    }
    let n = set.positions.len().min(document.performers.len());
    let positions = &set.positions[..n];
    let performers = &document.performers[..n];

    let width = sanitized_dimension(document.grid.width, 100.0);
    let height = sanitized_dimension(document.grid.height, 53.333);
    let axis_x = width * 0.5;

    let symmetry = symmetry_score(
        performers,
        positions,
        axis_x,
        height,
        params.worst_offenders_len,
    );
    let density_uniformity = density_uniformity_score(
        positions,
        width,
        height,
        params.density_grid_cells_x.max(1),
        params.density_grid_cells_y.max(1),
    );
    let overall = weighted_overall(symmetry.score, density_uniformity, params);

    Some(AestheticScore {
        symmetry,
        density_uniformity,
        overall,
    })
}

/// Left-right (mirror) symmetry about a vertical axis at `axis_x`.
///
/// For every finite-position performer, mirrors their position across
/// `axis_x` and finds the straight-line distance to the *nearest other
/// finite-position performer* to that mirrored point. A perfectly symmetric
/// formation has every mirrored point coincide exactly with some other real
/// performer, giving distance 0 for everyone and `score == 100`.
///
/// ## Why an independent nearest-neighbor scan instead of a bijective assignment
///
/// `pathing::optimal_assignment` solves a *bijective* (one-to-one) matching
/// via a greedy seed plus an `O(passes * n^2)` 2-opt refinement pass
/// (`max_passes = 4n + 8`). At `n = 1,000` that refinement is up to ~4,000
/// passes over ~500,000 pairs each — several orders of magnitude past the
/// "a few ms" budget this score needs, since it is meant to run as live
/// design feedback. A strict bijection is also not what visual-symmetry
/// judgement calls for: a designer checks "does *something* mirror me", not
/// "is there a unique pairing" — two performers who happen to be
/// interchangeable across the axis, or several stacked on the axis itself,
/// should not be penalized just because a one-to-one solver couldn't
/// resolve the tie. So this scans, per mirrored point, all other performers
/// for the nearest one: a plain `O(n^2)` loop with a firm, easy-to-reason
/// bound (~1e6 distance evaluations at `n = 1,000`) and no iterative
/// refinement that could blow past it.
///
/// ## Normalization
///
/// The mean mirror-distance is normalized against `reference = grid height /
/// 2` (half the field's short dimension). This keeps the score
/// resolution-independent — scaling the whole field roughly scales both
/// typical inter-performer spacing and any true asymmetry together, so the
/// ratio stays stable — while mapping "off by half the field" to a score of
/// 0 and "exact or near-exact mirror" to a score near 100:
/// `score = 100 * (1 - clamp(mean_distance / reference, 0, 1))`.
fn symmetry_score(
    performers: &[Performer],
    positions: &[Point],
    axis_x: f32,
    height: f32,
    worst_offenders_len: usize,
) -> SymmetryDetail {
    let finite: Vec<(PerformerId, Point)> = performers
        .iter()
        .zip(positions)
        .filter(|(_, p)| p.x.is_finite() && p.y.is_finite())
        .map(|(performer, p)| (performer.id, *p))
        .collect();

    if finite.len() < 2 {
        // Nothing to be asymmetric relative to (0 or 1 usable performer).
        // Treat as trivially symmetric rather than penalizing an edge case
        // this module was explicitly asked to tolerate without panicking.
        return SymmetryDetail {
            axis_x,
            score: 100.0,
            worst_offenders: Vec::new(),
        };
    }

    let mut distances: Vec<(PerformerId, f32)> = Vec::with_capacity(finite.len());
    for (index, &(id, p)) in finite.iter().enumerate() {
        let mirrored = Point {
            x: 2.0 * axis_x - p.x,
            y: p.y,
        };
        let nearest = finite
            .iter()
            .enumerate()
            .filter(|(other_index, _)| *other_index != index)
            .map(|(_, &(_, other_point))| crate::pathing::path_length(mirrored, other_point))
            .fold(f32::INFINITY, f32::min);
        distances.push((id, nearest));
    }

    let mean_distance = distances.iter().map(|&(_, d)| d).sum::<f32>() / distances.len() as f32;
    let reference = (height * 0.5).max(f32::EPSILON);
    let score = (100.0 * (1.0 - (mean_distance / reference).clamp(0.0, 1.0))).clamp(0.0, 100.0);

    let mut worst_offenders = distances;
    if worst_offenders_len > 0 {
        // Unstable sort is fine: ties keep an arbitrary but deterministic
        // (input-order-derived) relative order for equal distances, and we
        // never rely on stability elsewhere.
        worst_offenders.sort_unstable_by(|a, b| b.1.total_cmp(&a.1));
        worst_offenders.truncate(worst_offenders_len);
    } else {
        worst_offenders.clear();
    }

    SymmetryDetail {
        axis_x,
        score,
        worst_offenders,
    }
}

/// How evenly performers are spread across a coarse grid over the field.
///
/// **Reference indicator only — not an absolute beauty measure.** A
/// deliberately dense block, wedge, or circle formation is a legitimate and
/// often more visually striking design choice than an evenly-spread
/// formation; a low score here must not be read as "this formation is bad".
/// Any UI surfacing this value should label it as a spread/uniformity
/// indicator, not a quality score.
///
/// Splits the field into `cols * rows` equal cells, counts finite-position
/// performers per cell, and computes the coefficient of variation
/// (population standard deviation / mean) of the per-cell counts.
/// `score = 100 / (1 + cv)`, so a perfectly even spread (`cv == 0`) scores
/// 100 and increasingly clumped distributions approach 0 smoothly.
///
/// `O(n + cols * rows)`.
fn density_uniformity_score(
    positions: &[Point],
    width: f32,
    height: f32,
    cols: u16,
    rows: u16,
) -> f32 {
    let cols = cols.max(1) as usize;
    let rows = rows.max(1) as usize;
    let mut counts = vec![0u32; cols * rows];
    let mut total = 0u32;
    for p in positions {
        if !p.x.is_finite() || !p.y.is_finite() {
            continue;
        }
        let cx = ((p.x / width) * cols as f32)
            .floor()
            .clamp(0.0, cols as f32 - 1.0) as usize;
        let cy = ((p.y / height) * rows as f32)
            .floor()
            .clamp(0.0, rows as f32 - 1.0) as usize;
        counts[cy * cols + cx] += 1;
        total += 1;
    }
    if total == 0 {
        return 100.0;
    }
    let mean = total as f32 / counts.len() as f32;
    if mean <= f32::EPSILON {
        return 100.0;
    }
    let variance = counts
        .iter()
        .map(|&count| {
            let delta = count as f32 - mean;
            delta * delta
        })
        .sum::<f32>()
        / counts.len() as f32;
    let coefficient_of_variation = variance.sqrt() / mean;
    (100.0 / (1.0 + coefficient_of_variation)).clamp(0.0, 100.0)
}

fn weighted_overall(symmetry_score: f32, density_uniformity: f32, params: &AestheticParams) -> f32 {
    let symmetry_weight = params.symmetry_weight.max(0.0);
    let density_weight = params.density_weight.max(0.0);
    let total_weight = symmetry_weight + density_weight;
    if total_weight <= f32::EPSILON {
        return 0.0;
    }
    ((symmetry_score * symmetry_weight + density_uniformity * density_weight) / total_weight)
        .clamp(0.0, 100.0)
}

/// Coarsely searches for the mirror-axis x-coordinate that maximizes the
/// symmetry score, for formations deliberately designed off-center (e.g.
/// pushed toward one sideline so it is not centered on `grid.width / 2`).
///
/// Returns `(axis_x, score)`. Falls back to `(grid.width / 2, 0.0)` when
/// `set_index` is out of range or there is no usable data, matching the
/// non-panicking, no-set/no-performer handling of [`analyze_set`] (this
/// function does not return `Option` because a sensible default axis — the
/// field's own center — always exists even when there is nothing to score).
///
/// ## Why a fixed coarse grid instead of gradient descent or ternary search
///
/// The symmetry score is not unimodal in general (real formations commonly
/// have multiple local optima — e.g. a formation with two separate symmetric
/// sub-blocks), so hill-climbing could get stuck short of the global best.
/// A fixed sample count keeps the added cost bounded and predictable
/// instead: `SEARCH_STEPS` full symmetry evaluations, i.e.
/// `SEARCH_STEPS * n^2` distance checks total. With the default of 21 steps
/// and `n = 1,000` that is ~2.1e7 distance evaluations — tens of ms, well
/// inside budget for an explicit, user-triggered "check off-center
/// symmetry" action rather than something recomputed every frame.
pub fn best_symmetry_axis(document: &Document, set_index: usize) -> (f32, f32) {
    const SEARCH_STEPS: usize = 21;

    let width = sanitized_dimension(document.grid.width, 100.0);
    let fallback = (width * 0.5, 0.0);

    let Some(set) = document.sets.get(set_index) else {
        return fallback;
    };
    if set.positions.is_empty() || document.performers.is_empty() {
        return fallback;
    }
    let n = set.positions.len().min(document.performers.len());
    let positions = &set.positions[..n];
    let performers = &document.performers[..n];
    let height = sanitized_dimension(document.grid.height, 53.333);

    let mut best_axis = width * 0.5;
    let mut best_score = f32::NEG_INFINITY;
    for step in 0..SEARCH_STEPS {
        let axis_x = width * step as f32 / (SEARCH_STEPS - 1) as f32;
        // worst_offenders_len = 0: the search only needs the score, so skip
        // building and sorting the offenders list on every candidate axis.
        let detail = symmetry_score(performers, positions, axis_x, height, 0);
        if detail.score > best_score {
            best_score = detail.score;
            best_axis = axis_x;
        }
    }
    (best_axis, best_score.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Document;

    #[test]
    fn perfectly_symmetric_formation_scores_near_maximum() {
        let mut document = Document::demo(1, 4); // 4 performers, grid.width = 100.0
        document.sets[0].positions = vec![
            Point { x: 40.0, y: 10.0 },
            Point { x: 60.0, y: 10.0 },
            Point { x: 40.0, y: 20.0 },
            Point { x: 60.0, y: 20.0 },
        ];
        let score = analyze_set(&document, 0, &AestheticParams::default()).expect("set exists");
        assert!(
            score.symmetry.score > 99.9,
            "expected near-perfect symmetry, got {}",
            score.symmetry.score
        );
    }

    #[test]
    fn completely_asymmetric_formation_scores_low() {
        let mut document = Document::demo(1, 4);
        document.sets[0].positions = vec![
            Point { x: 10.0, y: 10.0 },
            Point { x: 15.0, y: 10.0 },
            Point { x: 20.0, y: 10.0 },
            Point { x: 25.0, y: 10.0 },
        ];
        let score = analyze_set(&document, 0, &AestheticParams::default()).expect("set exists");
        assert!(
            score.symmetry.score < 10.0,
            "expected low symmetry, got {}",
            score.symmetry.score
        );
        assert!(!score.symmetry.worst_offenders.is_empty());
    }

    #[test]
    fn uniform_grid_scores_high_density_uniformity() {
        let mut document = Document::demo(10, 10); // 100 performers
        let width = document.grid.width;
        let height = document.grid.height;
        let cols = 10usize;
        let rows = 10usize;
        let mut positions = Vec::with_capacity(cols * rows);
        for r in 0..rows {
            for c in 0..cols {
                positions.push(Point {
                    x: (c as f32 + 0.5) * (width / cols as f32),
                    y: (r as f32 + 0.5) * (height / rows as f32),
                });
            }
        }
        document.sets[0].positions = positions;
        let score = analyze_set(&document, 0, &AestheticParams::default()).expect("set exists");
        assert!(
            score.density_uniformity > 99.0,
            "expected near-perfect density uniformity, got {}",
            score.density_uniformity
        );
    }

    #[test]
    fn handles_degenerate_inputs_without_panicking() {
        let params = AestheticParams::default();

        let mut no_performers = Document::demo(1, 1);
        no_performers.performers.clear();
        assert!(analyze_set(&no_performers, 0, &params).is_none());
        let (axis, score) = best_symmetry_axis(&no_performers, 0);
        assert!(axis.is_finite());
        assert!(score.is_finite());

        let single = Document::demo(1, 1);
        assert!(analyze_set(&single, 0, &params).is_some());

        let mut nan_positions = Document::demo(1, 4);
        nan_positions.sets[0].positions[0].x = f32::NAN;
        nan_positions.sets[0].positions[1].y = f32::INFINITY;
        let score = analyze_set(&nan_positions, 0, &params).expect("still analyzable");
        assert!(score.overall.is_finite());
        assert!(score.symmetry.score.is_finite());
        assert!(score.density_uniformity.is_finite());

        assert!(analyze_set(&single, 42, &params).is_none());
    }

    #[test]
    fn thousand_performers_completes_without_panicking() {
        let document = Document::demo(25, 40); // 1,000 performers
        assert_eq!(document.performers.len(), 1_000);
        let score = analyze_set(&document, 0, &AestheticParams::default()).expect("set exists");
        assert!(score.overall.is_finite());
        assert!(score.symmetry.score.is_finite());
        assert!(score.density_uniformity.is_finite());

        let (axis, axis_score) = best_symmetry_axis(&document, 0);
        assert!(axis.is_finite());
        assert!(axis_score.is_finite());
    }

    #[test]
    fn analysis_is_deterministic() {
        let document = Document::demo(20, 20); // 400 performers
        let params = AestheticParams::default();
        let first = analyze_set(&document, 0, &params).expect("set exists");
        let second = analyze_set(&document, 0, &params).expect("set exists");
        assert_eq!(first, second);
    }
}
