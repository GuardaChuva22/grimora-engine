// Grimora scene shader: lit flat-shaded meshes, sky, and the shadow depth pass.
// Everything here outputs linear HDR color; tonemapping happens in post.wgsl.

const MAX_POINT_LIGHTS: u32 = 32u;
const MAX_CASCADES: u32 = 4u;

struct PointLight {
    position_radius: vec4<f32>,   // xyz position, w radius
    color_intensity: vec4<f32>,   // rgb linear color, w intensity
    params: vec4<f32>,            // x flicker
}

struct Globals {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    cascade_view_proj: array<mat4x4<f32>, MAX_CASCADES>,
    cascade_splits: vec4<f32>,    // view depth where each shadow cascade ends
    cascade_texels: vec4<f32>,    // world size of a shadow texel in each cascade
    camera_pos: vec4<f32>,        // xyz, w = time in seconds
    camera_forward: vec4<f32>,
    sun_dir: vec4<f32>,           // xyz towards the sun, w = shadow texel size in uv
    sun_color: vec4<f32>,         // rgb * intensity, w = shadow strength
    ambient_sky: vec4<f32>,
    ambient_ground: vec4<f32>,
    fog_color: vec4<f32>,         // w = density
    fog_params: vec4<f32>,        // x height falloff, y base height, z sun scatter
    sky_zenith: vec4<f32>,
    sky_horizon: vec4<f32>,
    sky_ground: vec4<f32>,
    sun_disc: vec4<f32>,          // x cos(sun radius), y glow strength
    sky_sun: vec4<f32>,           // xyz where the sun is, w half width of its picture (tangent), 0 = none
    sky_moon: vec4<f32>,          // the same for the moon
    sky_glow: vec4<f32>,          // xyz where the source of the light really is
    sky_params: vec4<f32>,        // x stars, y sun picture brightness, z moon picture brightness
    wind: vec4<f32>,              // xy direction, z strength, w speed
    counts: vec4<u32>,            // x point light count, y shadows enabled, z shadow cascades
    point_lights: array<PointLight, MAX_POINT_LIGHTS>,
}

struct MaterialParams {
    base_color: vec4<f32>,        // linear
    params: vec4<f32>,            // x metallic, y roughness, z normal scale, w occlusion strength
    emissive: vec4<f32>,          // rgb emitted light (linear), w alpha cutoff
    flags: vec4<u32>,             // x has normal map, y alpha mode (0 opaque, 1 mask, 2 blend), z alpha output, w foliage
}

@group(0) @binding(0) var<uniform> g: Globals;
// Skinning matrices of every animated instance this frame (see InstanceIn.normal_0.w).
@group(0) @binding(1) var<storage, read> joint_matrices: array<mat4x4<f32>>;
@group(1) @binding(0) var shadow_map: texture_depth_2d_array;
@group(1) @binding(1) var shadow_sampler: sampler_comparison;
// Shadow passes only: the cascade being drawn (bound with a dynamic offset).
@group(1) @binding(2) var<uniform> cascade_view_proj: mat4x4<f32>;
@group(2) @binding(0) var<uniform> material: MaterialParams;
@group(2) @binding(1) var base_color_tex: texture_2d<f32>;
@group(2) @binding(2) var normal_tex: texture_2d<f32>;
@group(2) @binding(3) var metallic_roughness_tex: texture_2d<f32>;
@group(2) @binding(4) var occlusion_tex: texture_2d<f32>;
@group(2) @binding(5) var material_sampler: sampler;
@group(2) @binding(6) var emissive_tex: texture_2d<f32>;
// Sky pass only, in the group the materials use elsewhere.
@group(2) @binding(7) var sun_picture: texture_2d<f32>;
@group(2) @binding(8) var moon_picture: texture_2d<f32>;
@group(2) @binding(9) var sky_sampler: sampler;

const PI: f32 = 3.14159265;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
    @location(11) uv: vec2<f32>,
    @location(12) tangent: vec4<f32>,
    @location(13) joints: vec4<u32>,
    @location(14) weights: vec4<f32>,
}

