//! Typed video export configuration and FFmpeg argument generation.

use serde::{Deserialize, Serialize};
use std::fmt;

const MAX_DIMENSION: u32 = 8_192;
const MAX_BITRATE_KBPS: u32 = 1_000_000;
const MAX_AUDIO_BITRATE_KBPS: u32 = 1_536;
const MAX_DURATION_SECONDS: f32 = 24.0 * 60.0 * 60.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoConfigError {
    InvalidDimensions,
    InvalidFps,
    WebMCodecRequired,
    Vp9RequiresWebM,
    UnsupportedHardwareCodec,
    InvalidQuality,
    InvalidVideoBitrate,
    InvalidAudioBitrate,
    InvalidDuration,
}

impl VideoConfigError {
    pub fn message(self, locale: crate::Locale) -> &'static str {
        use crate::Locale::{En, Ja};
        match (self, locale) {
            (Self::InvalidDimensions, Ja) => "解像度の幅と高さは2以上の偶数にしてください",
            (Self::InvalidDimensions, En) => "Width and height must be even and at least 2",
            (Self::InvalidFps, Ja) => "FPSは1〜240にしてください",
            (Self::InvalidFps, En) => "FPS must be between 1 and 240",
            (Self::WebMCodecRequired, Ja) => "WebMではAV1またはVP9を選択してください",
            (Self::WebMCodecRequired, En) => "WebM requires AV1 or VP9",
            (Self::Vp9RequiresWebM, Ja) => "VP9はWebMコンテナで使用してください",
            (Self::Vp9RequiresWebM, En) => "VP9 requires the WebM container",
            (Self::UnsupportedHardwareCodec, Ja) => "AV1とVP9のハードウェアエンコードは未対応です",
            (Self::UnsupportedHardwareCodec, En) => "Hardware AV1 and VP9 encoding is unsupported",
            (Self::InvalidQuality, Ja) => "品質値は0〜51にしてください",
            (Self::InvalidQuality, En) => "Quality must be between 0 and 51",
            (Self::InvalidVideoBitrate, Ja) => "動画ビットレートが対応範囲外です",
            (Self::InvalidVideoBitrate, En) => "The video bitrate is outside the supported range",
            (Self::InvalidAudioBitrate, Ja) => "音声ビットレートが対応範囲外です",
            (Self::InvalidAudioBitrate, En) => "The audio bitrate is outside the supported range",
            (Self::InvalidDuration, Ja) => "動画の長さが対応範囲外です",
            (Self::InvalidDuration, En) => "The video duration is outside the supported range",
        }
    }
}

impl fmt::Display for VideoConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message(crate::Locale::En))
    }
}

