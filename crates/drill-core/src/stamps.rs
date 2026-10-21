//! Formation stamps for a student-facing editor.
//!
//! A stamp is a box on the field: line, circle, block, V, arc, or lettering.
//! Selected performers are paired onto that shape so the spacing along it is
//! even and nobody walks farther than they have to.

use crate::pathing::optimal_assignment;
use crate::shapes::circle;
use crate::{Point, evenly_spaced_arc, evenly_spaced_line};

/// The six stamps on the icon row, in toolbar order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StampKind {
    Line,
    Circle,
    Block,
    Vee,
    Arc,
    Letter,
}

impl StampKind {
    pub const ALL: [Self; 6] = [
        Self::Line,
        Self::Circle,
        Self::Block,
        Self::Vee,
        Self::Arc,
        Self::Letter,
    ];
}

/// Which part of a placed stamp a pointer is holding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StampGrab {
    Body,
    Resize,
    Rotate,
}

/// A placed stamp. `width` and `height` are the full box, in field units.
/// `rotation` is radians, counterclockwise from +x toward +y.
#[derive(Clone, Debug, PartialEq)]
pub struct Stamp {
    pub kind: StampKind,
    pub center: Point,
    pub width: f32,
    pub height: f32,
    pub rotation: f32,
    pub text: String,
}

impl Stamp {
    /// Box dragged from `a` to `b`. A line uses the drag as its length and
    /// direction. A circle uses the drag as its diameter. The others use the
    /// axis-aligned box and start unrotated.
    #[must_use]
    pub fn from_drag(kind: StampKind, a: Point, b: Point, text: &str) -> Self {
        let text = sanitize_text(text);
        if !a.x.is_finite() || !a.y.is_finite() || !b.x.is_finite() || !b.y.is_finite() {
            return Self {
                kind,
                center: Point { x: 0.0, y: 0.0 },
                width: 8.0,
                height: 8.0,
                rotation: 0.0,
                text,
            };
        }
        let center = Point {
            x: (a.x + b.x) * 0.5,
            y: (a.y + b.y) * 0.5,
        };
        match kind {
            StampKind::Line => {
                let dx = b.x - a.x;
                let dy = b.y - a.y;
                Self {
                    kind,
                    center,
                    width: (dx * dx + dy * dy).sqrt().max(4.0),
                    height: 4.0,
                    rotation: dy.atan2(dx),
                    text,
                }
            }
            StampKind::Circle => {
                let dx = b.x - a.x;
                let dy = b.y - a.y;
                let diameter = (dx * dx + dy * dy).sqrt().max(4.0);
                Self {
                    kind,
                    center,
                    width: diameter,
                    height: diameter,
                    rotation: 0.0,
                    text,
                }
            }
            StampKind::Block | StampKind::Vee | StampKind::Arc | StampKind::Letter => Self {
                kind,
                center,
                width: (b.x - a.x).abs().max(4.0),
                height: (b.y - a.y).abs().max(4.0),
                rotation: 0.0,
                text,
            },
        }
    }

    /// Exactly `count` field points, evenly spaced along the shape.
    #[must_use]
    pub fn sample(&self, count: usize) -> Vec<Point> {
        if count == 0 {
            return Vec::new();
        }
        let width = finite_span(self.width);
        let height = finite_span(self.height);
        let local = match self.kind {
            StampKind::Line => evenly_spaced_line(
                Point {
                    x: -width * 0.5,
                    y: 0.0,
                },
                Point {
                    x: width * 0.5,
                    y: 0.0,
                },
                count,
            ),
            StampKind::Circle => circle(Point { x: 0.0, y: 0.0 }, width.min(height) * 0.5, count),
            StampKind::Block => block_local(width, height, count),
            StampKind::Vee => vee_local(width, height, count),
            StampKind::Arc => arc_local(width, height, count),
            StampKind::Letter => letter_local(width, height, &self.text, count),
        };
        let rotation = if self.rotation.is_finite() {
            self.rotation
        } else {
            0.0
        };
        let center = if self.center.x.is_finite() && self.center.y.is_finite() {
            self.center
        } else {
            Point { x: 0.0, y: 0.0 }
        };
        local
            .into_iter()
            .map(|point| to_world(center, rotation, point.x, point.y))
            .collect()
    }

