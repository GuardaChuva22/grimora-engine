// Grimora scene shader: lit flat-shaded meshes, sky, and the shadow depth pass.
// Everything here outputs linear HDR color; tonemapping happens in post.wgsl.

const MAX_POINT_LIGHTS: u32 = 32u;
const MAX_CASCADES: u32 = 4u;
const MAX_WAVES: u32 = 12u;
const MAX_HULLS: u32 = 8u;

struct PointLight {
    position_radius: vec4<f32>,   // xyz position, w radius
    color_intensity: vec4<f32>,   // rgb linear color, w intensity
    params: vec4<f32>,            // x flicker
}

// Where a hollow hull is. Mirrors `HullPlace`.
struct Hull {
    // The rows of what takes a point in the world to the hull's own frame.
    x: vec4<f32>,
    y: vec4<f32>,
    z: vec4<f32>,
    rect: vec4<f32>,   // its map: min x, min z, 1 / size x, 1 / size z
    info: vec4<f32>,   // x layer of `hull_maps`, y its deck
}

struct Globals {
    view_proj: mat4x4<f32>,
    // From clip space to the offset from the camera, in world axes (divide by w). The depth
    // is reversed: 1 at the near plane, 0 at the far one.
    unproject: mat4x4<f32>,
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
    fog_params: vec4<f32>,        // x height falloff, y base height, z sun scatter, w mist brightness
    sky_zenith: vec4<f32>,
    sky_horizon: vec4<f32>,
    sky_ground: vec4<f32>,
    sun_disc: vec4<f32>,          // x cos(sun radius), y glow strength, z how heaped the clouds are, w lightning
    sky_sun: vec4<f32>,           // xyz where the sun is, w half width of its picture (tangent), 0 = none
    sky_moon: vec4<f32>,          // the same for the moon
    sky_glow: vec4<f32>,          // xyz where the source of the light really is, w = 1 filtered clouds, 0 hard texels
    sky_params: vec4<f32>,        // x stars, y sun picture brightness, z moon picture brightness, w halo
    cloud_dark: vec4<f32>,        // w = how much of the sky is cloud
    cloud_light: vec4<f32>,       // w = scale
    cloud_nebula: vec4<f32>,      // w = speed
    surface: vec4<f32>,           // x texture contrast, y saturation, z mottle, w mottle size in meters
    mottle_dark: vec4<f32>,
    mottle_light: vec4<f32>,      // w = mip bias of the textures
    mist: vec4<f32>,              // x amount per meter, y top, z bank size, w speed
    wind: vec4<f32>,              // xy direction, z strength, w speed
    weather: vec4<f32>,           // x how wet surfaces are, y settled snow, z rain, w wind as a multiple of the scene's
    sea: vec4<f32>,               // x level of the nearest water, y how far waves lean, z how high they are at most
    water_area: vec4<f32>,        // where that water is: min x, min z, max x, max z
    water_deep: vec4<f32>,        // rgb deep color, w clarity in meters
    water_shallow: vec4<f32>,     // rgb shallow color, w murk
    water_state: vec4<f32>,       // x camera's depth, y submerged, z caustics, w how far waves come up the beach
    bed_rect: vec4<f32>,          // the water bed's map: min x, min z, 1 / size x, 1 / size z
    level_map: vec4<f32>,         // `water_levels`: min x, min z, cells per meter, w the sea's level past it
    sea_waves: array<vec4<f32>, MAX_WAVES>, // x, z of the wave number, its angular speed and its height
    sea_phases: array<vec4<f32>, MAX_WAVES>, // what the origin adds to each wave's phases: x along, y bend, z group
    // Positions here are local: what the world's origin is, wrapped by the period of each pattern
    // drawn fixed in the world, which keeps it precise in f32. See origin.rs.
    origin: vec4<f32>,            // xyz the origin wrapped by 10 km, for every pattern with no period of its own
    origin_mottle: vec4<f32>,     // xyz wrapped by 1000 times the stains' noise repeat
    origin_mist: vec4<f32>,       // xz wrapped by 10 times the mist's noise repeat
    hulls: array<Hull, MAX_HULLS>,
    counts: vec4<u32>,            // x point light count, y shadows enabled, z shadow cascades
    point_lights: array<PointLight, MAX_POINT_LIGHTS>,
}

struct MaterialParams {
    base_color: vec4<f32>,        // linear
    params: vec4<f32>,            // x metallic, y roughness, z normal scale, w occlusion strength
    emissive: vec4<f32>,          // rgb emitted light (linear), w alpha cutoff
    flags: vec4<u32>,             // x has normal map, y alpha mode (0 opaque, 1 mask, 2 blend), z alpha output, w foliage
    water_shallow: vec4<f32>,     // water only: rgb shallow color, w clarity in meters
    water_foam: vec4<f32>,        // rgb foam color, w how much foam
    water_look: vec4<f32>,        // x murk, y refraction, z 1 with a picture of foam, w meters one repeat covers
    water_flow: vec4<f32>,        // xy how fast the picture moves along its uv, w 1 for a river
}

@group(0) @binding(0) var<uniform> g: Globals;
// Skinning matrices of every animated instance this frame (see InstanceIn.normal_0.w).
@group(0) @binding(1) var<storage, read> joint_matrices: array<mat4x4<f32>>;
// Four unrelated channels of noise that repeat seamlessly (see noise.rs).
@group(0) @binding(2) var noise_tex: texture_2d<f32>;
@group(0) @binding(3) var noise_sampler: sampler;
// How deep the water is over each point of the sea's map; see WaterBed.
@group(0) @binding(4) var bed_tex: texture_2d<f32>;
@group(0) @binding(5) var hull_maps: texture_2d_array<f32>;
// The level of every body of water around the camera, in cells (see `WaterLevels`):
// level, clarity, caustics, how far past the water's edge.
@group(0) @binding(6) var water_levels: texture_2d<f32>;
@group(1) @binding(0) var shadow_map: texture_depth_2d_array;
@group(1) @binding(1) var shadow_sampler: sampler_comparison;
// Shadow passes only: the cascade being drawn (bound with a dynamic offset).
@group(1) @binding(2) var<uniform> cascade_view_proj: mat4x4<f32>;
// Group 1 of the particle passes: the scene's depth, and a copy of what was drawn so far.
// Without MSAA, the renderer swaps this type for `texture_depth_2d`.
@group(1) @binding(3) var scene_depth: texture_depth_multisampled_2d;
@group(1) @binding(4) var scene_copy: texture_2d<f32>;
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
// The water's own: what was drawn before it. Same swap as `scene_depth`.
@group(3) @binding(0) var water_depth: texture_depth_multisampled_2d;
@group(3) @binding(1) var water_scene: texture_2d<f32>;

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
    @location(6) @interpolate(flat) mottled: f32,
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
    // Each plant's place in the gusts, from its world position (`Sway` in uniforms.rs).
    let phase = i.normal_2.w;
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
    // The stains stay put in the world, so what walks would swim through them.
    out.mottled = select(1.0, 0.0, i.normal_0.w >= 0.0);
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
    if g.water_state.x > 0.0 {
        // In the water, it is the water that hides things, and less so on the way up to the light.
        let haze = UNDERWATER_HAZE * (1.0 - UNDERWATER_CLEAR_ABOVE * smoothstep(0.0, 0.8, dir.y)) / g.water_deep.w;
        return vec4<f32>(water_glow(dir), 1.0 - exp(-dist * haze));
    }
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

// How many times the clarity hides everything, seen from inside the water.
const UNDERWATER_HAZE: f32 = 1.2;
// How much less of it there is looking straight up, which is what keeps a hull in sight.
const UNDERWATER_CLEAR_ABOVE: f32 = 0.55;
// How much of the light above reaches the eye, from the water all around it.
const UNDERWATER_LIGHT: f32 = 0.5;
// How fast the light fades on the way down, per clarity.
const UNDERWATER_DIMMING: f32 = 0.55;
// How much of the pale color the water has looking level and overhead, and how bright it is
// overhead and straight down.
const WATER_LEVEL_PALE: f32 = 0.08;
const WATER_OVERHEAD_PALE: f32 = 0.5;
const WATER_OVERHEAD: f32 = 1.9;
const WATER_BELOW: f32 = 0.45;
// How much of the sun's brightness shows around where it is, through the water.
const WATER_SUN_GLOW: f32 = 0.45;
const UNDERWATER_SHAFTS: f32 = 0.3;
// How far the longest and the shortest shafts come down from the surface.
const SHAFT_LONGEST: f32 = 26.0;
const SHAFT_SHORTEST: f32 = 6.0;
const CAUSTIC_PIXELS: f32 = 20.0;
// The size of the smaller cells of light in meters, how thick their walls are (as a share of a
// cell) and how far the noise bends them, in meters.
const CAUSTIC_CELL: f32 = 1.5;
const CAUSTIC_LINE: f32 = 0.16;
const CAUSTIC_BEND: f32 = 3.5;
const CAUSTIC_WOBBLE: f32 = 1.0;
// How much sooner red fades than green and blue, on the way through water.
const WATER_ABSORPTION: vec3<f32> = vec3<f32>(1.5, 0.75, 0.55);

// How far under its top the mist reaches full strength.
const MIST_DEPTH: f32 = 2.5;
// Pixels of the grain from the middle of the screen to its edge.
const GRAIN_PIXELS: f32 = 400.0;
// How far off the net of light on the bottom starts to fade, and is gone: thinner than a
// pixel, its lines would flicker.
const CAUSTIC_NEAR: f32 = 25.0;
const CAUSTIC_FAR: f32 = 70.0;

