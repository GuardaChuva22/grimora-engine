// HUD: textured quads over the tonemapped image, premultiplied alpha.

struct UiGlobals {
    // Target size in physical pixels, then atlas size in texels.
    sizes: vec4<f32>,
    // Size of the atlas of pictures in texels, in xy.
    images_size: vec4<f32>,
};

// Levels of alpha a dithered area is made of.
const DITHER_STEPS: f32 = 5.0;
// Flat bands a gradient is made of.
const GRADIENT_BANDS: f32 = 24.0;

@group(0) @binding(0) var<uniform> globals: UiGlobals;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;
@group(0) @binding(3) var images: texture_2d<f32>;

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
    let picture = textureSample(images, atlas_sampler, texel / globals.images_size.xy);

    // An outline alpha of -3 marks a gradient to the outline's color, across.
    if in.outline.a < -2.5 {
        let band = floor(clamp(in.texel.x, 0.0, 0.999) * GRADIENT_BANDS) / (GRADIENT_BANDS - 1.0);
        // The target is linear: this curve spaces the bands evenly to the eye.
        let eased = pow(band, 2.2);
        return vec4<f32>(mix(in.color.rgb, in.outline.rgb, eased) * in.color.a, in.color.a);
    }

    // An outline alpha of -2 marks a dithered area: `texel` counts the art's pixels.
    if in.outline.a < -1.5 {
        var bayer = array<f32, 16>(
            0.0, 8.0, 2.0, 10.0,
            12.0, 4.0, 14.0, 6.0,
            3.0, 11.0, 1.0, 9.0,
            15.0, 7.0, 13.0, 5.0,
        );
        let cell = vec2<u32>(floor(in.texel));
        let threshold = (bayer[(cell.y % 4u) * 4u + cell.x % 4u] + 0.5) / 16.0;
        let along = select(in.texel.x, in.texel.y, in.outline.z > 0.5);
        let wanted = mix(in.color.a, in.outline.x, clamp(along / max(in.outline.y, 1.0), 0.0, 1.0));
        // Steps of alpha, each mixed with the next by the pattern.
        let alpha = clamp(floor(wanted * DITHER_STEPS + threshold) / DITHER_STEPS, 0.0, 1.0);
        return vec4<f32>(in.color.rgb * alpha, alpha);
    }

    // A negative outline marks a quad that shows a picture, tinted by its color.
    if in.outline.a < 0.0 {
        let alpha = picture.a * in.color.a;
        return vec4<f32>(picture.rgb * in.color.rgb * alpha, alpha);
    }

    let fill = vec4<f32>(in.color.rgb * in.color.a, in.color.a);
    let line = vec4<f32>(in.outline.rgb * in.outline.a, in.outline.a) * coverage.g;
    return mix(line, fill, coverage.r);
}
