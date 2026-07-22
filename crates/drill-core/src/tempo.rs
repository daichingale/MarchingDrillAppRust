//! Tempo map: variable BPM over the count timeline, and count<->seconds<->measure
//! conversion. Pure logic, no UI.
//!
//! A show is measured in *counts* (usually 1 count = 1 beat). The tempo (BPM)
//! may change over the show, so the mapping between counts and wall-clock time
//! is piecewise-constant: each [`TempoChange`] anchors a BPM at a global count
//! and stays in effect until the next change. Time for a span of `N` counts at
//! `B` BPM is `N * 60 / B` seconds.

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

/// Fallback tempo used for an empty map or a guarded (non-finite/≤0) BPM.
pub const DEFAULT_BPM: f32 = 120.0;

/// A single tempo change: `bpm` takes effect at global `count` and holds until
/// the next change. The earliest change effectively covers count 0, so counts
/// before it borrow its BPM.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TempoChange {
    /// Global count at which this BPM begins.
    pub count: f32,
    /// Beats (counts) per minute from this point onward.
    pub bpm: f32,
}

/// An ordered set of tempo changes covering the whole timeline.
///
/// Invariants (upheld by the mutating methods): `events` is sorted ascending by
/// `count`, and each `count` is unique. The first event's BPM covers all counts
/// from 0 up to the second event. An empty map behaves as a constant
/// [`DEFAULT_BPM`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TempoMap {
    events: Vec<TempoChange>,
}

impl Default for TempoMap {
    /// A constant [`DEFAULT_BPM`] (120) starting at count 0.
    fn default() -> Self {
        Self::constant(DEFAULT_BPM)
    }
}

impl TempoMap {
    /// A map with a single, constant tempo anchored at count 0.
    pub fn constant(bpm: f32) -> Self {
        Self {
            events: vec![TempoChange { count: 0.0, bpm }],
        }
    }

    /// Build from arbitrary changes; they are sorted and deduplicated (a later
    /// duplicate count wins). An empty input yields an empty map.
    pub fn from_changes(changes: impl IntoIterator<Item = TempoChange>) -> Self {
        let mut map = Self { events: Vec::new() };
        for change in changes {
            map.set(change.count, change.bpm);
        }
        map
    }

    /// The tempo changes in ascending count order.
    pub fn events(&self) -> &[TempoChange] {
        &self.events
    }

    /// `true` when there are no explicit changes (behaves as [`DEFAULT_BPM`]).
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Insert a tempo change, or replace the BPM of an existing change at the
    /// same count. Keeps the map sorted. Negative counts clamp to 0. Returns
    /// `&mut self` for chaining.
    pub fn set(&mut self, count: f32, bpm: f32) -> &mut Self {
        let count = count.max(0.0);
        match self.events.iter().position(|e| e.count == count) {
            Some(i) => self.events[i].bpm = bpm,
            None => {
                self.events.push(TempoChange { count, bpm });
                self.events
                    .sort_by(|a, b| a.count.partial_cmp(&b.count).unwrap_or(Ordering::Equal));
            }
        }
        self
    }

    /// Remove the change anchored exactly at `count`, if any.
    pub fn remove(&mut self, count: f32) {
        self.events.retain(|e| e.count != count.max(0.0));
    }

    /// The active BPM at `global_count`. Counts before the first change use the
    /// first change's BPM; at an exact change boundary the new BPM applies.
    pub fn bpm_at(&self, global_count: f32) -> f32 {
        let Some(first) = self.events.first() else {
            return DEFAULT_BPM;
        };
        let count = global_count.max(0.0);
        let mut bpm = first.bpm;
        for event in &self.events {
            if segment_start(event.count) <= count {
                bpm = event.bpm;
            } else {
                break;
            }
        }
        sanitize_bpm(bpm)
    }

