// Grimora scene shader: lit flat-shaded meshes, sky, and the shadow depth pass.
// Everything here outputs linear HDR color; tonemapping happens in post.wgsl.

const MAX_POINT_LIGHTS: u32 = 32u;

struct PointLight {
    position_radius: vec4<f32>,   // xyz position, w radius
    color_intensity: vec4<f32>,   // rgb linear color, w intensity
    params: vec4<f32>,            // x flicker
}

struct Globals {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    light_view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,        // xyz, w = time in seconds
    sun_dir: vec4<f32>,           // xyz towards the sun, w = shadow texel size in uv
    sun_color: vec4<f32>,         // rgb * intensity, w = shadow strength
    ambient_sky: vec4<f32>,
    ambient_ground: vec4<f32>,
    fog_color: vec4<f32>,         // w = density
    fog_params: vec4<f32>,        // x height falloff, y base height, z sun scatter, w shadow texel in world units
    sky_zenith: vec4<f32>,
    sky_horizon: vec4<f32>,
    sky_ground: vec4<f32>,
    sun_disc: vec4<f32>,          // x cos(sun radius), y glow strength
    counts: vec4<u32>,            // x point light count, y shadows enabled
    point_lights: array<PointLight, MAX_POINT_LIGHTS>,
}

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var shadow_map: texture_depth_2d;
@group(1) @binding(1) var shadow_sampler: sampler_comparison;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
}

struct InstanceIn {
    @location(3) model_0: vec4<f32>,
    @location(4) model_1: vec4<f32>,
    @location(5) model_2: vec4<f32>,
    @location(6) model_3: vec4<f32>,
    @location(7) normal_0: vec4<f32>,
    @location(8) normal_1: vec4<f32>,
    @location(9) normal_2: vec4<f32>,
    @location(10) tint: vec4<f32>,  // rgb sRGB tint, a = emissive strength
}

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) albedo: vec3<f32>,
    @location(3) emissive: f32,
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

@vertex
fn vs_main(v: VertexIn, i: InstanceIn) -> VsOut {
    let model = mat4x4<f32>(i.model_0, i.model_1, i.model_2, i.model_3);
    let normal_m = mat3x3<f32>(i.normal_0.xyz, i.normal_1.xyz, i.normal_2.xyz);
    let world = model * vec4<f32>(v.position, 1.0);
    var out: VsOut;
    out.clip = g.view_proj * world;
    out.world_pos = world.xyz;
    out.normal = normalize(normal_m * v.normal);
    out.albedo = srgb_to_linear(v.color * i.tint.rgb);
    out.emissive = i.tint.a;
    return out;
}

@vertex
fn vs_shadow(v: VertexIn, i: InstanceIn) -> @builtin(position) vec4<f32> {
    let model = mat4x4<f32>(i.model_0, i.model_1, i.model_2, i.model_3);
    return g.light_view_proj * model * vec4<f32>(v.position, 1.0);
}

fn shadow_factor(world_pos: vec3<f32>, n: vec3<f32>) -> f32 {
    if g.counts.y == 0u {
        return 1.0;
    }
    // Normal offset pushes the lookup out of the surface to avoid shadow acne.
    let offset_pos = world_pos + n * g.fog_params.w * 1.5;
    let lp = g.light_view_proj * vec4<f32>(offset_pos, 1.0);
    let ndc = lp.xyz / lp.w;
    let uv = ndc.xy * vec2<f32>(0.5, -0.5) + 0.5;
    let texel = g.sun_dir.w;

    var sum = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let o = vec2<f32>(f32(x), f32(y)) * texel;
            sum += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + o, ndc.z);
        }
    }
    var lit = sum / 9.0;

    // Fade out near the edge of the shadowed area instead of cutting off hard.
    let edge = max(abs(ndc.x), abs(ndc.y));
    lit = mix(lit, 1.0, smoothstep(0.85, 1.0, edge));
    let inside = edge <= 1.0 && ndc.z <= 1.0 && ndc.z >= 0.0;
    lit = select(1.0, lit, inside);
    return mix(1.0, lit, g.sun_color.w);
}

fn hash13(p: vec3<f32>) -> f32 {
    var p3 = fract(p * 0.1031);
    p3 += dot(p3, p3.zyx + 31.32);
    return fract((p3.x + p3.y) * p3.z);
}

