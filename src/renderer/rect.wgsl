struct Uniforms {
    screen: vec2<f32>,
    _padding: vec2<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct Instance {
    @location(0) position: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) radius: f32,
    @location(4) shape: u32,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    // Pixel position relative to the rect's top-left corner.
    @location(1) local: vec2<f32>,
    @location(2) @interpolate(flat) size: vec2<f32>,
    @location(3) @interpolate(flat) radius: f32,
    @location(4) @interpolate(flat) shape: u32,
};

const SHAPE_RECT: u32 = 0u;
const SHAPE_TRIANGLE_RIGHT: u32 = 1u;
const SHAPE_TRIANGLE_DOWN: u32 = 2u;

@vertex
fn vs_main(@builtin(vertex_index) index: u32, instance: Instance) -> VertexOutput {
    let corner = vec2<f32>(f32(index & 1u), f32((index >> 1u) & 1u));
    let local = corner * instance.size;
    let pixel = instance.position + local;
    let ndc = vec2<f32>(
        pixel.x / uniforms.screen.x * 2.0 - 1.0,
        1.0 - pixel.y / uniforms.screen.y * 2.0,
    );
    var out: VertexOutput;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.color = instance.color;
    out.local = local;
    out.size = instance.size;
    out.radius = instance.radius;
    out.shape = instance.shape;
    return out;
}

// Signed distance to a box of half size `half` centered on the origin, with corners
// rounded by `radius`: negative inside.
fn sd_rounded_box(p: vec2<f32>, half: vec2<f32>, radius: f32) -> f32 {
    let r = min(radius, min(half.x, half.y));
    let q = abs(p) - half + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// Signed distance to the triangle p0 p1 p2: negative inside.
fn sd_triangle(p: vec2<f32>, p0: vec2<f32>, p1: vec2<f32>, p2: vec2<f32>) -> f32 {
    let e0 = p1 - p0;
    let e1 = p2 - p1;
    let e2 = p0 - p2;
    let v0 = p - p0;
    let v1 = p - p1;
    let v2 = p - p2;
    let pq0 = v0 - e0 * clamp(dot(v0, e0) / dot(e0, e0), 0.0, 1.0);
    let pq1 = v1 - e1 * clamp(dot(v1, e1) / dot(e1, e1), 0.0, 1.0);
    let pq2 = v2 - e2 * clamp(dot(v2, e2) / dot(e2, e2), 0.0, 1.0);
    let s = sign(e0.x * e2.y - e0.y * e2.x);
    let d = min(
        min(
            vec2<f32>(dot(pq0, pq0), s * (v0.x * e0.y - v0.y * e0.x)),
            vec2<f32>(dot(pq1, pq1), s * (v1.x * e1.y - v1.y * e1.x)),
        ),
        vec2<f32>(dot(pq2, pq2), s * (v2.x * e2.y - v2.y * e2.x)),
    );
    return -sqrt(d.x) * sign(d.y);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // A plain rect keeps the rasterizer's coverage: no antialiasing on its edges.
    if in.shape == SHAPE_RECT && in.radius <= 0.0 {
        return in.color;
    }
    let size = in.size;
    var d: f32;
    if in.shape == SHAPE_TRIANGLE_RIGHT {
        d = sd_triangle(in.local, vec2<f32>(0.0, 0.0), vec2<f32>(size.x, size.y * 0.5), vec2<f32>(0.0, size.y));
    } else if in.shape == SHAPE_TRIANGLE_DOWN {
        d = sd_triangle(in.local, vec2<f32>(0.0, 0.0), vec2<f32>(size.x, 0.0), vec2<f32>(size.x * 0.5, size.y));
    } else {
        let half = size * 0.5;
        d = sd_rounded_box(in.local - half, half, in.radius);
    }
    let coverage = clamp(0.5 - d, 0.0, 1.0);
    return vec4<f32>(in.color.rgb, in.color.a * coverage);
}
