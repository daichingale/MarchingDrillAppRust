//! "Show DNA" spatial analysis: how much of the show's duration each part of
//! the field was occupied by performers.
//!
//! This module deliberately performs analysis only — no rendering. Grid cell
//! values are exposed as a flat array so any renderer (egui/SVG/PDF/GPU) can
//! turn them into pixels however it likes, without `drill-core` depending on
//! `drill-render` (see the crate boundary rule documented at the workspace
//! root).

use crate::{Document, PerformerId, Point};

/// Grid cells are clamped to at least 1 per axis (never zero, never a panic)
/// and capped so a hostile/typo'd request (e.g. `u16::MAX` on both axes)
/// cannot exhaust memory.
const MAX_CELLS_PER_AXIS: u16 = 2048;

/// Upper bound on samples drawn from a single set-to-set transition,
/// regardless of `HeatmapParams::samples_per_count`. Keeps the analysis
/// bounded and fast even for pathological parameters.
const MAX_SAMPLES_PER_TRANSITION: usize = 512;

/// Field occupancy accumulated over a show (or a slice of it): for each cell
/// of a `cells_x` x `cells_y` grid over the field, the summed
/// "count x performers" dwell time spent inside that cell.
///
/// Values are not normalized; use [`FieldOccupancy::max`] /
/// [`FieldOccupancy::normalized`] for display purposes.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldOccupancy {
    pub cells_x: u16,
    pub cells_y: u16,
    pub grid_width: f32,
    pub grid_height: f32,
    /// Row-major, length `cells_x * cells_y`. Row 0 corresponds to
    /// `y in [0, cell_height)`, i.e. the same field-coordinate convention as
    /// `GridConfig`/`Point` (y grows away from the front sideline). Renderers
    /// are responsible for mapping that into screen space (see
    /// `drill_render::FieldMap`, which flips y already for on-field dots).
    values: Vec<f32>,
}

impl FieldOccupancy {
    fn new(cells_x: u16, cells_y: u16, grid_width: f32, grid_height: f32) -> Self {
        let cells_x = cells_x.clamp(1, MAX_CELLS_PER_AXIS);
        let cells_y = cells_y.clamp(1, MAX_CELLS_PER_AXIS);
        let grid_width = if grid_width.is_finite() && grid_width > 0.0 {
            grid_width
        } else {
            1.0
        };
        let grid_height = if grid_height.is_finite() && grid_height > 0.0 {
            grid_height
        } else {
            1.0
        };
        let len = cells_x as usize * cells_y as usize;
        Self {
            cells_x,
            cells_y,
            grid_width,
            grid_height,
            values: vec![0.0; len],
        }
    }

    #[inline]
    fn index(&self, cx: u16, cy: u16) -> Option<usize> {
        if cx >= self.cells_x || cy >= self.cells_y {
            return None;
        }
        Some(cy as usize * self.cells_x as usize + cx as usize)
    }

    /// Raw accumulated dwell value for a cell. Out-of-range indices return 0
    /// rather than panicking.
    pub fn get(&self, cx: u16, cy: u16) -> f32 {
        self.index(cx, cy).map_or(0.0, |i| self.values[i])
    }

    /// Largest cell value across the whole grid (0.0 if the grid is empty of
    /// activity, e.g. an empty performer selection).
    pub fn max(&self) -> f32 {
        self.values.iter().copied().fold(0.0_f32, f32::max)
    }

    /// Cell value scaled into `0.0..=1.0` against [`FieldOccupancy::max`].
    pub fn normalized(&self, cx: u16, cy: u16) -> f32 {
        let max = self.max();
        if max <= f32::EPSILON {
            return 0.0;
        }
        (self.get(cx, cy) / max).clamp(0.0, 1.0)
    }

    /// Field-space width/height of one grid cell.
    pub fn cell_size(&self) -> (f32, f32) {
        (
            self.grid_width / self.cells_x as f32,
            self.grid_height / self.cells_y as f32,
        )
    }

