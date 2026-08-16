//! Tempo map: variable BPM over the count timeline, and count<->seconds<->measure
//! conversion. Pure logic, no UI.
//!
//! A show is measured in *counts* (usually 1 count = 1 beat). The tempo (BPM)
//! may change over the show, so the mapping between counts and wall-clock time
//! is piecewise-constant: each [`TempoChange`] anchors a BPM at a global count
//! and stays in effect until the next change. Time for a span of `N` counts at
//! `B` BPM is `N * 60 / B` seconds.

use serde::{Deserialize, Deserializer, Serialize};
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
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TempoMap {
    events: Vec<TempoChange>,
    /// Elapsed seconds at the effective start of each event. This derived
    /// cache is deliberately omitted from the persistent representation.
    #[serde(skip)]
    prefix_seconds: Vec<f64>,
}

impl<'de> Deserialize<'de> for TempoMap {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct WireTempoMap {
            events: Vec<TempoChange>,
        }

        let wire = WireTempoMap::deserialize(deserializer)?;
        Ok(Self::from_changes(wire.events))
    }
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
            prefix_seconds: vec![0.0],
        }
    }

    /// Build from arbitrary changes; they are sorted and deduplicated (a later
    /// duplicate count wins). An empty input yields an empty map.
    pub fn from_changes(changes: impl IntoIterator<Item = TempoChange>) -> Self {
        let mut indexed = changes
            .into_iter()
            .enumerate()
            .map(|(ordinal, mut change)| {
                change.count = change.count.max(0.0);
                (ordinal, change)
            })
            .collect::<Vec<_>>();
        indexed.sort_by(|a, b| {
            a.1.count
                .partial_cmp(&b.1.count)
                .unwrap_or(Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        let mut events: Vec<TempoChange> = Vec::with_capacity(indexed.len());
        for (_, change) in indexed {
            if let Some(last) = events.last_mut()
                && last.count == change.count
            {
                *last = change;
            } else {
                events.push(change);
            }
        }
        let mut map = Self {
            events,
            prefix_seconds: Vec::new(),
        };
        map.rebuild_prefix();
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
        self.rebuild_prefix();
        self
    }

    /// Remove the change anchored exactly at `count`, if any.
    pub fn remove(&mut self, count: f32) {
        self.events.retain(|e| e.count != count.max(0.0));
        self.rebuild_prefix();
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
        self.seconds_at_f64(f64::from(global_count)) as f32
    }

    /// Sample-accurate count-to-time conversion using a cached prefix sum and
    /// binary search. The persistent tempo events remain `f32` for schema
    /// compatibility, while all timeline arithmetic is performed in `f64`.
    pub fn seconds_at_f64(&self, global_count: f64) -> f64 {
        let target = finite_non_negative_f64(global_count);
        let Some(index) = self.event_index_at_count(target) else {
            return target * 60.0 / f64::from(DEFAULT_BPM);
        };
        let start = effective_start(&self.events, index);
        self.prefix_seconds[index]
            + (target - start) * 60.0 / f64::from(sanitize_bpm(self.events[index].bpm))
    }

    /// Inverse of [`seconds_at`](Self::seconds_at): the global count reached
    /// after `seconds` of real time from count 0. Seconds ≤ 0 map to count 0.
    pub fn count_at(&self, seconds: f32) -> f32 {
        self.count_at_f64(f64::from(seconds)) as f32
    }

    /// Inverse of [`seconds_at_f64`](Self::seconds_at_f64), also O(log n).
    pub fn count_at_f64(&self, seconds: f64) -> f64 {
        let target = finite_non_negative_f64(seconds);
        if self.events.is_empty() {
            return target * f64::from(DEFAULT_BPM) / 60.0;
        }
        let index = self
            .prefix_seconds
            .partition_point(|&start_seconds| start_seconds <= target)
            .saturating_sub(1);
        let start = effective_start(&self.events, index);
        start
            + (target - self.prefix_seconds[index])
                * f64::from(sanitize_bpm(self.events[index].bpm))
                / 60.0
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

    fn event_index_at_count(&self, target: f64) -> Option<usize> {
        if self.events.is_empty() {
            return None;
        }
        Some(
            self.events
                .partition_point(|event| f64::from(segment_start(event.count)) <= target)
                .saturating_sub(1),
        )
    }

    fn rebuild_prefix(&mut self) {
        self.prefix_seconds.clear();
        self.prefix_seconds.reserve(self.events.len());
        if self.events.is_empty() {
            return;
        }
        self.prefix_seconds.push(0.0);
        for index in 1..self.events.len() {
            let previous_start = effective_start(&self.events, index - 1);
            let start = effective_start(&self.events, index);
            let elapsed = (start - previous_start) * 60.0
                / f64::from(sanitize_bpm(self.events[index - 1].bpm));
            self.prefix_seconds
                .push(self.prefix_seconds[index - 1] + elapsed);
        }
    }
}

fn effective_start(events: &[TempoChange], index: usize) -> f64 {
    if index == 0 {
        0.0
    } else {
        f64::from(segment_start(events[index].count))
    }
}

fn finite_non_negative_f64(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
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
        assert!(approx(back.seconds_at(32.0), map.seconds_at(32.0)));
    }

    #[test]
    fn f64_mapping_stays_within_half_a_sample_over_two_hours() {
        let mut map = TempoMap::constant(137.0);
        for count in (64..16_000).step_by(64) {
            map.set(count as f32, 80.0 + (count % 113) as f32);
        }
        let count = map.count_at_f64(2.0 * 60.0 * 60.0);
        let seconds = map.seconds_at_f64(count);
        assert!((seconds - 7_200.0).abs() <= 0.5 / 48_000.0);
    }

    #[test]
    fn f64_boundary_uses_new_tempo() {
        let mut map = TempoMap::constant(120.0);
        map.set(16.0, 60.0);
        let boundary = map.seconds_at_f64(16.0);
        let sample = 1.0 / 48_000.0;
        assert!(map.count_at_f64(boundary - sample) < 16.0);
        assert!(map.count_at_f64(boundary + sample) > 16.0);
    }
}
