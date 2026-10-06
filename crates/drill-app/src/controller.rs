//! UI-independent application decisions.
//!
//! Keeping time progression and field interaction math here makes those rules
//! deterministic and testable without constructing an egui frame or audio device.

use drill_core::playback::{AdvanceResult, PlaybackRange, advance};
use drill_core::{Document, Point};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PlaybackDecision {
    Seek(f32),
    LoopTo(f32),
    StopAt(f32),
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PlaybackInput {
    pub global_count: f32,
    pub audio_count: Option<f32>,
    pub dt_seconds: f32,
    pub speed: f32,
    pub range_start: u32,
    pub range_end: u32,
    pub loop_enabled: bool,
}

/// Advances playback from either the sample clock or the tempo map.
///
/// The sample clock wins when available. This prevents graphics-frame jitter
/// from accumulating into audio/video sync drift.
pub(crate) fn playback_decision(document: &Document, input: PlaybackInput) -> PlaybackDecision {
    if let Some(count) = input.audio_count {
        if count >= input.range_end as f32 {
            return if input.loop_enabled {
                PlaybackDecision::LoopTo(input.range_start as f32)
            } else {
                PlaybackDecision::StopAt(input.range_end as f32)
            };
        }
        return PlaybackDecision::Seek(count.max(input.range_start as f32));
    }

    let result = advance(
        input.global_count,
        input.dt_seconds,
        input.speed,
        PlaybackRange::new(
            input.range_start as f32,
            input.range_end as f32,
            document.timeline_counts() as f32,
        ),
        input.loop_enabled,
        &document.tempo,
    );
    match result {
        AdvanceResult::Running(count) => PlaybackDecision::Seek(count),
        AdvanceResult::Looped(count) => PlaybackDecision::LoopTo(count),
        AdvanceResult::Stopped(count) => PlaybackDecision::StopAt(count),
    }
}

/// Counts still left in a count-in after `dt_seconds` at `bpm` and `speed`.
///
/// A count-in holds the picture: the drill does not move until this reaches
/// zero. Non-finite or non-positive tempo and speed fall back to 120 BPM and
/// 1× so a bad clock cannot stall or jump the wait.
pub(crate) fn advance_count_in(remaining: f32, dt_seconds: f32, bpm: f32, speed: f32) -> f32 {
    if !(remaining.is_finite() && remaining > 0.0) {
        return 0.0;
    }
    let bpm = if bpm.is_finite() && bpm > 0.0 {
        bpm
    } else {
        120.0
    };
    let speed = if speed.is_finite() && speed > 0.0 {
        speed
    } else {
        1.0
    };
    let dt = if dt_seconds.is_finite() && dt_seconds > 0.0 {
        dt_seconds
    } else {
        0.0
    };
    (remaining - dt * (bpm / 60.0) * speed).max(0.0)
}

/// Pointer travel (pixels) below which a press is a click, not a move.
/// egui reports `drag_started` for tiny motion, and snapping a zero-pixel
/// drag would jump an off-grid performer the moment they were clicked.
pub(crate) const DRAG_COMMIT_PIXELS: f32 = 4.0;

pub(crate) fn pointer_drag_committed(screen_delta: (f32, f32)) -> bool {
    screen_delta.0.hypot(screen_delta.1) >= DRAG_COMMIT_PIXELS
}

/// Snap (optional) then clamp onto the writable field. `width`/`height` are
/// the canvas size; [`drill_core::GridConfig::max_x`] is the last on-grid
/// point that still fits, so clamping to the canvas then snapping can land
/// a performer off-grid or past the sideline.
pub(crate) fn field_point(raw: Point, document: &Document, snap: bool) -> Point {
    let point = if snap { document.grid.snap(raw) } else { raw };
    Point {
        x: point.x.clamp(0.0, document.grid.max_x()),
        y: point.y.clamp(0.0, document.grid.max_y()),
    }
}

/// `scale` must be the same field<->screen scale the view is drawn and
/// hit-tested with (see [`drill_render::FieldMap`]), not an independent
/// per-axis stretch — otherwise a drag moves a performer by a different
/// amount than the pointer actually traveled whenever the viewport's aspect
/// ratio doesn't match the field's.
pub(crate) fn drag_point(
    start: Point,
    screen_delta: (f32, f32),
    scale: f32,
    document: &Document,
    snap: bool,
) -> Point {
    let scale = scale.max(f32::EPSILON);
    let dx = screen_delta.0 / scale;
    let dy = -screen_delta.1 / scale;
    field_point(
        Point {
            x: start.x + dx,
            y: start.y + dy,
        },
        document,
        snap,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_clock_stops_exactly_at_out_point() {
        let document = Document::demo(2, 2);
        let decision = playback_decision(
            &document,
            PlaybackInput {
                global_count: 3.0,
                audio_count: Some(99.0),
                dt_seconds: 0.016,
                speed: 1.0,
                range_start: 2,
                range_end: 8,
                loop_enabled: false,
            },
        );
        assert_eq!(decision, PlaybackDecision::StopAt(8.0));
    }

    #[test]
    fn audio_clock_loops_to_exact_in_point() {
        let document = Document::demo(2, 2);
        let decision = playback_decision(
            &document,
            PlaybackInput {
                global_count: 7.9,
                audio_count: Some(8.0),
                dt_seconds: 0.016,
                speed: 1.0,
                range_start: 2,
                range_end: 8,
                loop_enabled: true,
            },
        );
        assert_eq!(decision, PlaybackDecision::LoopTo(2.0));
    }

    #[test]
    fn count_in_uses_the_tempo_and_never_goes_negative() {
        assert!((advance_count_in(4.0, 0.5, 120.0, 1.0) - 3.0).abs() < 1e-4);
        assert!((advance_count_in(4.0, 0.5, 120.0, 2.0) - 2.0).abs() < 1e-4);
        assert_eq!(advance_count_in(1.0, 10.0, 120.0, 1.0), 0.0);
        assert_eq!(advance_count_in(4.0, f32::NAN, 120.0, 1.0), 4.0);
        assert!((advance_count_in(4.0, 0.5, f32::NAN, 1.0) - 3.0).abs() < 1e-4);
    }

    #[test]
    fn drag_is_clamped_and_snapped_by_document_grid() {
        let document = Document::demo(2, 2);
        let point = drag_point(
            Point { x: 0.0, y: 0.0 },
            (-1000.0, 1000.0),
            1.0,
            &document,
            true,
        );
        assert_eq!(point, Point { x: 0.0, y: 0.0 });
    }

    #[test]
    fn drag_uses_one_shared_scale_not_per_axis_stretch() {
        // A non-square viewport must not distort drag distance: moving the
        // pointer the same number of pixels on x and y should move the
        // performer the same document-unit distance on both axes.
        let document = Document::demo(2, 2);
        let point = drag_point(
            Point { x: 10.0, y: 10.0 },
            (20.0, 20.0),
            4.0,
            &document,
            true,
        );
        assert_eq!(point, document.grid.snap(Point { x: 15.0, y: 5.0 }));
    }

    #[test]
    fn drag_clamps_to_the_last_on_grid_point_not_the_canvas_edge() {
        let document = Document::blank(1);
        let max = document.grid.max_x();
        assert!(max < document.grid.width);
        let point = drag_point(
            Point {
                x: max - 1.0,
                y: 10.0,
            },
            (100_000.0, 0.0),
            1.0,
            &document,
            true,
        );
        assert_eq!(point.x, max);
        assert_eq!(point, document.grid.snap(point));
        let free = drag_point(Point { x: 8.0, y: 8.0 }, (3.0, -2.0), 1.0, &document, false);
        assert_eq!(free, Point { x: 11.0, y: 10.0 });
    }

    #[test]
    fn tiny_pointer_travel_is_a_click_not_a_move() {
        assert!(!pointer_drag_committed((0.0, 0.0)));
        assert!(!pointer_drag_committed((3.0, 0.0)));
        assert!(pointer_drag_committed((DRAG_COMMIT_PIXELS, 0.0)));
        assert!(pointer_drag_committed((3.0, 3.0)));
    }
}
