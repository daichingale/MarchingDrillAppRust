//! Selectable application color themes (window chrome, panels, toolbars --
//! not the field view, which has its own `drill_render::Theme` and stays
//! print-styled regardless of this setting). Persisted the same boring way
//! as `recent_projects.rs`: a small JSON file under `app_data_dir()`, loaded
//! once at startup and rewritten only when the user changes the selection.

use eframe::egui::{self, Color32};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum AppTheme {
    /// The original DrillForge dark theme: cool blue-grays, no strong accent.
    #[default]
    Studio,
    /// Bright, neutral grays with a warm orange accent, in the spirit of a
    /// modern light-mode DAW.
    Daylight,
    /// Near-black panels with a saturated green accent, in the spirit of a
    /// classic dark-mode DAW.
    Nightline,
}

impl AppTheme {
    pub(crate) const ALL: [Self; 3] = [Self::Studio, Self::Daylight, Self::Nightline];

    pub(crate) fn label(self, locale: drill_core::Locale) -> &'static str {
        use drill_core::Locale;
        match (locale, self) {
            (Locale::Ja, Self::Studio) => "スタジオ（既定）",
            (Locale::En, Self::Studio) => "Studio (default)",
            (Locale::Ja, Self::Daylight) => "デイライト（明るい）",
            (Locale::En, Self::Daylight) => "Daylight (bright)",
            (Locale::Ja, Self::Nightline) => "ナイトライン（濃色・緑）",
            (Locale::En, Self::Nightline) => "Nightline (dark, green accent)",
        }
    }

    /// Builds the full `egui::Visuals` for this theme. Only touches window
    /// chrome / panels / widgets; never the field view's own render theme.
    pub(crate) fn visuals(self) -> egui::Visuals {
        match self {
            Self::Studio => studio_visuals(),
            Self::Daylight => daylight_visuals(),
            Self::Nightline => nightline_visuals(),
        }
    }

    /// Applies both the visuals and the theme preference to `ctx`.
    ///
    /// `set_visuals` alone is not enough: egui keeps separate dark/light
    /// style slots and picks between them via `ThemePreference`, which
    /// defaults to `System` (follows the OS). If the OS is in dark mode and
    /// the user picks a light `AppTheme` (Daylight), `set_visuals` writes
    /// into the light slot but egui keeps rendering from the dark slot, so
    /// the change silently never appears. Pinning the preference here to
    /// match the theme we just set makes egui actually render that slot.
    pub(crate) fn apply(self, ctx: &egui::Context) {
        let visuals = self.visuals();
        ctx.set_theme(if visuals.dark_mode {
            egui::ThemePreference::Dark
        } else {
            egui::ThemePreference::Light
        });
        ctx.set_visuals(visuals);
    }

    pub(crate) fn load() -> Self {
        preferences_path()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub(crate) fn persist(self) {
        let Some(path) = preferences_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(bytes) = serde_json::to_vec(&self) {
            let _ = std::fs::write(path, bytes);
        }
    }
}

fn preferences_path() -> Option<PathBuf> {
    Some(super::project_state::app_data_dir().join("theme.json"))
}

/// The original theme, unchanged from `bootstrap.rs`'s previous hardcoded
/// `install_fonts` visuals.
fn studio_visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(Color32::from_gray(225));
    visuals.panel_fill = Color32::from_rgb(13, 17, 23);
    visuals.window_fill = Color32::from_rgb(20, 26, 34);
    visuals.faint_bg_color = Color32::from_rgb(28, 36, 47);
    visuals.extreme_bg_color = Color32::from_rgb(8, 11, 15);
    visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(20, 26, 34);
    visuals.widgets.noninteractive.weak_bg_fill = Color32::from_rgb(20, 26, 34);
    visuals.widgets.noninteractive.fg_stroke.color = Color32::from_gray(205);
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(36, 45, 58);
    visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(36, 45, 58);
    visuals.widgets.inactive.fg_stroke.color = Color32::from_gray(225);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(52, 66, 84);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(52, 66, 84);
    visuals.widgets.active.bg_fill = Color32::from_rgb(67, 88, 112);
    visuals.widgets.active.weak_bg_fill = Color32::from_rgb(67, 88, 112);
    visuals.widgets.open.bg_fill = Color32::from_rgb(45, 58, 74);
    visuals.widgets.open.weak_bg_fill = Color32::from_rgb(45, 58, 74);
    visuals
}

