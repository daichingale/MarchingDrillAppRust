use drill_jobs::{Job, JobErrorCode, JobFailure, JobKind, JobMsg};
use eframe::egui;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UnderlayFailure {
    Io,
    TooLarge,
    Cancelled,
    Unsupported,
    Invalid,
    Dimensions,
    Allocation,
    Worker,
}

impl UnderlayFailure {
    #[cfg(test)]
    fn code(self) -> &'static str {
        match self {
            Self::Io => "io",
            Self::TooLarge => "too-large",
            Self::Cancelled => "cancelled",
            Self::Unsupported => "unsupported",
            Self::Invalid => "invalid",
            Self::Dimensions => "dimensions",
            Self::Allocation => "allocation",
            Self::Worker => "worker",
        }
    }

    #[cfg(test)]
    fn from_code(code: &str) -> Self {
        match code {
            "io" => Self::Io,
            "too-large" => Self::TooLarge,
            "cancelled" => Self::Cancelled,
            "unsupported" => Self::Unsupported,
            "invalid" => Self::Invalid,
            "dimensions" => Self::Dimensions,
            "allocation" => Self::Allocation,
            _ => Self::Worker,
        }
    }

    fn job_failure(self) -> JobFailure {
        JobFailure::new(match self {
            Self::Io => JobErrorCode::Io,
            Self::TooLarge => JobErrorCode::TooLarge,
            Self::Cancelled => JobErrorCode::Cancelled,
            Self::Unsupported => JobErrorCode::Unsupported,
            Self::Invalid | Self::Dimensions | Self::Allocation => JobErrorCode::Decode,
            Self::Worker => JobErrorCode::Internal,
        })
    }

    pub(crate) fn localized(self, locale: drill_core::Locale) -> &'static str {
        match (self, locale) {
            (Self::Io, drill_core::Locale::Ja) => "画像ファイルを読み込めません",
            (Self::Io, drill_core::Locale::En) => "The image file could not be read",
            (Self::TooLarge, drill_core::Locale::Ja) => "画像が32 MiBの安全上限を超えています",
            (Self::TooLarge, drill_core::Locale::En) => "The image exceeds the 32 MiB safety limit",
            (Self::Cancelled, drill_core::Locale::Ja) => "読み込みをキャンセルしました",
            (Self::Cancelled, drill_core::Locale::En) => "Loading was cancelled",
            (Self::Unsupported, drill_core::Locale::Ja) => "PNGまたはJPEG画像を選んでください",
            (Self::Unsupported, drill_core::Locale::En) => "Choose a PNG or JPEG image",
            (Self::Invalid, drill_core::Locale::Ja) => "画像が壊れているか途中で切れています",
            (Self::Invalid, drill_core::Locale::En) => "The image is malformed or truncated",
            (Self::Dimensions, drill_core::Locale::Ja) => "画像の寸法が安全上限を超えています",
            (Self::Dimensions, drill_core::Locale::En) => {
                "The image dimensions exceed the safety limit"
            }
            (Self::Allocation, drill_core::Locale::Ja) => "画像の展開サイズが安全ではありません",
            (Self::Allocation, drill_core::Locale::En) => "The decoded image size is unsafe",
            (Self::Worker, drill_core::Locale::Ja) => "画像処理中に内部エラーが発生しました",
            (Self::Worker, drill_core::Locale::En) => "An internal image-processing error occurred",
        }
    }
}

impl From<drill_interop::underlay::UnderlayError> for UnderlayFailure {
    fn from(value: drill_interop::underlay::UnderlayError) -> Self {
        use drill_interop::underlay::UnderlayError;
        match value {
            UnderlayError::TooLarge => Self::TooLarge,
            UnderlayError::Unsupported => Self::Unsupported,
            UnderlayError::Invalid => Self::Invalid,
            UnderlayError::Dimensions => Self::Dimensions,
            UnderlayError::Allocation => Self::Allocation,
        }
    }
}