    /// Cumulative real time, in seconds, from count 0 to `global_count`,
    /// integrating across each piecewise-constant BPM segment. Counts ≤ 0 map
    /// to 0 seconds.
    pub fn seconds_at(&self, global_count: f32) -> f32 {
        let target = global_count.max(0.0);
        if self.events.is_empty() {
            return target * 60.0 / DEFAULT_BPM;
        }
        let mut seconds = 0.0;
        for i in 0..self.events.len() {
            let start = if i == 0 {
                0.0
            } else {
                segment_start(self.events[i].count)
            };
            if target <= start {
                break;
            }
            let end = self
                .events
                .get(i + 1)
                .map_or(f32::INFINITY, |e| segment_start(e.count));
            let span = (target.min(end) - start).max(0.0);
            seconds += span * 60.0 / sanitize_bpm(self.events[i].bpm);
            if target <= end {
                break;
            }
        }
        seconds
    }

    /// Inverse of [`seconds_at`](Self::seconds_at): the global count reached
    /// after `seconds` of real time from count 0. Seconds ≤ 0 map to count 0.
    pub fn count_at(&self, seconds: f32) -> f32 {
        let target = seconds.max(0.0);
        if self.events.is_empty() {
            return target * DEFAULT_BPM / 60.0;
        }
        let mut elapsed = 0.0;
        for i in 0..self.events.len() {
            let start = if i == 0 {
                0.0
            } else {
                segment_start(self.events[i].count)
            };
            let end = self
                .events
                .get(i + 1)
                .map_or(f32::INFINITY, |e| segment_start(e.count));
            let bpm = sanitize_bpm(self.events[i].bpm);
            let seg_seconds = (end - start) * 60.0 / bpm;
            if target <= elapsed + seg_seconds {
                return start + (target - elapsed) * bpm / 60.0;
            }
            elapsed += seg_seconds;
        }
        // Unreachable: the final segment extends to infinity.
        self.events.last().map_or(0.0, |e| e.count)
    }

    /// Musical position for `global_count`, assuming counts map 1:1 to beats
    /// starting at measure 1, beat 1. Returns `(measure, beat)` where `beat` is
    /// in `[1.0, beats_per_measure + 1.0)`. `beats_per_measure` is clamped to at
    /// least 1.
    pub fn measure_beat(&self, global_count: f32, beats_per_measure: u16) -> (u32, f32) {
        let per_measure = beats_per_measure.max(1) as f32;
        let count = global_count.max(0.0);
        let measure = (count / per_measure).floor();
        let beat = count - measure * per_measure + 1.0;
        (measure as u32 + 1, beat)
    }
}

/// Effective segment start: a change anchored before count 0 still begins at 0.
#[inline]
fn segment_start(count: f32) -> f32 {
    count.max(0.0)
}

