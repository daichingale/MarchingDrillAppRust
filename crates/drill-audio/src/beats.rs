//! Suggest a constant BPM from a decoded reference track.
//!
//! The estimate is a normalized autocorrelation of the onset envelope over
//! the first part of the file. It is a suggestion: the project tempo map is
//! unchanged until the user applies it.

use crate::AudioAsset;

const HOP_HZ: f32 = 200.0;
const MIN_BPM: f32 = 60.0;
const MAX_BPM: f32 = 200.0;
const MAX_SECONDS: f32 = 24.0;
const MIN_CONFIDENCE: f32 = 1.35;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BpmGuess {
    pub bpm: f32,
    pub confidence: f32,
}

/// Best constant tempo in `60..=200` BPM, or `None` when the file is too
/// short, too quiet, or has no repeating onset.
#[must_use]
pub fn suggest_bpm(asset: &AudioAsset) -> Option<BpmGuess> {
    let rate = asset.sample_rate();
    if rate < 1_000 || asset.frames() < u64::from(rate) / 2 {
        return None;
    }
    let hop = ((rate as f32) / HOP_HZ).round().max(1.0) as usize;
    let max_frames = ((MAX_SECONDS * rate as f32) as u64).min(asset.frames()) as usize;
    let channels = usize::from(asset.channels()).max(1);
    let samples = asset.samples();
    let mut energy = Vec::new();
    let mut frame = 0usize;
    while frame + hop <= max_frames {
        let start = frame.saturating_mul(channels);
        let end = (frame + hop).saturating_mul(channels);
        let Some(slice) = samples.get(start..end) else {
            break;
        };
        let mut sum = 0.0f64;
        for chunk in slice.chunks(channels) {
            let mut mono = 0i32;
            for sample in chunk {
                mono += i32::from(*sample);
            }
            sum += f64::from(mono.abs()) / f64::from(channels as i32);
        }
        energy.push(sum / hop as f64);
        frame += hop;
    }
    if energy.len() < 32 {
        return None;
    }
    let mut flux = vec![0.0f64; energy.len()];
    for index in 1..energy.len() {
        let rise = energy[index] - energy[index - 1];
        if rise > 0.0 {
            flux[index] = rise;
        }
    }
    let mean = flux.iter().copied().sum::<f64>() / flux.len() as f64;
    if mean < 1.0 {
        return None;
    }
    let hops_per_sec = f64::from(rate) / hop as f64;
    let lag_min = (hops_per_sec * 60.0 / f64::from(MAX_BPM)).ceil() as usize;
    let lag_max = (hops_per_sec * 60.0 / f64::from(MIN_BPM)).floor() as usize;
    if lag_min < 1 || lag_max <= lag_min || lag_max >= flux.len() {
        return None;
    }
    let mut scores = vec![0.0f64; lag_max + 1];
    let mut best_lag = lag_min;
    let mut best = 0.0f64;
    let mut total = 0.0f64;
    for lag in lag_min..=lag_max {
        let mut sum = 0.0f64;
        let mut index = 0usize;
        while index + lag < flux.len() {
            sum += flux[index] * flux[index + lag];
            index += 1;
        }
        let score = sum / (flux.len() - lag) as f64;
        scores[lag] = score;
        total += score;
        if score > best {
            best = score;
            best_lag = lag;
        }
    }
    // A steady pulse also correlates at twice the beat. When the faster
    // octave is nearly as strong, that is the beat people count.
    let half = best_lag / 2;
    if half >= lag_min && scores[half] >= best * 0.85 {
        best_lag = half;
        best = scores[half];
    }
    let lag = refine_lag(&scores, best_lag, lag_min, lag_max);
    let bpm = (60.0 * hops_per_sec / lag) as f32;
    if !(MIN_BPM..=MAX_BPM).contains(&bpm) {
        return None;
    }
    let average = total / (lag_max - lag_min + 1) as f64;
    let confidence = (best / average.max(1e-9)) as f32;
    (confidence >= MIN_CONFIDENCE).then_some(BpmGuess { bpm, confidence })
}

fn refine_lag(scores: &[f64], lag: usize, lag_min: usize, lag_max: usize) -> f64 {
    if lag <= lag_min || lag >= lag_max {
        return lag as f64;
    }
    let left = scores[lag - 1];
    let mid = scores[lag];
    let right = scores[lag + 1];
    let denom = left - 2.0 * mid + right;
    if denom.abs() < 1e-12 {
        return lag as f64;
    }
    let delta = (0.5 * (left - right) / denom).clamp(-0.5, 0.5);
    lag as f64 + delta
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pulses(bpm: f32, channels: u8) -> AudioAsset {
        let rate = 8_000u32;
        let frames = rate as usize * 8;
        let period = ((rate as f32 * 60.0) / bpm).round() as usize;
        let mut samples = vec![0i16; frames * usize::from(channels)];
        let mut frame = 0usize;
        while frame < frames {
            for step in 0..30.min(frames - frame) {
                let value = (12_000.0 * (1.0 - step as f32 / 30.0)) as i16;
                for channel in 0..usize::from(channels) {
                    samples[(frame + step) * usize::from(channels) + channel] = value;
                }
            }
            frame += period.max(1);
        }
        AudioAsset::from_interleaved(samples, rate, channels, 1.0).unwrap()
    }

    #[test]
    fn a_steady_pulse_suggests_its_tempo() {
        for bpm in [90.0, 120.0, 144.0] {
            let guess = suggest_bpm(&pulses(bpm, 1)).unwrap_or_else(|| panic!("no guess at {bpm}"));
            assert!(
                (guess.bpm - bpm).abs() < 1.5,
                "expected {bpm}, got {} ({})",
                guess.bpm,
                guess.confidence
            );
            assert!(guess.confidence >= MIN_CONFIDENCE);
        }
    }

    #[test]
    fn stereo_pulses_use_the_same_tempo() {
        let guess = suggest_bpm(&pulses(120.0, 2)).expect("stereo guess");
        assert!((guess.bpm - 120.0).abs() < 1.5);
    }

    #[test]
    fn silence_does_not_invent_a_tempo() {
        let asset = AudioAsset::from_interleaved(vec![0i16; 8_000 * 4], 8_000, 1, 1.0).unwrap();
        assert!(suggest_bpm(&asset).is_none());
    }
}
