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
    /// Bright, neutral grays with a blue accent.
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
        // Not a color, but this is the one chokepoint every context passes
        // through (startup in `bootstrap.rs`, restore in `DrillApp::new`, and
        // every user theme switch), so the app's motion budget is pinned here
        // rather than being re-set from three places.
        //
        // `Style::animation_time` is what `Context::animate_bool_responsive`
        // reads, which is what every `CollapsingHeader` in the inspector uses
        // for its open/close tween. egui's default is 0.2s; at the density of
        // a drill inspector -- where sections get opened and closed constantly
        // while chasing a coordinate -- that reads as a lurch rather than a
        // reveal. 0.15s keeps the easing legible without ever making the
        // writer wait on it.
        // `all_styles_mut` rather than `global_style_mut`: egui keeps separate
        // dark and light `Style` slots, and this app switches between them
        // (Daylight is light, the other two are dark). Writing only the active
        // slot would silently lose the tuning the first time the user crossed
        // that boundary from a context we didn't re-apply.
        ctx.all_styles_mut(|style| {
            style.animation_time = COLLAPSE_ANIMATION_SECONDS;
            polish_chrome(style, style.visuals.dark_mode);
        });
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

/// Open/close tween length for every `CollapsingHeader` in the app, via
/// `Style::animation_time`. See [`AppTheme::apply`].
const COLLAPSE_ANIMATION_SECONDS: f32 = 0.15;

/// Shared chrome tokens. Field rendering stays on `drill_render::Theme`;
/// these only tune toolbars, panels, cards, and buttons.
pub(crate) const ACCENT: Color32 = Color32::from_rgb(76, 163, 255);
pub(crate) const ACCENT_HOVER: Color32 = Color32::from_rgb(232, 242, 255);
pub(crate) const ACCENT_SOFT: Color32 = Color32::from_rgb(214, 232, 255);
pub(crate) const SECONDARY_TEXT: Color32 = Color32::from_rgb(84, 90, 102);
pub(crate) const HAIRLINE: Color32 = Color32::from_rgb(214, 218, 226);
pub(crate) const CORNER: u8 = 10;
pub(crate) const CORNER_SM: u8 = 8;

fn polish_chrome(style: &mut egui::Style, dark: bool) {
    use egui::TextStyle;
    style.spacing.item_spacing = egui::Vec2::new(10.0, 8.0);
    style.spacing.button_padding = egui::Vec2::new(12.0, 5.0);
    style.spacing.interact_size.y = 28.0;
    style
        .text_styles
        .insert(TextStyle::Body, egui::FontId::proportional(14.0));
    style
        .text_styles
        .insert(TextStyle::Small, egui::FontId::proportional(12.0));
    style
        .text_styles
        .insert(TextStyle::Button, egui::FontId::proportional(14.0));
    let rounding = egui::CornerRadius::same(CORNER);
    style.visuals.widgets.noninteractive.corner_radius = rounding;
    style.visuals.widgets.inactive.corner_radius = rounding;
    style.visuals.widgets.hovered.corner_radius = rounding;
    style.visuals.widgets.active.corner_radius = rounding;
    style.visuals.widgets.open.corner_radius = rounding;
    style.visuals.window_corner_radius = egui::CornerRadius::same(12);
    style.visuals.menu_corner_radius = rounding;
    style.visuals.window_shadow = egui::Shadow::NONE;
    style.visuals.popup_shadow = egui::Shadow::NONE;
    if !dark {
        style.visuals.widgets.hovered.bg_fill = ACCENT_HOVER;
        style.visuals.widgets.hovered.weak_bg_fill = ACCENT_HOVER;
        style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, ACCENT);
        style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, HAIRLINE);
        style.visuals.widgets.noninteractive.fg_stroke.color = SECONDARY_TEXT;
        style.visuals.selection.bg_fill = ACCENT_SOFT;
        style.visuals.selection.stroke.color = ACCENT;
        style.visuals.hyperlink_color = ACCENT;
    }
}

pub(crate) fn surface_frame(ui: &egui::Ui) -> egui::Frame {
    egui::Frame::new()
        .fill(ui.visuals().extreme_bg_color)
        .stroke(egui::Stroke::new(1.0, HAIRLINE))
        .corner_radius(CORNER)
        .inner_margin(egui::Margin::same(10))
}

pub(crate) fn toolbar_frame(ui: &egui::Ui) -> egui::Frame {
    egui::Frame::new()
        .fill(ui.visuals().panel_fill)
        .stroke(egui::Stroke::new(1.0, HAIRLINE))
        .corner_radius(0)
        .inner_margin(egui::Margin::symmetric(10, 4))
}

pub(crate) fn primary_button<'a>(text: impl Into<egui::WidgetText>) -> egui::Button<'a> {
    egui::Button::new(text)
        .fill(ACCENT)
        .stroke(egui::Stroke::NONE)
        .corner_radius(CORNER)
}

pub(crate) fn quiet_button<'a>(text: impl Into<egui::WidgetText>) -> egui::Button<'a> {
    egui::Button::new(text)
        .fill(Color32::TRANSPARENT)
        .stroke(egui::Stroke::new(1.0, HAIRLINE))
        .corner_radius(CORNER)
}

