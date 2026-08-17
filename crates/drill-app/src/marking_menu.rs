//! Radial marking menu for the field canvas selection commands.
//!
//! A marking menu is deliberately *direction* based, not position based: the
//! command is chosen by the angle of the vector from the press point to the
//! release point, never by what happens to sit under the cursor. That single
//! property is what lets a practiced user flick and release before the wheel
//! has even been drawn, while a first-time user simply waits out the reveal
//! delay and reads the labels. Both paths run the same resolution code below.
//!
//! Only presentation lives here. The nine commands are exactly the ones the
//! field canvas has always offered; `app_ui` maps a resolved
//! [`MarkingAction`] onto the same editing calls the old list menu used.

use std::f32::consts::TAU;

use eframe::egui::{Align2, Pos2, Vec2};

/// Radius, in points, of the central cancel well. A release inside it is the
/// standard marking-menu escape hatch and resolves to nothing.
pub(crate) const DEAD_ZONE: f32 = 26.0;
/// Outer radius of the primary wheel.
pub(crate) const OUTER_RADIUS: f32 = 96.0;
/// Outer radius of the nested two-item wheel.
pub(crate) const SUB_RADIUS: f32 = 58.0;
/// Cancel well of the nested wheel; smaller, because the whole wheel is.
pub(crate) const SUB_DEAD_ZONE: f32 = 16.0;
/// How long the button must be held before the wheel is drawn. Releases
/// before this still resolve, which is the entire point of the fast path.
pub(crate) const REVEAL_DELAY: f64 = 0.15;
/// Extra dwell toward the sub-menu slice before the nested wheel opens.
pub(crate) const SUB_DELAY: f64 = 0.22;

/// The eight compass slices, ordered counter-clockwise from due east so the
/// array index doubles as the slice index used by the angle math.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Direction {
    E,
    Ne,
    N,
    Nw,
    W,
    Sw,
    S,
    Se,
}

impl Direction {
    pub(crate) const ALL: [Self; 8] = [
        Self::E,
        Self::Ne,
        Self::N,
        Self::Nw,
        Self::W,
        Self::Sw,
        Self::S,
        Self::Se,
    ];

    /// Counter-clockwise angle from due east, in radians.
    pub(crate) fn angle(self) -> f32 {
        let slot = match self {
            Self::E => 0.0,
            Self::Ne => 1.0,
            Self::N => 2.0,
            Self::Nw => 3.0,
            Self::W => 4.0,
            Self::Sw => 5.0,
            Self::S => 6.0,
            Self::Se => 7.0,
        };
        slot * (TAU / 8.0)
    }

    /// Unit vector in *screen* space, where y grows downward.
    pub(crate) fn unit(self) -> Vec2 {
        unit_vector(self.angle())
    }

    /// Where a slice's label sits relative to its anchor point just outside
    /// the rim, so long Japanese labels never overlap the wheel or clip.
    pub(crate) fn label_align(self) -> Align2 {
        match self {
            Self::E => Align2::LEFT_CENTER,
            Self::Ne => Align2::LEFT_BOTTOM,
            Self::N => Align2::CENTER_BOTTOM,
            Self::Nw => Align2::RIGHT_BOTTOM,
            Self::W => Align2::RIGHT_CENTER,
            Self::Sw => Align2::RIGHT_TOP,
            Self::S => Align2::CENTER_TOP,
            Self::Se => Align2::LEFT_TOP,
        }
    }
}

/// One of the nine field commands the wheel dispatches.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum MarkingAction {
    AlignHorizontal,
    AlignVertical,
    DistributeHorizontal,
    DistributeVertical,
    Straighten,
    CopyFormation,
    PasteFormation,
    Lock,
    Hide,
}

/// What a primary slice does when released on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Slice {
    Action(MarkingAction),
    /// Opens the nested wheel holding the two least-used commands.
    SubMenu,
}

/// Slot assignment. The four cardinals are the easiest points to hit blind,
/// so the four most frequently repeated commands live there; the two least
/// used ones are folded behind a single nested slice rather than crowding
/// nine items into eight slots.
pub(crate) fn slice_for(direction: Direction) -> Slice {
    match direction {
        Direction::N => Slice::Action(MarkingAction::AlignVertical),
        Direction::Ne => Slice::Action(MarkingAction::DistributeVertical),
        Direction::E => Slice::Action(MarkingAction::Straighten),
        Direction::Se => Slice::Action(MarkingAction::PasteFormation),
        Direction::S => Slice::Action(MarkingAction::AlignHorizontal),
        Direction::Sw => Slice::Action(MarkingAction::DistributeHorizontal),
        Direction::W => Slice::Action(MarkingAction::CopyFormation),
        Direction::Nw => Slice::SubMenu,
    }
}

