use drill_core::{
    Document, Locale, Point,
    camera::Camera,
    stadium::{Lighting, Weather},
    visibility::{OcclusionResult, VisibilityScratch, visibility_from_seat_with_ids},
};
use eframe::egui;

pub(crate) struct StadiumInspector {
    pub(crate) enabled: bool,
    scratch: VisibilityScratch,
    results: Vec<OcclusionResult>,
    pub(crate) hidden: usize,
    pub(crate) impaired: usize,
    pub(crate) minimum_visible: f32,
    weather: Weather,
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
        }
    }
}

impl StadiumInspector {
    pub(crate) fn lighting(&self) -> Lighting {
        Lighting::preset(self.weather)
    }
    pub(crate) fn analyze(&mut self, document: &Document, positions: &[Point], camera: Camera) {
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
    }

    pub(crate) fn visible_fraction(&self, index: usize) -> Option<f32> {
        self.enabled
            .then(|| self.results.get(index).map_or(1.0, |r| r.visible_fraction))
    }

    pub(crate) fn controls(&mut self, ui: &mut egui::Ui, locale: Locale) -> bool {
        let mut refresh = false;
        ui.horizontal(|ui| {
            if ui
                .checkbox(
                    &mut self.enabled,
                    super::i18n::registered(locale, "stadium-inspector.001"),
                )
                .changed()
                && self.enabled
            {
                refresh = true;
            }
            if self.enabled
                && ui
                    .button(super::i18n::registered(locale, "stadium-inspector.002"))
                    .clicked()
            {
                refresh = true;
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
        i.analyze(&d, &p, Camera::audience_view(&d.grid));
        assert!(i.hidden <= p.len());
        assert!((0.0..=1.0).contains(&i.minimum_visible));
    }
}
