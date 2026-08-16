//! Deterministic playback clock, independent from UI and wall-clock APIs.

use crate::tempo::TempoMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaybackRange {
    pub start: f32,
    pub end: f32,
}

impl PlaybackRange {
    pub fn new(start: f32, end: f32, total_counts: f32) -> Self {
        let total = finite_non_negative(total_counts);
        let start = finite_non_negative(start).min(total);
        let end = finite_non_negative(end).clamp(start, total);
        Self { start, end }
    }

    pub fn is_empty(self) -> bool {
        self.end <= self.start
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AdvanceResult {
    Running(f32),
    Looped(f32),
    Stopped(f32),
}

impl AdvanceResult {
    pub fn count(self) -> f32 {
        match self {
            Self::Running(count) | Self::Looped(count) | Self::Stopped(count) => count,
        }
    }
}

/// Advances playback in real seconds while respecting variable tempo.
///
/// This function is deterministic and allocation-free. Large frame gaps are
/// supported, and looping preserves overshoot instead of visibly stuttering at
/// the range boundary.
pub fn advance(
    current_count: f32,
    elapsed_seconds: f32,
    speed: f32,
    range: PlaybackRange,
    looping: bool,
    tempo: &TempoMap,
) -> AdvanceResult {
    if range.is_empty() {
        return AdvanceResult::Stopped(range.start);
    }
    let current = if current_count.is_finite() {
        current_count.clamp(range.start, range.end)
    } else {
        range.start
    };
    let elapsed = finite_non_negative(elapsed_seconds);
    let speed = if speed.is_finite() {
        speed.max(0.0)
    } else {
        1.0
    };
    let target_seconds =
        tempo.seconds_at_f64(f64::from(current)) + f64::from(elapsed) * f64::from(speed);
    let end_seconds = tempo.seconds_at_f64(f64::from(range.end));
    if target_seconds < end_seconds {
        return AdvanceResult::Running(tempo.count_at_f64(target_seconds) as f32);
    }
    if !looping {
        return AdvanceResult::Stopped(range.end);
    }
    let start_seconds = tempo.seconds_at_f64(f64::from(range.start));
    let duration = end_seconds - start_seconds;
    if duration <= f64::EPSILON {
        return AdvanceResult::Stopped(range.start);
    }
    let wrapped_seconds = start_seconds + (target_seconds - start_seconds).rem_euclid(duration);
    AdvanceResult::Looped(tempo.count_at_f64(wrapped_seconds) as f32)
}

fn finite_non_negative(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::TempoChange;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn advances_at_constant_tempo() {
        let tempo = TempoMap::constant(120.0);
        let result = advance(
            0.0,
            1.0,
            1.0,
            PlaybackRange::new(0.0, 16.0, 16.0),
            false,
            &tempo,
        );
        assert!(matches!(result, AdvanceResult::Running(count) if approx(count, 2.0)));
    }

    #[test]
    fn stops_exactly_at_range_end() {
        let tempo = TempoMap::constant(120.0);
        let result = advance(
            7.5,
            1.0,
            1.0,
            PlaybackRange::new(4.0, 8.0, 16.0),
            false,
            &tempo,
        );
        assert_eq!(result, AdvanceResult::Stopped(8.0));
    }

    #[test]
    fn loop_preserves_overshoot() {
        let tempo = TempoMap::constant(120.0);
        let result = advance(
            7.0,
            1.0,
            1.0,
            PlaybackRange::new(4.0, 8.0, 16.0),
            true,
            &tempo,
        );
        assert!(matches!(result, AdvanceResult::Looped(count) if approx(count, 5.0)));
    }

    #[test]
    fn integrates_across_tempo_change() {
        let tempo = TempoMap::from_changes([
            TempoChange {
                count: 0.0,
                bpm: 120.0,
            },
            TempoChange {
                count: 4.0,
                bpm: 60.0,
            },
        ]);
        let result = advance(
            3.0,
            2.0,
            1.0,
            PlaybackRange::new(0.0, 16.0, 16.0),
            false,
            &tempo,
        );
        assert!(matches!(result, AdvanceResult::Running(count) if approx(count, 5.5)));
    }

    #[test]
    fn guards_invalid_values() {
        let tempo = TempoMap::default();
        let range = PlaybackRange::new(f32::NAN, f32::INFINITY, 16.0);
        assert_eq!(
            range,
            PlaybackRange {
                start: 0.0,
                end: 0.0
            }
        );
        assert_eq!(
            advance(f32::NAN, -1.0, f32::NAN, range, true, &tempo),
            AdvanceResult::Stopped(0.0)
        );
    }

    #[test]
    fn many_small_steps_match_one_large_step() {
        let tempo = TempoMap::from_changes([
            TempoChange {
                count: 0.0,
                bpm: 137.0,
            },
            TempoChange {
                count: 64.0,
                bpm: 83.0,
            },
            TempoChange {
                count: 128.0,
                bpm: 191.0,
            },
        ]);
        let range = PlaybackRange::new(0.0, 256.0, 256.0);
        let one = advance(0.0, 10.0, 1.0, range, false, &tempo).count();
        let mut many = 0.0;
        for _ in 0..600 {
            many = advance(many, 1.0 / 60.0, 1.0, range, false, &tempo).count();
        }
        assert!((one - many).abs() < 1e-3, "{one} != {many}");
    }
}
