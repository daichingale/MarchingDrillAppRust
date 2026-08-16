use drill_core::{
    Document, Locale, Point, Revision,
    camera::Camera,
    stadium::{Lighting, Weather},
    visibility::{OcclusionResult, VisibilityScratch, visibility_from_seat_with_ids},
};
use eframe::egui;

/// Exact scene identity for an audience-visibility result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VisibilityKey {
    revision: Revision,
    set_index: usize,
    count_bits: u32,
    camera_bits: [u32; 9],
}

impl VisibilityKey {
    pub(crate) fn new(revision: Revision, set_index: usize, count: f32, camera: Camera) -> Self {
        Self {
            revision,
            set_index,
            count_bits: count.to_bits(),
            camera_bits: [
                camera.target[0].to_bits(),
                camera.target[1].to_bits(),
                camera.target[2].to_bits(),
                camera.yaw.to_bits(),
                camera.pitch.to_bits(),
                camera.distance.to_bits(),
                camera.fov_y_rad.to_bits(),
                camera.near.to_bits(),
                camera.far.to_bits(),
            ],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VisibilityRefresh {
    None,
    Manual,
}

pub(crate) struct StadiumInspector {
    pub(crate) enabled: bool,
    scratch: VisibilityScratch,
    results: Vec<OcclusionResult>,
    pub(crate) hidden: usize,
    pub(crate) impaired: usize,
    pub(crate) minimum_visible: f32,
    weather: Weather,
    result_key: Option<VisibilityKey>,
    analysis_runs: u64,
}

impl Default for StadiumInspector {
    fn default() -> Self {
        Self {
            enabled: false,
            scratch: VisibilityScratch::default(),
            results: Vec::new(),
            hidden: 0,
            impaired: 0,
            minimum_visible: 1.0,
            weather: Weather::Clear,
            result_key: None,
            analysis_runs: 0,
        }
    }
}

impl StadiumInspector {
    pub(crate) fn lighting(&self) -> Lighting {
        Lighting::preset(self.weather)
    }
    pub(crate) fn analyze(
        &mut self,
        document: &Document,
        positions: &[Point],
        camera: Camera,
        key: VisibilityKey,
        force: bool,
    ) -> bool {
        if !force && self.result_key == Some(key) {
            return false;
        }
        let heights: Vec<f32> = document.performers.iter().map(|p| p.height_m).collect();
        let ids: Vec<_> = document.performers.iter().map(|p| p.id).collect();
        visibility_from_seat_with_ids(
            positions,
            &heights,
            &ids,
            camera.position(),
            0.25,
            &mut self.scratch,
            &mut self.results,
        );
        self.hidden = self
            .results
            .iter()
            .filter(|r| r.visible_fraction < 0.25)
            .count();
        self.impaired = self
            .results
            .iter()
            .filter(|r| r.visible_fraction < 0.75)
            .count();
        self.minimum_visible = self
            .results
            .iter()
            .map(|r| r.visible_fraction)
            .fold(1.0, f32::min);
        self.result_key = Some(key);
        self.analysis_runs = self.analysis_runs.saturating_add(1);
        true
    }

    pub(crate) fn visible_fraction(&self, key: VisibilityKey, index: usize) -> Option<f32> {
        (self.enabled && self.result_key == Some(key))
            .then(|| self.results.get(index).map_or(1.0, |r| r.visible_fraction))
    }

    pub(crate) fn is_current(&self, key: VisibilityKey) -> bool {
        self.result_key == Some(key)
    }

    /// Returns the current scene's performer indexes below `threshold`.
    /// Indexes intentionally follow the document order used by the renderer;
    /// callers can then apply their own session-only interaction filters.
    pub(crate) fn diagnostic_indexes(&self, key: VisibilityKey, threshold: f32) -> Vec<usize> {
        if !self.enabled || self.result_key != Some(key) {
            return Vec::new();
        }
        self.results
            .iter()
            .enumerate()
            .filter_map(|(index, result)| (result.visible_fraction < threshold).then_some(index))
            .collect()
    }

    pub(crate) fn controls(
        &mut self,
        ui: &mut egui::Ui,
        locale: Locale,
        key: VisibilityKey,
    ) -> VisibilityRefresh {
        let mut refresh = VisibilityRefresh::None;
        ui.horizontal(|ui| {
            if ui
                .checkbox(
                    &mut self.enabled,
                    super::i18n::registered(locale, "stadium-inspector.001"),
                )
                .changed()
                && self.enabled
            {
                refresh = VisibilityRefresh::None;
            }
            if self.enabled
                && ui
                    .button(super::i18n::registered(locale, "stadium-inspector.002"))
                    .clicked()
            {
                refresh = VisibilityRefresh::Manual;
            }
        });
        ui.horizontal(|ui| {
            ui.label(super::i18n::registered(locale, "stadium-inspector.003"));
            for (weather, ja, en) in [
                (Weather::Clear, "晴天", "Clear"),
                (Weather::Overcast, "曇天", "Overcast"),
                (Weather::NightLights, "ナイター", "Night"),
            ] {
                ui.selectable_value(
                    &mut self.weather,
                    weather,
                    if locale == Locale::Ja { ja } else { en },
                );
            }
        });
        if self.enabled {
            ui.small(if self.is_current(key) {
                super::i18n::registered(locale, "stadium-inspector.005")
            } else {
                super::i18n::registered(locale, "stadium-inspector.006")
            });
            ui.label(if locale == Locale::Ja {
                format!(
                    "見えにくい演者: {} / 完全遮蔽に近い: {} / 最低視認率: {:.0}%",
                    self.impaired,
                    self.hidden,
                    self.minimum_visible * 100.0
                )
            } else {
                format!(
                    "Impaired: {} / nearly hidden: {} / minimum visibility: {:.0}%",
                    self.impaired,
                    self.hidden,
                    self.minimum_visible * 100.0
                )
            });
            ui.small(super::i18n::registered(locale, "stadium-inspector.004"));
        }
        refresh
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn analysis_summary_is_bounded() {
        let d = Document::demo(2, 2);
        let p = d.sets[0].positions.clone();
        let mut i = StadiumInspector {
            enabled: true,
            ..StadiumInspector::default()
        };
        let camera = Camera::audience_view(&d.grid);
        let key = VisibilityKey::new(Revision(1), 0, 0.0, camera);
        assert!(i.analyze(&d, &p, camera, key, false));
        assert!(i.hidden <= p.len());
        assert!((0.0..=1.0).contains(&i.minimum_visible));
    }

    #[test]
    fn diagnostic_is_cached_per_exact_scene_and_marks_other_scenes_stale() {
        let d = Document::demo(2, 2);
        let p = d.sets[0].positions.clone();
        let camera = Camera::audience_view(&d.grid);
        let key = VisibilityKey::new(Revision(7), 0, 0.0, camera);
        let mut i = StadiumInspector::default();
        assert!(i.analyze(&d, &p, camera, key, false));
        assert!(!i.analyze(&d, &p, camera, key, false));
        assert_eq!(i.analysis_runs, 1);
        let changed = VisibilityKey::new(Revision(7), 0, 1.0, camera);
        assert!(!i.is_current(changed));
        assert!(i.visible_fraction(changed, 0).is_none());
        assert!(i.analyze(&d, &p, camera, changed, true));
        assert_eq!(i.analysis_runs, 2);
    }

    #[test]
    fn diagnostic_indexes_are_only_exposed_for_the_current_enabled_scene() {
        let d = Document::demo(2, 2);
        let p = d.sets[0].positions.clone();
        let camera = Camera::audience_view(&d.grid);
        let key = VisibilityKey::new(Revision(3), 0, 0.0, camera);
        let mut i = StadiumInspector {
            enabled: true,
            ..StadiumInspector::default()
        };
        assert!(i.analyze(&d, &p, camera, key, false));
        assert!(i.diagnostic_indexes(key, 1.01).len() == p.len());
        assert!(
            i.diagnostic_indexes(VisibilityKey::new(Revision(3), 0, 1.0, camera), 1.01)
                .is_empty()
        );
        i.enabled = false;
        assert!(i.diagnostic_indexes(key, 1.01).is_empty());
    }
}
