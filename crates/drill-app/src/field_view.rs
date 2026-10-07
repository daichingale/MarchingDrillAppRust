use super::*;

/// Lower bound of the interactive zoom range. Mirrors the clamp
/// `drill_render::FieldMap::with_view` applies internally, so the viewport can
/// never ask the renderer for a magnification it will silently refuse.
pub(crate) const MIN_ZOOM: f32 = 0.25;
/// Upper bound of the interactive zoom range. See [`MIN_ZOOM`].
pub(crate) const MAX_ZOOM: f32 = 8.0;

/// Tuning for the canvas "feel".
///
/// Every constant here is expressed per *second* (a decay rate, a time
/// constant, an angular frequency) and is consumed through `powf`/`exp`, never
/// multiplied once per frame. That is what makes the motion identical at 60,
/// 144 and 240 Hz: `v *= 0.9` every frame decays twice as fast when the frame
/// rate doubles, `v *= 0.9f32.powf(dt)` does not.
mod feel {
    /// Fraction of a glide's velocity that survives one full second of
    /// coasting. 0.0025 is a half-life of ~116 ms, so a moderate release
    /// speed is under the stop threshold in well under a second, while a
    /// fling at the velocity cap takes a bit over a second to fully settle --
    /// about the length of a firm macOS trackpad flick, long enough to read
    /// as momentum without ever feeling like the canvas is dragging its feet.
    pub(super) const PAN_FRICTION_PER_SEC: f32 = 0.0025;
    /// Speed (field units/second) below which a glide is simply over. The
    /// default field is 100 units wide across a ~900 px canvas, so 0.2
    /// units/second is under two pixels per second: invisible, and the point
    /// at which we stop asking for repaints.
    pub(super) const PAN_STOP_SPEED: f32 = 0.2;
    /// Time constant of the exponential moving average that smooths the
    /// pointer velocity during a drag. 60 ms is short enough to track the
    /// final flick of the wrist and long enough to reject the single-frame
    /// jitter that a raw `delta / dt` would fling on.
    pub(super) const VELOCITY_TAU: f32 = 0.06;
    /// A pointer that has not meaningfully moved for this long counts as
    /// parked, and releasing it must not fling. Without this, someone who
    /// pans quickly, carefully settles on a spot, and lets go still carries
    /// the stale velocity of the approach and watches the view sail away.
    pub(super) const PARKED_AFTER: f32 = 0.07;
    /// Pointer motion below this many pixels in a frame counts as "parked".
    pub(super) const PARKED_PIXELS: f32 = 0.4;
    /// Hard ceiling on fling speed as a multiple of the field width per
    /// second. Combined with the friction above, a maximal fling coasts about
    /// half a field -- far, but never across the whole show and back.
    pub(super) const MAX_FLING_FIELDS_PER_SEC: f32 = 3.0;

    /// Coefficient of the classic iOS rubber-band curve
    /// `f(x) = (1 - 1/(x * C / d + 1)) * d`. 0.55 is the value UIScrollView
    /// uses; it makes the first few pixels of overscroll feel nearly free and
    /// the rest progressively heavier.
    pub(super) const RUBBER_C: f32 = 0.55;
    /// The `d` of that curve, as a fraction of the smaller of (visible extent,
    /// field extent) on the axis. It is an asymptote, not a clamp: no amount
    /// of pulling moves the view further than this. 18% is enough to feel the
    /// give without ever pushing a meaningful amount of the drill off screen.
    pub(super) const RUBBER_FRACTION: f32 = 0.18;
    /// Angular frequency of the critically damped spring that returns an
    /// overscrolled view to the edge. A critically damped spring settles in
    /// about `5 / omega` seconds, so 18 rad/s is ~0.28 s: the same order as
    /// the iOS bounce-back, and critically (not under-) damped because a
    /// visible wobble at the field edge would be a toy, not a tool.
    pub(super) const RUBBER_OMEGA: f32 = 18.0;
    /// Overscroll smaller than this many field units is treated as zero.
    pub(super) const EDGE_EPSILON: f32 = 1e-3;

    /// Time constant of the exponential approach of `zoom` to `zoom_target`.
    /// 80 ms reaches 99% of the way in ~0.37 s -- fast enough that a single
    /// wheel notch feels like a direct response rather than a transition.
    pub(super) const ZOOM_TAU: f32 = 0.08;
    /// Log-distance from the target at which the zoom animation is finished.
    pub(super) const ZOOM_EPSILON: f32 = 1e-4;

    /// Time constant for the dot "lift" fading in when a drag starts and out
    /// when it ends. Deliberately very short: this is a state cue, not an
    /// animation to be admired.
    pub(super) const LIFT_TAU: f32 = 0.045;
    /// How long the landing settle lasts after dragged dots snap to the grid.
    pub(super) const SETTLE_SECONDS: f32 = 0.18;
}

/// Velocity multiplier after `dt` seconds of exponential pan friction.
pub(crate) fn friction_decay(dt: f32) -> f32 {
    feel::PAN_FRICTION_PER_SEC.powf(dt.max(0.0))
}

/// Distance covered by an exponentially decaying velocity `v0` over `dt`
/// seconds. This is the exact integral of `v0 * k^t`, not `v0 * dt`, so
/// splitting a step in half and taking it twice lands in precisely the same
/// place -- the property that makes the glide identical at every frame rate.
pub(crate) fn friction_travel(v0: f32, dt: f32) -> f32 {
    let k = friction_decay(dt);
    v0 * (k - 1.0) / feel::PAN_FRICTION_PER_SEC.ln()
}

/// How far past the edge the view actually moves when the pointer has pulled
/// `raw` units past it. `limit` is the asymptote: the result approaches but
/// never reaches it, which is why hard pulls feel like they are hitting
/// something rather than stopping at a wall.
pub(crate) fn rubber_offset(raw: f32, limit: f32) -> f32 {
    if limit <= 0.0 || raw <= 0.0 || !raw.is_finite() {
        return 0.0;
    }
    limit * (1.0 - 1.0 / (raw * feel::RUBBER_C / limit + 1.0))
}

/// Inverse of [`rubber_offset`]: the raw pull that produced a given visible
/// offset. Applying further drag deltas in raw space and re-mapping keeps the
/// band's shape exact regardless of how the pull is chopped into frames.
pub(crate) fn rubber_raw(offset: f32, limit: f32) -> f32 {
    if limit <= 0.0 || offset <= 0.0 || !offset.is_finite() {
        return 0.0;
    }
    let offset = offset.min(limit * (1.0 - 1e-6));
    limit * offset / (feel::RUBBER_C * (limit - offset))
}

/// Normalised travel bounds for one axis: the range the center may occupy
/// without showing anything past the field edge. Collapses to a single point
/// when the whole field already fits, which is what the original hard clamp
/// did too.
pub(crate) fn axis_bounds(half_extent: f32, field_extent: f32) -> (f32, f32) {
    if half_extent * 2.0 >= field_extent {
        let mid = field_extent * 0.5;
        (mid, mid)
    } else {
        (half_extent, field_extent - half_extent)
    }
}

