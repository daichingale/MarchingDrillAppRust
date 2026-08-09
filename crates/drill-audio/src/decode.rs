use crate::{AudioAsset, AudioError, DecodeLimits, PeakPyramid};
use std::fs::File;
use std::io::Cursor;
use std::path::Path;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;

pub trait ProgressSink: Send + Sync {
    fn set_progress(&self, permille_x10: u32);
    fn is_cancelled(&self) -> bool;
}

pub struct NullProgress;

impl ProgressSink for NullProgress {
    fn set_progress(&self, _permille_x10: u32) {}

    fn is_cancelled(&self) -> bool {
        false
    }
}

#[derive(Clone, Debug)]
pub struct SourceInfo {
    pub codec: String,
    pub container: String,
    pub source_sample_rate: u32,
    pub source_channels: u16,
    pub declared_duration_seconds: Option<f64>,
    pub decoded_frames: u64,
    pub recovered_errors: u32,
}

#[derive(Clone, Debug)]
pub struct DecodeOutput {
    pub asset: AudioAsset,
    pub peaks: PeakPyramid,
    pub info: SourceInfo,
}

pub fn decode_file(
    path: &Path,
    limits: DecodeLimits,
    progress: &dyn ProgressSink,
) -> Result<DecodeOutput, AudioError> {
    let metadata = std::fs::metadata(path)?;
    let file = File::open(path)?;
    let source = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());
    let mut hint = Hint::new();
    let extension = path.extension().and_then(|value| value.to_str());
    if let Some(extension) = extension {
        hint.with_extension(extension);
    }
    decode_source(
        source,
        hint,
        extension.unwrap_or("unknown"),
        metadata.len(),
        limits,
        progress,
    )
}

/// Decodes an in-memory audio asset, such as an entry embedded in a `.drillproj`.
pub fn decode_bytes(
    bytes: Vec<u8>,
    extension: Option<&str>,
    limits: DecodeLimits,
    progress: &dyn ProgressSink,
) -> Result<DecodeOutput, AudioError> {
    let byte_len = bytes.len() as u64;
    let source = MediaSourceStream::new(
        Box::new(Cursor::new(bytes)),
        MediaSourceStreamOptions::default(),
    );
    let mut hint = Hint::new();
    if let Some(extension) = extension {
        hint.with_extension(extension);
    }
    decode_source(
        source,
        hint,
        extension.unwrap_or("unknown"),
        byte_len,
        limits,
        progress,
    )
}

