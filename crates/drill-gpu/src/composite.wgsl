struct VertexOut { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> };
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;

@vertex fn vs_main(@builtin(vertex_index) vertex: u32) -> VertexOut {
    let p = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    var out: VertexOut;
    out.position = vec4(p[vertex], 0.0, 1.0);
    out.uv = vec2((p[vertex].x + 1.0) * 0.5, (1.0 - p[vertex].y) * 0.5);
    return out;
}
@fragment fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return textureSample(source, source_sampler, in.uv);
}