    #[must_use]
    pub fn resize_handle(&self) -> Point {
        let (width, height, rotation, center) = self.frame();
        to_world(center, rotation, width * 0.5, -height * 0.5)
    }

    /// `gap` is the field-unit offset past the upfield edge, so the handle
    /// stays a fingertip away from the dots.
    #[must_use]
    pub fn rotate_handle(&self, gap: f32) -> Point {
        let (width, height, rotation, center) = self.frame();
        let gap = if gap.is_finite() { gap.max(0.0) } else { 0.0 };
        let _ = width;
        to_world(center, rotation, 0.0, height * 0.5 + gap)
    }

    #[must_use]
    pub fn hit(&self, point: Point, tolerance: f32, rotate_gap: f32) -> Option<StampGrab> {
        if !point.x.is_finite() || !point.y.is_finite() {
            return None;
        }
        let tolerance = if tolerance.is_finite() {
            tolerance.max(0.0)
        } else {
            0.0
        };
        if distance(point, self.rotate_handle(rotate_gap)) <= tolerance {
            return Some(StampGrab::Rotate);
        }
        if distance(point, self.resize_handle()) <= tolerance {
            return Some(StampGrab::Resize);
        }
        let (width, height, rotation, center) = self.frame();
        let (lx, ly) = to_local(center, rotation, point);
        if lx.abs() <= width * 0.5 + tolerance && ly.abs() <= height * 0.5 + tolerance {
            Some(StampGrab::Body)
        } else {
            None
        }
    }

    /// Corner sets the half-width and half-height. The center stays put.
    #[must_use]
    pub fn with_resize(&self, at: Point) -> Self {
        let mut next = self.clone();
        if !at.x.is_finite() || !at.y.is_finite() {
            return next;
        }
        let (_, _, rotation, center) = self.frame();
        let (lx, ly) = to_local(center, rotation, at);
        next.width = (lx.abs() * 2.0).clamp(4.0, 160.0);
        next.height = (ly.abs() * 2.0).clamp(4.0, 160.0);
        next
    }

    /// Points the upfield axis at `at`.
    #[must_use]
    pub fn with_rotate(&self, at: Point) -> Self {
        let mut next = self.clone();
        if !at.x.is_finite() || !at.y.is_finite() {
            return next;
        }
        let center = self.frame().3;
        let dx = at.x - center.x;
        let dy = at.y - center.y;
        if dx * dx + dy * dy < 1e-6 {
            return next;
        }
        next.rotation = dy.atan2(dx) - std::f32::consts::FRAC_PI_2;
        next
    }

    #[must_use]
    pub fn translate(&self, delta: Point) -> Self {
        let mut next = self.clone();
        if delta.x.is_finite() && delta.y.is_finite() {
            next.center.x += delta.x;
            next.center.y += delta.y;
        }
        next
    }

    /// World-space corners of the box, starting at local (-w/2, -h/2) and
    /// walking counterclockwise.
    #[must_use]
    pub fn corners(&self) -> [Point; 4] {
        let (width, height, rotation, center) = self.frame();
        [
            to_world(center, rotation, -width * 0.5, -height * 0.5),
            to_world(center, rotation, width * 0.5, -height * 0.5),
            to_world(center, rotation, width * 0.5, height * 0.5),
            to_world(center, rotation, -width * 0.5, height * 0.5),
        ]
    }

    fn frame(&self) -> (f32, f32, f32, Point) {
        let width = finite_span(self.width);
        let height = finite_span(self.height);
        let rotation = if self.rotation.is_finite() {
            self.rotation
        } else {
            0.0
        };
        let center = if self.center.x.is_finite() && self.center.y.is_finite() {
            self.center
        } else {
            Point { x: 0.0, y: 0.0 }
        };
        (width, height, rotation, center)
    }
}

