//! Surface- and UI-independent wgpu instanced performer renderer.

use bytemuck::{Pod, Zeroable};
use drill_core::{
    Document, Point,
    camera::Camera,
    roster::PerformerKind,
    stadium::{Lighting, LodThresholds, PerformerLod},
};
use drill_render::{DisplayList, DrawCmd, Rgba};
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};
use wgpu::util::DeviceExt;

pub const MAX_PERFORMERS: usize = 16_384;
/// Kept public so CI can validate the exact shader contract without an adapter.
pub const PERFORMER_SHADER_WGSL: &str = include_str!("shader.wgsl");

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct PerformerInstance {
    pub world: [f32; 3],
    pub radius: f32,
    pub fill: u32,
    pub stroke: u32,
    pub flags: u32,
    pub _pad: u32,
}
const _: () = assert!(std::mem::size_of::<PerformerInstance>() == 32);

pub struct InstanceFlags;
impl InstanceFlags {
    pub const SYMBOL_MASK: u32 = 0x0f;
    pub const SELECTED: u32 = 0x10;
    pub const HOVERED: u32 = 0x20;
    pub const WARN_MASK: u32 = 0xc0;
    pub const GHOST: u32 = 0x100;
    pub const DIMMED: u32 = 0x200;
    pub const INDEX_SHIFT: u32 = 12;
    pub const INDEX_MASK: u32 = 0xffff_f000;
    pub const KIND_SHIFT: u32 = 10;
    pub const KIND_MASK: u32 = 0xc00;
    pub fn with_index(flags: u32, index: usize) -> u32 {
        (flags & !Self::INDEX_MASK) | ((index.min(0x000f_ffff) as u32) << Self::INDEX_SHIFT)
    }
    pub fn index(flags: u32) -> usize {
        ((flags & Self::INDEX_MASK) >> Self::INDEX_SHIFT) as usize
    }
}

fn kind_flags(kind: PerformerKind) -> u32 {
    (match kind {
        PerformerKind::Wind => 0,
        PerformerKind::Percussion => 1,
        PerformerKind::Guard => 2,
        PerformerKind::Prop => 3,
    }) << InstanceFlags::KIND_SHIFT
}

/// Public CPU mirror of shader classification, used by fallback/parity tests.
pub fn classify_lod(screen_height_px: f32, thresholds: LodThresholds) -> PerformerLod {
    drill_core::stadium::choose_lod(screen_height_px, &thresholds)
}

/// CPU reference for the WGSL lighting equation. Keeping this public makes
/// cross-backend visual parity testable without requiring a physical adapter.
pub fn shade_for_fallback(rgb: [u8; 3], lighting: Lighting, distance_m: f32) -> [u8; 3] {
    drill_core::stadium::shade_color(rgb, lighting, distance_m)
}

/// CPU mirror of the 2D vertex path, returning physical framebuffer pixels.
pub fn project_field_2d(instance: &PerformerInstance, pixels_per_point: f32) -> [f32; 2] {
    [
        instance.world[0] * pixels_per_point,
        instance.world[1] * pixels_per_point,
    ]
}

/// CPU mirror of the stadium shader's world-to-screen projection.
pub fn project_stadium(camera: &Camera, world: [f32; 3], viewport: [u32; 2]) -> Option<[f32; 2]> {
    if viewport.contains(&0) {
        return None;
    }
    let aspect = viewport[0] as f32 / viewport[1] as f32;
    let matrix = multiply(perspective(camera, aspect), camera.view_matrix());
    let clip = mul_vec4(matrix, [world[0], world[1], world[2], 1.0]);
    if clip[3] <= f32::EPSILON {
        return None;
    }
    let ndc = [clip[0] / clip[3], clip[1] / clip[3], clip[2] / clip[3]];
    if !(0.0..=1.0).contains(&ndc[2]) {
        return None;
    }
    Some([
        (ndc[0] * 0.5 + 0.5) * viewport[0] as f32,
        (0.5 - ndc[1] * 0.5) * viewport[1] as f32,
    ])
}

