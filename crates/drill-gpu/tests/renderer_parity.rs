use drill_core::{
    Document, Point,
    camera::Camera,
    roster::PerformerKind,
    stadium::{Lighting, LodThresholds, PerformerLod, Weather, choose_lod},
};
use drill_gpu::{
    GpuFrame, InstanceFlags, PERFORMER_SHADER_WGSL, classify_lod, project_field_2d,
    project_stadium, projected_height_px, shade_for_fallback,
};
use drill_render::{
    BuildScratch, DisplayList, DrawCmd, RenderOptions, Scene, Theme, Vec2, Viewport, build_field_2d,
};

const EPSILON_PX: f32 = 0.002;

fn close(a: f32, b: f32) {
    assert!((a - b).abs() <= EPSILON_PX, "{a} != {b}");
}

#[test]
fn field_2d_matrix_preserves_cpu_centers_colors_radius_and_dpi() {
    let document = Document::demo(3, 4);
    let positions = &document.sets[0].positions;
    for (width, height, dpi) in [
        (640.0, 480.0, 1.0),
        (1280.0, 720.0, 1.25),
        (1920.0, 1080.0, 1.5),
        (3840.0, 2160.0, 2.0),
    ] {
        let mut display = DisplayList::new();
        build_field_2d(
            &Scene {
                document: &document,
                positions,
                viewport: Viewport {
                    size: Vec2 {
                        x: width,
                        y: height,
                    },
                    ui_scale: dpi,
                },
                options: &RenderOptions::default(),
                theme: &Theme::SCREEN_DARK,
            },
            &mut BuildScratch,
            &mut display,
        );
        let cpu: Vec<_> = display
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCmd::Dot {
                    center,
                    radius,
                    fill,
                    stroke,
                } => Some((*center, *radius, *fill, *stroke)),
                _ => None,
            })
            .collect();
        let mut gpu = GpuFrame::new();
        gpu.update_from_display_list(&display);
        assert_eq!(cpu.len(), gpu.instances().len());
        for ((center, radius, fill, stroke), instance) in cpu.iter().zip(gpu.instances()) {
            close(instance.world[0], center.x);
            close(instance.world[1], center.y);
            close(instance.radius, *radius);
            assert_eq!(
                instance.fill.to_le_bytes(),
                [fill.0, fill.1, fill.2, fill.3]
            );
            assert_eq!(
                instance.stroke.to_le_bytes(),
                [stroke.0, stroke.1, stroke.2, stroke.3]
            );
            let physical = project_field_2d(instance, dpi);
            close(physical[0], center.x * dpi);
            close(physical[1], center.y * dpi);
        }
    }
}

#[test]
fn stadium_matrix_matches_projection_kind_lod_lighting_and_presets() {
    let mut document = Document::demo(1, 4);
    let kinds = [
        PerformerKind::Wind,
        PerformerKind::Percussion,
        PerformerKind::Guard,
        PerformerKind::Prop,
    ];
    for (performer, kind) in document.performers.iter_mut().zip(kinds) {
        performer.kind = kind;
    }
    let positions = document.sets[0].positions.clone();
    let grid = &document.grid;
    let cameras = [
        Camera::audience_view(grid),
        Camera::press_box(grid),
        Camera::overhead(grid),
        Camera::end_zone(grid, true),
        Camera::end_zone(grid, false),
        Camera::performer_pov(
            Point {
                x: grid.width * 0.5,
                y: 0.0,
            },
            0.0,
            20.0,
        ),
    ];
    for camera in cameras {
        let mut frame = GpuFrame::new();
        frame.update_stadium_view(&document, &positions, Some(camera));
        for instance in frame.instances() {
            let index = InstanceFlags::index(instance.flags);
            let expected_kind = kinds[index] as u32;
            assert_eq!(
                (instance.flags & InstanceFlags::KIND_MASK) >> InstanceFlags::KIND_SHIFT,
                expected_kind
            );
            for viewport in [[640, 480], [1920, 1080], [3840, 2160]] {
                let gpu = project_stadium(&camera, instance.world, viewport);
                let cpu = camera.project(instance.world, viewport[0] as f32, viewport[1] as f32);
                assert_eq!(gpu.is_some(), cpu.is_some());
                if let (Some(gpu), Some(cpu)) = (gpu, cpu) {
                    close(gpu[0], cpu[0]);
                    close(gpu[1], cpu[1]);
                    let height =
                        projected_height_px(&camera, instance.world, instance.radius, viewport)
                            .unwrap();
                    assert_eq!(
                        classify_lod(height, LodThresholds::default()),
                        choose_lod(height, &LodThresholds::default())
                    );
                }
            }
        }
    }

    let thresholds = LodThresholds {
        simple_figure_px: 10.0,
        silhouette_px: 40.0,
    };
    for (px, expected) in [
        (f32::NAN, PerformerLod::Billboard),
        (9.999, PerformerLod::Billboard),
        (10.0, PerformerLod::SimpleFigure),
        (39.999, PerformerLod::SimpleFigure),
        (40.0, PerformerLod::InstrumentSilhouette),
    ] {
        assert_eq!(classify_lod(px, thresholds), expected);
    }
    for weather in [Weather::Clear, Weather::Overcast, Weather::NightLights] {
        let lighting = Lighting::preset(weather);
        for distance in [0.0, 10.0, 75.0, 500.0] {
            assert_eq!(
                shade_for_fallback([7, 128, 251], lighting, distance),
                drill_core::stadium::shade_color([7, 128, 251], lighting, distance)
            );
        }
    }
}

#[test]
fn wgsl_contract_covers_the_cpu_parity_policy() {
    wgpu::naga::front::wgsl::parse_str(PERFORMER_SHADER_WGSL).expect("valid WGSL");
    for contract in [
        "view.mode == 0u",
        "height_px < view.sky_tint.w",
        "height_px < view.lighting.w",
        "(in.flags >> 10u) & 3u",
        "clamp(1.0 - exp(-view.lighting.z",
        "color.rgb * light",
    ] {
        assert!(
            PERFORMER_SHADER_WGSL.contains(contract),
            "missing shader contract: {contract}"
        );
    }
}
