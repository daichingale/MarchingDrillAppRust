//! Deterministic, device-independent audio decoding and waveform data.

mod asset;
mod click;
mod clock;
mod decode;
mod output;
mod peaks;
mod rate;

pub use asset::{AudioAsset, DecodeLimits};
pub use click::{
    ClickEvent, ClickMixer, ClickMode, ClickSchedule, ClickSettings, ClickVoices, CountIn,
    DuckingMode, render_mix_with_clicks,
};
pub use clock::{ClockSample, PlaybackClock};
pub use decode::{DecodeOutput, NullProgress, ProgressSink, SourceInfo, decode_bytes, decode_file};
pub use output::{
    AudioOutput, Block, MixerState, OutputDeviceInfo, OutputDiagnostics, OutputError,
    probe_default_output, render_block,
};
pub use peaks::{PEAK_LEVEL_SHIFTS, PEAK_LEVELS, Peak, PeakPyramid};
pub use rate::{
    LoopRange, PitchQuality, PlaybackRate, RateError, RateMode, RateProcessor,
    render_block_rate_linear,
};

use std::fmt;
use std::io;

#[derive(Debug)]
pub enum AudioError {
    Io(io::Error),
    FileTooLarge { bytes: u64, limit: u64 },
    TooLong { frames: u64, limit: u64 },
    InvalidLimits,
    InvalidAsset,
    UnsupportedFormat,
    UnsupportedCodec,
    MissingAudioTrack,
    MissingSampleRate,
    SampleRateOutOfRange { rate: u32 },
    TooManyChannels { channels: u16 },
    Corrupt { decoded_frames: u64 },
    OutOfMemory,
    Cancelled,
}

impl fmt::Display for AudioError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "audio I/O failed: {error}"),
            Self::FileTooLarge { bytes, limit } => {
                write!(
                    formatter,
                    "audio file is too large ({bytes} > {limit} bytes)"
                )
            }
            Self::TooLong { frames, limit } => {
                write!(
                    formatter,
                    "decoded audio is too long ({frames} > {limit} frames)"
                )
            }
            Self::InvalidLimits => formatter.write_str("invalid audio decode limits"),
            Self::InvalidAsset => formatter.write_str("invalid decoded audio asset"),
            Self::UnsupportedFormat => formatter.write_str("unsupported audio container"),
            Self::UnsupportedCodec => formatter.write_str("unsupported audio codec"),
            Self::MissingAudioTrack => formatter.write_str("no audio track was found"),
            Self::MissingSampleRate => formatter.write_str("audio sample rate is missing"),
            Self::SampleRateOutOfRange { rate } => {
                write!(formatter, "audio sample rate is outside limits: {rate}")
            }
            Self::TooManyChannels { channels } => {
                write!(formatter, "audio has too many channels: {channels}")
            }
            Self::Corrupt { decoded_frames } => {
                write!(
                    formatter,
                    "audio is corrupt after {decoded_frames} decoded frames"
                )
            }
            Self::OutOfMemory => formatter.write_str("not enough memory to decode audio"),
            Self::Cancelled => formatter.write_str("audio decode was cancelled"),
        }
    }
}

impl std::error::Error for AudioError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for AudioError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
