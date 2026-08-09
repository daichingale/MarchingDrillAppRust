//! Thin egui-wgpu adapter. GPU state remains reconstructable and never owns document data.

use drill_core::stadium::{Lighting, LodThresholds};
use drill_gpu::{GpuFrame, GpuHealth, GpuHealthHandle, GpuRenderer, GpuView};
use eframe::{egui, egui_wgpu};
use std::sync::{Arc, Mutex};

struct Resource(GpuRenderer);

pub(crate) struct Bridge {
    frame: Arc<Mutex<GpuFrame>>,
    health: GpuHealthHandle,
    enabled: bool,
}

impl Bridge {
    pub(crate) fn install(cc: &eframe::CreationContext<'_>) -> Option<Self> {
        let state = cc.wgpu_render_state.as_ref()?;
        let renderer = GpuRenderer::new(&state.device, state.target_format, 1);
        let health = renderer.health_handle();
        state
            .renderer
            .write()
            .callback_resources
            .insert(Resource(renderer));
        Some(Self {
            frame: Arc::new(Mutex::new(GpuFrame::new())),
            health,
            enabled: true,
        })
    }
    pub(crate) fn available(&self) -> bool {
        self.health.get() == GpuHealth::Healthy
    }
    pub(crate) fn active(&self) -> bool {
        self.enabled && self.available()
    }
    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }
    pub(crate) fn update(&self, list: &drill_render::DisplayList) {
        if let Ok(mut frame) = self.frame.try_lock() {
            frame.update_from_display_list(list);
        }
    }
    pub(crate) fn update_stadium(
        &self,
        document: &drill_core::Document,
        positions: &[drill_core::Point],
        camera: drill_core::camera::Camera,
    ) {
        if let Ok(mut frame) = self.frame.try_lock() {
            frame.update_stadium_view(document, positions, Some(camera));
        }
    }
    pub(crate) fn callback(&self, rect: egui::Rect) -> egui::Shape {
        self.callback_view(rect, GpuView::Field2D)
    }
    pub(crate) fn callback_stadium(
        &self,
        rect: egui::Rect,
        camera: drill_core::camera::Camera,
        lighting: Lighting,
    ) -> egui::Shape {
        self.callback_view(
            rect,
            GpuView::Stadium3D {
                camera,
                lighting,
                lod: LodThresholds::default(),
            },
        )
    }
    fn callback_view(&self, rect: egui::Rect, view: GpuView) -> egui::Shape {
        egui_wgpu::Callback::new_paint_callback(
            rect,
            Paint {
                frame: Arc::clone(&self.frame),
                viewport_points: [rect.width(), rect.height()],
                view,
            },
        )
        .into()
    }
}

struct Paint {
    frame: Arc<Mutex<GpuFrame>>,
    viewport_points: [f32; 2],
    view: GpuView,
}
impl egui_wgpu::CallbackTrait for Paint {
    fn prepare(
        &self,
        device: &eframe::wgpu::Device,
        queue: &eframe::wgpu::Queue,
        screen: &egui_wgpu::ScreenDescriptor,
        encoder: &mut eframe::wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<eframe::wgpu::CommandBuffer> {
        let viewport = [
            (self.viewport_points[0] * screen.pixels_per_point)
                .round()
                .max(0.0) as u32,
            (self.viewport_points[1] * screen.pixels_per_point)
                .round()
                .max(0.0) as u32,
        ];
        if let (Some(renderer), Ok(frame)) =
            (resources.get_mut::<Resource>(), self.frame.try_lock())
        {
            let prepared = renderer.0.prepare_view(
                device,
                queue,
                &frame,
                viewport,
                screen.pixels_per_point,
                self.view,
            );
            if prepared && matches!(self.view, GpuView::Stadium3D { .. }) {
                renderer.0.encode_offscreen_3d(device, encoder, viewport);
            }
        }
        Vec::new()
    }
    fn paint(
        &self,
        _: egui::PaintCallbackInfo,
        pass: &mut eframe::wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        if let Some(renderer) = resources.get::<Resource>() {
            if matches!(self.view, GpuView::Stadium3D { .. }) {
                renderer.0.draw_offscreen(pass);
            } else {
                renderer.0.draw(pass);
            }
        }
    }
}