impl std::error::Error for VideoConfigError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportPreset {
    Fast,
    Standard,
    HighQuality,
    Youtube4k,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoCodec {
    H264,
    H265,
    Av1,
    Vp9,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoContainer {
    Mp4,
    Mov,
    WebM,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EncoderBackend {
    Auto,
    Software,
    Nvidia,
    Intel,
    Amd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RateControl {
    ConstantQuality,
    Bitrate,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VideoExportConfig {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub codec: VideoCodec,
    pub container: VideoContainer,
    pub backend: EncoderBackend,
    pub rate_control: RateControl,
    pub quality: u8,
    pub bitrate_kbps: u32,
    pub audio_enabled: bool,
    pub audio_bitrate_kbps: u32,
    pub faststart: bool,
}

impl Default for VideoExportConfig {
    fn default() -> Self {
        Self::preset(ExportPreset::Standard)
    }
}

impl VideoExportConfig {
    pub fn preset(preset: ExportPreset) -> Self {
        let mut config = Self {
            width: 1920,
            height: 1080,
            fps: 60,
            codec: VideoCodec::H264,
            container: VideoContainer::Mp4,
            backend: EncoderBackend::Auto,
            rate_control: RateControl::ConstantQuality,
            quality: 20,
            bitrate_kbps: 12_000,
            audio_enabled: true,
            audio_bitrate_kbps: 192,
            faststart: true,
        };
        match preset {
            ExportPreset::Fast => {
                config.fps = 30;
                config.quality = 25;
                config.width = 1280;
                config.height = 720;
            }
            ExportPreset::Standard => {}
            ExportPreset::HighQuality => {
                config.quality = 16;
                config.bitrate_kbps = 30_000;
            }
            ExportPreset::Youtube4k => {
                config.width = 3840;
                config.height = 2160;
                config.quality = 18;
                config.bitrate_kbps = 45_000;
            }
        }
        config
    }

    pub fn validate(&self) -> Result<(), VideoConfigError> {
        if self.width < 2
            || self.height < 2
            || self.width > MAX_DIMENSION
            || self.height > MAX_DIMENSION
            || !self.width.is_multiple_of(2)
            || !self.height.is_multiple_of(2)
        {
            return Err(VideoConfigError::InvalidDimensions);
        }
        if !(1..=240).contains(&self.fps) {
            return Err(VideoConfigError::InvalidFps);
        }
        if self.container == VideoContainer::WebM
            && !matches!(self.codec, VideoCodec::Av1 | VideoCodec::Vp9)
        {
            return Err(VideoConfigError::WebMCodecRequired);
        }
        if self.container != VideoContainer::WebM && self.codec == VideoCodec::Vp9 {
            return Err(VideoConfigError::Vp9RequiresWebM);
        }
        if matches!(self.codec, VideoCodec::Av1 | VideoCodec::Vp9)
            && !matches!(
                self.backend,
                EncoderBackend::Auto | EncoderBackend::Software
            )
        {
            return Err(VideoConfigError::UnsupportedHardwareCodec);
        }
        if self.quality > 51 {
            return Err(VideoConfigError::InvalidQuality);
        }
        if self.rate_control == RateControl::Bitrate
            && !(1..=MAX_BITRATE_KBPS).contains(&self.bitrate_kbps)
        {
            return Err(VideoConfigError::InvalidVideoBitrate);
        }
        if self.audio_enabled && !(1..=MAX_AUDIO_BITRATE_KBPS).contains(&self.audio_bitrate_kbps) {
            return Err(VideoConfigError::InvalidAudioBitrate);
        }
        Ok(())
    }

    pub fn extension(&self) -> &'static str {
        match self.container {
            VideoContainer::Mp4 => "mp4",
            VideoContainer::Mov => "mov",
            VideoContainer::WebM => "webm",
        }
    }

    pub fn frame_count(&self, duration_seconds: f32) -> u64 {
        self.checked_frame_count(duration_seconds).unwrap_or(0)
    }

    pub fn checked_frame_count(&self, duration_seconds: f32) -> Result<u64, VideoConfigError> {
        self.validate_duration(duration_seconds)?;
        Ok((f64::from(duration_seconds) * f64::from(self.fps)).ceil() as u64)
    }

    pub fn estimated_megabytes(&self, duration_seconds: f32) -> f32 {
        self.checked_estimated_megabytes(duration_seconds)
            .unwrap_or(0.0)
    }

    pub fn checked_estimated_megabytes(
        &self,
        duration_seconds: f32,
    ) -> Result<f32, VideoConfigError> {
        self.validate()?;
        self.validate_duration(duration_seconds)?;
        let video = if self.rate_control == RateControl::Bitrate {
            self.bitrate_kbps
        } else {
            estimated_quality_bitrate(self)
        };
        let audio = if self.audio_enabled {
            self.audio_bitrate_kbps
        } else {
            0
        };
        Ok((video + audio) as f32 * duration_seconds / 8_000.0)
    }

    fn validate_duration(&self, duration_seconds: f32) -> Result<(), VideoConfigError> {
        if !duration_seconds.is_finite()
            || !(0.0..=MAX_DURATION_SECONDS).contains(&duration_seconds)
        {
            return Err(VideoConfigError::InvalidDuration);
        }
        Ok(())
    }

    pub fn ffmpeg_args(
        &self,
        audio_path: Option<&str>,
        output_path: &str,
    ) -> Result<Vec<String>, VideoConfigError> {
        self.validate()?;
        let mut args = vec![
            "-hide_banner".into(),
            "-y".into(),
            "-f".into(),
            "rawvideo".into(),
            "-pixel_format".into(),
            "rgba".into(),
            "-video_size".into(),
            format!("{}x{}", self.width, self.height),
            "-framerate".into(),
            self.fps.to_string(),
            "-i".into(),
            "pipe:0".into(),
        ];
        if self.audio_enabled
            && let Some(path) = audio_path
        {
            args.extend([
                "-i".into(),
                path.into(),
                "-map".into(),
                "0:v:0".into(),
                "-map".into(),
                "1:a:0".into(),
                "-c:a".into(),
                audio_codec(self.container).into(),
                "-b:a".into(),
                format!("{}k", self.audio_bitrate_kbps),
                "-shortest".into(),
            ]);
        }
        args.extend([
            "-c:v".into(),
            video_encoder(self.codec, self.backend).into(),
        ]);
        match self.rate_control {
            RateControl::ConstantQuality => {
                args.extend([quality_flag(self.backend).into(), self.quality.to_string()])
            }
            RateControl::Bitrate => args.extend(["-b:v".into(), format!("{}k", self.bitrate_kbps)]),
        }
        args.extend(["-pix_fmt".into(), "yuv420p".into()]);
        if self.faststart && self.container == VideoContainer::Mp4 {
            args.extend(["-movflags".into(), "+faststart".into()]);
        }
        args.extend([
            "-progress".into(),
            "pipe:1".into(),
            "-nostats".into(),
            output_path.into(),
        ]);
        Ok(args)
    }
}

fn video_encoder(codec: VideoCodec, backend: EncoderBackend) -> &'static str {
    match (codec, backend) {
        (VideoCodec::H264, EncoderBackend::Nvidia) => "h264_nvenc",
        (VideoCodec::H264, EncoderBackend::Intel) => "h264_qsv",
        (VideoCodec::H264, EncoderBackend::Amd) => "h264_amf",
        (VideoCodec::H265, EncoderBackend::Nvidia) => "hevc_nvenc",
        (VideoCodec::H265, EncoderBackend::Intel) => "hevc_qsv",
        (VideoCodec::H265, EncoderBackend::Amd) => "hevc_amf",
        (VideoCodec::H264, _) => "libx264",
        (VideoCodec::H265, _) => "libx265",
        (VideoCodec::Av1, _) => "libsvtav1",
        (VideoCodec::Vp9, _) => "libvpx-vp9",
    }
}

fn quality_flag(backend: EncoderBackend) -> &'static str {
    if backend == EncoderBackend::Software || backend == EncoderBackend::Auto {
        "-crf"
    } else {
        "-cq"
    }
}

fn audio_codec(container: VideoContainer) -> &'static str {
    if container == VideoContainer::WebM {
        "libopus"
    } else {
        "aac"
    }
}

fn estimated_quality_bitrate(config: &VideoExportConfig) -> u32 {
    let pixels = config.width as f32 * config.height as f32;
    let fps_factor = config.fps as f32 / 30.0;
    let quality_factor = (52_u32.saturating_sub(config.quality as u32)) as f32 / 32.0;
    (pixels / 1_000.0 * fps_factor * quality_factor).max(500.0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_errors_are_machine_readable_and_localized() {
        let config = VideoExportConfig {
            fps: 0,
            ..VideoExportConfig::default()
        };
        let error = config.validate().unwrap_err();
        assert_eq!(error, VideoConfigError::InvalidFps);
        assert_eq!(
            error.message(crate::Locale::En),
            "FPS must be between 1 and 240"
        );
        assert_ne!(
            error.message(crate::Locale::Ja),
            error.message(crate::Locale::En)
        );
    }

    #[test]
    fn standard_is_compatible_mp4() {
        assert!(VideoExportConfig::default().validate().is_ok());
    }

    #[test]
    fn rejects_odd_dimensions_and_bad_webm_codec() {
        let mut config = VideoExportConfig {
            width: 1919,
            ..VideoExportConfig::default()
        };
        assert!(config.validate().is_err());
        config.width = 1920;
        config.container = VideoContainer::WebM;
        assert!(config.validate().is_err());
    }

    #[test]
    fn frame_count_is_deterministic() {
        let config = VideoExportConfig::default();
        assert_eq!(config.frame_count(10.0), 600);
    }

    #[test]
    fn ffmpeg_arguments_are_separate_and_include_progress() {
        let config = VideoExportConfig::default();
        let args = config
            .ffmpeg_args(Some("music file.wav"), "show.mp4")
            .unwrap();
        assert!(args.windows(2).any(|pair| pair == ["-progress", "pipe:1"]));
        assert!(args.contains(&"music file.wav".to_owned()));
        assert_eq!(args.last().unwrap(), "show.mp4");
    }

    #[test]
    fn estimates_nonzero_output_size() {
        assert!(VideoExportConfig::default().estimated_megabytes(60.0) > 1.0);
    }

    #[test]
    fn rejects_unsupported_codec_backend_combinations() {
        for codec in [VideoCodec::Av1, VideoCodec::Vp9] {
            for backend in [
                EncoderBackend::Nvidia,
                EncoderBackend::Intel,
                EncoderBackend::Amd,
            ] {
                let config = VideoExportConfig {
                    codec,
                    backend,
                    container: VideoContainer::WebM,
                    ..VideoExportConfig::default()
                };
                assert!(config.validate().is_err());
                assert!(config.ffmpeg_args(None, "show.webm").is_err());
            }
        }
    }

    #[test]
    fn software_av1_and_vp9_use_crf() {
        for codec in [VideoCodec::Av1, VideoCodec::Vp9] {
            let config = VideoExportConfig {
                codec,
                container: VideoContainer::WebM,
                backend: EncoderBackend::Software,
                ..VideoExportConfig::default()
            };
            let args = config.ffmpeg_args(None, "show.webm").unwrap();
            assert!(args.contains(&"-crf".to_owned()));
            assert!(!args.contains(&"-cq".to_owned()));
        }
    }

    #[test]
    fn rejects_unbounded_configuration_and_duration() {
        let config = VideoExportConfig {
            width: MAX_DIMENSION + 2,
            ..VideoExportConfig::default()
        };
        assert!(config.validate().is_err());
        let config = VideoExportConfig {
            audio_bitrate_kbps: MAX_AUDIO_BITRATE_KBPS + 1,
            ..VideoExportConfig::default()
        };
        assert!(config.validate().is_err());

        let config = VideoExportConfig::default();
        for duration in [f32::NAN, f32::INFINITY, -1.0, MAX_DURATION_SECONDS + 1.0] {
            assert!(config.checked_frame_count(duration).is_err());
            assert!(config.checked_estimated_megabytes(duration).is_err());
            assert_eq!(config.frame_count(duration), 0);
            assert_eq!(config.estimated_megabytes(duration), 0.0);
        }
    }
}