/// Projected upright performer height used by both the WGSL and CPU LOD policy.
pub fn projected_height_px(
    camera: &Camera,
    foot: [f32; 3],
    height_m: f32,
    viewport: [u32; 2],
) -> Option<f32> {
    let base = project_stadium(camera, foot, viewport)?;
    let head = project_stadium(
        camera,
        [foot[0], foot[1] + height_m.max(0.0), foot[2]],
        viewport,
    )?;
    Some((head[1] - base[1]).abs())
}

fn rgba(v: Rgba) -> u32 {
    u32::from(v.0) | u32::from(v.1) << 8 | u32::from(v.2) << 16 | u32::from(v.3) << 24
}

/// Reusable upload packet in DisplayList-local logical coordinates.
#[derive(Debug, Default)]
pub struct GpuFrame {
    instances: Vec<PerformerInstance>,
    dropped_nonfinite: usize,
    truncated: bool,
}
impl GpuFrame {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn instances(&self) -> &[PerformerInstance] {
        &self.instances
    }
    pub fn capacity(&self) -> usize {
        self.instances.capacity()
    }
    pub fn dropped_nonfinite(&self) -> usize {
        self.dropped_nonfinite
    }
    pub fn truncated(&self) -> bool {
        self.truncated
    }
    /// Exact DisplayList dot centers are retained, ensuring CPU/GPU field-map parity.
    pub fn update_from_display_list(&mut self, list: &DisplayList) {
        self.instances.clear();
        self.dropped_nonfinite = 0;
        self.truncated = false;
        let count = list
            .commands()
            .iter()
            .filter(|c| matches!(c, DrawCmd::Dot { .. }))
            .count()
            .min(MAX_PERFORMERS);
        self.instances
            .reserve(count.saturating_sub(self.instances.capacity()));
        for command in list.commands() {
            let DrawCmd::Dot {
                center,
                radius,
                fill,
                stroke,
            } = *command
            else {
                continue;
            };
            if self.instances.len() == MAX_PERFORMERS {
                self.truncated = true;
                break;
            }
            if !center.x.is_finite() || !center.y.is_finite() || !radius.is_finite() {
                self.dropped_nonfinite += 1;
                continue;
            }
            let index = self.instances.len();
            self.instances.push(PerformerInstance {
                world: [center.x, center.y, 0.0],
                radius: radius.max(0.0),
                fill: rgba(fill),
                stroke: rgba(stroke),
                flags: InstanceFlags::with_index(0, index),
                _pad: 0,
            });
        }
    }

    /// Build world-space stadium instances without allocating after warm-up.
    /// `radius` stores performer height in metres in this mode.
    pub fn update_stadium(&mut self, document: &Document, positions: &[Point]) {
        self.update_stadium_view(document, positions, None);
    }