fn decode_source(
    source: MediaSourceStream,
    hint: Hint,
    container: &str,
    byte_len: u64,
    limits: DecodeLimits,
    progress: &dyn ProgressSink,
) -> Result<DecodeOutput, AudioError> {
    let limits = limits.validate()?;
    if byte_len > limits.max_file_bytes {
        return Err(AudioError::FileTooLarge {
            bytes: byte_len,
            limit: limits.max_file_bytes,
        });
    }
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            source,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|_| AudioError::UnsupportedFormat)?;

    let track = format
        .default_track(TrackType::Audio)
        .ok_or(AudioError::MissingAudioTrack)?;
    let track_id = track.id;
    let track_frames = track.num_frames;
    let codec_params = match track.codec_params.clone() {
        Some(CodecParameters::Audio(params)) => params,
        _ => return Err(AudioError::MissingAudioTrack),
    };
    let sample_rate = codec_params
        .sample_rate
        .ok_or(AudioError::MissingSampleRate)?;
    if !(limits.min_sample_rate..=limits.max_sample_rate).contains(&sample_rate) {
        return Err(AudioError::SampleRateOutOfRange { rate: sample_rate });
    }
    let source_channels = codec_params
        .channels
        .as_ref()
        .map(|channels| channels.count() as u16)
        .ok_or(AudioError::MissingAudioTrack)?;
    if source_channels == 0 || source_channels > limits.max_source_channels {
        return Err(AudioError::TooManyChannels {
            channels: source_channels,
        });
    }
    let output_channels = source_channels.min(2) as u8;
    let max_frames = (limits.max_duration_seconds * f64::from(sample_rate)).floor() as u64;
    let declared_duration_seconds =
        track_frames.map(|frames| frames as f64 / f64::from(sample_rate));
    if track_frames.is_some_and(|frames| frames > max_frames) {
        return Err(AudioError::TooLong {
            frames: track_frames.unwrap_or(0),
            limit: max_frames,
        });
    }
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&codec_params, &AudioDecoderOptions::default())
        .map_err(|_| AudioError::UnsupportedCodec)?;

    let mut samples = Vec::<i16>::new();
    let estimated_samples = track_frames
        .unwrap_or(0)
        .saturating_mul(u64::from(output_channels));
    reserve_bounded(&mut samples, estimated_samples, limits.max_reserve_bytes)?;
    let mut packet_samples = Vec::<i16>::new();
    let mut decoded_frames = 0_u64;
    let mut packets = 0_u64;
    let mut errors = 0_u32;
    let mut consecutive_errors = 0_u32;
    let progress_interval = (sample_rate / 10).max(1) as u64;
    let mut next_progress = progress_interval;

    loop {
        if progress.is_cancelled() {
            return Err(AudioError::Cancelled);
        }
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(_) => return Err(AudioError::Corrupt { decoded_frames }),
        };
        if packet.track_id != track_id {
            continue;
        }
        packets += 1;
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => {
                consecutive_errors = 0;
                decoded
            }
            Err(SymphoniaError::DecodeError(_)) => {
                errors = errors.saturating_add(1);
                consecutive_errors = consecutive_errors.saturating_add(1);
                let ratio = errors as f32 / packets as f32;
                if consecutive_errors > limits.max_consecutive_decode_errors
                    || ratio > limits.max_decode_error_ratio
                {
                    return Err(AudioError::Corrupt { decoded_frames });
                }
                continue;
            }
            Err(SymphoniaError::ResetRequired) => {
                decoder = symphonia::default::get_codecs()
                    .make_audio_decoder(&codec_params, &AudioDecoderOptions::default())
                    .map_err(|_| AudioError::UnsupportedCodec)?;
                continue;
            }
            Err(_) => return Err(AudioError::Corrupt { decoded_frames }),
        };
        packet_samples.clear();
        decoded.copy_to_vec_interleaved(&mut packet_samples);
        let packet_frames = packet_samples.len() / usize::from(source_channels);
        let new_frames = decoded_frames.saturating_add(packet_frames as u64);
        if new_frames > max_frames {
            return Err(AudioError::TooLong {
                frames: new_frames,
                limit: max_frames,
            });
        }
        let needed = packet_frames.saturating_mul(usize::from(output_channels));
        reserve_additional(&mut samples, needed, limits.max_reserve_bytes)?;
        downmix_into(
            &packet_samples,
            usize::from(source_channels),
            usize::from(output_channels),
            &mut samples,
        );
        decoded_frames = new_frames;
        if decoded_frames >= next_progress {
            let denominator = track_frames.unwrap_or(max_frames).max(1);
            let value = decoded_frames
                .saturating_mul(10_000)
                .checked_div(denominator)
                .unwrap_or(0)
                .min(10_000) as u32;
            progress.set_progress(value);
            next_progress = decoded_frames.saturating_add(progress_interval);
        }
    }
    progress.set_progress(10_000);
    let asset = AudioAsset::from_interleaved(samples, sample_rate, output_channels, 1.0)?;
    let peaks = PeakPyramid::build(&asset);
    let container = container.to_ascii_lowercase();
    Ok(DecodeOutput {
        info: SourceInfo {
            codec: format!("{:?}", codec_params.codec),
            container,
            source_sample_rate: sample_rate,
            source_channels,
            declared_duration_seconds,
            decoded_frames,
            recovered_errors: errors,
        },
        asset,
        peaks,
    })
}

