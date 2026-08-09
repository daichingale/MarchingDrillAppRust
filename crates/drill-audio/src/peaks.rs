use crate::AudioAsset;

pub const PEAK_LEVEL_SHIFTS: [u32; 4] = [8, 10, 12, 14];
pub const PEAK_LEVELS: usize = PEAK_LEVEL_SHIFTS.len();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Peak {
    pub min: i16,
    pub max: i16,
}

impl Peak {
    pub const SILENT: Self = Self { min: 0, max: 0 };

    #[must_use]
    pub fn merge(self, other: Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }
}

#[derive(Clone, Debug)]
pub struct PeakPyramid {
    channels: u8,
    frames: u64,
    levels: [Vec<Peak>; PEAK_LEVELS],
}

impl PeakPyramid {
    #[must_use]
    pub fn build(asset: &AudioAsset) -> Self {
        let mut result = Self {
            channels: asset.channels(),
            frames: asset.frames(),
            levels: std::array::from_fn(|_| Vec::new()),
        };
        Self::build_into(asset, &mut result);
        result
    }

    pub fn build_into(asset: &AudioAsset, out: &mut Self) {
        out.channels = asset.channels();
        out.frames = asset.frames();
        for level in &mut out.levels {
            level.clear();
        }
        build_base(asset, &mut out.levels[0]);
        for level_index in 1..PEAK_LEVELS {
            let (lower, upper) = out.levels.split_at_mut(level_index);
            merge_level(
                &lower[level_index - 1],
                &mut upper[0],
                usize::from(out.channels),
            );
        }
    }

    #[must_use]
    pub const fn channels(&self) -> u8 {
        self.channels
    }

    #[must_use]
    pub const fn frames(&self) -> u64 {
        self.frames
    }

    /// Produces bucket-major, channel-minor min/max values for `[start, end)`.
    pub fn range(
        &self,
        asset: &AudioAsset,
        start: u64,
        end: u64,
        out_buckets: usize,
        out: &mut Vec<Peak>,
    ) {
        out.clear();
        if out_buckets == 0 || start >= end || start >= self.frames {
            return;
        }
        let end = end.min(self.frames);
        let channels = usize::from(self.channels);
        if out
            .try_reserve(out_buckets.saturating_mul(channels))
            .is_err()
        {
            return;
        }
        let span = end - start;
        let frames_per_output = span.div_ceil(out_buckets as u64).max(1);
        let level = self.level_for(frames_per_output);
        for bucket in 0..out_buckets {
            let bucket_start = start + span.saturating_mul(bucket as u64) / out_buckets as u64;
            let bucket_end = start + span.saturating_mul((bucket + 1) as u64) / out_buckets as u64;
            for channel in 0..channels {
                out.push(match level {
                    Some(level) => self.range_from_level(level, bucket_start, bucket_end, channel),
                    None => raw_range(asset, bucket_start, bucket_end, channel),
                });
            }
        }
    }

    fn level_for(&self, frames_per_bucket: u64) -> Option<usize> {
        PEAK_LEVEL_SHIFTS
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, shift)| ((1_u64 << shift) <= frames_per_bucket).then_some(index))
    }

    fn range_from_level(&self, level: usize, start: u64, end: u64, channel: usize) -> Peak {
        let width = 1_u64 << PEAK_LEVEL_SHIFTS[level];
        let first = start / width;
        let last = end.saturating_sub(1) / width;
        let channels = usize::from(self.channels);
        (first..=last).fold(Peak::SILENT, |peak, bucket| {
            let index = usize::try_from(bucket)
                .ok()
                .and_then(|bucket| bucket.checked_mul(channels))
                .and_then(|index| index.checked_add(channel));
            index
                .and_then(|index| self.levels[level].get(index).copied())
                .map_or(peak, |value| peak.merge(value))
        })
    }

    #[must_use]
    pub fn resident_bytes(&self) -> u64 {
        self.levels
            .iter()
            .map(|level| level.len() as u64 * std::mem::size_of::<Peak>() as u64)
            .sum()
    }
}

fn build_base(asset: &AudioAsset, out: &mut Vec<Peak>) {
    let channels = usize::from(asset.channels());
    let bucket_frames = 1_usize << PEAK_LEVEL_SHIFTS[0];
    for frames in asset.samples().chunks(bucket_frames * channels) {
        for channel in 0..channels {
            let mut peak = Peak::SILENT;
            for sample in frames.iter().skip(channel).step_by(channels) {
                peak.min = peak.min.min(*sample);
                peak.max = peak.max.max(*sample);
            }
            out.push(peak);
        }
    }
}

fn merge_level(lower: &[Peak], upper: &mut Vec<Peak>, channels: usize) {
    for group in lower.chunks(channels * 4) {
        for channel in 0..channels {
            let peak = (channel..group.len())
                .step_by(channels)
                .fold(Peak::SILENT, |peak, index| peak.merge(group[index]));
            upper.push(peak);
        }
    }
}

fn raw_range(asset: &AudioAsset, start: u64, end: u64, channel: usize) -> Peak {
    let channels = usize::from(asset.channels());
    let length = usize::try_from(end.saturating_sub(start)).unwrap_or(usize::MAX);
    asset
        .frames_range(start, length)
        .iter()
        .skip(channel)
        .step_by(channels)
        .fold(Peak::SILENT, |mut peak, sample| {
            peak.min = peak.min.min(*sample);
            peak.max = peak.max.max(*sample);
            peak
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pyramid_preserves_channel_extrema() {
        let mut samples = vec![0; 1_024 * 2];
        samples[20] = -12_345;
        samples[401] = 23_456;
        let asset = AudioAsset::from_interleaved(samples, 48_000, 2, 1.0).unwrap();
        let pyramid = PeakPyramid::build(&asset);
        let mut output = Vec::new();
        pyramid.range(&asset, 0, asset.frames(), 1, &mut output);
        assert_eq!(
            output,
            [
                Peak {
                    min: -12_345,
                    max: 0
                },
                Peak {
                    min: 0,
                    max: 23_456
                }
            ]
        );
    }

    #[test]
    fn range_has_stable_shape_and_handles_empty_requests() {
        let asset = AudioAsset::from_interleaved(vec![1; 4_096], 48_000, 1, 1.0).unwrap();
        let pyramid = PeakPyramid::build(&asset);
        let mut output = vec![Peak::SILENT];
        pyramid.range(&asset, 10, 100, 7, &mut output);
        assert_eq!(output.len(), 7);
        pyramid.range(&asset, 100, 10, 7, &mut output);
        assert!(output.is_empty());
    }

    #[test]
    fn pyramid_memory_stays_below_pcm_budget() {
        let asset = AudioAsset::from_interleaved(vec![0; 480_000 * 2], 48_000, 2, 1.0).unwrap();
        let pyramid = PeakPyramid::build(&asset);
        assert!(pyramid.resident_bytes() * 10_000 <= asset.resident_bytes() * 105);
    }
}
