//! Deterministic, venue-neutral stadium geometry and display policy.

use crate::{GridConfig, Point};
use serde::{Deserialize, Serialize};
use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StadiumModel {
    pub sideline_margin_m: f32,
    pub end_zone_margin_m: f32,
    pub stands: Vec<StandSection>,
    pub press_box: Option<PressBox>,
    pub lighting: Lighting,
}

impl Default for StadiumModel {
    fn default() -> Self {
        Self::generic(&GridConfig::default())
    }
}

impl StadiumModel {
    pub fn generic(grid: &GridConfig) -> Self {
        Self {
            sideline_margin_m: 3.0,
            end_zone_margin_m: 5.0,
            stands: vec![
                StandSection {
                    id: StandId(0),
                    baseline: StandBaseline::Home,
                    rows: 40,
                    row_rise_m: 0.35,
                    row_depth_m: 0.75,
                    front_offset_m: 3.0,
                    curvature_sagitta_m: 0.0,
                    extent: 0.0..grid.width,
                },
                StandSection {
                    id: StandId(1),
                    baseline: StandBaseline::Visitor,
                    rows: 20,
                    row_rise_m: 0.30,
                    row_depth_m: 0.75,
                    front_offset_m: 3.0,
                    curvature_sagitta_m: 0.0,
                    extent: 0.0..grid.width,
                },
            ],
            press_box: Some(PressBox {
                stand: StandId(0),
                height_above_stand_m: 4.0,
                width_fraction: 0.3,
            }),
            lighting: Lighting::preset(Weather::Clear),
        }
    }

    pub fn stand(&self, id: StandId) -> Option<&StandSection> {
        self.stands.iter().find(|stand| stand.id == id)
    }

