use super::DrillApp;
use super::app_theme::AppTheme;
use eframe::egui::{self};
use eframe::egui_wgpu::{SurfaceConfig, WgpuConfiguration};
use std::sync::Arc;

pub fn run() -> eframe::Result {
    drill_project::crash::install_hook(
        super::project_state::app_data_dir(),
        env!("CARGO_PKG_VERSION"),
    );
    eframe::run_native(
        "DrillForge",
        native_options(),
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

/// Window and renderer configuration, split out from [`run`] so the
/// latency-critical surface settings are asserted by a test instead of being
/// taken on trust.
fn native_options() -> eframe::NativeOptions {
    eframe::NativeOptions {
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
        // Trade queued GPU throughput for responsiveness.
        //
        // egui-wgpu defaults to `SurfaceConfig::HIGH_THROUGHPUT`
        // (`desired_maximum_frame_latency: Some(2)`), which lets the
        // presentation engine keep two frames in flight. That hides GPU
        // hitches in renderers that do heavy per-frame work, at the cost of
        // one extra frame between a pointer event and the pixels that answer
        // it -- ~7ms at 144Hz, ~17ms at 60Hz, on top of the frame we are
        // already building.
        //
        // This editor does trivial GPU work per frame: a couple of thousand
        // instanced dots and egui's own UI mesh. Throughput has never been
        // the constraint here, so the queued frame buys nothing and costs
        // perceived latency on every drag of a performer. `LOW_LATENCY`
        // (same `AutoVsync` present mode, `desired_maximum_frame_latency:
        // Some(1)`) is documented upstream as the "good default for GUIs with
        // very little (or no) extra GPU work" -- exactly this app.
        //
        // Unused under the Glow screenshot path above; `NativeOptions` simply
        // ignores `wgpu_options` when the Glow renderer is selected.
        wgpu_options: WgpuConfiguration::default().with_surface_config(SurfaceConfig::LOW_LATENCY),
        ..Default::default()
    }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression guard for the input-latency configuration. egui-wgpu's
    /// default is `HIGH_THROUGHPUT`, so this silently reverts to two queued
    /// frames if the explicit setting is ever dropped during a dependency
    /// bump or a refactor of `native_options`.
    #[test]
    fn surface_is_configured_for_low_latency_presentation() {
        let surface = native_options().wgpu_options.surface;
        assert_eq!(surface, SurfaceConfig::LOW_LATENCY);
        assert_eq!(
            surface.desired_maximum_frame_latency,
            Some(1),
            "one queued frame is the whole point of the setting"
        );
        assert_ne!(
            surface,
            SurfaceConfig::HIGH_THROUGHPUT,
            "egui-wgpu's default must not be what we ship"
        );
        // Vsync stays on: the goal is to shorten the queue, not to tear.
        assert_eq!(
            surface.present_mode,
            eframe::wgpu::PresentMode::AutoVsync,
            "low latency must not come from disabling vsync"
        );
    }

    /// The deterministic screenshot harness only exists in eframe's Glow
    /// integration, so the wgpu surface tuning must not be what selects the
    /// renderer.
    #[test]
    fn renderer_selection_is_independent_of_surface_tuning() {
        // Not asserting on the env var itself (tests share a process); this
        // pins the shape of the option so a Glow build still gets a valid,
        // simply-unused wgpu configuration rather than a panic or a default
        // that contradicts the comment above it.
        let options = native_options();
        assert_eq!(options.wgpu_options.surface, SurfaceConfig::LOW_LATENCY);
    }
}