struct InstanceIn {
    @location(3) model_0: vec4<f32>,
    @location(4) model_1: vec4<f32>,
    @location(5) model_2: vec4<f32>,
    @location(6) model_3: vec4<f32>,
    @location(7) normal_0: vec4<f32>, // w = first joint matrix, or -1 when not skinned
    @location(8) normal_1: vec4<f32>, // w = how much the wind moves it (see MeshData::wind)
    @location(9) normal_2: vec4<f32>,
    @location(10) tint: vec4<f32>,  // rgb sRGB tint, a = emissive strength
}

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) albedo: vec3<f32>,
    @location(3) emissive: f32,
    @location(4) uv: vec2<f32>,
    @location(5) tangent: vec4<f32>,
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

// Model-space skinning matrix of a vertex: its joints' matrices blended by
// weight. Identity for instances without joints and for unweighted vertices.
fn skin_matrix(v: VertexIn, i: InstanceIn) -> mat4x4<f32> {
    let total = dot(v.weights, vec4<f32>(1.0));
    if i.normal_0.w < 0.0 || total <= 0.0 {
        return mat4x4<f32>(vec4<f32>(1.0, 0.0, 0.0, 0.0), vec4<f32>(0.0, 1.0, 0.0, 0.0),
                           vec4<f32>(0.0, 0.0, 1.0, 0.0), vec4<f32>(0.0, 0.0, 0.0, 1.0));
    }
    let first = u32(i.normal_0.w);
    let w = v.weights / total;
    return joint_matrices[first + v.joints.x] * w.x + joint_matrices[first + v.joints.y] * w.y
        + joint_matrices[first + v.joints.z] * w.z + joint_matrices[first + v.joints.w] * w.w;
}

fn instance_model(i: InstanceIn) -> mat4x4<f32> {
    return mat4x4<f32>(i.model_0, i.model_1, i.model_2, i.model_3);
}

// Moves a vertex downwind, the farther the higher above its model's origin.
// Each plant has its own phase along the wind, so gusts roll across a field.
fn sway(world: vec4<f32>, height: f32, i: InstanceIn) -> vec4<f32> {
    let amount = i.normal_1.w;
    if amount <= 0.0 {
        return world;
    }
    let t = g.camera_pos.w * g.wind.w;
    let phase = dot(i.model_3.xz, g.wind.xy) * 0.4 + i.model_3.x * 0.17;
    let gust = 0.5 + 0.5 * sin(t + phase) + 0.25 * sin(t * 2.7 + phase * 1.9 + height * 3.0);
    let push = height * height * amount * g.wind.z * gust;
    return world + vec4<f32>(g.wind.x * push, 0.0, g.wind.y * push, 0.0);
}

@vertex
fn vs_main(v: VertexIn, i: InstanceIn) -> VsOut {
    let skin = skin_matrix(v, i);
    // Joints rotate (and at most uniformly scale), so their 3x3 also takes normals.
    let skin_n = mat3x3<f32>(skin[0].xyz, skin[1].xyz, skin[2].xyz);
    let normal_m = mat3x3<f32>(i.normal_0.xyz, i.normal_1.xyz, i.normal_2.xyz) * skin_n;
    let world = sway(instance_model(i) * skin * vec4<f32>(v.position, 1.0), v.position.y, i);
    var out: VsOut;
    out.clip = g.view_proj * world;
    out.world_pos = world.xyz;
    out.normal = normalize(normal_m * v.normal);
    out.albedo = srgb_to_linear(v.color * i.tint.rgb);
    out.emissive = i.tint.a;
    out.uv = v.uv;
    // Not normalized here: untextured meshes have a zero tangent.
    out.tangent = vec4<f32>(normal_m * v.tangent.xyz, v.tangent.w);
    return out;
}

@vertex
fn vs_shadow(v: VertexIn, i: InstanceIn) -> @builtin(position) vec4<f32> {
    return cascade_view_proj * sway(instance_model(i) * skin_matrix(v, i) * vec4<f32>(v.position, 1.0), v.position.y, i);
}

struct ShadowMaskedOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_shadow_masked(v: VertexIn, i: InstanceIn) -> ShadowMaskedOut {
    var out: ShadowMaskedOut;
    out.clip = cascade_view_proj * sway(instance_model(i) * skin_matrix(v, i) * vec4<f32>(v.position, 1.0), v.position.y, i);
    out.uv = v.uv;
    return out;
}

