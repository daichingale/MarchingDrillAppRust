//! Drill-book "continuity": a human-readable Japanese description of how a single
//! performer moves between consecutive sets. Pure logic, no UI.
//!
//! # Direction convention
//!
//! Field `y` increases toward the BACK of the field (away from the audience), so:
//!
//! * decreasing `y` moves toward the audience — 前
//! * increasing `y` moves away from the audience — 後ろ
//! * increasing `x` moves — 右
//! * decreasing `x` moves — 左
//!
//! Combined moves use 8-way names in horizontal-then-vertical order
//! (`右前` / `左後ろ` / `右後ろ` / `左前`), or a single axis (`前` / `右` …).
//! A move shorter than roughly a quarter step is reported as `静止` (a hold).

use crate::{Document, Locale, pathing::path_length};
use serde::{Deserialize, Serialize};

/// Moves smaller than this (in steps) are treated as a hold (`静止`), and an axis
/// component below this magnitude does not contribute to the direction name.
const HOLD_THRESHOLD_STEPS: f32 = 0.25;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TravelDirection {
    Hold,
    Right,
    Left,
    Forward,
    Backward,
    RightForward,
    RightBackward,
    LeftForward,
    LeftBackward,
}

impl TravelDirection {
    pub fn text(self, locale: Locale) -> &'static str {
        use TravelDirection::*;
        match (self, locale) {
            (Hold, Locale::Ja) => "静止",
            (Right, Locale::Ja) => "右",
            (Left, Locale::Ja) => "左",
            (Forward, Locale::Ja) => "前",
            (Backward, Locale::Ja) => "後ろ",
            (RightForward, Locale::Ja) => "右前",
            (RightBackward, Locale::Ja) => "右後ろ",
            (LeftForward, Locale::Ja) => "左前",
            (LeftBackward, Locale::Ja) => "左後ろ",
            (Hold, Locale::En) => "hold",
            (Right, Locale::En) => "right",
            (Left, Locale::En) => "left",
            (Forward, Locale::En) => "forward",
            (Backward, Locale::En) => "backward",
            (RightForward, Locale::En) => "forward-right",
            (RightBackward, Locale::En) => "backward-right",
            (LeftForward, Locale::En) => "forward-left",
            (LeftBackward, Locale::En) => "backward-left",
        }
    }
}

/// One performer's travel across a single set-to-set transition, with both the
/// numeric metrics and a ready-to-print Japanese `description`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContinuitySegment {
    /// Source set index (0-based).
    pub from_set: usize,
    /// Destination set index (0-based).
    pub to_set: usize,
    /// Counts allotted to the move (taken from the source set).
    pub counts: u16,
    /// Horizontal travel in steps (positive = 右), rounded to 0.25.
    pub steps_x: f32,
    /// Vertical travel in steps (positive = 後ろ), rounded to 0.25.
    pub steps_y: f32,
    /// Straight-line travel in steps, rounded to 0.25.
    pub distance_steps: f32,
    /// 8-way Japanese direction name, or `静止` for a hold.
    pub direction: String,
    /// Language-neutral direction used by UI and exporters.
    pub direction_kind: TravelDirection,
    /// Steps travelled per count (`distance_steps / counts`), rounded to 0.25.
    pub step_size_per_count: f32,
    /// Human-readable one-line summary, e.g.
    /// `"セット1→2: 16カウントで右前方へ 8.0歩 (0.50 steps/count)"`.
    pub description: String,
}

/// Round to the nearest quarter (0.25).
fn round_quarter(value: f32) -> f32 {
    (value * 4.0).round() / 4.0
}

/// Field-unit size of one horizontal step, e.g. 5/8 for standard 8-to-5 spacing.
fn horizontal_step(doc: &Document) -> f32 {
    doc.grid.horizontal_units / f32::from(doc.grid.horizontal_steps.max(1))
}

/// Field-unit size of one vertical step.
fn vertical_step(doc: &Document) -> f32 {
    doc.grid.vertical_units / f32::from(doc.grid.vertical_steps.max(1))
}

/// 8-way direction name from rounded per-axis step deltas, or `静止` for a hold.
fn direction_kind(steps_x: f32, steps_y: f32) -> TravelDirection {
    use TravelDirection::*;
    let h = steps_x.abs() >= HOLD_THRESHOLD_STEPS;
    let v = steps_y.abs() >= HOLD_THRESHOLD_STEPS;
    match (h, v, steps_x.is_sign_positive(), steps_y.is_sign_positive()) {
        (false, false, _, _) => Hold,
        (true, false, true, _) => Right,
        (true, false, false, _) => Left,
        (false, true, _, false) => Forward,
        (false, true, _, true) => Backward,
        (true, true, true, false) => RightForward,
        (true, true, true, true) => RightBackward,
        (true, true, false, false) => LeftForward,
        (true, true, false, true) => LeftBackward,
    }
}