/// Move `value` by `delta` along one axis, spending whatever part of the delta
/// leaves `[min, max]` through the rubber band. Motion back toward the range
/// is never resisted, so pulling out and pushing back in tracks the pointer
/// one-to-one on the way home.
pub(crate) fn rubber_axis(value: f32, delta: f32, min: f32, max: f32, limit: f32) -> f32 {
    debug_assert!(min <= max, "axis_bounds must normalise the range");
    if delta == 0.0 || !delta.is_finite() || !value.is_finite() {
        return value;
    }
    if delta < 0.0 {
        // Mirror the axis and reuse the positive-direction case below.
        return -rubber_axis(-value, -delta, -max, -min, limit);
    }
    if value < min {
        // Returning from a low-side overscroll: unresisted up to the edge.
        let room = min - value;
        if delta <= room {
            return value + delta;
        }
        return rubber_axis(min, delta - room, min, max, limit);
    }
    if value < max {
        let room = max - value;
        if delta <= room {
            return value + delta;
        }
        return rubber_axis(max, delta - room, min, max, limit);
    }
    // At or past `max`: the remainder goes through the band.
    let raw = rubber_raw(value - max, limit) + delta;
    max + rubber_offset(raw, limit)
}

/// One step of a critically damped spring pulling `offset` toward zero,
/// returning `(offset, velocity)`. This is the analytic solution rather than a
/// Euler integration, so it is exact for any `dt`, cannot overshoot, and
/// cannot blow up when a frame stalls.
pub(crate) fn spring_step(offset: f32, velocity: f32, dt: f32) -> (f32, f32) {
    let omega = feel::RUBBER_OMEGA;
    let decay = (-omega * dt).exp();
    let coupled = velocity + omega * offset;
    (
        (offset + coupled * dt) * decay,
        (velocity - coupled * omega * dt) * decay,
    )
}

/// Advance one axis of the free viewport: spring back if it is overscrolled,
/// otherwise coast under friction. Returns whether the axis is still moving.
fn step_axis(pos: &mut f32, velocity: &mut f32, min: f32, max: f32, dt: f32) -> bool {
    let target = pos.clamp(min, max);
    let over = *pos - target;
    if over.abs() > feel::EDGE_EPSILON {
        let (offset, next_velocity) = spring_step(over, *velocity, dt);
        *pos = target + offset;
        *velocity = next_velocity;
        if offset.abs() <= feel::EDGE_EPSILON && next_velocity.abs() <= feel::PAN_STOP_SPEED {
            *pos = target;
            *velocity = 0.0;
            return false;
        }
        return true;
    }
    *pos = target;
    if velocity.abs() <= feel::PAN_STOP_SPEED {
        *velocity = 0.0;
        return false;
    }
    *pos += friction_travel(*velocity, dt);
    *velocity *= friction_decay(dt);
    true
}

/// Non-persistent navigation state for the desktop 2D field. The document
/// remains in field units; this only changes the lens through which it is
/// viewed, so pan/zoom are never undoable edits.
///
/// It also carries the small amount of *motion* state the canvas needs -- pan
/// momentum, the eased zoom target, and the two scalars behind the dot
/// lift/land cues. Keeping it here rather than on `DrillApp` means all canvas
/// feel lives in one `Copy` struct that resets cleanly with the view, and the
/// dot cues cost two floats total no matter how many hundreds of performers
/// the paint loop walks.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FieldViewport {
    pub(crate) center: Point,
    pub(crate) zoom: f32,
    pub(crate) pan_last_pointer: Option<Pos2>,
    /// Smoothed pan velocity in **field units per second** (x right, y up in
    /// field space, matching `center`). Non-zero only while gliding.
    pan_velocity: Vec2,
    /// How long the pointer has been effectively stationary during the current
    /// drag. See `feel::PARKED_AFTER`.
    pan_parked: f32,
    /// Where `zoom` is easing to. Wheel notches accumulate here.
    zoom_target: f32,
    /// The field point that must stay pinned under the cursor for the duration
    /// of the current zoom gesture, together with the cursor's offset from the
    /// canvas rect's top-left. Re-derived from the live map on every wheel
    /// event, so the pin is always exact rather than drifting with the target.
    zoom_anchor: Option<(Point, Vec2)>,
    /// True while performer dots are being dragged.
    dot_drag: bool,
    /// Eased 0..1 companion to `dot_drag` driving the dot lift.
    dot_lift: f32,
    /// Seconds remaining in the landing settle after a drag commits.
    dot_settle: f32,
}

impl FieldViewport {
    pub(crate) fn fit(grid: &GridConfig) -> Self {
        Self {
            center: Point {
                x: grid.width * 0.5,
                y: grid.height * 0.5,
            },
            zoom: 1.0,
            pan_last_pointer: None,
            pan_velocity: Vec2::ZERO,
            pan_parked: 0.0,
            zoom_target: 1.0,
            zoom_anchor: None,
            dot_drag: false,
            dot_lift: 0.0,
            dot_settle: 0.0,
        }
    }

    pub(crate) fn reset(&mut self, grid: &GridConfig) {
        *self = Self::fit(grid);
    }

    /// Screen pixels per field unit at the current zoom.
    fn unit_scale(&self, grid: &GridConfig, size: Vec2) -> f32 {
        let fit = (size.x / grid.width.max(f32::EPSILON))
            .min(size.y / grid.height.max(f32::EPSILON))
            .max(f32::EPSILON);
        (fit * self.zoom).max(f32::EPSILON)
    }

    /// Half of the visible field extent on each axis, in field units.
    fn half_extents(&self, grid: &GridConfig, size: Vec2) -> (f32, f32) {
        let unit = self.unit_scale(grid, size);
        (size.x / (2.0 * unit), size.y / (2.0 * unit))
    }

    /// `(min_x, max_x, min_y, max_y, limit_x, limit_y)` for the current zoom.
    fn travel(&self, grid: &GridConfig, size: Vec2) -> (f32, f32, f32, f32, f32, f32) {
        let (half_x, half_y) = self.half_extents(grid, size);
        let (min_x, max_x) = axis_bounds(half_x, grid.width);
        let (min_y, max_y) = axis_bounds(half_y, grid.height);
        let limit_x = (half_x * 2.0).min(grid.width) * feel::RUBBER_FRACTION;
        let limit_y = (half_y * 2.0).min(grid.height) * feel::RUBBER_FRACTION;
        (min_x, max_x, min_y, max_y, limit_x, limit_y)
    }

    /// Hard clamp: the view never shows past the field edge. Kept for
    /// non-interactive callers (`reset`, programmatic fit, tests). Interactive
    /// pan and momentum deliberately go through the rubber band instead.
    pub(crate) fn clamp_center(&mut self, grid: &GridConfig, size: Vec2) {
        let (min_x, max_x, min_y, max_y, ..) = self.travel(grid, size);
        self.center.x = self.center.x.clamp(min_x, max_x);
        self.center.y = self.center.y.clamp(min_y, max_y);
    }