// Alpha-tested shadow casters. Blended surfaces cast shadow where mostly opaque.
@fragment
fn fs_shadow_masked(in: ShadowMaskedOut) {
    let alpha = textureSample(base_color_tex, material_sampler, in.uv).a * material.base_color.a;
    let cutoff = select(0.5, material.emissive.w, material.flags.y == 1u);
    if alpha < cutoff {
        discard;
    }
}

// Final coverage for the material's alpha mode, discarding cut-out texels.
// With alpha-to-coverage, mask edges get a one-pixel ramp (`width` is the
// screen-space derivative of alpha). Blended surfaces just output their alpha.
fn coverage(alpha: f32, width: f32) -> f32 {
    let mode = material.flags.y;
    let a2c = material.flags.z == 1u;
    if mode == 0u {
        return 1.0;
    }
    if mode == 1u {
        let cutoff = material.emissive.w;
        if !a2c {
            if alpha < cutoff {
                discard;
            }
            return 1.0;
        }
        let ramp = clamp((alpha - cutoff) / max(width, 1e-4) + 0.5, 0.0, 1.0);
        if ramp <= 0.0 {
            discard;
        }
        return ramp;
    }
    if alpha < 0.004 {
        discard;
    }
    return alpha;
}

// Lit fraction (0..1) from cascade `i`, 1 outside its map.
fn sample_cascade(i: u32, world_pos: vec3<f32>, n: vec3<f32>) -> f32 {
    // Normal offset pushes the lookup out of the surface to avoid shadow acne.
    let offset_pos = world_pos + n * g.cascade_texels[i] * 1.5;
    let lp = g.cascade_view_proj[i] * vec4<f32>(offset_pos, 1.0);
    let ndc = lp.xyz / lp.w;
    let uv = ndc.xy * vec2<f32>(0.5, -0.5) + 0.5;
    let texel = g.sun_dir.w;

    var sum = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let o = vec2<f32>(f32(x), f32(y)) * texel;
            sum += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + o, i, ndc.z);
        }
    }
    let inside = max(abs(ndc.x), abs(ndc.y)) <= 1.0 && ndc.z <= 1.0 && ndc.z >= 0.0;
    return select(1.0, sum / 9.0, inside);
}

fn shadow_factor(world_pos: vec3<f32>, n: vec3<f32>) -> f32 {
    if g.counts.y == 0u {
        return 1.0;
    }
    // The nearest cascade that reaches this far: each covers the view from the
    // camera out to its split.
    let count = g.counts.z;
    let depth = dot(world_pos - g.camera_pos.xyz, g.camera_forward.xyz);
    var i = 0u;
    while i + 1u < count && depth > g.cascade_splits[i] {
        i++;
    }
    var lit = sample_cascade(i, world_pos, n);

    // Cross-fade into the next cascade over the end of this one so the seam
    // doesn't show; the last one fades out to unshadowed.
    let start = select(0.0, g.cascade_splits[max(i, 1u) - 1u], i > 0u);
    let end = g.cascade_splits[i];
    let fade = smoothstep(mix(start, end, 0.85), end, depth);
    if fade > 0.0 {
        var next = 1.0;
        if i + 1u < count {
            next = sample_cascade(i + 1u, world_pos, n);
        }
        lit = mix(lit, next, fade);
    }
    return mix(1.0, lit, g.sun_color.w);
}

fn hash13(p: vec3<f32>) -> f32 {
    var p3 = fract(p * 0.1031);
    p3 += dot(p3, p3.zyx + 31.32);
    return fract((p3.x + p3.y) * p3.z);
}

// ---------------------------------------------------------------- shading

struct Surface {
    albedo: vec3<f32>,
    n: vec3<f32>,
    v: vec3<f32>,
    metallic: f32,
    roughness: f32,
    f0: vec3<f32>,
}

fn d_ggx(n_dot_h: f32, a: f32) -> f32 {
    let a2 = a * a;
    let d = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d);
}