pub fn format_segment(value: &ContinuitySegment, locale: Locale) -> String {
    if value.direction_kind == TravelDirection::Hold {
        return if locale == Locale::Ja {
            format!(
                "セット{}→{}: {}カウント静止",
                value.from_set + 1,
                value.to_set + 1,
                value.counts
            )
        } else {
            format!(
                "Set {}→{}: hold for {} counts",
                value.from_set + 1,
                value.to_set + 1,
                value.counts
            )
        };
    }
    if locale == Locale::Ja {
        format!(
            "セット{}→{}: {}カウントで{}方へ {:.1}歩 ({:.2} steps/count)",
            value.from_set + 1,
            value.to_set + 1,
            value.counts,
            value.direction_kind.text(locale),
            value.distance_steps,
            value.step_size_per_count
        )
    } else {
        format!(
            "Set {}→{}: {} counts, {} for {:.1} steps ({:.2} steps/count)",
            value.from_set + 1,
            value.to_set + 1,
            value.counts,
            value.direction_kind.text(locale),
            value.distance_steps,
            value.step_size_per_count
        )
    }
}

/// Build the segment describing the transition from `from_index` to `to_index`
/// for a single performer.
fn segment(
    doc: &Document,
    performer_index: usize,
    from_index: usize,
    to_index: usize,
) -> Option<ContinuitySegment> {
    let from = doc.sets.get(from_index)?;
    let to = doc.sets.get(to_index)?;
    let a = *from.positions.get(performer_index)?;
    let b = *to.positions.get(performer_index)?;

    let hstep = horizontal_step(doc);
    let vstep = vertical_step(doc);
    let counts = from.counts;

    let steps_x = round_quarter((b.x - a.x) / hstep);
    let steps_y = round_quarter((b.y - a.y) / vstep);
    // Distance is measured in horizontal steps, assuming near-uniform 8-to-5.
    let raw_distance_steps = path_length(a, b) / hstep;
    let distance_steps = round_quarter(raw_distance_steps);
    let step_size_per_count = if counts > 0 {
        round_quarter(distance_steps / f32::from(counts))
    } else {
        0.0
    };

    // Hold is decided on the true (unrounded) travel, so anything under ~a
    // quarter step reads as 静止 regardless of quarter-step rounding.
    let is_hold = raw_distance_steps < HOLD_THRESHOLD_STEPS;
    let direction_kind = if is_hold {
        TravelDirection::Hold
    } else {
        direction_kind(steps_x, steps_y)
    };
    let mut value = ContinuitySegment {
        from_set: from_index,
        to_set: to_index,
        counts,
        steps_x,
        steps_y,
        distance_steps,
        direction: direction_kind.text(Locale::Ja).to_owned(),
        direction_kind,
        step_size_per_count,
        description: String::new(),
    };
    value.description = format_segment(&value, Locale::Ja);
    Some(value)
}

/// Continuity for one performer: a segment for every consecutive set pair.
///
/// Returns an empty vector if `performer_index` is out of range or there are
/// fewer than two sets.
pub fn performer_continuity(doc: &Document, performer_index: usize) -> Vec<ContinuitySegment> {
    if performer_index >= doc.performers.len() {
        return Vec::new();
    }
    (0..doc.sets.len().saturating_sub(1))
        .filter_map(|i| segment(doc, performer_index, i, i + 1))
        .collect()
}

