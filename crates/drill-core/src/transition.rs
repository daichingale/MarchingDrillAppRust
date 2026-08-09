use crate::{DrillError, Performer, PerformerId, Point};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_SET_COUNTS: u32 = 4_096;
pub const MAX_VIA_POINTS: usize = 256;
const ARC_RESOLUTION: usize = 32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetCounts {
    pub moves: u16,
    pub hold: u16,
}
impl SetCounts {
    pub const fn from_legacy(moves: u16) -> Self {
        Self { moves, hold: 0 }
    }
    pub const fn total(self) -> u32 {
        self.moves as u32 + self.hold as u32
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gate {
    pub depart: f32,
    pub arrive: Option<f32>,
}
impl Gate {
    pub const FULL: Self = Self {
        depart: 0.0,
        arrive: None,
    };
    pub fn is_full(&self) -> bool {
        *self == Self::FULL
    }
    pub fn resolve(self, moves: f32) -> (f32, f32) {
        let moves = if moves.is_finite() {
            moves.max(0.0)
        } else {
            0.0
        };
        let depart = if self.depart.is_finite() {
            self.depart.clamp(0.0, moves)
        } else {
            0.0
        };
        let arrive = self
            .arrive
            .filter(|v| v.is_finite())
            .unwrap_or(moves)
            .clamp(depart, moves);
        (depart, arrive)
    }
}
impl Default for Gate {
    fn default() -> Self {
        Self::FULL
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChordPoint {
    pub along: f32,
    pub lateral: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PathVia {
    Relative(Vec<ChordPoint>),
    Absolute(Vec<Point>),
}
impl Default for PathVia {
    fn default() -> Self {
        Self::Relative(Vec::new())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum RouteShape {
    #[default]
    Straight,
    Curve {
        control: ChordPoint,
    },
    Path {
        via: PathVia,
    },
    Arc {
        bulge: f32,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Easing {
    #[default]
    Linear,
    Ramp {
        in_counts: f32,
        out_counts: f32,
    },
    EaseIn,
    EaseOut,
    Smooth,
    Custom {
        c1: f32,
        c2: f32,
    },
}
impl Easing {
    pub fn apply(self, u: f32, window: f32) -> f32 {
        let u = u.clamp(0.0, 1.0);
        match self {
            Self::Linear => u,
            Self::EaseIn => u * u,
            Self::EaseOut => u * (2.0 - u),
            Self::Smooth => u * u * (3.0 - 2.0 * u),
            Self::Custom { c1, c2 } => {
                let v = 1.0 - u;
                3.0 * v * v * u * c1 + 3.0 * v * u * u * c2 + u * u * u
            }
            Self::Ramp {
                in_counts,
                out_counts,
            } => {
                let w = window.max(f32::EPSILON);
                let mut a = in_counts.max(0.0).min(w);
                let mut b = out_counts.max(0.0).min(w);
                if a + b > w {
                    let scale = w / (a + b);
                    a *= scale;
                    b *= scale;
                }
                let d = (w - (a + b) * 0.5).max(f32::EPSILON);
                let t = u * w;
                if a > 0.0 && t < a {
                    t * t / (2.0 * a * d)
                } else if b > 0.0 && t > w - b {
                    1.0 - (w - t) * (w - t) / (2.0 * b * d)
                } else {
                    (a * 0.5 + t - a) / d
                }
            }
        }
        .clamp(0.0, 1.0)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Route {
    #[serde(default, skip_serializing_if = "is_straight")]
    pub shape: RouteShape,
    #[serde(default, skip_serializing_if = "Gate::is_full")]
    pub gate: Gate,
    #[serde(default, skip_serializing_if = "is_linear")]
    pub easing: Easing,
}
fn is_straight(v: &RouteShape) -> bool {
    matches!(v, RouteShape::Straight)
}
fn is_linear(v: &Easing) -> bool {
    matches!(v, Easing::Linear)
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RouteTable {
    #[serde(default, skip_serializing_if = "is_default_route")]
    pub default: Route,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub overrides: BTreeMap<PerformerId, Route>,
}
fn is_default_route(v: &Route) -> bool {
    *v == Route::default()
}
impl RouteTable {
    pub fn route_for(&self, id: PerformerId) -> &Route {
        self.overrides.get(&id).unwrap_or(&self.default)
    }
    pub fn is_trivial(&self) -> bool {
        self.overrides.is_empty() && is_default_route(&self.default)
    }
    pub fn validate(&self, performers: &[Performer], counts: SetCounts) -> Result<(), DrillError> {
        if counts.total() > MAX_SET_COUNTS {
            return Err(DrillError::InvalidTransition);
        }
        let ids = performers.iter().map(|p| p.id).collect::<BTreeSet<_>>();
        if self.overrides.len() > ids.len() || self.overrides.keys().any(|id| !ids.contains(id)) {
            return Err(DrillError::InvalidTransition);
        }
        for route in std::iter::once(&self.default).chain(self.overrides.values()) {
            validate_route(route, counts.moves)?;
        }
        Ok(())
    }
    pub fn clamped_to(&self, counts: SetCounts) -> Option<Self> {
        let mut next = self.clone();
        let mut changed = false;
        for route in std::iter::once(&mut next.default).chain(next.overrides.values_mut()) {
            let (depart, arrive) = route.gate.resolve(f32::from(counts.moves));
            let gate = Gate {
                depart,
                arrive: route.gate.arrive.map(|_| arrive),
            };
            changed |= gate != route.gate;
            route.gate = gate;
        }
        changed.then_some(next)
    }
}
fn validate_route(route: &Route, moves: u16) -> Result<(), DrillError> {
    let m = f32::from(moves);
    let (d, a) = route.gate.resolve(m);
    if !route.gate.depart.is_finite()
        || d != route.gate.depart
        || route.gate.arrive.is_some_and(|v| !v.is_finite() || v != a)
    {
        return Err(DrillError::InvalidTransition);
    }
    let finite = |v: f32| v.is_finite();
    match &route.shape {
        RouteShape::Straight => {}
        RouteShape::Curve { control } if finite(control.along) && finite(control.lateral) => {}
        RouteShape::Arc { bulge } if finite(*bulge) => {}
        RouteShape::Path { via } => {
            let n = match via {
                PathVia::Relative(v) => {
                    if v.iter().any(|p| !finite(p.along) || !finite(p.lateral)) {
                        return Err(DrillError::InvalidTransition);
                    }
                    v.len()
                }
                PathVia::Absolute(v) => {
                    if v.iter().any(|p| !finite(p.x) || !finite(p.y)) {
                        return Err(DrillError::InvalidTransition);
                    }
                    v.len()
                }
            };
            if n > MAX_VIA_POINTS {
                return Err(DrillError::InvalidTransition);
            }
        }
        _ => return Err(DrillError::InvalidTransition),
    }
    match route.easing {
        Easing::Ramp {
            in_counts,
            out_counts,
        } if !finite(in_counts) || !finite(out_counts) || in_counts < 0.0 || out_counts < 0.0 => {
            Err(DrillError::InvalidTransition)
        }
        Easing::Custom { c1, c2 }
            if !finite(c1) || !finite(c2) || !(0.0..=c2).contains(&c1) || c2 > 1.0 =>
        {
            Err(DrillError::InvalidTransition)
        }
        _ => Ok(()),
    }
}

pub fn evaluate(route: &Route, start: Point, end: Point, local_count: f32, moves: u16) -> Point {
    let (depart, arrive) = route.gate.resolve(f32::from(moves));
    if local_count <= depart {
        return start;
    }
    if local_count >= arrive {
        return end;
    }
    let span = arrive - depart;
    let u = route.easing.apply(
        (local_count - depart) * (1.0 / span.max(f32::EPSILON)),
        span,
    );
    let t = match &route.shape {
        RouteShape::Curve { control } => curve_t_at(*control, u),
        _ => u,
    };
    sample_shape(&route.shape, start, end, t)
}
fn sample_shape(shape: &RouteShape, start: Point, end: Point, t: f32) -> Point {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let field = |a: f32, l: f32| Point {
        x: start.x + dx * a - dy * l,
        y: start.y + dy * a + dx * l,
    };
    match shape {
        RouteShape::Straight => start.lerp(end, t),
        RouteShape::Curve { control } => {
            let v = 1.0 - t;
            field(
                2.0 * v * t * control.along + t * t,
                2.0 * v * t * control.lateral,
            )
        }
        RouteShape::Path { via } => sample_path(via, start, end, t),
        RouteShape::Arc { bulge } if bulge.abs() >= 1e-4 => {
            let theta = 4.0 * (2.0 * bulge).atan();
            let center = field(0.5, (4.0 * bulge * bulge - 1.0) / (8.0 * bulge));
            let a0 = (start.y - center.y).atan2(start.x - center.x);
            let r = ((start.x - center.x).powi(2) + (start.y - center.y).powi(2)).sqrt();
            Point {
                x: center.x + r * (a0 - theta * t).cos(),
                y: center.y + r * (a0 - theta * t).sin(),
            }
        }
        RouteShape::Arc { .. } => start.lerp(end, t),
    }
}

#[inline]
fn distance(a: Point, b: Point) -> f32 {
    ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt()
}

fn curve_t_at(control: ChordPoint, fraction: f32) -> f32 {
    let mut cumulative = [0.0; ARC_RESOLUTION + 1];
    let sample = |t: f32| {
        let v = 1.0 - t;
        Point {
            x: 2.0 * v * t * control.along + t * t,
            y: 2.0 * v * t * control.lateral,
        }
    };
    let mut previous = sample(0.0);
    for index in 1..=ARC_RESOLUTION {
        let point = sample(index as f32 / ARC_RESOLUTION as f32);
        cumulative[index] = cumulative[index - 1] + distance(previous, point);
        previous = point;
    }
    inverse_arc_table(&cumulative, fraction)
}

fn inverse_arc_table(cumulative: &[f32], fraction: f32) -> f32 {
    let total = cumulative.last().copied().unwrap_or_default();
    if fraction <= 0.0 || total <= f32::EPSILON {
        return 0.0;
    }
    if fraction >= 1.0 {
        return 1.0;
    }
    let target = fraction * total;
    let upper = cumulative.partition_point(|&value| value < target).max(1);
    let lower = upper - 1;
    let span = (cumulative[upper] - cumulative[lower]).max(f32::EPSILON);
    (lower as f32 + (target - cumulative[lower]) / span) / (cumulative.len() - 1) as f32
}

fn path_point(via: &PathVia, start: Point, end: Point, index: usize) -> Point {
    let count = match via {
        PathVia::Relative(v) => v.len(),
        PathVia::Absolute(v) => v.len(),
    };
    if index == 0 {
        return start;
    }
    if index == count + 1 {
        return end;
    }
    match via {
        PathVia::Relative(v) => {
            let p = v[index - 1];
            let dx = end.x - start.x;
            let dy = end.y - start.y;
            Point {
                x: start.x + dx * p.along - dy * p.lateral,
                y: start.y + dy * p.along + dx * p.lateral,
            }
        }
        PathVia::Absolute(v) => v[index - 1],
    }
}

fn sample_path(via: &PathVia, start: Point, end: Point, fraction: f32) -> Point {
    let via_len = match via {
        PathVia::Relative(v) => v.len(),
        PathVia::Absolute(v) => v.len(),
    };
    let mut total = 0.0;
    let mut previous = start;
    for index in 1..=via_len + 1 {
        let point = path_point(via, start, end, index);
        total += distance(previous, point);
        previous = point;
    }
    if total <= f32::EPSILON {
        return end;
    }
    let mut target = fraction.clamp(0.0, 1.0) * total;
    previous = start;
    for index in 1..=via_len + 1 {
        let point = path_point(via, start, end, index);
        let length = distance(previous, point);
        if target <= length || index == via_len + 1 {
            return previous.lerp(point, target / length.max(f32::EPSILON));
        }
        target -= length;
        previous = point;
    }
    end
}

pub fn arrival_count(route: &Route, moves: u16) -> f32 {
    route.gate.resolve(f32::from(moves)).1
}
pub fn route_length(route: &Route, start: Point, end: Point) -> f32 {
    let mut total = 0.0;
    let mut prev = start;
    for i in 1..=64 {
        let p = sample_shape(&route.shape, start, end, i as f32 / 64.0);
        total += ((p.x - prev.x).powi(2) + (p.y - prev.y).powi(2)).sqrt();
        prev = p
    }
    total
}

/// Precomputed inverse arc-length map. Building may allocate; querying never does.
#[derive(Clone, Debug, PartialEq)]
pub struct ArcTable {
    cumulative: Box<[f32]>,
    points: Box<[Point]>,
    total: f32,
}

impl ArcTable {
    pub const RESOLUTION: usize = ARC_RESOLUTION;

    pub fn build(sample: impl Fn(f32) -> (f32, f32)) -> Self {
        let mut points = Vec::with_capacity(Self::RESOLUTION + 1);
        for index in 0..=Self::RESOLUTION {
            let (x, y) = sample(index as f32 / Self::RESOLUTION as f32);
            points.push(Point { x, y });
        }
        Self::from_points(points)
    }

    pub fn build_polyline(points: &[(f32, f32)]) -> Self {
        Self::from_points(points.iter().map(|&(x, y)| Point { x, y }).collect())
    }

    fn from_points(points: Vec<Point>) -> Self {
        let mut cumulative = Vec::with_capacity(points.len());
        cumulative.push(0.0);
        let mut total = 0.0;
        for pair in points.windows(2) {
            total += distance(pair[0], pair[1]);
            cumulative.push(total);
        }
        Self {
            cumulative: cumulative.into_boxed_slice(),
            points: points.into_boxed_slice(),
            total,
        }
    }

    #[inline]
    pub fn t_at(&self, fraction: f32) -> f32 {
        inverse_arc_table(&self.cumulative, fraction)
    }

    #[inline]
    pub fn length(&self, chord_length: f32) -> f32 {
        self.total * chord_length
    }

    #[inline]
    fn point_at(&self, fraction: f32) -> Point {
        if self.points.len() < 2 {
            return self.points.first().copied().unwrap_or_default();
        }
        let t = self.t_at(fraction) * (self.points.len() - 1) as f32;
        let index = (t.floor() as usize).min(self.points.len() - 2);
        self.points[index].lerp(self.points[index + 1], t - index as f32)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum LaneKind {
    Straight,
    Curve,
    Path,
    Arc,
}

#[derive(Clone, Copy, Debug)]
struct Lane {
    start: Point,
    end: Point,
    depart: f32,
    inv_span: f32,
    easing: Easing,
    kind: LaneKind,
    table: u16,
    param: [f32; 2],
}

const NO_TABLE: u16 = u16::MAX;

/// Flat, reusable representation for real-time transition playback.
#[derive(Debug, Default)]
pub struct TransitionPlan {
    set_index: usize,
    source_signature: u64,
    counts: SetCounts,
    lanes: Vec<Lane>,
    tables: Vec<ArcTable>,
}

impl TransitionPlan {
    pub fn performer_count(&self) -> usize {
        self.lanes.len()
    }
    pub fn set_index(&self) -> usize {
        self.set_index
    }
    pub fn counts(&self) -> SetCounts {
        self.counts
    }
}

impl crate::Document {
    /// Compiles one transition, retaining the plan's lane/table capacities.
    pub fn plan_transition(&self, set: crate::SetId, plan: &mut TransitionPlan) {
        let Some(index) = self.sets.iter().position(|candidate| candidate.id == set) else {
            plan.lanes.clear();
            plan.tables.clear();
            return;
        };
        let from = &self.sets[index];
        let to = self.sets.get(index + 1).unwrap_or(from);
        let signature = transition_signature(self, index);
        if plan.source_signature == signature
            && plan.set_index == index
            && plan.lanes.len() == from.positions.len()
        {
            return;
        }
        plan.set_index = index;
        plan.source_signature = signature;
        plan.counts = SetCounts {
            moves: from.counts,
            hold: from.hold,
        };
        plan.lanes.clear();
        plan.tables.clear();
        plan.lanes
            .reserve(from.positions.len().saturating_sub(plan.lanes.capacity()));
        let default_curve_table = if let RouteShape::Curve { control } = &from.routes.default.shape
        {
            Some(push_table(&mut plan.tables, curve_table(*control)))
        } else {
            None
        };
        for (performer_index, (&start, &end)) in
            from.positions.iter().zip(&to.positions).enumerate()
        {
            let route = self
                .performers
                .get(performer_index)
                .map(|p| from.routes.route_for(p.id))
                .unwrap_or(&from.routes.default);
            let (depart, arrive) = route.gate.resolve(f32::from(from.counts));
            let mut lane = Lane {
                start,
                end,
                depart,
                inv_span: if arrive > depart {
                    1.0 / (arrive - depart)
                } else {
                    0.0
                },
                easing: route.easing,
                kind: LaneKind::Straight,
                table: NO_TABLE,
                param: [0.0; 2],
            };
            match &route.shape {
                RouteShape::Straight => {}
                RouteShape::Curve { control } => {
                    lane.kind = LaneKind::Curve;
                    lane.param = [control.along, control.lateral];
                    lane.table = if std::ptr::eq(route, &from.routes.default) {
                        default_curve_table.expect("default curve table was compiled")
                    } else {
                        push_table(&mut plan.tables, curve_table(*control))
                    };
                }
                RouteShape::Path { via } => {
                    lane.kind = LaneKind::Path;
                    let count = match via {
                        PathVia::Relative(v) => v.len(),
                        PathVia::Absolute(v) => v.len(),
                    };
                    let points = (0..=count + 1)
                        .map(|i| path_point(via, start, end, i))
                        .map(|p| (p.x, p.y))
                        .collect::<Vec<_>>();
                    lane.table = push_table(&mut plan.tables, ArcTable::build_polyline(&points));
                }
                RouteShape::Arc { bulge } => {
                    lane.kind = LaneKind::Arc;
                    lane.param[0] = *bulge;
                }
            }
            plan.lanes.push(lane);
        }
    }
}

fn transition_signature(document: &crate::Document, index: usize) -> u64 {
    #[inline]
    fn mix(hash: &mut u64, value: u64) {
        *hash ^= value;
        *hash = hash.wrapping_mul(0x100000001b3);
    }
    fn mix_route(hash: &mut u64, route: &Route) {
        mix(hash, route.gate.depart.to_bits() as u64);
        mix(
            hash,
            route.gate.arrive.map_or(u32::MAX, f32::to_bits) as u64,
        );
        match route.easing {
            Easing::Linear => mix(hash, 0),
            Easing::Ramp {
                in_counts,
                out_counts,
            } => {
                mix(hash, 1);
                mix(hash, in_counts.to_bits() as u64);
                mix(hash, out_counts.to_bits() as u64);
            }
            Easing::EaseIn => mix(hash, 2),
            Easing::EaseOut => mix(hash, 3),
            Easing::Smooth => mix(hash, 4),
            Easing::Custom { c1, c2 } => {
                mix(hash, 5);
                mix(hash, c1.to_bits() as u64);
                mix(hash, c2.to_bits() as u64);
            }
        }
        match &route.shape {
            RouteShape::Straight => mix(hash, 10),
            RouteShape::Curve { control } => {
                mix(hash, 11);
                mix(hash, control.along.to_bits() as u64);
                mix(hash, control.lateral.to_bits() as u64);
            }
            RouteShape::Arc { bulge } => {
                mix(hash, 12);
                mix(hash, bulge.to_bits() as u64);
            }
            RouteShape::Path { via } => {
                mix(hash, 13);
                match via {
                    PathVia::Relative(points) => {
                        for point in points {
                            mix(hash, point.along.to_bits() as u64);
                            mix(hash, point.lateral.to_bits() as u64);
                        }
                    }
                    PathVia::Absolute(points) => {
                        for point in points {
                            mix(hash, point.x.to_bits() as u64);
                            mix(hash, point.y.to_bits() as u64);
                        }
                    }
                }
            }
        }
    }
    let Some(from) = document.sets.get(index) else {
        return 0;
    };
    let to = document.sets.get(index + 1).unwrap_or(from);
    let mut hash = 0xcbf29ce484222325;
    mix(&mut hash, from.id.get() as u64);
    mix(&mut hash, from.counts as u64);
    mix(&mut hash, from.hold as u64);
    for ((start, end), performer) in from
        .positions
        .iter()
        .zip(&to.positions)
        .zip(&document.performers)
    {
        mix(&mut hash, performer.id.get() as u64);
        mix(&mut hash, start.x.to_bits() as u64);
        mix(&mut hash, start.y.to_bits() as u64);
        mix(&mut hash, end.x.to_bits() as u64);
        mix(&mut hash, end.y.to_bits() as u64);
        mix_route(&mut hash, from.routes.route_for(performer.id));
    }
    hash.max(1)
}

fn curve_table(control: ChordPoint) -> ArcTable {
    ArcTable::build(|t| {
        let v = 1.0 - t;
        (
            2.0 * v * t * control.along + t * t,
            2.0 * v * t * control.lateral,
        )
    })
}

fn push_table(tables: &mut Vec<ArcTable>, table: ArcTable) -> u16 {
    let index = tables.len();
    tables.push(table);
    u16::try_from(index).unwrap_or(NO_TABLE)
}

#[inline]
fn eval_lane(plan: &TransitionPlan, lane: &Lane, local_count: f32) -> Point {
    if local_count <= lane.depart {
        return lane.start;
    }
    if lane.inv_span == 0.0 {
        return lane.end;
    }
    let u = ((local_count - lane.depart) * lane.inv_span).clamp(0.0, 1.0);
    if u >= 1.0 {
        return lane.end;
    }
    let e = lane.easing.apply(u, 1.0 / lane.inv_span);
    match lane.kind {
        LaneKind::Straight => lane.start.lerp(lane.end, e),
        LaneKind::Curve => {
            let t = plan.tables[lane.table as usize].t_at(e);
            let v = 1.0 - t;
            let dx = lane.end.x - lane.start.x;
            let dy = lane.end.y - lane.start.y;
            let along = 2.0 * v * t * lane.param[0] + t * t;
            let lateral = 2.0 * v * t * lane.param[1];
            Point {
                x: lane.start.x + dx * along - dy * lateral,
                y: lane.start.y + dy * along + dx * lateral,
            }
        }
        LaneKind::Path => plan.tables[lane.table as usize].point_at(e),
        LaneKind::Arc => sample_shape(
            &RouteShape::Arc {
                bulge: lane.param[0],
            },
            lane.start,
            lane.end,
            e,
        ),
    }
}

/// Evaluates a compiled plan without allocating when `out` was warmed once.
pub fn eval(plan: &TransitionPlan, local_count: f32, out: &mut Vec<Point>) {
    out.clear();
    out.reserve(plan.lanes.len().saturating_sub(out.capacity()));
    let count = local_count.clamp(0.0, plan.counts.total() as f32);
    out.extend(plan.lanes.iter().map(|lane| eval_lane(plan, lane, count)));
}

/// Smallest unsigned change of travel direction at an arrival, in radians.
/// `None` means one of the adjacent moves has no measurable direction.
pub fn arrival_turn_angle(before: Point, arrival: Point, after: Point) -> Option<f32> {
    let ax = arrival.x - before.x;
    let ay = arrival.y - before.y;
    let bx = after.x - arrival.x;
    let by = after.y - arrival.y;
    let al = (ax * ax + ay * ay).sqrt();
    let bl = (bx * bx + by * by).sqrt();
    if al <= f32::EPSILON || bl <= f32::EPSILON {
        return None;
    }
    Some(((ax * bx + ay * by) / (al * bl)).clamp(-1.0, 1.0).acos())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f32, y: f32) -> Point {
        Point { x, y }
    }

    #[test]
    fn gate_and_easing_are_endpoint_exact_and_monotone() {
        let gate = Gate {
            depart: 2.0,
            arrive: Some(6.0),
        };
        let route = Route {
            gate,
            easing: Easing::Smooth,
            ..Route::default()
        };
        assert_eq!(
            evaluate(&route, p(0.0, 0.0), p(8.0, 0.0), 2.0, 8),
            p(0.0, 0.0)
        );
        assert_eq!(
            evaluate(&route, p(0.0, 0.0), p(8.0, 0.0), 6.0, 8),
            p(8.0, 0.0)
        );
        let xs = (0..=16)
            .map(|i| evaluate(&route, p(0.0, 0.0), p(8.0, 0.0), 2.0 + i as f32 / 4.0, 8).x)
            .collect::<Vec<_>>();
        assert!(xs.windows(2).all(|v| v[0] <= v[1]));
    }

    #[test]
    fn override_selects_by_stable_performer_id() {
        let id = PerformerId::new(7).unwrap();
        let other = PerformerId::new(8).unwrap();
        let mut table = RouteTable::default();
        table.overrides.insert(
            id,
            Route {
                shape: RouteShape::Curve {
                    control: ChordPoint {
                        along: 0.5,
                        lateral: 0.5,
                    },
                },
                ..Route::default()
            },
        );
        assert!(matches!(
            table.route_for(id).shape,
            RouteShape::Curve { .. }
        ));
        assert!(matches!(table.route_for(other).shape, RouteShape::Straight));
    }

    #[test]
    fn polyline_uses_distance_not_segment_index() {
        let route = Route {
            shape: RouteShape::Path {
                via: PathVia::Absolute(vec![p(9.0, 0.0)]),
            },
            ..Route::default()
        };
        let midpoint = evaluate(&route, p(0.0, 0.0), p(10.0, 0.0), 4.0, 8);
        assert!((midpoint.x - 5.0).abs() < 1e-4);
    }

    #[test]
    fn route_table_rejects_unknown_ids_and_clamps_gates() {
        let doc = crate::Document::demo(1, 1);
        let unknown = PerformerId::new(99).unwrap();
        let mut table = RouteTable::default();
        table.overrides.insert(unknown, Route::default());
        assert_eq!(
            table.validate(&doc.performers, SetCounts { moves: 8, hold: 0 }),
            Err(DrillError::InvalidTransition)
        );
        table.overrides.clear();
        table.default.gate = Gate {
            depart: 6.0,
            arrive: Some(12.0),
        };
        let clamped = table.clamped_to(SetCounts { moves: 8, hold: 0 }).unwrap();
        assert_eq!(
            clamped.default.gate,
            Gate {
                depart: 6.0,
                arrive: Some(8.0)
            }
        );
    }

    #[test]
    fn arrival_metrics_are_pure_and_finite() {
        let route = Route {
            shape: RouteShape::Arc { bulge: 0.5 },
            gate: Gate {
                depart: 1.0,
                arrive: Some(7.0),
            },
            ..Route::default()
        };
        assert_eq!(arrival_count(&route, 8), 7.0);
        assert!(route_length(&route, p(0.0, 0.0), p(10.0, 0.0)).is_finite());
        assert!(
            (arrival_turn_angle(p(0.0, 0.0), p(1.0, 0.0), p(1.0, 1.0)).unwrap()
                - std::f32::consts::FRAC_PI_2)
                .abs()
                < 1e-5
        );
    }

    #[test]
    fn persisted_routes_and_hold_round_trip_and_legacy_defaults() {
        let mut doc = crate::Document::demo(1, 1);
        doc.sets[0].hold = 4;
        doc.sets[0].routes.default = Route {
            gate: Gate {
                depart: 1.0,
                arrive: Some(7.0),
            },
            easing: Easing::EaseOut,
            ..Route::default()
        };
        let json = doc.to_json().unwrap();
        let loaded = crate::Document::from_json(&json).unwrap();
        assert_eq!(loaded.sets[0].hold, 4);
        assert_eq!(loaded.sets[0].routes, doc.sets[0].routes);
        let mut value = serde_json::to_value(&doc).unwrap();
        for set in value["sets"].as_array_mut().unwrap() {
            set.as_object_mut().unwrap().remove("hold");
            set.as_object_mut().unwrap().remove("routes");
        }
        let legacy = crate::Document::from_json(&serde_json::to_string(&value).unwrap()).unwrap();
        assert_eq!(legacy.sets[0].hold, 0);
        assert!(legacy.sets[0].routes.is_trivial());
    }

    #[test]
    fn count_and_route_edits_are_exactly_reversible() {
        let mut doc = crate::Document::demo(1, 1);
        let id = doc.sets[0].id;
        let counts = SetCounts { moves: 12, hold: 4 };
        let inverse = crate::Edit::SetCounts { set_id: id, counts }
            .apply(&mut doc)
            .unwrap();
        assert_eq!((doc.sets[0].counts, doc.sets[0].hold), (12, 4));
        inverse.apply(&mut doc).unwrap();
        assert_eq!((doc.sets[0].counts, doc.sets[0].hold), (16, 0));
        let routes = RouteTable {
            default: Route {
                easing: Easing::Smooth,
                ..Route::default()
            },
            ..RouteTable::default()
        };
        let inverse = crate::Edit::SetRoutes {
            set_id: id,
            routes: routes.clone(),
        }
        .apply(&mut doc)
        .unwrap();
        assert_eq!(doc.sets[0].routes, routes);
        inverse.apply(&mut doc).unwrap();
        assert!(doc.sets[0].routes.is_trivial());
    }

    #[test]
    fn count_evaluation_honors_gate_and_ensemble_hold() {
        let mut doc = crate::Document::demo(1, 1);
        doc.sets[0].counts = 8;
        doc.sets[0].hold = 4;
        doc.sets[0].routes.default.gate = Gate {
            depart: 2.0,
            arrive: Some(6.0),
        };
        let start = doc.sets[0].positions[0];
        let end = doc.sets[1].positions[0];
        let mut out = Vec::new();
        doc.positions_at_count(0, 1.0, &mut out);
        assert_eq!(out[0], start);
        doc.positions_at_count(0, 7.0, &mut out);
        assert_eq!(out[0], end);
        doc.positions_at_count(0, 10.0, &mut out);
        assert_eq!(out[0], end);
        assert_eq!(doc.timeline_counts(), 12);
    }

    #[test]
    fn arc_table_is_monotone_and_endpoint_exact() {
        let table = ArcTable::build(|t| (t, 4.0 * t * (1.0 - t)));
        assert_eq!(table.t_at(-1.0), 0.0);
        assert_eq!(table.t_at(0.0), 0.0);
        assert_eq!(table.t_at(1.0), 1.0);
        assert_eq!(table.t_at(2.0), 1.0);
        let values = (0..=100)
            .map(|i| table.t_at(i as f32 / 100.0))
            .collect::<Vec<_>>();
        assert!(values.windows(2).all(|pair| pair[0] <= pair[1]));
    }

    #[test]
    fn compiled_plan_matches_reference_for_every_shape_and_gate() {
        for shape in [
            RouteShape::Straight,
            RouteShape::Curve {
                control: ChordPoint {
                    along: 0.35,
                    lateral: 0.6,
                },
            },
            RouteShape::Path {
                via: PathVia::Relative(vec![
                    ChordPoint {
                        along: 0.2,
                        lateral: 0.3,
                    },
                    ChordPoint {
                        along: 0.8,
                        lateral: -0.2,
                    },
                ]),
            },
            RouteShape::Arc { bulge: 0.4 },
        ] {
            let mut doc = crate::Document::demo(1, 1);
            doc.sets[0].routes.default = Route {
                shape,
                gate: Gate {
                    depart: 2.0,
                    arrive: Some(13.0),
                },
                easing: Easing::Smooth,
            };
            let mut plan = TransitionPlan::default();
            doc.plan_transition(doc.sets[0].id, &mut plan);
            let mut reference = Vec::new();
            let mut compiled = Vec::new();
            for half_count in 0..=32 {
                let count = half_count as f32 * 0.5;
                doc.positions_at_count(0, count, &mut reference);
                eval(&plan, count, &mut compiled);
                assert_eq!(reference.len(), compiled.len());
                for (expected, actual) in reference.iter().zip(&compiled) {
                    assert!(
                        distance(*expected, *actual) <= 1.0e-5,
                        "mismatch at count {count}: {expected:?} != {actual:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn curve_moves_at_nearly_uniform_arc_distance() {
        let route = Route {
            shape: RouteShape::Curve {
                control: ChordPoint {
                    along: 0.5,
                    lateral: 0.75,
                },
            },
            ..Route::default()
        };
        let points = (0..=32)
            .map(|i| evaluate(&route, p(0.0, 0.0), p(20.0, 0.0), i as f32 / 2.0, 16))
            .collect::<Vec<_>>();
        let lengths = points
            .windows(2)
            .map(|w| distance(w[0], w[1]))
            .collect::<Vec<_>>();
        let average = lengths.iter().sum::<f32>() / lengths.len() as f32;
        assert!(
            lengths
                .iter()
                .all(|length| (length - average).abs() <= average * 0.015)
        );
    }

    #[test]
    fn unchanged_plan_compile_reuses_the_exact_storage() {
        let mut doc = crate::Document::demo(2, 2);
        doc.sets[0].routes.default.shape = RouteShape::Curve {
            control: ChordPoint {
                along: 0.5,
                lateral: 0.4,
            },
        };
        let mut plan = TransitionPlan::default();
        doc.plan_transition(doc.sets[0].id, &mut plan);
        let lanes = plan.lanes.as_ptr();
        let tables = plan.tables.as_ptr();
        doc.plan_transition(doc.sets[0].id, &mut plan);
        assert_eq!(plan.lanes.as_ptr(), lanes);
        assert_eq!(plan.tables.as_ptr(), tables);
    }
}
