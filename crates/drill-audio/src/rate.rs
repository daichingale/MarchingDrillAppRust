use crate::{AudioAsset, MixerState};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaybackRate(f32);

impl PlaybackRate {
    pub const MIN: f32 = 0.25;
    pub const MAX: f32 = 4.0;
    pub const NORMAL: Self = Self(1.0);

    pub fn new(value: f32) -> Result<Self, RateError> {
        (value.is_finite() && (Self::MIN..=Self::MAX).contains(&value))
            .then_some(Self(value))
            .ok_or(RateError::InvalidRate)
    }

    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }
}

impl Default for PlaybackRate {
    fn default() -> Self {
        Self::NORMAL
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PitchQuality {
    Draft,
    Standard,
    High,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateMode {
    Resample,
    PreservePitch(PitchQuality),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoopRange {
    pub start: u64,
    pub end: u64,
}

impl LoopRange {
    #[must_use]
    pub const fn is_valid(self) -> bool {
        self.start < self.end
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateError {
    InvalidRate,
    InvalidChannels,
}

impl fmt::Display for RateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidRate => "playback rate must be finite and in 0.25..=4.0",
            Self::InvalidChannels => "pitch-preserving processor supports 1 or 2 channels",
        })
    }
}
impl std::error::Error for RateError {}

#[allow(clippy::too_many_arguments)]
pub fn render_block_rate_linear(
    asset: &AudioAsset,
    output_rate: u32,
    output_channels: usize,
    rate: PlaybackRate,
    loop_range: Option<LoopRange>,
    gain: f32,
    muted: bool,
    state: &mut MixerState,
    out: &mut [f32],
) -> usize {
    if output_rate == 0 || output_channels == 0 {
        out.fill(0.0);
        return 0;
    }
    let frames = out.len() / output_channels;
    let source_channels = usize::from(asset.channels());
    let step = ((f64::from(asset.sample_rate()) / f64::from(output_rate))
        * f64::from(rate.get())
        * (1_u64 << 32) as f64)
        .round()
        .clamp(1.0, u64::MAX as f64) as u64;
    for frame in 0..frames {
        normalize_loop(&mut state.source_position_q32, loop_range);
        let base = state.source_position_q32 >> 32;
        let fraction = (state.source_position_q32 & 0xffff_ffff) as f32 / (1_u64 << 32) as f32;
        let next = loop_range
            .filter(|r| r.is_valid() && base + 1 >= r.end)
            .map_or(base.saturating_add(1), |r| r.start);
        let a = asset.frames_range(base, 1);
        let b = asset.frames_range(next, 1);
        for channel in 0..output_channels {
            let value = if muted || a.is_empty() {
                0.0
            } else {
                let c = channel.min(source_channels - 1);
                let first = f32::from(a[c]) / 32_768.0;
                let second = b.get(c).map_or(first, |s| f32::from(*s) / 32_768.0);
                (first + (second - first) * fraction) * gain
            };
            out[frame * output_channels + channel] = value.clamp(-1.0, 1.0);
        }
        state.source_position_q32 = state.source_position_q32.saturating_add(step);
    }
    state.output_position = state.output_position.saturating_add(frames as i64);
    frames
}

fn normalize_loop(position: &mut u64, range: Option<LoopRange>) {
    let Some(range) = range.filter(|r| r.is_valid()) else {
        return;
    };
    let frame = *position >> 32;
    if frame >= range.end {
        let wrapped = range.start + (frame.saturating_sub(range.start)) % (range.end - range.start);
        *position = (wrapped << 32) | (*position & 0xffff_ffff);
    }
}

/// WSOLA-style overlap-add. Scratch is fully allocated in `new`; `render` is allocation-free.
pub struct RateProcessor {
    quality: PitchQuality,
    channels: usize,
    window: usize,
    search: usize,
    tail: Vec<f32>,
    chunk: Vec<f32>,
    cursor: usize,
    initialized: bool,
    source: f64,
    output: i64,
    last_rate: PlaybackRate,
}

impl RateProcessor {
    pub fn new(channels: usize, quality: PitchQuality) -> Result<Self, RateError> {
        if !(1..=2).contains(&channels) {
            return Err(RateError::InvalidChannels);
        }
        let (window, search) = match quality {
            PitchQuality::Draft => (256, 32),
            PitchQuality::Standard => (1_024, 128),
            PitchQuality::High => (2_048, 256),
        };
        let size = window / 2 * channels;
        Ok(Self {
            quality,
            channels,
            window,
            search,
            tail: vec![0.0; size],
            chunk: vec![0.0; size],
            cursor: size,
            initialized: false,
            source: 0.0,
            output: 0,
            last_rate: PlaybackRate::NORMAL,
        })
    }

    #[must_use]
    pub const fn quality(&self) -> PitchQuality {
        self.quality
    }
    #[must_use]
    pub fn source_frame(&self) -> u64 {
        self.source.max(0.0) as u64
    }
    #[must_use]
    pub const fn output_position(&self) -> i64 {
        self.output
    }

    pub fn seek(&mut self, source: u64, output: i64) {
        self.source = source as f64;
        self.output = output;
        self.initialized = false;
        self.cursor = self.chunk.len();
        self.tail.fill(0.0);
    }

    pub fn render(
        &mut self,
        asset: &AudioAsset,
        rate: PlaybackRate,
        loop_range: Option<LoopRange>,
        gain: f32,
        muted: bool,
        out: &mut [f32],
    ) -> usize {
        if (rate.get() - self.last_rate.get()).abs() > 0.25 {
            self.seek(self.source_frame(), self.output);
        }
        self.last_rate = rate;
        let target = out.len() / self.channels * self.channels;
        let mut written = 0;
        while written < target {
            if self.cursor == self.chunk.len() {
                self.generate(asset, rate, loop_range);
            }
            let count = (self.chunk.len() - self.cursor).min(target - written);
            for i in 0..count {
                out[written + i] = if muted {
                    0.0
                } else {
                    (self.chunk[self.cursor + i] * gain).clamp(-1.0, 1.0)
                };
            }
            self.cursor += count;
            written += count;
        }
        out[target..].fill(0.0);
        let frames = target / self.channels;
        self.output = self.output.saturating_add(frames as i64);
        frames
    }

    fn generate(&mut self, asset: &AudioAsset, rate: PlaybackRate, range: Option<LoopRange>) {
        let hop = self.window / 2;
        let expected = wrap(self.source.round().max(0.0) as u64, range);
        let selected = if self.initialized {
            self.best_match(asset, expected, range)
        } else {
            expected
        };
        for frame in 0..hop {
            let fade = frame as f32 / hop as f32;
            for channel in 0..self.channels {
                let i = frame * self.channels + channel;
                let incoming = sample(asset, selected + frame as u64, channel, range);
                self.chunk[i] = if self.initialized {
                    self.tail[i] * (1.0 - fade) + incoming * fade
                } else {
                    incoming
                };
                self.tail[i] = sample(asset, selected + hop as u64 + frame as u64, channel, range);
            }
        }
        self.initialized = true;
        self.cursor = 0;
        self.source = selected as f64 + hop as f64 * f64::from(rate.get());
        if let Some(r) = range.filter(|r| r.is_valid())
            && self.source >= r.end as f64
        {
            self.source =
                r.start as f64 + (self.source - r.start as f64) % (r.end - r.start) as f64;
            self.initialized = false;
        }
    }

    fn best_match(&self, asset: &AudioAsset, expected: u64, range: Option<LoopRange>) -> u64 {
        let mut best = expected;
        let mut best_score = f64::NEG_INFINITY;
        for delta in -(self.search as i64)..=self.search as i64 {
            let candidate = if delta < 0 {
                expected.saturating_sub(delta.unsigned_abs())
            } else {
                expected.saturating_add(delta as u64)
            };
            let mut score = 0.0;
            for frame in (0..self.window / 2).step_by(4) {
                for channel in 0..self.channels {
                    score += f64::from(self.tail[frame * self.channels + channel])
                        * f64::from(sample(asset, candidate + frame as u64, channel, range));
                }
            }
            if score > best_score {
                best_score = score;
                best = candidate;
            }
        }
        wrap(best, range)
    }
}

fn wrap(frame: u64, range: Option<LoopRange>) -> u64 {
    let Some(r) = range.filter(|r| r.is_valid()) else {
        return frame;
    };
    if frame < r.start {
        r.start
    } else if frame >= r.end {
        r.start + (frame - r.start) % (r.end - r.start)
    } else {
        frame
    }
}
fn sample(asset: &AudioAsset, frame: u64, channel: usize, range: Option<LoopRange>) -> f32 {
    asset
        .frames_range(wrap(frame, range), 1)
        .get(channel.min(usize::from(asset.channels()) - 1))
        .map_or(0.0, |s| f32::from(*s) / 32_768.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rate_range() {
        assert!(PlaybackRate::new(0.25).is_ok());
        assert!(PlaybackRate::new(4.0).is_ok());
        assert!(PlaybackRate::new(0.2).is_err());
        assert!(PlaybackRate::new(f32::NAN).is_err());
    }
    #[test]
    fn one_x_is_exact() {
        let asset =
            AudioAsset::from_interleaved(vec![0, 16384, -16384, i16::MAX], 48_000, 1, 1.0).unwrap();
        let mut state = MixerState::new();
        let mut out = [0.0; 4];
        render_block_rate_linear(
            &asset,
            48_000,
            1,
            PlaybackRate::NORMAL,
            None,
            1.0,
            false,
            &mut state,
            &mut out,
        );
        assert_eq!(out, [0.0, 0.5, -0.5, f32::from(i16::MAX) / 32_768.0]);
    }
    #[test]
    fn repeated_seek_rate_loop_stays_finite() {
        let samples = (0..4096)
            .map(|i| ((i as f32 * 0.1).sin() * 20000.0) as i16)
            .collect::<Vec<_>>();
        let asset = AudioAsset::from_interleaved(samples, 48_000, 1, 1.0).unwrap();
        let mut p = RateProcessor::new(1, PitchQuality::Draft).unwrap();
        let mut out = [0.0; 513];
        for i in 0..100 {
            if i % 7 == 0 {
                p.seek((i * 13) as u64, i);
            }
            let rate = PlaybackRate::new(0.25 + (i % 16) as f32 * 0.25).unwrap();
            p.render(
                &asset,
                rate,
                Some(LoopRange {
                    start: 128,
                    end: 2048,
                }),
                1.0,
                false,
                &mut out,
            );
            assert!(out.iter().all(|s| s.is_finite()));
        }
    }
}