    /// Safety clamp for the interactive path: allows the full rubber-band
    /// overshoot but nothing beyond it, so a programmatic `center` write or a
    /// viewport resize can never strand the view somewhere the spring would
    /// take a visible age to crawl back from.
    pub(crate) fn contain_center(&mut self, grid: &GridConfig, size: Vec2) {
        let (min_x, max_x, min_y, max_y, limit_x, limit_y) = self.travel(grid, size);
        self.center.x = self.center.x.clamp(min_x - limit_x, max_x + limit_x);
        self.center.y = self.center.y.clamp(min_y - limit_y, max_y + limit_y);
    }

    /// Non-interactive pan by a pixel delta, hard-clamped. No current caller
    /// drives this outside tests (the canvas always goes through
    /// [`Self::drag_pan`]), but it is the direct pre-rubber-band primitive the
    /// bounds test exercises and is worth keeping as a documented, tested
    /// building block for any future non-interactive pan (e.g. a "nudge view"
    /// command).
    #[allow(dead_code)]
    pub(crate) fn pan_pixels(&mut self, delta: Vec2, grid: &GridConfig, size: Vec2) {
        let unit = self.unit_scale(grid, size);
        self.center.x -= delta.x / unit;
        self.center.y += delta.y / unit;
        self.clamp_center(grid, size);
    }

    /// A pan gesture started. Grabbing the canvas stops any glide in progress,
    /// which is the universal behaviour of momentum surfaces.
    pub(crate) fn begin_pan(&mut self, pointer: Option<Pos2>) {
        self.pan_last_pointer = pointer;
        self.pan_velocity = Vec2::ZERO;
        self.pan_parked = 0.0;
    }

    pub(crate) fn end_pan(&mut self) {
        self.pan_last_pointer = None;
        self.pan_parked = 0.0;
    }

    /// Stop a glide without disturbing an in-progress rubber-band return.
    pub(crate) fn stop_glide(&mut self) {
        self.pan_velocity = Vec2::ZERO;
    }

    /// One frame of an interactive pan drag: applies the pointer delta through
    /// the rubber band and folds it into the smoothed release velocity.
    pub(crate) fn drag_pan(&mut self, delta: Vec2, dt: f32, grid: &GridConfig, size: Vec2) {
        let unit = self.unit_scale(grid, size);
        let (min_x, max_x, min_y, max_y, limit_x, limit_y) = self.travel(grid, size);
        let before = self.center;
        self.center.x = rubber_axis(self.center.x, -delta.x / unit, min_x, max_x, limit_x);
        self.center.y = rubber_axis(self.center.y, delta.y / unit, min_y, max_y, limit_y);

        // Velocity comes from the motion that *happened*, not the motion that
        // was asked for. Deep in the rubber band the view barely moves, so a
        // release there glides barely at all -- which is right: the band, not
        // the flick, is what the hand is fighting at that point.
        let step = dt.max(1.0 / 1000.0);
        let instant = Vec2::new(
            (self.center.x - before.x) / step,
            (self.center.y - before.y) / step,
        );
        let alpha = 1.0 - (-dt / feel::VELOCITY_TAU).exp();
        self.pan_velocity += (instant - self.pan_velocity) * alpha;

        if delta.length() < feel::PARKED_PIXELS {
            self.pan_parked += dt;
        } else {
            self.pan_parked = 0.0;
        }
        if self.pan_parked >= feel::PARKED_AFTER {
            self.pan_velocity = Vec2::ZERO;
        }

        let cap = grid.width * feel::MAX_FLING_FIELDS_PER_SEC;
        if self.pan_velocity.length() > cap {
            self.pan_velocity = self.pan_velocity.normalized() * cap;
        }
    }

    /// Set the zoom directly, cancelling any animation. For programmatic
    /// callers (e.g. "zoom to selection") that want the new value immediately.
    pub(crate) fn set_zoom(&mut self, zoom: f32) {
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self.zoom_target = self.zoom;
        self.zoom_anchor = None;
    }

    /// Center on a field rectangle and zoom until it fills most of the window.
    /// A single dot still gets a readable margin instead of the maximum zoom.
    pub(crate) fn frame_bounds(&mut self, grid: &GridConfig, size: Vec2, min: Point, max: Point) {
        if size.x < 1.0 || size.y < 1.0 || !min.x.is_finite() || !max.x.is_finite() {
            return;
        }
        let step = grid.horizontal_units / f32::from(grid.horizontal_steps.max(1));
        let min_span = (step * 8.0).max(1.0);
        let width = (max.x - min.x).abs().max(min_span);
        let height = (max.y - min.y).abs().max(min_span);
        let fit = (size.x / grid.width.max(f32::EPSILON))
            .min(size.y / grid.height.max(f32::EPSILON))
            .max(f32::EPSILON);
        let pad = 1.35;
        let zoom = (size.x / (fit * width * pad)).min(size.y / (fit * height * pad));
        if !zoom.is_finite() {
            return;
        }
        self.center = Point {
            x: (min.x + max.x) * 0.5,
            y: (min.y + max.y) * 0.5,
        };
        self.set_zoom(zoom);
        self.stop_glide();
        self.clamp_center(grid, size);
    }