// Smith-Schlick visibility, already divided by 4 n.l n.v.
fn v_smith(n_dot_v: f32, n_dot_l: f32, a: f32) -> f32 {
    let k = a * 0.5;
    let gv = n_dot_v / (n_dot_v * (1.0 - k) + k);
    let gl = n_dot_l / (n_dot_l * (1.0 - k) + k);
    return gv * gl / max(4.0 * n_dot_v * n_dot_l, 1e-4);
}

fn fresnel(f0: vec3<f32>, cos_theta: f32) -> vec3<f32> {
    return f0 + (1.0 - f0) * pow(1.0 - cos_theta, 5.0);
}

// Light leaving the surface towards the eye for one light of `radiance`.
// Diffuse keeps the engine's original Lambert scale (no 1/pi), so specular is
// scaled by pi to match. `wrap` softens the terminator for stylized fill lights.
fn shade(s: Surface, l: vec3<f32>, radiance: vec3<f32>, wrap: f32) -> vec3<f32> {
    let n_dot_l_raw = dot(s.n, l);
    let n_dot_l = max(n_dot_l_raw, 0.0);
    let diffuse_term = max(n_dot_l_raw * (1.0 - wrap) + wrap, 0.0);
    let h = normalize(l + s.v);
    let n_dot_v = max(dot(s.n, s.v), 1e-4);
    let a = max(s.roughness * s.roughness, 0.002);
    let f = fresnel(s.f0, max(dot(s.v, h), 0.0));
    let specular = d_ggx(max(dot(s.n, h), 0.0), a) * v_smith(n_dot_v, n_dot_l, a) * f * PI * n_dot_l;
    let diffuse = s.albedo * (1.0 - s.metallic) * (1.0 - f) * diffuse_term;
    return (diffuse + specular) * radiance;
}

fn point_lights(world_pos: vec3<f32>, s: Surface) -> vec3<f32> {
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
        total += shade(s, l, point_light_radiance(pl, d, t), 0.2);
    }
    return total;
}

// Light from `pl` at distance `d` (inside its radius) at time `t`.
fn point_light_radiance(pl: PointLight, d: f32, t: f32) -> vec3<f32> {
    let window = clamp(1.0 - pow(d / pl.position_radius.w, 4.0), 0.0, 1.0);
    let atten = window * window / (d * d + 1.0);

    // Two out-of-phase sines per light read as an irregular fire flicker.
    let seed = hash13(pl.position_radius.xyz) * 100.0;
    let wobble = 0.5 + 0.5 * sin(t * 11.0 + seed) * sin(t * 6.7 + seed * 1.7);
    let flicker = 1.0 - pl.params.x * 0.35 * wobble;

    return pl.color_intensity.rgb * pl.color_intensity.w * atten * flicker;
}

fn sky_gradient(dir: vec3<f32>) -> vec3<f32> {
    if dir.y >= 0.0 {
        return mix(g.sky_horizon.rgb, g.sky_zenith.rgb, pow(dir.y, 0.5));
    }
    return mix(g.sky_horizon.rgb, g.sky_ground.rgb, pow(-dir.y, 0.4));
}

fn hemisphere(dir: vec3<f32>) -> vec3<f32> {
    return mix(g.ambient_ground.rgb, g.ambient_sky.rgb, dir.y * 0.5 + 0.5);
}

// Fog color between the camera and `world_pos` (rgb) and how much of it there is (a).
fn fog_at(world_pos: vec3<f32>) -> vec4<f32> {
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
    return vec4<f32>(g.fog_color.rgb + g.sun_color.rgb * scatter * 0.15, amount);
}

fn apply_fog(color: vec3<f32>, world_pos: vec3<f32>) -> vec3<f32> {
    let fog = fog_at(world_pos);
    return mix(color, fog.rgb, fog.a);
}

