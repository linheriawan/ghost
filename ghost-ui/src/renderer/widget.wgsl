// SDF Rounded Rectangle shader for widget backgrounds
// Renders anti-aliased rounded rectangles with per-pixel SDF evaluation
// Output: premultiplied alpha (matching SpritePipeline)

struct Uniforms {
    viewport: vec4<f32>, // width, height, 0, 0
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) rect_min: vec2<f32>,
    @location(3) rect_max: vec2<f32>,
    @location(4) corner_radius: f32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) pixel_pos: vec2<f32>,
    @location(2) rect_min: vec2<f32>,
    @location(3) rect_max: vec2<f32>,
    @location(4) corner_radius: f32,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;

    // Convert pixel coordinates to clip space (-1 to 1)
    let clip_x = (in.position.x / uniforms.viewport.x) * 2.0 - 1.0;
    let clip_y = 1.0 - (in.position.y / uniforms.viewport.y) * 2.0;

    out.clip_position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    out.color = in.color;
    out.pixel_pos = in.position;
    out.rect_min = in.rect_min;
    out.rect_max = in.rect_max;
    out.corner_radius = in.corner_radius;

    return out;
}

// SDF for a rounded rectangle
// p: point relative to rect center
// b: half-size of the rectangle
// r: corner radius
fn sdf_rounded_rect(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2<f32>(r, r);
    return length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Calculate rect center and half-size
    let center = (in.rect_min + in.rect_max) * 0.5;
    let half_size = (in.rect_max - in.rect_min) * 0.5;

    // Clamp corner radius to half of the smallest dimension
    let max_radius = min(half_size.x, half_size.y);
    let radius = min(in.corner_radius, max_radius);

    // Evaluate SDF
    let dist = sdf_rounded_rect(in.pixel_pos - center, half_size, radius);

    // Anti-aliased edge (1px smoothstep)
    let alpha = 1.0 - smoothstep(-0.5, 0.5, dist);

    // Apply color alpha and SDF alpha
    let final_alpha = in.color.a * alpha;

    // Premultiplied alpha output
    return vec4<f32>(in.color.rgb * final_alpha, final_alpha);
}