    /// Accumulate a wheel notch into the zoom target and pin the field point
    /// currently under the cursor for the whole gesture.
    pub(crate) fn zoom_toward(
        &mut self,
        factor: f32,
        pointer: Pos2,
        rect: Rect,
        grid: &GridConfig,
    ) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let local = Vec2::new(pointer.x - rect.left(), pointer.y - rect.top());
        // Derive the anchor from the map as it is *right now* (mid-animation
        // if a previous notch is still easing). That keeps the pin exact: the
        // point under the cursor at the instant of the notch is the point that
        // stays there, no matter where the previous gesture had got to.
        let pinned = self.map(grid, rect.size()).unmap(drill_render::Vec2 {
            x: local.x,
            y: local.y,
        });
        self.zoom_anchor = Some((pinned, local));
        self.zoom_target = (self.zoom_target * factor).clamp(MIN_ZOOM, MAX_ZOOM);
    }

    fn map(&self, grid: &GridConfig, size: Vec2) -> drill_render::FieldMap {
        drill_render::FieldMap::with_view(
            grid.width,
            grid.height,
            drill_render::Vec2 {
                x: size.x,
                y: size.y,
            },
            0.0,
            Some(self.center),
            self.zoom,
        )
    }

    /// Apply a new zoom value while holding `zoom_anchor` fixed under the
    /// cursor. The map is affine in `center` with a unit coefficient, so
    /// correcting by `pinned - unmap(local)` after the zoom change is exact in
    /// one step -- there is no iteration and no residual slide, which is why
    /// this can be run every frame of an eased zoom and still keep the point
    /// under the cursor perfectly still.
    fn apply_zoom(&mut self, zoom: f32, grid: &GridConfig, size: Vec2) {
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        if let Some((pinned, local)) = self.zoom_anchor {
            let local = drill_render::Vec2 {
                x: local.x,
                y: local.y,
            };
            let now = self.map(grid, size).unmap(local);
            self.center.x += pinned.x - now.x;
            self.center.y += pinned.y - now.y;
        }
        // Only the rubber limit, not the hard edge: if zooming out at a corner
        // pushes the center out of range, the spring in `tick` glides it back
        // instead of snapping it, and the anchored point slides only as far as
        // physically necessary.
        self.contain_center(grid, size);
    }

    /// Immediate, non-animated cursor-anchored zoom step, hard-clamped. The
    /// interactive wheel path now goes through [`Self::zoom_toward`] +
    /// [`Self::tick`] instead so the zoom eases; this direct primitive has no
    /// production caller left but stays as the tested, documented building
    /// block for any future instant-zoom command (and it's what the original
    /// anchoring test exercises).
    #[allow(dead_code)]
    pub(crate) fn zoom_at(&mut self, factor: f32, pointer: Pos2, rect: Rect, grid: &GridConfig) {
        let size = rect.size();
        let local = Vec2::new(pointer.x - rect.left(), pointer.y - rect.top());
        let pinned = self.map(grid, size).unmap(drill_render::Vec2 {
            x: local.x,
            y: local.y,
        });
        self.zoom_anchor = Some((pinned, local));
        self.apply_zoom(self.zoom * factor, grid, size);
        self.zoom_anchor = None;
        self.zoom_target = self.zoom;
        self.clamp_center(grid, size);
    }

    /// Report whether performer dots are currently being dragged. Called once
    /// per frame from the canvas rather than paired begin/end calls, so an
    /// abandoned drag can never leave the lift stuck on.
    pub(crate) fn set_dot_drag(&mut self, dragging: bool) {
        self.dot_drag = dragging;
    }

    /// Dragged dots have just landed on the grid: run the settle once.
    pub(crate) fn dots_landed(&mut self) {
        self.dot_drag = false;
        self.dot_settle = feel::SETTLE_SECONDS;
    }

    /// 0..1 lift amount for dots held under the pointer.
    pub(crate) fn dot_lift(&self) -> f32 {
        self.dot_lift
    }

    /// 1 at the instant dots land, easing out to 0. Cubic ease-out so the
    /// visible part of the settle is over almost immediately and the tail just
    /// removes the last hair of motion -- on the 500th repetition this reads
    /// as "it landed", not as an animation.
    pub(crate) fn dot_settle_phase(&self) -> f32 {
        if self.dot_settle <= 0.0 {
            return 0.0;
        }
        let t = (self.dot_settle / feel::SETTLE_SECONDS).clamp(0.0, 1.0);
        t * t * t
    }

    /// Advance every canvas animation by `dt` seconds. Returns `true` while
    /// anything is still moving, which is the caller's cue to request another
    /// repaint -- and, just as importantly, returns `false` the moment
    /// everything is at rest so the app stops repainting entirely.
    pub(crate) fn tick(&mut self, dt: f32, grid: &GridConfig, rect: Rect) -> bool {
        let size = rect.size();
        let mut active = false;

        // --- zoom -----------------------------------------------------------
        // Ease log(zoom), not zoom: magnification is multiplicative, so a
        // constant rate of change in log space is what the eye reads as a
        // constant rate of zooming. Exponential approach rather than a spring
        // because (a) a new wheel notch re-targets it instantly with no
        // settling artefact, and (b) it can never overshoot -- an overshooting
        // zoom is both very visible and slightly nauseating.
        if self.zoom > 0.0 && self.zoom_target > 0.0 {
            let gap = self.zoom_target.ln() - self.zoom.ln();
            if gap.abs() > feel::ZOOM_EPSILON {
                let t = 1.0 - (-dt / feel::ZOOM_TAU).exp();
                self.apply_zoom((self.zoom.ln() + gap * t).exp(), grid, size);
                active = true;
            } else if self.zoom != self.zoom_target {
                self.apply_zoom(self.zoom_target, grid, size);
                self.zoom_anchor = None;
            }
        }

        // --- pan momentum and rubber-band return ----------------------------
        // Only while the pointer is not driving the pan: during a drag the
        // hand is the physics.
        if self.pan_last_pointer.is_none() {
            let (min_x, max_x, min_y, max_y, ..) = self.travel(grid, size);
            let moving_x = step_axis(
                &mut self.center.x,
                &mut self.pan_velocity.x,
                min_x,
                max_x,
                dt,
            );
            let moving_y = step_axis(
                &mut self.center.y,
                &mut self.pan_velocity.y,
                min_y,
                max_y,
                dt,
            );
            active |= moving_x || moving_y;
        }

        // --- dot lift / land settle -----------------------------------------
        let lift_target = if self.dot_drag { 1.0 } else { 0.0 };
        if (self.dot_lift - lift_target).abs() > 1e-3 {
            self.dot_lift += (lift_target - self.dot_lift) * (1.0 - (-dt / feel::LIFT_TAU).exp());
            active = true;
        } else {
            self.dot_lift = lift_target;
        }
        if self.dot_settle > 0.0 {
            self.dot_settle = (self.dot_settle - dt).max(0.0);
            active = true;
        }

        active
    }
}

