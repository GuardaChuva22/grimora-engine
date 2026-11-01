// HUD: textured quads over the tonemapped image, premultiplied alpha.

struct UiGlobals {
    // Target size in physical pixels, then atlas size in texels.
    sizes: vec4<f32>,
};

@group(0) @binding(0) var<uniform> globals: UiGlobals;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

// Mirrors `UiQuad` in ui.rs.
struct Quad {
    @location(0) rect: vec4<f32>,
    @location(1) uv: vec4<f32>,
    @location(2) color: vec4<f32>,
    @location(3) outline: vec4<f32>,
};

struct UiOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) texel: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) outline: vec4<f32>,
};

@vertex
fn vs_ui(@builtin(vertex_index) index: u32, quad: Quad) -> UiOut {
    let corner = vec2<f32>(f32(index & 1u), f32(index >> 1u));
    let p = quad.rect.xy + corner * quad.rect.zw;
    let screen = globals.sizes.xy;
    var out: UiOut;
    out.clip = vec4<f32>(p.x / screen.x * 2.0 - 1.0, 1.0 - p.y / screen.y * 2.0, 0.0, 1.0);
    out.texel = quad.uv.xy + corner * quad.uv.zw;
    out.color = quad.color;
    out.outline = quad.outline;
    return out;
}

@fragment
fn fs_ui(in: UiOut) -> @location(0) vec4<f32> {
    // Sharp pixels at any scale: sample the texel centre, blending only
    // across the one screen pixel that straddles a texel edge.
    let width = max(fwidth(in.texel), vec2<f32>(1e-4));
    let seam = floor(in.texel + 0.5);
    let texel = seam + clamp((in.texel - seam) / width, vec2<f32>(-0.5), vec2<f32>(0.5));
    let coverage = textureSample(atlas, atlas_sampler, texel / globals.sizes.zw).rg;

    let fill = vec4<f32>(in.color.rgb * in.color.a, in.color.a);
    let line = vec4<f32>(in.outline.rgb * in.outline.a, in.outline.a) * coverage.g;
    return mix(line, fill, coverage.r);
}