@fragment
fn fs_main(in: VsOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    // Sample every texture up front: sampling must stay in uniform control flow.
    let base_sample = textureSample(base_color_tex, material_sampler, in.uv);
    let normal_sample = textureSample(normal_tex, material_sampler, in.uv).xyz;
    let mr_sample = textureSample(metallic_roughness_tex, material_sampler, in.uv);
    let occlusion_sample = textureSample(occlusion_tex, material_sampler, in.uv).r;
    let emissive_sample = textureSample(emissive_tex, material_sampler, in.uv).rgb;
    // Derivatives also need uniform control flow, so take them before any discard.
    let raw_alpha = base_sample.a * material.base_color.a;
    let alpha = coverage(raw_alpha, fwidth(raw_alpha));

    var n = normalize(in.normal);
    if !front && material.flags.w == 0u {
        n = -n;
    }
    if material.flags.x == 1u {
        // Gram-Schmidt keeps the interpolated tangent perpendicular to the normal.
        let t = normalize(in.tangent.xyz - n * dot(n, in.tangent.xyz));
        let b = cross(n, t) * in.tangent.w;
        var tn = normal_sample * 2.0 - 1.0;
        tn = vec3<f32>(tn.xy * material.params.z, tn.z);
        n = normalize(t * tn.x + b * tn.y + n * tn.z);
    }

    var s: Surface;
    s.albedo = in.albedo * base_sample.rgb * material.base_color.rgb;
    s.n = n;
    s.v = normalize(g.camera_pos.xyz - in.world_pos);
    s.metallic = material.params.x * mr_sample.b;
    s.roughness = clamp(material.params.y * mr_sample.g, 0.04, 1.0);
    s.f0 = mix(vec3<f32>(0.04), s.albedo, s.metallic);
    let occlusion = mix(1.0, occlusion_sample, material.params.w);

    let shadow = shadow_factor(in.world_pos, in.normal);
    var color = shade(s, g.sun_dir.xyz, g.sun_color.rgb * shadow, 0.0);
    color += point_lights(in.world_pos, s);

    // Ambient: hemisphere light for diffuse, a blurred sky for reflections.
    let n_dot_v = max(dot(s.n, s.v), 1e-4);
    let r = reflect(-s.v, s.n);
    let environment = mix(sky_gradient(r), hemisphere(r), s.roughness);
    let f_ambient = s.f0 + (max(vec3<f32>(1.0 - s.roughness), s.f0) - s.f0) * pow(1.0 - n_dot_v, 5.0);
    let ambient = hemisphere(s.n) * s.albedo * (1.0 - s.metallic) + environment * f_ambient * (1.0 - s.roughness * 0.5);
    color += ambient * occlusion;

    color += s.albedo * in.emissive + material.emissive.rgb * emissive_sample;
    color = apply_fog(color, in.world_pos);
    return vec4<f32>(color, alpha);
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

// Where `dir` falls on a picture hanging in the sky at `at`, upright; 0..1 inside it.
fn picture_uv(dir: vec3<f32>, at: vec4<f32>) -> vec2<f32> {
    let facing = dot(dir, at.xyz);
    if at.w <= 0.0 || facing <= 0.0 {
        return vec2<f32>(-1.0);
    }
    var right = cross(at.xyz, vec3<f32>(0.0, 1.0, 0.0));
    if dot(right, right) < 1e-4 {
        right = vec3<f32>(1.0, 0.0, 0.0);
    }
    right = normalize(right);
    let up = cross(right, at.xyz);
    let p = dir / facing;
    return vec2<f32>(dot(p, right), -dot(p, up)) / at.w * 0.5 + 0.5;
}

fn picture_mask(uv: vec2<f32>) -> f32 {
    return select(0.0, 1.0, all(uv >= vec2<f32>(0.0)) && all(uv <= vec2<f32>(1.0)));
}

// Scattered points of light that fade towards the horizon and twinkle a little.
fn stars(dir: vec3<f32>) -> f32 {
    if dir.y <= 0.0 {
        return 0.0;
    }
    let scaled = dir * 170.0;
    let cell = floor(scaled);
    let chance = hash13(cell + vec3<f32>(11.0));
    if chance < 0.985 {
        return 0.0;
    }
    let bright = (chance - 0.985) / 0.015;
    let round = 1.0 - smoothstep(0.1, 0.45, length(fract(scaled) - 0.5));
    let twinkle = 0.75 + 0.25 * sin(g.camera_pos.w * (1.0 + bright * 2.0) + chance * 500.0);
    return round * (0.25 + bright * bright * 1.6) * twinkle * smoothstep(0.02, 0.25, dir.y);
}

@fragment
fn fs_sky(in: SkyOut) -> @location(0) vec4<f32> {
    let far = g.inv_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    let dir = normalize(far.xyz / far.w - g.camera_pos.xyz);

    var color = sky_gradient(dir);
    color += vec3<f32>(stars(dir) * g.sky_params.x);

    let cos_sun = dot(dir, g.sky_glow.xyz);
    let glow = pow(max(cos_sun, 0.0), 64.0) * g.sun_disc.y + pow(max(cos_sun, 0.0), 6.0) * g.sun_disc.y * 0.15;
    color += g.sun_color.rgb * glow;

    // The pictures of the sun and moon, or else the plain disc where the light is.
    let above = smoothstep(-0.015, 0.01, dir.y);
    if g.sky_sun.w > 0.0 || g.sky_moon.w > 0.0 {
        let moon_uv = picture_uv(dir, g.sky_moon);
        let moon = textureSampleLevel(moon_picture, sky_sampler, moon_uv, 0.0);
        color = mix(color, moon.rgb * g.sky_params.z, moon.a * picture_mask(moon_uv) * above);
        let sun_uv = picture_uv(dir, g.sky_sun);
        let sun = textureSampleLevel(sun_picture, sky_sampler, sun_uv, 0.0);
        color = mix(color, sun.rgb * g.sky_params.y, sun.a * picture_mask(sun_uv) * above);
    } else {
        let disc = smoothstep(g.sun_disc.x, g.sun_disc.x + 0.0004, cos_sun);
        color += g.sun_color.rgb * disc * 8.0;
    }

    // Fog the sky as if it were very far away, so the horizon melts into the fog.
    color = apply_fog(color, g.camera_pos.xyz + dir * 600.0);
    return vec4<f32>(color, 1.0);
}

// ---------------------------------------------------------------- water

struct WaterOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) tint: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

