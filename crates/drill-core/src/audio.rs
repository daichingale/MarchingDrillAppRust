//! Reference-audio sync model: how a recorded track lines up with the count
//! timeline. Pure logic, no playback — the app layer drives an actual audio
//! device using the mappings computed here.
//!
//! The show timeline is measured in *counts*; a [`crate::tempo::TempoMap`]
//! converts counts to show-relative seconds (0 s at count 0). An
//! [`AudioTrack`] adds a single [`offset_seconds`](AudioTrack::offset_seconds)
//! so that show time can be translated to a playback position within the file.

use crate::tempo::TempoMap;
use serde::{Deserialize, Serialize};

/// A reference audio file aligned to the count timeline.
///
/// The alignment is a single scalar: [`offset_seconds`](Self::offset_seconds)
/// is the playback position (in seconds into the file) that corresponds to
/// global count 0.
///
/// - **Positive** offset: the show starts partway into the file (there is
///   audio, e.g. a lead-in, before count 0).
/// - **Negative** offset: count 0 precedes the start of the audio (the file
///   begins after the show does).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AudioTrack {
    /// Path to the audio file (interpreted by the app layer).
    pub path: String,
    /// Total length of the audio file in seconds.
    pub duration_seconds: f32,
    /// Playback position (seconds into the file) aligned to global count 0.
    /// Positive = show starts inside the file; negative = count 0 precedes it.
    pub offset_seconds: f32,
}

/// Playback position (seconds into the file) for a given global count.
///
/// This is `offset_seconds + tempo.seconds_at(global_count)` and may be
/// negative when the offset is negative and the count is small.
pub fn count_to_audio_time(track: &AudioTrack, tempo: &TempoMap, global_count: f32) -> f32 {
    track.offset_seconds + tempo.seconds_at(global_count)
}

/// Global count for a given playback position (seconds into the file).
///
/// Inverse of [`count_to_audio_time`]. Audio times before the show's count 0
/// clamp to count 0 (via [`TempoMap::count_at`], whose input is clamped).
pub fn audio_time_to_count(track: &AudioTrack, tempo: &TempoMap, audio_seconds: f32) -> f32 {
    tempo.count_at(audio_seconds - track.offset_seconds)
}

/// Show-relative seconds timestamp of every count `0..=total_counts`.
///
/// Entry `n` is `tempo.seconds_at(n)` — the wall-clock time of count `n`
/// measured from count 0 (i.e. *not* offset into the file). Useful for
/// generating a metronome/click track. The result has `total_counts + 1`
/// entries and is built in a single allocation.
pub fn click_track(tempo: &TempoMap, total_counts: u32) -> Vec<f32> {
    let mut clicks = Vec::with_capacity(total_counts as usize + 1);
    for n in 0..=total_counts {
        clicks.push(tempo.seconds_at(n as f32));
    }
    clicks
}

/// Show-relative seconds timestamps of the counts that fall on a measure
/// downbeat — every `beats_per_measure` counts starting at count 0.
///
/// `beats_per_measure` is clamped to at least 1. The result is built in a
/// single allocation.
pub fn downbeats(tempo: &TempoMap, total_counts: u32, beats_per_measure: u16) -> Vec<f32> {
    let step = beats_per_measure.max(1) as u32;
    let mut beats = Vec::with_capacity((total_counts / step) as usize + 1);
    let mut n = 0;
    while n <= total_counts {
        beats.push(tempo.seconds_at(n as f32));
        n += step;
    }
    beats
}

/// Whether the audio position mapped from `global_count` lies within the file,
/// i.e. inside the closed interval `[0, duration_seconds]`.
pub fn is_within_audio(track: &AudioTrack, tempo: &TempoMap, global_count: f32) -> bool {
    let t = count_to_audio_time(track, tempo, global_count);
    (0.0..=track.duration_seconds).contains(&t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    // 120 bpm => 2 counts/sec => 0.5 s per count.
    fn tempo() -> TempoMap {
        TempoMap::constant(120.0)
    }

    fn make_track(offset: f32, duration: f32) -> AudioTrack {
        AudioTrack {
            path: "ref.wav".into(),
            duration_seconds: duration,
            offset_seconds: offset,
        }
    }

    #[test]
    fn count_and_audio_time_are_inverses() {
        let tempo = tempo();
        let track = make_track(1.5, 300.0);
        for count in [0.0, 1.0, 4.0, 16.5, 128.0] {
            let audio = count_to_audio_time(&track, &tempo, count);
            assert!(
                approx(audio_time_to_count(&track, &tempo, audio), count),
                "round trip failed at count {count}"
            );
        }
        // Hand-checked: count 4 at 0.5 s/count = 2.0 s, plus 1.5 s offset.
        assert!(approx(count_to_audio_time(&track, &tempo, 4.0), 3.5));
    }

    #[test]
    fn audio_time_before_count_zero_clamps() {
        let tempo = tempo();
        let track = make_track(2.0, 300.0);
        // Any audio time <= offset maps to count 0.
        assert!(approx(audio_time_to_count(&track, &tempo, 0.0), 0.0));
        assert!(approx(audio_time_to_count(&track, &tempo, 1.0), 0.0));
    }

    #[test]
    fn click_track_has_one_entry_per_count_plus_one() {
        let tempo = tempo();
        let clicks = click_track(&tempo, 8);
        assert_eq!(clicks.len(), 9);
        assert!(approx(clicks[0], 0.0));
        // 0.5 s spacing between consecutive counts.
        for pair in clicks.windows(2) {
            assert!(approx(pair[1] - pair[0], 0.5));
        }
        assert!(approx(*clicks.last().unwrap(), 4.0));
    }

    #[test]
    fn downbeats_in_four_four_land_every_two_seconds() {
        let tempo = tempo();
        // 4 counts/measure at 0.5 s/count = 2.0 s/measure.
        let beats = downbeats(&tempo, 16, 4);
        assert_eq!(beats.len(), 5); // counts 0,4,8,12,16
        for (i, &t) in beats.iter().enumerate() {
            assert!(approx(t, i as f32 * 2.0));
        }
    }

    #[test]
    fn downbeats_clamps_zero_beats_per_measure() {
        let tempo = tempo();
        // Clamped to 1 => a downbeat on every count.
        let beats = downbeats(&tempo, 4, 0);
        assert_eq!(beats.len(), 5);
    }

    #[test]
    fn is_within_audio_boundaries() {
        let tempo = tempo();
        // offset 0, duration 4 s. Count 0 -> 0 s, count 8 -> 4 s.
        let track = make_track(0.0, 4.0);
        assert!(is_within_audio(&track, &tempo, 0.0)); // 0 s, lower bound
        assert!(is_within_audio(&track, &tempo, 8.0)); // 4 s, upper bound
        assert!(!is_within_audio(&track, &tempo, 8.5)); // 4.25 s, past end

        // Negative offset places count 0 before the file start.
        let late = make_track(-1.0, 10.0);
        assert!(!is_within_audio(&late, &tempo, 0.0)); // -1 s, before start
        assert!(is_within_audio(&late, &tempo, 2.0)); // -1 + 1 = 0 s, lower bound
    }

    #[test]
    fn json_round_trip() {
        let track = make_track(1.5, 42.0);
        let json = serde_json::to_string(&track).unwrap();
        let back: AudioTrack = serde_json::from_str(&json).unwrap();
        assert!(approx(back.offset_seconds, 1.5));
        assert!(approx(back.duration_seconds, 42.0));
        assert_eq!(back.path, "ref.wav");
    }
}