    /// Rejects pathological imported geometry before it reaches a renderer.
    pub fn validate(&self) -> Result<(), &'static str> {
        if !self.sideline_margin_m.is_finite() || !self.end_zone_margin_m.is_finite() {
            return Err("non-finite stadium margin");
        }
        if self.stands.len() > 64 {
            return Err("too many stand sections");
        }
        if self.stands.iter().any(|s| s.rows > 512 || !s.is_finite()) {
            return Err("invalid stand section");
        }
        if !self.lighting.is_valid() {
            return Err("invalid lighting");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StandId(pub u16);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StandSection {
    pub id: StandId,
    pub baseline: StandBaseline,
    pub rows: u16,
    pub row_rise_m: f32,
    pub row_depth_m: f32,
    pub front_offset_m: f32,
    pub curvature_sagitta_m: f32,
    pub extent: Range<f32>,
}

impl StandSection {
    fn is_finite(&self) -> bool {
        [
            self.row_rise_m,
            self.row_depth_m,
            self.front_offset_m,
            self.curvature_sagitta_m,
            self.extent.start,
            self.extent.end,
        ]
        .iter()
        .all(|v| v.is_finite())
            && self.row_rise_m >= 0.0
            && self.row_depth_m > 0.0
            && self.front_offset_m >= 0.0
            && self.extent.end >= self.extent.start
    }

    pub fn seat_eye_position(&self, grid: &GridConfig, along_frac: f32, row: u16) -> [f32; 3] {
        let along = if along_frac.is_finite() {
            along_frac.clamp(0.0, 1.0)
        } else {
            0.5
        };
        let row = row.min(self.rows.saturating_sub(1));
        let axis = self.extent.start + (self.extent.end - self.extent.start) * along;
        let bow = self.curvature_sagitta_m * (1.0 - (2.0 * along - 1.0).powi(2));
        let depth = self.front_offset_m + self.row_depth_m * f32::from(row) + bow;
        let height = 1.2 + self.row_rise_m * f32::from(row);
        match self.baseline {
            StandBaseline::Home => [axis, height, -depth],
            StandBaseline::Visitor => [axis, height, grid.height + depth],
            StandBaseline::EndZoneNear => [-depth, height, axis],
            StandBaseline::EndZoneFar => [grid.width + depth, height, axis],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StandBaseline {
    Home,
    Visitor,
    EndZoneNear,
    EndZoneFar,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PressBox {
    pub stand: StandId,
    pub height_above_stand_m: f32,
    pub width_fraction: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PerformerLod {
    Billboard,
    SimpleFigure,
    InstrumentSilhouette,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LodThresholds {
    pub simple_figure_px: f32,
    pub silhouette_px: f32,
}
impl Default for LodThresholds {
    fn default() -> Self {
        Self {
            simple_figure_px: 10.0,
            silhouette_px: 40.0,
        }
    }
}
pub fn choose_lod(screen_height_px: f32, thresholds: &LodThresholds) -> PerformerLod {
    if !screen_height_px.is_finite() || screen_height_px < thresholds.simple_figure_px {
        PerformerLod::Billboard
    } else if screen_height_px < thresholds.silhouette_px {
        PerformerLod::SimpleFigure
    } else {
        PerformerLod::InstrumentSilhouette
    }
}

/// Venue-neutral directional fill plus atmospheric fog. Shared by CPU and
/// GPU parity tests; output remains deterministic across platforms.
pub fn shade_color(rgb: [u8; 3], lighting: Lighting, distance_m: f32) -> [u8; 3] {
    let sun = lighting.sun_elevation_deg.to_radians().sin().max(0.0);
    let light = (lighting.ambient + (1.0 - lighting.ambient) * sun).clamp(0.0, 1.0);
    let fog = (1.0 - (-lighting.fog_density.max(0.0) * distance_m.max(0.0)).exp()).clamp(0.0, 0.85);
    std::array::from_fn(|i| {
        let lit = f32::from(rgb[i]) / 255.0 * light;
        ((lit + (lighting.sky_tint[i] - lit) * fog).clamp(0.0, 1.0) * 255.0).round() as u8
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroundShadow {
    pub center: Point,
    pub radius_m: f32,
    pub alpha: f32,
}
pub fn ground_shadow_for(center: Point, radius_m: f32) -> GroundShadow {
    GroundShadow {
        center,
        radius_m: radius_m.max(0.0),
        alpha: 0.35,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FieldLogo {
    pub asset_path: String,
    pub anchor: Point,
    pub size_m: (f32, f32),
    pub rotation_deg: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Lighting {
    pub sun_elevation_deg: f32,
    pub sun_azimuth_deg: f32,
    pub ambient: f32,
    pub sky_tint: [f32; 3],
    pub fog_density: f32,
}
impl Lighting {
    pub fn preset(weather: Weather) -> Self {
        match weather {
            Weather::Clear => Self {
                sun_elevation_deg: 55.0,
                sun_azimuth_deg: 200.0,
                ambient: 0.35,
                sky_tint: [0.55, 0.65, 0.75],
                fog_density: 0.015,
            },
            Weather::Overcast => Self {
                sun_elevation_deg: 90.0,
                sun_azimuth_deg: 0.0,
                ambient: 0.75,
                sky_tint: [0.6, 0.6, 0.62],
                fog_density: 0.03,
            },
            Weather::NightLights => Self {
                sun_elevation_deg: 35.0,
                sun_azimuth_deg: 0.0,
                ambient: 0.2,
                sky_tint: [0.05, 0.06, 0.1],
                fog_density: 0.05,
            },
        }
    }
    pub fn is_valid(&self) -> bool {
        [
            self.sun_elevation_deg,
            self.sun_azimuth_deg,
            self.ambient,
            self.sky_tint[0],
            self.sky_tint[1],
            self.sky_tint[2],
            self.fog_density,
        ]
        .iter()
        .all(|v| v.is_finite())
            && (0.0..=1.0).contains(&self.ambient)
            && self.sky_tint.iter().all(|v| (0.0..=1.0).contains(v))
            && self.fog_density >= 0.0
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Weather {
    Clear,
    Overcast,
    NightLights,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generic_is_bounded_and_valid() {
        let model = StadiumModel::generic(&GridConfig::default());
        assert_eq!(model.stands.len(), 2);
        assert!(model.validate().is_ok());
    }
    #[test]
    fn straight_seat_is_exact() {
        let g = GridConfig::default();
        let s = &StadiumModel::generic(&g).stands[0];
        let eye = s.seat_eye_position(&g, 0.5, 2);
        assert!((eye[0] - 50.0).abs() < 1.0e-5);
        assert!((eye[1] - 1.9).abs() < 1.0e-5);
        assert!((eye[2] + 4.5).abs() < 1.0e-5);
    }
    #[test]
    fn lod_edges_are_stable() {
        let t = LodThresholds::default();
        assert_eq!(choose_lod(9.99, &t), PerformerLod::Billboard);
        assert_eq!(choose_lod(10.0, &t), PerformerLod::SimpleFigure);
        assert_eq!(choose_lod(40.0, &t), PerformerLod::InstrumentSilhouette);
    }
    #[test]
    fn lighting_is_bounded_and_fogs_toward_sky() {
        let l = Lighting::preset(Weather::NightLights);
        let near = shade_color([255, 100, 0], l, 0.0);
        let far = shade_color([255, 100, 0], l, 1000.0);
        assert!(near[0] > far[0]);
        assert!((i16::from(far[2]) - ((l.sky_tint[2] * 255.0) as i16)).abs() < 45);
    }
}