/// Softens the Studio/Daylight/Nightline switch from a hard flash into a
/// dissolve, by washing the outgoing theme's dominant color over the new one
/// for a moment and fading it out.
///
/// A true per-field interpolation of `egui::Visuals` was considered and
/// rejected. `Visuals` is ~40 colors plus strokes, shadows, corner radii and
/// booleans; several of those fields (`dark_mode`, `collapsing_header_frame`,
/// text-cursor settings) have no meaningful midpoint, and any field egui adds
/// in a future version would silently drop out of the transition and pop.
/// That is a lot of fragile surface for an effect the eye reads for 180ms.
/// One full-window veil of `panel_fill` -- the color that covers most of the
/// window in every one of these themes -- gets the perceptual result, degrades
/// gracefully, and cannot go stale.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ThemeFade {
    /// Outgoing panel color and seconds elapsed, or `None` when idle.
    active: Option<(Color32, f32)>,
}

impl ThemeFade {
    /// Short on purpose. The switch must still read as immediate; this only
    /// removes the hard edge, it is not meant to be watched.
    const DURATION: f32 = 0.18;
    /// Deliberately below 1.0. A fully opaque first frame would make the
    /// window blink to a flat color, which is a worse artifact than the
    /// flash it replaces.
    const PEAK_ALPHA: f32 = 0.8;

    /// Call with the theme being switched *away from*, just before applying
    /// the new one.
    pub(crate) fn begin(&mut self, outgoing: AppTheme) {
        self.active = Some((outgoing.visuals().panel_fill, 0.0));
    }

    /// Advances the fade by `dt` and paints this frame of it. Returns true
    /// while the fade is still running, so the caller keeps the repaint loop
    /// alive for exactly that long and no longer.
    pub(crate) fn paint(&mut self, ctx: &egui::Context, dt: f32) -> bool {
        let Some((color, elapsed)) = self.active.as_mut() else {
            return false;
        };
        *elapsed += dt;
        let t = *elapsed / Self::DURATION;
        if t >= 1.0 {
            self.active = None;
            return false;
        }
        // Quadratic ease-out on the alpha: the outgoing theme is more than
        // half gone within the first 55ms, so the new theme is what the eye
        // actually settles on, with the old one trailing off behind it.
        let fade = 1.0 - t;
        let alpha = Self::PEAK_ALPHA * fade * fade;
        let veil = Color32::from_rgba_unmultiplied(
            color.r(),
            color.g(),
            color.b(),
            (alpha * 255.0).round() as u8,
        );
        // A dedicated foreground layer: z-ordered above the panels regardless
        // of when in the frame this runs, and it never registers interaction,
        // so the UI underneath stays fully clickable throughout the fade.
        ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("app-theme-fade"),
        ))
        .rect_filled(ctx.viewport_rect(), 0.0, veil);
        true
    }
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

/// Bright neutral grays, dark text, blue selection/accent.
fn daylight_visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::light();
    let accent = ACCENT;
    visuals.override_text_color = Some(Color32::from_rgb(32, 36, 44));
    visuals.panel_fill = Color32::from_rgb(244, 246, 250);
    visuals.window_fill = Color32::from_rgb(255, 255, 255);
    visuals.faint_bg_color = Color32::from_rgb(236, 239, 245);
    visuals.extreme_bg_color = Color32::from_rgb(255, 255, 255);
    visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(244, 246, 250);
    visuals.widgets.noninteractive.weak_bg_fill = Color32::from_rgb(244, 246, 250);
    visuals.widgets.noninteractive.fg_stroke.color = SECONDARY_TEXT;
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(255, 255, 255);
    visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(255, 255, 255);
    visuals.widgets.inactive.fg_stroke.color = Color32::from_rgb(32, 36, 44);
    visuals.widgets.hovered.bg_fill = ACCENT_HOVER;
    visuals.widgets.hovered.weak_bg_fill = ACCENT_HOVER;
    visuals.widgets.hovered.fg_stroke.color = Color32::from_rgb(24, 40, 72);
    visuals.widgets.active.bg_fill = accent;
    visuals.widgets.active.weak_bg_fill = accent;
    visuals.widgets.active.fg_stroke.color = Color32::WHITE;
    visuals.widgets.open.bg_fill = Color32::from_rgb(236, 239, 245);
    visuals.widgets.open.weak_bg_fill = Color32::from_rgb(236, 239, 245);
    visuals.selection.bg_fill = ACCENT_SOFT;
    visuals.selection.stroke.color = accent;
    visuals.hyperlink_color = accent;
    visuals.window_shadow = egui::Shadow::NONE;
    visuals.popup_shadow = egui::Shadow::NONE;
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

    #[test]
    fn daylight_is_light_with_blue_accent_and_no_shadow() {
        let visuals = AppTheme::Daylight.visuals();
        assert!(!visuals.dark_mode);
        assert_eq!(visuals.hyperlink_color, ACCENT);
        assert_eq!(visuals.window_shadow, egui::Shadow::NONE);
        assert_eq!(visuals.popup_shadow, egui::Shadow::NONE);
    }
}