impl DrillApp {
    pub(crate) fn draw_stadium(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        painter: &egui::Painter,
        rect: Rect,
    ) {
        if self.camera_program_preview
            && let Some(pose) = self.document.camera_program.evaluate(self.count_position)
        {
            self.camera = pose.orbit_camera();
        }
        if response.dragged() {
            self.camera_program_preview = false;
            let d = response.drag_delta();
            self.camera.yaw += d.x * 0.008;
            self.camera.pitch = (self.camera.pitch + d.y * 0.008).clamp(0.03, 1.54);
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.camera.distance =
                    (self.camera.distance * (1.0 - scroll * 0.0015)).clamp(8.0, 2000.0);
            }
        }
        painter.rect_filled(rect, 4.0, Color32::from_rgb(16, 20, 28));
        let visibility_key = stadium_inspector::VisibilityKey::new(
            self.history.revision(),
            self.current_set,
            self.count_position,
            self.camera,
        );
        let mut refresh_visibility = stadium_inspector::VisibilityRefresh::None;
        egui::Area::new("stadium-visibility-controls".into())
            .fixed_pos(rect.left_top() + Vec2::new(12.0, 34.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    refresh_visibility =
                        self.stadium_inspector
                            .controls(ui, self.locale, visibility_key);
                });
            });
        if self.stadium_inspector.enabled
            // A smooth playback advances the key every frame. Keep the last
            // diagnosis visibly stale while it runs rather than doing an
            // O(P²) visibility pass on the UI thread for every video frame.
            // Pausing refreshes the new scene once; the button still forces
            // an immediate user-requested refresh during playback.
            && (refresh_visibility == stadium_inspector::VisibilityRefresh::Manual
                || (!self.playing && !self.stadium_inspector.is_current(visibility_key)))
        {
            self.stadium_inspector.analyze(
                &self.document,
                &self.frame_positions,
                self.camera,
                visibility_key,
                refresh_visibility == stadium_inspector::VisibilityRefresh::Manual,
            );
        }
        // Keep diagnosis navigation separate from the analysis controls: the
        // buttons operate on the fresh, exact camera/set/count result only,
        // and they never write drill data. A white selection ring provides a
        // non-colour-only focus cue in the stadium itself.
        if self.stadium_inspector.enabled && self.stadium_inspector.is_current(visibility_key) {
            let nearly_hidden = self
                .stadium_inspector
                .diagnostic_indexes(visibility_key, 0.25);
            let impaired = self
                .stadium_inspector
                .diagnostic_indexes(visibility_key, 0.75);
            let eligible_impaired = impaired
                .iter()
                .filter(|&&index| self.is_selectable_index(index))
                .count();
            egui::Area::new("stadium-visibility-review".into())
                .fixed_pos(rect.left_top() + Vec2::new(12.0, 178.0))
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.strong(super::i18n::registered(
                            self.locale,
                            "stadium-inspector.007",
                        ));
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .add_enabled(
                                    !nearly_hidden.is_empty(),
                                    egui::Button::new(format!(
                                        "{} ({})",
                                        super::i18n::registered(
                                            self.locale,
                                            "stadium-inspector.009"
                                        ),
                                        nearly_hidden.len()
                                    )),
                                )
                                .on_hover_text(super::i18n::registered(
                                    self.locale,
                                    "stadium-inspector.008",
                                ))
                                .clicked()
                            {
                                self.select_visibility_targets(&nearly_hidden);
                            }
                            if ui
                                .add_enabled(
                                    !impaired.is_empty(),
                                    egui::Button::new(format!(
                                        "{} ({})",
                                        super::i18n::registered(
                                            self.locale,
                                            "stadium-inspector.010"
                                        ),
                                        impaired.len()
                                    )),
                                )
                                .on_hover_text(super::i18n::registered(
                                    self.locale,
                                    "stadium-inspector.008",
                                ))
                                .clicked()
                            {
                                self.select_visibility_targets(&impaired);
                            }
                        });
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(
                                    eligible_impaired > 0,
                                    egui::Button::new(super::i18n::registered(
                                        self.locale,
                                        "stadium-inspector.011",
                                    )),
                                )
                                .clicked()
                            {
                                self.focus_visibility_target(&impaired, -1);
                            }
                            if ui
                                .add_enabled(
                                    eligible_impaired > 0,
                                    egui::Button::new(super::i18n::registered(
                                        self.locale,
                                        "stadium-inspector.012",
                                    )),
                                )
                                .clicked()
                            {
                                self.focus_visibility_target(&impaired, 1);
                            }
                        });
                        ui.small(format!(
                            "{}: {}",
                            super::i18n::registered(self.locale, "stadium-inspector.008"),
                            eligible_impaired
                        ));
                        ui.small(super::i18n::registered(
                            self.locale,
                            "stadium-inspector.013",
                        ));
                    });
                });
        }
        let grid = &self.document.grid;
        let vw = rect.width();
        let vh = rect.height();
        let origin = rect.left_top().to_vec2();
        let proj = |world: [f32; 3]| {
            self.camera
                .project(world, vw, vh)
                .map(|[x, y]| Pos2::new(x, y) + origin)
        };
        let field = |p: Point| proj(drill_core::camera::field_to_world(p, 0.0));

        let corners = [
            Point { x: 0.0, y: 0.0 },
            Point {
                x: grid.width,
                y: 0.0,
            },
            Point {
                x: grid.width,
                y: grid.height,
            },
            Point {
                x: 0.0,
                y: grid.height,
            },
        ];
        if let Some(poly) = corners
            .iter()
            .map(|&c| field(c))
            .collect::<Option<Vec<_>>>()
        {
            painter.add(egui::Shape::convex_polygon(
                poly,
                Color32::from_rgb(25, 71, 45),
                Stroke::new(2.0, Color32::from_gray(210)),
            ));
        }
        // Raked grandstands provide a stable depth frame without pretending to
        // model a particular venue.
        for (near_side, rows) in [(true, 8_u16), (false, 5_u16)] {
            for row in 0..rows {
                let depth = 4.0 + f32::from(row) * 1.2;
                let height = 0.5 + f32::from(row) * 0.65;
                let z = if near_side {
                    -depth
                } else {
                    grid.height + depth
                };
                if let (Some(a), Some(b)) = (proj([0.0, height, z]), proj([grid.width, height, z]))
                {
                    painter.line_segment([a, b], Stroke::new(1.0, Color32::from_rgb(76, 91, 108)));
                }
            }
        }
        let mut unit = 0.0;
        while unit <= grid.width + 0.001 {
            let major = (unit / (grid.major_line_interval * 2.0)).fract().abs() < 0.001;
            if let (Some(a), Some(b)) = (
                field(Point { x: unit, y: 0.0 }),
                field(Point {
                    x: unit,
                    y: grid.height,
                }),
            ) {
                painter.line_segment(
                    [a, b],
                    Stroke::new(
                        if major { 1.5 } else { 0.5 },
                        Color32::from_white_alpha(if major { 120 } else { 45 }),
                    ),
                );
            }
            unit += grid.major_line_interval.max(0.25);
        }
        // Mirror the vertical yard-line grid onto the depth axis so both read
        // with equal weight in the 3D stadium view too -- previously this axis
        // had only the (often empty) `grid.hashes` lines below.
        let mut depth = 0.0;
        while depth <= grid.height + 0.001 {
            let major = (depth / (grid.major_line_interval * 2.0)).fract().abs() < 0.001;
            if let (Some(a), Some(b)) = (
                field(Point { x: 0.0, y: depth }),
                field(Point {
                    x: grid.width,
                    y: depth,
                }),
            ) {
                painter.line_segment(
                    [a, b],
                    Stroke::new(
                        if major { 1.5 } else { 0.5 },
                        Color32::from_white_alpha(if major { 120 } else { 45 }),
                    ),
                );
            }
            depth += grid.major_line_interval.max(0.25);
        }
        // Center reference lines, bolder than the regular major grid --
        // mirrors `drill_render::Theme::center`.
        if let (Some(a), Some(b)) = (
            field(Point {
                x: grid.width * 0.5,
                y: 0.0,
            }),
            field(Point {
                x: grid.width * 0.5,
                y: grid.height,
            }),
        ) {
            painter.line_segment([a, b], Stroke::new(2.0, Color32::from_white_alpha(200)));
        }
        if let (Some(a), Some(b)) = (
            field(Point {
                x: 0.0,
                y: grid.height * 0.5,
            }),
            field(Point {
                x: grid.width,
                y: grid.height * 0.5,
            }),
        ) {
            painter.line_segment([a, b], Stroke::new(2.0, Color32::from_white_alpha(200)));
        }
        for hash in &grid.hashes {
            if let (Some(a), Some(b)) = (
                field(Point {
                    x: 0.0,
                    y: hash.position,
                }),
                field(Point {
                    x: grid.width,
                    y: hash.position,
                }),
            ) {
                painter.line_segment(
                    [a, b],
                    Stroke::new(hash.weight.clamp(0.25, 4.0), Color32::from_white_alpha(110)),
                );
            }
        }

        let gpu_stadium = self.gpu.as_ref().is_some_and(|gpu| gpu.active());
        if let Some(gpu) = self.gpu.as_ref().filter(|gpu| gpu.active()) {
            gpu.update_stadium(&self.document, &self.frame_positions, self.camera);
            painter.add(gpu.callback_stadium(rect, self.camera, self.stadium_inspector.lighting()));
        }
        let eye = self.camera.position();
        let depth = |i: usize| {
            let w = drill_core::camera::field_to_world(self.frame_positions[i], 0.0);
            let (dx, dy, dz) = (w[0] - eye[0], w[1] - eye[1], w[2] - eye[2]);
            dx * dx + dy * dy + dz * dz
        };
        let mut order: Vec<usize> = (0..self.frame_positions.len()).collect();
        order.sort_by(|&a, &b| depth(b).total_cmp(&depth(a)));
        let focal = vh / (2.0 * (self.camera.fov_y_rad * 0.5).tan());
        for i in order {
            if gpu_stadium {
                break;
            }
            let ground = drill_core::camera::field_to_world(self.frame_positions[i], 0.0);
            let Some(pos) = proj(ground) else { continue };
            let dist = depth(i).sqrt().max(0.001);
            let performer = &self.document.performers[i];
            let height_px = (performer.height_m * focal / dist).clamp(2.0, 80.0);
            let radius = (height_px * 0.22).clamp(1.5, 18.0);
            let color = performer.resolved_color(&self.document.sections);
            let color =
                drill_core::stadium::shade_color(color, self.stadium_inspector.lighting(), dist);
            painter.circle_filled(
                pos + Vec2::new(0.0, radius * 0.35),
                radius * 0.9,
                Color32::from_black_alpha(75),
            );
            let body_color = Color32::from_rgb(color[0], color[1], color[2]);
            match drill_core::stadium::choose_lod(
                height_px,
                &drill_core::stadium::LodThresholds::default(),
            ) {
                drill_core::stadium::PerformerLod::Billboard => {
                    painter.circle_filled(pos, radius, body_color);
                }
                drill_core::stadium::PerformerLod::SimpleFigure => {
                    let head = pos - Vec2::new(0.0, height_px * 0.78);
                    painter.line_segment(
                        [pos, head + Vec2::new(0.0, radius)],
                        Stroke::new(radius.max(2.0), body_color),
                    );
                    painter.circle_filled(head, radius * 0.75, body_color);
                }
                drill_core::stadium::PerformerLod::InstrumentSilhouette => {
                    let head = pos - Vec2::new(0.0, height_px * 0.82);
                    painter.circle_filled(head, radius * 0.65, body_color);
                    let body = egui::Rect::from_center_size(
                        pos - Vec2::new(0.0, height_px * 0.35),
                        Vec2::new(radius * 1.4, height_px * 0.55),
                    );
                    match performer.kind {
                        drill_core::PerformerKind::Wind => {
                            painter.add(egui::Shape::convex_polygon(
                                vec![
                                    body.left_bottom(),
                                    body.right_bottom(),
                                    Pos2::new(body.center().x, body.top()),
                                ],
                                body_color,
                                Stroke::NONE,
                            ));
                        }
                        drill_core::PerformerKind::Percussion => {
                            painter.rect_filled(body, 2.0, body_color);
                        }
                        drill_core::PerformerKind::Guard => {
                            painter.line_segment(
                                [body.left_bottom(), body.right_top()],
                                Stroke::new(radius.max(2.0), body_color),
                            );
                            painter.rect_filled(
                                egui::Rect::from_min_size(
                                    body.right_top() - Vec2::new(0.0, radius),
                                    Vec2::splat(radius * 1.3),
                                ),
                                1.0,
                                body_color,
                            );
                        }
                        drill_core::PerformerKind::Prop => {
                            painter.rect_filled(body, 0.0, body_color);
                        }
                    }
                }
            }
            if self.selected.contains(&i) {
                painter.circle_stroke(pos, radius + 3.0, Stroke::new(2.0, Color32::WHITE));
            }
            if let Some(visible) = self.stadium_inspector.visible_fraction(visibility_key, i)
                && visible < 0.75
            {
                let red = ((1.0 - visible) * 255.0).round() as u8;
                painter.circle_stroke(
                    pos,
                    radius + 5.0,
                    Stroke::new(2.5, Color32::from_rgb(red.max(120), 45, 45)),
                );
            }
        }
        if gpu_stadium && self.stadium_inspector.enabled {
            for (i, &point) in self.frame_positions.iter().enumerate() {
                let Some(visible) = self.stadium_inspector.visible_fraction(visibility_key, i)
                else {
                    continue;
                };
                if visible >= 0.75 {
                    continue;
                }
                let Some(pos) = proj(drill_core::camera::field_to_world(point, 0.0)) else {
                    continue;
                };
                let red = ((1.0 - visible) * 255.0).round() as u8;
                painter.circle_stroke(
                    pos,
                    7.0,
                    Stroke::new(2.5, Color32::from_rgb(red.max(120), 45, 45)),
                );
            }
        }
        painter.text(
            rect.left_top() + Vec2::new(12.0, 12.0),
            egui::Align2::LEFT_TOP,
            if self.camera_program_preview {
                "REAL VIEW · カメラショット追従中（ドラッグで解除）"
            } else {
                "REAL VIEW · 自由視点（ドラッグ回転・ホイールズーム）"
            },
            egui::FontId::proportional(12.0),
            Color32::from_white_alpha(170),
        );
    }
}

