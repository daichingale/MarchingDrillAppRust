//! Allocation-reusing swept collision and stride clinic.

use crate::{Document, PerformerId, Point, Unit};

const NONE: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StepStyle {
    EightToFive,
    SixToFive,
    Metric { meters: f32 },
    Custom { units_per_step: f32 },
}

impl StepStyle {
    pub fn step_units(self, unit: Unit) -> f32 {
        match self {
            Self::EightToFive => 5.0 / 8.0,
            Self::SixToFive => 5.0 / 6.0,
            Self::Metric { meters } => match unit {
                Unit::Meters => meters,
                Unit::Yards => meters * 1.093_613_3,
            },
            Self::Custom { units_per_step } => units_per_step,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StrideRating {
    Comfortable,
    Aggressive,
    Impossible,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClinicParams {
    pub style: StepStyle,
    pub collision_radius: f32,
    pub danger_radius: f32,
    pub crowded_radius: f32,
    pub aggressive_above: f32,
    pub impossible_above: f32,
    pub samples_per_count: u8,
    pub report_at_least: Severity,
    pub max_events: usize,
}

impl Default for ClinicParams {
    fn default() -> Self {
        Self {
            style: StepStyle::EightToFive,
            collision_radius: 0.5,
            danger_radius: 0.75,
            crowded_radius: 1.25,
            aggressive_above: 1.0,
            impossible_above: 1.5,
            samples_per_count: 2,
            report_at_least: Severity::Danger,
            max_events: 16_384,
        }
    }
}

impl ClinicParams {
    fn sanitized(self) -> Self {
        let contact = positive(self.collision_radius, 0.5);
        let danger = positive(self.danger_radius, contact).max(contact);
        let crowded = positive(self.crowded_radius, danger).max(danger);
        Self {
            style: self.style,
            collision_radius: contact,
            danger_radius: danger,
            crowded_radius: crowded,
            aggressive_above: positive(self.aggressive_above, 1.0),
            impossible_above: positive(self.impossible_above, 1.5)
                .max(positive(self.aggressive_above, 1.0)),
            samples_per_count: self.samples_per_count.max(1),
            report_at_least: self.report_at_least,
            max_events: self.max_events.clamp(1, 1_000_000),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Crowded,
    Danger,
    Contact,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CollisionEvent {
    pub a: PerformerId,
    pub b: PerformerId,
    pub count: f32,
    pub distance: f32,
    pub severity: Severity,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrideEvent {
    pub performer: PerformerId,
    pub units_per_count: f32,
    pub rating: StrideRating,
}

#[derive(Debug, Default)]
pub struct ScanScratch {
    prev: Vec<Point>,
    curr: Vec<Point>,
    mid: Vec<Point>,
    next: Vec<u32>,
    heads: Vec<u32>,
    stamps: Vec<u32>,
    occupied: Vec<u32>,
    generation: u32,
    events: Vec<CollisionEvent>,
    strides: Vec<StrideEvent>,
}

impl ScanScratch {
    pub fn reserve(&mut self, performers: usize, cells: usize, max_events: usize) {
        reserve_exact(&mut self.prev, performers);
        reserve_exact(&mut self.curr, performers);
        reserve_exact(&mut self.mid, performers);
        reserve_exact(&mut self.next, performers);
        reserve_exact(&mut self.heads, cells);
        reserve_exact(&mut self.stamps, cells);
        reserve_exact(&mut self.occupied, performers);
        reserve_exact(&mut self.events, max_events);
        reserve_exact(&mut self.strides, performers);
    }

    pub fn capacities(&self) -> [usize; 9] {
        [
            self.prev.capacity(),
            self.curr.capacity(),
            self.mid.capacity(),
            self.next.capacity(),
            self.heads.capacity(),
            self.stamps.capacity(),
            self.occupied.capacity(),
            self.events.capacity(),
            self.strides.capacity(),
        ]
    }

    pub fn collision_events(&self) -> &[CollisionEvent] {
        &self.events
    }

    pub fn stride_events(&self) -> &[StrideEvent] {
        &self.strides
    }
}

pub struct ClinicReport<'a> {
    pub collisions: &'a [CollisionEvent],
    pub strides: &'a [StrideEvent],
    pub suppressed: usize,
}

pub fn scan_transition<'a>(
    document: &Document,
    set_index: usize,
    params: ClinicParams,
    scratch: &'a mut ScanScratch,
) -> ClinicReport<'a> {
    let params = params.sanitized();
    let Some(from) = document.sets.get(set_index) else {
        scratch.events.clear();
        scratch.strides.clear();
        return ClinicReport {
            collisions: &scratch.events,
            strides: &scratch.strides,
            suppressed: 0,
        };
    };
    let Some(to) = document.sets.get(set_index + 1) else {
        scratch.events.clear();
        scratch.strides.clear();
        return ClinicReport {
            collisions: &scratch.events,
            strides: &scratch.strides,
            suppressed: 0,
        };
    };
    let performers = document
        .performers
        .len()
        .min(from.positions.len())
        .min(to.positions.len());
    // Positions between two sets are linear, so closest approach over the complete
    // transition is exact. Subdividing by count repeated the same swept test up to
    // hundreds of times without improving accuracy.
    let total_samples = 1;
    let max_segment_motion = (0..performers)
        .map(|index| distance(from.positions[index], to.positions[index]) / total_samples as f32)
        .fold(0.0, f32::max);
    let cell_width = (params.crowded_radius + max_segment_motion).max(0.01);
    let cols = ((document.grid.width / cell_width).ceil() as usize).max(1) + 2;
    let rows = ((document.grid.height / cell_width).ceil() as usize).max(1) + 2;
    let cells = cols.saturating_mul(rows).min(4_000_000);
    scratch.reserve(performers, cells, params.max_events);
    scratch.events.clear();
    scratch.strides.clear();

    let counts = f32::from(from.counts.max(1));
    let step = params
        .style
        .step_units(document.grid.unit)
        .max(f32::EPSILON);
    for index in 0..performers {
        let distance = distance(from.positions[index], to.positions[index]);
        let units_per_count = distance / counts;
        let ratio = units_per_count / step;
        let rating = if ratio > params.impossible_above {
            StrideRating::Impossible
        } else if ratio > params.aggressive_above {
            StrideRating::Aggressive
        } else {
            StrideRating::Comfortable
        };
        scratch.strides.push(StrideEvent {
            performer: document.performers[index].id,
            units_per_count,
            rating,
        });
    }

    let mut suppressed = 0;
    for sample in 0..total_samples {
        let t0 = sample as f32 / total_samples as f32;
        let t1 = (sample + 1) as f32 / total_samples as f32;
        scratch.prev.clear();
        scratch.curr.clear();
        scratch.mid.clear();
        for index in 0..performers {
            let p0 = from.positions[index].lerp(to.positions[index], t0);
            let p1 = from.positions[index].lerp(to.positions[index], t1);
            scratch.prev.push(p0);
            scratch.curr.push(p1);
            scratch.mid.push(p0.lerp(p1, 0.5));
        }
        begin_grid(scratch, cells, performers);
        for index in 0..performers {
            let (cx, cy) = cell_of(scratch.mid[index], cell_width, cols, rows);
            insert_grid(scratch, index, cx, cy, cols);
        }
        // Visit only occupied cells. Each unordered cell pair is considered once:
        // the cell itself plus the four forward neighbours cover the 3x3 stencil.
        for occupied_index in 0..scratch.occupied.len() {
            let cell = scratch.occupied[occupied_index] as usize;
            let cx = cell % cols;
            let cy = cell / cols;
            let neighbours = [
                (cx, cy),
                (cx + 1, cy),
                (cx.saturating_sub(1), cy + 1),
                (cx, cy + 1),
                (cx + 1, cy + 1),
            ];
            let mut first = scratch.heads[cell];
            while first != NONE {
                let i = first as usize;
                for (neighbour_index, &(nx, ny)) in neighbours.iter().enumerate() {
                    if nx >= cols || ny >= rows {
                        continue;
                    }
                    let neighbour = ny * cols + nx;
                    if scratch.stamps[neighbour] != scratch.generation {
                        continue;
                    }
                    let mut second = if neighbour_index == 0 {
                        scratch.next[i]
                    } else {
                        scratch.heads[neighbour]
                    };
                    while second != NONE {
                        record_pair(
                            document,
                            params,
                            scratch,
                            counts,
                            i,
                            second as usize,
                            &mut suppressed,
                        );
                        second = scratch.next[second as usize];
                    }
                }
                first = scratch.next[i];
            }
        }
    }
    ClinicReport {
        collisions: &scratch.events,
        strides: &scratch.strides,
        suppressed,
    }
}

fn begin_grid(scratch: &mut ScanScratch, cells: usize, performers: usize) {
    scratch.generation = scratch.generation.wrapping_add(1);
    if scratch.generation == 0 {
        scratch.stamps[..cells].fill(0);
        scratch.generation = 1;
    }
    scratch.heads.resize(cells, NONE);
    scratch.stamps.resize(cells, 0);
    scratch.next.resize(performers, NONE);
    scratch.occupied.clear();
}

fn insert_grid(scratch: &mut ScanScratch, index: usize, x: usize, y: usize, cols: usize) {
    let cell = y * cols + x;
    if scratch.stamps[cell] != scratch.generation {
        scratch.stamps[cell] = scratch.generation;
        scratch.heads[cell] = NONE;
        scratch.occupied.push(cell as u32);
    }
    scratch.next[index] = scratch.heads[cell];
    scratch.heads[cell] = index as u32;
}

fn record_pair(
    document: &Document,
    params: ClinicParams,
    scratch: &mut ScanScratch,
    counts: f32,
    i: usize,
    j: usize,
    suppressed: &mut usize,
) {
    let (fraction, closest_squared) = closest_approach_squared(
        scratch.prev[i],
        scratch.curr[i],
        scratch.prev[j],
        scratch.curr[j],
    );
    if closest_squared >= params.crowded_radius * params.crowded_radius {
        return;
    }
    let severity = if closest_squared < params.collision_radius * params.collision_radius {
        Severity::Contact
    } else if closest_squared < params.danger_radius * params.danger_radius {
        Severity::Danger
    } else {
        Severity::Crowded
    };
    if severity < params.report_at_least {
        return;
    }
    let closest = closest_squared.sqrt();
    let a = document.performers[i].id.min(document.performers[j].id);
    let b = document.performers[i].id.max(document.performers[j].id);
    let count = fraction * counts;
    if let Some(existing) = scratch
        .events
        .iter_mut()
        .find(|event| event.a == a && event.b == b)
    {
        if closest < existing.distance {
            existing.distance = closest;
            existing.count = count;
            existing.severity = severity;
        }
    } else if scratch.events.len() < params.max_events {
        scratch.events.push(CollisionEvent {
            a,
            b,
            count,
            distance: closest,
            severity,
        });
    } else {
        *suppressed += 1;
    }
}

fn cell_of(point: Point, width: f32, cols: usize, rows: usize) -> (usize, usize) {
    let x = ((point.x / width).floor() as isize + 1).clamp(0, cols as isize - 1) as usize;
    let y = ((point.y / width).floor() as isize + 1).clamp(0, rows as isize - 1) as usize;
    (x, y)
}

#[cfg(test)]
fn closest_approach(a0: Point, a1: Point, b0: Point, b1: Point) -> (f32, f32) {
    let (fraction, squared) = closest_approach_squared(a0, a1, b0, b1);
    (fraction, squared.sqrt())
}

fn closest_approach_squared(a0: Point, a1: Point, b0: Point, b1: Point) -> (f32, f32) {
    let rx = a0.x - b0.x;
    let ry = a0.y - b0.y;
    let vx = (a1.x - a0.x) - (b1.x - b0.x);
    let vy = (a1.y - a0.y) - (b1.y - b0.y);
    let vv = vx * vx + vy * vy;
    let t = if vv > f32::EPSILON {
        (-(rx * vx + ry * vy) / vv).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let dx = rx + vx * t;
    let dy = ry + vy * t;
    (t, dx * dx + dy * dy)
}

fn distance(a: Point, b: Point) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

fn positive(value: f32, fallback: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        fallback
    }
}

fn reserve_exact<T>(values: &mut Vec<T>, capacity: usize) {
    if values.capacity() < capacity {
        values.reserve_exact(capacity - values.capacity());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swept_scan_catches_crossing_missed_at_endpoints() {
        let mut document = Document::demo(1, 2);
        document.sets[0].counts = 16;
        document.sets[0].positions = vec![Point { x: 10.0, y: 10.0 }, Point { x: 20.0, y: 10.0 }];
        document.sets[1].positions = vec![Point { x: 20.0, y: 10.0 }, Point { x: 10.0, y: 10.0 }];
        let mut scratch = ScanScratch::default();
        let report = scan_transition(&document, 0, ClinicParams::default(), &mut scratch);
        assert_eq!(report.collisions.len(), 1);
        let event = report.collisions[0];
        assert!(event.distance < 1e-5);
        assert!((event.count - 8.0).abs() <= 0.5);
        assert!(event.a < event.b);
    }

    #[test]
    fn warm_scan_reuses_all_scratch_capacities() {
        let document = Document::demo(10, 10);
        let mut scratch = ScanScratch::default();
        scan_transition(&document, 0, ClinicParams::default(), &mut scratch);
        let capacities = scratch.capacities();
        for _ in 0..20 {
            scan_transition(&document, 0, ClinicParams::default(), &mut scratch);
            assert_eq!(scratch.capacities(), capacities);
        }
    }

    #[test]
    fn spatial_hash_matches_bruteforce_swept_pairs() {
        let mut document = Document::demo(2, 5);
        document.sets[0].counts = 4;
        for (index, point) in document.sets[0].positions.iter_mut().enumerate() {
            *point = Point {
                x: (index * 7 % 23) as f32,
                y: (index * 11 % 19) as f32,
            };
        }
        for (index, point) in document.sets[1].positions.iter_mut().enumerate() {
            *point = Point {
                x: (index * 13 % 23) as f32,
                y: (index * 5 % 19) as f32,
            };
        }
        let params = ClinicParams {
            crowded_radius: 1.5,
            samples_per_count: 3,
            report_at_least: Severity::Crowded,
            ..ClinicParams::default()
        };
        let mut scratch = ScanScratch::default();
        let report = scan_transition(&document, 0, params, &mut scratch);
        let samples = usize::from(document.sets[0].counts) * usize::from(params.samples_per_count);
        for i in 0..document.performers.len() {
            for j in i + 1..document.performers.len() {
                let mut minimum = f32::INFINITY;
                for sample in 0..samples {
                    let t0 = sample as f32 / samples as f32;
                    let t1 = (sample + 1) as f32 / samples as f32;
                    let (_, distance) = closest_approach(
                        document.sets[0].positions[i].lerp(document.sets[1].positions[i], t0),
                        document.sets[0].positions[i].lerp(document.sets[1].positions[i], t1),
                        document.sets[0].positions[j].lerp(document.sets[1].positions[j], t0),
                        document.sets[0].positions[j].lerp(document.sets[1].positions[j], t1),
                    );
                    minimum = minimum.min(distance);
                }
                if minimum < params.crowded_radius {
                    let a = document.performers[i].id.min(document.performers[j].id);
                    let b = document.performers[i].id.max(document.performers[j].id);
                    assert!(
                        report
                            .collisions
                            .iter()
                            .any(|event| event.a == a && event.b == b)
                    );
                }
            }
        }
    }

    #[test]
    fn stride_rating_uses_selected_step_style() {
        let mut document = Document::demo(1, 1);
        document.sets[0].counts = 1;
        document.sets[0].positions[0] = Point { x: 0.0, y: 0.0 };
        document.sets[1].positions[0] = Point { x: 2.0, y: 0.0 };
        let mut scratch = ScanScratch::default();
        let report = scan_transition(&document, 0, ClinicParams::default(), &mut scratch);
        assert_eq!(report.strides[0].rating, StrideRating::Impossible);
    }
}
