// Textured quad shader with brightness modifier for ButtonImage
// Based on sprite.wgsl but adds brightness for hover/press visual feedback
// Output: premultiplied alpha (matching SpritePipeline)

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) tex_coords: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
}

struct Uniforms {
    viewport: vec4<f32>,    // width, height, 0, 0
    rect: vec4<f32>,        // x, y, width, height (pixels)
    brightness: f32,        // 1.0 normal, 1.15 hover, 0.9 pressed
    _padding: vec3<f32>,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;

    // in.position is 0..1 unit quad, map to pixel rect then to clip space
    let pixel_x = uniforms.rect.x + in.position.x * uniforms.rect.z;
    let pixel_y = uniforms.rect.y + in.position.y * uniforms.rect.w;

    let clip_x = (pixel_x / uniforms.viewport.x) * 2.0 - 1.0;
    let clip_y = 1.0 - (pixel_y / uniforms.viewport.y) * 2.0;

    out.clip_position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    out.tex_coords = in.tex_coords;

    return out;
}

@group(0) @binding(1)
var t_diffuse: texture_2d<f32>;
@group(0) @binding(2)
var s_diffuse: sampler;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(t_diffuse, s_diffuse, in.tex_coords);

    // Apply brightness modifier and clamp
    let brightened = clamp(color.rgb * uniforms.brightness, vec3<f32>(0.0), vec3<f32>(1.0));

    // Premultiplied alpha output
    let alpha = color.a;
    return vec4<f32>(brightened * alpha, alpha);
}