fn point_lights(world_pos: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    var total = vec3<f32>(0.0);
    let t = g.camera_pos.w;
    for (var i = 0u; i < g.counts.x; i++) {
        let pl = g.point_lights[i];
        let to_light = pl.position_radius.xyz - world_pos;
        let d = length(to_light);
        let radius = pl.position_radius.w;
        if d >= radius {
            continue;
        }
        let l = to_light / max(d, 1e-4);
        let window = clamp(1.0 - pow(d / radius, 4.0), 0.0, 1.0);
        let atten = window * window / (d * d + 1.0);

        // Two out-of-phase sines per light read as an irregular fire flicker.
        let seed = hash13(pl.position_radius.xyz) * 100.0;
        let wobble = 0.5 + 0.5 * sin(t * 11.0 + seed) * sin(t * 6.7 + seed * 1.7);
        let flicker = 1.0 - pl.params.x * 0.35 * wobble;

        let wrap = max(dot(n, l) * 0.8 + 0.2, 0.0);
        total += pl.color_intensity.rgb * pl.color_intensity.w * atten * wrap * flicker;
    }
    return total;
}

fn apply_fog(color: vec3<f32>, world_pos: vec3<f32>) -> vec3<f32> {
    let cam = g.camera_pos.xyz;
    let v = world_pos - cam;
    let dist = length(v);
    let dir = v / max(dist, 1e-4);
    let density = g.fog_color.w;
    let falloff = g.fog_params.x;
    let h0 = cam.y - g.fog_params.y;

    // Analytic integral of density * exp(-falloff * height) along the view ray.
    var integral = exp(-falloff * h0);
    let k = falloff * v.y;
    if abs(k) > 1e-3 {
        integral = integral * (1.0 - exp(-k)) / k;
    }
    let amount = clamp(1.0 - exp(-density * dist * integral), 0.0, 1.0);

    let scatter = pow(max(dot(dir, g.sun_dir.xyz), 0.0), 8.0) * g.fog_params.z;
    let fog = g.fog_color.rgb + g.sun_color.rgb * scatter * 0.15;
    return mix(color, fog, amount);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let n = normalize(in.normal);
    let l = g.sun_dir.xyz;
    let ndl = max(dot(n, l), 0.0);
    let shadow = shadow_factor(in.world_pos, n);

    let hemi = mix(g.ambient_ground.rgb, g.ambient_sky.rgb, n.y * 0.5 + 0.5);
    let light = g.sun_color.rgb * ndl * shadow + hemi + point_lights(in.world_pos, n);

    var color = in.albedo * light + in.albedo * in.emissive;
    color = apply_fog(color, in.world_pos);
    return vec4<f32>(color, 1.0);
}

// ---------------------------------------------------------------- sky

struct SkyOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex
fn vs_sky(@builtin(vertex_index) idx: u32) -> SkyOut {
    // One triangle that covers the whole screen.
    let uv = vec2<f32>(f32((idx << 1u) & 2u), f32(idx & 2u));
    let ndc = uv * 2.0 - 1.0;
    var out: SkyOut;
    out.clip = vec4<f32>(ndc, 1.0, 1.0);
    out.ndc = ndc;
    return out;
}

@fragment
fn fs_sky(in: SkyOut) -> @location(0) vec4<f32> {
    let far = g.inv_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    let dir = normalize(far.xyz / far.w - g.camera_pos.xyz);

    var color: vec3<f32>;
    if dir.y >= 0.0 {
        color = mix(g.sky_horizon.rgb, g.sky_zenith.rgb, pow(dir.y, 0.5));
    } else {
        color = mix(g.sky_horizon.rgb, g.sky_ground.rgb, pow(-dir.y, 0.4));
    }

    let cos_sun = dot(dir, g.sun_dir.xyz);
    let disc = smoothstep(g.sun_disc.x, g.sun_disc.x + 0.0004, cos_sun);
    let glow = pow(max(cos_sun, 0.0), 64.0) * g.sun_disc.y + pow(max(cos_sun, 0.0), 6.0) * g.sun_disc.y * 0.15;
    color += g.sun_color.rgb * (disc * 8.0 + glow);

    // Fog the sky as if it were very far away, so the horizon melts into the fog.
    color = apply_fog(color, g.camera_pos.xyz + dir * 600.0);
    return vec4<f32>(color, 1.0);
}
