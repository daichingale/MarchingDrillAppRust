//! Audience sight-line and formation readability diagnostics.
//!
//! The analyzer is UI/GPU independent and reuses all working storage after
//! warm-up. Angular buckets turn a full cast from quadratic pair scanning into
//! a bounded neighborhood query for ordinary stadium viewpoints.

use crate::{PerformerId, Point, camera::Camera};

const AZ_BINS: usize = 256;
const EL_BINS: usize = 128;
const BIN_COUNT: usize = AZ_BINS * EL_BINS;

#[derive(Clone, Debug, PartialEq)]
pub struct OcclusionResult {
    pub target: PerformerId,
    pub blocked_by: Vec<PerformerId>,
    pub visible_fraction: f32,
}

#[derive(Default, Debug)]
pub struct VisibilityScratch {
    bucket_head: Vec<i32>,
    bucket_next: Vec<i32>,
    directions: Vec<[f32; 3]>,
    distances: Vec<f32>,
    candidates: Vec<usize>,
    seen_generation: Vec<u32>,
    generation: u32,
}

impl VisibilityScratch {
    pub fn reserve(&mut self, performers: usize) {
        if self.bucket_head.len() != BIN_COUNT {
            self.bucket_head.resize(BIN_COUNT, -1);
        }
        self.bucket_next
            .reserve(performers.saturating_sub(self.bucket_next.capacity()));
        self.directions
            .reserve(performers.saturating_sub(self.directions.capacity()));
        self.distances
            .reserve(performers.saturating_sub(self.distances.capacity()));
        self.candidates
            .reserve(128_usize.saturating_sub(self.candidates.capacity()));
        self.seen_generation.resize(performers, 0);
    }

    pub fn capacities(&self) -> (usize, usize, usize) {
        (
            self.bucket_next.capacity(),
            self.directions.capacity(),
            self.candidates.capacity(),
        )
    }
}

pub fn visibility_from_seat(
    positions: &[Point],
    heights_m: &[f32],
    eye: [f32; 3],
    shoulder_radius_m: f32,
    scratch: &mut VisibilityScratch,
    out: &mut Vec<OcclusionResult>,
) {
    visibility_from_seat_with_ids(
        positions,
        heights_m,
        &[],
        eye,
        shoulder_radius_m,
        scratch,
        out,
    );
}

