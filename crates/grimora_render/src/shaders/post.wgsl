// Grimora post-processing: bloom composite, tonemapping and color grading.

struct FullscreenOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_fullscreen(@builtin(vertex_index) idx: u32) -> FullscreenOut {
    let uv = vec2<f32>(f32((idx << 1u) & 2u), f32(idx & 2u));
    var out: FullscreenOut;
    out.clip = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(uv.x, 1.0 - uv.y);
    return out;
}

struct PostParams {
    a: vec4<f32>,   // x exposure, y contrast, z saturation, w vignette
    b: vec4<f32>,   // x grain, y bloom, z time, w bloom mip count
}

@group(0) @binding(0) var hdr: texture_2d<f32>;
@group(0) @binding(1) var hdr_sampler: sampler;
@group(0) @binding(2) var bloom: texture_2d<f32>;
@group(0) @binding(3) var<uniform> params: PostParams;

// ACES filmic curve fit (Narkowicz 2015).
fn aces(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn hash12(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

@fragment
fn fs_tonemap(in: FullscreenOut) -> @location(0) vec4<f32> {
    let exposure = params.a.x;
    let contrast = params.a.y;
    let saturation = params.a.z;
    let vignette = params.a.w;
    let grain = params.b.x;
    let bloom_strength = params.b.y;
    let time = params.b.z;

    var color = textureSampleLevel(hdr, hdr_sampler, in.uv, 0.0).rgb;
    // Each mip adds its own blur level, so average them back to the scene's energy.
    let glow = textureSampleLevel(bloom, hdr_sampler, in.uv, 0.0).rgb / max(params.b.w, 1.0);
    color = mix(color, glow, bloom_strength);

    color = aces(color * exposure);

    // Grade in a perceptual (gamma) space so contrast pivots around mid-grey.
    var p = pow(color, vec3<f32>(1.0 / 2.2));
    let luma = dot(p, vec3<f32>(0.2126, 0.7152, 0.0722));
    p = mix(vec3<f32>(luma), p, saturation);
    p = (p - 0.5) * contrast + 0.5;

    let d = distance(in.uv, vec2<f32>(0.5));
    p *= 1.0 - vignette * smoothstep(0.25, 0.85, d);

    // Film grain doubles as dithering, which hides banding in fog gradients.
    let noise = hash12(in.clip.xy + fract(time) * 1000.0) - 0.5;
    p += noise * (grain + 1.0 / 255.0);

    color = pow(clamp(p, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(2.2));
    return vec4<f32>(color, 1.0);
}
