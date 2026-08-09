struct View {
    viewport: vec2<f32>, pixels_per_point: f32, mode: u32,
    view_projection: mat4x4<f32>, camera_right: vec4<f32>,
    lighting: vec4<f32>, sky_tint: vec4<f32>
};
@group(0) @binding(0) var<uniform> view: View;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) fill: u32,
    @location(2) @interpolate(flat) stroke: u32,
    @location(3) @interpolate(flat) flags: u32,
    @location(4) @interpolate(flat) lod: u32,
    @location(5) fog: f32,
};

@vertex fn vs_main(@builtin(vertex_index) vertex: u32,
    @location(0) world: vec3<f32>, @location(1) radius: f32,
    @location(2) fill: u32, @location(3) stroke: u32,
    @location(4) flags: u32) -> VertexOut {
    let corner = vec2<f32>(f32(vertex & 1u) * 2.0 - 1.0, f32(vertex >> 1u) * 2.0 - 1.0);
    var out: VertexOut;
    if (view.mode == 0u) {
        let pixel = world.xy * view.pixels_per_point + corner * radius * view.pixels_per_point;
        out.position = vec4<f32>(pixel.x * 2.0 / view.viewport.x - 1.0,
            1.0 - pixel.y * 2.0 / view.viewport.y, 0.0, 1.0);
    } else {
        // Camera-facing upright performer billboard. Radius is height in metres.
        let billboard = view.camera_right.xyz * corner.x * radius * 0.20
            + vec3<f32>(0.0, (corner.y + 1.0) * radius * 0.5, 0.0);
        let foot_clip = view.view_projection * vec4<f32>(world, 1.0);
        let head_clip = view.view_projection * vec4<f32>(world + vec3<f32>(0.0, radius, 0.0), 1.0);
        out.position = view.view_projection * vec4<f32>(world + billboard, 1.0);
        let foot_y = foot_clip.y / max(foot_clip.w, 0.0001);
        let head_y = head_clip.y / max(head_clip.w, 0.0001);
        let height_px = abs(head_y - foot_y) * view.viewport.y * 0.5;
        out.lod = select(select(2u, 1u, height_px < view.sky_tint.w), 0u, height_px < view.lighting.w);
        out.fog = clamp(1.0 - exp(-view.lighting.z * max(foot_clip.w, 0.0)), 0.0, 0.85);
    }
    if (view.mode == 0u) { out.lod = 0u; out.fog = 0.0; }
    out.local = corner; out.fill = fill; out.stroke = stroke; out.flags = flags;
    return out;
}

fn rgba(v: u32) -> vec4<f32> {
    return vec4<f32>(f32(v & 255u), f32((v >> 8u) & 255u),
        f32((v >> 16u) & 255u), f32((v >> 24u) & 255u)) / 255.0;
}
@fragment fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let d = length(in.local) - 1.0;
    let aa = max(fwidth(d), 0.002);
    var alpha = 1.0 - smoothstep(-aa, aa, d);
    if (in.lod == 1u) {
        let head = length((in.local - vec2<f32>(0.0, 0.68)) / vec2<f32>(0.42, 0.22));
        let torso = max(abs(in.local.x) - 0.28, abs(in.local.y + 0.12) - 0.55);
        alpha = max(1.0 - smoothstep(1.0-aa, 1.0+aa, head), 1.0 - smoothstep(-aa, aa, torso));
    } else if (in.lod == 2u) {
        let kind = (in.flags >> 10u) & 3u;
        if (kind == 0u) { alpha = 1.0 - smoothstep(-aa, aa, max(abs(in.local.y)-0.72, abs(in.local.x)-(0.22+0.22*(in.local.y+1.0)))); }
        else if (kind == 1u) { alpha = 1.0 - smoothstep(-aa, aa, max(abs(in.local.x)-0.72, abs(in.local.y)-0.48)); }
        else if (kind == 2u) { alpha = 1.0 - smoothstep(-aa, aa, max(abs(in.local.x + in.local.y*0.30)-0.48, abs(in.local.y)-0.85)); }
        else { alpha = 1.0 - smoothstep(-aa, aa, max(abs(in.local.x)-0.65, abs(in.local.y)-0.75)); }
    }
    var color = rgba(in.fill);
    let light = clamp(view.lighting.y + (1.0-view.lighting.y)*max(sin(view.lighting.x),0.0), 0.0, 1.0);
    color = vec4<f32>(mix(color.rgb * light, view.sky_tint.rgb, in.fog), color.a);
    if ((in.flags & 0xf0u) != 0u) {
        let ring_alpha = 1.0 - smoothstep(-aa, aa, abs(d + 0.10) - 0.09);
        color = mix(color, rgba(in.stroke), ring_alpha); alpha = max(alpha, ring_alpha);
    }
    if (alpha <= 0.0) { discard; }
    return vec4<f32>(color.rgb * alpha, alpha);
}