    /// Camera-aware variant. Far-to-near ordering is retained as a deterministic
    /// fallback for hosts without the offscreen depth pass.
    pub fn update_stadium_view(
        &mut self,
        document: &Document,
        positions: &[Point],
        camera: Option<Camera>,
    ) {
        self.instances.clear();
        self.dropped_nonfinite = 0;
        self.truncated = positions.len() > MAX_PERFORMERS;
        let count = positions
            .len()
            .min(document.performers.len())
            .min(MAX_PERFORMERS);
        self.instances
            .reserve(count.saturating_sub(self.instances.capacity()));
        for (index, (&point, performer)) in positions
            .iter()
            .zip(&document.performers)
            .take(count)
            .enumerate()
        {
            if !point.x.is_finite() || !point.y.is_finite() || !performer.height_m.is_finite() {
                self.dropped_nonfinite += 1;
                continue;
            }
            let color = performer.resolved_color(&document.sections);
            let packed = u32::from(color[0])
                | u32::from(color[1]) << 8
                | u32::from(color[2]) << 16
                | 0xff00_0000;
            self.instances.push(PerformerInstance {
                world: drill_core::camera::field_to_world(point, 0.0),
                radius: performer.height_m.clamp(0.2, 3.0),
                fill: packed,
                stroke: 0xffff_ffff,
                flags: InstanceFlags::with_index(kind_flags(performer.kind), index),
                _pad: 0,
            });
        }
        if let Some(camera) = camera {
            let eye = camera.position();
            self.instances.sort_unstable_by(|a, b| {
                distance_squared(b.world, eye).total_cmp(&distance_squared(a.world, eye))
            });
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum GpuHealth {
    Healthy = 0,
    Degraded = 1,
}
#[derive(Clone, Debug)]
pub struct GpuHealthHandle(Arc<AtomicU8>);
impl Default for GpuHealthHandle {
    fn default() -> Self {
        Self(Arc::new(AtomicU8::new(0)))
    }
}
impl GpuHealthHandle {
    pub fn get(&self) -> GpuHealth {
        if self.0.load(Ordering::Acquire) == 0 {
            GpuHealth::Healthy
        } else {
            GpuHealth::Degraded
        }
    }
    pub fn degrade(&self) {
        self.0.store(1, Ordering::Release);
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ViewUniform {
    viewport: [f32; 2],
    pixels_per_point: f32,
    mode: u32,
    view_projection: [[f32; 4]; 4],
    camera_right: [f32; 4],
    lighting: [f32; 4],
    sky_tint: [f32; 4],
}

#[derive(Clone, Copy, Debug)]
pub enum GpuView {
    Field2D,
    Stadium3D {
        camera: Camera,
        lighting: Lighting,
        lod: LodThresholds,
    },
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderStats {
    pub uploaded_instances: u32,
    pub instance_capacity: u32,
    pub buffer_reallocations: u32,
}

pub struct GpuRenderer {
    pipeline: wgpu::RenderPipeline,
    depth_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,
    composite_layout: wgpu::BindGroupLayout,
    composite_sampler: wgpu::Sampler,
    offscreen: Option<Offscreen>,
    bind_group: wgpu::BindGroup,
    uniform: wgpu::Buffer,
    instances: wgpu::Buffer,
    capacity: usize,
    count: u32,
    health: GpuHealthHandle,
    stats: RenderStats,
}

struct Offscreen {
    size: [u32; 2],
    color: wgpu::Texture,
    depth: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}
impl GpuRenderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, samples: u32) -> Self {
        let health = GpuHealthHandle::default();
        let lost = health.clone();
        device.set_device_lost_callback(move |_, _| lost.degrade());
        let error = health.clone();
        device.on_uncaptured_error(Arc::new(move |_| error.degrade()));
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("drill view"),
            contents: bytemuck::bytes_of(&ViewUniform {
                viewport: [1.0; 2],
                pixels_per_point: 1.0,
                mode: 0,
                view_projection: identity(),
                camera_right: [1.0, 0.0, 0.0, 0.0],
                lighting: [0.0, 0.0, 1.0, 0.0],
                sky_tint: [0.0, 0.0, 0.0, 0.0],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("drill view"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("drill view"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("drill sdf"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("drill pipeline"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let make_pipeline = |depth: bool| {
            let vertex_layout = wgpu::VertexBufferLayout {
                array_stride: 32,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![0=>Float32x3, 1=>Float32, 2=>Uint32, 3=>Uint32, 4=>Uint32],
            };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("drill performers"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[vertex_layout],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: depth.then_some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: samples.max(1),
                    ..Default::default()
                },
                multiview_mask: None,
                cache: None,
            })
        };
        let pipeline = make_pipeline(false);
        let depth_pipeline = make_pipeline(true);
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("drill offscreen composite"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("drill offscreen composite"),
            source: wgpu::ShaderSource::Wgsl(include_str!("composite.wgsl").into()),
        });
        let composite_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("drill offscreen composite"),
                bind_group_layouts: &[Some(&composite_layout)],
                immediate_size: 0,
            });
        let composite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("drill offscreen composite"),
            layout: Some(&composite_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &composite_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &composite_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let composite_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("drill offscreen composite"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let capacity = 1024;
        let instances = make_instance_buffer(device, capacity);
        Self {
            pipeline,
            depth_pipeline,
            composite_pipeline,
            composite_layout,
            composite_sampler,
            offscreen: None,
            bind_group,
            uniform,
            instances,
            capacity,
            count: 0,
            health,
            stats: RenderStats {
                instance_capacity: capacity as u32,
                ..Default::default()
            },
        }
    }
    pub fn health_handle(&self) -> GpuHealthHandle {
        self.health.clone()
    }
    pub fn stats(&self) -> RenderStats {
        self.stats
    }
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &GpuFrame,
        viewport: [u32; 2],
        ppp: f32,
    ) -> bool {
        self.prepare_view(device, queue, frame, viewport, ppp, GpuView::Field2D)
    }

    pub fn prepare_view(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &GpuFrame,
        viewport: [u32; 2],
        ppp: f32,
        view: GpuView,
    ) -> bool {
        if self.health.get() == GpuHealth::Degraded || viewport.contains(&0) {
            self.count = 0;
            return false;
        }
        let count = frame.instances.len().min(MAX_PERFORMERS);
        if count > self.capacity {
            self.capacity = count.next_power_of_two().min(MAX_PERFORMERS);
            self.instances = make_instance_buffer(device, self.capacity);
            self.stats.buffer_reallocations += 1;
            self.stats.instance_capacity = self.capacity as u32;
        }
        let (mode, view_projection, camera_right, lighting, sky_tint) = match view {
            GpuView::Field2D => (
                0,
                identity(),
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0; 4],
            ),
            GpuView::Stadium3D {
                camera,
                lighting,
                lod,
            } => {
                let aspect = viewport[0] as f32 / viewport[1] as f32;
                let view = camera.view_matrix();
                let projection = perspective(&camera, aspect);
                let vp = multiply(projection, view);
                let eye = camera.position();
                let forward = normalize3([
                    camera.target[0] - eye[0],
                    camera.target[1] - eye[1],
                    camera.target[2] - eye[2],
                ]);
                let right = normalize3([forward[2], 0.0, -forward[0]]);
                (
                    1,
                    transpose(vp),
                    [right[0], right[1], right[2], 0.0],
                    [
                        lighting.sun_elevation_deg.to_radians(),
                        lighting.ambient,
                        lighting.fog_density,
                        lod.simple_figure_px,
                    ],
                    [
                        lighting.sky_tint[0],
                        lighting.sky_tint[1],
                        lighting.sky_tint[2],
                        lod.silhouette_px,
                    ],
                )
            }
        };
        queue.write_buffer(
            &self.uniform,
            0,
            bytemuck::bytes_of(&ViewUniform {
                viewport: [viewport[0] as f32, viewport[1] as f32],
                pixels_per_point: ppp.max(0.01),
                mode,
                view_projection,
                camera_right,
                lighting,
                sky_tint,
            }),
        );
        if count > 0 {
            queue.write_buffer(
                &self.instances,
                0,
                bytemuck::cast_slice(&frame.instances[..count]),
            );
        }
        self.count = count as u32;
        self.stats.uploaded_instances = self.count;
        true
    }
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.count == 0 || self.health.get() == GpuHealth::Degraded {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.instances.slice(..));
        pass.draw(0..4, 0..self.count);
    }

    /// Render a 3D view with a real depth attachment. The resulting transparent
    /// texture is composited by `draw_offscreen` in the host UI pass.
    pub fn encode_offscreen_3d(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        viewport: [u32; 2],
    ) -> bool {
        if self.count == 0 || self.health.get() == GpuHealth::Degraded || viewport.contains(&0) {
            return false;
        }
        if self
            .offscreen
            .as_ref()
            .is_none_or(|target| target.size != viewport)
        {
            self.offscreen = Some(make_offscreen(
                device,
                &self.composite_layout,
                &self.composite_sampler,
                viewport,
            ));
        }
        let target = self
            .offscreen
            .as_ref()
            .expect("offscreen target initialized");
        let color = target.color.create_view(&Default::default());
        let depth = target.depth.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("drill stadium depth pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &color,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.depth_pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.instances.slice(..));
        pass.draw(0..4, 0..self.count);
        true
    }

    pub fn draw_offscreen(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(target) = &self.offscreen else {
            return;
        };
        if self.health.get() == GpuHealth::Degraded {
            return;
        }
        pass.set_pipeline(&self.composite_pipeline);
        pass.set_bind_group(0, &target.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

fn make_offscreen(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    size: [u32; 2],
) -> Offscreen {
    let color = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("drill stadium color"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("drill stadium depth"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = color.create_view(&Default::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("drill stadium composite"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    Offscreen {
        size,
        color,
        depth,
        bind_group,
    }
}

fn identity() -> [[f32; 4]; 4] {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}
fn perspective(camera: &Camera, aspect: f32) -> [[f32; 4]; 4] {
    let t = (camera.fov_y_rad * 0.5).tan();
    let (n, f) = (camera.near, camera.far);
    [
        [1.0 / (aspect * t), 0.0, 0.0, 0.0],
        [0.0, 1.0 / t, 0.0, 0.0],
        // WebGPU uses a zero-to-one NDC depth range.
        [0.0, 0.0, f / (n - f), f * n / (n - f)],
        [0.0, 0.0, -1.0, 0.0],
    ]
}
fn multiply(a: [[f32; 4]; 4], b: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut out = [[0.0; 4]; 4];
    for r in 0..4 {
        for c in 0..4 {
            for k in 0..4 {
                out[r][c] += a[r][k] * b[k][c];
            }
        }
    }
    out
}
fn mul_vec4(matrix: [[f32; 4]; 4], value: [f32; 4]) -> [f32; 4] {
    std::array::from_fn(|row| {
        matrix[row][0] * value[0]
            + matrix[row][1] * value[1]
            + matrix[row][2] * value[2]
            + matrix[row][3] * value[3]
    })
}
fn transpose(a: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut out = [[0.0; 4]; 4];
    for r in 0..4 {
        for c in 0..4 {
            out[r][c] = a[c][r];
        }
    }
    out
}
fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if n < 1.0e-6 {
        return [1.0, 0.0, 0.0];
    }
    [v[0] / n, v[1] / n, v[2] / n]
}
fn distance_squared(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}
fn make_instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("drill performer instances"),
        size: (capacity * 32) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use drill_render::{BuildScratch, RenderOptions, Scene, Theme, Vec2, Viewport, build_field_2d};
    #[test]
    fn layout_and_flags_are_stable() {
        assert_eq!(std::mem::size_of::<PerformerInstance>(), 32);
        for i in [0, 1, 999, MAX_PERFORMERS - 1] {
            let f = InstanceFlags::with_index(InstanceFlags::SELECTED, i);
            assert_eq!(InstanceFlags::index(f), i);
            assert_ne!(f & InstanceFlags::SELECTED, 0);
        }
    }
    #[test]
    fn stadium_shader_is_valid_wgsl() {
        wgpu::naga::front::wgsl::parse_str(include_str!("shader.wgsl"))
            .expect("stadium shader must parse on every supported adapter");
    }
    #[test]
    fn frame_matches_display_list_and_reuses_memory() {
        let d = drill_core::Document::demo(10, 10);
        let p = d.sets[0].positions.clone();
        let mut l = DisplayList::new();
        build_field_2d(
            &Scene {
                document: &d,
                positions: &p,
                viewport: Viewport {
                    size: Vec2 {
                        x: 1200.0,
                        y: 700.0,
                    },
                    ui_scale: 1.0,
                },
                options: &RenderOptions::default(),
                theme: &Theme::SCREEN_DARK,
            },
            &mut BuildScratch,
            &mut l,
        );
        let expected: Vec<_> = l
            .commands()
            .iter()
            .filter_map(|c| {
                if let DrawCmd::Dot { center, .. } = c {
                    Some(*center)
                } else {
                    None
                }
            })
            .collect();
        let mut f = GpuFrame::new();
        f.update_from_display_list(&l);
        let ptr = f.instances.as_ptr();
        f.update_from_display_list(&l);
        assert_eq!(ptr, f.instances.as_ptr());
        assert_eq!(f.instances.len(), expected.len());
        for (i, c) in f.instances.iter().zip(expected) {
            assert_eq!([i.world[0], i.world[1]], [c.x, c.y]);
        }
    }
    #[test]
    fn health_degrades_monotonically() {
        let h = GpuHealthHandle::default();
        assert_eq!(h.get(), GpuHealth::Healthy);
        h.degrade();
        assert_eq!(h.get(), GpuHealth::Degraded);
    }
    #[test]
    fn stadium_frame_uses_world_coordinates_height_and_reuses_memory() {
        let document = drill_core::Document::demo(10, 10);
        let positions = document.sets[0].positions.clone();
        let mut frame = GpuFrame::new();
        frame.update_stadium(&document, &positions);
        assert_eq!(frame.instances().len(), 100);
        let first = frame.instances()[0];
        assert_eq!(first.world, [positions[0].x, 0.0, positions[0].y]);
        assert_eq!(first.radius, document.performers[0].height_m);
        let ptr = frame.instances().as_ptr();
        frame.update_stadium(&document, &positions);
        assert_eq!(ptr, frame.instances().as_ptr());
    }

    #[test]
    fn stadium_kind_lod_and_lighting_match_cpu_policy() {
        let mut document = drill_core::Document::demo(1, 4);
        document.performers[0].kind = PerformerKind::Wind;
        document.performers[1].kind = PerformerKind::Percussion;
        document.performers[2].kind = PerformerKind::Guard;
        document.performers[3].kind = PerformerKind::Prop;
        let positions = document.sets[0].positions.clone();
        let mut frame = GpuFrame::new();
        frame.update_stadium(&document, &positions);
        for (i, instance) in frame.instances().iter().enumerate() {
            assert_eq!(
                (instance.flags & InstanceFlags::KIND_MASK) >> InstanceFlags::KIND_SHIFT,
                i as u32
            );
        }
        let thresholds = LodThresholds::default();
        for px in [0.0, 9.99, 10.0, 39.99, 40.0, 100.0] {
            assert_eq!(
                classify_lod(px, thresholds),
                drill_core::stadium::choose_lod(px, &thresholds)
            );
        }
        let lighting = Lighting::preset(drill_core::stadium::Weather::Overcast);
        assert_eq!(
            shade_for_fallback([200, 120, 40], lighting, 75.0),
            drill_core::stadium::shade_color([200, 120, 40], lighting, 75.0)
        );
    }

    #[test]
    fn camera_matrix_maps_target_in_front_of_camera() {
        let camera = Camera::default();
        let vp = multiply(perspective(&camera, 16.0 / 9.0), camera.view_matrix());
        let p = camera.target;
        let clip = [
            vp[0][0] * p[0] + vp[0][1] * p[1] + vp[0][2] * p[2] + vp[0][3],
            vp[1][0] * p[0] + vp[1][1] * p[1] + vp[1][2] * p[2] + vp[1][3],
            vp[2][0] * p[0] + vp[2][1] * p[1] + vp[2][2] * p[2] + vp[2][3],
            vp[3][0] * p[0] + vp[3][1] * p[1] + vp[3][2] * p[2] + vp[3][3],
        ];
        assert!(clip[3] > 0.0);
        assert!((clip[0] / clip[3]).abs() < 1.0e-5);
        assert!((clip[1] / clip[3]).abs() < 1.0e-5);
    }

    #[test]
    fn perspective_uses_webgpu_zero_to_one_depth() {
        let camera = Camera::default();
        let projection = perspective(&camera, 1.0);
        let ndc_z = |distance: f32| {
            let z = -distance;
            let clip_z = projection[2][2] * z + projection[2][3];
            let clip_w = projection[3][2] * z;
            clip_z / clip_w
        };
        assert!(ndc_z(camera.near).abs() < 1.0e-5);
        assert!((ndc_z(camera.far) - 1.0).abs() < 1.0e-5);
    }
}
