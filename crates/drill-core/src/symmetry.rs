//! Left-right pairing across a field's center line.
//!
//! Full-mode radial symmetry ([`crate::shapes::apply_radial`]) rotates a
//! selection in place. This module answers a different question: which
//! existing performer, in this set, stands opposite someone else, and where
//! that partner should go when the first person moves. It reuses
//! [`crate::editing::mirror`] for the reflection and never stores a pairing
//! on the document.

use crate::Point;
use crate::editing;

/// Half a horizontal step. A dot snapped onto the lattice can sit this far
/// from the geometric center line, or from a perfect mirror, and still read
/// as "on the line" / "already mirrored".
pub fn center_tolerance(step: f32) -> f32 {
    if step.is_finite() && step > 0.0 {
        step * 0.5
    } else {
        1e-3
    }
}

/// `point` stands on the vertical center line, within `tolerance`.
pub fn on_center_line(point: Point, axis_x: f32, tolerance: f32) -> bool {
    if !point.x.is_finite() {
        return false;
    }
    let tolerance = if tolerance.is_finite() && tolerance > 0.0 {
        tolerance
    } else {
        0.0
    };
    (point.x - axis_x).abs() <= tolerance
}

/// Partner index for every performer. `None` means unpaired: they stand on
/// the center line, or nobody on the opposite side is left to match.
///
/// Off-center performers are matched greedily. The cost of a pair is the
/// distance from the right-hand performer to the mirror of the left-hand
/// one, so "nearest" means nearest to where the mirror would stand, not
/// merely the closest body across the line. Each performer is used once.
/// Equal costs break toward the lower index.
pub fn pair_across_center(points: &[Point], axis_x: f32, tolerance: f32) -> Vec<Option<usize>> {
    let n = points.len();
    let mut partner = vec![None; n];
    let mut left = Vec::new();
    let mut right = Vec::new();
    for (index, point) in points.iter().enumerate() {
        if !point.x.is_finite() || !point.y.is_finite() {
            continue;
        }
        if on_center_line(*point, axis_x, tolerance) {
            continue;
        }
        if point.x < axis_x {
            left.push(index);
        } else {
            right.push(index);
        }
    }
    let mut candidates = Vec::new();
    for &left_index in &left {
        let mirrored = editing::mirror(std::slice::from_ref(&points[left_index]), axis_x);
        let Some(target) = mirrored.first().copied() else {
            continue;
        };
        for &right_index in &right {
            let dx = points[right_index].x - target.x;
            let dy = points[right_index].y - target.y;
            candidates.push((dx.hypot(dy), left_index, right_index));
        }
    }
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    let mut left_used = vec![false; n];
    let mut right_used = vec![false; n];
    for &(_, left_index, right_index) in &candidates {
        if left_used[left_index] || right_used[right_index] {
            continue;
        }
        left_used[left_index] = true;
        right_used[right_index] = true;
        partner[left_index] = Some(right_index);
        partner[right_index] = Some(left_index);
    }
    partner
}

/// How many performers are off the center line and not sitting on their
/// partner's mirror (or have no partner at all). A misaligned pair counts
/// as two people. Center-line performers are not counted.
pub fn unmirrored_count(points: &[Point], axis_x: f32, tolerance: f32) -> usize {
    let partners = pair_across_center(points, axis_x, tolerance);
    let mut count = 0;
    for (index, point) in points.iter().enumerate() {
        if !point.x.is_finite() || !point.y.is_finite() || on_center_line(*point, axis_x, tolerance)
        {
            continue;
        }
        let mirrored = match partners[index] {
            Some(other) => points.get(other).is_none_or(|other_point| {
                mirror_gap(*point, *other_point, axis_x) > tolerance + 1e-3
            }),
            None => true,
        };
        if mirrored {
            count += 1;
        }
    }
    count
}

fn mirror_gap(point: Point, other: Point, axis_x: f32) -> f32 {
    let mirrored = editing::mirror(std::slice::from_ref(&point), axis_x);
    let Some(target) = mirrored.first() else {
        return f32::MAX;
    };
    (other.x - target.x).hypot(other.y - target.y)
}

fn reflect(point: Point, axis_x: f32) -> Point {
    editing::mirror(std::slice::from_ref(&point), axis_x)
        .first()
        .copied()
        .unwrap_or(point)
}

