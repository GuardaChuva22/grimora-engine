// Grimora bloom: progressive downsample, then additive upsample.

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

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;

fn tap(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(src, src_sampler, uv, 0.0).rgb;
}

// 13-tap downsample (Jimenez, "Next Generation Post Processing in Call of Duty").
@fragment
fn fs_downsample(in: FullscreenOut) -> @location(0) vec4<f32> {
    let t = 1.0 / vec2<f32>(textureDimensions(src));
    let uv = in.uv;
    let a = tap(uv + t * vec2<f32>(-2.0, -2.0));
    let b = tap(uv + t * vec2<f32>(0.0, -2.0));
    let c = tap(uv + t * vec2<f32>(2.0, -2.0));
    let d = tap(uv + t * vec2<f32>(-2.0, 0.0));
    let e = tap(uv);
    let f = tap(uv + t * vec2<f32>(2.0, 0.0));
    let g = tap(uv + t * vec2<f32>(-2.0, 2.0));
    let h = tap(uv + t * vec2<f32>(0.0, 2.0));
    let i = tap(uv + t * vec2<f32>(2.0, 2.0));
    let j = tap(uv + t * vec2<f32>(-1.0, -1.0));
    let k = tap(uv + t * vec2<f32>(1.0, -1.0));
    let l = tap(uv + t * vec2<f32>(-1.0, 1.0));
    let m = tap(uv + t * vec2<f32>(1.0, 1.0));
    var color = e * 0.125 + (a + c + g + i) * 0.03125 + (b + d + f + h) * 0.0625 + (j + k + l + m) * 0.125;
    // Clamp very bright pixels (the sun disc) so they don't flicker as fireflies.
    color = min(color, vec3<f32>(64.0));
    return vec4<f32>(color, 1.0);
}

// 3x3 tent upsample, blended additively onto the next larger mip.
@fragment
fn fs_upsample(in: FullscreenOut) -> @location(0) vec4<f32> {
    let t = 1.0 / vec2<f32>(textureDimensions(src));
    let uv = in.uv;
    var color = tap(uv) * 4.0;
    color += (tap(uv + t * vec2<f32>(-1.0, 0.0)) + tap(uv + t * vec2<f32>(1.0, 0.0))
            + tap(uv + t * vec2<f32>(0.0, -1.0)) + tap(uv + t * vec2<f32>(0.0, 1.0))) * 2.0;
    color += tap(uv + t * vec2<f32>(-1.0, -1.0)) + tap(uv + t * vec2<f32>(1.0, -1.0))
           + tap(uv + t * vec2<f32>(-1.0, 1.0)) + tap(uv + t * vec2<f32>(1.0, 1.0));
    return vec4<f32>(color / 16.0, 1.0);
}