/// Move `selected` indexes from `basis` onto the stamp. Unselected indexes
/// stay where they were. Pairing minimizes total travel.
#[must_use]
pub fn placed_positions(basis: &[Point], selected: &[usize], stamp: &Stamp) -> Vec<Point> {
    let mut out = basis.to_vec();
    let selected: Vec<usize> = selected
        .iter()
        .copied()
        .filter(|&index| index < basis.len())
        .collect();
    if selected.is_empty() {
        return out;
    }
    let targets = stamp.sample(selected.len());
    let from: Vec<Point> = selected.iter().map(|&index| basis[index]).collect();
    let assignment = optimal_assignment(&from, &targets);
    for (rank, index) in selected.into_iter().enumerate() {
        let Some(&slot) = assignment.get(rank) else {
            continue;
        };
        let Some(&point) = targets.get(slot) else {
            continue;
        };
        if point.x.is_finite() && point.y.is_finite() {
            out[index] = point;
        }
    }
    out
}

fn finite_span(value: f32) -> f32 {
    if value.is_finite() {
        value.abs().clamp(4.0, 160.0)
    } else {
        8.0
    }
}

fn to_world(center: Point, rotation: f32, x: f32, y: f32) -> Point {
    let (sin, cos) = rotation.sin_cos();
    Point {
        x: center.x + x * cos - y * sin,
        y: center.y + x * sin + y * cos,
    }
}

fn to_local(center: Point, rotation: f32, point: Point) -> (f32, f32) {
    let dx = point.x - center.x;
    let dy = point.y - center.y;
    let (sin, cos) = rotation.sin_cos();
    (dx * cos + dy * sin, -dx * sin + dy * cos)
}

fn distance(a: Point, b: Point) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

fn block_local(width: f32, height: f32, count: usize) -> Vec<Point> {
    if count == 0 {
        return Vec::new();
    }
    if count == 1 {
        return vec![Point { x: 0.0, y: 0.0 }];
    }
    let aspect = if height < 0.01 {
        1.0
    } else {
        (width / height).clamp(0.2, 8.0)
    };
    let cols = ((count as f32) * aspect)
        .sqrt()
        .round()
        .clamp(1.0, count as f32) as usize;
    let rows = count.div_ceil(cols);
    let mut out = Vec::with_capacity(count);
    let mut placed = 0;
    for row in 0..rows {
        let n = (count - placed).min(cols);
        let y = if rows == 1 {
            0.0
        } else {
            -height * 0.5 + height * row as f32 / (rows - 1) as f32
        };
        let spacing = if cols == 1 {
            0.0
        } else {
            width / (cols - 1) as f32
        };
        let row_span = spacing * n.saturating_sub(1) as f32;
        let x0 = -row_span * 0.5;
        for col in 0..n {
            out.push(Point {
                x: x0 + spacing * col as f32,
                y,
            });
            placed += 1;
        }
    }
    out
}

fn vee_local(width: f32, height: f32, count: usize) -> Vec<Point> {
    sample_polyline(
        &[
            Point {
                x: -width * 0.5,
                y: height * 0.5,
            },
            Point {
                x: 0.0,
                y: -height * 0.5,
            },
            Point {
                x: width * 0.5,
                y: height * 0.5,
            },
        ],
        count,
    )
}

fn arc_local(width: f32, height: f32, count: usize) -> Vec<Point> {
    let half = width * 0.5;
    let sagitta = height * 0.5;
    let radius = (half * half + sagitta * sagitta) / (2.0 * sagitta);
    let center_y = sagitta - radius;
    let start = (0.0 - center_y).atan2(-half);
    let end = (0.0 - center_y).atan2(half);
    evenly_spaced_arc(
        Point {
            x: 0.0,
            y: center_y,
        },
        radius,
        start,
        end,
        count,
    )
}

