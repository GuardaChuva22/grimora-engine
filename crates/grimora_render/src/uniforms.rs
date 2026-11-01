//! CPU mirrors of the shader uniform/instance layouts, and how to fill them.

use bytemuck::{Pod, Zeroable};
use grimora_core::environment::Shadows;
use grimora_core::environment::srgb_to_linear_vec;
use grimora_core::glam::camera::rh::{proj::directx, view};
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
    inv_view_proj: [[f32; 4]; 4],
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
    counts: [u32; 4],
    point_lights: [GpuPointLight; MAX_POINT_LIGHTS],
}

/// Mirrors `PostParams` in post.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct PostParams {
    a: [f32; 4],
    b: [f32; 4],
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

    /// `joints` is the instance's first matrix in the joint buffer, for skinned meshes.
    pub fn new(transform: &Transform, color: Vec3, emissive: f32, joints: Option<u32>) -> Self {
        let n = transform.normal_matrix();
        // The normal matrix's spare w carries the joint offset (-1 = not skinned),
        // which saves a vertex attribute. f32 holds integers exactly up to 2^24.
        let joints = joints.map_or(-1.0, |j| j as f32);
        Self {
            model: transform.matrix().to_cols_array_2d(),
            normal: [n.x_axis.extend(joints).into(), n.y_axis.extend(0.0).into(), n.z_axis.extend(0.0).into()],
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

/// The lights to upload: all of them, or when there are more than the shader
/// takes, those whose reach comes closest to `eye`.
fn nearest_lights(lights: &[PointLight], eye: Vec3) -> Vec<&PointLight> {
    let mut chosen: Vec<&PointLight> = lights.iter().collect();
    if chosen.len() > MAX_POINT_LIGHTS {
        let gap = |l: &PointLight| l.position.distance(eye) - l.radius;
        chosen.select_nth_unstable_by(MAX_POINT_LIGHTS - 1, |a, b| gap(a).total_cmp(&gap(b)));
        chosen.truncate(MAX_POINT_LIGHTS);
    }
    chosen
}

fn lin(c: Vec3) -> Vec4 {
    srgb_to_linear_vec(c).extend(0.0)
}

pub fn globals(
    camera: &Camera,
    aspect: f32,
    env: &Environment,
    lights: &[PointLight],
    time: f32,
    shadow_map_size: u32,
) -> (Globals, Cascades) {
    let view_proj = camera.view_projection(aspect);
    let sun_dir = env.sun.direction.normalize_or(Vec3::Y);
    let cascades = shadow_cascades(camera, aspect, sun_dir, &env.shadows, shadow_map_size);

    let mut point_lights = [GpuPointLight::zeroed(); MAX_POINT_LIGHTS];
    let count = lights.len().min(MAX_POINT_LIGHTS);
    for (gpu, light) in point_lights.iter_mut().zip(nearest_lights(lights, camera.position)) {
        *gpu = GpuPointLight {
            position_radius: light.position.extend(light.radius.max(0.01)).into(),
            color_intensity: srgb_to_linear_vec(light.color).extend(light.intensity).into(),
            params: [light.flicker, 0.0, 0.0, 0.0],
        };
    }

    let sun_color = srgb_to_linear_vec(env.sun.color) * env.sun.intensity;
    let ambient = env.ambient.intensity;
    let globals = Globals {
        view_proj: view_proj.to_cols_array_2d(),
        inv_view_proj: view_proj.inverse().to_cols_array_2d(),
        cascade_view_proj: cascades.view_proj.map(|m| m.to_cols_array_2d()),
        cascade_splits: cascades.splits,
        cascade_texels: cascades.texels,
        camera_pos: camera.position.extend(time).into(),
        camera_forward: camera.forward().extend(0.0).into(),
        sun_dir: sun_dir.extend(1.0 / shadow_map_size as f32).into(),
        sun_color: sun_color.extend(env.shadows.strength.clamp(0.0, 1.0)).into(),
        ambient_sky: (lin(env.ambient.sky) * ambient).into(),
        ambient_ground: (lin(env.ambient.ground) * ambient).into(),
        fog_color: srgb_to_linear_vec(env.fog.color).extend(env.fog.density.max(0.0)).into(),
        fog_params: [env.fog.height_falloff.max(0.0), env.fog.base_height, env.fog.sun_scatter, 0.0],
        sky_zenith: lin(env.sky.zenith).into(),
        sky_horizon: lin(env.sky.horizon).into(),
        sky_ground: lin(env.sky.ground).into(),
        sun_disc: [env.sky.sun_size.to_radians().cos(), env.sky.sun_glow, 0.0, 0.0],
        counts: [count as u32, u32::from(env.shadows.enabled), cascades.count as u32, 0],
        point_lights,
    };
    (globals, cascades)
}

pub fn post_params(env: &Environment, time: f32, bloom_mips: u32) -> PostParams {
    let p = &env.post;
    PostParams {
        a: [p.exposure, p.contrast, p.saturation, p.vignette],
        b: [p.grain, p.bloom.clamp(0.0, 1.0), time, bloom_mips.max(1) as f32],
    }
}

// Uniform buffers need 16-byte multiples; instance stride must match the shader.
const _: () = assert!(size_of::<Globals>().is_multiple_of(16));
const _: () = assert!(size_of::<PostParams>() == 32);
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
        assert!(chosen.iter().all(|l| l.position.x >= 80.0));
    }
}
