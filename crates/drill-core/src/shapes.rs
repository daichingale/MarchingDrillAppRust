//! Pure formation generators complementing the line/arc helpers in [`crate`].
//!
//! Every generator returns a `Vec<Point>` and, unless noted otherwise, yields
//! EXACTLY `count` points. `count == 0` produces an empty vector and `count == 1`
//! produces a single, sensibly-placed point (mirroring [`crate::evenly_spaced_line`]).
//! All functions are pure and allocate a single output `Vec` where practical.

use crate::{DrillError, PerformerId, Point, SectionId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::f32::consts::TAU;

/// A rectangular block grid of `cols * rows` points in row-major order.
///
/// The first point is `top_left`; `dx` advances along columns (x) and `dy`
/// advances along rows (y). The returned length is exactly `cols * rows`, so an
/// empty dimension yields an empty vector.
pub fn block(top_left: Point, cols: usize, rows: usize, dx: f32, dy: f32) -> Vec<Point> {
    let mut out = Vec::with_capacity(cols * rows);
    for r in 0..rows {
        for c in 0..cols {
            out.push(Point {
                x: top_left.x + c as f32 * dx,
                y: top_left.y + r as f32 * dy,
            });
        }
    }
    out
}

/// A `cols * rows` block sized to evenly fill the rectangle spanned by
/// `rect_min`..`rect_max` (row-major, corners on the rectangle edges).
///
/// With a single column/row the points are centered on that axis, matching the
/// midpoint behaviour of [`crate::evenly_spaced_line`] for `count == 1`.
pub fn block_fit(rect_min: Point, rect_max: Point, cols: usize, rows: usize) -> Vec<Point> {
    let mut out = Vec::with_capacity(cols * rows);
    let span = |min: f32, max: f32, i: usize, n: usize| -> f32 {
        match n {
            0 => min,
            1 => (min + max) * 0.5,
            _ => min + (max - min) * i as f32 / (n - 1) as f32,
        }
    };
    for r in 0..rows {
        let y = span(rect_min.y, rect_max.y, r, rows);
        for c in 0..cols {
            out.push(Point {
                x: span(rect_min.x, rect_max.x, c, cols),
                y,
            });
        }
    }
    out
}

/// A closed circle of `count` points evenly distributed over the full 2π,
/// with no duplicated endpoint.
///
/// Points start at the top (−y) and proceed clockwise. `count == 1` returns the
/// top point.
pub fn circle(center: Point, radius: f32, count: usize) -> Vec<Point> {
    (0..count)
        .map(|i| {
            // Start at the top (angle −π/2 in standard math coords) and step
            // clockwise. In field coords y grows downward, so clockwise on
            // screen corresponds to increasing standard angle.
            let angle = -std::f32::consts::FRAC_PI_2 + TAU * i as f32 / count.max(1) as f32;
            Point {
                x: center.x + radius * angle.cos(),
                y: center.y + radius * angle.sin(),
            }
        })
        .collect()
}

/// An Archimedean spiral of `count` points from `start_radius` to `end_radius`
/// over `turns` full revolutions.
///
/// Radius and angle vary linearly in the curve parameter, then the result is
/// redistributed by arc length so adjacent performers have uniform spacing.
/// `count == 1` returns the spiral midpoint.
pub fn spiral(
    center: Point,
    start_radius: f32,
    end_radius: f32,
    turns: f32,
    count: usize,
) -> Vec<Point> {
    let sample = |t: f32| {
        let radius = start_radius + (end_radius - start_radius) * t;
        let angle = -std::f32::consts::FRAC_PI_2 + TAU * turns * t;
        Point {
            x: center.x + radius * angle.cos(),
            y: center.y + radius * angle.sin(),
        }
    };
    match count {
        0 => Vec::new(),
        1 => vec![sample(0.5)],
        _ => sample_curve_by_arc_length(count, sample),
    }
}

/// Points sampled evenly by arc length along a Bézier curve of arbitrary degree,
/// evaluated with De Casteljau's algorithm.
///
/// Returns an empty vector when `control_points` is empty; when there is a
/// single control point, returns `count` copies of it. Otherwise the returned
/// length is exactly `count`, spanning `t = 0..=1` (endpoints preserved).
pub fn bezier(control_points: &[Point], count: usize) -> Vec<Point> {
    if control_points.is_empty() {
        return Vec::new();
    }
    if control_points.len() == 1 {
        return vec![control_points[0]; count];
    }
    // Scratch buffer reused across samples for the De Casteljau reduction.
    let mut scratch = Vec::with_capacity(control_points.len());
    let mut de_casteljau = |t: f32| {
        scratch.clear();
        scratch.extend_from_slice(control_points);
        let mut len = scratch.len();
        while len > 1 {
            for i in 0..len - 1 {
                scratch[i] = scratch[i].lerp(scratch[i + 1], t);
            }
            len -= 1;
        }
        scratch[0]
    };
    match count {
        0 => Vec::new(),
        1 => vec![de_casteljau(0.5)],
        _ => sample_curve_by_arc_length(count, de_casteljau),
    }
}

/// Approximate a parametric curve densely, then use the exact polyline
/// arc-length distributor. The bounded density keeps formation tools
/// deterministic while making the discretization error much smaller than the
/// field-grid snap resolution.
fn sample_curve_by_arc_length(count: usize, mut sample: impl FnMut(f32) -> Point) -> Vec<Point> {
    debug_assert!(count >= 2);
    let segments = count.saturating_mul(32).clamp(256, 32_768);
    let mut dense = Vec::with_capacity(segments + 1);
    for i in 0..=segments {
        dense.push(sample(i as f32 / segments as f32));
    }
    polyline(&dense, count)
}

pub const MAX_SHAPE_VERTICES: usize = 1_024;
pub const MAX_RAW_PATH_POINTS: usize = 4_096;

/// Bounded Douglas-Peucker simplification for pointer input. Non-finite
/// samples are discarded and output is deterministically capped.
pub fn simplify_free_path(points: &[Point], tolerance: f32) -> Vec<Point> {
    let finite = points
        .iter()
        .copied()
        .filter(|p| p.x.is_finite() && p.y.is_finite())
        .take(MAX_RAW_PATH_POINTS)
        .collect::<Vec<_>>();
    if finite.len() <= 2 {
        return finite;
    }
    let tolerance = tolerance.max(0.0);
    let mut keep = vec![false; finite.len()];
    keep[0] = true;
    keep[finite.len() - 1] = true;
    let mut stack = vec![(0usize, finite.len() - 1)];
    while let Some((start, end)) = stack.pop() {
        let a = finite[start];
        let b = finite[end];
        let vx = b.x - a.x;
        let vy = b.y - a.y;
        let denom = vx * vx + vy * vy;
        let mut farthest = (0.0f32, None);
        for (offset, p) in finite[start + 1..end].iter().enumerate() {
            let t = if denom > f32::EPSILON {
                (((p.x - a.x) * vx + (p.y - a.y) * vy) / denom).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let projected = Point {
                x: a.x + vx * t,
                y: a.y + vy * t,
            };
            let d = distance(*p, projected);
            if d > farthest.0 {
                farthest = (d, Some(start + 1 + offset));
            }
        }
        if farthest.0 > tolerance
            && let Some(index) = farthest.1
        {
            keep[index] = true;
            stack.push((index, end));
            stack.push((start, index));
        }
    }
    finite
        .into_iter()
        .zip(keep)
        .filter_map(|(point, keep)| keep.then_some(point))
        .take(MAX_SHAPE_VERTICES)
        .collect()
}

/// Persistable, editable source geometry for a formation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ShapeSpec {
    Line {
        start: Point,
        end: Point,
    },
    Arc {
        center: Point,
        radius: f32,
        start_angle: f32,
        end_angle: f32,
    },
    Circle {
        center: Point,
        radius: f32,
    },
    Ellipse {
        center: Point,
        radius_x: f32,
        radius_y: f32,
        rotation: f32,
    },
    Block {
        top_left: Point,
        cols: usize,
        rows: usize,
        dx: f32,
        dy: f32,
    },
    BlockFit {
        rect_min: Point,
        rect_max: Point,
        cols: usize,
        rows: usize,
    },
    Spiral {
        center: Point,
        start_radius: f32,
        end_radius: f32,
        turns: f32,
    },
    Bezier {
        control_points: Vec<Point>,
    },
    Parabola {
        vertex: Point,
        curvature: f32,
        half_width: f32,
        rotation: f32,
    },
    SineWave {
        start: Point,
        end: Point,
        amplitude: f32,
        cycles: f32,
        phase: f32,
    },
    Star {
        center: Point,
        outer_radius: f32,
        inner_radius: f32,
        points: u32,
        rotation: f32,
    },
    Polygon {
        center: Point,
        radius: f32,
        sides: u32,
        rotation: f32,
    },
    Cross {
        center: Point,
        arm_length: f32,
        arm_width: f32,
    },
    FreePath {
        vertices: Vec<Point>,
    },
    Text {
        contours: Vec<Vec<Point>>,
    },
}

impl ShapeSpec {
    pub fn validate(&self) -> Result<(), DrillError> {
        let finite = |p: Point| p.x.is_finite() && p.y.is_finite();
        let points_ok =
            |p: &[Point]| p.len() <= MAX_SHAPE_VERTICES && p.iter().copied().all(&finite);
        let ok = match self {
            Self::Line { start, end } => finite(*start) && finite(*end),
            Self::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => {
                finite(*center)
                    && *radius >= 0.0
                    && radius.is_finite()
                    && start_angle.is_finite()
                    && end_angle.is_finite()
            }
            Self::Circle { center, radius } => {
                finite(*center) && *radius >= 0.0 && radius.is_finite()
            }
            Self::Ellipse {
                center,
                radius_x,
                radius_y,
                rotation,
            } => {
                finite(*center)
                    && *radius_x >= 0.0
                    && *radius_y >= 0.0
                    && radius_x.is_finite()
                    && radius_y.is_finite()
                    && rotation.is_finite()
            }
            Self::Block {
                top_left,
                cols,
                rows,
                dx,
                dy,
            } => {
                finite(*top_left)
                    && cols.saturating_mul(*rows) <= crate::MAX_PERFORMERS
                    && dx.is_finite()
                    && dy.is_finite()
            }
            Self::BlockFit {
                rect_min,
                rect_max,
                cols,
                rows,
            } => {
                finite(*rect_min)
                    && finite(*rect_max)
                    && cols.saturating_mul(*rows) <= crate::MAX_PERFORMERS
            }
            Self::Spiral {
                center,
                start_radius,
                end_radius,
                turns,
            } => {
                finite(*center)
                    && *start_radius >= 0.0
                    && *end_radius >= 0.0
                    && start_radius.is_finite()
                    && end_radius.is_finite()
                    && turns.is_finite()
            }
            Self::Bezier { control_points } => {
                points_ok(control_points) && !control_points.is_empty()
            }
            Self::Parabola {
                vertex,
                curvature,
                half_width,
                rotation,
            } => {
                finite(*vertex)
                    && curvature.is_finite()
                    && *half_width >= 0.0
                    && half_width.is_finite()
                    && rotation.is_finite()
            }
            Self::SineWave {
                start,
                end,
                amplitude,
                cycles,
                phase,
            } => {
                finite(*start)
                    && finite(*end)
                    && amplitude.is_finite()
                    && cycles.is_finite()
                    && phase.is_finite()
            }
            Self::Star {
                center,
                outer_radius,
                inner_radius,
                points,
                rotation,
            } => {
                finite(*center)
                    && *outer_radius >= 0.0
                    && *inner_radius >= 0.0
                    && outer_radius.is_finite()
                    && inner_radius.is_finite()
                    && (2..=512).contains(points)
                    && rotation.is_finite()
            }
            Self::Polygon {
                center,
                radius,
                sides,
                rotation,
            } => {
                finite(*center)
                    && *radius >= 0.0
                    && radius.is_finite()
                    && (3..=1024).contains(sides)
                    && rotation.is_finite()
            }
            Self::Cross {
                center,
                arm_length,
                arm_width,
            } => {
                finite(*center)
                    && *arm_length >= 0.0
                    && *arm_width >= 0.0
                    && arm_length.is_finite()
                    && arm_width.is_finite()
            }
            Self::FreePath { vertices } => points_ok(vertices) && !vertices.is_empty(),
            Self::Text { contours } => {
                contours.len() <= 256
                    && contours.iter().all(|c| points_ok(c))
                    && contours.iter().map(Vec::len).sum::<usize>() <= MAX_SHAPE_VERTICES
            }
        };
        if ok {
            Ok(())
        } else {
            Err(DrillError::InvalidEdit)
        }
    }

    /// Samples exactly `count` points in deterministic arc-length order.
    pub fn sample(&self, count: usize, out: &mut Vec<Point>) {
        out.clear();
        if count == 0 {
            return;
        }
        let curve =
            |eval: &mut dyn FnMut(f32) -> Point| sample_curve_by_arc_length(count.max(2), eval);
        let mut points = match self {
            Self::Line { start, end } => crate::evenly_spaced_line(*start, *end, count),
            Self::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => {
                crate::evenly_spaced_arc(*center, radius.max(0.0), *start_angle, *end_angle, count)
            }
            Self::Circle { center, radius } => circle(*center, radius.max(0.0), count),
            Self::Ellipse {
                center,
                radius_x,
                radius_y,
                rotation,
            } => curve(&mut |t| {
                rotate_local(
                    *center,
                    radius_x.max(0.0) * (TAU * t).cos(),
                    radius_y.max(0.0) * (TAU * t).sin(),
                    *rotation,
                )
            }),
            Self::Block {
                top_left,
                cols,
                rows,
                dx,
                dy,
            } => {
                let mut p = block(*top_left, *cols, *rows, *dx, *dy);
                p.truncate(count);
                p.resize(count, *top_left);
                p
            }
            Self::BlockFit {
                rect_min,
                rect_max,
                cols,
                rows,
            } => {
                let mut p = block_fit(*rect_min, *rect_max, *cols, *rows);
                p.truncate(count);
                p.resize(count, rect_min.lerp(*rect_max, 0.5));
                p
            }
            Self::Spiral {
                center,
                start_radius,
                end_radius,
                turns,
            } => spiral(
                *center,
                start_radius.max(0.0),
                end_radius.max(0.0),
                *turns,
                count,
            ),
            Self::Bezier { control_points } => bezier(control_points, count),
            Self::Parabola {
                vertex,
                curvature,
                half_width,
                rotation,
            } => curve(&mut |t| {
                let x = (t * 2.0 - 1.0) * half_width.max(0.0);
                rotate_local(*vertex, x, *curvature * x * x, *rotation)
            }),
            Self::SineWave {
                start,
                end,
                amplitude,
                cycles,
                phase,
            } => {
                let dx = end.x - start.x;
                let dy = end.y - start.y;
                let len = (dx * dx + dy * dy).sqrt().max(f32::EPSILON);
                curve(&mut |t| {
                    let b = start.lerp(*end, t);
                    let d = *amplitude * (*phase + TAU * *cycles * t).sin();
                    Point {
                        x: b.x - dy / len * d,
                        y: b.y + dx / len * d,
                    }
                })
            }
            Self::Star {
                center,
                outer_radius,
                inner_radius,
                points,
                rotation,
            } => closed_polyline(
                &star_outline(
                    *center,
                    outer_radius.max(0.0),
                    inner_radius.max(0.0),
                    *points,
                    *rotation,
                ),
                count,
            ),
            Self::Polygon {
                center,
                radius,
                sides,
                rotation,
            } => closed_polyline(
                &polygon_outline(*center, radius.max(0.0), *sides, *rotation),
                count,
            ),
            Self::Cross {
                center,
                arm_length,
                arm_width,
            } => closed_polyline(
                &cross_outline(*center, arm_length.max(0.0), arm_width.max(0.0)),
                count,
            ),
            Self::FreePath { vertices } => polyline(vertices, count),
            Self::Text { contours } => sample_text(contours, count),
        };
        points.truncate(count);
        if points.len() != count {
            points.resize(count, points.last().copied().unwrap_or_default());
        }
        out.append(&mut points);
    }
}

fn rotate_local(c: Point, x: f32, y: f32, a: f32) -> Point {
    let (s, k) = a.sin_cos();
    Point {
        x: c.x + x * k - y * s,
        y: c.y + x * s + y * k,
    }
}
fn star_outline(c: Point, ro: f32, ri: f32, n: u32, r: f32) -> Vec<Point> {
    let n = (n.max(2) * 2) as usize;
    (0..n)
        .map(|i| {
            let a = r - std::f32::consts::FRAC_PI_2 + TAU * i as f32 / n as f32;
            let q = if i % 2 == 0 { ro } else { ri };
            Point {
                x: c.x + q * a.cos(),
                y: c.y + q * a.sin(),
            }
        })
        .collect()
}
fn polygon_outline(c: Point, radius: f32, n: u32, r: f32) -> Vec<Point> {
    let n = n.max(3) as usize;
    (0..n)
        .map(|i| {
            let a = r - std::f32::consts::FRAC_PI_2 + TAU * i as f32 / n as f32;
            Point {
                x: c.x + radius * a.cos(),
                y: c.y + radius * a.sin(),
            }
        })
        .collect()
}
fn cross_outline(c: Point, l: f32, w: f32) -> Vec<Point> {
    let w = w * 0.5;
    [
        (w, -l),
        (w, -w),
        (l, -w),
        (l, w),
        (w, w),
        (w, l),
        (-w, l),
        (-w, w),
        (-l, w),
        (-l, -w),
        (-w, -w),
        (-w, -l),
    ]
    .map(|(x, y)| Point {
        x: c.x + x,
        y: c.y + y,
    })
    .to_vec()
}

pub fn closed_polyline(v: &[Point], count: usize) -> Vec<Point> {
    if v.is_empty() || count == 0 {
        return Vec::new();
    }
    let mut q = v.to_vec();
    q.push(v[0]);
    let open = polyline(&q, count + 1);
    open.into_iter().take(count).collect()
}
fn perimeter(v: &[Point]) -> f32 {
    if v.len() < 2 {
        return 0.0;
    }
    (0..v.len())
        .map(|i| distance(v[i], v[(i + 1) % v.len()]))
        .sum()
}
fn sample_text(contours: &[Vec<Point>], count: usize) -> Vec<Point> {
    let lengths: Vec<_> = contours.iter().map(|c| perimeter(c)).collect();
    let total: f32 = lengths.iter().sum();
    if total <= 0.0 {
        return vec![Point::default(); count];
    }
    let mut shares: Vec<_> = lengths
        .iter()
        .map(|l| (l / total * count as f32).floor() as usize)
        .collect();
    let mut order: Vec<_> = (0..contours.len()).collect();
    order.sort_by(|&a, &b| {
        ((lengths[b] / total * count as f32).fract())
            .total_cmp(&(lengths[a] / total * count as f32).fract())
            .then(a.cmp(&b))
    });
    for i in order.into_iter().take(count - shares.iter().sum::<usize>()) {
        shares[i] += 1
    }
    let mut out = Vec::with_capacity(count);
    for (c, n) in contours.iter().zip(shares) {
        out.extend(closed_polyline(c, n))
    }
    out
}
fn distance(a: Point, b: Point) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineFit {
    pub point: Point,
    pub direction: Point,
}
pub fn fit_line(p: &[Point]) -> Option<LineFit> {
    if p.len() < 2 || p.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
        return None;
    }
    let c = crate::editing::centroid(p);
    let (mut xx, mut xy, mut yy) = (0.0, 0.0, 0.0);
    for p in p {
        let x = p.x - c.x;
        let y = p.y - c.y;
        xx += x * x;
        xy += x * y;
        yy += y * y
    }
    let l = ((xx + yy) + ((xx - yy).powi(2) + 4.0 * xy * xy).sqrt()) * 0.5;
    if l < 1e-8 {
        return None;
    }
    let (d, e) = if xy.abs() > 1e-12 {
        (xy, l - xx)
    } else if xx >= yy {
        (1.0, 0.0)
    } else {
        (0.0, 1.0)
    };
    let z = (d * d + e * e).sqrt();
    Some(LineFit {
        point: c,
        direction: Point { x: d / z, y: e / z },
    })
}
pub fn snap_to_line(p: &[Point], f: &LineFit) -> Vec<Point> {
    p.iter()
        .map(|p| {
            let t = (p.x - f.point.x) * f.direction.x + (p.y - f.point.y) * f.direction.y;
            Point {
                x: f.point.x + t * f.direction.x,
                y: f.point.y + t * f.direction.y,
            }
        })
        .collect()
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CircleFit {
    pub center: Point,
    pub radius: f32,
}
pub fn fit_circle(p: &[Point]) -> Option<CircleFit> {
    if p.len() < 3 {
        return None;
    }
    let a = p[0];
    let mut best = None;
    for pair in p[1..].windows(2) {
        let (b, c) = (pair[0], pair[1]);
        let d = 2. * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
        if d.abs() > 1e-6 {
            let aa = a.x * a.x + a.y * a.y;
            let bb = b.x * b.x + b.y * b.y;
            let cc = c.x * c.x + c.y * c.y;
            let center = Point {
                x: (aa * (b.y - c.y) + bb * (c.y - a.y) + cc * (a.y - b.y)) / d,
                y: (aa * (c.x - b.x) + bb * (a.x - c.x) + cc * (b.x - a.x)) / d,
            };
            best = Some(CircleFit {
                center,
                radius: p.iter().map(|q| distance(*q, center)).sum::<f32>() / p.len() as f32,
            });
            break;
        }
    }
    best
}
pub fn snap_to_circle(p: &[Point], f: &CircleFit) -> Vec<Point> {
    p.iter()
        .map(|p| {
            let dx = p.x - f.center.x;
            let dy = p.y - f.center.y;
            let l = (dx * dx + dy * dy).sqrt();
            if l < 1e-6 {
                Point {
                    x: f.center.x + f.radius,
                    y: f.center.y,
                }
            } else {
                Point {
                    x: f.center.x + dx / l * f.radius,
                    y: f.center.y + dy / l * f.radius,
                }
            }
        })
        .collect()
}

pub fn morph(from: &[Point], to: &[Point], t: f32) -> Vec<Point> {
    if from.len() != to.len() {
        return Vec::new();
    }
    let assignment = crate::pathing::optimal_assignment(from, to);
    from.iter()
        .enumerate()
        .map(|(i, p)| p.lerp(to[assignment[i]], t.clamp(0., 1.)))
        .collect()
}
pub fn apply_radial(
    center: Point,
    fold: u32,
    groups: &[Vec<PerformerId>],
    positions: &mut BTreeMap<PerformerId, Point>,
) {
    let fold = fold.max(2);
    for g in groups {
        let Some(master) = g.first() else { continue };
        let Some(p) = positions.get(master).copied() else {
            continue;
        };
        for (k, id) in g.iter().take(fold as usize).enumerate() {
            positions.insert(
                *id,
                rotate_local(
                    center,
                    p.x - center.x,
                    p.y - center.y,
                    TAU * k as f32 / fold as f32,
                ),
            );
        }
    }
}
#[derive(Clone, Debug)]
pub struct AssignmentGroup {
    pub section: SectionId,
    pub performers: Vec<PerformerId>,
}
pub fn assign_to_shape(
    groups: &[AssignmentGroup],
    targets: &[Point],
    current: &BTreeMap<PerformerId, Point>,
) -> (BTreeMap<PerformerId, Point>, Vec<PerformerId>) {
    let mut out = BTreeMap::new();
    let mut unplaced = Vec::new();
    let mut cursor = 0;
    for g in groups {
        let take = g.performers.len().min(targets.len().saturating_sub(cursor));
        let ids = &g.performers[..take];
        let from: Vec<_> = ids
            .iter()
            .map(|id| current.get(id).copied().unwrap_or_default())
            .collect();
        let run = &targets[cursor..cursor + take];
        let a = crate::pathing::optimal_assignment(&from, run);
        for (i, id) in ids.iter().enumerate() {
            out.insert(*id, run[a[i]]);
        }
        unplaced.extend_from_slice(&g.performers[take..]);
        cursor += take
    }
    (out, unplaced)
}

/// Distribute `count` points evenly by ARC LENGTH along the connected polyline
/// through `vertices`, so spacing is uniform even with uneven segment lengths.
///
/// Endpoints are preserved. Returns empty for `count == 0` or empty input;
/// `count == 1` returns the first vertex. A single vertex (or a zero-length
/// path) yields `count` copies of that vertex.
pub fn polyline(vertices: &[Point], count: usize) -> Vec<Point> {
    if vertices.is_empty() || count == 0 {
        return Vec::new();
    }
    if vertices.len() == 1 {
        return vec![vertices[0]; count];
    }
    if count == 1 {
        return vec![vertices[0]];
    }
    // Cumulative arc length at each vertex.
    let mut cumulative = Vec::with_capacity(vertices.len());
    let mut total = 0.0f32;
    cumulative.push(0.0f32);
    for pair in vertices.windows(2) {
        let dx = pair[1].x - pair[0].x;
        let dy = pair[1].y - pair[0].y;
        total += (dx * dx + dy * dy).sqrt();
        cumulative.push(total);
    }
    // Degenerate zero-length path: all vertices coincide.
    if total == 0.0 {
        return vec![vertices[0]; count];
    }
    let mut out = Vec::with_capacity(count);
    let mut seg = 0usize;
    for i in 0..count {
        let target = total * i as f32 / (count - 1) as f32;
        // Advance to the segment containing `target`.
        while seg + 2 < vertices.len() && cumulative[seg + 1] < target {
            seg += 1;
        }
        let seg_start = cumulative[seg];
        let seg_len = cumulative[seg + 1] - seg_start;
        let local = if seg_len > 0.0 {
            (target - seg_start) / seg_len
        } else {
            0.0
        };
        out.push(vertices[seg].lerp(vertices[seg + 1], local));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evenly_spaced_line;

    const EPS: f32 = 1e-4;

    fn close(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < EPS && (a.y - b.y).abs() < EPS
    }

    #[test]
    fn block_has_expected_count_and_corners() {
        let tl = Point { x: 1.0, y: 2.0 };
        let pts = block(tl, 4, 3, 2.0, 5.0);
        assert_eq!(pts.len(), 12);
        // Top-left corner.
        assert_eq!(pts[0], tl);
        // Top-right corner (row 0, last col).
        assert_eq!(
            pts[3],
            Point {
                x: 1.0 + 3.0 * 2.0,
                y: 2.0
            }
        );
        // Bottom-left corner (last row, col 0).
        assert_eq!(
            pts[8],
            Point {
                x: 1.0,
                y: 2.0 + 2.0 * 5.0
            }
        );
        // Bottom-right corner.
        assert_eq!(pts[11], Point { x: 7.0, y: 12.0 });
    }

    #[test]
    fn block_fit_fills_rectangle_corners() {
        let min = Point { x: 0.0, y: 0.0 };
        let max = Point { x: 30.0, y: 20.0 };
        let pts = block_fit(min, max, 4, 3);
        assert_eq!(pts.len(), 12);
        assert!(close(pts[0], min));
        assert!(close(pts[3], Point { x: 30.0, y: 0.0 }));
        assert!(close(pts[8], Point { x: 0.0, y: 20.0 }));
        assert!(close(pts[11], max));
    }

    #[test]
    fn block_empty_dimension_is_empty() {
        assert!(block(Point::default(), 0, 5, 1.0, 1.0).is_empty());
        assert!(block(Point::default(), 5, 0, 1.0, 1.0).is_empty());
    }

    #[test]
    fn circle_points_lie_on_radius() {
        let center = Point { x: 10.0, y: -3.0 };
        let pts = circle(center, 7.0, 16);
        assert_eq!(pts.len(), 16);
        for p in &pts {
            let r = ((p.x - center.x).powi(2) + (p.y - center.y).powi(2)).sqrt();
            assert!((r - 7.0).abs() < 1e-3, "radius {r}");
        }
        // First point is at the top (−y).
        assert!(close(
            pts[0],
            Point {
                x: center.x,
                y: center.y - 7.0
            }
        ));
        // Going clockwise means the second point moves to +x (screen right).
        assert!(pts[1].x > center.x);
    }

    #[test]
    fn circle_count_zero_and_one() {
        assert!(circle(Point::default(), 5.0, 0).is_empty());
        let one = circle(Point { x: 2.0, y: 2.0 }, 5.0, 1);
        assert_eq!(one.len(), 1);
        assert!(close(one[0], Point { x: 2.0, y: -3.0 }));
    }

    #[test]
    fn spiral_endpoints_have_expected_radii() {
        let center = Point { x: 0.0, y: 0.0 };
        let pts = spiral(center, 1.0, 10.0, 3.0, 50);
        assert_eq!(pts.len(), 50);
        let r0 = (pts[0].x.powi(2) + pts[0].y.powi(2)).sqrt();
        let rn = (pts[49].x.powi(2) + pts[49].y.powi(2)).sqrt();
        assert!((r0 - 1.0).abs() < 1e-3);
        assert!((rn - 10.0).abs() < 1e-3);
    }

    #[test]
    fn spiral_count_zero_and_one() {
        assert!(spiral(Point::default(), 1.0, 5.0, 2.0, 0).is_empty());
        assert_eq!(spiral(Point::default(), 1.0, 5.0, 2.0, 1).len(), 1);
    }

    #[test]
    fn bezier_two_points_matches_straight_line() {
        let a = Point { x: 0.0, y: 5.0 };
        let b = Point { x: 10.0, y: -5.0 };
        let curve = bezier(&[a, b], 7);
        let line = evenly_spaced_line(a, b, 7);
        assert_eq!(curve.len(), line.len());
        for (c, l) in curve.iter().zip(&line) {
            assert!(close(*c, *l), "{c:?} vs {l:?}");
        }
    }

    #[test]
    fn bezier_preserves_endpoints() {
        let ctrl = [
            Point { x: 0.0, y: 0.0 },
            Point { x: 5.0, y: 10.0 },
            Point { x: 10.0, y: 0.0 },
        ];
        let pts = bezier(&ctrl, 9);
        assert_eq!(pts.len(), 9);
        assert!(close(pts[0], ctrl[0]));
        assert!(close(pts[8], ctrl[2]));
    }

    #[test]
    fn bezier_spacing_is_uniform_by_arc_length() {
        let points = bezier(
            &[
                Point { x: 0.0, y: 0.0 },
                Point { x: 1.0, y: 12.0 },
                Point { x: 20.0, y: 12.0 },
            ],
            25,
        );
        assert_uniform_chord_spacing(&points, 0.025);
    }

    #[test]
    fn spiral_spacing_is_uniform_by_arc_length() {
        let start_radius = 1.0;
        let end_radius = 12.0;
        let turns = 2.5;
        let points = spiral(Point::default(), start_radius, end_radius, turns, 40);
        // Chords naturally shorten near the tight inner turns, so recover the
        // curve parameter from the radius and integrate each original-curve
        // interval instead of comparing chord lengths.
        let ts: Vec<f32> = points
            .iter()
            .map(|point| {
                let radius = (point.x * point.x + point.y * point.y).sqrt();
                (radius - start_radius) / (end_radius - start_radius)
            })
            .collect();
        let arc_distances: Vec<f32> = ts
            .windows(2)
            .map(|pair| spiral_arc_length(start_radius, end_radius, turns, pair[0], pair[1]))
            .collect();
        assert_uniform_distances(&arc_distances, 0.002);
    }

    fn assert_uniform_chord_spacing(points: &[Point], relative_tolerance: f32) {
        let distances: Vec<f32> = points
            .windows(2)
            .map(|pair| {
                let dx = pair[1].x - pair[0].x;
                let dy = pair[1].y - pair[0].y;
                (dx * dx + dy * dy).sqrt()
            })
            .collect();
        assert_uniform_distances(&distances, relative_tolerance);
    }

    fn assert_uniform_distances(distances: &[f32], relative_tolerance: f32) {
        let mean = distances.iter().sum::<f32>() / distances.len() as f32;
        let max_deviation = distances
            .iter()
            .map(|distance| (distance - mean).abs())
            .fold(0.0, f32::max);
        assert!(
            max_deviation / mean <= relative_tolerance,
            "relative spacing deviation {} exceeded {}",
            max_deviation / mean,
            relative_tolerance
        );
    }

    fn spiral_arc_length(
        start_radius: f32,
        end_radius: f32,
        turns: f32,
        from: f32,
        to: f32,
    ) -> f32 {
        let steps = 256;
        let dr = end_radius - start_radius;
        let angular_rate = TAU * turns;
        let dt = (to - from) / steps as f32;
        (0..steps)
            .map(|i| {
                let t = from + (i as f32 + 0.5) * dt;
                let radius = start_radius + dr * t;
                (dr * dr + radius * radius * angular_rate * angular_rate).sqrt() * dt
            })
            .sum()
    }

    #[test]
    fn bezier_empty_and_single() {
        assert!(bezier(&[], 5).is_empty());
        let single = bezier(&[Point { x: 3.0, y: 4.0 }], 4);
        assert_eq!(single.len(), 4);
        for p in single {
            assert_eq!(p, Point { x: 3.0, y: 4.0 });
        }
        assert!(bezier(&[Point { x: 1.0, y: 1.0 }], 0).is_empty());
    }

    #[test]
    fn polyline_preserves_endpoints_and_count() {
        let verts = [
            Point { x: 0.0, y: 0.0 },
            Point { x: 3.0, y: 0.0 },
            Point { x: 3.0, y: 4.0 }, // uneven segment lengths: 3 then 4
        ];
        let pts = polyline(&verts, 8);
        assert_eq!(pts.len(), 8);
        assert!(close(pts[0], verts[0]));
        assert!(close(pts[7], verts[2]));
    }

    #[test]
    fn polyline_spacing_is_uniform_by_arc_length() {
        // Collinear vertices with uneven segment lengths (2 then 5): the
        // Euclidean gap between consecutive samples equals the arc-length gap,
        // so uniform arc-length spacing is directly observable.
        let verts = [
            Point { x: 0.0, y: 1.0 },
            Point { x: 2.0, y: 1.0 },
            Point { x: 7.0, y: 1.0 },
        ];
        let pts = polyline(&verts, 8);
        // Total length 7, 7 gaps => each gap 1.0.
        for pair in pts.windows(2) {
            let d = ((pair[1].x - pair[0].x).powi(2) + (pair[1].y - pair[0].y).powi(2)).sqrt();
            assert!((d - 1.0).abs() < 1e-3, "gap {d}");
        }
    }

    #[test]
    fn polyline_edge_cases() {
        assert!(polyline(&[], 5).is_empty());
        assert!(polyline(&[Point { x: 1.0, y: 1.0 }], 0).is_empty());
        let one_vert = polyline(&[Point { x: 1.0, y: 1.0 }], 3);
        assert_eq!(one_vert, vec![Point { x: 1.0, y: 1.0 }; 3]);
        let single = polyline(&[Point { x: 0.0, y: 0.0 }, Point { x: 4.0, y: 0.0 }], 1);
        assert_eq!(single, vec![Point { x: 0.0, y: 0.0 }]);
    }

    #[test]
    fn every_parametric_shape_is_bounded_finite_and_exact_count() {
        let c = Point { x: 10.0, y: 20.0 };
        let specs = vec![
            ShapeSpec::Ellipse {
                center: c,
                radius_x: 8.0,
                radius_y: 3.0,
                rotation: 0.3,
            },
            ShapeSpec::Parabola {
                vertex: c,
                curvature: 0.1,
                half_width: 8.0,
                rotation: 0.2,
            },
            ShapeSpec::SineWave {
                start: Point::default(),
                end: c,
                amplitude: 3.0,
                cycles: 2.0,
                phase: 0.0,
            },
            ShapeSpec::Star {
                center: c,
                outer_radius: 8.0,
                inner_radius: 4.0,
                points: 5,
                rotation: 0.0,
            },
            ShapeSpec::Polygon {
                center: c,
                radius: 7.0,
                sides: 6,
                rotation: 0.0,
            },
            ShapeSpec::Cross {
                center: c,
                arm_length: 8.0,
                arm_width: 3.0,
            },
            ShapeSpec::FreePath {
                vertices: vec![Point::default(), c],
            },
            ShapeSpec::Text {
                contours: vec![vec![
                    Point::default(),
                    Point { x: 2.0, y: 0.0 },
                    Point { x: 1.0, y: 2.0 },
                ]],
            },
        ];
        for spec in specs {
            spec.validate().unwrap();
            let mut out = Vec::new();
            spec.sample(73, &mut out);
            assert_eq!(out.len(), 73);
            assert!(out.iter().all(|p| p.x.is_finite() && p.y.is_finite()));
        }
    }

    #[test]
    fn invalid_shapes_are_rejected_without_allocation_bomb() {
        assert!(
            ShapeSpec::Circle {
                center: Point::default(),
                radius: -1.0
            }
            .validate()
            .is_err()
        );
        assert!(
            ShapeSpec::Polygon {
                center: Point::default(),
                radius: 2.0,
                sides: u32::MAX,
                rotation: 0.0
            }
            .validate()
            .is_err()
        );
        assert!(
            ShapeSpec::FreePath {
                vertices: vec![Point::default(); MAX_SHAPE_VERTICES + 1]
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn free_path_simplification_is_finite_bounded_and_keeps_corners() {
        let mut raw = (0..5_000)
            .map(|i| Point {
                x: i as f32 * 0.01,
                y: 0.0,
            })
            .collect::<Vec<_>>();
        raw[100] = Point {
            x: f32::NAN,
            y: 0.0,
        };
        raw[2_000] = Point { x: 20.0, y: 10.0 };
        let simplified = simplify_free_path(&raw, 0.05);
        assert!(simplified.len() <= MAX_SHAPE_VERTICES);
        assert!(
            simplified
                .iter()
                .all(|p| p.x.is_finite() && p.y.is_finite())
        );
        assert!(simplified.iter().any(|p| p.y > 9.0));
    }

    #[test]
    fn fitting_and_morph_workflows_preserve_identity_count() {
        let points = [
            Point { x: -2.0, y: -1.9 },
            Point::default(),
            Point { x: 2.0, y: 2.1 },
        ];
        let line = fit_line(&points).unwrap();
        assert!(
            snap_to_line(&points, &line)
                .windows(2)
                .all(|w| distance(w[0], w[1]) > 0.0)
        );
        let circle = [
            Point { x: 1.0, y: 0.0 },
            Point { x: 0.0, y: 1.0 },
            Point { x: -1.0, y: 0.0 },
            Point { x: 0.0, y: -1.0 },
        ];
        let fit = fit_circle(&circle).unwrap();
        assert!((fit.radius - 1.0).abs() < 1e-4);
        let halfway = morph(&points, &snap_to_line(&points, &line), 0.5);
        assert_eq!(halfway.len(), points.len());
    }
}
