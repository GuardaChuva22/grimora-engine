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
    c: vec4<f32>,   // rgb shadow tint, w vivid
    d: vec4<f32>,   // x sharpen, y how far under water the camera's picture is, z near, w far
    e: vec4<f32>,   // xy where the light is on the screen, z how strong its shafts are, w paint
    f: vec4<f32>,   // x where the paint starts, y where it is full, in meters
}

@group(0) @binding(0) var hdr: texture_2d<f32>;
@group(0) @binding(1) var hdr_sampler: sampler;
@group(0) @binding(2) var bloom: texture_2d<f32>;
@group(0) @binding(3) var<uniform> params: PostParams;
// Reversed: 1 at the near plane, 0 at the far one and on the sky.
@group(0) @binding(4) var depth_tex: texture_depth_multisampled_2d;
// The shafts of light, at a quarter of the size (`fs_rays`).
@group(0) @binding(5) var rays_tex: texture_2d<f32>;

// How far ahead of the camera what a pixel shows is, in meters; the far plane on the sky.
fn view_distance(pixel: vec2<i32>) -> f32 {
    let d = textureLoad(depth_tex, pixel, 0);
    let near = params.d.z;
    let far = params.d.w;
    return near * far / (d * (far - near) + near);
}

// The strokes far things are laid on in: of the four squares around a pixel, it takes the
// color of the one that varies least, so a patch of land becomes a few flat dabs whose edges
// keep to the shapes in it (a Kuwahara filter).
fn painted(pixel: vec2<i32>, radius: i32) -> vec3<f32> {
    let size = vec2<i32>(textureDimensions(hdr)) - 1;
    var mean = array<vec3<f32>, 4>(vec3<f32>(0.0), vec3<f32>(0.0), vec3<f32>(0.0), vec3<f32>(0.0));
    var spread = array<f32, 4>(0.0, 0.0, 0.0, 0.0);
    var square = array<f32, 4>(0.0, 0.0, 0.0, 0.0);
    for (var y = -radius; y <= radius; y++) {
        for (var x = -radius; x <= radius; x++) {
            // In a perceptual space, or the brightest dab would always win.
            let c = sqrt(textureLoad(hdr, clamp(pixel + vec2<i32>(x, y), vec2<i32>(0), size), 0).rgb);
            let l = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
            if x <= 0 && y <= 0 { mean[0] += c; square[0] += l; spread[0] += l * l; }
            if x >= 0 && y <= 0 { mean[1] += c; square[1] += l; spread[1] += l * l; }
            if x <= 0 && y >= 0 { mean[2] += c; square[2] += l; spread[2] += l * l; }
            if x >= 0 && y >= 0 { mean[3] += c; square[3] += l; spread[3] += l * l; }
        }
    }
    let n = f32((radius + 1) * (radius + 1));
    var best = mean[0] / n;
    var least = 1e9;
    for (var i = 0; i < 4; i++) {
        let m = square[i] / n;
        let v = spread[i] / n - m * m;
        if v < least {
            least = v;
            best = mean[i] / n;
        }
    }
    return best * best;
}

// The shafts of light: from each pixel towards the light on the screen, how much bright sky
// shows along the way, nearest the light counting most. What stands in front of the sky
// leaves a shadow in them.
const SHAFT_STEPS: i32 = 32;
const SHAFT_REACH: f32 = 0.9;
const SHAFT_DECAY: f32 = 0.965;
const SHAFT_TIGHT: f32 = 6.0;
// Only what is brighter than this gives off shafts: the sun and the glow right around it.
const SHAFT_FLOOR: f32 = 0.9;
const SHAFT_BRIGHTEST: f32 = 6.0;
fn light_shafts(uv: vec2<f32>, jitter: f32) -> vec3<f32> {
    let size = vec2<f32>(textureDimensions(hdr));
    let aspect = vec2<f32>(size.x / size.y, 1.0);
    let light = params.e.xy;
    let step = (light - uv) * SHAFT_REACH / f32(SHAFT_STEPS);
    var at = uv + step * jitter;
    var weight = 1.0;
    var sum = vec3<f32>(0.0);
    for (var i = 0; i < SHAFT_STEPS; i++) {
        if all(at >= vec2<f32>(0.0)) && all(at <= vec2<f32>(1.0)) {
            let pixel = vec2<i32>(at * size);
            let sky = select(0.0, 1.0, textureLoad(depth_tex, pixel, 0) <= 0.0);
            let c = clamp(textureLoad(hdr, pixel, 0).rgb - vec3<f32>(SHAFT_FLOOR), vec3<f32>(0.0), vec3<f32>(SHAFT_BRIGHTEST));
            let near_light = exp(-length((at - light) * aspect) * SHAFT_TIGHT);
            sum += c * sky * near_light * weight;
        }
        weight *= SHAFT_DECAY;
        at += step;
    }
    return sum * (2.0 / f32(SHAFT_STEPS));
}

