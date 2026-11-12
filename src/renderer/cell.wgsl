struct Uniforms {
    screen: vec2<f32>,
    _padding: vec2<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var mask_atlas: texture_2d<f32>;
@group(0) @binding(2) var color_atlas: texture_2d<f32>;

struct Instance {
    @location(0) position: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) is_color: u32,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    // Atlas texel coordinates, in pixels.
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) is_color: u32,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32, instance: Instance) -> VertexOutput {
    let corner = vec2<f32>(f32(index & 1u), f32((index >> 1u) & 1u));
    let pixel = instance.position + corner * instance.size;
    let ndc = vec2<f32>(
        pixel.x / uniforms.screen.x * 2.0 - 1.0,
        1.0 - pixel.y / uniforms.screen.y * 2.0,
    );
    var out: VertexOutput;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = instance.uv + corner * instance.size;
    out.color = instance.color;
    out.is_color = instance.is_color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Quads are aligned on whole pixels: each fragment reads exactly one texel.
    let texel = vec2<i32>(floor(in.uv));
    if in.is_color != 0u {
        return textureLoad(color_atlas, texel, 0);
    }
    let coverage = textureLoad(mask_atlas, texel, 0).r;
    return vec4<f32>(in.color.rgb, in.color.a * coverage);
}
