//! Pure geometric editing transforms on a selection of points.
//!
//! Every function takes a `&[Point]` selection and returns a `Vec<Point>` of the
//! same length and order, so the caller can map results back to selection
//! indices. Inputs are never mutated. Field coordinates: `x` runs across the
//! field, `y` is depth.

use crate::Point;

/// Average position of the selection. Returns the origin for an empty slice.
pub fn centroid(points: &[Point]) -> Point {
    if points.is_empty() {
        return Point { x: 0.0, y: 0.0 };
    }
    let n = points.len() as f32;
    let (sx, sy) = points
        .iter()
        .fold((0.0f32, 0.0f32), |(ax, ay), p| (ax + p.x, ay + p.y));
    Point {
        x: sx / n,
        y: sy / n,
    }
}

/// Mean of the `y` coordinates (0.0 for an empty slice).
fn mean_y(points: &[Point]) -> f32 {
    if points.is_empty() {
        return 0.0;
    }
    points.iter().map(|p| p.y).sum::<f32>() / points.len() as f32
}

/// Mean of the `x` coordinates (0.0 for an empty slice).
fn mean_x(points: &[Point]) -> f32 {
    if points.is_empty() {
        return 0.0;
    }
    points.iter().map(|p| p.x).sum::<f32>() / points.len() as f32
}

/// Set every point's `y` to the selection's mean depth, keeping `x`.
/// Produces a horizontal line at the average depth.
pub fn align_horizontal(points: &[Point]) -> Vec<Point> {
    let y = mean_y(points);
    points.iter().map(|p| Point { x: p.x, y }).collect()
}

/// Set every point's `x` to the selection's mean, keeping `y`.
/// Produces a vertical line at the average horizontal position.
pub fn align_vertical(points: &[Point]) -> Vec<Point> {
    let x = mean_x(points);
    points.iter().map(|p| Point { x, y: p.y }).collect()
}

/// Respace the `x` coordinates evenly between the current min and max `x`,
/// keeping each point's `y`. Even spacing is assigned in left-to-right order,
/// then mapped back to the original selection order. Returns a copy when there
/// are fewer than 3 points (nothing to redistribute).
pub fn distribute_horizontal(points: &[Point]) -> Vec<Point> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut order: Vec<usize> = (0..points.len()).collect();
    order.sort_by(|&a, &b| points[a].x.total_cmp(&points[b].x));
    let min = points[order[0]].x;
    let max = points[*order.last().unwrap()].x;
    let step = (max - min) / (points.len() - 1) as f32;
    let mut result = points.to_vec();
    for (rank, &idx) in order.iter().enumerate() {
        result[idx].x = min + step * rank as f32;
    }
    result
}

/// Respace the `y` coordinates evenly between the current min and max `y`,
/// keeping each point's `x`. See [`distribute_horizontal`] for ordering rules.
pub fn distribute_vertical(points: &[Point]) -> Vec<Point> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut order: Vec<usize> = (0..points.len()).collect();
    order.sort_by(|&a, &b| points[a].y.total_cmp(&points[b].y));
    let min = points[order[0]].y;
    let max = points[*order.last().unwrap()].y;
    let step = (max - min) / (points.len() - 1) as f32;
    let mut result = points.to_vec();
    for (rank, &idx) in order.iter().enumerate() {
        result[idx].y = min + step * rank as f32;
    }
    result
}

/// Reflect every point's `x` across the vertical line `x = axis_x`.
pub fn mirror(points: &[Point], axis_x: f32) -> Vec<Point> {
    points
        .iter()
        .map(|p| Point {
            x: 2.0 * axis_x - p.x,
            y: p.y,
        })
        .collect()
}

/// Reflect every point's `y` across the horizontal line `y = axis_y`.
pub fn flip_vertical_axis(points: &[Point], axis_y: f32) -> Vec<Point> {
    points
        .iter()
        .map(|p| Point {
            x: p.x,
            y: 2.0 * axis_y - p.y,
        })
        .collect()
}

/// Mirror the selection about its centroid's `x` (horizontal flip in place).
pub fn flip_horizontal(points: &[Point]) -> Vec<Point> {
    mirror(points, centroid(points).x)
}

