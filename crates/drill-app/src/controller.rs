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

pub(crate) fn drag_point(
    start: Point,
    screen_delta: (f32, f32),
    viewport: (f32, f32),
    document: &Document,
) -> Point {
    let dx = screen_delta.0 / viewport.0.max(f32::EPSILON) * document.grid.width;
    let dy = -screen_delta.1 / viewport.1.max(f32::EPSILON) * document.grid.height;
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
        let point = drag_point(
            Point { x: 0.0, y: 0.0 },
            (-1000.0, 1000.0),
            (100.0, 100.0),
            &document,
        );
        assert_eq!(point, Point { x: 0.0, y: 0.0 });
    }
}