/// Screen-space unit vector for a counter-clockwise-from-east angle.
fn unit_vector(angle: f32) -> Vec2 {
    Vec2::new(angle.cos(), -angle.sin())
}

/// Resolve a drag vector to a slice direction. `None` means the gesture ended
/// inside the cancel well and nothing should fire.
pub(crate) fn resolve(dx: f32, dy: f32, dead_zone: f32) -> Option<Direction> {
    if !dx.is_finite() || !dy.is_finite() || dx.hypot(dy) < dead_zone {
        return None;
    }
    // Screen y grows downward; negate it so the angle reads as ordinary
    // counter-clockwise-from-east math and "north" really is up.
    let angle = (-dy).atan2(dx).rem_euclid(TAU);
    let slot = (angle / (TAU / 8.0)).round() as usize % 8;
    Some(Direction::ALL[slot])
}

/// Where each nested item is drawn, and therefore which flick reaches it.
/// Both sit clear of the primary wheel, which is down and to the right of
/// the nested one.
pub(crate) const SUB_ITEMS: [(Direction, MarkingAction); 2] = [
    (Direction::W, MarkingAction::Lock),
    (Direction::N, MarkingAction::Hide),
];

/// Two items 90 degrees apart leave two half-planes, split along their
/// bisector. The drawn wedges are narrower than this, so the hit region is
/// always at least as forgiving as it looks.
pub(crate) fn resolve_sub(dx: f32, dy: f32, dead_zone: f32) -> Option<MarkingAction> {
    if !dx.is_finite() || !dy.is_finite() || dx.hypot(dy) < dead_zone {
        return None;
    }
    let angle = (-dy).atan2(dx).rem_euclid(TAU);
    Some(if (TAU * 3.0 / 8.0..TAU * 7.0 / 8.0).contains(&angle) {
        MarkingAction::Lock
    } else {
        MarkingAction::Hide
    })
}

/// Vertices of one pie wedge for `Shape::convex_polygon`. Spans up to a half
/// turn stay convex, which is all epaint fills correctly.
pub(crate) fn wedge_points(center: Pos2, radius: f32, direction: Direction, span: f32) -> Vec<Pos2> {
    const STEPS: usize = 12;
    let base = direction.angle() - span * 0.5;
    let mut points = Vec::with_capacity(STEPS + 2);
    points.push(center);
    for step in 0..=STEPS {
        let angle = base + span * (step as f32 / STEPS as f32);
        points.push(center + unit_vector(angle) * radius);
    }
    points
}

/// Live gesture state. Session-only, like the knife and free-draw modes it
/// sits beside on `DrillApp`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MarkingMenuState {
    /// Press point. The wheel is centered here and every angle is measured
    /// from it, so the wheel appearing late never moves the target.
    pub(crate) center: Pos2,
    pub(crate) opened_at: f64,
    pub(crate) pointer: Pos2,
    /// Center of the nested wheel, once it has opened.
    pub(crate) sub_center: Option<Pos2>,
    /// When the pointer first pointed at the sub-menu slice, for the dwell.
    pub(crate) sub_dwell_since: Option<f64>,
    /// Set when the button was released on the sub-menu slice before the
    /// nested wheel had opened: it stays up awaiting a click rather than
    /// leaving that flick at a dead end.
    pub(crate) sticky: bool,
}

impl MarkingMenuState {
    pub(crate) fn new(center: Pos2, now: f64) -> Self {
        Self {
            center,
            opened_at: now,
            pointer: center,
            sub_center: None,
            sub_dwell_since: None,
            sticky: false,
        }
    }

    /// Whether the wheel should be drawn yet.
    pub(crate) fn revealed(&self, now: f64) -> bool {
        self.sticky || self.sub_center.is_some() || now - self.opened_at >= REVEAL_DELAY
    }

    pub(crate) fn direction(&self) -> Option<Direction> {
        let delta = self.pointer - self.center;
        resolve(delta.x, delta.y, DEAD_ZONE)
    }

    pub(crate) fn sub_action(&self) -> Option<MarkingAction> {
        let center = self.sub_center?;
        let delta = self.pointer - center;
        resolve_sub(delta.x, delta.y, SUB_DEAD_ZONE)
    }

    /// Anchor for the nested wheel: pushed out along the sub-menu slice so it
    /// reads as growing out of that slice rather than covering the wheel.
    pub(crate) fn open_sub(&mut self) {
        self.sub_center = Some(self.center + Direction::Nw.unit() * (OUTER_RADIUS * 0.92));
    }
}
