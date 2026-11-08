//! CPU mirrors of the shader uniform/instance layouts, and how to fill them.

use bytemuck::{Pod, Zeroable};
use grimora_core::environment::Shadows;
use grimora_core::environment::srgb_to_linear_vec;
use grimora_core::glam::camera::rh::{proj::directx, view};
use grimora_core::water::{HullMap, MAX_HULLS, MAX_WAVES, WaterHull, Wave};
use grimora_core::{Camera, Environment, Mat4, PointLight, Transform, Vec3, Vec4};

/// Must match `MAX_POINT_LIGHTS` in scene.wgsl.
pub const MAX_POINT_LIGHTS: usize = 32;
/// Must match `MAX_CASCADES` in scene.wgsl.
pub const MAX_CASCADES: usize = 4;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct GpuPointLight {
    position_radius: [f32; 4],
    color_intensity: [f32; 4],
    params: [f32; 4],
}

/// Mirrors `Globals` in scene.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Globals {
    view_proj: [[f32; 4]; 4],
    /// From clip space to the offset from the camera (see `unproject`).
    unproject: [[f32; 4]; 4],
    cascade_view_proj: [[[f32; 4]; 4]; MAX_CASCADES],
    /// View depth where each cascade ends.
    cascade_splits: [f32; 4],
    /// World size of one shadow texel in each cascade.
    cascade_texels: [f32; 4],
    camera_pos: [f32; 4],
    camera_forward: [f32; 4],
    sun_dir: [f32; 4],
    sun_color: [f32; 4],
    ambient_sky: [f32; 4],
    ambient_ground: [f32; 4],
    fog_color: [f32; 4],
    fog_params: [f32; 4],
    sky_zenith: [f32; 4],
    sky_horizon: [f32; 4],
    sky_ground: [f32; 4],
    sun_disc: [f32; 4],
    sky_sun: [f32; 4],
    sky_moon: [f32; 4],
    sky_glow: [f32; 4],
    sky_params: [f32; 4],
    cloud_dark: [f32; 4],
    cloud_light: [f32; 4],
    cloud_nebula: [f32; 4],
    surface: [f32; 4],
    mottle_dark: [f32; 4],
    mottle_light: [f32; 4],
    mist: [f32; 4],
    wind: [f32; 4],
    weather: [f32; 4],
    /// x level of the nearest water, y how far waves lean, z how high they are at most.
    sea: [f32; 4],
    /// Where that water is: min x, min z, max x, max z.
    water_area: [f32; 4],
    /// rgb deep color, w clarity in meters.
    water_deep: [f32; 4],
    /// rgb shallow color, w murk.
    water_shallow: [f32; 4],
    /// x camera's depth, y submerged, z caustics, w how far waves come up the beach.
    water_state: [f32; 4],
    /// The water bed's map: min x, min z, 1 / size x, 1 / size z; 0 size without one.
    bed_rect: [f32; 4],
    /// Where `WaterLevels` lies: min x, min z, cells per meter, and the sea's level past it.
    level_map: [f32; 4],
    /// x, z of the wave number, its angular speed and its height.
    sea_waves: [[f32; 4]; MAX_WAVES],
    /// What the origin adds to each wave's phases: along, bend, group.
    sea_phases: [[f32; 4]; MAX_WAVES],
    /// The world's origin wrapped by 10 km, by the stains' and by the mist's period.
    origin: [f32; 4],
    origin_mottle: [f32; 4],
    origin_mist: [f32; 4],
    hulls: [HullPlace; MAX_HULLS],
    /// Lights, shadows on, cascades, hollow hulls.
    counts: [u32; 4],
    point_lights: [GpuPointLight; MAX_POINT_LIGHTS],
}

/// Mirrors `PostParams` in post.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct PostParams {
    a: [f32; 4],
    b: [f32; 4],
    c: [f32; 4],
    d: [f32; 4],
}

/// Where in its gusts each plant is: drawn from its world position in `f64`, so a plant keeps
/// its sway wherever the origin is. `sway` in `scene.wgsl` reads it.
pub(crate) struct Sway {
    origin: grimora_core::DVec2,
    wind: grimora_core::DVec2,
}

