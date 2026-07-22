//! Pure formation generators complementing the line/arc helpers in [`crate`].
//!
//! Every generator returns a `Vec<Point>` and, unless noted otherwise, yields
//! EXACTLY `count` points. `count == 0` produces an empty vector and `count == 1`
//! produces a single, sensibly-placed point (mirroring [`crate::evenly_spaced_line`]).
//! All functions are pure and allocate a single output `Vec` where practical.

use crate::Point;
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
/// Both radius and angle vary linearly in the sample parameter. `count == 1`
/// returns the spiral midpoint.
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
        _ => (0..count)
            .map(|i| sample(i as f32 / (count - 1) as f32))
            .collect(),
    }
}

/// Points sampled evenly in the parameter `t` along a Bézier curve of arbitrary
/// degree, evaluated with De Casteljau's algorithm.
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
        _ => (0..count)
            .map(|i| de_casteljau(i as f32 / (count - 1) as f32))
            .collect(),
    }
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
}
