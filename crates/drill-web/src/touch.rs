//! Pointer math for the phone field.
//!
//! One finger selects and drags a performer. Two fingers pinch-zoom and pan.
//! The hit target grows on a narrow screen so a fingertip can land on a dot
//! that is only a few pixels wide.

use drill_core::Point;
use drill_render::{FieldMap, Vec2};

/// Below this width the toolbar keeps one row and tucks the rest away.
pub const COMPACT_WIDTH: f32 = 520.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FieldView {
    pub center: Point,
    pub zoom: f32,
}

impl FieldView {
    #[must_use]
    pub fn fit(grid_width: f32, grid_height: f32) -> Self {
        Self {
            center: Point {
                x: grid_width * 0.5,
                y: grid_height * 0.5,
            },
            zoom: 1.0,
        }
    }

    /// Frames the performers instead of the empty field around them.
    ///
    /// A phone is only a few hundred pixels wide. Fitting all 100 yards makes
    /// every dot smaller than a fingertip and piles a block on top of itself.
    #[must_use]
    pub fn frame_cast(grid_width: f32, grid_height: f32, positions: &[Point]) -> Self {
        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;
        for point in positions {
            if !point.x.is_finite() || !point.y.is_finite() {
                continue;
            }
            min_x = min_x.min(point.x);
            min_y = min_y.min(point.y);
            max_x = max_x.max(point.x);
            max_y = max_y.max(point.y);
        }
        if !min_x.is_finite() || max_x < min_x {
            return Self::fit(grid_width, grid_height);
        }
        let span = (max_x - min_x).max(max_y - min_y).max(8.0);
        let zoom = (grid_width / span * 0.62).clamp(1.0, 6.0);
        Self {
            center: Point {
                x: (min_x + max_x) * 0.5,
                y: (min_y + max_y) * 0.5,
            },
            zoom,
        }
    }
}

#[must_use]
pub fn chrome_is_compact(width: f32) -> bool {
    width < COMPACT_WIDTH
}

/// Screen-pixel radius that still counts as a tap on a performer.
#[must_use]
pub fn hit_radius(compact: bool) -> f32 {
    if compact { 44.0 } else { 22.0 }
}

/// Visual dot radius. Larger than the desktop cross so it reads on a phone,
/// still smaller than the hit target so neighbors stay separable.
#[must_use]
pub fn dot_radius(compact: bool) -> f32 {
    if compact { 16.0 } else { 11.0 }
}

/// Pinch zoom about the current field center, plus a two-finger pan.
///
/// `translation` is in screen pixels, y down. `scale` is the field's current
/// pixels-per-unit, used to turn that pan into yards.
#[must_use]
pub fn apply_two_finger(
    mut view: FieldView,
    zoom_delta: f32,
    translation_x: f32,
    translation_y: f32,
    scale: f32,
) -> FieldView {
    let zoom_delta = if zoom_delta.is_finite() && zoom_delta > 0.0 {
        zoom_delta
    } else {
        1.0
    };
    view.zoom = (view.zoom * zoom_delta).clamp(0.35, 8.0);
    let scale = if scale.is_finite() && scale > f32::EPSILON {
        scale
    } else {
        1.0
    };
    if translation_x.is_finite() {
        view.center.x -= translation_x / scale;
    }
    if translation_y.is_finite() {
        view.center.y += translation_y / scale;
    }
    view
}

/// Nearest performer within `radius` screen pixels, if any.
#[must_use]
pub fn pick_performer(
    map: &FieldMap,
    positions: &[Point],
    local: Vec2,
    radius: f32,
) -> Option<usize> {
    let mut best: Option<usize> = None;
    let mut best_distance = radius;
    for (index, point) in positions.iter().enumerate() {
        let screen = map.map(*point);
        let dx = screen.x - local.x;
        let dy = screen.y - local.y;
        let distance = (dx * dx + dy * dy).sqrt();
        if distance <= best_distance {
            best_distance = distance;
            best = Some(index);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> FieldMap {
        FieldMap::new(100.0, 53.333, Vec2 { x: 390.0, y: 700.0 }, 16.0)
    }

    #[test]
    fn narrow_screens_collapse_and_grow_the_target() {
        assert!(chrome_is_compact(390.0));
        assert!(!chrome_is_compact(844.0));
        assert!(hit_radius(true) >= 44.0);
        assert!(hit_radius(false) < hit_radius(true));
        assert!(dot_radius(true) > dot_radius(false));
    }

    #[test]
    fn framing_the_cast_zooms_past_the_empty_field() {
        let positions = [Point { x: 15.0, y: 10.0 }, Point { x: 30.0, y: 15.0 }];
        let view = FieldView::frame_cast(100.0, 53.333, &positions);
        assert!(view.zoom > 2.0);
        assert!(view.center.x < 40.0);
        assert!(view.center.y < 20.0);
        assert_eq!(FieldView::frame_cast(100.0, 50.0, &[]).zoom, 1.0);
    }

    #[test]
    fn pinch_zooms_and_two_fingers_pan() {
        let view = FieldView::fit(100.0, 50.0);
        let zoomed = apply_two_finger(view, 2.0, 0.0, 0.0, 4.0);
        assert!((zoomed.zoom - 2.0).abs() < 0.001);
        assert_eq!(zoomed.center, view.center);
        let panned = apply_two_finger(view, 1.0, 8.0, -4.0, 4.0);
        assert!((panned.center.x - (view.center.x - 2.0)).abs() < 0.001);
        assert!((panned.center.y - (view.center.y - 1.0)).abs() < 0.001);
        let junk = apply_two_finger(view, f32::NAN, f32::NAN, f32::INFINITY, 0.0);
        assert_eq!(junk.zoom, 1.0);
        assert_eq!(junk.center, view.center);
    }

    #[test]
    fn a_tap_inside_the_finger_target_selects_that_performer() {
        let map = map();
        let positions = [Point { x: 20.0, y: 20.0 }, Point { x: 40.0, y: 20.0 }];
        let on_first = map.map(positions[0]);
        let nearby = Vec2 {
            x: on_first.x + 30.0,
            y: on_first.y,
        };
        assert_eq!(
            pick_performer(&map, &positions, nearby, hit_radius(true)),
            Some(0)
        );
        assert_eq!(
            pick_performer(&map, &positions, nearby, hit_radius(false)),
            None
        );
        let on_second = map.map(positions[1]);
        assert_eq!(
            pick_performer(
                &map,
                &positions,
                Vec2 {
                    x: on_second.x - 4.0,
                    y: on_second.y,
                },
                hit_radius(true),
            ),
            Some(1)
        );
    }
}