/// Flip the selection about its centroid's `y` (vertical flip in place).
pub fn flip_vertical(points: &[Point]) -> Vec<Point> {
    flip_vertical_axis(points, centroid(points).y)
}

/// Rotate the selection by `angle_rad` (counter-clockwise) about `pivot`.
pub fn rotate(points: &[Point], angle_rad: f32, pivot: Point) -> Vec<Point> {
    let (sin, cos) = angle_rad.sin_cos();
    points
        .iter()
        .map(|p| {
            let dx = p.x - pivot.x;
            let dy = p.y - pivot.y;
            Point {
                x: pivot.x + dx * cos - dy * sin,
                y: pivot.y + dx * sin + dy * cos,
            }
        })
        .collect()
}

/// Rotate the selection by `angle_rad` about its own centroid.
pub fn rotate_about_centroid(points: &[Point], angle_rad: f32) -> Vec<Point> {
    rotate(points, angle_rad, centroid(points))
}

/// Scale the selection by `(sx, sy)` about `pivot`.
pub fn scale(points: &[Point], sx: f32, sy: f32, pivot: Point) -> Vec<Point> {
    points
        .iter()
        .map(|p| Point {
            x: pivot.x + (p.x - pivot.x) * sx,
            y: pivot.y + (p.y - pivot.y) * sy,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    const EPS: f32 = 1e-4;

    fn close(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < EPS && (a.y - b.y).abs() < EPS
    }

    #[test]
    fn centroid_of_empty_is_origin() {
        assert_eq!(centroid(&[]), Point { x: 0.0, y: 0.0 });
    }

    #[test]
    fn centroid_averages_positions() {
        let pts = [
            Point { x: 0.0, y: 0.0 },
            Point { x: 4.0, y: 2.0 },
            Point { x: 2.0, y: 4.0 },
        ];
        assert!(close(centroid(&pts), Point { x: 2.0, y: 2.0 }));
    }

    #[test]
    fn align_horizontal_sets_all_y_to_mean() {
        let pts = [
            Point { x: 1.0, y: 0.0 },
            Point { x: 5.0, y: 6.0 },
            Point { x: 9.0, y: 3.0 },
        ];
        let out = align_horizontal(&pts);
        assert_eq!(out.len(), pts.len());
        for (o, p) in out.iter().zip(&pts) {
            assert!((o.y - 3.0).abs() < EPS);
            assert_eq!(o.x, p.x);
        }
    }

    #[test]
    fn align_vertical_sets_all_x_to_mean() {
        let pts = [
            Point { x: 0.0, y: 1.0 },
            Point { x: 6.0, y: 5.0 },
            Point { x: 3.0, y: 9.0 },
        ];
        let out = align_vertical(&pts);
        for (o, p) in out.iter().zip(&pts) {
            assert!((o.x - 3.0).abs() < EPS);
            assert_eq!(o.y, p.y);
        }
    }

    #[test]
    fn distribute_horizontal_makes_equal_gaps_and_keeps_endpoints_and_order() {
        // Unevenly spaced, collinear along x, in left-to-right order.
        let pts = [
            Point { x: 0.0, y: 1.0 },
            Point { x: 1.0, y: 2.0 },
            Point { x: 9.0, y: 3.0 },
        ];
        let out = distribute_horizontal(&pts);
        // Endpoints preserved.
        assert!((out[0].x - 0.0).abs() < EPS);
        assert!((out[2].x - 9.0).abs() < EPS);
        // Equal gaps.
        let g1 = out[1].x - out[0].x;
        let g2 = out[2].x - out[1].x;
        assert!((g1 - g2).abs() < EPS);
        assert!((out[1].x - 4.5).abs() < EPS);
        // y untouched, order preserved.
        for (o, p) in out.iter().zip(&pts) {
            assert_eq!(o.y, p.y);
        }
    }

    #[test]
    fn distribute_horizontal_restores_original_order_when_unsorted() {
        // Provided out of left-to-right order; result must respace by rank but
        // keep the input order/index alignment.
        let pts = [
            Point { x: 10.0, y: 0.0 },
            Point { x: 0.0, y: 0.0 },
            Point { x: 5.0, y: 0.0 },
        ];
        let out = distribute_horizontal(&pts);
        assert!((out[0].x - 10.0).abs() < EPS); // was rightmost -> max
        assert!((out[1].x - 0.0).abs() < EPS); // was leftmost -> min
        assert!((out[2].x - 5.0).abs() < EPS); // middle
    }

    #[test]
    fn distribute_vertical_makes_equal_gaps() {
        let pts = [
            Point { x: 1.0, y: 0.0 },
            Point { x: 2.0, y: 2.0 },
            Point { x: 3.0, y: 10.0 },
        ];
        let out = distribute_vertical(&pts);
        assert!((out[0].y - 0.0).abs() < EPS);
        assert!((out[2].y - 10.0).abs() < EPS);
        assert!((out[1].y - 5.0).abs() < EPS);
        for (o, p) in out.iter().zip(&pts) {
            assert_eq!(o.x, p.x);
        }
    }

    #[test]
    fn distribute_returns_copy_for_small_selections() {
        let pts = [Point { x: 0.0, y: 0.0 }, Point { x: 9.0, y: 1.0 }];
        assert_eq!(distribute_horizontal(&pts), pts.to_vec());
        assert_eq!(distribute_vertical(&pts), pts.to_vec());
    }

    #[test]
    fn mirror_is_its_own_inverse() {
        let pts = [
            Point { x: 3.0, y: 1.0 },
            Point { x: 7.0, y: 4.0 },
            Point { x: -2.0, y: 8.0 },
        ];
        let once = mirror(&pts, 5.0);
        let twice = mirror(&once, 5.0);
        for (o, p) in twice.iter().zip(&pts) {
            assert!(close(*o, *p));
        }
        // Sanity: reflection across x=5 sends x=3 to x=7.
        assert!((once[0].x - 7.0).abs() < EPS);
    }

    #[test]
    fn flip_vertical_axis_reflects_y() {
        let pts = [Point { x: 1.0, y: 2.0 }];
        let out = flip_vertical_axis(&pts, 10.0);
        assert!(close(out[0], Point { x: 1.0, y: 18.0 }));
    }

    #[test]
    fn rotate_by_pi_reflects_through_pivot() {
        let pivot = Point { x: 1.0, y: 1.0 };
        let pts = [Point { x: 4.0, y: 5.0 }];
        let out = rotate(&pts, PI, pivot);
        // 180 deg about (1,1) maps (4,5) -> (-2,-3).
        assert!(close(out[0], Point { x: -2.0, y: -3.0 }));
    }

    #[test]
    fn rotate_quarter_turn_about_origin() {
        let out = rotate(
            &[Point { x: 1.0, y: 0.0 }],
            PI / 2.0,
            Point { x: 0.0, y: 0.0 },
        );
        assert!(close(out[0], Point { x: 0.0, y: 1.0 }));
    }

    #[test]
    fn rotate_about_centroid_preserves_centroid() {
        let pts = [
            Point { x: 0.0, y: 0.0 },
            Point { x: 4.0, y: 0.0 },
            Point { x: 2.0, y: 3.0 },
        ];
        let before = centroid(&pts);
        let after = centroid(&rotate_about_centroid(&pts, 0.7));
        assert!(close(before, after));
    }

    #[test]
    fn flip_horizontal_twice_is_identity() {
        let pts = [
            Point { x: 2.0, y: 1.0 },
            Point { x: 8.0, y: 5.0 },
            Point { x: 5.0, y: -3.0 },
        ];
        let out = flip_horizontal(&flip_horizontal(&pts));
        for (o, p) in out.iter().zip(&pts) {
            assert!(close(*o, *p));
        }
    }

    #[test]
    fn flip_vertical_twice_is_identity() {
        let pts = [
            Point { x: 2.0, y: 1.0 },
            Point { x: 8.0, y: 5.0 },
            Point { x: 5.0, y: -3.0 },
        ];
        let out = flip_vertical(&flip_vertical(&pts));
        for (o, p) in out.iter().zip(&pts) {
            assert!(close(*o, *p));
        }
    }

    #[test]
    fn scale_expands_about_pivot() {
        let pts = [Point { x: 2.0, y: 2.0 }];
        let out = scale(&pts, 2.0, 3.0, Point { x: 0.0, y: 0.0 });
        assert!(close(out[0], Point { x: 4.0, y: 6.0 }));
    }
}