/// Bright neutral grays, dark text, warm orange selection/accent -- evokes a
/// light-mode DAW control surface without copying any specific product's
/// exact palette.
fn daylight_visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::light();
    let accent = Color32::from_rgb(240, 130, 40);
    visuals.override_text_color = Some(Color32::from_gray(35));
    visuals.panel_fill = Color32::from_rgb(226, 226, 228);
    visuals.window_fill = Color32::from_rgb(235, 235, 237);
    visuals.faint_bg_color = Color32::from_rgb(216, 216, 219);
    visuals.extreme_bg_color = Color32::from_rgb(250, 250, 251);
    visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(226, 226, 228);
    visuals.widgets.noninteractive.weak_bg_fill = Color32::from_rgb(226, 226, 228);
    visuals.widgets.noninteractive.fg_stroke.color = Color32::from_gray(60);
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(206, 206, 210);
    visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(206, 206, 210);
    visuals.widgets.inactive.fg_stroke.color = Color32::from_gray(35);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(224, 178, 138);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(224, 178, 138);
    visuals.widgets.hovered.fg_stroke.color = Color32::from_gray(20);
    visuals.widgets.active.bg_fill = accent;
    visuals.widgets.active.weak_bg_fill = accent;
    visuals.widgets.active.fg_stroke.color = Color32::WHITE;
    visuals.widgets.open.bg_fill = Color32::from_rgb(214, 214, 218);
    visuals.widgets.open.weak_bg_fill = Color32::from_rgb(214, 214, 218);
    visuals.selection.bg_fill = accent;
    visuals.selection.stroke.color = Color32::from_gray(20);
    visuals.hyperlink_color = accent;
    visuals
}

/// Near-black panels, saturated green selection/accent -- evokes a
/// dark-mode DAW control surface without copying any specific product's
/// exact palette.
fn nightline_visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();
    let accent = Color32::from_rgb(90, 200, 110);
    visuals.override_text_color = Some(Color32::from_gray(215));
    visuals.panel_fill = Color32::from_rgb(18, 20, 18);
    visuals.window_fill = Color32::from_rgb(14, 16, 14);
    visuals.faint_bg_color = Color32::from_rgb(24, 27, 24);
    visuals.extreme_bg_color = Color32::from_rgb(6, 7, 6);
    visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(18, 20, 18);
    visuals.widgets.noninteractive.weak_bg_fill = Color32::from_rgb(18, 20, 18);
    visuals.widgets.noninteractive.fg_stroke.color = Color32::from_gray(190);
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(32, 36, 32);
    visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(32, 36, 32);
    visuals.widgets.inactive.fg_stroke.color = Color32::from_gray(210);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(38, 68, 42);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(38, 68, 42);
    visuals.widgets.hovered.fg_stroke.color = Color32::WHITE;
    visuals.widgets.active.bg_fill = accent;
    visuals.widgets.active.weak_bg_fill = accent;
    visuals.widgets.active.fg_stroke.color = Color32::from_gray(10);
    visuals.widgets.open.bg_fill = Color32::from_rgb(28, 42, 30);
    visuals.widgets.open.weak_bg_fill = Color32::from_rgb(28, 42, 30);
    visuals.selection.bg_fill = accent;
    visuals.selection.stroke.color = Color32::BLACK;
    visuals.hyperlink_color = accent;
    visuals
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_theme_has_both_locale_labels() {
        for theme in AppTheme::ALL {
            assert!(!theme.label(drill_core::Locale::Ja).is_empty());
            assert!(!theme.label(drill_core::Locale::En).is_empty());
        }
    }

    #[test]
    fn round_trips_through_json() {
        for theme in AppTheme::ALL {
            let bytes = serde_json::to_vec(&theme).unwrap();
            let loaded: AppTheme = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(loaded, theme);
        }
    }
}