impl Sway {
    pub(crate) fn of(env: &Environment) -> Self {
        let wind = env.wind.direction.normalize_or(grimora_core::Vec2::X).as_dvec2();
        let origin = env.origin.world();
        Self { origin: grimora_core::DVec2::new(origin.x, origin.z), wind }
    }

    pub(crate) fn phase(&self, position: Vec3) -> f32 {
        let at = self.origin + grimora_core::DVec2::new(f64::from(position.x), f64::from(position.z));
        (at.dot(self.wind) * 0.4 + at.x * 0.17).rem_euclid(std::f64::consts::TAU) as f32
    }
}

/// Per-instance data: model matrix, normal matrix and tint (+ emissive in `w`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct InstanceRaw {
    model: [[f32; 4]; 4],
    normal: [[f32; 4]; 3],
    tint: [f32; 4],
}

impl InstanceRaw {
    pub const ATTRIBUTES: [wgpu::VertexAttribute; 8] = wgpu::vertex_attr_array![
        3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4,
        7 => Float32x4, 8 => Float32x4, 9 => Float32x4,
        10 => Float32x4,
    ];

    /// `joints` is the instance's first matrix in the joint buffer, for skinned
    /// meshes; `wind` is the mesh's `MeshData::wind` and `phase` where its gusts are ([`Sway`]).
    pub fn new(transform: &Transform, color: Vec3, emissive: f32, joints: Option<u32>, wind: f32, phase: f32) -> Self {
        let n = transform.normal_matrix();
        // The normal matrix's spare w carries the joint offset (-1 = not skinned),
        // which saves a vertex attribute. f32 holds integers exactly up to 2^24.
        let joints = joints.map_or(-1.0, |j| j as f32);
        Self {
            model: transform.matrix().to_cols_array_2d(),
            // And the next ones the mesh's wind and its phase, for the vertex shaders to move it.
            normal: [n.x_axis.extend(joints).into(), n.y_axis.extend(wind).into(), n.z_axis.extend(phase).into()],
            tint: color.extend(emissive).into(),
        }
    }

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

// Field order of `grimora_core::Vertex`: position, normal, color, uv, tangent, joints, weights.
pub const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 7] = wgpu::vertex_attr_array![
    0 => Float32x3, 1 => Float32x3, 2 => Float32x3, 11 => Float32x2, 12 => Float32x4, 13 => Uint8x4, 14 => Unorm8x4,
];

pub fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: size_of::<grimora_core::Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &VERTEX_ATTRIBUTES,
    }
}

/// The sun's shadow maps for one frame, nearest the camera first.
#[derive(Debug, Clone, Copy)]
pub struct Cascades {
    pub count: usize,
    pub view_proj: [Mat4; MAX_CASCADES],
    /// View depth where each cascade ends.
    pub splits: [f32; MAX_CASCADES],
    /// World size of one shadow texel in each cascade.
    pub texels: [f32; MAX_CASCADES],
}

/// How far up-sun of a cascade's area shadow casters are still drawn, so tall
/// things outside the view still shade it under a low sun.
const CASTER_REACH: f32 = 100.0;

/// Where `count` cascades end between `near` and `far`: mostly logarithmic
/// (detail where it's needed, close to the camera), blended with even steps so
/// the first cascade isn't uselessly tiny.
fn cascade_splits(near: f32, far: f32, count: usize) -> [f32; MAX_CASCADES] {
    const LOG_WEIGHT: f32 = 0.75;
    let mut splits = [far; MAX_CASCADES];
    for (i, split) in splits.iter_mut().enumerate().take(count) {
        let t = (i + 1) as f32 / count as f32;
        let log = near * (far / near).powf(t);
        let even = near + (far - near) * t;
        *split = LOG_WEIGHT * log + (1.0 - LOG_WEIGHT) * even;
    }
    splits
}