pub(crate) enum UnderlayEvent {
    Ready {
        name: String,
        model: Option<drill_core::underlay::ImageUnderlay>,
    },
    Failed(UnderlayFailure),
}
struct Loaded {
    name: String,
    bytes: Vec<u8>,
    source_path: String,
    decoded: drill_interop::underlay::DecodedUnderlay,
}
pub(crate) struct UnderlayState {
    job: Option<Job<Loaded>>,
    pub texture: Option<egui::TextureHandle>,
    previous: Option<egui::TextureHandle>,
    pub opacity: f32,
    pub visible: bool,
    pub name: Option<String>,
    pub asset_bytes: Option<Vec<u8>>,
}
impl Default for UnderlayState {
    fn default() -> Self {
        Self {
            job: None,
            texture: None,
            previous: None,
            opacity: 0.30,
            visible: true,
            name: None,
            asset_bytes: None,
        }
    }
}
impl UnderlayState {
    pub fn load_bytes(&mut self, bytes: Vec<u8>, name: String) {
        self.job = Some(Job::spawn_typed(JobKind::Import, move |progress| {
            let limits = drill_interop::underlay::UnderlayLimits::default();
            if bytes.len() > limits.max_bytes {
                return Err(UnderlayFailure::TooLarge.job_failure());
            }
            let decoded = drill_interop::underlay::decode_underlay(&bytes, limits)
                .map_err(|error| UnderlayFailure::from(error).job_failure())?;
            progress.set(1.0);
            Ok(Loaded {
                name,
                bytes,
                source_path: String::new(),
                decoded,
            })
        }));
    }
    pub fn load(&mut self, path: PathBuf) {
        self.job = Some(Job::spawn_typed(JobKind::Import, move |progress| {
            let meta = std::fs::metadata(&path).map_err(|_| UnderlayFailure::Io.job_failure())?;
            let limits = drill_interop::underlay::UnderlayLimits::default();
            if meta.len() > limits.max_bytes as u64 {
                return Err(UnderlayFailure::TooLarge.job_failure());
            }
            let bytes = std::fs::read(&path).map_err(|_| UnderlayFailure::Io.job_failure())?;
            progress.set(0.3);
            if progress.is_cancelled() {
                return Err(UnderlayFailure::Cancelled.job_failure());
            }
            let decoded = drill_interop::underlay::decode_underlay(&bytes, limits)
                .map_err(|error| UnderlayFailure::from(error).job_failure())?;
            progress.set(1.0);
            Ok(Loaded {
                name: path
                    .file_name()
                    .map_or_else(|| "underlay".into(), |v| v.to_string_lossy().into_owned()),
                decoded,
                bytes,
                source_path: path.to_string_lossy().into_owned(),
            })
        }));
    }
    pub fn poll(&mut self, ctx: &egui::Context) -> Option<UnderlayEvent> {
        let msg = self.job.as_mut().and_then(Job::poll)?;
        self.job = None;
        Some(match msg {
            JobMsg::Done(v) => {
                let size = [v.decoded.width as usize, v.decoded.height as usize];
                let image = egui::ColorImage::from_rgba_unmultiplied(size, &v.decoded.rgba);
                self.previous = self.texture.take();
                self.texture = Some(ctx.load_texture(
                    format!("underlay-{}", hex8(&v.decoded.source_hash)),
                    image,
                    egui::TextureOptions::LINEAR,
                ));
                self.name = Some(v.name.clone());
                self.asset_bytes = Some(v.bytes);
                self.visible = true;
                let model =
                    (!v.source_path.is_empty()).then(|| drill_core::underlay::ImageUnderlay {
                        content_hash: hex64(&v.decoded.source_hash),
                        byte_len: self
                            .asset_bytes
                            .as_ref()
                            .map_or(0, |bytes| bytes.len() as u64),
                        original_name: v.name.clone(),
                        external_path: Some(v.source_path),
                        placement: drill_core::underlay::UnderlayPlacement::default(),
                    });
                UnderlayEvent::Ready {
                    name: v.name,
                    model,
                }
            }
            JobMsg::Failed(error) => UnderlayEvent::Failed(match error.code {
                drill_jobs::JobErrorCode::Io => UnderlayFailure::Io,
                drill_jobs::JobErrorCode::Cancelled => UnderlayFailure::Cancelled,
                drill_jobs::JobErrorCode::InvalidInput => UnderlayFailure::Invalid,
                drill_jobs::JobErrorCode::TooLarge => UnderlayFailure::TooLarge,
                drill_jobs::JobErrorCode::Unsupported => UnderlayFailure::Unsupported,
                drill_jobs::JobErrorCode::Decode => UnderlayFailure::Invalid,
                _ => UnderlayFailure::Worker,
            }),
            JobMsg::Cancelled => UnderlayEvent::Failed(UnderlayFailure::Cancelled),
        })
    }
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
    pub fn remove(&mut self) {
        self.previous = self.texture.take();
        self.name = None;
        self.asset_bytes = None;
    }
    pub fn undo(&mut self) -> bool {
        if self.previous.is_none() {
            return false;
        }
        std::mem::swap(&mut self.texture, &mut self.previous);
        true
    }
    pub fn paint(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        placement: Option<&drill_core::underlay::UnderlayPlacement>,
    ) {
        let visible = placement.map_or(self.visible, |p| p.visible);
        if visible && let Some(t) = &self.texture {
            let p = placement.cloned().unwrap_or_default();
            let center = egui::Pos2::new(
                rect.center().x + p.x / 100.0 * rect.width(),
                rect.center().y - p.y / 100.0 * rect.height(),
            );
            let half = egui::Vec2::new(
                rect.width() * p.scale_x * 0.5,
                rect.height() * p.scale_y * 0.5,
            );
            let (sin, cos) = p.rotation_radians.sin_cos();
            let rotate = |x: f32, y: f32| {
                egui::Pos2::new(center.x + x * cos - y * sin, center.y + x * sin + y * cos)
            };
            let mut mesh = egui::Mesh::with_texture(t.id());
            let color = egui::Color32::WHITE.gamma_multiply(p.opacity);
            mesh.vertices.extend([
                egui::epaint::Vertex {
                    pos: rotate(-half.x, -half.y),
                    uv: egui::Pos2::new(0.0, 0.0),
                    color,
                },
                egui::epaint::Vertex {
                    pos: rotate(half.x, -half.y),
                    uv: egui::Pos2::new(1.0, 0.0),
                    color,
                },
                egui::epaint::Vertex {
                    pos: rotate(half.x, half.y),
                    uv: egui::Pos2::new(1.0, 1.0),
                    color,
                },
                egui::epaint::Vertex {
                    pos: rotate(-half.x, half.y),
                    uv: egui::Pos2::new(0.0, 1.0),
                    color,
                },
            ]);
            mesh.indices.extend([0, 1, 2, 0, 2, 3]);
            painter.add(egui::Shape::mesh(mesh));
        }
    }
}
fn hex8(hash: &[u8; 32]) -> String {
    hash[..4].iter().map(|b| format!("{b:02x}")).collect()
}
fn hex64(hash: &[u8; 32]) -> String {
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_failure_has_stable_code_and_both_locales() {
        let variants = [
            UnderlayFailure::Io,
            UnderlayFailure::TooLarge,
            UnderlayFailure::Cancelled,
            UnderlayFailure::Unsupported,
            UnderlayFailure::Invalid,
            UnderlayFailure::Dimensions,
            UnderlayFailure::Allocation,
            UnderlayFailure::Worker,
        ];
        for error in variants {
            assert_eq!(UnderlayFailure::from_code(error.code()), error);
            let ja = error.localized(drill_core::Locale::Ja);
            let en = error.localized(drill_core::Locale::En);
            assert!(!ja.is_empty() && !en.is_empty() && ja != en);
            assert!(
                !en.chars()
                    .any(|ch| matches!(ch, '\u{3040}'..='\u{30ff}' | '\u{4e00}'..='\u{9fff}'))
            );
        }
        assert_eq!(
            UnderlayFailure::from_code("future-code"),
            UnderlayFailure::Worker
        );
    }
}
