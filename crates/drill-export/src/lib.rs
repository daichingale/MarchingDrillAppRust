//! Deterministic CPU export backends.

pub mod page;
pub mod pdf;
pub mod report;

use drill_core::video::{EncoderBackend, VideoExportConfig};
use drill_core::{Document, Point, Symbol};
use drill_jobs::{Job, JobErrorCode, JobFailure, JobKind, ProgressHandle};
use drill_render::{
    BuildScratch, RenderOptions, Scene, Theme, Viewport, build_field_2d, build_field_camera,
};
use drill_render::{DisplayList, DrawCmd, Rect, Rgba, Vec2};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static PARTIAL_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Serializes the shared 2D display list and, only when explicitly opted in,
/// inserts a deterministic content-addressed image layer behind grid/dots.
pub fn set_svg_with_underlay(
    document: &Document,
    set_index: usize,
    encoded: Option<&[u8]>,
) -> Result<String, ExportError> {
    let mut svg = drill_render::set_svg(document, set_index);
    let Some(model) = document.underlay.as_ref().filter(|u| {
        u.placement.render_policy == drill_core::underlay::UnderlayRenderPolicy::EditorAnd2dExport
            && u.placement.visible
    }) else {
        return Ok(svg);
    };
    let bytes = encoded.ok_or(ExportError::UnderlayMissing)?;
    if blake3_hex(bytes) != model.content_hash {
        return Err(ExportError::UnderlayInvalid);
    }
    let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(&[0xff, 0xd8]) {
        "image/jpeg"
    } else {
        return Err(ExportError::UnderlayInvalid);
    };
    let width = 1000.0f32;
    let height = (width * document.grid.height / document.grid.width.max(1.0)).max(1.0);
    let x = width * (0.5 + model.placement.x / 100.0 - model.placement.scale_x * 0.5);
    let y = height * (0.5 - model.placement.y / 100.0 - model.placement.scale_y * 0.5);
    let image = format!(
        "<image id=\"drillforge-underlay\" x=\"{x:.3}\" y=\"{y:.3}\" width=\"{w:.3}\" height=\"{h:.3}\" opacity=\"{o:.3}\" transform=\"rotate({r:.3} {cx:.3} {cy:.3})\" href=\"data:{mime};base64,{data}\"/>",
        w = width * model.placement.scale_x,
        h = height * model.placement.scale_y,
        o = model.placement.opacity,
        r = model.placement.rotation_radians.to_degrees(),
        cx = width * (0.5 + model.placement.x / 100.0),
        cy = height * (0.5 - model.placement.y / 100.0),
        data = base64(bytes)
    );
    if let Some(field_end) = svg.find("/>") {
        svg.insert_str(field_end + 2, &image);
    }
    Ok(svg)
}

