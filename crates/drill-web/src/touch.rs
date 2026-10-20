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

/// How the floor sits in the phone viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldOrientation {
    /// Front sideline along the bottom. A wide screen shows the full width.
    Wide,
    /// Sideline-to-sideline runs down the screen, so a tall phone holds the
    /// whole floor instead of a thin horizontal strip.
    Tall,
}

impl FieldOrientation {
    #[must_use]
    pub fn for_viewport(width: f32, height: f32) -> Self {
        if height > width {
            Self::Tall
        } else {
            Self::Wide
        }
    }
}

/// Screen mapping for the phone field. `zoom` 1 is the fit for this
/// orientation: the whole floor on a tall screen, and the full width on a
/// wide screen.
#[derive(Clone, Copy, Debug)]
pub struct PhoneMap {
    pub scale: f32,
    pub origin_x: f32,
    pub origin_y: f32,
    pub orientation: FieldOrientation,
    pub grid_width: f32,
    pub grid_height: f32,
}

impl PhoneMap {
    #[must_use]
    pub fn from_view(grid_width: f32, grid_height: f32, viewport: Vec2, view: FieldView) -> Self {
        let orientation = FieldOrientation::for_viewport(viewport.x, viewport.y);
        let zoom = if view.zoom.is_finite() && view.zoom > 0.0 {
            view.zoom
        } else {
            1.0
        };
        let fit = match orientation {
            FieldOrientation::Tall => {
                let across = viewport.x / grid_height.max(f32::EPSILON);
                let along = viewport.y / grid_width.max(f32::EPSILON);
                across.min(along)
            }
            FieldOrientation::Wide => viewport.x / grid_width.max(f32::EPSILON),
        };
        let scale = fit * zoom;
        let center = view.center;
        let (origin_x, origin_y) = match orientation {
            FieldOrientation::Tall => (
                viewport.x * 0.5 - center.y * scale,
                viewport.y * 0.5 - center.x * scale,
            ),
            FieldOrientation::Wide => (
                viewport.x * 0.5 - center.x * scale,
                viewport.y * 0.5 - (grid_height - center.y) * scale,
            ),
        };
        Self {
            scale,
            origin_x,
            origin_y,
            orientation,
            grid_width,
            grid_height,
        }
    }

    #[must_use]
    pub fn map(&self, point: Point) -> Vec2 {
        match self.orientation {
            FieldOrientation::Tall => Vec2 {
                x: self.origin_x + point.y * self.scale,
                y: self.origin_y + point.x * self.scale,
            },
            FieldOrientation::Wide => Vec2 {
                x: self.origin_x + point.x * self.scale,
                y: self.origin_y + (self.grid_height - point.y) * self.scale,
            },
        }
    }

    #[must_use]
    pub fn unmap(&self, screen: Vec2) -> Point {
        let scale = self.scale.max(f32::EPSILON);
        match self.orientation {
            FieldOrientation::Tall => Point {
                x: (screen.y - self.origin_y) / scale,
                y: (screen.x - self.origin_x) / scale,
            },
            FieldOrientation::Wide => Point {
                x: (screen.x - self.origin_x) / scale,
                y: self.grid_height - (screen.y - self.origin_y) / scale,
            },
        }
    }

    /// Axis-aligned turf in viewport pixels.
    #[must_use]
    pub fn turf(&self) -> (Vec2, Vec2) {
        let a = self.map(Point { x: 0.0, y: 0.0 });
        let b = self.map(Point {
            x: self.grid_width,
            y: self.grid_height,
        });
        (
            Vec2 {
                x: a.x.min(b.x),
                y: a.y.min(b.y),
            },
            Vec2 {
                x: a.x.max(b.x),
                y: a.y.max(b.y),
            },
        )
    }
}

/// Fit for the current screen. Wide screens fill the width and keep the
/// cast inside the vertical crop. Tall screens show the whole floor.
#[must_use]
pub fn fitted_view(
    grid_width: f32,
    grid_height: f32,
    viewport: Vec2,
    positions: &[Point],
) -> FieldView {
    let orientation = FieldOrientation::for_viewport(viewport.x, viewport.y);
    let mut center = Point {
        x: grid_width * 0.5,
        y: grid_height * 0.5,
    };
    if orientation == FieldOrientation::Wide {
        let scale = viewport.x / grid_width.max(f32::EPSILON);
        let visible = viewport.y / scale.max(f32::EPSILON);
        if let Some(mid) = cast_mid_y(positions)
            && visible < grid_height
        {
            let half = visible * 0.5;
            center.y = mid.clamp(half, (grid_height - half).max(half));
        }
    }
    FieldView { center, zoom: 1.0 }
}

fn cast_mid_y(positions: &[Point]) -> Option<f32> {
    let mut min_y = f32::MAX;
    let mut max_y = f32::MIN;
    for point in positions {
        if point.y.is_finite() {
            min_y = min_y.min(point.y);
            max_y = max_y.max(point.y);
        }
    }
    if min_y.is_finite() && max_y >= min_y {
        Some((min_y + max_y) * 0.5)
    } else {
        None
    }
}