/// Positions after the user moves `driven` performers.
///
/// - A driven performer on the center line keeps the requested depth and
///   stays on the line (no sideways travel).
/// - Their partner, if they have one, moves to the mirror of the source
///   performer's new position.
/// - When both members of a pair are driven, the one who started on the
///   left is the source and the other is overwritten to the mirror. When
///   only one is driven, that one is the source.
/// - An unpaired performer moves alone, to the requested position.
///
/// The result is not snapped. Callers snap and clamp the way any other
/// formation edit does.
pub fn mirror_move(
    current: &[Point],
    axis_x: f32,
    tolerance: f32,
    driven: &[(usize, Point)],
) -> Vec<Point> {
    let mut next = current.to_vec();
    if current.is_empty() || driven.is_empty() {
        return next;
    }
    let partners = pair_across_center(current, axis_x, tolerance);
    let mut driven_set = vec![false; current.len()];
    for &(index, mut point) in driven {
        if index >= current.len() || !point.x.is_finite() || !point.y.is_finite() {
            continue;
        }
        if on_center_line(current[index], axis_x, tolerance) {
            point.x = axis_x;
        }
        next[index] = point;
        driven_set[index] = true;
    }
    let mut seen = vec![false; current.len()];
    for index in 0..current.len() {
        let Some(other) = partners[index] else {
            continue;
        };
        if seen[index] {
            continue;
        }
        seen[index] = true;
        seen[other] = true;
        let index_driven = driven_set[index];
        let other_driven = driven_set[other];
        if !index_driven && !other_driven {
            continue;
        }
        let source = if index_driven && !other_driven {
            index
        } else if other_driven && !index_driven {
            other
        } else if current[index].x < current[other].x
            || ((current[index].x - current[other].x).abs() <= f32::EPSILON && index < other)
        {
            index
        } else {
            other
        };
        let slave = if source == index { other } else { index };
        next[slave] = reflect(next[source], axis_x);
    }
    next
}