// `normal_1.w` is the height of the waves. They run along the wind and across it,
// and the shading takes its normals from the faces they make.
@vertex
fn vs_water(v: VertexIn, i: InstanceIn) -> WaterOut {
    var world = instance_model(i) * vec4<f32>(v.position, 1.0);
    let t = g.camera_pos.w * max(g.wind.w, 0.2);
    let along = dot(world.xz, g.wind.xy);
    let across = dot(world.xz, vec2<f32>(-g.wind.y, g.wind.x));
    world.y += i.normal_1.w * (sin(along * 1.3 - t * 1.1) + 0.6 * sin(across * 2.1 + t * 0.8 + along * 0.7));
    var out: WaterOut;
    out.clip = g.view_proj * world;
    out.world_pos = world.xyz;
    out.tint = srgb_to_linear(v.color * i.tint.rgb);
    out.uv = v.uv;
    return out;
}

@fragment
fn fs_water(in: WaterOut) -> @location(0) vec4<f32> {
    // Two layers of the picture drifting at different speeds with the wind.
    let drift = g.wind.xy * g.camera_pos.w * g.wind.w * 0.012;
    let layer_a = textureSample(base_color_tex, material_sampler, in.uv + drift);
    let layer_b = textureSample(base_color_tex, material_sampler, in.uv * 0.61 - drift * 0.7 + vec2<f32>(0.37, 0.11));
    var n = normalize(cross(dpdy(in.world_pos), dpdx(in.world_pos)));
    if n.y < 0.0 {
        n = -n;
    }

    var s: Surface;
    s.albedo = in.tint * (layer_a.rgb + layer_b.rgb) * 0.5 * material.base_color.rgb;
    s.n = n;
    s.v = normalize(g.camera_pos.xyz - in.world_pos);
    s.metallic = 0.0;
    s.roughness = material.params.y;
    s.f0 = vec3<f32>(0.02);

    var color = shade(s, g.sun_dir.xyz, g.sun_color.rgb * shadow_factor(in.world_pos, n), 0.0);
    color += point_lights(in.world_pos, s) + hemisphere(n) * s.albedo;

    // The water mirrors the sky more the lower you look along it, and hides what's under it.
    let mirror = 0.02 + 0.98 * pow(1.0 - max(dot(n, s.v), 0.0), 5.0);
    color = mix(color, sky_gradient(reflect(-s.v, n)), mirror);
    let alpha = mix(material.base_color.a, 1.0, mirror);
    return vec4<f32>(apply_fog(color, in.world_pos), alpha);
}

