use super::DrillApp;
use eframe::egui::{self, Color32, Vec2};
use std::sync::Arc;

pub fn run() -> eframe::Result {
    drill_project::crash::install_hook(
        super::project_state::app_data_dir(),
        env!("CARGO_PKG_VERSION"),
    );
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("DrillForge")
            .with_inner_size([1280.0, 800.0]),
        // eframe's deterministic screenshot harness currently exists only in
        // the Glow integration. Production stays on wgpu.
        renderer: if std::env::var_os("EFRAME_SCREENSHOT_TO").is_some() {
            eframe::Renderer::Glow
        } else {
            eframe::Renderer::Wgpu
        },
        ..Default::default()
    };
    eframe::run_native(
        "DrillForge",
        options,
        Box::new(|creation| {
            install_fonts(&creation.egui_ctx);
            if let Ok(scale) = std::env::var("DRILLFORGE_QA_SCALE")
                && let Ok(scale) = scale.parse::<f32>()
                && (1.0..=2.0).contains(&scale)
            {
                creation.egui_ctx.set_zoom_factor(scale);
            }
            Ok(Box::new(DrillApp::new(creation)))
        }),
    )
}

fn install_fonts(context: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "Noto Sans JP".into(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../../../assets/NotoSansJP.ttf"
        ))),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "Noto Sans JP".into());
    }
    context.set_fonts(fonts);
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
    context.set_visuals(visuals);
    context.all_styles_mut(|style| {
        style.spacing.item_spacing = Vec2::new(8.0, 7.0);
        style.spacing.button_padding = Vec2::new(10.0, 5.0);
    });
}