// ACES filmic curve fit (Narkowicz 2015).
fn aces(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

// The same curve on the brightest channel only, scaling the others along: a
// color keeps its hue however bright it gets, where plain ACES bleaches it.
fn aces_keeping_hue(x: vec3<f32>) -> vec3<f32> {
    let peak = max(max(x.r, x.g), x.b);
    if peak <= 0.0 {
        return vec3<f32>(0.0);
    }
    return x * aces(vec3<f32>(peak)).r / peak;
}

fn hash12(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

// Unsharp mask in a roughly perceptual space, or bright lights would ring.
fn sharpened(color: vec3<f32>, pixel: vec2<i32>, amount: f32) -> vec3<f32> {
    let size = vec2<i32>(textureDimensions(hdr)) - 1;
    let at = pixel;
    let around = sqrt(textureLoad(hdr, clamp(at + vec2<i32>(1, 0), vec2<i32>(0), size), 0).rgb)
        + sqrt(textureLoad(hdr, clamp(at - vec2<i32>(1, 0), vec2<i32>(0), size), 0).rgb)
        + sqrt(textureLoad(hdr, clamp(at + vec2<i32>(0, 1), vec2<i32>(0), size), 0).rgb)
        + sqrt(textureLoad(hdr, clamp(at - vec2<i32>(0, 1), vec2<i32>(0), size), 0).rgb);
    let here = sqrt(color);
    let crisp = max(here + (here - around * 0.25) * amount, vec3<f32>(0.0));
    return crisp * crisp;
}

const UNDERWATER_WOBBLE: f32 = 0.0022;
const UNDERWATER_VIGNETTE: f32 = 0.4;

@fragment
fn fs_tonemap(in: FullscreenOut) -> @location(0) vec4<f32> {
    let exposure = params.a.x;
    let contrast = params.a.y;
    let saturation = params.a.z;
    let vignette = params.a.w;
    let grain = params.b.x;
    let bloom_strength = params.b.y;
    let time = params.b.z;
    let shadow_tint = params.c.rgb;
    let vivid = params.c.w;
    let sharpen = params.d.x;

    // Under water the whole picture wavers, slowly, as if seen through a lens that breathes.
    let submerged = params.d.y;
    let wobble = vec2<f32>(sin(in.uv.y * 19.0 + time * 1.7) + 0.5 * sin(in.uv.y * 47.0 - time * 2.3),
                           cos(in.uv.x * 15.0 + time * 1.3) + 0.5 * cos(in.uv.x * 41.0 + time * 1.9));
    let uv = in.uv + wobble * UNDERWATER_WOBBLE * submerged;
    var color = textureSampleLevel(hdr, hdr_sampler, uv, 0.0).rgb;
    let pixel = vec2<i32>(uv * vec2<f32>(textureDimensions(hdr)));
    // Far things in strokes; the sky (depth 0) is left alone, or its stars would go.
    let paint = params.e.w;
    var brushed = 0.0;
    if paint > 0.0 {
        let far = textureLoad(depth_tex, pixel, 0) > 0.0;
        brushed = select(0.0, smoothstep(params.f.x, params.f.y, view_distance(pixel)) * paint, far);
        if brushed > 0.01 {
            let radius = clamp(i32(round(f32(textureDimensions(hdr).y) / 360.0)), 2, 4);
            color = mix(color, painted(pixel, radius), brushed);
        }
    }
    if sharpen > 0.0 {
        color = mix(sharpened(color, pixel, sharpen), color, brushed);
    }
    if params.e.z > 0.0 {
        color += textureSampleLevel(rays_tex, hdr_sampler, uv, 0.0).rgb;
    }
    // Each mip adds its own blur level, so average them back to the scene's energy.
    let glow = textureSampleLevel(bloom, hdr_sampler, uv, 0.0).rgb / max(params.b.w, 1.0);
    color = mix(color, glow, bloom_strength);

    color = mix(aces(color * exposure), aces_keeping_hue(color * exposure), vivid);

    // Grade in a perceptual (gamma) space so contrast pivots around mid-grey.
    var p = pow(color, vec3<f32>(1.0 / 2.2));
    let luma = dot(p, vec3<f32>(0.2126, 0.7152, 0.0722));
    p = mix(vec3<f32>(luma), p, saturation);
    p = (p - 0.5) * contrast + 0.5;
    let dark = 1.0 - clamp(luma * 2.5, 0.0, 1.0);
    p += shadow_tint * dark * dark;

    let d = distance(in.uv, vec2<f32>(0.5));
    p *= 1.0 - (vignette + UNDERWATER_VIGNETTE * submerged) * smoothstep(0.25, 0.85, d);

    // Film grain doubles as dithering, which hides banding in fog gradients.
    let noise = hash12(in.clip.xy + fract(time) * 1000.0) - 0.5;
    p += noise * (grain + 1.0 / 255.0);

    color = pow(clamp(p, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(2.2));
    return vec4<f32>(color, 1.0);
}

// The shafts of light, worked out small: soft as they are, a quarter of the pixels is enough.
// The start of each pixel's steps is staggered in a fixed pattern, which the blowing up smooths.
@fragment
fn fs_rays(in: FullscreenOut) -> @location(0) vec4<f32> {
    let jitter = fract(52.9829189 * fract(dot(in.clip.xy, vec2<f32>(0.06711056, 0.00583715))));
    return vec4<f32>(light_shafts(in.uv, jitter) * params.e.z, 1.0);
}

// A portrait for the UI: its colors with the alpha kept, in hard pixels, and no grain or vignette.
@fragment
fn fs_portrait(in: FullscreenOut) -> @location(0) vec4<f32> {
    let texel = vec2<i32>(in.clip.xy);
    let hdr_color = textureLoad(hdr, texel, 0);
    if hdr_color.a < 0.5 {
        return vec4<f32>(0.0);
    }
    let lit = hdr_color.rgb / hdr_color.a * params.a.x;
    let color = mix(aces(lit), aces_keeping_hue(lit), params.c.w);
    return vec4<f32>(color, 1.0);
}