    /// Field-space `(min, max)` corners of a cell, useful for renderers that
    /// draw one rectangle per cell. Does not validate `cx`/`cy` are in range.
    pub fn cell_bounds(&self, cx: u16, cy: u16) -> (Point, Point) {
        let (w, h) = self.cell_size();
        let min = Point {
            x: cx as f32 * w,
            y: cy as f32 * h,
        };
        let max = Point {
            x: min.x + w,
            y: min.y + h,
        };
        (min, max)
    }

    /// Adds `weight` of dwell time to whichever cell `point` falls in.
    /// Points outside the field bounds, or a non-positive/non-finite weight,
    /// are silently dropped rather than panicking or wrapping around.
    fn accumulate(&mut self, point: Point, weight: f32) {
        if !point.x.is_finite()
            || !point.y.is_finite()
            || !weight.is_finite()
            || weight <= 0.0
            || point.x < 0.0
            || point.y < 0.0
            || point.x > self.grid_width
            || point.y > self.grid_height
        {
            return;
        }
        let (cell_w, cell_h) = self.cell_size();
        let cx = ((point.x / cell_w.max(f32::EPSILON)) as usize).min(self.cells_x as usize - 1);
        let cy = ((point.y / cell_h.max(f32::EPSILON)) as usize).min(self.cells_y as usize - 1);
        let idx = cy * self.cells_x as usize + cx;
        self.values[idx] += weight;
    }
}

/// Parameters controlling grid resolution and sampling density for
/// [`analyze_show_occupancy`] / [`analyze_occupancy_for`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeatmapParams {
    pub cells_x: u16,
    pub cells_y: u16,
    /// Sampling density along each transition. `1.0` samples roughly once
    /// per count; higher values sample more densely. Non-finite or
    /// non-positive values fall back to `1.0`. Regardless of this value, at
    /// most [`MAX_SAMPLES_PER_TRANSITION`] samples are drawn from any single
    /// transition, so pathological inputs cannot blow up runtime.
    pub samples_per_count: f32,
}

impl Default for HeatmapParams {
    fn default() -> Self {
        Self {
            cells_x: 40,
            cells_y: 24,
            samples_per_count: 1.0,
        }
    }
}

/// Computes field occupancy across the entire show (all sets, all
/// transitions) for every performer in the document.
pub fn analyze_show_occupancy(document: &Document, params: &HeatmapParams) -> FieldOccupancy {
    let indices: Vec<usize> = (0..document.performers.len()).collect();
    analyze_indices(document, &indices, params)
}

/// Computes field occupancy restricted to the given performers (e.g. one
/// section or an ad-hoc subset). Unknown ids are ignored.
pub fn analyze_occupancy_for(
    document: &Document,
    performer_ids: &[PerformerId],
    params: &HeatmapParams,
) -> FieldOccupancy {
    let indices: Vec<usize> = performer_ids
        .iter()
        .filter_map(|id| {
            document
                .performers
                .iter()
                .position(|performer| performer.id == *id)
        })
        .collect();
    analyze_indices(document, &indices, params)
}