/// The performer's label followed by one continuity line per transition.
///
/// Returns an empty string if `performer_index` is out of range.
pub fn continuity_text(doc: &Document, performer_index: usize) -> String {
    continuity_text_localized(doc, performer_index, Locale::Ja)
}
pub fn continuity_text_localized(doc: &Document, performer_index: usize, locale: Locale) -> String {
    let Some(performer) = doc.performers.get(performer_index) else {
        return String::new();
    };
    let mut text = performer.label.clone();
    for segment in performer_continuity(doc, performer_index) {
        text.push('\n');
        text.push_str(&format_segment(&segment, locale));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Point;

    /// A two-set demo whose single performer's positions we can override.
    fn moving_doc(from: Point, to: Point, counts: u16) -> Document {
        let mut doc = Document::demo(1, 1);
        doc.sets[0].counts = counts;
        doc.sets[0].positions[0] = from;
        doc.sets[1].positions[0] = to;
        doc
    }

    #[test]
    fn pure_positive_x_reports_migi_and_step_math() {
        // Default grid: horizontal_units 5, horizontal_steps 8 => 0.625 units/step.
        // +5.0 units => 8 steps to the 右, over 16 counts => 0.5 steps/count.
        let doc = moving_doc(Point { x: 10.0, y: 20.0 }, Point { x: 15.0, y: 20.0 }, 16);
        let segs = performer_continuity(&doc, 0);
        assert_eq!(segs.len(), 1);
        let s = &segs[0];
        assert_eq!(s.direction, "右");
        assert!((s.steps_x - 8.0).abs() < 1e-6);
        assert!((s.steps_y - 0.0).abs() < 1e-6);
        assert!((s.distance_steps - 8.0).abs() < 1e-6);
        assert!((s.step_size_per_count - 0.5).abs() < 1e-6);
        assert_eq!(s.from_set, 0);
        assert_eq!(s.to_set, 1);
        assert_eq!(s.counts, 16);
        assert!(s.description.contains("右方へ"));
        assert!(s.description.contains("8.0歩"));
        assert!(s.description.contains("0.50 steps/count"));
    }

    #[test]
    fn negative_y_is_forward_and_diagonal_combines_horizontal_then_vertical() {
        // +x (右) and -y (前) => 右前.
        let doc = moving_doc(Point { x: 10.0, y: 20.0 }, Point { x: 15.0, y: 15.0 }, 16);
        let s = &performer_continuity(&doc, 0)[0];
        assert_eq!(s.direction, "右前");
        assert!(s.steps_x > 0.0);
        assert!(s.steps_y < 0.0);
        assert!(s.description.contains("右前方へ"));
    }

    #[test]
    fn positive_y_is_backward() {
        // -x (左) and +y (後ろ) => 左後ろ.
        let doc = moving_doc(Point { x: 15.0, y: 15.0 }, Point { x: 10.0, y: 20.0 }, 16);
        let s = &performer_continuity(&doc, 0)[0];
        assert_eq!(s.direction, "左後ろ");
    }

    #[test]
    fn stationary_performer_reports_hold() {
        let doc = moving_doc(Point { x: 12.0, y: 12.0 }, Point { x: 12.0, y: 12.0 }, 16);
        let s = &performer_continuity(&doc, 0)[0];
        assert_eq!(s.direction, "静止");
        assert!((s.distance_steps).abs() < 1e-6);
        assert_eq!(s.description, "セット1→2: 16カウント静止");
    }

    #[test]
    fn tiny_move_under_quarter_step_is_hold() {
        // 0.05 units ~= 0.08 steps < 0.25 => 静止.
        let doc = moving_doc(Point { x: 12.0, y: 12.0 }, Point { x: 12.05, y: 12.0 }, 16);
        let s = &performer_continuity(&doc, 0)[0];
        assert_eq!(s.direction, "静止");
    }

    #[test]
    fn one_segment_per_consecutive_pair() {
        let mut doc = Document::demo(2, 2);
        // Add a third set so we expect sets.len() - 1 == 2 segments.
        let mut third = doc.sets[1].clone();
        third.name = "セット 3".into();
        doc.sets.push(third);
        let segs = performer_continuity(&doc, 0);
        assert_eq!(segs.len(), doc.sets.len() - 1);
        assert_eq!(segs.len(), 2);
    }

    #[test]
    fn step_size_math_checked_by_hand() {
        // 10 units of +x with default 0.625 units/step => 16 steps; 8 counts => 2.0/count.
        let doc = moving_doc(Point { x: 0.0, y: 30.0 }, Point { x: 10.0, y: 30.0 }, 8);
        let s = &performer_continuity(&doc, 0)[0];
        assert!((s.distance_steps - 16.0).abs() < 1e-6);
        assert!((s.step_size_per_count - 2.0).abs() < 1e-6);
    }

    #[test]
    fn zero_counts_guarded() {
        let doc = moving_doc(Point { x: 0.0, y: 0.0 }, Point { x: 5.0, y: 0.0 }, 0);
        let s = &performer_continuity(&doc, 0)[0];
        assert_eq!(s.step_size_per_count, 0.0);
    }

    #[test]
    fn out_of_range_performer_is_empty() {
        let doc = Document::demo(1, 1);
        assert!(performer_continuity(&doc, 99).is_empty());
        assert_eq!(continuity_text(&doc, 99), "");
    }

    #[test]
    fn continuity_text_starts_with_label_then_lines() {
        let doc = moving_doc(Point { x: 10.0, y: 20.0 }, Point { x: 15.0, y: 20.0 }, 16);
        let text = continuity_text(&doc, 0);
        let mut lines = text.lines();
        assert_eq!(lines.next().unwrap(), doc.performers[0].label);
        assert!(lines.next().unwrap().starts_with("セット1→2:"));
    }

    #[test]
    fn english_continuity_is_complete_and_has_no_japanese_glyphs() {
        let doc = moving_doc(Point { x: 10.0, y: 20.0 }, Point { x: 15.0, y: 15.0 }, 16);
        let value = &performer_continuity(&doc, 0)[0];
        assert_eq!(value.direction_kind, TravelDirection::RightForward);
        let text = format_segment(value, Locale::En);
        assert!(text.contains("Set 1→2: 16 counts, forward-right"), "{text}");
        assert!(
            !text.chars().any(|c| ('\u{3040}'..='\u{30ff}').contains(&c)
                || ('\u{4e00}'..='\u{9fff}').contains(&c))
        );
    }
}