/// Guard against non-finite or non-positive BPM, which would produce infinite
/// or negative durations.
#[inline]
fn sanitize_bpm(bpm: f32) -> f32 {
    if bpm.is_finite() && bpm > 0.0 {
        bpm
    } else {
        DEFAULT_BPM
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn default_is_constant_120() {
        let map = TempoMap::default();
        assert_eq!(map.events().len(), 1);
        assert!(approx(map.bpm_at(0.0), 120.0));
        assert!(approx(map.bpm_at(1000.0), 120.0));
    }

    #[test]
    fn constant_tempo_round_trips() {
        let map = TempoMap::constant(144.0);
        // 144 bpm -> 144 counts is 144 * 60 / 144 = 60 s.
        assert!(approx(map.seconds_at(144.0), 60.0));
        for count in [0.0, 1.0, 7.5, 32.0, 160.0] {
            let seconds = map.seconds_at(count);
            assert!(
                approx(map.count_at(seconds), count),
                "round trip failed at {count}"
            );
        }
    }

    #[test]
    fn two_segment_hand_computed_seconds() {
        // 120 bpm for counts [0,16), then 60 bpm from count 16 on.
        let mut map = TempoMap::constant(120.0);
        map.set(16.0, 60.0);
        // First 16 counts at 120 bpm: 16 * 60 / 120 = 8 s.
        assert!(approx(map.seconds_at(16.0), 8.0));
        // Next 16 counts at 60 bpm: 16 * 60 / 60 = 16 s, total 24 s.
        assert!(approx(map.seconds_at(32.0), 24.0));
        // Partway into the first segment.
        assert!(approx(map.seconds_at(8.0), 4.0));
        // Inverse must agree.
        assert!(approx(map.count_at(8.0), 16.0));
        assert!(approx(map.count_at(24.0), 32.0));
        assert!(approx(map.count_at(16.0), 24.0));
    }

    #[test]
    fn bpm_at_boundaries() {
        let mut map = TempoMap::constant(120.0);
        map.set(16.0, 60.0);
        assert!(approx(map.bpm_at(0.0), 120.0));
        assert!(approx(map.bpm_at(15.999), 120.0));
        assert!(approx(map.bpm_at(16.0), 60.0)); // boundary takes the new tempo
        assert!(approx(map.bpm_at(500.0), 60.0));
    }

    #[test]
    fn counts_before_first_event_use_first_bpm() {
        // First change is not at 0; earlier counts borrow its BPM.
        let map = TempoMap::from_changes([TempoChange {
            count: 8.0,
            bpm: 90.0,
        }]);
        assert!(approx(map.bpm_at(0.0), 90.0));
        assert!(approx(map.bpm_at(4.0), 90.0));
        // Time before the first anchor integrates at the first BPM.
        assert!(approx(map.seconds_at(8.0), 8.0 * 60.0 / 90.0));
    }

    #[test]
    fn empty_map_behaves_as_default() {
        let map = TempoMap::from_changes([]);
        assert!(map.is_empty());
        assert!(approx(map.bpm_at(10.0), DEFAULT_BPM));
        assert!(approx(map.seconds_at(120.0), 60.0));
        assert!(approx(map.count_at(60.0), 120.0));
    }

    #[test]
    fn guards_zero_and_negative_bpm() {
        let map = TempoMap::constant(0.0);
        // Guarded to DEFAULT_BPM, so time is finite.
        assert!(map.seconds_at(120.0).is_finite());
        assert!(approx(map.seconds_at(120.0), 60.0));
        assert!(approx(map.bpm_at(0.0), DEFAULT_BPM));
    }

    #[test]
    fn set_replaces_and_keeps_sorted() {
        let mut map = TempoMap::constant(120.0);
        map.set(32.0, 80.0);
        map.set(16.0, 60.0);
        map.set(16.0, 100.0); // replace, not duplicate
        let counts: Vec<f32> = map.events().iter().map(|e| e.count).collect();
        assert_eq!(counts, vec![0.0, 16.0, 32.0]);
        assert!(approx(map.bpm_at(16.0), 100.0));
    }

    #[test]
    fn negative_counts_and_seconds_clamp_to_zero() {
        let map = TempoMap::constant(120.0);
        assert!(approx(map.seconds_at(-5.0), 0.0));
        assert!(approx(map.count_at(-5.0), 0.0));
    }

    #[test]
    fn measure_beat_mapping() {
        let map = TempoMap::default();
        assert_eq!(map.measure_beat(0.0, 4), (1, 1.0));
        assert_eq!(map.measure_beat(4.0, 4), (2, 1.0));
        assert_eq!(map.measure_beat(5.0, 4), (2, 2.0));
        // counts 4,5,6,7 -> beats 1..4 of measure 2, so 7.5 -> beat 4.5.
        let (measure, beat) = map.measure_beat(7.5, 4);
        assert_eq!(measure, 2);
        assert!(approx(beat, 4.5));
        // beats_per_measure of 0 is clamped to 1.
        assert_eq!(map.measure_beat(3.0, 0), (4, 1.0));
    }

    #[test]
    fn json_round_trip() {
        let mut map = TempoMap::constant(120.0);
        map.set(16.0, 60.0);
        let json = serde_json::to_string(&map).unwrap();
        let back: TempoMap = serde_json::from_str(&json).unwrap();
        assert_eq!(back.events(), map.events());
    }
}