/// Fits one shadow map per cascade around the part of the view from the
/// camera out to that cascade's split. Each fits a bounding sphere, whose
/// size depends only on the field of view, and is snapped to whole texels, so
/// shadow edges do not shimmer as the camera turns or moves.
pub fn shadow_cascades(camera: &Camera, aspect: f32, sun_dir: Vec3, shadows: &Shadows, map_size: u32) -> Cascades {
    let dir = sun_dir.normalize_or(Vec3::Y);
    let up = if dir.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
    let rotation = view::look_to_mat4(Vec3::ZERO, -dir, up);
    let count = (shadows.cascades as usize).clamp(1, MAX_CASCADES);
    let near = camera.near.max(0.01);
    let splits = cascade_splits(near, shadows.distance.max(near * 2.0), count);
    // Squared tangent from the view axis to the slice's corners.
    let k2 = (camera.fov_y * 0.5).tan().powi(2) * (1.0 + aspect * aspect);

    let mut cascades =
        Cascades { count, view_proj: [Mat4::IDENTITY; MAX_CASCADES], splits, texels: [0.0; MAX_CASCADES] };
    for (i, &end) in splits.iter().enumerate().take(count) {
        let start = near;
        // The sphere through the slice's near and far corners, centered on the view axis.
        let along = ((start + end) * 0.5 * (1.0 + k2)).min(end);
        let radius = ((end - along).powi(2) + end * end * k2).sqrt();
        let texel = 2.0 * radius / map_size as f32;

        let mut center = rotation.transform_point3(camera.position + camera.forward() * along);
        center.x = (center.x / texel).floor() * texel;
        center.y = (center.y / texel).floor() * texel;
        let center = rotation.inverse().transform_point3(center);

        let back = radius + CASTER_REACH;
        let view = view::look_to_mat4(center + dir * back, -dir, up);
        let proj = directx::orthographic(-radius, radius, -radius, radius, 0.0, back + radius);
        cascades.view_proj[i] = proj * view;
        cascades.texels[i] = texel;
    }
    cascades
}

/// Meters over which a light fades out as another one takes its place among the uploaded.
const LIGHT_SWAP_FADE: f32 = 8.0;

/// The lights to upload, each with how much of it to show: all of them in full, or when
/// there are more than the shader takes, those whose reach comes closest to `eye`. Those
/// nearly as far as the first one left out fade, so a light is at nothing by the time it
/// swaps places with that one, and none pops on or off as the camera moves.
fn nearest_lights(lights: &[PointLight], eye: Vec3) -> Vec<(&PointLight, f32)> {
    let mut chosen: Vec<&PointLight> = lights.iter().collect();
    if chosen.len() <= MAX_POINT_LIGHTS {
        return chosen.into_iter().map(|l| (l, 1.0)).collect();
    }
    let gap = |l: &PointLight| l.position.distance(eye) - l.radius;
    chosen.select_nth_unstable_by(MAX_POINT_LIGHTS, |a, b| gap(a).total_cmp(&gap(b)));
    let left_out = gap(chosen[MAX_POINT_LIGHTS]);
    chosen.truncate(MAX_POINT_LIGHTS);
    chosen.into_iter().map(|l| (l, ((left_out - gap(l)) / LIGHT_SWAP_FADE).clamp(0.0, 1.0))).collect()
}

fn lin(c: Vec3) -> Vec4 {
    srgb_to_linear_vec(c).extend(0.0)
}

/// Which pictures the sky has to draw, and whether its clouds are filtered or
/// show hard texels (`Renderer::set_smooth_textures`).
#[derive(Debug, Clone, Copy, Default)]
pub struct SkySprites {
    pub sun: bool,
    pub moon: bool,
    pub smooth: bool,
}

/// How bright the pictures of the sun and moon are: the sun's reaches the bloom.
/// How many times the ambient light a full flash of lightning adds.
const LIGHTNING_AMBIENT: f32 = 2.5;
const SUN_SPRITE_BRIGHTNESS: f32 = 1.8;
const MOON_SPRITE_BRIGHTNESS: f32 = 0.7;

/// The projection the scene is drawn with: the camera's, with the depth reversed
/// (see `SCENE_DEPTH_COMPARE`). Culling keeps using `Camera::view_projection`.
pub fn scene_projection(camera: &Camera, aspect: f32) -> Mat4 {
    directx::perspective(camera.fov_y, aspect.max(1e-4), camera.far, camera.near)
}

pub fn scene_view_projection(camera: &Camera, aspect: f32) -> Mat4 {
    scene_projection(camera, aspect) * camera.view()
}