#[cfg(test)]
mod viewport_tests {
    use super::*;

    fn grid() -> GridConfig {
        GridConfig::default()
    }

    #[test]
    fn pan_is_bounded_to_the_field_at_high_magnification() {
        let grid = grid();
        let mut view = FieldViewport::fit(&grid);
        view.zoom = 4.0;
        view.pan_pixels(
            Vec2::new(-100_000.0, 100_000.0),
            &grid,
            Vec2::new(800.0, 500.0),
        );
        assert!(view.center.x >= 0.0 && view.center.x <= grid.width);
        assert!(view.center.y >= 0.0 && view.center.y <= grid.height);
    }

    #[test]
    fn zoom_keeps_the_point_below_the_pointer_fixed() {
        let grid = grid();
        let rect = Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(900.0, 500.0));
        let pointer = Pos2::new(580.0, 245.0);
        let mut view = FieldViewport::fit(&grid);
        let before = drill_render::FieldMap::with_view(
            grid.width,
            grid.height,
            drill_render::Vec2 {
                x: rect.width(),
                y: rect.height(),
            },
            0.0,
            Some(view.center),
            view.zoom,
        )
        .unmap(drill_render::Vec2 {
            x: pointer.x - rect.left(),
            y: pointer.y - rect.top(),
        });
        view.zoom_at(2.0, pointer, rect, &grid);
        let after = drill_render::FieldMap::with_view(
            grid.width,
            grid.height,
            drill_render::Vec2 {
                x: rect.width(),
                y: rect.height(),
            },
            0.0,
            Some(view.center),
            view.zoom,
        )
        .unmap(drill_render::Vec2 {
            x: pointer.x - rect.left(),
            y: pointer.y - rect.top(),
        });
        assert!((before.x - after.x).abs() < 0.001);
        assert!((before.y - after.y).abs() < 0.001);
    }

    #[test]
    fn friction_travel_is_frame_rate_independent() {
        // The whole point of the `powf` form: one 1/60 s step must land in the
        // same place as four 1/240 s steps, or the glide is longer on a slow
        // machine than a fast one.
        let mut coarse_v = 40.0_f32;
        let coarse = friction_travel(coarse_v, 1.0 / 60.0);
        coarse_v *= friction_decay(1.0 / 60.0);

        let mut fine_v = 40.0_f32;
        let mut fine = 0.0;
        for _ in 0..4 {
            fine += friction_travel(fine_v, 1.0 / 240.0);
            fine_v *= friction_decay(1.0 / 240.0);
        }
        assert!((coarse - fine).abs() < 1e-4, "{coarse} vs {fine}");
        assert!((coarse_v - fine_v).abs() < 1e-4, "{coarse_v} vs {fine_v}");
    }

    #[test]
    fn friction_brings_a_glide_to_rest_within_about_a_second() {
        // A release at the velocity cap (see `MAX_FLING_FIELDS_PER_SEC`) is
        // the longest glide the canvas can produce; it should still settle in
        // close to a second, not linger.
        let mut velocity = 100.0_f32;
        let mut elapsed = 0.0_f32;
        while velocity.abs() > super::feel::PAN_STOP_SPEED && elapsed < 5.0 {
            velocity *= friction_decay(1.0 / 120.0);
            elapsed += 1.0 / 120.0;
        }
        assert!(
            (0.7..1.3).contains(&elapsed),
            "glide settled in {elapsed}s, expected close to a second"
        );
    }

    #[test]
    fn rubber_offset_is_monotonic_bounded_and_zero_at_rest() {
        let limit = 12.0;
        assert_eq!(rubber_offset(0.0, limit), 0.0);
        assert_eq!(rubber_offset(-5.0, limit), 0.0);
        assert_eq!(rubber_offset(5.0, 0.0), 0.0);
        let mut previous = 0.0;
        let mut raw = 0.0;
        while raw < 500.0 {
            raw += 0.25;
            let offset = rubber_offset(raw, limit);
            assert!(offset > previous, "not monotonic at raw={raw}");
            assert!(offset < limit, "escaped the asymptote at raw={raw}");
            previous = offset;
        }
        // Resistance grows: the first unit of pull buys far more travel than
        // the ten-thousandth.
        assert!(rubber_offset(1.0, limit) - rubber_offset(0.0, limit) > 0.4);
        assert!(rubber_offset(10_000.0, limit) - rubber_offset(9_999.0, limit) < 1e-4);
    }

    #[test]
    fn rubber_raw_inverts_rubber_offset() {
        let limit = 9.0;
        for raw in [0.5_f32, 2.0, 7.5, 30.0, 120.0] {
            let round_trip = rubber_raw(rubber_offset(raw, limit), limit);
            assert!((round_trip - raw).abs() < raw * 1e-3 + 1e-3, "{raw}");
        }
    }

    #[test]
    fn rubber_axis_tracks_the_pointer_inside_the_range_and_resists_outside() {
        let (min, max, limit) = (10.0, 90.0, 8.0);
        // Fully inside: one-to-one.
        assert!((rubber_axis(50.0, 5.0, min, max, limit) - 55.0).abs() < 1e-5);
        assert!((rubber_axis(50.0, -5.0, min, max, limit) - 45.0).abs() < 1e-5);
        // Crossing the edge: the part inside is free, the part outside is not.
        let crossed = rubber_axis(88.0, 10.0, min, max, limit);
        assert!(crossed > max && crossed < max + limit);
        assert!(crossed < 98.0, "overscroll was not resisted");
        // Symmetric on the low side: pulling out from a point 2 units inside
        // `min` by the same total delta as the high-side case above should
        // leave the same *distance past the edge*, mirrored.
        let low = rubber_axis(12.0, -10.0, min, max, limit);
        assert!(
            ((min - low) - (crossed - max)).abs() < 1e-4,
            "{low} vs {crossed}"
        );
        // Never past the asymptote, however hard you pull.
        assert!(rubber_axis(max, 100_000.0, min, max, limit) < max + limit);
        // Coming back in is unresisted and lands exactly where the pointer says.
        let out = rubber_axis(max, 20.0, min, max, limit);
        let back = rubber_axis(out, -(out - max) - 5.0, min, max, limit);
        assert!((back - (max - 5.0)).abs() < 1e-4, "{back}");
    }

    #[test]
    fn rubber_axis_handles_a_pinned_axis() {
        // When the whole field fits, `axis_bounds` collapses the range to a
        // point; overscroll must still work in both directions from it.
        let (min, max) = axis_bounds(80.0, 100.0);
        assert!((min - 50.0).abs() < 1e-6 && (max - 50.0).abs() < 1e-6);
        let out = rubber_axis(min, 30.0, min, max, 6.0);
        assert!(out > min && out < min + 6.0);
        let back = rubber_axis(min, -30.0, min, max, 6.0);
        assert!(back < min && back > min - 6.0);
    }

    #[test]
    fn spring_returns_to_the_edge_without_overshooting() {
        let mut offset = 6.0_f32;
        let mut velocity = 0.0_f32;
        let mut elapsed = 0.0_f32;
        while offset.abs() > 1e-3 && elapsed < 3.0 {
            let (next_offset, next_velocity) = spring_step(offset, velocity, 1.0 / 120.0);
            assert!(next_offset >= -1e-4, "critically damped spring overshot");
            assert!(next_offset <= offset + 1e-6, "spring moved away from rest");
            offset = next_offset;
            velocity = next_velocity;
            elapsed += 1.0 / 120.0;
        }
        // `5/omega` (~0.28s) is the amplitude-decay rule of thumb; settling
        // all the way to a tight 1e-3 absolute threshold from a real 6-unit
        // overscroll takes a bit longer in practice, which the wider window
        // below allows for.
        assert!(
            (0.15..0.75).contains(&elapsed),
            "rubber band settled in {elapsed}s"
        );
    }

    #[test]
    fn spring_absorbs_momentum_arriving_at_the_edge() {
        // Momentum carrying the view past the edge must bounce, not dead-stop:
        // it keeps travelling outward briefly, then comes home.
        let mut offset = 0.0_f32;
        let mut velocity = 60.0_f32;
        let mut peak = 0.0_f32;
        for _ in 0..600 {
            let (next_offset, next_velocity) = spring_step(offset, velocity, 1.0 / 120.0);
            offset = next_offset;
            velocity = next_velocity;
            peak = peak.max(offset);
        }
        assert!(peak > 0.5, "momentum did not carry past the edge at all");
        assert!(offset.abs() < 1e-2, "never returned to rest, at {offset}");
    }

    #[test]
    fn animated_zoom_keeps_the_anchor_under_the_cursor_every_frame() {
        let grid = grid();
        let rect = Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(900.0, 500.0));
        // Off-center pointer near a corner, where any anchoring error shows up
        // fastest.
        let pointer = Pos2::new(760.0, 140.0);
        let local = drill_render::Vec2 {
            x: pointer.x - rect.left(),
            y: pointer.y - rect.top(),
        };
        let mut view = FieldViewport::fit(&grid);
        let sample = |view: &FieldViewport| {
            drill_render::FieldMap::with_view(
                grid.width,
                grid.height,
                drill_render::Vec2 {
                    x: rect.width(),
                    y: rect.height(),
                },
                0.0,
                Some(view.center),
                view.zoom,
            )
            .unmap(local)
        };
        let pinned = sample(&view);
        view.zoom_toward(2.5, pointer, rect, &grid);
        let mut frames = 0;
        while view.tick(1.0 / 144.0, &grid, rect) && frames < 2000 {
            frames += 1;
            let now = sample(&view);
            assert!(
                (now.x - pinned.x).abs() < 0.02 && (now.y - pinned.y).abs() < 0.02,
                "anchor slid on frame {frames}: {now:?} vs {pinned:?}"
            );
        }
        assert!(frames > 5, "zoom snapped instead of animating");
        assert!((view.zoom - 2.5).abs() < 1e-3, "zoom {} ", view.zoom);
    }

    #[test]
    fn zoom_target_stays_inside_the_supported_range() {
        let grid = grid();
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 500.0));
        let pointer = rect.center();
        let mut view = FieldViewport::fit(&grid);
        for _ in 0..40 {
            view.zoom_toward(2.0, pointer, rect, &grid);
        }
        for _ in 0..4000 {
            if !view.tick(1.0 / 60.0, &grid, rect) {
                break;
            }
        }
        assert!((view.zoom - MAX_ZOOM).abs() < 1e-3, "{}", view.zoom);
        for _ in 0..80 {
            view.zoom_toward(0.5, pointer, rect, &grid);
        }
        for _ in 0..4000 {
            if !view.tick(1.0 / 60.0, &grid, rect) {
                break;
            }
        }
        assert!((view.zoom - MIN_ZOOM).abs() < 1e-3, "{}", view.zoom);
    }

    #[test]
    fn tick_stops_asking_for_repaints_once_everything_is_at_rest() {
        let grid = grid();
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 500.0));
        let mut view = FieldViewport::fit(&grid);
        view.zoom = 4.0;
        view.zoom_target = 4.0;
        view.begin_pan(Some(Pos2::new(400.0, 250.0)));
        for step in 0..10 {
            view.drag_pan(Vec2::new(-14.0, 0.0), 1.0 / 120.0, &grid, rect.size());
            let _ = step;
        }
        view.end_pan();
        let mut frames = 0;
        while view.tick(1.0 / 120.0, &grid, rect) {
            frames += 1;
            assert!(frames < 5000, "animation never settled");
        }
        assert!(frames > 5, "release produced no glide at all");
        assert!(!view.tick(1.0 / 120.0, &grid, rect));
    }

    #[test]
    fn a_parked_pointer_releases_without_flinging() {
        let grid = grid();
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 500.0));
        let mut view = FieldViewport::fit(&grid);
        view.zoom = 4.0;
        view.zoom_target = 4.0;
        view.begin_pan(Some(Pos2::new(400.0, 250.0)));
        // Fast approach...
        for _ in 0..10 {
            view.drag_pan(Vec2::new(-20.0, 0.0), 1.0 / 120.0, &grid, rect.size());
        }
        // ...then carefully hold still for a tenth of a second before letting
        // go. The view must not sail off.
        for _ in 0..12 {
            view.drag_pan(Vec2::ZERO, 1.0 / 120.0, &grid, rect.size());
        }
        view.end_pan();
        let settled = view.center;
        for _ in 0..600 {
            if !view.tick(1.0 / 120.0, &grid, rect) {
                break;
            }
        }
        assert!(
            (view.center.x - settled.x).abs() < 0.05,
            "parked release drifted by {}",
            (view.center.x - settled.x).abs()
        );
    }

    #[test]
    fn interactive_pan_overscrolls_then_springs_back_to_the_edge() {
        let grid = grid();
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 500.0));
        let mut view = FieldViewport::fit(&grid);
        view.zoom = 4.0;
        view.zoom_target = 4.0;
        let (min_x, ..) = view.travel(&grid, rect.size());
        view.center.x = min_x;
        view.begin_pan(Some(Pos2::new(400.0, 250.0)));
        for _ in 0..30 {
            view.drag_pan(Vec2::new(24.0, 0.0), 1.0 / 120.0, &grid, rect.size());
        }
        assert!(
            view.center.x < min_x - 0.05,
            "the edge did not give at all: {}",
            view.center.x
        );
        view.end_pan();
        for _ in 0..2000 {
            if !view.tick(1.0 / 120.0, &grid, rect) {
                break;
            }
        }
        assert!(
            (view.center.x - min_x).abs() < 1e-2,
            "did not return to the edge: {} vs {min_x}",
            view.center.x
        );
    }

    #[test]
    fn dot_lift_and_settle_ease_and_expire() {
        let grid = grid();
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 500.0));
        let mut view = FieldViewport::fit(&grid);
        assert_eq!(view.dot_lift(), 0.0);
        view.set_dot_drag(true);
        for _ in 0..30 {
            view.tick(1.0 / 120.0, &grid, rect);
        }
        assert!(
            view.dot_lift() > 0.95,
            "lift never rose: {}",
            view.dot_lift()
        );
        view.dots_landed();
        assert!(view.dot_settle_phase() > 0.99);
        let mut frames = 0;
        while view.tick(1.0 / 120.0, &grid, rect) {
            frames += 1;
            assert!(frames < 1000);
        }
        assert_eq!(view.dot_lift(), 0.0);
        assert_eq!(view.dot_settle_phase(), 0.0);
    }

    #[test]
    fn frame_bounds_zooms_in_on_a_small_group() {
        let grid = grid();
        let size = Vec2::new(800.0, 600.0);
        let mut view = FieldViewport::fit(&grid);
        view.frame_bounds(
            &grid,
            size,
            Point { x: 45.0, y: 20.0 },
            Point { x: 55.0, y: 30.0 },
        );
        assert!(view.zoom > 2.0, "zoom stayed wide: {}", view.zoom);
        assert!((view.center.x - 50.0).abs() < 1.0, "{}", view.center.x);
        assert!((view.center.y - 25.0).abs() < 2.0, "{}", view.center.y);
    }
}