/// Move the opposite side onto the mirrors of `master_is_left`.
///
/// People on the chosen side stay where they are. Their partners move to
/// the exact mirror. Center-line performers and unpaired people stay.
pub fn align_to_side(
    current: &[Point],
    axis_x: f32,
    tolerance: f32,
    master_is_left: bool,
) -> Vec<Point> {
    let mut next = current.to_vec();
    let partners = pair_across_center(current, axis_x, tolerance);
    for (index, point) in current.iter().enumerate() {
        if on_center_line(*point, axis_x, tolerance) {
            continue;
        }
        let on_left = point.x < axis_x;
        if on_left != master_is_left {
            continue;
        }
        let Some(slave) = partners[index] else {
            continue;
        };
        if let Some(slot) = next.get_mut(slave) {
            *slot = reflect(*point, axis_x);
        }
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    const AXIS: f32 = 50.0;
    const TOL: f32 = 0.5;

    fn p(x: f32, y: f32) -> Point {
        Point { x, y }
    }

    fn close(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3
    }

    #[test]
    fn pairs_the_nearest_mirror_not_the_body_just_across_the_line() {
        // L's mirror is (90, 10). R_far sits on that mirror. R_near is only
        // one step across the line, much closer to L as a body, but a worse
        // mirror. The pair is L ↔ R_far.
        let points = vec![p(10.0, 10.0), p(90.0, 12.0), p(51.0, 10.0)];
        let partners = pair_across_center(&points, AXIS, TOL);
        assert_eq!(partners[0], Some(1));
        assert_eq!(partners[1], Some(0));
        assert_eq!(partners[2], None);
    }

    #[test]
    fn two_people_on_one_side_do_not_share_one_partner() {
        let points = vec![p(10.0, 10.0), p(10.0, 30.0), p(90.0, 10.0)];
        let partners = pair_across_center(&points, AXIS, TOL);
        assert_eq!(partners[0], Some(2));
        assert_eq!(partners[1], None);
        assert_eq!(partners[2], Some(0));
    }

    #[test]
    fn center_line_performers_are_unpaired() {
        let points = vec![p(50.0, 10.0), p(10.0, 10.0), p(90.0, 10.0)];
        let partners = pair_across_center(&points, AXIS, TOL);
        assert_eq!(partners[0], None);
        assert_eq!(partners[1], Some(2));
    }

    #[test]
    fn a_near_center_dot_within_tolerance_stays_on_the_line() {
        assert!(on_center_line(p(50.4, 8.0), AXIS, TOL));
        assert!(!on_center_line(p(51.0, 8.0), AXIS, TOL));
    }

    #[test]
    fn moving_one_side_places_the_partner_on_the_mirror() {
        let current = vec![p(10.0, 20.0), p(80.0, 4.0)];
        let next = mirror_move(&current, AXIS, TOL, &[(0, p(16.0, 24.0))]);
        assert!(close(next[0], p(16.0, 24.0)));
        assert!(close(next[1], p(84.0, 24.0)));
    }

    #[test]
    fn moving_the_right_side_mirrors_back_to_the_left() {
        let current = vec![p(10.0, 20.0), p(90.0, 20.0)];
        let next = mirror_move(&current, AXIS, TOL, &[(1, p(70.0, 28.0))]);
        assert!(close(next[1], p(70.0, 28.0)));
        assert!(close(next[0], p(30.0, 28.0)));
    }

    #[test]
    fn an_unpaired_performer_moves_alone() {
        let current = vec![p(10.0, 20.0), p(12.0, 30.0)];
        let next = mirror_move(&current, AXIS, TOL, &[(0, p(18.0, 22.0))]);
        assert!(close(next[0], p(18.0, 22.0)));
        assert!(close(next[1], p(12.0, 30.0)));
    }

    #[test]
    fn a_center_performer_keeps_depth_and_drops_sideways_travel() {
        let current = vec![p(50.0, 12.0), p(10.0, 12.0)];
        let next = mirror_move(&current, AXIS, TOL, &[(0, p(80.0, 18.0))]);
        assert!(close(next[0], p(50.0, 18.0)));
        assert!(close(next[1], current[1]));
    }

    #[test]
    fn both_members_driven_keeps_the_left_and_overwrites_the_right() {
        let current = vec![p(10.0, 20.0), p(90.0, 20.0)];
        let next = mirror_move(
            &current,
            AXIS,
            TOL,
            &[(0, p(14.0, 22.0)), (1, p(60.0, 40.0))],
        );
        assert!(close(next[0], p(14.0, 22.0)));
        assert!(close(next[1], p(86.0, 22.0)));
    }

    #[test]
    fn align_fixes_the_other_side_and_leaves_the_chosen_side() {
        // The right-hand dot is closer to the mirror of the first person
        // than to the mirror of the third, so the third stays unpaired.
        let current = vec![p(10.0, 20.0), p(70.0, 20.0), p(40.0, 40.0)];
        let next = align_to_side(&current, AXIS, TOL, true);
        assert!(close(next[0], current[0]));
        assert!(close(next[1], p(90.0, 20.0)));
        assert!(close(next[2], current[2]));
    }

    #[test]
    fn align_to_the_right_moves_the_left() {
        let current = vec![p(10.0, 20.0), p(70.0, 24.0)];
        let next = align_to_side(&current, AXIS, TOL, false);
        assert!(close(next[1], current[1]));
        assert!(close(next[0], p(30.0, 24.0)));
    }

    #[test]
    fn unmirrored_count_is_two_for_one_bad_pair_and_zero_when_exact() {
        let off = vec![p(10.0, 20.0), p(70.0, 20.0)];
        assert_eq!(unmirrored_count(&off, AXIS, TOL), 2);
        let exact = vec![p(10.0, 20.0), p(90.0, 20.0)];
        assert_eq!(unmirrored_count(&exact, AXIS, TOL), 0);
        let center = vec![p(50.0, 20.0)];
        assert_eq!(unmirrored_count(&center, AXIS, TOL), 0);
        let alone = vec![p(10.0, 20.0)];
        assert_eq!(unmirrored_count(&alone, AXIS, TOL), 1);
    }

    #[test]
    fn a_snap_sized_gap_does_not_count_as_broken() {
        let almost = vec![p(10.0, 20.0), p(90.0 + 0.4, 20.0)];
        assert_eq!(unmirrored_count(&almost, AXIS, TOL), 0);
        let broken = vec![p(10.0, 20.0), p(90.0 + 2.0, 20.0)];
        assert_eq!(unmirrored_count(&broken, AXIS, TOL), 2);
    }
}