fn letter_local(width: f32, height: f32, text: &str, count: usize) -> Vec<Point> {
    let chars: Vec<char> = sanitize_text(text).chars().collect();
    if chars.is_empty() || count == 0 {
        return Vec::new();
    }
    let n = chars.len() as f32;
    let gap = 0.18;
    let units = n + (n - 1.0) * gap;
    let cell = width / units;
    let mut strokes = Vec::new();
    for (index, ch) in chars.iter().copied().enumerate() {
        let x0 = -width * 0.5 + index as f32 * cell * (1.0 + gap);
        for stroke in glyph(ch) {
            let poly = stroke
                .into_iter()
                .map(|(gx, gy)| Point {
                    x: x0 + gx * cell,
                    y: -height * 0.5 + gy * height,
                })
                .collect();
            strokes.push(poly);
        }
    }
    sample_strokes(&strokes, count)
}

fn sanitize_text(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if ch.is_control() || ch.is_whitespace() {
            continue;
        }
        out.push(ch);
        if out.chars().count() >= 8 {
            break;
        }
    }
    if out.is_empty() {
        out.push('A');
    }
    out
}

fn sample_polyline(points: &[Point], count: usize) -> Vec<Point> {
    if count == 0 || points.is_empty() {
        return Vec::new();
    }
    if points.len() == 1 || count == 1 {
        return vec![points[points.len() / 2]; count];
    }
    let mut lengths = Vec::with_capacity(points.len());
    lengths.push(0.0);
    for window in points.windows(2) {
        lengths.push(lengths.last().copied().unwrap_or(0.0) + distance(window[0], window[1]));
    }
    let total = lengths.last().copied().unwrap_or(0.0);
    if total <= f32::EPSILON {
        return vec![points[0]; count];
    }
    (0..count)
        .map(|index| {
            let target = total * index as f32 / (count - 1) as f32;
            let slot = lengths
                .partition_point(|&length| length < target)
                .saturating_sub(1)
                .min(points.len() - 2);
            let span = lengths[slot + 1] - lengths[slot];
            let t = if span <= f32::EPSILON {
                0.0
            } else {
                ((target - lengths[slot]) / span).clamp(0.0, 1.0)
            };
            points[slot].lerp(points[slot + 1], t)
        })
        .collect()
}

fn polyline_len(points: &[Point]) -> f32 {
    points
        .windows(2)
        .map(|window| distance(window[0], window[1]))
        .sum()
}

fn sample_strokes(strokes: &[Vec<Point>], count: usize) -> Vec<Point> {
    if strokes.is_empty() || count == 0 {
        return Vec::new();
    }
    let lengths: Vec<f32> = strokes.iter().map(|stroke| polyline_len(stroke)).collect();
    let total: f32 = lengths.iter().sum();
    if total <= f32::EPSILON {
        return vec![strokes[0].first().copied().unwrap_or_default(); count];
    }
    let mut shares: Vec<usize> = lengths
        .iter()
        .map(|length| ((*length / total) * count as f32).floor() as usize)
        .collect();
    let mut order: Vec<usize> = (0..strokes.len()).collect();
    order.sort_by(|&left, &right| {
        let left_frac = (lengths[left] / total * count as f32).fract();
        let right_frac = (lengths[right] / total * count as f32).fract();
        right_frac.total_cmp(&left_frac).then(left.cmp(&right))
    });
    let deficit = count.saturating_sub(shares.iter().sum());
    for index in order.into_iter().take(deficit) {
        shares[index] += 1;
    }
    let mut out = Vec::with_capacity(count);
    for (stroke, share) in strokes.iter().zip(shares) {
        if share == 0 {
            continue;
        }
        out.extend(sample_polyline(stroke, share));
    }
    if out.len() < count {
        let pad = out.last().copied().unwrap_or_default();
        out.resize(count, pad);
    }
    out.truncate(count);
    out
}

