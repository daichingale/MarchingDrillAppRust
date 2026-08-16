use serde::{Deserialize, Serialize};

pub const MAX_UNDERLAY_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnderlayRenderPolicy {
    /// Editor reference only: excluded from 3D, print and video exports.
    #[default]
    Editor2dOnly,
    /// Included in flat 2D exports; still excluded from 3D views.
    EditorAnd2dExport,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnderlayPlacement {
    pub x: f32,
    pub y: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub rotation_radians: f32,
    pub opacity: f32,
    pub visible: bool,
    #[serde(default)]
    pub render_policy: UnderlayRenderPolicy,
}

impl Default for UnderlayPlacement {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            rotation_radians: 0.0,
            opacity: 0.30,
            visible: true,
            render_policy: UnderlayRenderPolicy::Editor2dOnly,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageUnderlay {
    /// BLAKE3 hex digest: stable content address shared with `.drillproj` assets.
    pub content_hash: String,
    pub byte_len: u64,
    pub original_name: String,
    /// Optional external source hint. Missing files degrade to metadata-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_path: Option<String>,
    #[serde(default)]
    pub placement: UnderlayPlacement,
}

impl ImageUnderlay {
    pub fn validate(&self) -> bool {
        self.content_hash.len() == 64
            && self.content_hash.bytes().all(|b| b.is_ascii_hexdigit())
            && self.byte_len <= MAX_UNDERLAY_BYTES
            && !self.original_name.is_empty()
            && self.original_name.len() <= crate::MAX_TEXT_BYTES
            && self
                .external_path
                .as_ref()
                .is_none_or(|p| p.len() <= crate::MAX_TEXT_BYTES)
            && [
                self.placement.x,
                self.placement.y,
                self.placement.scale_x,
                self.placement.scale_y,
                self.placement.rotation_radians,
                self.placement.opacity,
            ]
            .into_iter()
            .all(f32::is_finite)
            && self.placement.scale_x > 0.0
            && self.placement.scale_y > 0.0
            && (0.0..=1.0).contains(&self.placement.opacity)
    }
}
