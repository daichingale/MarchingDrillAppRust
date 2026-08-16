//! Reference-audio sync model: how a recorded track lines up with the count
//! timeline. Pure logic, no playback — the app layer drives an actual audio
//! device using the mappings computed here.
//!
//! The show timeline is measured in *counts*; a [`crate::tempo::TempoMap`]
//! converts counts to show-relative seconds (0 s at count 0). An
//! [`AudioTrack`] adds a single [`offset_seconds`](AudioTrack::offset_seconds)
//! so that show time can be translated to a playback position within the file.

use crate::tempo::{TempoChange, TempoMap};
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;

pub const MAX_SYNC_ANCHORS: usize = 4_096;
pub const TEMPO_MISMATCH_TOLERANCE: f64 = 0.005;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AnchorId(u64);

impl AnchorId {
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SyncAnchor {
    pub count: f64,
    pub seconds: f64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnchorError {
    TooMany,
    NonFinite,
    Negative,
    DuplicateCount,
    DuplicateId,
    MissingId,
    NonMonotonic,
    InvalidSampleRate,
    SampleOutOfRange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleIndex(pub i64);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TempoMismatch {
    pub segment: usize,
    pub start_count: f64,
    pub end_count: f64,
    pub anchor_bpm: f64,
    pub tempo_bpm: f64,
    pub drift_seconds: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnchorResidual {
    pub anchor_index: usize,
    pub milliseconds: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TempoProposal {
    pub changes: Vec<TempoChange>,
    pub residuals: Vec<AnchorResidual>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct AnchorMap {
    anchors: Vec<SyncAnchor>,
    #[serde(skip)]
    ids: Vec<AnchorId>,
    #[serde(skip)]
    next_id: u64,
}

impl<'de> Deserialize<'de> for AnchorMap {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct WireAnchorMap {
            anchors: Vec<SyncAnchor>,
        }
        let wire = WireAnchorMap::deserialize(deserializer)?;
        Self::try_from_anchors(wire.anchors).map_err(serde::de::Error::custom)
    }
}

impl std::fmt::Display for AnchorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl AnchorMap {
    pub fn try_from_anchors(
        anchors: impl IntoIterator<Item = SyncAnchor>,
    ) -> Result<Self, AnchorError> {
        let mut anchors = anchors.into_iter().collect::<Vec<_>>();
        if anchors.len() > MAX_SYNC_ANCHORS {
            return Err(AnchorError::TooMany);
        }
        if anchors
            .iter()
            .any(|a| !a.count.is_finite() || !a.seconds.is_finite())
        {
            return Err(AnchorError::NonFinite);
        }
        if anchors.iter().any(|a| a.count < 0.0 || a.seconds < 0.0) {
            return Err(AnchorError::Negative);
        }
        anchors.sort_by(|a, b| a.count.total_cmp(&b.count));
        for pair in anchors.windows(2) {
            if pair[0].count == pair[1].count {
                return Err(AnchorError::DuplicateCount);
            }
            if pair[0].seconds >= pair[1].seconds {
                return Err(AnchorError::NonMonotonic);
            }
        }
        let ids = (1..=anchors.len() as u64).map(AnchorId).collect();
        Ok(Self {
            next_id: anchors.len() as u64 + 1,
            anchors,
            ids,
        })
    }

    pub fn from_offset(offset_seconds: f64) -> Result<Self, AnchorError> {
        Self::try_from_anchors([SyncAnchor {
            count: 0.0,
            seconds: offset_seconds,
        }])
    }

    pub fn anchors(&self) -> &[SyncAnchor] {
        &self.anchors
    }

    pub fn id_at(&self, index: usize) -> Option<AnchorId> {
        self.ids.get(index).copied()
    }

    pub fn get(&self, id: AnchorId) -> Option<SyncAnchor> {
        self.ids
            .iter()
            .position(|candidate| *candidate == id)
            .and_then(|index| self.anchors.get(index).copied())
    }

    pub fn is_empty(&self) -> bool {
        self.anchors.is_empty()
    }

    pub fn validate(&self) -> Result<(), AnchorError> {
        Self::try_from_anchors(self.anchors.iter().copied()).map(|_| ())
    }

    pub fn set(&mut self, anchor: SyncAnchor) -> Result<usize, AnchorError> {
        self.add(anchor).map(|id| {
            self.ids
                .iter()
                .position(|candidate| *candidate == id)
                .expect("new anchor id must exist")
        })
    }

    pub fn remove(&mut self, index: usize) -> Option<SyncAnchor> {
        if index >= self.anchors.len() {
            return None;
        }
        self.ids.remove(index);
        Some(self.anchors.remove(index))
    }

    pub fn add(&mut self, anchor: SyncAnchor) -> Result<AnchorId, AnchorError> {
        let id = AnchorId(self.next_id.max(1));
        self.add_with_id(id, anchor)?;
        Ok(id)
    }

    pub fn add_with_id(&mut self, id: AnchorId, anchor: SyncAnchor) -> Result<(), AnchorError> {
        if id.0 == 0 || self.ids.contains(&id) {
            return Err(AnchorError::DuplicateId);
        }
        let index = self
            .anchors
            .binary_search_by(|candidate| candidate.count.total_cmp(&anchor.count))
            .unwrap_or_else(|index| index);
        let mut anchors = self.anchors.clone();
        anchors.insert(index, anchor);
        Self::try_from_anchors(anchors)?;
        self.anchors.insert(index, anchor);
        self.ids.insert(index, id);
        self.next_id = self.next_id.max(id.0.saturating_add(1));
        Ok(())
    }

    pub fn move_anchor(
        &mut self,
        id: AnchorId,
        anchor: SyncAnchor,
    ) -> Result<SyncAnchor, AnchorError> {
        let index = self
            .ids
            .iter()
            .position(|candidate| *candidate == id)
            .ok_or(AnchorError::MissingId)?;
        let previous = self.anchors[index];
        let mut candidate = self.clone();
        candidate.remove(index);
        candidate.add_with_id(id, anchor)?;
        *self = candidate;
        Ok(previous)
    }

    pub fn remove_id(&mut self, id: AnchorId) -> Option<SyncAnchor> {
        let index = self.ids.iter().position(|candidate| *candidate == id)?;
        self.remove(index)
    }

    pub fn nearest_anchor(&self, count: f64, max_distance: f64) -> Option<(AnchorId, SyncAnchor)> {
        if !count.is_finite() || !max_distance.is_finite() || max_distance < 0.0 {
            return None;
        }
        self.anchors
            .iter()
            .copied()
            .zip(self.ids.iter().copied())
            .min_by(|(a, _), (b, _)| (a.count - count).abs().total_cmp(&(b.count - count).abs()))
            .filter(|(anchor, _)| (anchor.count - count).abs() <= max_distance)
            .map(|(anchor, id)| (id, anchor))
    }

    pub fn snap_count(&self, count: f64, tolerance: f64) -> f64 {
        self.nearest_anchor(count, tolerance)
            .map_or(count, |(_, anchor)| anchor.count)
    }

    pub fn warning_summary(&self, tempo: &TempoMap) -> Option<(usize, f64)> {
        let mut mismatches = Vec::new();
        self.mismatches(tempo, &mut mismatches);
        (!mismatches.is_empty()).then(|| {
            let max_ms = mismatches
                .iter()
                .map(|item| item.drift_seconds.abs() * 1_000.0)
                .fold(0.0, f64::max);
            (mismatches.len(), max_ms)
        })
    }

    pub fn file_seconds_at(&self, count: f64, tempo: &TempoMap) -> f64 {
        let count = finite_non_negative_f64(count);
        match self.anchors.as_slice() {
            [] => tempo.seconds_at_f64(count),
            [anchor] => {
                anchor.seconds + tempo.seconds_at_f64(count) - tempo.seconds_at_f64(anchor.count)
            }
            anchors => {
                let index = anchors
                    .partition_point(|anchor| anchor.count <= count)
                    .saturating_sub(1)
                    .min(anchors.len() - 2);
                interpolate(
                    count,
                    anchors[index].count,
                    anchors[index + 1].count,
                    anchors[index].seconds,
                    anchors[index + 1].seconds,
                )
            }
        }
    }

    pub fn count_at(&self, file_seconds: f64, tempo: &TempoMap) -> f64 {
        let seconds = finite_non_negative_f64(file_seconds);
        match self.anchors.as_slice() {
            [] => tempo.count_at_f64(seconds),
            [anchor] => {
                tempo.count_at_f64(tempo.seconds_at_f64(anchor.count) + seconds - anchor.seconds)
            }
            anchors => {
                let index = anchors
                    .partition_point(|anchor| anchor.seconds <= seconds)
                    .saturating_sub(1)
                    .min(anchors.len() - 2);
                interpolate(
                    seconds,
                    anchors[index].seconds,
                    anchors[index + 1].seconds,
                    anchors[index].count,
                    anchors[index + 1].count,
                )
            }
        }
    }

    pub fn sample_at(
        &self,
        count: f64,
        tempo: &TempoMap,
        sample_rate: u32,
    ) -> Result<SampleIndex, AnchorError> {
        if sample_rate == 0 {
            return Err(AnchorError::InvalidSampleRate);
        }
        let sample = self.file_seconds_at(count, tempo) * f64::from(sample_rate);
        if !sample.is_finite() || sample < i64::MIN as f64 || sample > i64::MAX as f64 {
            return Err(AnchorError::SampleOutOfRange);
        }
        Ok(SampleIndex(sample.round() as i64))
    }

    pub fn count_at_sample(
        &self,
        sample: SampleIndex,
        tempo: &TempoMap,
        sample_rate: u32,
    ) -> Result<f64, AnchorError> {
        if sample_rate == 0 {
            return Err(AnchorError::InvalidSampleRate);
        }
        Ok(self.count_at(sample.0 as f64 / f64::from(sample_rate), tempo))
    }

    pub fn implied_bpm(&self, index: usize) -> Option<f64> {
        let pair = self.anchors.get(index..index.checked_add(2)?)?;
        let seconds = pair[1].seconds - pair[0].seconds;
        (seconds > 0.0).then(|| 60.0 * (pair[1].count - pair[0].count) / seconds)
    }

    pub fn mismatches(&self, tempo: &TempoMap, out: &mut Vec<TempoMismatch>) {
        out.clear();
        for (index, pair) in self.anchors.windows(2).enumerate() {
            let Some(anchor_bpm) = self.implied_bpm(index) else {
                continue;
            };
            let tempo_seconds =
                tempo.seconds_at_f64(pair[1].count) - tempo.seconds_at_f64(pair[0].count);
            let tempo_bpm = 60.0 * (pair[1].count - pair[0].count) / tempo_seconds;
            let drift_seconds = (pair[1].seconds - pair[0].seconds) - tempo_seconds;
            if ((anchor_bpm - tempo_bpm) / tempo_bpm).abs() > TEMPO_MISMATCH_TOLERANCE {
                out.push(TempoMismatch {
                    segment: index,
                    start_count: pair[0].count,
                    end_count: pair[1].count,
                    anchor_bpm,
                    tempo_bpm,
                    drift_seconds,
                });
            }
        }
    }

    pub fn tempo_proposal(&self, tempo: &TempoMap) -> TempoProposal {
        let mut changes = Vec::with_capacity(self.anchors.len().saturating_sub(1));
        for (index, anchor) in self
            .anchors
            .iter()
            .enumerate()
            .take(self.anchors.len().saturating_sub(1))
        {
            if let Some(bpm) = self.implied_bpm(index) {
                changes.push(TempoChange {
                    count: anchor.count as f32,
                    bpm: bpm as f32,
                });
            }
        }
        let residuals = self
            .anchors
            .iter()
            .enumerate()
            .map(|(anchor_index, anchor)| AnchorResidual {
                anchor_index,
                milliseconds: (anchor.seconds - tempo.seconds_at_f64(anchor.count)) * 1_000.0,
            })
            .collect();
        TempoProposal { changes, residuals }
    }
}

fn interpolate(value: f64, x0: f64, x1: f64, y0: f64, y1: f64) -> f64 {
    y0 + (value - x0) * (y1 - y0) / (x1 - x0)
}

fn finite_non_negative_f64(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

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
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AudioTrack {
    /// Path to the audio file (interpreted by the app layer).
    pub path: String,
    /// Total length of the audio file in seconds.
    pub duration_seconds: f32,
    /// Playback position (seconds into the file) aligned to global count 0.
    /// Positive = show starts inside the file; negative = count 0 precedes it.
    pub offset_seconds: f32,
    /// v2 multi-point synchronization. Empty preserves the legacy offset.
    #[serde(default)]
    pub anchors: AnchorMap,
    #[serde(default)]
    pub gain_db: f32,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub trim_start_seconds: f32,
    /// Zero means the physical end of the source file.
    #[serde(default)]
    pub trim_end_seconds: f32,
    #[serde(default)]
    pub fade_in_seconds: f32,
    #[serde(default)]
    pub fade_out_seconds: f32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AudioTrackError {
    InvalidAnchors(AnchorError),
    InvalidDuration,
    InvalidOffset,
    InvalidGain,
    InvalidTrimStart,
    InvalidTrimEnd,
    InvalidFade,
    FadeExceedsDuration,
}

impl AudioTrackError {
    pub fn message(&self, locale: crate::Locale) -> &'static str {
        use crate::Locale::{En, Ja};
        match (self, locale) {
            (Self::InvalidAnchors(_), Ja) => "音源の同期アンカーが不正です",
            (Self::InvalidAnchors(_), En) => "The audio sync anchors are invalid",
            (Self::InvalidDuration, Ja) => "音源の長さが不正です",
            (Self::InvalidDuration, En) => "The audio duration is invalid",
            (Self::InvalidOffset, Ja) => "音源の同期位置が不正です",
            (Self::InvalidOffset, En) => "The audio sync position is invalid",
            (Self::InvalidGain, Ja) => "音量が不正です",
            (Self::InvalidGain, En) => "The audio gain is invalid",
            (Self::InvalidTrimStart, Ja) => "トリム開始位置が音源範囲外です",
            (Self::InvalidTrimStart, En) => "The trim start is outside the audio",
            (Self::InvalidTrimEnd, Ja) => "トリム終了位置が音源範囲外です",
            (Self::InvalidTrimEnd, En) => "The trim end is outside the audio",
            (Self::InvalidFade, Ja) => "フェード時間は0以上にしてください",
            (Self::InvalidFade, En) => "Fade durations must be non-negative",
            (Self::FadeExceedsDuration, Ja) => "フェード時間の合計が音源の有効時間を超えています",
            (Self::FadeExceedsDuration, En) => {
                "The combined fades exceed the effective audio duration"
            }
        }
    }
}

impl fmt::Display for AudioTrackError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message(crate::Locale::En))
    }
}

impl std::error::Error for AudioTrackError {}

impl AudioTrack {
    pub fn gain_linear(&self) -> f32 {
        if self.muted {
            0.0
        } else {
            10.0_f32.powf(self.gain_db.clamp(-96.0, 24.0) / 20.0)
        }
    }

    pub fn effective_duration(&self) -> f32 {
        let end = if self.trim_end_seconds > 0.0 {
            self.trim_end_seconds.min(self.duration_seconds)
        } else {
            self.duration_seconds
        };
        (end - self.trim_start_seconds.max(0.0)).max(0.0)
    }

    pub fn validate(&self) -> Result<(), AudioTrackError> {
        self.anchors
            .validate()
            .map_err(AudioTrackError::InvalidAnchors)?;
        if !self.duration_seconds.is_finite() || self.duration_seconds < 0.0 {
            return Err(AudioTrackError::InvalidDuration);
        }
        if !self.offset_seconds.is_finite() {
            return Err(AudioTrackError::InvalidOffset);
        }
        if !self.gain_db.is_finite() {
            return Err(AudioTrackError::InvalidGain);
        }
        if !self.trim_start_seconds.is_finite()
            || self.trim_start_seconds < 0.0
            || self.trim_start_seconds > self.duration_seconds
        {
            return Err(AudioTrackError::InvalidTrimStart);
        }
        if !self.trim_end_seconds.is_finite()
            || self.trim_end_seconds < 0.0
            || self.trim_end_seconds > self.duration_seconds
            || (self.trim_end_seconds > 0.0 && self.trim_end_seconds < self.trim_start_seconds)
        {
            return Err(AudioTrackError::InvalidTrimEnd);
        }
        if !self.fade_in_seconds.is_finite()
            || !self.fade_out_seconds.is_finite()
            || self.fade_in_seconds < 0.0
            || self.fade_out_seconds < 0.0
        {
            return Err(AudioTrackError::InvalidFade);
        }
        if self.fade_in_seconds + self.fade_out_seconds > self.effective_duration() {
            return Err(AudioTrackError::FadeExceedsDuration);
        }
        Ok(())
    }
}

/// Playback position (seconds into the file) for a given global count.
///
/// This is `offset_seconds + tempo.seconds_at(global_count)` and may be
/// negative when the offset is negative and the count is small.
pub fn count_to_audio_time(track: &AudioTrack, tempo: &TempoMap, global_count: f32) -> f32 {
    if track.anchors.is_empty() {
        track.offset_seconds + tempo.seconds_at(global_count)
    } else {
        track
            .anchors
            .file_seconds_at(f64::from(global_count), tempo) as f32
    }
}

/// Global count for a given playback position (seconds into the file).
///
/// Inverse of [`count_to_audio_time`]. Audio times before the show's count 0
/// clamp to count 0 (via [`TempoMap::count_at`], whose input is clamped).
pub fn audio_time_to_count(track: &AudioTrack, tempo: &TempoMap, audio_seconds: f32) -> f32 {
    if track.anchors.is_empty() {
        tempo.count_at(audio_seconds - track.offset_seconds)
    } else {
        track.anchors.count_at(f64::from(audio_seconds), tempo) as f32
    }
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

    #[test]
    fn track_errors_are_machine_readable_and_localized() {
        let track = AudioTrack {
            duration_seconds: -1.0,
            ..AudioTrack::default()
        };
        let error = track.validate().unwrap_err();
        assert_eq!(error, AudioTrackError::InvalidDuration);
        assert_eq!(
            error.message(crate::Locale::En),
            "The audio duration is invalid"
        );
        assert_ne!(
            error.message(crate::Locale::Ja),
            error.message(crate::Locale::En)
        );
    }

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
            anchors: AnchorMap::default(),
            gain_db: 0.0,
            muted: false,
            trim_start_seconds: 0.0,
            trim_end_seconds: 0.0,
            fade_in_seconds: 0.0,
            fade_out_seconds: 0.0,
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

    #[test]
    fn non_destructive_adjustments_are_validated() {
        let mut track = make_track(0.0, 10.0);
        track.trim_start_seconds = 2.0;
        track.trim_end_seconds = 8.0;
        track.gain_db = -6.0;
        assert!(track.validate().is_ok());
        assert!((track.effective_duration() - 6.0).abs() < 1e-3);
        assert!((track.gain_linear() - 0.501).abs() < 0.01);
        track.muted = true;
        assert_eq!(track.gain_linear(), 0.0);
    }

    #[test]
    fn rejects_non_finite_adjustments() {
        for mutate in [
            |track: &mut AudioTrack| track.duration_seconds = f32::NAN,
            |track: &mut AudioTrack| track.offset_seconds = f32::INFINITY,
            |track: &mut AudioTrack| track.gain_db = f32::NAN,
            |track: &mut AudioTrack| track.trim_start_seconds = f32::NAN,
            |track: &mut AudioTrack| track.trim_end_seconds = f32::INFINITY,
            |track: &mut AudioTrack| track.fade_in_seconds = f32::INFINITY,
            |track: &mut AudioTrack| track.fade_out_seconds = f32::NAN,
        ] {
            let mut track = make_track(0.0, 10.0);
            mutate(&mut track);
            assert!(track.validate().is_err());
        }
    }

    #[test]
    fn rejects_trim_and_fade_outside_effective_audio() {
        let mut track = make_track(0.0, 10.0);
        track.trim_end_seconds = 11.0;
        assert!(track.validate().is_err());

        track.trim_end_seconds = 8.0;
        track.trim_start_seconds = 2.0;
        track.fade_in_seconds = 3.1;
        track.fade_out_seconds = 3.0;
        assert!(track.validate().is_err());
    }

    #[test]
    fn anchor_map_round_trips_within_half_a_sample() {
        let tempo = TempoMap::from_changes([
            TempoChange {
                count: 0.0,
                bpm: 120.0,
            },
            TempoChange {
                count: 64.0,
                bpm: 90.0,
            },
        ]);
        let anchors = AnchorMap::try_from_anchors([
            SyncAnchor {
                count: 0.0,
                seconds: 1.25,
            },
            SyncAnchor {
                count: 64.0,
                seconds: 33.25,
            },
            SyncAnchor {
                count: 128.0,
                seconds: 75.916_666_666_7,
            },
        ])
        .unwrap();
        for count in [0.0, 7.25, 63.999, 64.0, 91.5, 128.0] {
            let sample = anchors.sample_at(count, &tempo, 48_000).unwrap();
            let back = anchors.count_at_sample(sample, &tempo, 48_000).unwrap();
            let sample_error = (anchors.file_seconds_at(back, &tempo)
                - anchors.file_seconds_at(count, &tempo))
            .abs()
                * 48_000.0;
            assert!(sample_error <= 0.5 + 1e-9, "error={sample_error}");
        }
    }

    #[test]
    fn single_anchor_preserves_tempo_and_legacy_offset_semantics() {
        let tempo = TempoMap::from_changes([
            TempoChange {
                count: 0.0,
                bpm: 120.0,
            },
            TempoChange {
                count: 16.0,
                bpm: 60.0,
            },
        ]);
        let anchors = AnchorMap::from_offset(2.0).unwrap();
        for count in [0.0, 8.0, 16.0, 24.0] {
            assert!(
                (anchors.file_seconds_at(count, &tempo) - (2.0 + tempo.seconds_at_f64(count)))
                    .abs()
                    < 1e-12
            );
        }
    }

    #[test]
    fn rejects_invalid_duplicate_and_non_monotonic_anchors() {
        assert_eq!(
            AnchorMap::try_from_anchors([SyncAnchor {
                count: f64::NAN,
                seconds: 0.0
            }]),
            Err(AnchorError::NonFinite)
        );
        assert_eq!(
            AnchorMap::try_from_anchors([
                SyncAnchor {
                    count: 4.0,
                    seconds: 1.0
                },
                SyncAnchor {
                    count: 4.0,
                    seconds: 2.0
                },
            ]),
            Err(AnchorError::DuplicateCount)
        );
        assert_eq!(
            AnchorMap::try_from_anchors([
                SyncAnchor {
                    count: 4.0,
                    seconds: 2.0
                },
                SyncAnchor {
                    count: 8.0,
                    seconds: 1.0
                },
            ]),
            Err(AnchorError::NonMonotonic)
        );
    }

    #[test]
    fn mismatches_and_tempo_proposal_are_explicit() {
        let tempo = TempoMap::constant(120.0);
        let anchors = AnchorMap::try_from_anchors([
            SyncAnchor {
                count: 0.0,
                seconds: 0.0,
            },
            SyncAnchor {
                count: 16.0,
                seconds: 10.0,
            },
        ])
        .unwrap();
        let mut mismatches = Vec::new();
        anchors.mismatches(&tempo, &mut mismatches);
        assert_eq!(mismatches.len(), 1);
        assert!((mismatches[0].drift_seconds - 2.0).abs() < 1e-12);
        let proposal = anchors.tempo_proposal(&tempo);
        assert_eq!(proposal.changes.len(), 1);
        assert!((proposal.changes[0].bpm - 96.0).abs() < 1e-6);
        assert!((proposal.residuals[1].milliseconds - 2_000.0).abs() < 1e-9);
    }

    #[test]
    fn legacy_audio_json_without_anchors_remains_compatible() {
        let track = make_track(1.5, 42.0);
        let mut value = serde_json::to_value(track).unwrap();
        value.as_object_mut().unwrap().remove("anchors");
        let loaded: AudioTrack = serde_json::from_value(value).unwrap();
        assert!(loaded.anchors.is_empty());
        assert!(approx(count_to_audio_time(&loaded, &tempo(), 4.0), 3.5));
    }

    #[test]
    fn nearest_snap_and_warning_diagnostics_are_bounded() {
        let anchors = AnchorMap::try_from_anchors([
            SyncAnchor {
                count: 0.0,
                seconds: 0.0,
            },
            SyncAnchor {
                count: 8.0,
                seconds: 5.0,
            },
        ])
        .unwrap();
        let (id, nearest) = anchors.nearest_anchor(7.8, 0.25).unwrap();
        assert_eq!(anchors.get(id), Some(nearest));
        assert_eq!(anchors.snap_count(7.8, 0.25), 8.0);
        assert_eq!(anchors.snap_count(7.0, 0.25), 7.0);
        let (segments, max_ms) = anchors.warning_summary(&TempoMap::constant(120.0)).unwrap();
        assert_eq!(segments, 1);
        assert!((max_ms - 1_000.0).abs() < 1e-9);
    }

    #[test]
    fn anchor_ids_rebuild_deterministically_from_legacy_json() {
        let anchors = AnchorMap::try_from_anchors([
            SyncAnchor {
                count: 0.0,
                seconds: 0.0,
            },
            SyncAnchor {
                count: 8.0,
                seconds: 4.0,
            },
        ])
        .unwrap();
        let json = serde_json::to_string(&anchors).unwrap();
        assert!(!json.contains("ids"));
        let loaded: AnchorMap = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.id_at(0).unwrap().get(), 1);
        assert_eq!(loaded.id_at(1).unwrap().get(), 2);
    }
}