/// Takes a point in clip space (x, y, reversed depth, 1) to where it is from the camera, in
/// world axes, once divided by its w. It leaves the camera's position out, and is worked out in
/// f64: the inverse of the whole view-projection, in f32, carries the camera's position through a
/// far plane kilometers away, and the sky, which comes from it, shook as the camera moved.
pub fn unproject(camera: &Camera, aspect: f32) -> Mat4 {
    let turn = view::look_to_mat4(Vec3::ZERO, camera.forward(), Vec3::Y).as_dmat4();
    (scene_projection(camera, aspect).as_dmat4() * turn).inverse().as_mat4()
}

#[allow(clippy::too_many_arguments)]
pub fn globals(
    camera: &Camera,
    aspect: f32,
    env: &Environment,
    lights: &[PointLight],
    time: f32,
    shadow_map_size: u32,
    max_cascades: u32,
    sprites: SkySprites,
    bed_rect: Vec4,
    level_map: Vec4,
    hulls: &[HullPlace],
) -> (Globals, Cascades) {
    let view_proj = scene_view_projection(camera, aspect);
    let sun_dir = env.sun.direction.normalize_or(Vec3::Y);
    let shadows = Shadows { cascades: env.shadows.cascades.min(max_cascades), ..env.shadows };
    let cascades = shadow_cascades(camera, aspect, sun_dir, &shadows, shadow_map_size);

    let mut point_lights = [GpuPointLight::zeroed(); MAX_POINT_LIGHTS];
    let count = lights.len().min(MAX_POINT_LIGHTS);
    for (gpu, (light, shown)) in point_lights.iter_mut().zip(nearest_lights(lights, camera.position)) {
        *gpu = GpuPointLight {
            position_radius: light.position.extend(light.radius.max(0.01)).into(),
            color_intensity: srgb_to_linear_vec(light.color).extend(light.intensity * shown).into(),
            params: [light.flicker, 0.0, 0.0, 0.0],
        };
    }

    let sun_color = srgb_to_linear_vec(env.sun.color) * env.sun.intensity;
    let sun_at = env.sky.sun_position.unwrap_or(sun_dir).normalize_or(Vec3::Y);
    let moon_at = env.sky.moon_position.unwrap_or(-sun_dir).normalize_or(Vec3::NEG_Y);
    let wind_dir = env.wind.direction.normalize_or(grimora_core::Vec2::X);
    let falling = &env.precipitation;
    // Lightning lights everything from everywhere.
    let flash = falling.flash.max(0.0);
    let ambient = env.ambient.intensity * (1.0 + flash * LIGHTNING_AMBIENT);
    let clouds = &env.sky.clouds;
    let surface = &env.surface;
    // A stain with less of its color keeps how much it lightens or darkens.
    let stain =
        |color: Vec3| Vec3::splat(color.dot(Vec3::new(0.3, 0.5, 0.2))).lerp(color, surface.stain_color.clamp(0.0, 1.0));
    let water = &env.water;
    let wave = |w: &Wave| [w.k.x, w.k.y, w.omega, w.height];
    let tallest: f32 = water.waves.iter().map(|w| w.height).sum();
    // An empty area, with nothing in it, when there is no water.
    let area = if water.active { [water.min.x, water.min.y, water.max.x, water.max.y] } else { [1.0, 1.0, 0.0, 0.0] };
    let globals = Globals {
        view_proj: view_proj.to_cols_array_2d(),
        unproject: unproject(camera, aspect).to_cols_array_2d(),
        cascade_view_proj: cascades.view_proj.map(|m| m.to_cols_array_2d()),
        cascade_splits: cascades.splits,
        cascade_texels: cascades.texels,
        camera_pos: camera.position.extend(time).into(),
        camera_forward: camera.forward().extend(0.0).into(),
        sun_dir: sun_dir.extend(1.0 / shadow_map_size as f32).into(),
        sun_color: sun_color.extend(env.shadows.strength.clamp(0.0, 1.0)).into(),
        ambient_sky: (lin(env.ambient.sky) * ambient).into(),
        ambient_ground: (lin(env.ambient.ground) * ambient).into(),
        fog_color: (srgb_to_linear_vec(env.fog.color) * (1.0 + flash)).extend(env.fog.density.max(0.0)).into(),
        fog_params: [
            env.fog.height_falloff.max(0.0),
            env.fog.base_height,
            env.fog.sun_scatter,
            env.fog.mist.brightness,
        ],
        sky_zenith: lin(env.sky.zenith).into(),
        sky_horizon: lin(env.sky.horizon).into(),
        sky_ground: lin(env.sky.ground).into(),
        sun_disc: [env.sky.sun_size.to_radians().cos(), env.sky.sun_glow, clouds.puff.clamp(0.0, 1.0), flash],
        // The sprites' half width as a tangent; 0 means there is no picture to draw.
        sky_sun: sun_at.extend(if sprites.sun { env.sky.sun_sprite_size.to_radians().tan() } else { 0.0 }).into(),
        sky_moon: moon_at.extend(if sprites.moon { env.sky.moon_sprite_size.to_radians().tan() } else { 0.0 }).into(),
        sky_glow: env
            .sky
            .light_position
            .unwrap_or(sun_dir)
            .normalize_or(Vec3::Y)
            .extend(if sprites.smooth { 1.0 } else { 0.0 })
            .into(),
        sky_params: [
            env.sky.stars.clamp(0.0, 1.0),
            SUN_SPRITE_BRIGHTNESS,
            MOON_SPRITE_BRIGHTNESS,
            env.sky.halo.max(0.0),
        ],
        cloud_dark: lin(clouds.dark).with_w(clouds.amount.clamp(0.0, 1.0)).into(),
        cloud_light: lin(clouds.light).with_w(clouds.scale.max(0.01)).into(),
        cloud_nebula: lin(clouds.nebula).with_w(clouds.speed).into(),
        surface: [
            surface.contrast.max(0.0),
            surface.saturation.max(0.0),
            surface.mottle.clamp(0.0, 1.0),
            surface.mottle_size.max(0.01),
        ],
        mottle_dark: stain(surface.mottle_dark).extend(0.0).into(),
        mottle_light: stain(surface.mottle_light).extend(-surface.crisp.max(0.0)).into(),
        mist: [
            (env.fog.mist.amount * env.fog.mist.strength).max(0.0),
            env.fog.mist.top,
            env.fog.mist.size.max(0.1),
            env.fog.mist.speed,
        ],
        wind: [wind_dir.x, wind_dir.y, (env.wind.strength * env.wind.scale).max(0.0), env.wind.speed],
        weather: [
            falling.wet.clamp(0.0, 1.0),
            falling.snow_cover.clamp(0.0, 1.0),
            falling.rain.clamp(0.0, 1.0),
            env.wind.scale.max(0.0),
        ],
        sea: [water.level, water.lean, tallest, 0.0],
        water_area: area,
        water_deep: srgb_to_linear_vec(water.look.deep).extend(water.look.clarity.max(0.01)).into(),
        water_shallow: srgb_to_linear_vec(water.look.shallow).extend(water.look.murk).into(),
        water_state: [water.camera_depth, water.submerged, water.look.caustics.max(0.0), water.reach],
        bed_rect: bed_rect.to_array(),
        level_map: level_map.to_array(),
        sea_waves: water.waves.each_ref().map(wave),
        sea_phases: water.waves.each_ref().map(|w| {
            let p = w.phase(env.origin);
            [p.along, p.bend, p.group, 0.0]
        }),
        origin: env.origin.wrapped().extend(0.0).into(),
        // The stains' noise repeats every `4 * size` meters, and 1000 of those line up with all of
        // its frequencies; the mist's every `4 * size`, and 10 of those with its two.
        origin_mottle: env.origin.wrapped_by(f64::from(surface.mottle_size.max(0.01)) * 4000.0).extend(0.0).into(),
        origin_mist: env.origin.wrapped_by(f64::from(env.fog.mist.size.max(0.1)) * 40.0).extend(0.0).into(),
        hulls: {
            let mut places = [HullPlace::zeroed(); MAX_HULLS];
            let count = hulls.len().min(MAX_HULLS);
            places[..count].copy_from_slice(&hulls[..count]);
            places
        },
        counts: [
            count as u32,
            u32::from(env.shadows.enabled),
            cascades.count as u32,
            hulls.len().min(MAX_HULLS) as u32,
        ],
        point_lights,
    };
    (globals, cascades)
}

