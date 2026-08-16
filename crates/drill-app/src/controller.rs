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
) -> Point {
    let scale = scale.max(f32::EPSILON);
    let dx = screen_delta.0 / scale;
    let dy = -screen_delta.1 / scale;
    document.grid.snap(Point {
        x: (start.x + dx).clamp(0.0, document.grid.width),
        y: (start.y + dy).clamp(0.0, document.grid.height),
    })
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
    fn drag_is_clamped_and_snapped_by_document_grid() {
        let document = Document::demo(2, 2);
        let point = drag_point(Point { x: 0.0, y: 0.0 }, (-1000.0, 1000.0), 1.0, &document);
        assert_eq!(point, Point { x: 0.0, y: 0.0 });
    }

    #[test]
    fn drag_uses_one_shared_scale_not_per_axis_stretch() {
        // A non-square viewport must not distort drag distance: moving the
        // pointer the same number of pixels on x and y should move the
        // performer the same document-unit distance on both axes.
        let document = Document::demo(2, 2);
        let point = drag_point(Point { x: 10.0, y: 10.0 }, (20.0, 20.0), 4.0, &document);
        assert_eq!(point, document.grid.snap(Point { x: 15.0, y: 5.0 }));
    }
}