// ---------------------------------------------------------------- particles

// Mirrors `ParticleInstance` in grimora_core.
struct ParticleIn {
    @location(0) position_size: vec4<f32>,
    @location(1) color: vec4<f32>,          // sRGB, opacity
    @location(2) stretch_rotation: vec4<f32>,
    @location(3) params: vec4<f32>,         // x emissive, y lit, z additive, w shape
}

struct ParticleOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,          // premultiplied
    @location(2) @interpolate(flat) shape: f32,
}

// Light falling on a particle, without shadows: a particle has no surface, so
// each source counts in full and the sun is softened as if scattered inside it.
fn particle_light(world_pos: vec3<f32>) -> vec3<f32> {
    var total = (g.ambient_sky.rgb + g.ambient_ground.rgb) * 0.5 + g.sun_color.rgb * 0.35;
    for (var i = 0u; i < g.counts.x; i++) {
        let pl = g.point_lights[i];
        let d = distance(pl.position_radius.xyz, world_pos);
        if d < pl.position_radius.w {
            total += point_light_radiance(pl, d, g.camera_pos.w);
        }
    }
    return total;
}

@vertex
fn vs_particle(@builtin(vertex_index) index: u32, p: ParticleIn) -> ParticleOut {
    let corner = vec2<f32>(f32(index & 1u), f32(index >> 1u)) * 2.0 - 1.0;
    let center = p.position_size.xyz;
    let half_size = p.position_size.w * 0.5;

    // Camera axes; straight up or down, any right vector will do.
    let forward = g.camera_forward.xyz;
    var right = cross(forward, vec3<f32>(0.0, 1.0, 0.0));
    if dot(right, right) < 1e-4 {
        right = vec3<f32>(1.0, 0.0, 0.0);
    }
    right = normalize(right);
    let up = cross(right, forward);

    // The long side follows the motion as seen on screen; otherwise it just turns.
    let along = vec2<f32>(dot(p.stretch_rotation.xyz, right), dot(p.stretch_rotation.xyz, up));
    let stretch = length(along);
    var axis = vec2<f32>(cos(p.stretch_rotation.w), sin(p.stretch_rotation.w));
    if stretch > 1e-4 {
        axis = along / stretch;
    }
    let side = vec2<f32>(-axis.y, axis.x);
    let offset = axis * corner.y * (half_size + stretch) + side * corner.x * half_size;
    let world = center + right * offset.x + up * offset.y;

    var light = vec3<f32>(p.params.x);
    if p.params.y > 0.0 {
        light += particle_light(center) * p.params.y;
    }
    let color = srgb_to_linear(p.color.rgb) * light;

    // Fog fades added light away and tints what covers the background. Blending
    // comes from alpha alone, so additive particles write none. Right at the lens
    // they fade out instead of filling the screen.
    let opacity = p.color.a * smoothstep(0.1, 0.8, distance(center, g.camera_pos.xyz));
    let cover = opacity * (1.0 - p.params.z);
    let fog = fog_at(center);

    var out: ParticleOut;
    out.clip = g.view_proj * vec4<f32>(world, 1.0);
    out.uv = corner;
    out.color = vec4<f32>(color * opacity * (1.0 - fog.a) + fog.rgb * fog.a * cover, cover);
    out.shape = p.params.w;
    return out;
}

@fragment
fn fs_particle(in: ParticleOut) -> @location(0) vec4<f32> {
    // Derivatives need uniform control flow, so take them before branching.
    let diamond = abs(in.uv.x) + abs(in.uv.y);
    let edge = max(fwidth(diamond), 1e-4);
    var mask = clamp((1.0 - diamond) / edge, 0.0, 1.0);
    if in.shape < 0.5 {
        let falloff = clamp(1.0 - dot(in.uv, in.uv), 0.0, 1.0);
        mask = falloff * falloff;
    }
    return in.color * mask;
}