pub fn post_params(env: &Environment, time: f32, bloom_mips: u32) -> PostParams {
    let p = &env.post;
    PostParams {
        a: [p.exposure, p.contrast, p.saturation, p.vignette],
        b: [p.grain, p.bloom.clamp(0.0, 1.0), time, bloom_mips.max(1) as f32],
        c: [p.shadow_tint.x, p.shadow_tint.y, p.shadow_tint.z, p.vivid.clamp(0.0, 1.0)],
        d: [p.sharpen.max(0.0), env.water.submerged.clamp(0.0, 1.0), 0.0, 0.0],
    }
}

// Uniform buffers need 16-byte multiples; instance stride must match the shader.
const _: () = assert!(size_of::<Globals>().is_multiple_of(16));
const _: () = assert!(size_of::<PostParams>() == 64);
const _: () = assert!(size_of::<InstanceRaw>() == 128);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cascades_grow_and_cover_the_view() {
        let shadows = Shadows { distance: 120.0, cascades: 4, ..Default::default() };
        let mut camera = Camera::new(Vec3::new(3.0, 2.0, 5.0), 0.7, -0.2);
        let c = shadow_cascades(&camera, 16.0 / 9.0, Vec3::new(-0.6, 0.3, -0.5), &shadows, 2048);
        assert_eq!(c.count, 4);
        assert!(c.splits.windows(2).all(|w| w[0] < w[1]) && c.splits[3] == 120.0, "{:?}", c.splits);
        assert!(c.splits[0] > 3.0 && c.splits[0] < 15.0, "{:?}", c.splits);
        assert!(c.texels.windows(2).all(|w| w[0] < w[1]), "{:?}", c.texels);
        // Every point of the view up to a cascade's split lands inside its map.
        for i in 0..4 {
            let frustum = grimora_core::Frustum::from_view_projection(c.view_proj[i]);
            for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0), (0.0, 0.0)] {
                let corner = camera.position
                    + (camera.forward()
                        + camera.right() * x * 0.577 * 16.0 / 9.0
                        + camera.forward().cross(camera.right()).normalize() * -y * 0.577)
                        * c.splits[i];
                let sphere = grimora_core::BoundingSphere { center: corner, radius: 0.0 };
                assert!(frustum.intersects_sphere(sphere), "cascade {i} misses {corner}");
            }
        }
        // Turning in place only moves the maps by whole texels: same size, no shimmer.
        camera.yaw += 0.3;
        let turned = shadow_cascades(&camera, 16.0 / 9.0, Vec3::new(-0.6, 0.3, -0.5), &shadows, 2048);
        assert_eq!(c.texels, turned.texels);
    }

    #[test]
    fn keeps_the_lights_nearest_the_eye() {
        let lights: Vec<PointLight> = (0..40)
            .map(|i| PointLight { position: Vec3::new(i as f32 * 10.0, 0.0, 0.0), radius: 5.0, ..Default::default() })
            .collect();
        assert_eq!(nearest_lights(&lights[..10], Vec3::ZERO).len(), 10);
        let chosen = nearest_lights(&lights, Vec3::new(400.0, 0.0, 0.0));
        assert_eq!(chosen.len(), MAX_POINT_LIGHTS);
        // Standing past the far end, the 8 lights nearest the origin are the ones left out.
        assert!(chosen.iter().all(|(l, _)| l.position.x >= 80.0));
    }

    /// The sky's direction at a point of the screen, as `fs_sky` works it out in f32.
    fn sky_direction(camera: &Camera, ndc: (f32, f32)) -> Vec3 {
        let p = unproject(camera, 16.0 / 9.0) * Vec4::new(ndc.0, ndc.1, 1.0, 1.0);
        p.truncate().normalize()
    }

    #[test]
    fn the_sky_does_not_shake_as_the_camera_moves() {
        // Far from the origin, just under where it shifts, with a far plane kilometers away:
        // only the turn of the camera may move the sky, never where it stands.
        let mut camera = Camera::new(Vec3::new(1990.0, 86.0, -1730.0), 0.7, 0.05);
        camera.far = 14000.0;
        let corners = [(0.0, 0.0), (-1.0, -1.0), (1.0, 0.3), (0.2, 1.0)];
        let still: Vec<Vec3> = corners.iter().map(|&c| sky_direction(&camera, c)).collect();
        for step in 1..200 {
            // A follow camera's small steps, the way it trails a walking character.
            let mut moved = camera;
            moved.position += Vec3::new(0.013, 0.004, -0.009) * step as f32;
            for (&c, a) in corners.iter().zip(&still) {
                let b = sky_direction(&moved, c);
                assert!(a.angle_between(b) < 1e-6, "the sky moved {} rad at {c:?}", a.angle_between(b));
            }
        }
        // And the direction is the one through that point of the screen.
        let middle = sky_direction(&camera, (0.0, 0.0));
        assert!(middle.angle_between(camera.forward()) < 1e-5);
    }

    #[test]
    fn depth_is_read_back_exactly_far_away() {
        // What `behind_water` does: a point 3 km away, through the reversed depth, back to the world.
        let mut camera = Camera::new(Vec3::new(1990.0, 86.0, -1730.0), 0.4, -0.1);
        camera.far = 14000.0;
        let aspect = 16.0 / 9.0;
        let point = camera.position + (camera.forward() + camera.right() * 0.2) * 3000.0;
        let clip = scene_view_projection(&camera, aspect) * point.extend(1.0);
        let ndc = clip / clip.w;
        assert!(ndc.z > 0.0 && ndc.z < 1e-3, "far away is near 0: {}", ndc.z);
        let offset = unproject(&camera, aspect) * Vec4::new(ndc.x, ndc.y, ndc.z, 1.0);
        let back = camera.position + offset.truncate() / offset.w;
        assert!(back.distance(point) < 0.5, "{back} is not {point}");
    }

    /// How bright each light is uploaded, by where it is.
    fn shown_lights(lights: &[PointLight], eye: Vec3) -> Vec<(i32, f32)> {
        let mut shown: Vec<(i32, f32)> =
            nearest_lights(lights, eye).into_iter().map(|(l, s)| (l.position.x as i32, s * l.intensity)).collect();
        shown.sort_by_key(|&(x, _)| x);
        shown
    }

    #[test]
    fn lights_fade_as_they_swap_instead_of_popping() {
        let lights: Vec<PointLight> = (0..48)
            .map(|i| PointLight {
                position: Vec3::new(i as f32 * 3.0, 0.0, 0.0),
                radius: 6.0,
                intensity: 2.0,
                ..Default::default()
            })
            .collect();
        // Walking along the row, no light's brightness jumps between two steps of 2 cm.
        let mut before = shown_lights(&lights, Vec3::ZERO);
        for step in 1..=3000 {
            let eye = Vec3::new(step as f32 * 0.02, 1.0, 0.0);
            let now = shown_lights(&lights, eye);
            let brightness = |set: &[(i32, f32)], x: i32| set.iter().find(|l| l.0 == x).map_or(0.0, |l| l.1);
            for light in &lights {
                let x = light.position.x as i32;
                let (a, b) = (brightness(&before, x), brightness(&now, x));
                assert!((a - b).abs() < 0.02, "the light at {x} went from {a} to {b} at {eye}");
            }
            before = now;
        }
        // In the middle of the row, the nearest are still in full.
        let middle = shown_lights(&lights, Vec3::new(72.0, 1.0, 0.0));
        assert!(middle.iter().any(|&(x, s)| x == 72 && s == 2.0));
    }
}

/// Where a hollow hull is, and which layer of the maps it uses. Mirrors `Hull` in scene.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(crate) struct HullPlace {
    x: [f32; 4],
    y: [f32; 4],
    z: [f32; 4],
    rect: [f32; 4],
    info: [f32; 4],
}

impl HullPlace {
    pub(crate) fn new(hull: &WaterHull, layer: usize) -> Self {
        let map: &HullMap = &hull.map;
        let to_local = Mat4::from_quat(hull.rotation.inverse()) * Mat4::from_translation(-hull.position);
        let row = |i: usize| to_local.row(i).to_array();
        let size = (map.max - map.min).max(glam::Vec2::splat(1e-5));
        Self {
            x: row(0),
            y: row(1),
            z: row(2),
            rect: [map.min.x, map.min.y, 1.0 / size.x, 1.0 / size.y],
            info: [layer as f32, map.deck, 0.0, 0.0],
        }
    }
}