/// Stable-ID variant for documents whose performer IDs are not dense.
pub fn visibility_from_seat_with_ids(
    positions: &[Point],
    heights_m: &[f32],
    performer_ids: &[PerformerId],
    eye: [f32; 3],
    shoulder_radius_m: f32,
    scratch: &mut VisibilityScratch,
    out: &mut Vec<OcclusionResult>,
) {
    let count = positions
        .len()
        .min(heights_m.len())
        .min(crate::MAX_PERFORMERS);
    scratch.reserve(count);
    scratch.bucket_head.fill(-1);
    scratch.bucket_next.clear();
    scratch.directions.clear();
    scratch.distances.clear();
    for i in 0..count {
        let sight = [
            positions[i].x,
            heights_m[i].clamp(0.2, 3.0) * 0.85,
            positions[i].y,
        ];
        let v = sub(sight, eye);
        let d = norm(v);
        let dir = if d.is_finite() && d > 1.0e-5 {
            scale(v, 1.0 / d)
        } else {
            [0.0, 0.0, 0.0]
        };
        scratch.directions.push(dir);
        scratch.distances.push(d);
        let bin = direction_bin(dir);
        scratch.bucket_next.push(scratch.bucket_head[bin]);
        scratch.bucket_head[bin] = i as i32;
    }
    if out.len() > count {
        out.truncate(count);
    }
    while out.len() < count {
        let raw = out.len() as u32 + 1;
        out.push(OcclusionResult {
            target: PerformerId::new(raw).expect("bounded nonzero id"),
            blocked_by: Vec::new(),
            visible_fraction: 1.0,
        });
    }
    let shoulder = if shoulder_radius_m.is_finite() {
        shoulder_radius_m.clamp(0.05, 2.0)
    } else {
        0.25
    };
    for (target, result) in out.iter_mut().enumerate().take(count) {
        result.target = performer_ids
            .get(target)
            .copied()
            .unwrap_or_else(|| PerformerId::new(target as u32 + 1).unwrap());
        result.blocked_by.clear();
        result.visible_fraction = 1.0;
        let dist_t = scratch.distances[target];
        if !dist_t.is_finite() || dist_t <= 1.0e-5 {
            continue;
        }
        scratch.generation = scratch.generation.wrapping_add(1).max(1);
        if scratch.generation == 1 {
            scratch.seen_generation.fill(0);
        }
        scratch.candidates.clear();
        let (az, el) = direction_coords(scratch.directions[target]);
        // A 9x9 neighborhood covers a 5.6-degree shoulder footprint. Very
        // close performers use the correctness-first bounded full scan.
        let radius_bins = ((shoulder.atan2(dist_t.max(shoulder))
            / (std::f32::consts::TAU / AZ_BINS as f32))
            .ceil() as usize)
            .clamp(1, 8);
        if radius_bins == 8 {
            scratch.candidates.extend(0..count);
        } else {
            for de in -(radius_bins as isize)..=radius_bins as isize {
                let e = (el as isize + de).clamp(0, EL_BINS as isize - 1) as usize;
                for da in -(radius_bins as isize)..=radius_bins as isize {
                    let a = (az as isize + da).rem_euclid(AZ_BINS as isize) as usize;
                    let mut item = scratch.bucket_head[e * AZ_BINS + a];
                    while item >= 0 {
                        let idx = item as usize;
                        if scratch.seen_generation[idx] != scratch.generation {
                            scratch.seen_generation[idx] = scratch.generation;
                            scratch.candidates.push(idx);
                        }
                        item = scratch.bucket_next[idx];
                    }
                }
            }
        }
        for &other in &scratch.candidates {
            if other == target {
                continue;
            }
            let dist_o = scratch.distances[other];
            if !(dist_o > 1.0e-5 && dist_o < dist_t) {
                continue;
            }
            let angle = dot(scratch.directions[target], scratch.directions[other])
                .clamp(-1.0, 1.0)
                .acos();
            let angular_radius = shoulder.atan2(dist_o);
            if angle < angular_radius {
                result.blocked_by.push(
                    performer_ids
                        .get(other)
                        .copied()
                        .unwrap_or_else(|| PerformerId::new(other as u32 + 1).unwrap()),
                );
                let remaining = (angle / angular_radius.max(1.0e-6)).clamp(0.0, 1.0);
                result.visible_fraction = result.visible_fraction.min(remaining);
            }
        }
        result.blocked_by.sort_by(|a, b| {
            let ai = performer_ids
                .iter()
                .position(|id| id == a)
                .unwrap_or(a.get() as usize - 1);
            let bi = performer_ids
                .iter()
                .position(|id| id == b)
                .unwrap_or(b.get() as usize - 1);
            scratch.distances[ai].total_cmp(&scratch.distances[bi])
        });
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RowSpec {
    pub members: Vec<PerformerId>,
    pub expected_shape: RowShape,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RowShape {
    Straight,
    Arc { radius_m: f32 },
}
#[derive(Clone, Debug, PartialEq)]
pub struct RowFlatnessReport {
    pub field_space_residual_m: f32,
    pub screen_space_deviation_px: f32,
    pub depth_spread_ratio: f32,
    pub flags: Vec<RowFlag>,
}
#[derive(Clone, Debug, PartialEq)]
pub enum RowFlag {
    CrookedInField { residual_m: f32 },
    CrookedFromSeat { deviation_px: f32 },
    Telescoped { depth_spread_ratio: f32 },
    PartiallyOccluded { members: Vec<PerformerId> },
}

#[allow(clippy::too_many_arguments)] // Public contract mirrors design doc 22.
pub fn analyze_row_from_seat(
    positions: &[Point],
    heights_m: &[f32],
    row: &RowSpec,
    camera: &Camera,
    viewport: (f32, f32),
    crooked_px_threshold: f32,
    telescoped_ratio_threshold: f32,
    scratch: &mut VisibilityScratch,
) -> RowFlatnessReport {
    let indices: Vec<usize> = row
        .members
        .iter()
        .filter_map(|id| usize::try_from(id.get() - 1).ok())
        .filter(|&i| i < positions.len() && i < heights_m.len())
        .collect();
    if indices.len() < 2 {
        return RowFlatnessReport {
            field_space_residual_m: 0.0,
            screen_space_deviation_px: 0.0,
            depth_spread_ratio: 1.0,
            flags: Vec::new(),
        };
    }
    let points: Vec<Point> = indices.iter().map(|&i| positions[i]).collect();
    let field_residual = match row.expected_shape {
        RowShape::Straight => line_residual(&points),
        RowShape::Arc { radius_m } => arc_residual(&points, radius_m),
    };
    let projected: Vec<[f32; 2]> = indices
        .iter()
        .filter_map(|&i| {
            camera.project_point(positions[i], heights_m[i] * 0.85, viewport.0, viewport.1)
        })
        .collect();
    let screen_deviation = screen_line_residual(&projected);
    let eye = camera.position();
    let mut near = f32::INFINITY;
    let mut far = 0.0_f32;
    for &i in &indices {
        let d = norm(sub(
            [positions[i].x, heights_m[i] * 0.85, positions[i].y],
            eye,
        ));
        near = near.min(d);
        far = far.max(d);
    }
    let depth_spread = if near.is_finite() && near > 1.0e-5 {
        far / near
    } else {
        1.0
    };
    let mut visibility = Vec::new();
    visibility_from_seat(positions, heights_m, eye, 0.25, scratch, &mut visibility);
    let occluded: Vec<_> = indices
        .iter()
        .filter(|&&i| visibility.get(i).is_some_and(|r| r.visible_fraction < 0.5))
        .map(|&i| PerformerId::new(i as u32 + 1).unwrap())
        .collect();
    let mut flags = Vec::new();
    if field_residual > 0.15 {
        flags.push(RowFlag::CrookedInField {
            residual_m: field_residual,
        });
    }
    if screen_deviation > crooked_px_threshold.max(0.0) {
        flags.push(RowFlag::CrookedFromSeat {
            deviation_px: screen_deviation,
        });
    }
    if depth_spread > telescoped_ratio_threshold.max(1.0) {
        flags.push(RowFlag::Telescoped {
            depth_spread_ratio: depth_spread,
        });
    }
    if !occluded.is_empty() {
        flags.push(RowFlag::PartiallyOccluded { members: occluded });
    }
    RowFlatnessReport {
        field_space_residual_m: field_residual,
        screen_space_deviation_px: screen_deviation,
        depth_spread_ratio: depth_spread,
        flags,
    }
}

fn line_residual(p: &[Point]) -> f32 {
    let n = p.len() as f32;
    let mx = p.iter().map(|p| p.x).sum::<f32>() / n;
    let my = p.iter().map(|p| p.y).sum::<f32>() / n;
    let (xx, xy, yy) = p.iter().fold((0.0, 0.0, 0.0), |(xx, xy, yy), p| {
        let x = p.x - mx;
        let y = p.y - my;
        (xx + x * x, xy + x * y, yy + y * y)
    });
    let angle = 0.5 * (2.0 * xy).atan2(xx - yy);
    let normal = [-angle.sin(), angle.cos()];
    p.iter()
        .map(|p| ((p.x - mx) * normal[0] + (p.y - my) * normal[1]).abs())
        .fold(0.0, f32::max)
}
fn arc_residual(p: &[Point], radius: f32) -> f32 {
    let n = p.len() as f32;
    let c = [
        p.iter().map(|p| p.x).sum::<f32>() / n,
        p.iter().map(|p| p.y).sum::<f32>() / n,
    ];
    let mean = if radius.is_finite() && radius > 0.0 {
        radius
    } else {
        p.iter()
            .map(|p| ((p.x - c[0]).powi(2) + (p.y - c[1]).powi(2)).sqrt())
            .sum::<f32>()
            / n
    };
    p.iter()
        .map(|p| (((p.x - c[0]).powi(2) + (p.y - c[1]).powi(2)).sqrt() - mean).abs())
        .fold(0.0, f32::max)
}
fn screen_line_residual(p: &[[f32; 2]]) -> f32 {
    if p.len() < 3 {
        return 0.0;
    }
    let a = p[0];
    let b = p[p.len() - 1];
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-5 {
        return f32::INFINITY;
    }
    p[1..p.len() - 1]
        .iter()
        .map(|v| ((v[0] - a[0]) * dy - (v[1] - a[1]) * dx).abs() / len)
        .fold(0.0, f32::max)
}
fn direction_coords(d: [f32; 3]) -> (usize, usize) {
    let az = ((d[0].atan2(d[2]) + std::f32::consts::PI) / std::f32::consts::TAU * AZ_BINS as f32)
        .floor() as isize;
    let el = ((d[1].clamp(-1.0, 1.0).asin() + std::f32::consts::FRAC_PI_2) / std::f32::consts::PI
        * EL_BINS as f32)
        .floor() as isize;
    (
        az.rem_euclid(AZ_BINS as isize) as usize,
        el.clamp(0, EL_BINS as isize - 1) as usize,
    )
}
fn direction_bin(d: [f32; 3]) -> usize {
    let (a, e) = direction_coords(d);
    e * AZ_BINS + a
}
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn norm(a: [f32; 3]) -> f32 {
    dot(a, a).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nearer_performer_blocks_aligned_target() {
        let p = [Point { x: 0.0, y: 5.0 }, Point { x: 0.0, y: 10.0 }];
        let h = [1.7, 1.7];
        let mut s = VisibilityScratch::default();
        let mut out = Vec::new();
        visibility_from_seat(&p, &h, [0.0, 1.45, 0.0], 0.3, &mut s, &mut out);
        assert_eq!(out[1].blocked_by, vec![PerformerId::new(1).unwrap()]);
        assert!(out[1].visible_fraction < 0.1);
    }
    #[test]
    fn warm_call_keeps_capacities() {
        let p = vec![Point { x: 1.0, y: 1.0 }; 100];
        let h = vec![1.7; 100];
        let mut s = VisibilityScratch::default();
        let mut out = Vec::new();
        visibility_from_seat(&p, &h, [0.0, 5.0, 0.0], 0.3, &mut s, &mut out);
        let cap = s.capacities();
        visibility_from_seat(&p, &h, [0.0, 5.0, 0.0], 0.3, &mut s, &mut out);
        assert_eq!(cap, s.capacities());
    }
    #[test]
    fn straight_row_reports_outlier() {
        let p = [
            Point { x: 0.0, y: 0.0 },
            Point { x: 1.0, y: 0.5 },
            Point { x: 2.0, y: 0.0 },
        ];
        let h = [1.7; 3];
        let row = RowSpec {
            members: (1..=3).map(|i| PerformerId::new(i).unwrap()).collect(),
            expected_shape: RowShape::Straight,
        };
        let mut s = VisibilityScratch::default();
        let r = analyze_row_from_seat(
            &p,
            &h,
            &row,
            &Camera::at_eye([1.0, 5.0, -10.0], [1.0, 0.0, 0.0], 1.0),
            (1920.0, 1080.0),
            4.0,
            2.5,
            &mut s,
        );
        assert!(r.field_space_residual_m > 0.3);
        assert!(
            r.flags
                .iter()
                .any(|f| matches!(f, RowFlag::CrookedInField { .. }))
        );
    }
}
