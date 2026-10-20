//! Phone and browser field for DrillForge.
//!
//! The desktop crate cannot be compiled to wasm as it stands: native file
//! dialogs, the audio device, Blender, and worker threads are all in that
//! binary. This crate keeps the part a phone needs — the document, the field
//! map, and the same sync values the desktop publishes — and draws it with
//! eframe's WebGL backend.

mod app;
mod touch;

pub use app::{PhoneApp, synced_pair};
pub use touch::{FieldView, apply_two_finger, chrome_is_compact, hit_radius, pick_performer};

use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
pub fn run() -> eframe::Result {
    let size = window_size();
    eframe::run_native(
        "DrillForge",
        eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default()
                .with_title("DrillForge")
                .with_inner_size(size)
                .with_min_inner_size([320.0, 320.0]),
            renderer: eframe::Renderer::Glow,
            ..Default::default()
        },
        Box::new(|creation| {
            install_fonts(&creation.egui_ctx);
            Ok(Box::new(PhoneApp::new()))
        }),
    )
}

#[cfg(target_arch = "wasm32")]
pub fn run() -> eframe::Result {
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn window_size() -> [f32; 2] {
    if let Ok(raw) = std::env::var("DRILLFORGE_QA_SIZE") {
        let mut parts = raw.split('x');
        if let (Some(width), Some(height)) = (parts.next(), parts.next())
            && let (Ok(width), Ok(height)) = (width.parse::<f32>(), height.parse::<f32>())
            && (200.0..=2560.0).contains(&width)
            && (200.0..=2000.0).contains(&height)
        {
            return [width, height];
        }
    }
    [1280.0, 800.0]
}

fn install_fonts(context: &eframe::egui::Context) {
    let mut fonts = eframe::egui::FontDefinitions::default();
    fonts.font_data.insert(
        "Noto Sans JP".into(),
        Arc::new(eframe::egui::FontData::from_static(include_bytes!(
            "../../../assets/NotoSansJP.ttf"
        ))),
    );
    for family in [
        eframe::egui::FontFamily::Proportional,
        eframe::egui::FontFamily::Monospace,
    ] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "Noto Sans JP".into());
    }
    context.set_fonts(fonts);
}

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct WebHandle {
    runner: eframe::WebRunner,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
#[allow(clippy::new_without_default)]
impl WebHandle {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            runner: eframe::WebRunner::new(),
        }
    }

    #[wasm_bindgen]
    pub async fn start(
        &self,
        canvas: web_sys::HtmlCanvasElement,
    ) -> Result<(), wasm_bindgen::JsValue> {
        self.runner
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|creation| {
                    install_fonts(&creation.egui_ctx);
                    Ok(Box::new(PhoneApp::new()))
                }),
            )
            .await
    }
}
