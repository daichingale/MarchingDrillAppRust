use crate::AudioError;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct AudioAsset {
    samples: Arc<[i16]>,
    frames: u64,
    sample_rate: u32,
    channels: u8,
    peak_scale: f32,
}

impl AudioAsset {
    pub fn from_interleaved(
        samples: impl Into<Arc<[i16]>>,
        sample_rate: u32,
        channels: u8,
        peak_scale: f32,
    ) -> Result<Self, AudioError> {
        let samples = samples.into();
        if sample_rate == 0
            || !(1..=2).contains(&channels)
            || !peak_scale.is_finite()
            || peak_scale < 1.0
            || samples.len() % usize::from(channels) != 0
        {
            return Err(AudioError::InvalidAsset);
        }
        let frames = u64::try_from(samples.len() / usize::from(channels))
            .map_err(|_| AudioError::InvalidAsset)?;
        Ok(Self {
            samples,
            frames,
            sample_rate,
            channels,
            peak_scale,
        })
    }

    #[must_use]
    pub const fn frames(&self) -> u64 {
        self.frames
    }

    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    #[must_use]
    pub const fn channels(&self) -> u8 {
        self.channels
    }

    #[must_use]
    pub const fn peak_scale(&self) -> f32 {
        self.peak_scale
    }

    #[must_use]
    pub fn duration_seconds(&self) -> f64 {
        self.frames as f64 / f64::from(self.sample_rate)
    }

    #[must_use]
    pub fn frames_range(&self, start: u64, len: usize) -> &[i16] {
        let channels = u64::from(self.channels);
        let begin = start.min(self.frames).saturating_mul(channels);
        let end = start
            .saturating_add(len as u64)
            .min(self.frames)
            .saturating_mul(channels);
        let Ok(begin) = usize::try_from(begin) else {
            return &[];
        };
        let Ok(end) = usize::try_from(end) else {
            return &[];
        };
        self.samples.get(begin..end).unwrap_or(&[])
    }

    #[must_use]
    pub fn samples(&self) -> &[i16] {
        &self.samples
    }

    #[must_use]
    pub fn resident_bytes(&self) -> u64 {
        self.samples.len() as u64 * std::mem::size_of::<i16>() as u64
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DecodeLimits {
    pub max_file_bytes: u64,
    pub max_duration_seconds: f64,
    pub min_sample_rate: u32,
    pub max_sample_rate: u32,
    pub max_source_channels: u16,
    pub max_consecutive_decode_errors: u32,
    pub max_decode_error_ratio: f32,
    pub max_reserve_bytes: usize,
}

impl Default for DecodeLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 1 << 30,
            max_duration_seconds: 1_800.0,
            min_sample_rate: 4_000,
            max_sample_rate: 384_000,
            max_source_channels: 8,
            max_consecutive_decode_errors: 64,
            max_decode_error_ratio: 0.05,
            max_reserve_bytes: 64 << 20,
        }
    }
}

impl DecodeLimits {
    pub(crate) fn validate(self) -> Result<Self, AudioError> {
        if self.max_file_bytes == 0
            || !self.max_duration_seconds.is_finite()
            || self.max_duration_seconds <= 0.0
            || self.min_sample_rate == 0
            || self.min_sample_rate > self.max_sample_rate
            || self.max_source_channels == 0
            || self.max_source_channels > 64
            || self.max_consecutive_decode_errors == 0
            || !self.max_decode_error_ratio.is_finite()
            || !(0.0..=1.0).contains(&self.max_decode_error_ratio)
            || self.max_reserve_bytes == 0
        {
            return Err(AudioError::InvalidLimits);
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_ranges_are_truncated_without_panicking() {
        let asset = AudioAsset::from_interleaved(vec![1, 2, 3, 4], 48_000, 2, 1.0).unwrap();
        assert_eq!(asset.frames_range(0, 1), &[1, 2]);
        assert_eq!(asset.frames_range(1, usize::MAX), &[3, 4]);
        assert!(asset.frames_range(2, 1).is_empty());
        assert!(asset.frames_range(u64::MAX, usize::MAX).is_empty());
    }

    #[test]
    fn malformed_assets_and_limits_are_rejected() {
        assert!(AudioAsset::from_interleaved(vec![1], 48_000, 2, 1.0).is_err());
        let limits = DecodeLimits {
            max_duration_seconds: f64::NAN,
            ..DecodeLimits::default()
        };
        assert!(limits.validate().is_err());
    }
}
