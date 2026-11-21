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
    @location(5) bend: f32,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    // Pixel position relative to the rect's top-left corner.
    @location(1) local: vec2<f32>,
    @location(2) @interpolate(flat) size: vec2<f32>,
    @location(3) @interpolate(flat) radius: f32,
    @location(4) @interpolate(flat) shape: u32,
    @location(5) @interpolate(flat) bend: f32,
};

const SHAPE_RECT: u32 = 0u;
const SHAPE_CHEVRON: u32 = 1u;
const SHAPE_HEXAGON: u32 = 2u;
const SHAPE_SCANLINES: u32 = 3u;

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
    out.bend = instance.bend;
    return out;
}

// Signed distance to a box of half size `half` centered on the origin, with corners
// rounded by `radius`: negative inside.
fn sd_rounded_box(p: vec2<f32>, half: vec2<f32>, radius: f32) -> f32 {
    let r = min(radius, min(half.x, half.y));
    let q = abs(p) - half + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// Distance to the segment a b.
fn sd_segment(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
    return length(pa - ba * h);
}

// Signed distance to a chevron stroke of half width `half_stroke` filling a box of size
// `size`: two segments from the box's left and right edges to its horizontal center,
// the middle `bend` times the bend room below the ends (above when negative).
fn sd_chevron(p: vec2<f32>, size: vec2<f32>, half_stroke: f32, bend: f32) -> f32 {
    let room = max(size.y - 2.0 * half_stroke, 0.0);
    let center = size.y * 0.5;
    let ends = center - bend * room * 0.5;
    let middle = center + bend * room * 0.5;
    let left = vec2<f32>(half_stroke, ends);
    let mid = vec2<f32>(size.x * 0.5, middle);
    let right = vec2<f32>(size.x - half_stroke, ends);
    return min(sd_segment(p, left, mid), sd_segment(p, mid, right)) - half_stroke;
}

// Signed distance to a regular hexagon of inradius `inradius` centered on the origin,
// its flat sides vertical (a vertex up and one down): negative inside.
fn sd_hexagon(p: vec2<f32>, inradius: f32) -> f32 {
    let k = vec3<f32>(-0.866025404, 0.5, 0.577350269);
    var q = abs(p.yx);
    q = q - 2.0 * min(dot(k.xy, q), 0.0) * k.xy;
    q = q - vec2<f32>(clamp(q.x, -k.z * inradius, k.z * inradius), inradius);
    return length(q) * sign(q.y);
}

// Signed distance to the outline of the hexagon held by a box of size `size`, a
// stroke of half width `half_stroke` inside it.
fn sd_hexagon_outline(p: vec2<f32>, size: vec2<f32>, half_stroke: f32) -> f32 {
    let inradius = size.x * 0.5;
    let middle = max(inradius - half_stroke, 0.0);
    return abs(sd_hexagon(p - size * 0.5, middle)) - half_stroke;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Scanlines: the instance's color on one row of physical pixels out of 3, nothing
    // on the others.
    if in.shape == SHAPE_SCANLINES {
        if u32(in.position.y) % 3u == 0u {
            return in.color;
        }
        return vec4<f32>(0.0);
    }
    // A plain rect keeps the rasterizer's coverage: no antialiasing on its edges.
    if in.shape == SHAPE_RECT && in.radius <= 0.0 {
        return in.color;
    }
    let size = in.size;
    var d: f32;
    if in.shape == SHAPE_CHEVRON {
        d = sd_chevron(in.local, size, in.radius, in.bend);
    } else if in.shape == SHAPE_HEXAGON {
        d = sd_hexagon_outline(in.local, size, in.radius);
    } else {
        let half = size * 0.5;
        d = sd_rounded_box(in.local - half, half, in.radius);
    }
    let coverage = clamp(0.5 - d, 0.0, 1.0);
    return vec4<f32>(in.color.rgb, in.color.a * coverage);
}