fn reserve_bounded(
    samples: &mut Vec<i16>,
    desired_samples: u64,
    max_reserve_bytes: usize,
) -> Result<(), AudioError> {
    let desired = usize::try_from(desired_samples).unwrap_or(usize::MAX);
    let max_samples = max_reserve_bytes / std::mem::size_of::<i16>();
    samples
        .try_reserve(desired.min(max_samples))
        .map_err(|_| AudioError::OutOfMemory)
}

fn reserve_additional(
    samples: &mut Vec<i16>,
    additional: usize,
    max_reserve_bytes: usize,
) -> Result<(), AudioError> {
    if samples.capacity().saturating_sub(samples.len()) >= additional {
        return Ok(());
    }
    let max_samples = max_reserve_bytes / std::mem::size_of::<i16>();
    if additional > max_samples {
        return Err(AudioError::OutOfMemory);
    }
    samples
        .try_reserve(additional.max(1))
        .map_err(|_| AudioError::OutOfMemory)
}

fn downmix_into(
    source: &[i16],
    source_channels: usize,
    output_channels: usize,
    out: &mut Vec<i16>,
) {
    for frame in source.chunks_exact(source_channels) {
        if output_channels == 1 {
            out.push(frame[0]);
            continue;
        }
        if source_channels == 2 {
            out.extend_from_slice(frame);
            continue;
        }
        let surround_sum: i32 = frame[2..].iter().map(|sample| i32::from(*sample)).sum();
        let surround = surround_sum / (source_channels - 1) as i32;
        out.push(
            (i32::from(frame[0]) + surround / 2).clamp(i16::MIN as i32, i16::MAX as i32) as i16,
        );
        out.push(
            (i32::from(frame[1]) + surround / 2).clamp(i16::MIN as i32, i16::MAX as i32) as i16,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multichannel_downmix_is_bounded_and_deterministic() {
        let mut output = Vec::new();
        downmix_into(&[30_000, -30_000, 10_000, 10_000], 4, 2, &mut output);
        assert_eq!(output, [i16::MAX, -26_667]);
    }

    #[test]
    fn oversized_file_is_rejected_before_probe() {
        let path =
            std::env::temp_dir().join(format!("drill-audio-limit-{}.bin", std::process::id()));
        std::fs::write(&path, [0; 16]).unwrap();
        let limits = DecodeLimits {
            max_file_bytes: 8,
            ..DecodeLimits::default()
        };
        assert!(matches!(
            decode_file(&path, limits, &NullProgress),
            Err(AudioError::FileTooLarge { .. })
        ));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn decodes_pcm_wav_into_resident_asset() {
        let path =
            std::env::temp_dir().join(format!("drill-audio-decode-{}.wav", std::process::id()));
        let pcm = [0_i16, 1_000, -1_000, i16::MAX];
        let data_bytes = (pcm.len() * 2) as u32;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_bytes).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&8_000_u32.to_le_bytes());
        wav.extend_from_slice(&16_000_u32.to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&16_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_bytes.to_le_bytes());
        for sample in pcm {
            wav.extend_from_slice(&sample.to_le_bytes());
        }
        std::fs::write(&path, &wav).unwrap();

        let decoded = decode_file(&path, DecodeLimits::default(), &NullProgress).unwrap();
        assert_eq!(decoded.asset.sample_rate(), 8_000);
        assert_eq!(decoded.asset.channels(), 1);
        assert_eq!(decoded.asset.samples(), &pcm);
        assert_eq!(decoded.info.decoded_frames, 4);
        let embedded =
            decode_bytes(wav, Some("wav"), DecodeLimits::default(), &NullProgress).unwrap();
        assert_eq!(embedded.asset.samples(), &pcm);
        assert_eq!(embedded.info.container, "wav");
        std::fs::remove_file(path).unwrap();
    }
}