/// Yard-line numbers painted on the floor: 10, 20, 30, 40, 50 from center.
#[must_use]
pub fn yard_line_numbers(width: f32) -> Vec<(f32, u8)> {
    let center = width * 0.5;
    let mut marks = Vec::new();
    for steps in 0..=4 {
        let number = 50 - steps * 10;
        if !(10..=50).contains(&number) {
            continue;
        }
        let delta = steps as f32 * 10.0;
        if steps == 0 {
            if center > 0.5 && center < width - 0.5 {
                marks.push((center, number as u8));
            }
            continue;
        }
        for x in [center - delta, center + delta] {
            if x > 0.5 && x < width - 0.5 {
                marks.push((x, number as u8));
            }
        }
    }
    marks.sort_by(|left, right| left.0.total_cmp(&right.0));
    marks
}

/// Circle diameter never drops below the phone minimum.
#[must_use]
pub fn phone_dot_radius(scale: f32) -> f32 {
    (scale * 0.45).clamp(11.0, 16.0)
}

/// Pinch zoom about the current field center, plus a two-finger pan.
///
/// `translation` is in screen pixels, y down. `scale` is the field's current
/// pixels-per-unit, used to turn that pan into yards. On a tall screen the
/// long axis runs vertically, so a sideways drag changes depth.
#[must_use]
pub fn apply_two_finger(
    mut view: FieldView,
    zoom_delta: f32,
    translation_x: f32,
    translation_y: f32,
    scale: f32,
    orientation: FieldOrientation,
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
    match orientation {
        FieldOrientation::Wide => {
            if translation_x.is_finite() {
                view.center.x -= translation_x / scale;
            }
            if translation_y.is_finite() {
                view.center.y += translation_y / scale;
            }
        }
        FieldOrientation::Tall => {
            if translation_x.is_finite() {
                view.center.y -= translation_x / scale;
            }
            if translation_y.is_finite() {
                view.center.x -= translation_y / scale;
            }
        }
    }
    view
}

/// Nearest performer within `radius` screen pixels, if any.
#[must_use]
pub fn pick_phone(map: &PhoneMap, positions: &[Point], local: Vec2, radius: f32) -> Option<usize> {
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
        let zoomed = apply_two_finger(view, 2.0, 0.0, 0.0, 4.0, FieldOrientation::Wide);
        assert!((zoomed.zoom - 2.0).abs() < 0.001);
        assert_eq!(zoomed.center, view.center);
        let panned = apply_two_finger(view, 1.0, 8.0, -4.0, 4.0, FieldOrientation::Wide);
        assert!((panned.center.x - (view.center.x - 2.0)).abs() < 0.001);
        assert!((panned.center.y - (view.center.y - 1.0)).abs() < 0.001);
        let tall = apply_two_finger(view, 1.0, 8.0, 4.0, 4.0, FieldOrientation::Tall);
        assert!((tall.center.y - (view.center.y - 2.0)).abs() < 0.001);
        assert!((tall.center.x - (view.center.x - 1.0)).abs() < 0.001);
        let junk = apply_two_finger(
            view,
            f32::NAN,
            f32::NAN,
            f32::INFINITY,
            0.0,
            FieldOrientation::Wide,
        );
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

    #[test]
    fn a_tall_screen_fills_with_the_long_axis_vertical() {
        let viewport = Vec2 { x: 390.0, y: 700.0 };
        let view = fitted_view(100.0, 53.333, viewport, &[]);
        let map = PhoneMap::from_view(100.0, 53.333, viewport, view);
        assert_eq!(map.orientation, FieldOrientation::Tall);
        let (min, max) = map.turf();
        assert!((max.y - min.y - 700.0).abs() < 1.0);
        assert!(max.x - min.x > 360.0);
        assert!(max.x - min.x <= 390.0);
        let dot = map.map(Point { x: 15.0, y: 10.0 });
        assert!(dot.x > min.x && dot.x < max.x);
        assert!(dot.y > min.y && dot.y < max.y);
        let back = map.unmap(dot);
        assert!((back.x - 15.0).abs() < 0.01);
        assert!((back.y - 10.0).abs() < 0.01);
    }

    #[test]
    fn a_wide_screen_fills_the_width_and_keeps_the_cast() {
        let viewport = Vec2 { x: 844.0, y: 240.0 };
        let positions = [Point { x: 15.0, y: 10.0 }, Point { x: 30.0, y: 15.0 }];
        let view = fitted_view(100.0, 53.333, viewport, &positions);
        let map = PhoneMap::from_view(100.0, 53.333, viewport, view);
        assert_eq!(map.orientation, FieldOrientation::Wide);
        let (min, max) = map.turf();
        assert!((max.x - min.x - 844.0).abs() < 1.0);
        assert!(max.y - min.y > viewport.y);
        for point in positions {
            let screen = map.map(point);
            assert!(screen.y > -1.0 && screen.y < viewport.y + 1.0, "{screen:?}");
        }
        assert!(phone_dot_radius(map.scale) * 2.0 >= 22.0);
    }

    #[test]
    fn yard_numbers_are_only_the_tens() {
        let marks = yard_line_numbers(100.0);
        let numbers: Vec<u8> = marks.iter().map(|(_, number)| *number).collect();
        assert_eq!(numbers, vec![10, 20, 30, 40, 50, 40, 30, 20, 10]);
        assert!(marks.windows(2).all(|pair| pair[1].0 - pair[0].0 > 9.0));
    }
}