fn analyze_indices(document: &Document, indices: &[usize], params: &HeatmapParams) -> FieldOccupancy {
    let mut occupancy = FieldOccupancy::new(
        params.cells_x,
        params.cells_y,
        document.grid.width,
        document.grid.height,
    );
    if indices.is_empty() {
        return occupancy;
    }
    if document.sets.len() < 2 {
        // No transitions to sample: the whole show is just this one static
        // formation, so it counts as full dwell time in its own cells.
        if let Some(set) = document.sets.first() {
            for &idx in indices {
                if let Some(&p) = set.positions.get(idx) {
                    occupancy.accumulate(p, 1.0);
                }
            }
        }
        return occupancy;
    }

    let samples_per_count = if params.samples_per_count.is_finite() && params.samples_per_count > 0.0
    {
        params.samples_per_count
    } else {
        1.0
    };

    let mut plan = crate::transition::TransitionPlan::default();
    let mut buffer: Vec<Point> = Vec::new();
    for set in document.sets.iter().take(document.sets.len() - 1) {
        document.plan_transition(set.id, &mut plan);
        let total = plan.counts().total() as f32;
        let requested = (total * samples_per_count).round() as i64 + 1;
        let sample_count = requested.clamp(1, MAX_SAMPLES_PER_TRANSITION as i64) as usize;
        // Distribute this transition's total dwell time evenly across the
        // samples we actually take, so the sum stays meaningful (~total
        // counts) even when MAX_SAMPLES_PER_TRANSITION caps a dense request.
        let weight = if total > 0.0 {
            total / sample_count as f32
        } else {
            1.0
        };
        for i in 0..sample_count {
            let local_count = if sample_count <= 1 {
                0.0
            } else {
                total * i as f32 / (sample_count - 1) as f32
            };
            crate::transition::eval(&plan, local_count, &mut buffer);
            for &idx in indices {
                if let Some(&p) = buffer.get(idx) {
                    occupancy.accumulate(p, weight);
                }
            }
        }
    }
    occupancy
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Document, Set, SetId};

    /// Builds a document with `performer_count` performers (reusing
    /// `Document::demo`'s valid roster/section plumbing) and replaces its
    /// sets with the caller-supplied per-set position lists.
    fn document_with_sets(performer_count: usize, set_positions: &[Vec<Point>], counts: u16) -> Document {
        let mut doc = Document::demo(1, performer_count.max(1));
        doc.performers.truncate(performer_count);
        doc.sets = set_positions
            .iter()
            .enumerate()
            .map(|(i, positions)| Set {
                id: SetId::new((i + 1) as u32).unwrap(),
                name: format!("Set {}", i + 1),
                annotation: Default::default(),
                counts,
                hold: 0,
                routes: Default::default(),
                shape: None,
                positions: positions.clone(),
            })
            .collect();
        doc
    }

    #[test]
    fn stationary_show_peaks_at_a_single_cell() {
        let still = Point { x: 37.5, y: 20.0 };
        let sets = vec![vec![still; 4], vec![still; 4], vec![still; 4]];
        let doc = document_with_sets(4, &sets, 8);
        let params = HeatmapParams {
            cells_x: 20,
            cells_y: 12,
            samples_per_count: 1.0,
        };
        let occ = analyze_show_occupancy(&doc, &params);

        let cell_w = occ.grid_width / occ.cells_x as f32;
        let cell_h = occ.grid_height / occ.cells_y as f32;
        let hot_cx = (still.x / cell_w) as u16;
        let hot_cy = (still.y / cell_h) as u16;

        let hot_value = occ.get(hot_cx, hot_cy);
        assert!(hot_value > 0.0);
        assert_eq!(hot_value, occ.max());
        assert_eq!(occ.normalized(hot_cx, hot_cy), 1.0);

        let mut other_total = 0.0_f32;
        for cy in 0..occ.cells_y {
            for cx in 0..occ.cells_x {
                if (cx, cy) != (hot_cx, hot_cy) {
                    other_total += occ.get(cx, cy);
                }
            }
        }
        assert_eq!(other_total, 0.0);
    }

    #[test]
    fn sweeping_performer_distributes_roughly_evenly() {
        // A single performer travels in a straight line across the full
        // width of the field. With a 1-row grid this should visit every
        // column roughly equally often.
        let doc = document_with_sets(
            1,
            &[
                vec![Point { x: 0.0, y: 26.0 }],
                vec![Point { x: 100.0, y: 26.0 }],
            ],
            100,
        );
        let params = HeatmapParams {
            cells_x: 10,
            cells_y: 1,
            samples_per_count: 1.0,
        };
        let occ = analyze_show_occupancy(&doc, &params);

        let values: Vec<f32> = (0..occ.cells_x).map(|cx| occ.get(cx, 0)).collect();
        let mean = values.iter().sum::<f32>() / values.len() as f32;
        assert!(mean > 0.0);
        for value in &values {
            // Within 60% of the mean: not perfectly uniform (edge cells can
            // get a slightly different share) but nowhere near "all in one
            // cell".
            assert!(
                (*value - mean).abs() < mean * 0.6,
                "value {value} too far from mean {mean}"
            );
        }
    }

    #[test]
    fn extreme_cell_counts_do_not_panic() {
        let doc = document_with_sets(
            2,
            &[
                vec![Point { x: 10.0, y: 10.0 }, Point { x: 20.0, y: 20.0 }],
                vec![Point { x: 15.0, y: 15.0 }, Point { x: 25.0, y: 25.0 }],
            ],
            10,
        );

        let one_by_one = analyze_show_occupancy(
            &doc,
            &HeatmapParams {
                cells_x: 1,
                cells_y: 1,
                samples_per_count: 1.0,
            },
        );
        assert_eq!(one_by_one.cells_x, 1);
        assert_eq!(one_by_one.cells_y, 1);
        assert!(one_by_one.max() > 0.0);

        let huge = analyze_show_occupancy(
            &doc,
            &HeatmapParams {
                cells_x: 1000,
                cells_y: 1000,
                samples_per_count: 1.0,
            },
        );
        assert_eq!(huge.cells_x, 1000);
        assert_eq!(huge.cells_y, 1000);
        assert!(huge.max() > 0.0);

        let zeroed = analyze_show_occupancy(
            &doc,
            &HeatmapParams {
                cells_x: 0,
                cells_y: 0,
                samples_per_count: 1.0,
            },
        );
        assert_eq!(zeroed.cells_x, 1);
        assert_eq!(zeroed.cells_y, 1);

        let capped = analyze_show_occupancy(
            &doc,
            &HeatmapParams {
                cells_x: u16::MAX,
                cells_y: u16::MAX,
                samples_per_count: 1.0,
            },
        );
        assert!(capped.cells_x <= MAX_CELLS_PER_AXIS);
        assert!(capped.cells_y <= MAX_CELLS_PER_AXIS);
    }

    #[test]
    fn large_cast_and_show_completes_quickly_without_panicking() {
        let performer_count = 1_000;
        let set_count = 64;
        let sets: Vec<Vec<Point>> = (0..set_count)
            .map(|s| {
                (0..performer_count)
                    .map(|p| {
                        let x = ((s * 7 + p) % 100) as f32;
                        let y = ((s * 3 + p) % 53) as f32;
                        Point { x, y }
                    })
                    .collect()
            })
            .collect();
        let doc = document_with_sets(performer_count, &sets, 16);

        let started = std::time::Instant::now();
        let occ = analyze_show_occupancy(&doc, &HeatmapParams::default());
        let elapsed = started.elapsed();

        assert!(occ.max() > 0.0);
        assert!(
            elapsed.as_millis() < 5_000,
            "analysis took too long: {elapsed:?}"
        );
    }

    #[test]
    fn unknown_and_empty_performer_selection_is_safe() {
        let doc = document_with_sets(
            2,
            &[
                vec![Point { x: 10.0, y: 10.0 }, Point { x: 20.0, y: 20.0 }],
                vec![Point { x: 15.0, y: 15.0 }, Point { x: 25.0, y: 25.0 }],
            ],
            10,
        );
        let params = HeatmapParams::default();
        let empty = analyze_occupancy_for(&doc, &[], &params);
        assert_eq!(empty.max(), 0.0);

        let unknown = analyze_occupancy_for(&doc, &[PerformerId::new(9_999).unwrap()], &params);
        assert_eq!(unknown.max(), 0.0);
    }
}