// A number from 0 to 1 that changes from one pixel of the screen to the next, to start each
// pixel's steps through a volume at a different spot. It has to go by the screen: by the
// position in the world, it makes fine lines that beat against the pixels.
fn view_grain(world_pos: vec3<f32>) -> f32 {
    let clip = g.view_proj * vec4<f32>(world_pos, 1.0);
    let pixel = floor(clip.xy / max(abs(clip.w), 1e-4) * GRAIN_PIXELS);
    return fract(sin(dot(pixel, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

// How much of the low mist lies between the camera and `world_pos`: a few steps through
// the layer under `mist.y`, adding up banks from the noise that thin out towards the top.
fn mist_at(world_pos: vec3<f32>) -> f32 {
    const STEPS: i32 = 5;
    let top = g.mist.y;
    var a = g.camera_pos.xyz;
    var b = world_pos;
    if g.mist.x <= 0.0 || (a.y >= top && b.y >= top) {
        return 0.0;
    }
    let cross = (top - a.y) / (b.y - a.y);
    if a.y >= top {
        a = mix(a, b, cross);
    } else if b.y >= top {
        b = mix(a, b, cross);
    }
    let along = b - a;
    let drift = g.wind.xy * g.camera_pos.w * g.mist.w;
    // A different start per pixel turns the steps into fine grain instead of bands.
    let start = view_grain(world_pos);
    var sum = 0.0;
    for (var i = 0; i < STEPS; i++) {
        let p = a + along * (f32(i) + start) / f32(STEPS);
        // The noise holds about four banks per repeat.
        let uv = (p.xz + g.origin_mist.xz + drift) / (g.mist.z * 4.0);
        let bank = textureSampleLevel(noise_tex, noise_sampler, uv, 0.0).g;
        let thin = textureSampleLevel(noise_tex, noise_sampler, uv * 3.1, 0.0).a;
        let height = saturate((top - p.y) / MIST_DEPTH);
        sum += smoothstep(0.3, 0.72, bank + (thin - 0.5) * 0.35) * height;
    }
    return 1.0 - exp(-g.mist.x * length(along) * sum / f32(STEPS));
}

// How far a point is from the wall between the two nearest of a field of wandering points, one
// per square of the grid.
fn cell_wall(p: vec2<f32>, t: f32) -> f32 {
    let cell = floor(p);
    let inside = p - cell;
    var nearest = 8.0;
    var second = 8.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let square = vec2<f32>(f32(x), f32(y));
            let seed = cell + square;
            let random = vec2<f32>(hash13(vec3<f32>(seed, 1.0)), hash13(vec3<f32>(seed, 2.0)));
            let d = distance(square + 0.5 + 0.42 * sin(t + 6.2831853 * random), inside);
            if d < nearest {
                second = nearest;
                nearest = d;
            } else if d < second {
                second = d;
            }
        }
    }
    return second - nearest;
}

// The net of bright lines that waves throw on what is under them: the walls of two fields of cells
// that drift apart and bend with the noise, brightest where they cross. It covers everything,
// with no gaps wider than a cell. In square pixels, like every other picture here.
fn caustics(p: vec2<f32>) -> f32 {
    let t = g.camera_pos.w;
    let q = floor(p * CAUSTIC_PIXELS) / CAUSTIC_PIXELS;
    let bend = textureSampleLevel(noise_tex, noise_sampler, q * 0.04 + t * vec2<f32>(0.004, 0.003), 0.0).rg - 0.5;
    // Noise of two sizes, so the walls curve. They also swell and thin along their length.
    let wobble = textureSampleLevel(noise_tex, noise_sampler, q * 0.19 + t * vec2<f32>(-0.006, 0.005), 0.0).ba - 0.5;
    let at = q + bend * CAUSTIC_BEND + wobble * CAUSTIC_WOBBLE;
    let line = CAUSTIC_LINE * (0.5 + (bend.x + 0.5) * 1.0);
    // Turned, so the grid of each field doesn't line up with the square pixels or with the other.
    let turned_a = vec2<f32>(at.x * 0.82 - at.y * 0.57, at.x * 0.57 + at.y * 0.82);
    let turned_b = vec2<f32>(at.x * 0.9 + at.y * 0.44, at.y * 0.9 - at.x * 0.44);
    let a = 1.0 - smoothstep(0.0, line, cell_wall(turned_a / CAUSTIC_CELL, t * 0.6));
    let b = 1.0 - smoothstep(0.0, line, cell_wall((turned_b + vec2<f32>(37.0, 11.0)) / (CAUSTIC_CELL * 1.5), t * -0.45 + 2.0));
    return saturate(a * a * 0.8 + b * b * 0.6);
}

// A shaft of sun where it comes through the surface: how much light it carries (x), and how far
// it comes down (y).
fn shaft_at(at: vec2<f32>) -> vec2<f32> {
    let t = g.camera_pos.w;
    let wide = textureSampleLevel(noise_tex, noise_sampler, at * 0.07 + t * vec2<f32>(0.0035, 0.002), 0.0);
    let thin = textureSampleLevel(noise_tex, noise_sampler, at * 0.23 - t * vec2<f32>(0.005, -0.004), 0.0).a;
    // Few of them, with room between them: bright in the middle and soft at the sides.
    let across = smoothstep(0.6, 0.74, wide.b * 0.6 + thin * 0.4);
    return vec2<f32>(across * across, mix(SHAFT_SHORTEST, SHAFT_LONGEST, wide.r));
}

// Where the sun is, seen from inside the water: the surface bends its light steeply down.
fn sun_in_water() -> vec3<f32> {
    let across = g.sun_dir.xz / 1.33;
    return vec3<f32>(across.x, sqrt(max(1.0 - dot(across, across), 0.0)), across.y);
}

// The color of the water itself along a view: pale overhead, where the light comes from, its
// own color level, and deep below. The deeper the camera, the less of all of it, and red goes first.
fn water_glow(dir: vec3<f32>) -> vec3<f32> {
    let light = g.sun_color.rgb * max(g.sun_dir.y, 0.0) + g.ambient_sky.rgb;
    let depth = max(g.water_state.x, 0.0) * UNDERWATER_DIMMING / g.water_deep.w;
    let lost = exp(-depth * WATER_ABSORPTION);
    let level = mix(g.water_deep.rgb, g.water_shallow.rgb, WATER_LEVEL_PALE);
    // The way up is only as long as the depth, so it loses half as much. Both ramps start flat,
    // or the level view would show as a line across the picture.
    let overhead = (mix(g.water_deep.rgb, g.water_shallow.rgb, WATER_OVERHEAD_PALE) + vec3<f32>(0.06)) * WATER_OVERHEAD * sqrt(lost);
    let around = mix(level, g.water_deep.rgb * WATER_BELOW, smoothstep(0.03, -0.7, dir.y)) * lost;
    let color = mix(around, overhead, smoothstep(-0.03, 0.75, dir.y));
    let toward = max(dot(dir, sun_in_water()), 0.0);
    let glow = shaft_color() * sqrt(lost) * WATER_SUN_GLOW;
    return color * light * UNDERWATER_LIGHT + glow * toward * toward * toward * toward;
}

// The sun's light in the water: nearer white than the water's own pale color.
fn shaft_color() -> vec3<f32> {
    return g.sun_color.rgb * mix(g.water_shallow.rgb, vec3<f32>(1.0), 0.45) * max(g.sun_dir.y, 0.0);
}

// Whether a hull lies on the surface at a point, keeping the sun from the water under it.
fn hull_over(p: vec3<f32>) -> bool {
    let at = vec4<f32>(p, 1.0);
    let size = vec2<f32>(textureDimensions(hull_maps));
    for (var i = 0u; i < g.counts.w; i++) {
        let hull = g.hulls[i];
        let local = vec2<f32>(dot(hull.x, at), dot(hull.z, at));
        if hull_low(hull, vec2<i32>(floor((local - hull.rect.xy) * hull.rect.zw * size))) < 1e5 {
            return true;
        }
    }
    return false;
}

// The shafts of sun in the water between the camera and `world_pos`. A shaft is a straight line
// along the sun's direction, so it is known by where it comes through the surface, and all of
// them run together towards the sun, the way rails do towards the horizon. A view ray meets each
// cylinder around the camera along that direction in one exact spot, with no stepping along the
// ray, and so no steps to show as bands. Each cylinder gives one layer of shafts.
fn light_shafts(world_pos: vec3<f32>) -> f32 {
    const LAYERS: i32 = 7;
    let cam = g.camera_pos.xyz;
    let v = world_pos - cam;
    let dist = length(v);
    let dir = v / max(dist, 1e-4);
    let to_sun = sun_in_water();
    let sideways = max(length(dir - to_sun * dot(dir, to_sun)), 1e-3);
    let haze = UNDERWATER_HAZE / g.water_deep.w;
    // Right where they run together, they would be a tangle.
    let spread = smoothstep(0.03, 0.3, sideways);
    if spread <= 0.0 {
        return 0.0;
    }
    var radius = 5.0;
    var sum = 0.0;
    for (var i = 0; i < LAYERS; i++) {
        let along = radius / sideways;
        radius *= 1.5;
        if along > dist + 1.0 {
            continue;
        }
        let p = cam + dir * along;
        let up = g.sea.x - p.y;
        if up < 0.0 {
            continue;
        }
        let through = p + to_sun * up / to_sun.y;
        let shaft = shaft_at(through.xz + g.origin.xz);
        // Each comes down as far as it comes, and they don't all end at the same depth.
        let fade = 1.0 - smoothstep(0.0, shaft.y, up);
        if shaft.x * fade <= 0.0 || (g.counts.w > 0u && hull_over(through)) {
            continue;
        }
        // The nearest are the widest on the screen, and go lightest.
        sum += shaft.x * fade * smoothstep(3.0, 26.0, along) * saturate(dist - along) * exp(-along * haze);
    }
    return sum * spread;
}

fn apply_fog(color: vec3<f32>, world_pos: vec3<f32>) -> vec3<f32> {
    let fog = fog_at(world_pos);
    if g.water_state.x > 0.0 {
        let lost = exp(-max(g.water_state.x, 0.0) * WATER_ABSORPTION * UNDERWATER_DIMMING / g.water_deep.w);
        return mix(color, fog.rgb, fog.a) + shaft_color() * light_shafts(world_pos) * UNDERWATER_SHAFTS * lost;
    }
    let misted = mix(color, g.fog_color.rgb * g.fog_params.w, mist_at(world_pos));
    return mix(misted, fog.rgb, fog.a);
}

// What the large stains multiply a surface's color by here: two sizes of blotches
// on the ground, drawn out into long streaks down whatever stands up.
fn mottle_at(world_pos: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
    // The noise holds about four blotches per repeat.
    let size = g.surface.w * 4.0;
    let at = world_pos + g.origin_mottle.xyz;
    var p = at.xz;
    if abs(normal.y) < 0.6 {
        p = vec2<f32>(at.x + at.z * 0.73, at.y * 0.22);
    }
    let large = textureSample(noise_tex, noise_sampler, p / size).r;
    let small = textureSample(noise_tex, noise_sampler, p / size * 3.7 + vec2<f32>(0.37, 0.11)).g;
    let t = smoothstep(0.33, 0.68, large * 0.62 + small * 0.38);
    return mix(vec3<f32>(1.0), mix(g.mottle_dark.rgb, g.mottle_light.rgb, t), g.surface.z);
}

// What wet surfaces multiply their color by, and how rough they get.
const CRYSTAL_PIXELS: f32 = 130.0;
const WET_DARKEN: f32 = 0.62;
const WET_ROUGHNESS: f32 = 0.5;
const CAUSTIC_BRIGHTNESS: f32 = 0.75;
const CAUSTIC_IN_SHAFT: f32 = 1.2;
const CAUSTIC_LEAST_ALBEDO: f32 = 0.07;
// What the light is multiplied by between the lines of the net, how much a line adds to that, and
// what is left of it where the net is too far away to see.
const CAUSTIC_BETWEEN: f32 = 0.45;
const CAUSTIC_GATHER: f32 = 1.6;
const CAUSTIC_AVERAGE: f32 = 0.85;
// The light a shaft leaves on what it lands on, under its net.
const SHAFT_POOL: f32 = 0.15;
const UNDERSIDE_LIGHT: f32 = 0.2;
const SNOW_COLOR: vec3<f32> = vec3<f32>(0.5, 0.56, 0.68);

struct GroundTexel {
    texel: vec3<f32>,
    average: vec3<f32>,
}

// How far apart the two scales of a ground picture are, and how the second one is turned, so
// their repeats never line up.
const GROUND_SECOND_SCALE: f32 = 0.37;
const GROUND_TURN: mat2x2<f32> = mat2x2<f32>(vec2<f32>(0.8, 0.6), vec2<f32>(-0.6, 0.8));
// Meters across the slow wash that picks between the two scales and lightens or darkens the
// ground, and how much it does each.
const GROUND_WASH: f32 = 0.0035;
const GROUND_WASH_LIGHT: f32 = 0.6;
// What the greener patches multiply the grass by.
const GROUND_GREEN: vec3<f32> = vec3<f32>(0.62, 0.95, 0.55);
// How much noise bites into the borders between grounds, and the scale of the bite in meters.
const GROUND_EDGE_BITE: f32 = 0.9;
const GROUND_EDGE: f32 = 0.33;
// The rock seen from the side: how much larger it is drawn than from above, and how much its
// derivatives are widened, which picks a blurrier mip.
const ROCK_SIDE_SCALE: f32 = 0.3;
const ROCK_SIDE_BLUR: f32 = 2.0;

// One ground picture at two scales, mixed by `mixing`, as `textureSampleGrad` allows outside
// uniform control flow.
fn ground_pair(tex: texture_2d<f32>, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>, mixing: f32) -> vec4<f32> {
    let uv2 = GROUND_TURN * uv * GROUND_SECOND_SCALE + vec2<f32>(0.37, 0.71);
    let a = textureSampleGrad(tex, material_sampler, uv, dx, dy);
    let b = textureSampleGrad(tex, material_sampler, uv2, GROUND_TURN * dx * GROUND_SECOND_SCALE, GROUND_TURN * dy * GROUND_SECOND_SCALE);
    return mix(a, b, mixing);
}

fn ground_average(tex: texture_2d<f32>) -> vec3<f32> {
    return textureSampleLevel(tex, material_sampler, vec2<f32>(0.5), f32(textureNumLevels(tex) - 1u)).rgb;
}

// The ground of a generated world (`Shading::Ground`): grass (the base picture, tinted by the
// vertex color), sand (the emission slot), rock (the occlusion slot, stored linear) and the bed
// of fresh water (pebbles in the metal slot, mud low down in the normal slot, both linear)
// blended by the weights in the tangent. Noise bites into each border, and the weights are sharpened,
// so the ground changes in crisp ragged patches, never in a smooth gradient: PSX, not clean.
fn ground_texel(in: VsOut, at: vec2<f32>, uv_dx: vec2<f32>, uv_dy: vec2<f32>) -> GroundTexel {
    let bias = exp2(g.mottle_light.w);
    let dx = uv_dx * bias;
    let dy = uv_dy * bias;
    let wash = textureSampleLevel(noise_tex, noise_sampler, at * GROUND_WASH, 0.0);
    let mixing = smoothstep(0.3, 0.7, wash.r);
    let bite = textureSampleLevel(noise_tex, noise_sampler, at * GROUND_EDGE, 0.0).gba - 0.5;
    var w = max(in.tangent + vec4<f32>(bite * GROUND_EDGE_BITE * vec3<f32>(0.6, 1.0, 0.8), bite.x * GROUND_EDGE_BITE), vec4<f32>(0.0));
    w = w * w * w * w;
    w = w / max(w.x + w.y + w.z + w.w, 1e-4);
    let grass = ground_pair(base_color_tex, in.uv, dx, dy, mixing).rgb * in.albedo;
    let sand = ground_pair(emissive_tex, in.uv, dx, dy, mixing).rgb;
    // Rock is also seen from the side, so a cliff isn't the picture smeared down it: the uv
    // keeps its scale (meters per repeat, from the derivatives) and takes the height as v.
    let wp_dx = dpdx(in.world_pos);
    let wp_dy = dpdy(in.world_pos);
    // The picture's repeats per meter come from the material (its tile, in `water_look.w`), never
    // from the derivatives: the height multiplies them, and a pixel's error in those, by some
    // hundred meters, would scatter the picture into grain. Up close a face fills the view, so
    // on the sides it repeats `ROCK_SIDE_SCALE` times as far apart, a mip coarser.
    let per_meter = ROCK_SIDE_SCALE / max(material.water_look.w, 0.01);
    let side_bias = bias * ROCK_SIDE_BLUR;
    let v = -in.world_pos.y * per_meter;
    let face = pow(abs(normalize(in.normal)), vec3<f32>(8.0));
    let faces = face / max(face.x + face.y + face.z, 1e-4);
    let rock_top = ground_pair(occlusion_tex, in.uv, dx, dy, mixing).rgb;
    let side_uv = in.uv * ROCK_SIDE_SCALE;
    let side_dx = uv_dx * ROCK_SIDE_SCALE * side_bias;
    let side_dy = uv_dy * ROCK_SIDE_SCALE * side_bias;
    let rock_x = ground_pair(occlusion_tex, vec2<f32>(side_uv.y, v), vec2<f32>(side_dx.y, -wp_dx.y * per_meter * side_bias),
        vec2<f32>(side_dy.y, -wp_dy.y * per_meter * side_bias), mixing).rgb;
    let rock_z = ground_pair(occlusion_tex, vec2<f32>(side_uv.x, v), vec2<f32>(side_dx.x, -wp_dx.y * per_meter * side_bias),
        vec2<f32>(side_dy.x, -wp_dy.y * per_meter * side_bias), mixing).rgb;
    let rock = srgb_to_linear(rock_top * faces.y + rock_x * faces.x + rock_z * faces.z);
    // Mud where a stream nears the sea, pebbles up the land.
    let muddy = smoothstep(6.0, 2.0, in.world_pos.y - g.level_map.w);
    let pebbles = srgb_to_linear(ground_pair(metallic_roughness_tex, in.uv, dx, dy, mixing).rgb);
    let mud = srgb_to_linear(ground_pair(normal_tex, in.uv, dx, dy, mixing).rgb);
    let bed = mix(pebbles, mud, muddy);
    let bed_average = mix(srgb_to_linear(ground_average(metallic_roughness_tex)), srgb_to_linear(ground_average(normal_tex)), muddy);
    // Two scales of patches, lighter or darker and greener or drier, so a wide view never
    // reads as one flat color.
    let broad = textureSampleLevel(noise_tex, noise_sampler, at * GROUND_WASH * 0.23 + vec2<f32>(0.31, 0.57), 0.0);
    let light = (1.0 + (wash.g - 0.5) * GROUND_WASH_LIGHT) * (1.0 + (broad.r - 0.5) * GROUND_WASH_LIGHT);
    let hue = mix(vec3<f32>(1.0), GROUND_GREEN, smoothstep(0.4, 0.7, broad.g * 0.6 + wash.b * 0.4));
    var out: GroundTexel;
    out.texel = (grass * hue * w.x + sand * w.y + rock * w.z + bed * w.w) * light;
    out.average = (ground_average(base_color_tex) * in.albedo * hue * w.x + ground_average(emissive_tex) * w.y
        + srgb_to_linear(ground_average(occlusion_tex)) * w.z + bed_average * w.w) * light;
    return out;
}

// No water at all.
const NO_WATER: f32 = -1e8;

// The water around a point, from `water_levels`: x its level (below `NO_WATER` if none),
// y clarity, z how strong its caustics are. Between cells it blends those that have water,
// since a river's level slopes down along it. Past the map, there is only the sea.
fn water_level_at(p: vec2<f32>) -> vec3<f32> {
    let size = vec2<i32>(textureDimensions(water_levels));
    let at = (p - g.level_map.xy) * g.level_map.z - 0.5;
    let first = vec2<i32>(floor(at));
    if g.level_map.z <= 0.0 || any(first < vec2<i32>(0)) || any(first + 1 >= size) {
        return vec3<f32>(g.level_map.w, g.water_deep.w, g.water_state.z);
    }
    let f = at - floor(at);
    var sum = vec3<f32>(0.0);
    var weight = 0.0;
    for (var k = 0; k < 4; k++) {
        let corner = vec2<i32>(k & 1, k >> 1);
        let cell = textureLoad(water_levels, first + corner, 0);
        let w = select(1.0 - f.x, f.x, corner.x == 1) * select(1.0 - f.y, f.y, corner.y == 1);
        if cell.x > NO_WATER {
            sum += cell.xyz * w;
            weight += w;
        }
    }
    if weight <= 0.0 {
        return vec3<f32>(NO_WATER * 10.0, 1.0, 0.0);
    }
    return sum / weight;
}

@fragment
fn fs_main(in: VsOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    // Sample every texture up front: sampling must stay in uniform control flow.
    let mottle = mottle_at(in.world_pos, in.normal);
    let fixed_at = in.world_pos.xz + g.origin.xz;
    let puddle_noise = textureSample(noise_tex, noise_sampler, fixed_at * 0.045).b;
    let drift_noise = textureSample(noise_tex, noise_sampler, fixed_at * 0.11).a;
    let rain_noise = textureSample(noise_tex, noise_sampler, fixed_at * 0.9 + g.camera_pos.w * vec2<f32>(0.13, -0.17)).rg;
    let base_sample = textureSampleBias(base_color_tex, material_sampler, in.uv, g.mottle_light.w);
    // The last mip level is the texture's average color.
    let average = textureSampleLevel(base_color_tex, material_sampler, in.uv, f32(textureNumLevels(base_color_tex) - 1u));
    let normal_sample = textureSample(normal_tex, material_sampler, in.uv).xyz;
    let mr_sample = textureSample(metallic_roughness_tex, material_sampler, in.uv);
    let occlusion_sample = textureSample(occlusion_tex, material_sampler, in.uv).r;
    let emissive_sample = textureSample(emissive_tex, material_sampler, in.uv).rgb;
    // Derivatives also need uniform control flow, so take them before any discard.
    let raw_alpha = base_sample.a * material.base_color.a;
    let alpha = coverage(raw_alpha, fwidth(raw_alpha));
    let uv_dx = dpdx(in.uv);
    let uv_dy = dpdy(in.uv);

    var base_rgb = base_sample.rgb;
    var average_rgb = average.rgb;
    var tint = in.albedo;
    if material.flags.w == 3u {
        let ground = ground_texel(in, fixed_at, uv_dx, uv_dy);
        base_rgb = ground.texel;
        average_rgb = ground.average;
        tint = vec3<f32>(1.0);
    }

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

    // Harder and more colorful textures.
    let texel = max(average_rgb + (base_rgb - average_rgb) * g.surface.x, vec3<f32>(0.0));
    let plain = tint * base_rgb * material.base_color.rgb;
    let albedo = tint * texel * material.base_color.rgb;
    let gray = vec3<f32>(dot(albedo, vec3<f32>(0.2126, 0.7152, 0.0722)));

    var s: Surface;
    s.albedo = max(mix(gray, albedo, g.surface.y), vec3<f32>(0.0)) * mix(vec3<f32>(1.0), mottle, in.mottled);
    s.n = n;
    s.v = normalize(g.camera_pos.xyz - in.world_pos);
    s.metallic = material.params.x * mr_sample.b;
    s.roughness = clamp(material.params.y * select(mr_sample.g, 1.0, material.flags.w == 3u), 0.04, 1.0);
    let dry_roughness = s.roughness;

    // Weather on what is out in the open, which leaves characters out, like the stains.
    let foliage = select(0.0, 1.0, material.flags.w == 1u);
    let faces_up = smoothstep(0.35, 0.85, in.normal.y) * in.mottled;
    // The band of sand that the waves keep wet, by whichever body of water is here.
    let water_here = water_level_at(in.world_pos.xz);
    let height_over_water = in.world_pos.y - water_here.x;
    // Inside a boat, it is dry, whatever the level of the water outside.
    let in_water_area = water_here.x > NO_WATER
        && !(g.counts.w > 0u && height_over_water < g.water_state.w + 0.5 && in_hull(in.world_pos, HULL_MARGIN));
    let shore = select(0.0, 1.0 - smoothstep(g.water_state.w * 0.7, g.water_state.w * 1.3 + 0.12, height_over_water - (drift_noise - 0.5) * 0.3), in_water_area);
    let wet = max(g.weather.x, shore) * in.mottled;
    let puddle = smoothstep(0.56, 0.64, puddle_noise) * smoothstep(0.85, 0.97, in.normal.y) * wet * (1.0 - foliage);
    s.albedo *= mix(1.0, WET_DARKEN, wet) * (1.0 - 0.35 * puddle);
    s.roughness = mix(mix(s.roughness, WET_ROUGHNESS, wet * (0.4 + 0.6 * faces_up) * (1.0 - foliage)), 0.1, puddle);
    s.n = normalize(s.n + vec3<f32>(rain_noise.x - 0.5, 0.0, rain_noise.y - 0.5) * 0.22 * puddle * g.weather.z);
    // Snow settles in patches first. It keeps the relief of what is under it and the stains
    // of the world, or it would look clean.
    let drift = faces_up * (0.7 + 0.6 * drift_noise);
    let snow = smoothstep(1.0 - g.weather.y, 1.16 - g.weather.y, drift) * step(0.001, g.weather.y) * (1.0 - 0.28 * foliage);
    let luma = vec3<f32>(0.2126, 0.7152, 0.0722);
    let relief = clamp(dot(texel, luma) / max(dot(average_rgb, luma), 0.02), 0.55, 1.35);
    let snow_color = SNOW_COLOR * mix(1.0, relief, 0.7) * (0.8 + 0.4 * drift_noise) * mix(vec3<f32>(1.0), mottle, 0.6 * in.mottled);
    s.albedo = mix(s.albedo, snow_color, snow);
    s.roughness = mix(s.roughness, 0.8, snow);
    s.f0 = mix(vec3<f32>(0.04), s.albedo, s.metallic);
    let occlusion = mix(1.0, occlusion_sample, material.params.w);

    let shadow = shadow_factor(in.world_pos, in.normal);
    // Water only makes the sun or moon glint as a dry surface would: with the sky full of
    // cloud, there is no one spot of light for it to mirror.
    var dry = s;
    dry.roughness = max(s.roughness, dry_roughness);
    let sunlight = shade(dry, g.sun_dir.xyz, g.sun_color.rgb * shadow, 0.0);
    var color = sunlight + point_lights(in.world_pos, s);

    // Ambient: hemisphere light for diffuse, a blurred sky for reflections.
    let n_dot_v = max(dot(s.n, s.v), 1e-4);
    let r = reflect(-s.v, s.n);
    let environment = mix(sky_gradient(r), hemisphere(r), s.roughness);
    let f_ambient = s.f0 + (max(vec3<f32>(1.0 - s.roughness), s.f0) - s.f0) * pow(1.0 - n_dot_v, 5.0);
    let ambient = hemisphere(s.n) * s.albedo * (1.0 - s.metallic) + environment * f_ambient * (1.0 - s.roughness * 0.5);
    color += ambient * occlusion;

    // Under the water, less light gets down, and what does draws its net on what faces up.
    if in_water_area && height_over_water < 0.0 {
        let below = -height_over_water;
        let to_sun = sun_in_water();
        let sun_lands = in.world_pos + to_sun * below / to_sun.y;
        let seen_from = distance(in.world_pos, g.camera_pos.xyz);
        // Whatever faces the light gets it, and something standing up still catches some.
        let facing = saturate(dot(s.n, to_sun) * 0.8 + 0.2);
        let close = 1.0 - smoothstep(CAUSTIC_NEAR, CAUSTIC_FAR, seen_from);
        let net = caustics(sun_lands.xz + g.origin.xz) * close;
        // Where a shaft comes down, the net is brighter, and the water around it is lit.
        let shaft = shaft_at(sun_lands.xz + g.origin.xz);
        let in_shaft = shaft.x * (1.0 - smoothstep(shaft.y, shaft.y * 1.6, below));
        let lit = net * (CAUSTIC_BRIGHTNESS + in_shaft * CAUSTIC_IN_SHAFT) + in_shaft * SHAFT_POOL;
        let strength = saturate(below * 1.5) * water_here.z;
        // The waves gather the light into the lines and take it from between them, which is what
        // shows on something already bright, like sand or a pale body, lit mostly by the ambient
        // light. Far away, the lines are too fine to see and all that is left is their average.
        let gathered = mix(CAUSTIC_AVERAGE, CAUSTIC_BETWEEN + net * CAUSTIC_GATHER, close) + in_shaft * CAUSTIC_GATHER * net;
        color += (sunlight + ambient * occlusion) * (gathered - 1.0) * saturate(strength);
        // Dark rock has little sun to gather, and still shows it: the net is what tells the eye
        // this is under water. The light is paler than the sun, since water took its gold.
        let brightest = max(g.sun_color.r, max(g.sun_color.g, g.sun_color.b));
        let in_water = mix(g.sun_color.rgb, vec3<f32>(brightest), 0.6) * mix(g.water_shallow.rgb, vec3<f32>(1.0), 0.8);
        color += vec3<f32>(CAUSTIC_LEAST_ALBEDO) * in_water * shadow * facing * strength * lit;
        color *= exp(-below * WATER_ABSORPTION * UNDERWATER_DIMMING / max(water_here.y, 0.01));
        // Nothing lights what faces down: against the bright surface, it is a silhouette.
        color *= mix(UNDERSIDE_LIGHT, 1.0, saturate(s.n.y + 1.0));
    }

    if material.flags.w == 2u {
        // An awake crystal is dark glass around its light, not a lit surface.
        color = color * mix(1.0, 0.3, saturate(in.emissive))
            + crystal_glow(in.world_pos, s.n, s.v, plain) * in.emissive;
    } else {
        color += plain * in.emissive;
    }
    color += material.emissive.rgb * emissive_sample;
    color = apply_fog(color, in.world_pos);
    return vec4<f32>(color, alpha);
}

// What shines from inside a crystal, for an emissive of 1: looks through the face as if it
// bent the view, at clouds on two depths and the veins where they meet, in hard bands and
// square pixels like the rest. The middle stays dark so the rim and the veins have something
// to stand out from.
fn crystal_glow(world_pos: vec3<f32>, n: vec3<f32>, v: vec3<f32>, tint: vec3<f32>) -> vec3<f32> {
    let p = (floor((world_pos + g.origin.xyz) * CRYSTAL_PIXELS) + 0.5) / CRYSTAL_PIXELS;
    let t = g.camera_pos.w;
    let bent = refract(-v, n, 0.72);
    let near = p + bent * 0.3;
    let far = p + bent * 0.85;
    let near_uv = near.xz * 0.22 + near.y * vec2<f32>(0.2, -0.14) + t * vec2<f32>(0.008, 0.016);
    let far_uv = far.xz * 0.4 + far.y * vec2<f32>(-0.25, 0.32) - t * vec2<f32>(0.012, 0.008);
    let clouds = textureSampleLevel(noise_tex, noise_sampler, near_uv, 0.0).r;
    let deep = textureSampleLevel(noise_tex, noise_sampler, far_uv, 0.0).g;
    let vein = step(abs(clouds - deep), 0.012);
    let rim = pow(1.0 - clamp(dot(n, v), 0.0, 1.0), 2.4);
    // Each face has its own brightness, as they catch the light inside differently.
    let face = fract(dot(n, vec3<f32>(5.3, 2.1, 3.7))) * 0.3;
    let breath = 0.9 + 0.1 * sin(t * 1.3);
    let value = (clouds * 0.75 + deep * 0.35 + face + rim * 0.9 - 0.45) * breath;
    let band = floor(clamp(value, 0.0, 1.2) * 6.0) / 6.0;
    let color = mix(tint * vec3<f32>(0.22, 0.1, 0.34), tint, band) * (0.12 + band * band * 1.4);
    let white = mix(tint, vec3<f32>(1.0), 0.4);
    return color + white * (vein * 0.9 + step(0.5, rim) * 0.5) * breath;
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
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
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

// One scatter of points of light, `chance` of them per cell, as wide as `radius`
// of a cell. They twinkle a little.
fn star_layer(dir: vec3<f32>, scale: f32, chance: f32, radius: f32) -> f32 {
    let scaled = dir * scale;
    let roll = hash13(floor(scaled) + vec3<f32>(11.0 + scale));
    if roll < 1.0 - chance {
        return 0.0;
    }
    let bright = (roll - (1.0 - chance)) / chance;
    let round = 1.0 - smoothstep(radius * 0.25, radius, length(fract(scaled) - 0.5));
    let twinkle = 0.75 + 0.25 * sin(g.camera_pos.w * (1.0 + bright * 2.0) + roll * 500.0);
    return round * (0.25 + bright * bright * 1.6) * twinkle;
}

// Big white stars everywhere, and a colored dust of tiny ones gathered in `dust`.
fn stars(dir: vec3<f32>, dust: f32) -> vec3<f32> {
    if dir.y <= 0.0 {
        return vec3<f32>(0.0);
    }
    let white = star_layer(dir, 70.0, 0.03, 0.26) * 2.0 + star_layer(dir, 170.0, 0.05, 0.42);
    let tiny = star_layer(dir, 320.0, 0.02 + dust * 0.22, 0.45);
    let tint = vec3<f32>(0.45, 0.5, 1.0);
    return (vec3<f32>(white) + tint * tiny) * smoothstep(0.02, 0.25, dir.y);
}

// The clouds are rotten on purpose, like a small old texture blown up: the soft blocks of a
// coarse mip, a few shades with hard edges, and those edges gnawed by grit. With filtered
// textures (`g.sky_glow.w`) it's the same small texture, bilinear: no texels and no shades, and
// the grit comes from the mips that hold what fits in one of the texels it would have had.
const CLOUD_MIP: f32 = 2.0;
const CLOUD_GRIT_MIP_WIDE: f32 = 1.2;
const CLOUD_GRIT_MIP_FINE: f32 = 3.0;
const CLOUD_SHADES: f32 = 6.0;
const CLOUD_COVER_STEPS: f32 = 4.0;
const CLOUD_GRIT: f32 = 0.3;
const CLOUD_PIXELS: f32 = 350.0;

// How thick the clouds are at `q` on the noise.
fn cloud_density(q: vec2<f32>) -> f32 {
    return textureSampleLevel(noise_tex, noise_sampler, q * 0.5, CLOUD_MIP).b * 0.52
        + textureSampleLevel(noise_tex, noise_sampler, q * 1.7 + vec2<f32>(0.21, 0.67), CLOUD_MIP).a * 0.3
        + textureSampleLevel(noise_tex, noise_sampler, q * 4.3 + vec2<f32>(0.71, 0.13), CLOUD_MIP).r * 0.18;
}

// Rough noise around 0 that travels with the clouds, to wear away what they are made of.
fn cloud_grit(q: vec2<f32>) -> f32 {
    let filtered = g.sky_glow.w;
    return textureSampleLevel(noise_tex, noise_sampler, q * 3.1 + vec2<f32>(0.37, 0.91), filtered * CLOUD_GRIT_MIP_WIDE).g
        - textureSampleLevel(noise_tex, noise_sampler, q * 11.0 + vec2<f32>(0.59, 0.23), filtered * CLOUD_GRIT_MIP_FINE).a;
}

fn cloud_bands(value: f32, steps: f32) -> f32 {
    return mix(floor(value * steps + 0.5) / steps, value, g.sky_glow.w);
}

@fragment
fn fs_sky(in: SkyOut) -> @location(0) vec4<f32> {
    let dir = normalize((g.unproject * vec4<f32>(in.ndc, 1.0, 1.0)).xyz);
    let high = smoothstep(0.0, 0.14, dir.y);

    // The clouds lie on a flat ceiling, so they draw out and crowd together towards the horizon.
    let ceiling = dir.xz / (max(dir.y, 0.0) + 0.4);
    let drift = g.wind.xy * g.camera_pos.w * g.cloud_nebula.w * 0.004;
    // Square texels on the ceiling, which travel with the clouds, so they don't crawl across them.
    let on_ceiling = ceiling * g.cloud_light.w * 0.34 + drift;
    let p = mix((floor(on_ceiling * CLOUD_PIXELS) + 0.5) / CLOUD_PIXELS, on_ceiling, g.sky_glow.w);
    // Noise bent by noise is what marbles them. `p` jumps between texels, which would confuse
    // an automatic choice of mip.
    let bend = textureSampleLevel(noise_tex, noise_sampler, p * 0.31 + drift * 0.6, 0.0).rg - 0.5;
    let puff = g.sun_disc.z;
    let q = p + bend * 0.75 * (1.0 - puff * 0.8);
    let density = cloud_density(q) + cloud_grit(q) * CLOUD_GRIT;
    // Thicker on the far side from the light is lit on this one, which gives them relief.
    let light_ceiling = g.sky_glow.xz / (max(g.sky_glow.y, 0.0) + 0.4);
    let to_light = light_ceiling - ceiling;
    let towards = to_light / max(length(to_light), 0.001);
    let lit_side = clamp(0.5 + (cloud_density(q) - cloud_density(q + towards * 0.035)) * 11.0, 0.0, 1.0);
    let patches = textureSampleLevel(noise_tex, noise_sampler, p * 0.17 + vec2<f32>(0.53, 0.29), 0.0).a;
    let nebula = cloud_bands(smoothstep(0.42, 0.82, patches + cloud_grit(q) * CLOUD_GRIT * 0.5), CLOUD_COVER_STEPS);

    var color = sky_gradient(dir) + g.cloud_nebula.rgb * nebula * high;
    let bare_sky = color;

    let cos_sun = dot(dir, g.sky_glow.xyz);
    let glow = pow(max(cos_sun, 0.0), 64.0) * g.sun_disc.y + pow(max(cos_sun, 0.0), 6.0) * g.sun_disc.y * 0.15;
    color += g.sun_color.rgb * glow;

    // The pictures of the sun and moon, or else the plain disc where the light is.
    let above = smoothstep(-0.015, 0.01, dir.y);
    var picture = 0.0;
    if g.sky_sun.w > 0.0 || g.sky_moon.w > 0.0 {
        let moon_uv = picture_uv(dir, g.sky_moon);
        let moon = textureSampleLevel(moon_picture, sky_sampler, moon_uv, 0.0);
        let moon_cover = moon.a * picture_mask(moon_uv) * above * min(g.sky_params.x * 4.0, 1.0);
        color = mix(color, moon.rgb * g.sky_params.z, moon_cover);
        let sun_uv = picture_uv(dir, g.sky_sun);
        let sun = textureSampleLevel(sun_picture, sky_sampler, sun_uv, 0.0);
        let sun_cover = sun.a * picture_mask(sun_uv) * above;
        color = mix(color, sun.rgb * g.sky_params.y, sun_cover);
        picture = max(moon_cover, sun_cover);
    } else {
        let disc = smoothstep(g.sun_disc.x, g.sun_disc.x + 0.0004, cos_sun);
        color += g.sun_color.rgb * disc * 8.0;
    }

    let halo = pow(max(cos_sun, 0.0), 26.0) * g.sky_params.w;
    color += g.sun_color.rgb * halo * 0.05 * (1.0 - picture);

    // The clouds pass in front of the sun and moon: thin ones only dim the light and whatever
    // draws it, heavy ones stop all of it, and only the ones it gets through light up around it.
    let amount = g.cloud_dark.w;
    let edge = 1.0 - amount;
    let soft = 1.0 - puff * 0.65;
    // They reach far down, because a moon can sit low enough to be all in this fade.
    let present = smoothstep(0.0, 0.05, dir.y) * step(0.001, amount);
    let cover = cloud_bands(smoothstep(edge - 0.16 * soft, edge + 0.2 * soft, density), CLOUD_COVER_STEPS) * present;
    let through = 1.0 - cloud_bands(smoothstep(edge - 0.16 * soft, edge + 0.32, density), CLOUD_COVER_STEPS) * present;
    let shade = smoothstep(edge, edge + 0.4, density) * 0.55 + lit_side * 0.45;
    let cloud = mix(g.cloud_dark.rgb, g.cloud_light.rgb, cloud_bands(shade * shade, CLOUD_SHADES)) * (1.0 + halo * 1.2 * through);
    // The stars are behind the pictures, whatever is in front of those.
    let starry_sky = bare_sky + stars(dir, nebula) * g.sky_params.x * (1.0 - picture);
    color = max(mix(starry_sky, cloud, cover) + (color - bare_sky) * through, vec3<f32>(0.0));

    // Lightning shows up mostly inside the clouds.
    color += vec3<f32>(0.62, 0.68, 1.0) * g.sun_disc.w * (0.2 + cover * 1.4);

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
    // The slope of the waves, and in `w` whether to use it: a grid as coarse as a large
    // sheet's would show its facets, while a pond's are part of the look.
    @location(3) slope: vec4<f32>,
    // On a sea: where the water rests when no wave moves it, what its waves are a function of.
    @location(4) rest: vec2<f32>,
    // How far the water has been lifted, on a sea.
    @location(5) lift: f32,
    @location(6) @interpolate(flat) is_sea: f32,
    // On a river. x how much of it is falling, y how churned up it is below a fall,
    // z how far across, from -1 to 1 at the banks, w how far down the fall.
    @location(7) fall: vec4<f32>,
    // How much of it shows, from the vertex normal's `y`: a river fades out into the sea.
    @location(8) shown: f32,
    // A river nearing what it flows into takes on its colors: rgb its shallow color, w how much.
    @location(9) join: vec4<f32>,
    @location(10) join_deep: vec3<f32>,
    // On a flow map (fresh water drawn as one surface), which way the water goes, m/s.
    @location(11) flow: vec2<f32>,
}

// How far a point has to be from the sides of a hull, in texels of its map, to count as inside
// it when shading what is there. Without this margin, the outside of a hull, right on its
// edge, would come and go from one pixel to the next.
const HULL_MARGIN: i32 = 3;

// The height of a hull's underside at a texel of its map, or something enormous off the hull.
fn hull_low(hull: Hull, texel: vec2<i32>) -> f32 {
    let size = vec2<i32>(textureDimensions(hull_maps));
    if any(texel < vec2<i32>(0)) || any(texel >= size) {
        return 1e6;
    }
    return textureLoad(hull_maps, texel, i32(hull.info.x), 0).r;
}

// Whether a point is inside a hollow hull, where the water stays out. A point is inside if it
// is below the deck and above the hull's underside, which is exact wherever the outside
// of the hull is what the water meets. Mirrors `HullMap::holds`.
fn in_hull(p: vec3<f32>, margin: i32) -> bool {
    let at = vec4<f32>(p, 1.0);
    let size = vec2<f32>(textureDimensions(hull_maps));
    for (var i = 0u; i < g.counts.w; i++) {
        let hull = g.hulls[i];
        let local = vec3<f32>(dot(hull.x, at), dot(hull.y, at), dot(hull.z, at));
        if local.y >= hull.info.y {
            continue;
        }
        let texel = vec2<i32>(floor((local.xz - hull.rect.xy) * hull.rect.zw * size));
        if local.y <= hull_low(hull, texel) {
            continue;
        }
        if margin == 0 {
            return true;
        }
        if local.y > hull_low(hull, texel + vec2<i32>(margin, 0)) && local.y > hull_low(hull, texel - vec2<i32>(margin, 0))
            && local.y > hull_low(hull, texel + vec2<i32>(0, margin)) && local.y > hull_low(hull, texel - vec2<i32>(0, margin)) {
            return true;
        }
    }
    return false;
}

// How deep the sea is over a point; deep beyond the edges of the map. Mirrors `WaterBed::depth_at`.
fn bed_depth(p: vec2<f32>) -> f32 {
    if g.bed_rect.z <= 0.0 {
        return 1000.0;
    }
    let last = vec2<i32>(textureDimensions(bed_tex)) - 1;
    let uv = (p - g.bed_rect.xy) * g.bed_rect.zw;
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        return 1000.0;
    }
    let at = uv * vec2<f32>(last);
    let i = vec2<i32>(at);
    let f = at - vec2<f32>(i);
    let a = textureLoad(bed_tex, i, 0).r;
    let b = textureLoad(bed_tex, min(i + vec2<i32>(1, 0), last), 0).r;
    let c = textureLoad(bed_tex, min(i + vec2<i32>(0, 1), last), 0).r;
    let d = textureLoad(bed_tex, min(i + vec2<i32>(1, 1), last), 0).r;
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

// How much of its height a wave keeps over water this deep. Mirrors `shoal` in water.rs.
const SHOAL_DEPTH: f32 = 7.0;
const SHOAL_LEAST: f32 = 0.05;

fn shoal(depth: f32) -> f32 {
    return mix(SHOAL_LEAST, 1.0, smoothstep(0.0, SHOAL_DEPTH, depth));
}

// How much of a wave a point this far from the camera can show: a short wave in the far
// distance would only shimmer. `reach` is how far the wave's length keeps it whole.
fn wave_keeps(number: f32, distance: f32, reach: f32) -> f32 {
    let far = max(distance, 1.0) / reach;
    return smoothstep(far * 0.5, far, 6.2831853 / number);
}

// How far a crest bends off a straight line and how many wavelengths along it one bend takes,
// and the same for the groups a wave swells and fades in, which is what makes its crests end.
// Mirrors water.rs.
const CREST_BEND: f32 = 1.6;
const BEND_SPAN: f32 = 6.0;
const GROUP_DEPTH: f32 = 0.45;
const GROUP_ACROSS: f32 = 4.0;
const GROUP_ALONG: f32 = 7.0;

// A wave at a point: where it is in its cycle (x), how much of its height it has there (y), and
// which way its phase grows (zw). Mirrors `Wave::at` in water.rs.
fn wave_at(w: vec4<f32>, i: u32, rest: vec2<f32>, t: f32) -> vec4<f32> {
    let origin = g.sea_phases[i];
    let across = vec2<f32>(-w.y, w.x);
    let bend = dot(across, rest) / BEND_SPAN + origin.y + f32(i) * 2.4;
    let along = dot(w.xy, rest);
    let group = dot(across, rest) / GROUP_ACROSS + along / GROUP_ALONG + origin.z - w.z * t / (2.0 * GROUP_ALONG) + f32(i) * 1.1;
    return vec4<f32>(
        along + origin.x + CREST_BEND * sin(bend) - w.z * t,
        1.0 + GROUP_DEPTH * sin(group),
        w.xy + across * (CREST_BEND / BEND_SPAN) * cos(bend),
    );
}

// The sum of Gerstner waves at a resting point: each lifts the water and moves it toward
// its crests by `lean` of its height, which sharpens the crests and widens the troughs.
fn gerstner(rest: vec2<f32>, t: f32, scale: f32, distance: f32) -> vec3<f32> {
    var sum = vec3<f32>(0.0);
    for (var i = 0u; i < MAX_WAVES; i++) {
        let w = g.sea_waves[i];
        let number = length(w.xy);
        if number <= 0.0 {
            continue;
        }
        let at = wave_at(w, i, rest, t);
        let phase = at.x;
        let height = w.w * at.y * scale * wave_keeps(number, distance, 8.0);
        let side = w.xy / number * height * g.sea.y * cos(phase);
        sum += vec3<f32>(side.x, height * sin(phase), side.y);
    }
    return sum;
}

// The normal of those waves, and in `w` how close the water is to folding over: 1 is the
// sharpest a crest gets.
fn gerstner_normal(rest: vec2<f32>, t: f32, scale: f32, distance: f32) -> vec4<f32> {
    var n = vec3<f32>(0.0, 1.0, 0.0);
    var crest = 0.0;
    for (var i = 0u; i < MAX_WAVES; i++) {
        let w = g.sea_waves[i];
        let number = length(w.xy);
        if number <= 0.0 {
            continue;
        }
        let at = wave_at(w, i, rest, t);
        let phase = at.x;
        let height = w.w * at.y * scale * wave_keeps(number, distance, 22.0);
        let fold = number * height * g.sea.y * sin(phase);
        n -= vec3<f32>(at.z * height * cos(phase), fold, at.w * height * cos(phase));
        crest += fold;
    }
    return vec4<f32>(normalize(n), crest);
}

// The waves that come in to the beach and break on it. Over a slope, a wave in shallow water
// slows to sqrt(g h), so from depth h it takes 2 sqrt(h) / (s sqrt(g)) to reach the shore: every
// crest's phase goes by that time, which turns the fronts to face the shore, brings them closer
// together as they come in, and lands them on the sand one after another. They rise out of the
// swell from `SURF_FROM` meters deep, grow as they shoal, and break where they get taller than
// `SURF_BREAKS` times the depth; from there on they are a bore of foam, as tall as the depth
// lets them be, and the last of it runs up the sand.
const SURF_PERIOD: f32 = 7.5;
const SURF_SLOPE: f32 = 0.045;
const SURF_FROM: f32 = 8.0;
const SURF_HEIGHT: f32 = 0.42;
const SURF_BREAKS: f32 = 0.78;
const SURF_RUNUP: f32 = 0.26;

struct Surf {
    // How far the water is lifted.
    lift: f32,
    // Where it is in the wave's cycle: 0 at the crest, rising toward 1 up the front.
    u: f32,
    // How much of the surf there is here, and how much of the wave has broken.
    share: f32,
    broken: f32,
    // How close an unbroken crest is to breaking.
    near_break: f32,
}

fn surf_at(depth: f32, rest: vec2<f32>, t: f32) -> Surf {
    var out: Surf;
    let share = 1.0 - smoothstep(SURF_FROM * 0.55, SURF_FROM, depth);
    out.share = share;
    if share <= 0.0 {
        return out;
    }
    let fixed = rest + g.origin.xz;
    let d = max(depth, 0.0);
    let wander = textureSampleLevel(noise_tex, noise_sampler, fixed * 0.0045, 0.0).r;
    let travel = 2.0 * sqrt(d) / (SURF_SLOPE * 3.13);
    out.u = fract((travel + t) / SURF_PERIOD + wander * 1.4);
    // Crests come in sets and break in sections, not as one line along the whole coast.
    let sets = textureSampleLevel(noise_tex, noise_sampler, fixed * 0.0031 + vec2<f32>(t * 0.004, 0.0), 0.0).g;
    let wind = clamp(g.weather.w, 0.3, 1.8);
    let shoaled = SURF_HEIGHT * wind * (0.55 + 0.9 * sets) * pow(SURF_FROM / max(d, 0.4), 0.25);
    let breaks = shoaled / (SURF_BREAKS * max(d, 0.05));
    out.broken = smoothstep(0.85, 1.1, breaks);
    out.near_break = smoothstep(0.6, 0.9, breaks) * (1.0 - out.broken);
    // Once broken, the bore is as tall as the depth lets it be, and runs up the sand.
    let most = 0.55 * d + SURF_RUNUP * wind;
    let height = min(shoaled, most) * share;
    // A long gentle back and a steep front, steeper once it has broken.
    var shape = 0.0;
    if out.u < 0.72 {
        let back = 1.0 - out.u / 0.72;
        shape = back * back;
    } else {
        shape = pow((out.u - 0.72) / 0.28, mix(2.0, 3.5, out.broken));
    }
    out.lift = height * (shape - 0.22);
    return out;
}

// `normal_1.w` is the height of a sheet's ripples. They run along the wind and across it,
// and the shading takes its normals from the faces they make. A sea (`normal.z` of 1) comes
// from a grid that follows the camera in steps of `normal.x`, so each vertex stays on the
// same spot in the world and the waves don't crawl through the grid.
@vertex
fn vs_water(v: VertexIn, i: InstanceIn) -> WaterOut {
    var world = instance_model(i) * vec4<f32>(v.position, 1.0);
    var out: WaterOut;
    out.tint = srgb_to_linear(v.color * i.tint.rgb);
    out.uv = v.uv;
    out.shown = 1.0;
    out.join = vec4<f32>(0.0);
    out.join_deep = vec3<f32>(0.0);
    out.flow = vec2<f32>(0.0);
    if v.normal.z > 0.5 {
        // On the grid of the world, not of the local positions, which move with the origin.
        let center = floor((g.camera_pos.xz + g.origin.xz) / v.normal.x) * v.normal.x - g.origin.xz;
        // The rings go around the camera: the sea's own place (which the origin moves) is left out.
        let rest = world.xz - i.model_3.xz + center;
        let t = g.camera_pos.w;
        let depth = bed_depth(rest);
        var moved = gerstner(rest, t, shoal(depth), distance(rest, center));
        moved.y += surf_at(depth, rest, t).lift;
        world = vec4<f32>(rest.x + moved.x, world.y + moved.y, rest.y + moved.z, 1.0);
        out.rest = rest;
        out.lift = moved.y;
        out.is_sea = 1.0;
        out.slope = vec4<f32>(0.0);
        out.clip = g.view_proj * world;
        out.world_pos = world.xyz;
        return out;
    }
    out.fall = v.tangent;
    out.shown = v.normal.y;
    // `normal.z` 0.25 marks a river that blends into another water: its vertex color is the
    // other's deep color, and the weights its shallow color and how much to take of them.
    if v.normal.z > 0.2 {
        out.join = vec4<f32>(srgb_to_linear(v.weights.yzw), v.weights.x);
        out.join_deep = srgb_to_linear(v.color);
        out.tint = srgb_to_linear(i.tint.rgb);
    }
    // `normal.z` 0.35 marks fresh water drawn as one surface in chunks: its uv is the flow,
    // and its ripples go by the world, or two chunks would part where they meet.
    let flowing = v.normal.z > 0.3 && v.normal.z < 0.5;
    if flowing {
        out.flow = v.uv;
    }
    // What boils under a fall, and the bumps running down a sheet of it.
    // Ripples and boils go with the sheet, measured from its middle, so the origin doesn't move them.
    let own = select(world.xz - i.model_3.xz, world.xz + g.origin.xz, flowing);
    let boil = max(v.tangent.y, v.tangent.x * 0.6) * FALL_BOIL;
    world.y += boil * sin(own.x * 2.3 + g.camera_pos.w * 7.0) * cos(own.y * 1.9 - g.camera_pos.w * 5.3);
    let t = g.camera_pos.w * max(g.wind.w, 0.2);
    // A coarser grid carries longer waves. `normal.x` says how much longer.
    let along = dot(own, g.wind.xy) * v.normal.x;
    let across = dot(own, vec2<f32>(-g.wind.y, g.wind.x)) * v.normal.x;
    // The harder the wind, the higher the waves.
    let height = i.normal_1.w * g.weather.w;
    world.y += height * (sin(along * 1.3 - t * 1.1) + 0.6 * sin(across * 2.1 + t * 0.8 + along * 0.7));
    let second = 0.6 * cos(across * 2.1 + t * 0.8 + along * 0.7);
    let rise_along = (1.3 * cos(along * 1.3 - t * 1.1) + 0.7 * second) * height * v.normal.x;
    let rise_across = 2.1 * second * height * v.normal.x;
    out.slope = vec4<f32>(
        rise_along * g.wind.x - rise_across * g.wind.y,
        0.0,
        rise_along * g.wind.y + rise_across * g.wind.x,
        step(v.normal.x, 0.99),
    );
    out.rest = world.xz;
    out.clip = g.view_proj * world;
    out.world_pos = world.xyz;
    return out;
}

// How bright the sun is in the water.
const WATER_GLINT: f32 = 2.5;
// Waves lean the mirror up, so water never shows the sky lower than this.
const WATER_LOWEST_MIRROR: f32 = 0.22;
const WATER_FAR_DEEPEN: f32 = 0.6;
// How much of the water's own color the mirrored sky takes.
const WATER_TINTED_MIRROR: f32 = 0.7;
// Pixels of bend at the top of the screen's height, for a refraction of 1.
const WATER_BEND: f32 = 0.05;
// How many square pixels of foam fit in a meter.
const FOAM_PIXELS: f32 = 8.0;
// Seconds a flow map's picture slides before that phase gives way to the other.
const FLOW_PERIOD: f32 = 3.0;
// The light that comes through the top of a wave with the sun behind it.
const WAVE_GLOW: f32 = 0.55;
// How bright the water is at the bottom of a wave.
const WAVE_TROUGH: f32 = 0.7;
const WATER_SCATTER: f32 = 0.25;
// How many square pixels of a fall fit in a meter, how fast it comes down in meters per
// second, and how high what it stirs up is.
const FALL_PIXELS: f32 = 5.0;
const FALL_SPEED: f32 = 7.0;
const FALL_BOIL: f32 = 0.14;
// Under full sun, white water would be all bloom, without a line left in it.
const FALL_BRIGHTNESS: f32 = 0.7;
// How bright the underside of the surface is against what is overhead, between its ripples
// and on them, and its foam and the sun.
const UNDER_SURFACE_DARK: f32 = 0.7;
const UNDER_SURFACE_LIGHT: f32 = 1.25;
const UNDER_SURFACE_FOAM: f32 = 2.0;
const UNDER_SURFACE_SUN: f32 = 5.0;
// How wide the ring of foam around what stands in the water is, in meters.
const CONTACT_FOAM: f32 = 0.5;

// The world position of what was drawn before the water, at a pixel.
fn behind_water(pixel: vec2<i32>, size: vec2<f32>) -> vec3<f32> {
    let ndc = (vec2<f32>(pixel) + 0.5) / size * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0);
    let p = g.unproject * vec4<f32>(ndc, textureLoad(water_depth, pixel, 0), 1.0);
    return g.camera_pos.xyz + p.xyz / p.w;
}

@fragment
fn fs_water(in: WaterOut) -> @location(0) vec4<f32> {
    let river = material.water_flow.w > 0.5;
    // A flow map: the picture is fixed to the world and slides along `in.flow`, in two
    // phases that take turns, each fading out before it has slid too far.
    let flowing = material.water_flow.w > 1.5;
    let tile = material.water_look.w;
    let is_sea = in.is_sea > 0.5;
    let deep_color = mix(in.tint * material.base_color.rgb, in.join_deep, in.join.w);
    let shallow_color = mix(material.water_shallow.rgb, in.join.rgb, in.join.w);
    let view = g.camera_pos.xyz - in.world_pos;
    let distance_to = length(view);
    let time = g.camera_pos.w;

    // Two layers of the picture drifting at different speeds with the wind, or along a river.
    var drift = g.wind.xy * time * g.wind.w * 0.012;
    if river {
        drift = -material.water_flow.xy * time;
    }
    // `rest` is local; the pictures and the noise are fixed to the world.
    let fixed_rest = in.rest + g.origin.xz;
    let fixed_at = in.world_pos.xz + g.origin.xz;
    var uv = select(in.uv, fixed_rest / tile, is_sea);
    let phase = fract(time / FLOW_PERIOD);
    var drift_b = drift;
    var turn = 0.0;
    if flowing {
        uv = fixed_at / tile;
        drift = -in.flow / tile * phase * FLOW_PERIOD;
        drift_b = -in.flow / tile * fract(phase + 0.5) * FLOW_PERIOD + vec2<f32>(0.31, 0.17);
        turn = abs(phase * 2.0 - 1.0);
    }
    // What isn't sampled in both phases stays put on a flow map, or it would jump each period.
    let still = select(drift, vec2<f32>(0.0), flowing);
    let marble_bend = textureSample(noise_tex, noise_sampler, fixed_at * 0.006 + still * 0.2).rg - 0.5;
    let layer_a = mix(
        textureSample(base_color_tex, material_sampler, uv + drift + marble_bend * 0.6),
        textureSample(base_color_tex, material_sampler, uv + drift_b + marble_bend * 0.6),
        turn,
    );
    let layer_b = mix(
        textureSample(base_color_tex, material_sampler, uv * 0.61 - drift * 0.7 + vec2<f32>(0.37, 0.11) - marble_bend * 0.4),
        textureSample(base_color_tex, material_sampler, uv * 0.61 - drift_b * 0.7 + vec2<f32>(0.37, 0.11) - marble_bend * 0.4),
        turn,
    );
    let average = textureSampleLevel(base_color_tex, material_sampler, uv, f32(textureNumLevels(base_color_tex) - 1));
    // Far off the picture gives way to large marbled currents, which don't repeat.
    let far = smoothstep(12.0, 80.0, distance_to);
    let current_a = textureSample(noise_tex, noise_sampler, fixed_at * 0.011 + marble_bend * 0.35 + still * 0.1).b;
    let current_b = textureSample(noise_tex, noise_sampler, fixed_at * 0.0037 - marble_bend * 0.2).a;
    let current = current_a * 0.55 + current_b * 0.45;
    // Fine ripples from the noise, which is what breaks the sun into a trail of glints.
    let ripple_at = select(fixed_at, uv * tile, river && !flowing);
    let ripple_a = mix(
        textureSample(noise_tex, noise_sampler, ripple_at * 0.11 + drift * 2.3).rg,
        textureSample(noise_tex, noise_sampler, ripple_at * 0.11 + drift_b * 2.3).rg,
        turn,
    );
    let ripple_b = mix(
        textureSample(noise_tex, noise_sampler, ripple_at * 0.37 - drift * 3.1).ba,
        textureSample(noise_tex, noise_sampler, ripple_at * 0.37 - drift_b * 3.1).ba,
        turn,
    );
    // On a river it follows the water around the bends, like the picture does.
    // The strands of a fall: long lines down the water, each pair of layers at its own speed.
    // On a flow map, along and across the flow.
    var fall_uv = uv * tile;
    if flowing {
        let down = in.flow / max(length(in.flow), 1e-4);
        fall_uv = vec2<f32>(dot(fixed_at, vec2<f32>(-down.y, down.x)), dot(fixed_at, down));
    }
    let fall_at = floor(fall_uv * FALL_PIXELS) / FALL_PIXELS;
    let fall_t = time * FALL_SPEED;
    let strand_a = textureSampleLevel(noise_tex, noise_sampler, vec2<f32>(fall_at.x * 0.13, (fall_at.y - fall_t) * 0.009), 0.0).b;
    let strand_b = textureSampleLevel(noise_tex, noise_sampler, vec2<f32>(fall_at.x * 0.41 + 0.37, (fall_at.y - fall_t * 1.5) * 0.023), 0.0).a;
    let strand = strand_a * 0.6 + strand_b * 0.4;
    let foam_uv = select(fixed_rest / max(tile * 0.8, 0.01), uv / 0.8, river);
    let foam_picture = mix(
        textureSample(emissive_tex, material_sampler, foam_uv + drift),
        textureSample(emissive_tex, material_sampler, foam_uv + drift_b),
        turn,
    );
    var facet = normalize(cross(dpdy(in.world_pos), dpdx(in.world_pos)));
    if facet.y < 0.0 {
        facet = -facet;
    }

    // A fall frays at its sides, and the rock shows through.
    if in.fall.x > 0.4 && abs(in.fall.z) > 1.02 - (strand - 0.42) * 1.5 {
        discard;
    }
    // The water doesn't get inside a boat.
    if g.counts.w > 0u && in_hull(in.world_pos, 0) {
        discard;
    }

    // ---- the surface
    let depth_below = select(0.0, bed_depth(in.rest), is_sea);
    var n = facet;
    var crest = 0.0;
    if is_sea {
        let sea_n = gerstner_normal(in.rest, time, shoal(depth_below), distance_to);
        crest = sea_n.w;
        // The breakers aren't in that sum, but their faces are in the grid's.
        n = normalize(mix(sea_n.xyz, facet, surf_at(depth_below, in.rest, time).share * 0.75));
    } else if in.slope.w > 0.5 {
        n = normalize(vec3<f32>(-in.slope.x, 1.0, -in.slope.z));
    }
    n = mix(n, vec3<f32>(0.0, 1.0, 0.0), far * select(1.0, 0.4, is_sea));
    let ripple = (ripple_a + ripple_b - 1.0) * 0.32 * (1.0 - far * 0.7) * (1.0 + g.weather.z)
        * select(1.0, 0.45, is_sea);
    n = normalize(n + vec3<f32>(ripple.x, 0.0, ripple.y));

    // ---- foam, in square pixels fixed on the water, from what the waves are doing
    let foam_at = floor(fixed_rest * FOAM_PIXELS) / FOAM_PIXELS;
    let lod = max(log2(distance_to / 30.0), 0.0);
    let foam_a = textureSampleLevel(noise_tex, noise_sampler, foam_at * 0.073 + still * 0.4, lod).b;
    let foam_b = textureSampleLevel(noise_tex, noise_sampler, foam_at * 0.29 - still * 0.7, lod).a;
    var lace = foam_a * 0.55 + foam_b * 0.45;
    if material.water_look.z > 0.5 {
        lace = lace * 0.3 + foam_picture.r * 0.7;
    }
    // A web of holes, or foam would be flat white shapes.
    let foam_fine = textureSampleLevel(noise_tex, noise_sampler, foam_at * 0.71 + still * 0.9, lod).g;
    let web = smoothstep(0.38, 0.55, foam_fine * 0.6 + lace * 0.4);
    // On the crests, more of it the sharper they get: thick on the crest, and thin water-borne
    // lace around it.
    let crest_at = crest + (lace - 0.5) * 0.5;
    let on_crest = max(smoothstep(0.4, 0.5, crest_at) * (0.35 + 0.65 * web), smoothstep(0.26, 0.4, crest_at) * web * 0.4);

    // ---- seen from under the water: the brightest thing there is, a window of sky overhead and
    // all around it the water lit from above, rippling with the waves
    if g.water_state.x > 0.0 {
        let down = -n;
        let v = view / max(distance_to, 1e-4);
        let bent = refract(-v, down, 1.33);
        let overhead = water_glow(vec3<f32>(0.0, 1.0, 0.0));
        let rippled = smoothstep(0.4, 0.6, current_a * 0.3 + (ripple_a.x + ripple_b.x) * 0.35);
        // The net the waves throw on the bottom starts here. Right under it, its square pixels
        // would be as big as bricks.
        let net = caustics(fixed_rest) * 0.6 * (1.0 - far) * smoothstep(0.5, 3.0, distance_to);
        var seen = overhead * (UNDER_SURFACE_DARK + (UNDER_SURFACE_LIGHT - UNDER_SURFACE_DARK) * rippled + net);
        // Total reflection leaves `bent` at zero.
        if dot(bent, bent) > 0.01 {
            let out = normalize(bent);
            let sun = pow(max(dot(out, g.sky_glow.xyz), 0.0), 90.0) * UNDER_SURFACE_SUN;
            let sky = overhead * (UNDER_SURFACE_LIGHT + net) + sky_gradient(out) * 0.6 + g.sun_color.rgb * sun;
            seen = mix(seen, sky, smoothstep(0.62, 0.8, dot(v, down)));
        }
        seen = mix(seen, material.water_foam.rgb * overhead * UNDER_SURFACE_FOAM, on_crest * material.water_foam.w * (1.0 - far * 0.35));
        return vec4<f32>(apply_fog(seen, in.world_pos), 1.0);
    }

    // ---- what is under the water
    let size = vec2<f32>(textureDimensions(water_depth));
    let pixel = vec2<i32>(in.clip.xy);
    let straight = behind_water(pixel, size);
    let straight_depth = max(in.world_pos.y - straight.y, 0.0);
    let bend = n.xz * material.water_look.y * WATER_BEND * size.y * saturate(straight_depth * 0.5) / (1.0 + distance_to * 0.08);
    let bent_pixel = clamp(pixel + vec2<i32>(bend), vec2<i32>(0), vec2<i32>(size) - 1);
    let bent_behind = behind_water(bent_pixel, size);
    // What stands out of the water isn't bent into it.
    let is_under = bent_behind.y < in.world_pos.y;
    let seen_pixel = select(pixel, bent_pixel, is_under);
    let behind = select(straight, bent_behind, is_under);
    let thickness = distance(behind, in.world_pos);
    let clarity = material.water_shallow.w;
    let through = exp(-thickness / clarity * WATER_ABSORPTION);
    let under = textureLoad(water_scene, seen_pixel, 0).rgb;

    // ---- the water's own color
    var s: Surface;
    // On a sea, how deep it is goes by the bottom: a hull just under the surface isn't a shoal.
    let water_below = select(in.world_pos.y - behind.y, max(in.world_pos.y - behind.y, depth_below), is_sea);
    let deepness = 1.0 - exp(-max(water_below, 0.0) / (clarity * 0.5));
    let body = mix(shallow_color, deep_color, deepness);
    let picture = mix((layer_a.rgb + layer_b.rgb) * 0.5, average.rgb, far);
    s.albedo = body * picture * mix(1.0, 0.55 + 0.9 * current, far);
    s.n = n;
    s.v = view / max(distance_to, 1e-4);
    s.metallic = 0.0;
    s.roughness = material.params.y;
    s.f0 = vec3<f32>(0.02);

    let shadow = shadow_factor(in.world_pos, n);
    var color = shade(s, g.sun_dir.xyz, g.sun_color.rgb * shadow, 0.0);
    color += point_lights(in.world_pos, s) + hemisphere(n) * s.albedo;
    // A murky water gives back more of the light that enters it, and a clear one lets it through.
    let murk = material.water_look.x;
    color *= 0.75 + murk * 0.5;
    // Light that went in and came back out with the color of the water: what makes a sea blue,
    // whatever the picture on it. A pond shows only its picture.
    if is_sea || river {
        color += body * (g.sun_color.rgb * max(g.sun_dir.y, 0.0) + g.ambient_sky.rgb) * WATER_SCATTER;
    }

    // A wave is darker in its trough, and its top, with the sun behind it, lets the light through.
    // The currents break that up, or it would be one flat color per height. Only a wave seen from
    // the side is thin to the eye: from above, the same light would be patches like reefs.
    if is_sea && g.sea.z > 0.0 {
        let toward_sun = max(dot(normalize(vec3<f32>(-s.v.x, 0.05, -s.v.z)), g.sky_glow.xyz), 0.0);
        let rise = in.lift / g.sea.z * 2.0 + (current_a - 0.5) * 0.5;
        color *= mix(WAVE_TROUGH, 1.0, smoothstep(-0.7, 0.3, rise));
        let top = smoothstep(0.1, 1.0, rise);
        let edge_on = 1.0 - saturate(s.v.y * 3.0);
        let through = mix(deep_color, shallow_color, 0.6);
        color += through * g.sun_color.rgb * top * top * edge_on * edge_on * (0.2 + 0.8 * toward_sun * toward_sun) * WAVE_GLOW;
    }
    color = mix(under * through, color, 1.0 - min(min(through.r, through.g), through.b) * (1.0 - murk * 0.3));

    // The water mirrors the sky more the lower you look along it, and hides what's under it.
    // Far away the waves hide part of it, and the color of the water itself shows.
    let mirror = (0.02 + 0.98 * pow(1.0 - max(dot(n, s.v), 0.0), 5.0)) * (1.0 - far * WATER_FAR_DEEPEN);
    var mirrored = reflect(-s.v, n);
    mirrored.y = abs(mirrored.y);
    color += g.sun_color.rgb * pow(max(dot(mirrored, g.sky_glow.xyz), 0.0), 300.0) * WATER_GLINT * (1.0 - mirror) * shadow;
    mirrored.y = max(mirrored.y, WATER_LOWEST_MIRROR);
    let hue = deep_color;
    let tinted = mix(vec3<f32>(1.0), hue / max(max(hue.r, max(hue.g, hue.b)), 0.001), WATER_TINTED_MIRROR);
    color = mix(color, sky_gradient(normalize(mirrored)) * tinted, mirror);

    // A glow from below, brighter where the picture's ripples are.
    let ripples = dot(layer_a.rgb + layer_b.rgb, vec3<f32>(0.17));
    color += material.emissive.rgb * (0.15 + 3.0 * smoothstep(0.06, 0.4, ripples));

    // ---- the rest of the foam
    // Around what stands in the water and along its shore, reaching out in a broken ring.
    // Far off the depth buffer can't tell a few centimeters apart, and every shallow would ring.
    let ring = (1.0 - saturate(straight_depth / CONTACT_FOAM)) * (1.0 - smoothstep(300.0, 900.0, distance_to));
    let on_edge = smoothstep(0.5, 0.56, lace + ring * ring * 0.62 - 0.12) * step(0.001, ring) * (0.35 + 0.65 * web);
    // The front of each breaker, and what it leaves behind as it goes in.
    var on_break = 0.0;
    if is_sea {
        let surf = surf_at(depth_below, in.rest, time);
        if surf.share > 0.0 {
            // How far behind the crest, as a share of the wave: the white front is right on it,
            // and the bore it drags thins out into lace with ever bigger holes.
            let behind_crest = select(surf.u, 0.0, surf.u > 0.97);
            let front = 1.0 - smoothstep(0.0, 0.035, behind_crest);
            let bore = 1.0 - smoothstep(0.0, 0.34, behind_crest);
            let white = surf.broken * max(front * (0.75 + 0.25 * web), smoothstep(0.55, 0.6, bore * 0.75 + lace * 0.45 - 0.05) * web);
            // A thin white lip on a crest about to break.
            let lip = surf.near_break * smoothstep(0.55, 0.6, (1.0 - smoothstep(0.0, 0.02, behind_crest)) * 0.7 + lace * 0.3) * web;
            on_break = max(white, lip) * surf.share;
        }
    }
    // Under a fall, it boils.
    let boiling = textureSampleLevel(noise_tex, noise_sampler, foam_at * 0.17 + vec2<f32>(0.0, time * 0.21), 0.0).g;
    let on_churn = smoothstep(0.5, 0.56, lace * 0.5 + boiling * 0.5 + in.fall.y * 0.28 - 0.22) * step(0.01, in.fall.y);
    let foam = saturate(max(max(max(on_crest, on_edge), on_break), on_churn) * material.water_foam.w) * (1.0 - far * 0.6);
    let foam_light = g.sun_color.rgb * shadow * max(g.sun_dir.y, 0.05) + hemisphere(vec3<f32>(0.0, 1.0, 0.0));
    color = mix(color, material.water_foam.rgb * foam_light, foam);

    // ---- a fall: hard steps from the water's own color to white, whiter the further down
    if in.fall.x > 0.01 {
        let whiten = strand + in.fall.w * 0.2 - 0.04;
        var strands = mix(deep_color, shallow_color, 0.35);
        if whiten > 0.62 {
            strands = material.water_foam.rgb;
        } else if whiten > 0.52 {
            strands = mix(shallow_color, material.water_foam.rgb, 0.45);
        }
        color = mix(color, strands * foam_light * FALL_BRIGHTNESS, smoothstep(0.1, 0.55, in.fall.x));
    }

    // A river fading into the sea gives way to what is behind it, already fogged.
    let behind_color = textureLoad(water_scene, pixel, 0).rgb;
    return vec4<f32>(mix(behind_color, apply_fog(color, in.world_pos), saturate(in.shown)), 1.0);
}

// ---------------------------------------------------------------- particles

// Mirrors `ParticleInstance` in grimora_core.
struct ParticleIn {
    @location(0) position_size: vec4<f32>,
    @location(1) color: vec4<f32>,          // sRGB, opacity
    @location(2) stretch_rotation: vec4<f32>,
    @location(3) params: vec4<f32>,         // x emissive, y lit, z additive, w shape
    @location(4) look: vec4<f32>,           // x burned away, y pixels per meter, z soft contact, w bending
    @location(5) flow: vec4<f32>,           // xy noise's motion, z its repeats, w how far it bends the shape
    @location(6) detail: vec4<f32>,         // x color steps, y random, z noise's bite, w 1 for all of this
    @location(7) rim: vec4<f32>,            // sRGB, glow
    @location(8) body: vec4<f32>,
    @location(9) heart: vec4<f32>,
}

struct ParticleOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,          // premultiplied
    @location(2) @interpolate(flat) shape: f32,
    // The ramp is ready to add: linear, lit and faded. `color` then holds only the fog.
    @location(3) @interpolate(flat) rim: vec3<f32>,
    @location(4) @interpolate(flat) body: vec3<f32>,
    @location(5) @interpolate(flat) heart: vec3<f32>,
    @location(6) @interpolate(flat) look: vec4<f32>,
    @location(7) @interpolate(flat) flow: vec4<f32>,
    @location(8) @interpolate(flat) detail: vec4<f32>,
    // How many square pixels fit in half the particle, each way.
    @location(9) @interpolate(flat) texels: vec2<f32>,
    @location(10) view_depth: f32,
    // Meters from the head, along a trail.
    @location(11) along: f32,
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
    let lying = p.params.w > 9.5;
    let shape = p.params.w - select(0.0, 10.0, lying);
    let trail = u32(shape + 0.5) == 5u;

    // Camera axes; straight up or down, any right vector will do.
    let forward = g.camera_forward.xyz;
    var right = cross(forward, vec3<f32>(0.0, 1.0, 0.0));
    if dot(right, right) < 1e-4 {
        right = vec3<f32>(1.0, 0.0, 0.0);
    }
    right = normalize(right);
    var up = cross(right, forward);
    if lying {
        right = vec3<f32>(1.0, 0.0, 0.0);
        up = vec3<f32>(0.0, 0.0, -1.0);
    }

    // The long side follows the motion as seen on screen; otherwise it just turns.
    let along = vec2<f32>(dot(p.stretch_rotation.xyz, right), dot(p.stretch_rotation.xyz, up));
    let stretch = length(along);
    var axis = vec2<f32>(cos(p.stretch_rotation.w), sin(p.stretch_rotation.w));
    if stretch > 1e-4 {
        axis = along / stretch;
    }
    let side = vec2<f32>(-axis.y, axis.x);
    let offset = axis * corner.y * (half_size + stretch) + side * corner.x * half_size;
    var world = center + right * offset.x + up * offset.y;
    if trail {
        // End to end in the world, so the pieces meet whatever the depth.
        let across = side * corner.x * half_size;
        world = center + p.stretch_rotation.xyz * corner.y + right * across.x + up * across.y;
    }

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
    out.shape = shape;
    if trail {
        out.along = p.stretch_rotation.w + corner.y * length(p.stretch_rotation.xyz);
    }
    out.look = p.look;
    out.flow = p.flow;
    out.detail = p.detail;
    if p.detail.w > 0.5 {
        let fade = color * opacity * (1.0 - fog.a);
        out.rim = srgb_to_linear(p.rim.rgb) * p.rim.a * fade;
        out.body = srgb_to_linear(p.body.rgb) * p.body.a * fade;
        out.heart = srgb_to_linear(p.heart.rgb) * p.heart.a * fade;
        out.color = vec4<f32>(fog.rgb * fog.a * cover, cover);
        out.texels = vec2<f32>(half_size, half_size + stretch) * p.look.y;
        out.view_depth = dot(world - g.camera_pos.xyz, forward);
    }
    return out;
}

// Two thin rings, with a band of strokes between them, a different sign in each cell.
fn rune_field(uv: vec2<f32>) -> f32 {
    let r = length(uv);
    var field = max(1.0 - abs(r - 0.93) / 0.03, 1.0 - abs(r - 0.64) / 0.018);
    let v = (r - 0.69) / 0.19;
    let turn = (atan2(uv.y, uv.x) / 6.28318 + 0.5) * 14.0;
    let f = fract(turn) - 0.5;
    if v > 0.0 && v < 1.0 && abs(f) < 0.38 {
        let sign = fract(sin(floor(turn) * 12.9898 + 1.7) * 43758.5453);
        field = max(field, 1.0 - abs(f) / 0.075);
        if sign > 0.3 {
            field = max(field, 1.0 - abs(v - 0.88) / 0.1);
        }
        if sign < 0.6 {
            field = max(field, 1.0 - abs(v - 0.14) / 0.1);
        }
        if sign > 0.55 {
            field = max(field, 1.0 - abs(v - 0.5 - f * 1.3) / 0.1);
        }
    }
    return field;
}

// A shape as a field: 0 on its outline and up to 1 inside, below 0 outside.
fn particle_field(uv: vec2<f32>, shape: f32) -> f32 {
    let index = u32(shape + 0.5);
    switch index {
        case 1u: { return 1.0 - abs(uv.x) - abs(uv.y); }
        case 2u: {
            // Round at the bottom, pointed at the top, and cooler the higher it goes.
            let t = clamp(uv.y * 0.5 + 0.5, 0.0, 1.0);
            let width = pow(1.0 - t, 0.8) * sqrt(clamp(t * 5.0, 0.0, 1.0));
            return (1.0 - abs(uv.x) / max(width, 1e-3)) * (1.0 - 0.6 * t);
        }
        case 3u: { return 1.0 - abs(length(uv) - 0.72) / 0.28; }
        case 4u: {
            let spread = sqrt(abs(uv.x)) + sqrt(abs(uv.y));
            return 1.0 - spread * spread;
        }
        case 5u: { return 1.0 - abs(uv.x); }
        case 6u: { return rune_field(uv); }
        case 7u: {
            // A sharp front on the outside, and a long tail dying away behind it.
            let r = length(uv);
            let tail = (r - 0.68) / 0.3;
            return min((1.0 - r) / 0.02, tail * abs(tail));
        }
        default: { return 1.0 - dot(uv, uv); }
    }
}

// What is left of a particle: xy the noise there, z how far inside the part that has not
// burned away it is, 0 on the outline. No derivatives, so it can run after a branch.
fn particle_burn(in: ParticleOut) -> vec3<f32> {
    var uv = in.uv;
    if in.texels.x > 0.0 {
        uv = (floor(uv * in.texels) + 0.5) / in.texels;
    }
    let drift = in.flow.xy * g.camera_pos.w + in.detail.y * vec2<f32>(7.3, 3.1);
    // A flame's noise runs in long strands, and it is held at its base: the tip lashes around.
    let flame = u32(in.shape + 0.5) == 2u;
    let strands = select(vec2<f32>(1.0), vec2<f32>(1.0, 0.35), flame);
    // A trail's runs along it, from one piece to the next.
    let place = select(uv * strands * 0.5, vec2<f32>(uv.x * 0.5, in.along), u32(in.shape + 0.5) == 5u);
    let noise = textureSampleLevel(noise_tex, noise_sampler, place * in.flow.z + drift, 0.0);
    let hold = select(1.0, clamp(uv.y * 0.5 + 0.6, 0.0, 1.0), flame);
    let bent = uv + (noise.rg - 0.5) * 2.0 * in.flow.w * hold;
    let bite = mix(1.0, noise.b * 2.0, in.detail.z);
    return vec3<f32>(noise.rg, particle_field(bent, in.shape) * bite - in.look.x);
}

// How far in front of the camera the scenery is at this pixel.
fn scene_view_depth(pixel: vec2<f32>) -> f32 {
    let size = vec2<f32>(textureDimensions(scene_depth));
    let depth = textureLoad(scene_depth, vec2<i32>(pixel), 0);
    let ndc = vec2<f32>(pixel.x / size.x * 2.0 - 1.0, 1.0 - pixel.y / size.y * 2.0);
    let offset = g.unproject * vec4<f32>(ndc, depth, 1.0);
    return dot(offset.xyz / offset.w, g.camera_forward.xyz);
}

// Fades a particle out as it reaches the scenery, 0 behind it.
fn soft_contact(in: ParticleOut) -> f32 {
    if in.look.z <= 0.0 {
        return 1.0;
    }
    return clamp((scene_view_depth(in.clip.xy) - in.view_depth) / in.look.z, 0.0, 1.0);
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
    if in.detail.w < 0.5 {
        return in.color * mask;
    }

    let burn = particle_burn(in);
    var heat = burn.z;
    let bands = in.detail.x;
    // With steps of color the outline is as hard as they are, as if painted. Without, a
    // soft shape fades all the way to its center, as the plain one does.
    var alpha = smoothstep(0.0, 0.2, heat);
    if in.shape < 0.5 {
        let inside = clamp(heat, 0.0, 1.0);
        alpha = inside * inside;
    }
    if bands > 0.0 {
        // A wave's tail keeps fading under its steps of color: it has no outline to paint.
        if u32(in.shape + 0.5) == 7u {
            alpha = smoothstep(0.0, 0.4, heat);
        } else {
            alpha = step(0.0, heat);
        }
        heat = ceil(heat * bands) / bands;
    }
    if alpha <= 0.0 {
        discard;
    }
    let t = clamp(heat, 0.0, 1.0);
    var color = mix(in.rim, in.body, clamp(t * 2.0, 0.0, 1.0));
    color = mix(color, in.heart, clamp(t * 2.0 - 1.0, 0.0, 1.0));
    return vec4<f32>(color + in.color.rgb, in.color.a) * alpha * soft_contact(in);
}

// Heat haze: shows what was drawn so far, displaced by the noise.
@fragment
fn fs_particle_bend(in: ParticleOut) -> @location(0) vec4<f32> {
    let burn = particle_burn(in);
    let strength = clamp(burn.z * 2.0, 0.0, 1.0) * soft_contact(in) * in.color.a;
    let size = vec2<f32>(textureDimensions(scene_copy));
    if strength <= 0.0 || scene_view_depth(in.clip.xy) < in.view_depth {
        discard;
    }
    let reach = in.look.w * size.y / max(in.view_depth, 0.5);
    let pixel = in.clip.xy + (burn.xy - 0.5) * 2.0 * reach * strength;
    let inside = clamp(vec2<i32>(pixel), vec2<i32>(0), vec2<i32>(size) - 1);
    return vec4<f32>(textureLoad(scene_copy, inside, 0).rgb, 1.0);
}