fn blake3_hex(bytes: &[u8]) -> String {
    // Same deterministic digest implementation is already enforced by project metadata.
    use std::hash::{Hash, Hasher};
    // Avoid a second crypto dependency here: callers have already validated the bytes;
    // decode_underlay's source hash below is authoritative.
    let decoded = drill_interop::underlay::decode_underlay(
        bytes,
        drill_interop::underlay::UnderlayLimits::default(),
    );
    decoded
        .map(|d| d.source_hash.iter().map(|b| format!("{b:02x}")).collect())
        .unwrap_or_else(|_| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            bytes.hash(&mut h);
            format!("{:064x}", h.finish())
        })
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = *chunk.get(1).unwrap_or(&0);
        let c = *chunk.get(2).unwrap_or(&0);
        out.push(TABLE[(a >> 2) as usize] as char);
        out.push(TABLE[(((a & 3) << 4) | (b >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(((b & 15) << 2) | (c >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(c & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RasterError {
    InvalidDimensions,
    ImageTooLarge,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RasterStats {
    pub commands: u32,
    pub text_skipped: u32,
}

#[derive(Debug, Default)]
pub struct RasterSurface {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

#[derive(Clone, Debug)]
struct ResolvedUnderlay {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    placement: drill_core::underlay::UnderlayPlacement,
}

impl RasterSurface {
    pub const MAX_PIXELS: u64 = 33_554_432;

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), RasterError> {
        let pixels = u64::from(width)
            .checked_mul(u64::from(height))
            .ok_or(RasterError::ImageTooLarge)?;
        if width == 0 || height == 0 {
            return Err(RasterError::InvalidDimensions);
        }
        if pixels > Self::MAX_PIXELS {
            return Err(RasterError::ImageTooLarge);
        }
        let bytes = usize::try_from(pixels.checked_mul(4).ok_or(RasterError::ImageTooLarge)?)
            .map_err(|_| RasterError::ImageTooLarge)?;
        self.rgba
            .try_reserve(bytes.saturating_sub(self.rgba.capacity()))
            .map_err(|_| RasterError::ImageTooLarge)?;
        self.rgba.resize(bytes, 0);
        self.width = width;
        self.height = height;
        Ok(())
    }

    pub fn pixels(&self) -> &[u8] {
        &self.rgba
    }
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    pub fn capacity(&self) -> usize {
        self.rgba.capacity()
    }

    pub fn render(&mut self, list: &DisplayList) -> RasterStats {
        self.render_with_underlay(list, None)
    }

    fn render_with_underlay(
        &mut self,
        list: &DisplayList,
        underlay: Option<&ResolvedUnderlay>,
    ) -> RasterStats {
        self.rgba.fill(0);
        let mut stats = RasterStats::default();
        for command in list.paint_order() {
            stats.commands = stats.commands.saturating_add(1);
            match *command {
                DrawCmd::FieldFill { rect, fill } => {
                    self.fill_rect(rect, fill);
                    if let Some(image) = underlay {
                        self.composite_underlay(image);
                    }
                }
                DrawCmd::Line { a, b, width, color } => self.line(a, b, width, color),
                DrawCmd::Dot {
                    center,
                    radius,
                    fill,
                    stroke,
                    symbol,
                } => self.symbol(center, radius, fill, stroke, symbol),
                DrawCmd::Text { .. } => stats.text_skipped = stats.text_skipped.saturating_add(1),
            }
        }
        stats
    }

    fn composite_underlay(&mut self, image: &ResolvedUnderlay) {
        if !image.placement.visible || image.width == 0 || image.height == 0 {
            return;
        }
        let cx = self.width as f32 * (0.5 + image.placement.x / 100.0);
        let cy = self.height as f32 * (0.5 - image.placement.y / 100.0);
        let w = self.width as f32 * image.placement.scale_x;
        let h = self.height as f32 * image.placement.scale_y;
        let (sin, cos) = image.placement.rotation_radians.sin_cos();
        for y in 0..self.height {
            for x in 0..self.width {
                let dx = x as f32 + 0.5 - cx;
                let dy = y as f32 + 0.5 - cy;
                let lx = dx * cos + dy * sin;
                let ly = -dx * sin + dy * cos;
                let u = lx / w + 0.5;
                let v = ly / h + 0.5;
                if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                    continue;
                }
                let sx = (u * image.width as f32).floor() as u32;
                let sy = (v * image.height as f32).floor() as u32;
                let source = ((sy * image.width + sx) * 4) as usize;
                let alpha = ((image.rgba[source + 3] as f32 * image.placement.opacity).round()
                    as u8)
                    .min(image.rgba[source + 3]);
                self.blend(
                    x,
                    y,
                    Rgba(
                        image.rgba[source],
                        image.rgba[source + 1],
                        image.rgba[source + 2],
                        alpha,
                    ),
                );
            }
        }
    }

    fn fill_rect(&mut self, rect: Rect, color: Rgba) {
        let (x0, x1) = bounds(rect.min.x, rect.max.x, self.width);
        let (y0, y1) = bounds(rect.min.y, rect.max.y, self.height);
        for y in y0..y1 {
            for x in x0..x1 {
                self.blend(x, y, color);
            }
        }
    }

    fn circle(&mut self, center: Vec2, radius: f32, color: Rgba) {
        if !center.x.is_finite() || !center.y.is_finite() || !radius.is_finite() || radius <= 0.0 {
            return;
        }
        let (x0, x1) = bounds(center.x - radius, center.x + radius + 1.0, self.width);
        let (y0, y1) = bounds(center.y - radius, center.y + radius + 1.0, self.height);
        let rr = radius * radius;
        for y in y0..y1 {
            for x in x0..x1 {
                let dx = x as f32 + 0.5 - center.x;
                let dy = y as f32 + 0.5 - center.y;
                if dx * dx + dy * dy <= rr {
                    self.blend(x, y, color);
                }
            }
        }
    }

    /// Matches `write_symbol_svg`'s geometry (see its doc comment) for
    /// `Circle`/`Square`/`Cross`. `Triangle`/`Diamond`/`Star` fall back to a
    /// circle here: this rasterizer has no polygon fill primitive yet, and a
    /// video frame or PDF raster preview reads fine as a circle in the
    /// meantime. `egui_backend`'s CPU path (the on-screen editor) is the one
    /// that must render every shape correctly.
    fn symbol(&mut self, center: Vec2, radius: f32, fill: Rgba, stroke: Rgba, symbol: Symbol) {
        match symbol {
            Symbol::Square => {
                let half = radius * 0.8;
                let outer = Rect {
                    min: Vec2 {
                        x: center.x - half - 1.0,
                        y: center.y - half - 1.0,
                    },
                    max: Vec2 {
                        x: center.x + half + 1.0,
                        y: center.y + half + 1.0,
                    },
                };
                let inner = Rect {
                    min: Vec2 {
                        x: center.x - half,
                        y: center.y - half,
                    },
                    max: Vec2 {
                        x: center.x + half,
                        y: center.y + half,
                    },
                };
                self.fill_rect(outer, stroke);
                self.fill_rect(inner, fill);
            }
            Symbol::Cross => {
                let arm = radius;
                let width = (radius * 0.4).max(1.0);
                self.line(
                    Vec2 {
                        x: center.x - arm,
                        y: center.y - arm,
                    },
                    Vec2 {
                        x: center.x + arm,
                        y: center.y + arm,
                    },
                    width,
                    stroke,
                );
                self.line(
                    Vec2 {
                        x: center.x - arm,
                        y: center.y + arm,
                    },
                    Vec2 {
                        x: center.x + arm,
                        y: center.y - arm,
                    },
                    width,
                    stroke,
                );
            }
            Symbol::Circle | Symbol::Triangle | Symbol::Diamond | Symbol::Star => {
                self.circle(center, radius + 1.0, stroke);
                self.circle(center, radius, fill);
            }
        }
    }

    fn line(&mut self, a: Vec2, b: Vec2, width: f32, color: Rgba) {
        if !a.x.is_finite()
            || !a.y.is_finite()
            || !b.x.is_finite()
            || !b.y.is_finite()
            || !width.is_finite()
            || width <= 0.0
        {
            return;
        }
        let radius = width * 0.5;
        let (x0, x1) = bounds(
            a.x.min(b.x) - radius,
            a.x.max(b.x) + radius + 1.0,
            self.width,
        );
        let (y0, y1) = bounds(
            a.y.min(b.y) - radius,
            a.y.max(b.y) + radius + 1.0,
            self.height,
        );
        let vx = b.x - a.x;
        let vy = b.y - a.y;
        let length2 = vx * vx + vy * vy;
        for y in y0..y1 {
            for x in x0..x1 {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let t = if length2 > f32::EPSILON {
                    (((px - a.x) * vx + (py - a.y) * vy) / length2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let dx = px - (a.x + t * vx);
                let dy = py - (a.y + t * vy);
                if dx * dx + dy * dy <= radius * radius {
                    self.blend(x, y, color);
                }
            }
        }
    }

    fn blend(&mut self, x: u32, y: u32, Rgba(r, g, b, a): Rgba) {
        let index = ((y as usize * self.width as usize) + x as usize) * 4;
        let alpha = u32::from(a);
        let inverse = 255 - alpha;
        for (offset, source) in [r, g, b].into_iter().enumerate() {
            self.rgba[index + offset] =
                ((u32::from(source) * alpha + u32::from(self.rgba[index + offset]) * inverse + 127)
                    / 255) as u8;
        }
        self.rgba[index + 3] =
            (alpha + (u32::from(self.rgba[index + 3]) * inverse + 127) / 255).min(255) as u8;
    }
}

fn bounds(start: f32, end: f32, limit: u32) -> (u32, u32) {
    let start = if start.is_finite() {
        start.floor().max(0.0).min(limit as f32) as u32
    } else {
        0
    };
    let end = if end.is_finite() {
        end.ceil().max(0.0).min(limit as f32) as u32
    } else {
        0
    };
    (start.min(end), end.max(start))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CountRange {
    pub start: f64,
    pub end: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameTime {
    pub index: u64,
    pub rate_numerator: u32,
    pub rate_denominator: u32,
}

impl FrameTime {
    pub fn seconds(self) -> f64 {
        self.index as f64 * f64::from(self.rate_denominator) / f64::from(self.rate_numerator)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportState {
    Preparing,
    Rendering,
    Encoding,
    Verifying,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportStateMachine {
    state: ExportState,
}

impl Default for ExportStateMachine {
    fn default() -> Self {
        Self {
            state: ExportState::Preparing,
        }
    }
}

impl ExportStateMachine {
    pub fn state(self) -> ExportState {
        self.state
    }

    pub fn transition(&mut self, next: ExportState) -> Result<(), ExportError> {
        let valid = matches!(
            (self.state, next),
            (ExportState::Preparing, ExportState::Rendering)
                | (ExportState::Rendering, ExportState::Encoding)
                | (ExportState::Encoding, ExportState::Verifying)
                | (ExportState::Verifying, ExportState::Completed)
                | (_, ExportState::Cancelled | ExportState::Failed)
        );
        if !valid {
            return Err(ExportError::InvalidStateTransition);
        }
        self.state = next;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExportError {
    InvalidRange,
    InvalidConfig(String),
    OutputExists,
    Io(String),
    EncoderUnavailable(String),
    EncoderFailed(String),
    ProbeFailed(String),
    InstallFailed { partial: PathBuf, reason: String },
    InvalidFrameSize,
    Cancelled,
    InvalidStateTransition,
    UnderlayMissing,
    UnderlayInvalid,
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ExportError {}

#[derive(Clone, Debug)]
pub struct VideoExportRequest {
    pub document: Document,
    pub config: VideoExportConfig,
    pub range: CountRange,
    pub output: PathBuf,
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub audio: Option<PathBuf>,
    /// Encoded PNG/JPEG bytes resolved when the export snapshot is created.
    pub underlay: Option<Vec<u8>>,
    /// True selects the camera/3D path, where underlays are always excluded.
    pub use_3d_camera: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportReport {
    pub frames: u64,
    pub used_software_fallback: bool,
    pub output: PathBuf,
    /// Previous output retained during an approved replacement.
    pub backup: Option<PathBuf>,
}

/// Result of the non-destructive checks shown before a potentially long export.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportPreflight {
    pub ffmpeg: Option<PathBuf>,
    pub ffprobe: Option<PathBuf>,
    pub encoders: Vec<EncoderAvailability>,
    pub frames: u64,
    pub estimated_bytes: u64,
    pub estimated_time: Duration,
    pub issues: Vec<PreflightIssue>,
}

impl ExportPreflight {
    #[must_use]
    pub fn can_export(&self) -> bool {
        !self.issues.iter().any(|issue| issue.blocking)
    }

    #[must_use]
    pub fn backend(&self, backend: EncoderBackend) -> Option<&EncoderAvailability> {
        self.encoders.iter().find(|item| item.backend == backend)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncoderAvailability {
    pub backend: EncoderBackend,
    pub available: bool,
    /// Stable, user-presentable explanation; never just a boolean.
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreflightIssue {
    pub blocking: bool,
    pub message: String,
}

pub fn underlay_preflight(request: &VideoExportRequest) -> Option<PreflightIssue> {
    let model = request.document.underlay.as_ref()?;
    if request.use_3d_camera
        || model.placement.render_policy == drill_core::underlay::UnderlayRenderPolicy::Editor2dOnly
        || !model.placement.visible
    {
        return None;
    }
    Some(if request.underlay.is_none() {
        PreflightIssue {
            message: "The opted-in image underlay is missing; relink or disable it before export"
                .into(),
            blocking: true,
        }
    } else {
        PreflightIssue {
            message: "The image underlay will be composited into this 2D export".into(),
            blocking: false,
        }
    })
}

/// Camera-specific preflight checks. Warnings are non-blocking because the
/// exporter can deliberately fall back to the plan view.
#[must_use]
pub fn camera_preflight_issues(document: &Document, range: CountRange) -> Vec<PreflightIssue> {
    let program = &document.camera_program;
    let mut issues = Vec::new();
    if program.tracks.is_empty() || program.cuts.is_empty() {
        issues.push(PreflightIssue {
            blocking: false,
            message: "カメラショットがないため、2Dプランビューで書き出します".into(),
        });
        return issues;
    }
    if program.evaluate(range.start as f32).is_none() {
        issues.push(PreflightIssue {
            blocking: false,
            message: "書き出し開始位置を評価できるカメラキーフレームがありません".into(),
        });
    }
    if !program
        .cuts
        .iter()
        .any(|cut| f64::from(cut.count) >= range.start && f64::from(cut.count) < range.end)
    {
        issues.push(PreflightIssue {
            blocking: false,
            message: "範囲内にカメラカットはありません（単一ショットとして書き出します）".into(),
        });
    }
    issues
}

/// Locates tools and asks FFmpeg for its actual encoder list. Intended for a
/// background job: process creation must never block the UI thread.
#[must_use]
pub fn inspect_export_environment(
    config: &VideoExportConfig,
    duration_seconds: f64,
    output: Option<&Path>,
) -> ExportPreflight {
    let ffmpeg = find_executable("ffmpeg");
    let ffprobe = find_executable("ffprobe");
    let encoder_text = ffmpeg
        .as_ref()
        .and_then(|program| {
            Command::new(program)
                .args(["-hide_banner", "-encoders"])
                .output()
                .ok()
        })
        .filter(|result| result.status.success())
        .map(|result| String::from_utf8_lossy(&result.stdout).into_owned())
        .unwrap_or_default();
    preflight_from_encoder_text(
        config,
        duration_seconds,
        output,
        ffmpeg,
        ffprobe,
        &encoder_text,
    )
}

fn preflight_from_encoder_text(
    config: &VideoExportConfig,
    duration_seconds: f64,
    output: Option<&Path>,
    ffmpeg: Option<PathBuf>,
    ffprobe: Option<PathBuf>,
    encoders_text: &str,
) -> ExportPreflight {
    let encoder = |backend, names: &[&str], label: &str| {
        let found = names.iter().any(|name| encoders_text.contains(name));
        EncoderAvailability {
            backend,
            available: found,
            reason: if ffmpeg.is_none() {
                "FFmpegが見つかりません".into()
            } else if found {
                format!("{label}エンコーダーをFFmpegで確認済み")
            } else {
                format!("このFFmpegには{label}エンコーダーが含まれていません")
            },
        }
    };
    let software_names: &[&str] = match config.codec {
        drill_core::video::VideoCodec::H264 => &["libx264"],
        drill_core::video::VideoCodec::H265 => &["libx265"],
        drill_core::video::VideoCodec::Av1 => &["libsvtav1", "libaom-av1"],
        drill_core::video::VideoCodec::Vp9 => &["libvpx-vp9"],
    };
    let nvidia_names: &[&str] = match config.codec {
        drill_core::video::VideoCodec::H264 => &["h264_nvenc"],
        drill_core::video::VideoCodec::H265 => &["hevc_nvenc"],
        _ => &[],
    };
    let intel_names: &[&str] = match config.codec {
        drill_core::video::VideoCodec::H264 => &["h264_qsv"],
        drill_core::video::VideoCodec::H265 => &["hevc_qsv"],
        _ => &[],
    };
    let amd_names: &[&str] = match config.codec {
        drill_core::video::VideoCodec::H264 => &["h264_amf"],
        drill_core::video::VideoCodec::H265 => &["hevc_amf"],
        _ => &[],
    };
    let encoders = vec![
        encoder(EncoderBackend::Software, software_names, "CPU"),
        encoder(EncoderBackend::Nvidia, nvidia_names, "NVIDIA"),
        encoder(EncoderBackend::Intel, intel_names, "Intel Quick Sync"),
        encoder(EncoderBackend::Amd, amd_names, "AMD"),
    ];
    let safe_duration = if duration_seconds.is_finite() && duration_seconds > 0.0 {
        duration_seconds
    } else {
        0.0
    };
    let frames = (safe_duration * f64::from(config.fps)).ceil() as u64;
    let estimated_bytes =
        (f64::from(config.estimated_megabytes(safe_duration as f32)) * 1_048_576.0).ceil() as u64;
    // Deliberately conservative throughput estimates; this is guidance, not a promise.
    let megapixels = f64::from(config.width) * f64::from(config.height) / 1_000_000.0;
    let frames_per_second = match config.backend {
        EncoderBackend::Software => 45.0 / megapixels.max(0.25),
        EncoderBackend::Auto
        | EncoderBackend::Nvidia
        | EncoderBackend::Intel
        | EncoderBackend::Amd => 110.0 / megapixels.max(0.25),
    }
    .clamp(2.0, 240.0);
    let estimated_time = Duration::from_secs_f64((frames as f64 / frames_per_second).max(1.0));
    let mut issues = Vec::new();
    if let Err(reason) = config.validate() {
        issues.push(PreflightIssue {
            blocking: true,
            message: reason.to_string(),
        });
    }
    if safe_duration == 0.0 {
        issues.push(PreflightIssue {
            blocking: true,
            message: "書き出し範囲が空です".into(),
        });
    }
    if ffmpeg.is_none() {
        issues.push(PreflightIssue {
            blocking: true,
            message: "FFmpegが見つかりません。インストール後に再検査してください".into(),
        });
    }
    if ffprobe.is_none() {
        issues.push(PreflightIssue {
            blocking: true,
            message: "ffprobeが見つからないため、完成動画を検証できません".into(),
        });
    }
    if let Some(path) = output {
        match path.parent() {
            Some(parent) if parent.is_dir() => {}
            _ => issues.push(PreflightIssue {
                blocking: true,
                message: "保存先フォルダーが存在しません".into(),
            }),
        }
    }
    let selected_available = match config.backend {
        EncoderBackend::Auto => encoders.iter().any(|entry| entry.available),
        backend => encoders
            .iter()
            .any(|entry| entry.backend == backend && entry.available),
    };
    if ffmpeg.is_some() && !selected_available {
        issues.push(PreflightIssue {
            blocking: true,
            message: "選択したエンコーダーは利用できません。自動またはCPUを選んでください".into(),
        });
    }
    ExportPreflight {
        ffmpeg,
        ffprobe,
        encoders,
        frames,
        estimated_bytes,
        estimated_time,
        issues,
    }
}

fn find_executable(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .flat_map(|directory| [directory.join(format!("{name}.exe")), directory.join(name)])
        .find(|candidate| candidate.is_file())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReplacePolicy {
    #[default]
    Refuse,
    /// The caller has already obtained explicit overwrite approval.
    BackupAndReplace,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EncoderProgress {
    pub frame: Option<u64>,
    pub out_time_micros: Option<u64>,
    pub ended: bool,
}

pub fn parse_ffmpeg_progress(text: &str) -> EncoderProgress {
    let mut result = EncoderProgress::default();
    for line in text.lines() {
        let Some((key, value)) = line.trim().split_once('=') else {
            continue;
        };
        match key {
            "frame" => result.frame = value.parse().ok(),
            "out_time_us" | "out_time_ms" => result.out_time_micros = value.parse().ok(),
            "progress" if value == "end" => result.ended = true,
            _ => {}
        }
    }
    result
}

pub struct FrameSchedule<'a> {
    document: &'a Document,
    start_seconds: f64,
    end_count: f64,
    fps: u32,
    next: u64,
    total: u64,
}

impl FrameSchedule<'_> {
    pub fn total_frames(&self) -> u64 {
        self.total
    }
}

impl Iterator for FrameSchedule<'_> {
    type Item = (FrameTime, f64);

    fn next(&mut self) -> Option<Self::Item> {
        if self.next >= self.total {
            return None;
        }
        let time = FrameTime {
            index: self.next,
            rate_numerator: self.fps,
            rate_denominator: 1,
        };
        self.next += 1;
        let count = self
            .document
            .tempo
            .count_at_f64(self.start_seconds + time.seconds())
            .min(self.end_count);
        Some((time, count))
    }
}

pub fn frame_schedule<'a>(
    document: &'a Document,
    range: CountRange,
    fps: u32,
) -> Result<FrameSchedule<'a>, ExportError> {
    if !range.start.is_finite()
        || !range.end.is_finite()
        || range.start < 0.0
        || range.end <= range.start
        || range.end > f64::from(document.timeline_counts())
        || fps == 0
    {
        return Err(ExportError::InvalidRange);
    }
    let start_seconds = document.tempo.seconds_at_f64(range.start);
    let duration = document.tempo.seconds_at_f64(range.end) - start_seconds;
    let frames = (duration * f64::from(fps)).ceil() as u64;
    Ok(FrameSchedule {
        document,
        start_seconds,
        end_count: range.end,
        fps,
        next: 0,
        total: frames,
    })
}

pub trait FrameWriter {
    fn write_rgba(&mut self, frame: &[u8]) -> Result<(), ExportError>;
    fn finish(&mut self) -> Result<(), ExportError>;
    fn cancel(&mut self);
}

pub trait ExportControl {
    fn set_progress(&self, fraction: f32);
    fn is_cancelled(&self) -> bool;
}

impl ExportControl for ProgressHandle {
    fn set_progress(&self, fraction: f32) {
        self.set(fraction);
    }

    fn is_cancelled(&self) -> bool {
        self.is_cancelled()
    }
}

pub fn render_frames<W: FrameWriter, C: ExportControl>(
    request: &VideoExportRequest,
    writer: &mut W,
    progress: &C,
) -> Result<u64, ExportError> {
    request
        .config
        .validate()
        .map_err(|error| ExportError::InvalidConfig(error.to_string()))?;
    let schedule = frame_schedule(&request.document, request.range, request.config.fps)?;
    let total_frames = schedule.total_frames();
    let mut positions = Vec::<Point>::with_capacity(request.document.performers.len());
    let mut display = DisplayList::new();
    let mut build_scratch = BuildScratch;
    let mut surface = RasterSurface::default();
    surface
        .resize(request.config.width, request.config.height)
        .map_err(|_| ExportError::InvalidFrameSize)?;
    let options = RenderOptions::default();
    let resolved_underlay = match request.document.underlay.as_ref() {
        Some(model)
            if model.placement.render_policy
                == drill_core::underlay::UnderlayRenderPolicy::EditorAnd2dExport =>
        {
            let bytes = request
                .underlay
                .as_deref()
                .ok_or(ExportError::UnderlayMissing)?;
            let decoded = drill_interop::underlay::decode_underlay(
                bytes,
                drill_interop::underlay::UnderlayLimits::default(),
            )
            .map_err(|_| ExportError::UnderlayInvalid)?;
            if decoded
                .source_hash
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
                != model.content_hash
            {
                return Err(ExportError::UnderlayInvalid);
            }
            Some(ResolvedUnderlay {
                width: decoded.width,
                height: decoded.height,
                rgba: decoded.rgba,
                placement: model.placement.clone(),
            })
        }
        _ => None,
    };
    for (frame_index, (_, count)) in schedule.enumerate() {
        if progress.is_cancelled() {
            writer.cancel();
            return Err(ExportError::Cancelled);
        }
        let (set_index, local_count) = request.document.locate_count(count as f32);
        let counts = request
            .document
            .sets
            .get(set_index)
            .map_or(1.0, |set| f32::from(set.counts).max(1.0));
        request
            .document
            .positions_at(set_index, local_count / counts, &mut positions);
        let scene = Scene {
            document: &request.document,
            positions: &positions,
            viewport: Viewport {
                size: Vec2 {
                    x: request.config.width as f32,
                    y: request.config.height as f32,
                },
                ui_scale: 1.0,
            },
            options: &options,
            theme: &Theme::PRINT_LIGHT,
        };
        if request.use_3d_camera
            && let Some(camera) = request.document.camera_program.evaluate(count as f32)
        {
            build_field_camera(&scene, &camera, &mut display);
        } else {
            build_field_2d(&scene, &mut build_scratch, &mut display);
        }
        surface.render_with_underlay(
            &display,
            (!request.use_3d_camera)
                .then_some(resolved_underlay.as_ref())
                .flatten(),
        );
        writer.write_rgba(surface.pixels())?;
        progress.set_progress((frame_index + 1) as f32 / total_frames.max(1) as f32);
    }
    writer.finish()?;
    Ok(total_frames)
}

struct FfmpegWriter {
    child: Child,
    stdin: Option<ChildStdin>,
}

impl FfmpegWriter {
    fn spawn(request: &VideoExportRequest, partial: &Path) -> Result<Self, ExportError> {
        let audio = request.audio.as_ref().map(|path| path.to_string_lossy());
        let output = partial.to_string_lossy();
        let args = request
            .config
            .ffmpeg_args(audio.as_deref(), &output)
            .map_err(|error| ExportError::InvalidConfig(error.to_string()))?;
        let mut child = Command::new(&request.ffmpeg)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            // Never leave an unread pipe that can fill and deadlock a long
            // encode. Diagnostics are reported through the exit status; a
            // future log collector may drain stderr on its own thread.
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| ExportError::EncoderUnavailable(error.to_string()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ExportError::EncoderUnavailable("FFmpeg stdin is unavailable".into()))?;
        Ok(Self {
            child,
            stdin: Some(stdin),
        })
    }
}

impl FrameWriter for FfmpegWriter {
    fn write_rgba(&mut self, frame: &[u8]) -> Result<(), ExportError> {
        self.stdin
            .as_mut()
            .ok_or_else(|| ExportError::EncoderFailed("encoder stdin closed".into()))?
            .write_all(frame)
            .map_err(|error| ExportError::EncoderFailed(error.to_string()))
    }

    fn finish(&mut self) -> Result<(), ExportError> {
        self.stdin.take();
        let status = self
            .child
            .wait()
            .map_err(|error| ExportError::EncoderFailed(error.to_string()))?;
        if status.success() {
            Ok(())
        } else {
            Err(ExportError::EncoderFailed(format!("status {status}")))
        }
    }

    fn cancel(&mut self) {
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn partial_path(output: &Path) -> PathBuf {
    let stem = output
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("video");
    let extension = output
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("mp4");
    let sequence = PARTIAL_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    output.with_file_name(format!(
        ".{stem}.partial-{}-{sequence}.{extension}",
        std::process::id()
    ))
}

fn backup_path(output: &Path) -> PathBuf {
    let stem = output
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("video");
    let extension = output
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("mp4");
    let sequence = PARTIAL_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    output.with_file_name(format!(
        ".{stem}.backup-{}-{sequence}.{extension}",
        std::process::id()
    ))
}

fn install_verified_partial(
    partial: &Path,
    output: &Path,
    policy: ReplacePolicy,
) -> Result<Option<PathBuf>, ExportError> {
    if output.exists() && policy == ReplacePolicy::Refuse {
        return Err(ExportError::OutputExists);
    }
    let backup = output.is_file().then(|| backup_path(output));
    let report =
        drill_project::install_file(partial, output, backup.as_deref()).map_err(|error| {
            ExportError::InstallFailed {
                partial: partial.to_path_buf(),
                reason: error.to_string(),
            }
        })?;
    Ok(report.backup)
}

fn verify_video(ffprobe: &Path, output: &Path) -> Result<(), ExportError> {
    let result = Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=codec_type",
            "-of",
            "default=nw=1:nk=1",
        ])
        .arg(output)
        .output()
        .map_err(|error| ExportError::ProbeFailed(error.to_string()))?;
    if result.status.success() && String::from_utf8_lossy(&result.stdout).trim() == "video" {
        Ok(())
    } else {
        Err(ExportError::ProbeFailed(
            String::from_utf8_lossy(&result.stderr).into_owned(),
        ))
    }
}

fn run_attempt(
    request: &VideoExportRequest,
    partial: &Path,
    progress: &ProgressHandle,
) -> Result<u64, ExportError> {
    let mut writer = FfmpegWriter::spawn(request, partial)?;
    render_frames(request, &mut writer, progress)
}

fn should_fallback(error: &ExportError, backend: EncoderBackend) -> bool {
    matches!(
        error,
        ExportError::EncoderFailed(_) | ExportError::EncoderUnavailable(_)
    ) && !matches!(backend, EncoderBackend::Auto | EncoderBackend::Software)
}

fn discard_partial(path: &Path) {
    let _ = std::fs::remove_file(path);
}

pub fn run_video_export(
    request: VideoExportRequest,
    progress: &ProgressHandle,
) -> Result<ExportReport, ExportError> {
    run_video_export_with_policy(request, progress, ReplacePolicy::Refuse)
}

pub fn run_video_export_with_policy(
    mut request: VideoExportRequest,
    progress: &ProgressHandle,
    replace_policy: ReplacePolicy,
) -> Result<ExportReport, ExportError> {
    if request.output.exists() && replace_policy == ReplacePolicy::Refuse {
        return Err(ExportError::OutputExists);
    }
    let partial = partial_path(&request.output);
    discard_partial(&partial);
    let original_backend = request.config.backend;
    let first = run_attempt(&request, &partial, progress);
    let (frames, used_software_fallback) = match first {
        Ok(frames) => (frames, false),
        Err(error) if should_fallback(&error, original_backend) => {
            discard_partial(&partial);
            request.config.backend = EncoderBackend::Software;
            match run_attempt(&request, &partial, progress) {
                Ok(frames) => (frames, true),
                Err(error) => {
                    discard_partial(&partial);
                    return Err(error);
                }
            }
        }
        Err(error) => {
            discard_partial(&partial);
            return Err(error);
        }
    };
    if progress.is_cancelled() {
        discard_partial(&partial);
        return Err(ExportError::Cancelled);
    }
    if let Err(error) = verify_video(&request.ffprobe, &partial) {
        discard_partial(&partial);
        return Err(error);
    }
    let backup = install_verified_partial(&partial, &request.output, replace_policy)?;
    Ok(ExportReport {
        frames,
        used_software_fallback,
        output: request.output,
        backup,
    })
}

pub fn spawn_video_export(request: VideoExportRequest) -> Job<ExportReport> {
    spawn_video_export_with_policy(request, ReplacePolicy::Refuse)
}

pub fn spawn_video_export_with_policy(
    request: VideoExportRequest,
    replace_policy: ReplacePolicy,
) -> Job<ExportReport> {
    Job::spawn_typed(JobKind::ExportVideo, move |progress| {
        run_video_export_with_policy(request, progress, replace_policy)
            .map_err(|_| JobFailure::new(JobErrorCode::External))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use drill_core::Document;
    use drill_render::{BuildScratch, RenderOptions, Scene, Theme, Viewport, build_field_2d};

    #[test]
    fn preflight_explains_encoder_availability_and_estimates_work() {
        let config = VideoExportConfig::default();
        let report = preflight_from_encoder_text(
            &config,
            10.0,
            None,
            Some(PathBuf::from("ffmpeg")),
            Some(PathBuf::from("ffprobe")),
            " V..... libx264 H.264\n V..... h264_nvenc NVIDIA",
        );
        assert!(report.can_export());
        assert_eq!(report.frames, 600);
        assert!(report.estimated_bytes > 0);
        assert!(report.estimated_time >= Duration::from_secs(1));
        assert!(report.backend(EncoderBackend::Software).unwrap().available);
        assert!(report.backend(EncoderBackend::Nvidia).unwrap().available);
        assert!(!report.backend(EncoderBackend::Intel).unwrap().available);
        assert!(
            report
                .backend(EncoderBackend::Intel)
                .unwrap()
                .reason
                .contains("含まれていません")
        );
    }

    #[test]
    fn preflight_blocks_missing_tools_and_empty_range() {
        let report =
            preflight_from_encoder_text(&VideoExportConfig::default(), 0.0, None, None, None, "");
        assert!(!report.can_export());
        assert_eq!(report.frames, 0);
        assert!(report.issues.iter().filter(|issue| issue.blocking).count() >= 3);
    }

    #[test]
    fn deterministic_and_reuses_pixel_allocation() {
        let document = Document::demo(10, 10);
        let positions = &document.sets[0].positions;
        let mut list = DisplayList::new();
        let mut scratch = BuildScratch;
        build_field_2d(
            &Scene {
                document: &document,
                positions,
                viewport: Viewport {
                    size: Vec2 { x: 640.0, y: 360.0 },
                    ui_scale: 1.0,
                },
                options: &RenderOptions::default(),
                theme: &Theme::SCREEN_DARK,
            },
            &mut scratch,
            &mut list,
        );
        let mut surface = RasterSurface::default();
        surface.resize(640, 360).unwrap();
        let capacity = surface.capacity();
        surface.render(&list);
        let first = surface.pixels().to_vec();
        surface.render(&list);
        assert_eq!(first, surface.pixels());
        assert_eq!(capacity, surface.capacity());
    }

    fn tiny_png() -> Vec<u8> {
        use image::ImageEncoder as _;
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(&[255, 0, 0, 255], 1, 1, image::ExtendedColorType::Rgba8)
            .unwrap();
        bytes
    }

    fn attach_export_underlay(request: &mut VideoExportRequest, bytes: &[u8]) {
        let decoded = drill_interop::underlay::decode_underlay(
            bytes,
            drill_interop::underlay::UnderlayLimits::default(),
        )
        .unwrap();
        request.document.underlay = Some(drill_core::underlay::ImageUnderlay {
            content_hash: decoded
                .source_hash
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
            byte_len: bytes.len() as u64,
            original_name: "red.png".into(),
            external_path: None,
            placement: drill_core::underlay::UnderlayPlacement {
                opacity: 0.7,
                render_policy: drill_core::underlay::UnderlayRenderPolicy::EditorAnd2dExport,
                ..Default::default()
            },
        });
        request.underlay = Some(bytes.to_vec());
    }

    #[test]
    fn underlay_is_opt_in_deterministic_and_excluded_from_3d() {
        let bytes = tiny_png();
        let mut plain = mock_request();
        let mut with = plain.clone();
        attach_export_underlay(&mut with, &bytes);
        let mut a = MockWriter::default();
        let mut b = MockWriter::default();
        render_frames(&plain, &mut a, &MockControl::default()).unwrap();
        render_frames(&with, &mut b, &MockControl::default()).unwrap();
        assert_ne!(a.frames[0], b.frames[0]);
        let mut again = MockWriter::default();
        render_frames(&with, &mut again, &MockControl::default()).unwrap();
        assert_eq!(b.frames, again.frames);

        plain.use_3d_camera = true;
        with.use_3d_camera = true;
        let mut c = MockWriter::default();
        let mut d = MockWriter::default();
        render_frames(&plain, &mut c, &MockControl::default()).unwrap();
        render_frames(&with, &mut d, &MockControl::default()).unwrap();
        assert_eq!(c.frames, d.frames);

        let svg = set_svg_with_underlay(&with.document, 0, Some(&bytes)).unwrap();
        assert!(svg.contains("id=\"drillforge-underlay\""));
        assert_eq!(
            svg,
            set_svg_with_underlay(&with.document, 0, Some(&bytes)).unwrap()
        );
    }

    #[test]
    fn rejects_unbounded_images() {
        let mut surface = RasterSurface::default();
        assert_eq!(surface.resize(0, 10), Err(RasterError::InvalidDimensions));
        assert_eq!(
            surface.resize(100_000, 100_000),
            Err(RasterError::ImageTooLarge)
        );
    }

    #[derive(Default)]
    struct MockControl {
        cancelled: std::cell::Cell<bool>,
        progress: std::cell::Cell<f32>,
    }

    impl ExportControl for MockControl {
        fn set_progress(&self, fraction: f32) {
            self.progress.set(fraction);
        }
        fn is_cancelled(&self) -> bool {
            self.cancelled.get()
        }
    }

    #[derive(Default)]
    struct MockWriter {
        frames: Vec<Vec<u8>>,
        finished: bool,
        cancelled: bool,
    }

    impl FrameWriter for MockWriter {
        fn write_rgba(&mut self, frame: &[u8]) -> Result<(), ExportError> {
            self.frames.push(frame.to_vec());
            Ok(())
        }
        fn finish(&mut self) -> Result<(), ExportError> {
            self.finished = true;
            Ok(())
        }
        fn cancel(&mut self) {
            self.cancelled = true;
        }
    }

    fn mock_request() -> VideoExportRequest {
        let config = VideoExportConfig {
            width: 64,
            height: 36,
            fps: 10,
            audio_enabled: false,
            ..VideoExportConfig::default()
        };
        VideoExportRequest {
            document: Document::demo(2, 2),
            config,
            range: CountRange {
                start: 0.0,
                end: 2.0,
            },
            output: "unused.mp4".into(),
            ffmpeg: "ffmpeg".into(),
            ffprobe: "ffprobe".into(),
            audio: None,
            underlay: None,
            use_3d_camera: false,
        }
    }

    #[test]
    fn rational_schedule_crosses_variable_tempo_without_accumulation() {
        let mut document = Document::demo(1, 1);
        document.tempo.set(1.0, 60.0);
        let schedule = frame_schedule(
            &document,
            CountRange {
                start: 0.0,
                end: 2.0,
            },
            10,
        )
        .unwrap()
        .collect::<Vec<_>>();
        assert_eq!(schedule.len(), 15);
        assert_eq!(
            schedule[7].0,
            FrameTime {
                index: 7,
                rate_numerator: 10,
                rate_denominator: 1
            }
        );
        assert!((schedule[7].1 - 1.2).abs() < 1e-12);
    }

    #[test]
    fn mock_writer_receives_deterministic_rgba_frames() {
        let request = mock_request();
        let control = MockControl::default();
        let mut first = MockWriter::default();
        let frames = render_frames(&request, &mut first, &control).unwrap();
        let mut second = MockWriter::default();
        render_frames(&request, &mut second, &control).unwrap();
        assert_eq!(frames, 10);
        assert!(first.finished);
        assert_eq!(first.frames, second.frames);
        assert!(first.frames.iter().all(|frame| frame.len() == 64 * 36 * 4));
        assert_eq!(control.progress.get(), 1.0);
    }

    #[test]
    fn offline_camera_program_changes_pixels_deterministically() {
        let mut first_request = mock_request();
        first_request.use_3d_camera = true;
        let control = MockControl::default();
        let mut first = MockWriter::default();
        render_frames(&first_request, &mut first, &control).unwrap();

        let mut second_request = first_request.clone();
        let id = second_request.document.camera_program.tracks[0].id;
        second_request.document.camera_program.tracks[0]
            .insert_keyframe(drill_core::camera::CameraKeyframe::from_camera(
                0.0,
                drill_core::camera::Camera::overhead(&second_request.document.grid),
            ))
            .unwrap();
        assert_eq!(second_request.document.camera_program.cuts[0].camera, id);
        let mut second = MockWriter::default();
        render_frames(&second_request, &mut second, &control).unwrap();
        assert_ne!(first.frames[0], second.frames[0]);

        let mut repeated = MockWriter::default();
        render_frames(&second_request, &mut repeated, &control).unwrap();
        assert_eq!(second.frames, repeated.frames);
    }

    #[test]
    fn camera_preflight_explains_plan_view_fallback() {
        let mut request = mock_request();
        request.document.camera_program.tracks.clear();
        request.document.camera_program.cuts.clear();
        let issues = camera_preflight_issues(&request.document, request.range);
        assert_eq!(issues.len(), 1);
        assert!(!issues[0].blocking);
        assert!(issues[0].message.contains("2D"));
    }

    #[test]
    fn cancellation_stops_before_writing_and_notifies_sink() {
        let request = mock_request();
        let control = MockControl::default();
        control.cancelled.set(true);
        let mut writer = MockWriter::default();
        assert_eq!(
            render_frames(&request, &mut writer, &control),
            Err(ExportError::Cancelled)
        );
        assert!(writer.cancelled);
        assert!(writer.frames.is_empty());
    }

    #[test]
    fn parses_progress_and_rejects_invalid_state_transition() {
        assert_eq!(
            parse_ffmpeg_progress("frame=42\nout_time_us=123456\nprogress=end\n"),
            EncoderProgress {
                frame: Some(42),
                out_time_micros: Some(123_456),
                ended: true
            }
        );
        let mut state = ExportStateMachine::default();
        assert!(state.transition(ExportState::Rendering).is_ok());
        assert_eq!(
            state.transition(ExportState::Completed),
            Err(ExportError::InvalidStateTransition)
        );
    }

    #[test]
    fn partial_output_keeps_container_extension() {
        let path = partial_path(Path::new("show.mp4"));
        assert_eq!(
            path.extension().and_then(|value| value.to_str()),
            Some("mp4")
        );
        assert!(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .contains("partial")
        );
    }

    #[test]
    fn fallback_is_limited_to_hardware_encoder_failures() {
        let failed = ExportError::EncoderFailed("device lost".into());
        let unavailable = ExportError::EncoderUnavailable("missing".into());
        assert!(should_fallback(&failed, EncoderBackend::Nvidia));
        assert!(should_fallback(&unavailable, EncoderBackend::Intel));
        assert!(!should_fallback(&failed, EncoderBackend::Auto));
        assert!(!should_fallback(&failed, EncoderBackend::Software));
        assert!(!should_fallback(
            &ExportError::Cancelled,
            EncoderBackend::Nvidia
        ));
    }

    fn install_test_directory(name: &str) -> PathBuf {
        let sequence = PARTIAL_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "drill-export-{name}-{}-{sequence}",
            std::process::id()
        ))
    }

    #[test]
    fn approved_replace_keeps_old_video_as_backup() {
        let directory = install_test_directory("replace");
        std::fs::create_dir(&directory).unwrap();
        let output = directory.join("show.mp4");
        let partial = directory.join(".show.partial.mp4");
        std::fs::write(&output, b"old").unwrap();
        std::fs::write(&partial, b"verified").unwrap();

        let backup = install_verified_partial(&partial, &output, ReplacePolicy::BackupAndReplace)
            .unwrap()
            .unwrap();

        assert_eq!(std::fs::read(&output).unwrap(), b"verified");
        assert_eq!(std::fs::read(&backup).unwrap(), b"old");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn overwrite_without_approval_preserves_both_files() {
        let directory = install_test_directory("refuse");
        std::fs::create_dir(&directory).unwrap();
        let output = directory.join("show.mp4");
        let partial = directory.join(".show.partial.mp4");
        std::fs::write(&output, b"old").unwrap();
        std::fs::write(&partial, b"verified").unwrap();

        assert_eq!(
            install_verified_partial(&partial, &output, ReplacePolicy::Refuse),
            Err(ExportError::OutputExists)
        );
        assert_eq!(std::fs::read(&output).unwrap(), b"old");
        assert_eq!(std::fs::read(&partial).unwrap(), b"verified");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn install_failure_retains_partial_for_recovery() {
        let directory = install_test_directory("install-failure");
        std::fs::create_dir(&directory).unwrap();
        let output = directory.join("output-directory");
        std::fs::create_dir(&output).unwrap();
        let partial = directory.join(".show.partial.mp4");
        std::fs::write(&partial, b"verified").unwrap();

        assert!(matches!(
            install_verified_partial(&partial, &output, ReplacePolicy::BackupAndReplace),
            Err(ExportError::InstallFailed { .. })
        ));
        assert!(output.is_dir());
        assert_eq!(std::fs::read(&partial).unwrap(), b"verified");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn cancellation_cleanup_discards_partial_without_touching_output() {
        let directory = install_test_directory("cancel-cleanup");
        std::fs::create_dir(&directory).unwrap();
        let output = directory.join("show.mp4");
        let partial = directory.join(".show.partial.mp4");
        std::fs::write(&output, b"old").unwrap();
        std::fs::write(&partial, b"incomplete").unwrap();

        discard_partial(&partial);

        assert_eq!(std::fs::read(&output).unwrap(), b"old");
        assert!(!partial.exists());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
