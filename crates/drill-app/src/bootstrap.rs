use super::app_theme::AppTheme;
use super::DrillApp;
use eframe::egui::{self, Vec2};
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
    // The actual color theme is user-selectable (see app_theme.rs) and
    // applied from DrillApp::new / whenever the selection changes; this just
    // seeds a sensible default so there's no unstyled flash before that
    // runs.
    AppTheme::default().apply(context);
    context.all_styles_mut(|style| {
        style.spacing.item_spacing = Vec2::new(8.0, 7.0);
        style.spacing.button_padding = Vec2::new(10.0, 5.0);
    });
}