fn glyph(ch: char) -> Vec<Vec<(f32, f32)>> {
    let upper = ch.to_ascii_uppercase();
    match upper {
        'A' => vec![
            vec![(0.08, 0.0), (0.5, 1.0), (0.92, 0.0)],
            vec![(0.24, 0.38), (0.76, 0.38)],
        ],
        'B' => vec![
            vec![(0.12, 0.0), (0.12, 1.0)],
            vec![
                (0.12, 1.0),
                (0.62, 1.0),
                (0.86, 0.82),
                (0.62, 0.52),
                (0.12, 0.52),
            ],
            vec![
                (0.12, 0.52),
                (0.7, 0.52),
                (0.92, 0.28),
                (0.68, 0.0),
                (0.12, 0.0),
            ],
        ],
        'C' => vec![vec![
            (0.86, 0.84),
            (0.55, 1.0),
            (0.18, 0.78),
            (0.1, 0.5),
            (0.18, 0.22),
            (0.55, 0.0),
            (0.86, 0.16),
        ]],
        'D' => vec![
            vec![(0.12, 0.0), (0.12, 1.0)],
            vec![
                (0.12, 1.0),
                (0.55, 1.0),
                (0.92, 0.72),
                (0.92, 0.28),
                (0.55, 0.0),
                (0.12, 0.0),
            ],
        ],
        'E' => vec![
            vec![(0.84, 1.0), (0.14, 1.0), (0.14, 0.0), (0.84, 0.0)],
            vec![(0.14, 0.5), (0.7, 0.5)],
        ],
        'F' => vec![
            vec![(0.14, 0.0), (0.14, 1.0), (0.84, 1.0)],
            vec![(0.14, 0.5), (0.68, 0.5)],
        ],
        'G' => vec![vec![
            (0.78, 0.86),
            (0.48, 1.0),
            (0.16, 0.78),
            (0.1, 0.48),
            (0.2, 0.16),
            (0.55, 0.0),
            (0.88, 0.16),
            (0.88, 0.42),
            (0.52, 0.42),
        ]],
        'H' => vec![
            vec![(0.14, 0.0), (0.14, 1.0)],
            vec![(0.86, 0.0), (0.86, 1.0)],
            vec![(0.14, 0.5), (0.86, 0.5)],
        ],
        'I' => vec![
            vec![(0.28, 0.0), (0.72, 0.0)],
            vec![(0.5, 0.0), (0.5, 1.0)],
            vec![(0.28, 1.0), (0.72, 1.0)],
        ],
        'J' => vec![vec![
            (0.78, 1.0),
            (0.78, 0.28),
            (0.62, 0.05),
            (0.32, 0.05),
            (0.18, 0.28),
        ]],
        'K' => vec![
            vec![(0.16, 0.0), (0.16, 1.0)],
            vec![(0.84, 1.0), (0.16, 0.48)],
            vec![(0.4, 0.64), (0.86, 0.0)],
        ],
        'L' => vec![vec![(0.16, 1.0), (0.16, 0.0), (0.84, 0.0)]],
        'M' => vec![vec![
            (0.06, 0.0),
            (0.06, 1.0),
            (0.5, 0.42),
            (0.94, 1.0),
            (0.94, 0.0),
        ]],
        'N' => vec![vec![(0.12, 0.0), (0.12, 1.0), (0.88, 0.0), (0.88, 1.0)]],
        'O' => vec![vec![
            (0.5, 1.0),
            (0.16, 0.78),
            (0.08, 0.5),
            (0.16, 0.22),
            (0.5, 0.0),
            (0.84, 0.22),
            (0.92, 0.5),
            (0.84, 0.78),
            (0.5, 1.0),
        ]],
        'P' => vec![
            vec![(0.14, 0.0), (0.14, 1.0)],
            vec![
                (0.14, 1.0),
                (0.66, 1.0),
                (0.9, 0.82),
                (0.66, 0.52),
                (0.14, 0.52),
            ],
        ],
        'Q' => vec![
            vec![
                (0.5, 1.0),
                (0.16, 0.78),
                (0.08, 0.5),
                (0.16, 0.22),
                (0.5, 0.02),
                (0.84, 0.22),
                (0.92, 0.5),
                (0.84, 0.78),
                (0.5, 1.0),
            ],
            vec![(0.58, 0.32), (0.92, 0.0)],
        ],
        'R' => vec![
            vec![(0.14, 0.0), (0.14, 1.0)],
            vec![
                (0.14, 1.0),
                (0.66, 1.0),
                (0.9, 0.82),
                (0.66, 0.52),
                (0.14, 0.52),
            ],
            vec![(0.48, 0.52), (0.9, 0.0)],
        ],
        'S' => vec![vec![
            (0.84, 0.86),
            (0.52, 1.0),
            (0.18, 0.84),
            (0.28, 0.62),
            (0.72, 0.4),
            (0.86, 0.18),
            (0.52, 0.0),
            (0.16, 0.16),
        ]],
        'T' => vec![vec![(0.08, 1.0), (0.92, 1.0)], vec![(0.5, 1.0), (0.5, 0.0)]],
        'U' => vec![vec![
            (0.12, 1.0),
            (0.12, 0.32),
            (0.28, 0.05),
            (0.72, 0.05),
            (0.88, 0.32),
            (0.88, 1.0),
        ]],
        'V' => vec![vec![(0.06, 1.0), (0.5, 0.0), (0.94, 1.0)]],
        'W' => vec![vec![
            (0.02, 1.0),
            (0.22, 0.0),
            (0.5, 0.62),
            (0.78, 0.0),
            (0.98, 1.0),
        ]],
        'X' => vec![vec![(0.1, 0.0), (0.9, 1.0)], vec![(0.1, 1.0), (0.9, 0.0)]],
        'Y' => vec![
            vec![(0.08, 1.0), (0.5, 0.48)],
            vec![(0.92, 1.0), (0.5, 0.48)],
            vec![(0.5, 0.48), (0.5, 0.0)],
        ],
        'Z' => vec![vec![(0.1, 1.0), (0.9, 1.0), (0.1, 0.0), (0.9, 0.0)]],
        '0' => vec![vec![
            (0.5, 1.0),
            (0.16, 0.78),
            (0.08, 0.5),
            (0.16, 0.22),
            (0.5, 0.0),
            (0.84, 0.22),
            (0.92, 0.5),
            (0.84, 0.78),
            (0.5, 1.0),
        ]],
        '1' => vec![vec![(0.32, 0.78), (0.5, 1.0), (0.5, 0.0)]],
        '2' => vec![vec![
            (0.14, 0.78),
            (0.32, 1.0),
            (0.7, 1.0),
            (0.9, 0.78),
            (0.7, 0.52),
            (0.12, 0.0),
            (0.9, 0.0),
        ]],
        '3' => vec![vec![
            (0.14, 0.86),
            (0.4, 1.0),
            (0.78, 0.9),
            (0.5, 0.55),
            (0.82, 0.4),
            (0.82, 0.16),
            (0.48, 0.0),
            (0.14, 0.16),
        ]],
        '4' => vec![
            vec![(0.68, 0.0), (0.68, 1.0)],
            vec![(0.68, 1.0), (0.12, 0.32), (0.88, 0.32)],
        ],
        '5' => vec![vec![
            (0.84, 1.0),
            (0.2, 1.0),
            (0.16, 0.55),
            (0.55, 0.62),
            (0.88, 0.4),
            (0.78, 0.08),
            (0.4, 0.0),
            (0.14, 0.16),
        ]],
        '6' => vec![vec![
            (0.78, 0.88),
            (0.48, 1.0),
            (0.18, 0.7),
            (0.14, 0.32),
            (0.36, 0.05),
            (0.7, 0.12),
            (0.82, 0.38),
            (0.48, 0.5),
            (0.18, 0.32),
        ]],
        '7' => vec![vec![(0.1, 1.0), (0.9, 1.0), (0.38, 0.0)]],
        '8' => vec![vec![
            (0.5, 0.52),
            (0.22, 0.78),
            (0.4, 1.0),
            (0.72, 0.92),
            (0.5, 0.52),
            (0.82, 0.28),
            (0.62, 0.0),
            (0.28, 0.08),
            (0.18, 0.32),
            (0.5, 0.52),
        ]],
        '9' => vec![vec![
            (0.22, 0.12),
            (0.52, 0.0),
            (0.82, 0.3),
            (0.86, 0.68),
            (0.64, 0.95),
            (0.3, 0.88),
            (0.18, 0.62),
            (0.52, 0.5),
            (0.82, 0.68),
        ]],
        _ => vec![vec![
            (0.16, 0.08),
            (0.84, 0.08),
            (0.84, 0.92),
            (0.16, 0.92),
            (0.16, 0.08),
        ]],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(kind: StampKind, width: f32, height: f32) -> Stamp {
        Stamp {
            kind,
            center: Point { x: 10.0, y: 20.0 },
            width,
            height,
            rotation: 0.0,
            text: "A".into(),
        }
    }

    fn spacing_ratio(points: &[Point]) -> f32 {
        let distances: Vec<f32> = points
            .windows(2)
            .map(|window| distance(window[0], window[1]))
            .filter(|span| *span > 1e-4)
            .collect();
        let min = distances.iter().copied().fold(f32::MAX, f32::min);
        let max = distances.iter().copied().fold(0.0, f32::max);
        if min <= f32::EPSILON {
            f32::MAX
        } else {
            max / min
        }
    }

    #[test]
    fn line_is_even_and_follows_the_drag() {
        let dragged = Stamp::from_drag(
            StampKind::Line,
            Point { x: 0.0, y: 0.0 },
            Point { x: 20.0, y: 0.0 },
            "",
        );
        let points = dragged.sample(5);
        assert_eq!(points.len(), 5);
        assert!((points[0].x).abs() < 0.05, "{:?}", points[0]);
        assert!((points[4].x - 20.0).abs() < 0.05, "{:?}", points[4]);
        assert!(points.iter().all(|point| point.y.abs() < 0.05));
        assert!(spacing_ratio(&points) < 1.02);
    }

    #[test]
    fn circle_points_share_a_radius_and_a_gap() {
        let points = stamp(StampKind::Circle, 20.0, 20.0).sample(16);
        assert_eq!(points.len(), 16);
        for point in &points {
            let radius = distance(*point, Point { x: 10.0, y: 20.0 });
            assert!((radius - 10.0).abs() < 0.05, "{radius}");
        }
        let mut around = points.clone();
        around.push(points[0]);
        assert!(spacing_ratio(&around) < 1.05, "{}", spacing_ratio(&around));
    }

    #[test]
    fn block_of_four_lands_on_the_corners() {
        let points = stamp(StampKind::Block, 10.0, 10.0).sample(4);
        assert_eq!(points.len(), 4);
        let mut xs: Vec<f32> = points.iter().map(|point| point.x).collect();
        let mut ys: Vec<f32> = points.iter().map(|point| point.y).collect();
        xs.sort_by(f32::total_cmp);
        ys.sort_by(f32::total_cmp);
        assert!((xs[0] - 5.0).abs() < 0.05);
        assert!((xs[3] - 15.0).abs() < 0.05);
        assert!((ys[0] - 15.0).abs() < 0.05);
        assert!((ys[3] - 25.0).abs() < 0.05);
    }

    #[test]
    fn vee_is_symmetric_and_even_along_both_arms() {
        let points = stamp(StampKind::Vee, 10.0, 10.0).sample(5);
        assert_eq!(points.len(), 5);
        assert!((points[2].x - 10.0).abs() < 0.05, "apex {:?}", points[2]);
        assert!((points[0].x - points[4].x).abs() > 4.0);
        assert!((points[0].y - points[4].y).abs() < 0.05);
        assert!((points[1].y - points[3].y).abs() < 0.05);
        assert!(spacing_ratio(&points) < 1.05, "{}", spacing_ratio(&points));
    }

    #[test]
    fn arc_bows_upfield_with_even_steps() {
        let points = stamp(StampKind::Arc, 20.0, 10.0).sample(9);
        assert_eq!(points.len(), 9);
        assert!((points[0].x - 0.0).abs() < 0.8, "{:?}", points[0]);
        assert!((points[8].x - 20.0).abs() < 0.8, "{:?}", points[8]);
        assert!(
            points[4].y > points[0].y + 2.0,
            "middle {:?} ends {:?}",
            points[4],
            points[0]
        );
        assert!(spacing_ratio(&points) < 1.15, "{}", spacing_ratio(&points));
    }

    #[test]
    fn letter_uses_every_selected_performer_inside_the_box() {
        let mut named = stamp(StampKind::Letter, 16.0, 20.0);
        named.text = "HI".into();
        let points = named.sample(40);
        assert_eq!(points.len(), 40);
        for point in &points {
            assert!((2.0..=18.0).contains(&point.x), "{point:?}");
            assert!((8.0..=42.0).contains(&point.y), "{point:?}");
        }
        let blank = Stamp::from_drag(
            StampKind::Letter,
            Point { x: 0.0, y: 0.0 },
            Point { x: 8.0, y: 8.0 },
            " \n",
        );
        assert_eq!(blank.text, "A");
        assert_eq!(blank.sample(6).len(), 6);
    }

    #[test]
    fn resize_and_rotate_handles_move_the_shape() {
        let original = stamp(StampKind::Line, 10.0, 4.0);
        let wider = original.with_resize(Point { x: 20.0, y: 16.0 });
        assert!((wider.width - 20.0).abs() < 0.05, "{}", wider.width);
        assert!((wider.height - 8.0).abs() < 0.05, "{}", wider.height);
        let spun = original.with_rotate(Point { x: 20.0, y: 20.0 });
        let points = spun.sample(2);
        let dx = points[1].x - points[0].x;
        let dy = points[1].y - points[0].y;
        assert!(dy.abs() > dx.abs(), "line should stand up {points:?}");
        let shifted = original.translate(Point { x: 3.0, y: -2.0 });
        assert!((shifted.center.x - 13.0).abs() < 0.01);
        assert!((shifted.center.y - 18.0).abs() < 0.01);
    }

    #[test]
    fn rotate_handle_wins_over_the_box() {
        let original = stamp(StampKind::Block, 12.0, 8.0);
        let gap = 3.0;
        assert_eq!(
            original.hit(original.rotate_handle(gap), 1.0, gap),
            Some(StampGrab::Rotate)
        );
        assert_eq!(
            original.hit(original.resize_handle(), 1.0, gap),
            Some(StampGrab::Resize)
        );
        assert_eq!(
            original.hit(original.center, 1.0, gap),
            Some(StampGrab::Body)
        );
        assert_eq!(original.hit(Point { x: 40.0, y: 40.0 }, 1.0, gap), None);
        let again = original.with_rotate(original.rotate_handle(gap));
        assert!((again.rotation - original.rotation).abs() < 0.02);
    }

    #[test]
    fn selected_performers_snap_and_the_rest_stay() {
        let basis = vec![
            Point { x: 0.0, y: 0.0 },
            Point { x: 1.0, y: 0.0 },
            Point { x: 2.0, y: 0.0 },
            Point { x: 9.0, y: 9.0 },
        ];
        let shape = Stamp {
            kind: StampKind::Line,
            center: Point { x: 5.0, y: 5.0 },
            width: 6.0,
            height: 4.0,
            rotation: 0.0,
            text: "A".into(),
        };
        let placed = placed_positions(&basis, &[0, 1, 2], &shape);
        assert_eq!(placed.len(), 4);
        assert_eq!(placed[3], basis[3]);
        let mut ys = placed[..3].iter().map(|point| point.y).collect::<Vec<_>>();
        ys.sort_by(f32::total_cmp);
        assert!((ys[0] - 5.0).abs() < 0.05);
        assert!((ys[2] - 5.0).abs() < 0.05);
        let targets = shape.sample(3);
        for point in &placed[..3] {
            assert!(
                targets
                    .iter()
                    .any(|target| distance(*point, *target) < 0.05),
                "{point:?} not on {targets:?}"
            );
        }
    }

    #[test]
    fn eighty_on_a_circle_stay_even() {
        let shape = Stamp {
            kind: StampKind::Circle,
            center: Point { x: 50.0, y: 26.7 },
            width: 36.0,
            height: 36.0,
            rotation: 0.15,
            text: "A".into(),
        };
        let points = shape.sample(80);
        assert_eq!(points.len(), 80);
        let mut around = points.clone();
        around.push(points[0]);
        assert!(spacing_ratio(&around) < 1.08, "{}", spacing_ratio(&around));
    }
}
