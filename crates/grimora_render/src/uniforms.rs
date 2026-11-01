//! CPU mirrors of the shader uniform/instance layouts, and how to fill them.

use bytemuck::{Pod, Zeroable};
use grimora_core::environment::srgb_to_linear_vec;
use grimora_core::glam::camera::rh::{proj::directx, view};
use grimora_core::{Camera, Environment, Mat4, PointLight, Transform, Vec3, Vec4};

/// Must match `MAX_POINT_LIGHTS` in scene.wgsl.
pub const MAX_POINT_LIGHTS: usize = 32;

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
    light_view_proj: [[f32; 4]; 4],
    camera_pos: [f32; 4],
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

    pub fn new(transform: &Transform, color: Vec3, emissive: f32) -> Self {
        let n = transform.normal_matrix();
        Self {
            model: transform.matrix().to_cols_array_2d(),
            normal: [n.x_axis.extend(0.0).into(), n.y_axis.extend(0.0).into(), n.z_axis.extend(0.0).into()],
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

pub const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3];

pub fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: size_of::<grimora_core::Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &VERTEX_ATTRIBUTES,
    }
}

/// Light-space matrix of a shadow map that covers `distance` units around the
/// camera. The center is snapped to whole shadow texels so shadow edges do not
/// shimmer as the camera moves. Returns the matrix and one texel's world size.
pub fn sun_view_proj(camera: &Camera, sun_dir: Vec3, distance: f32, map_size: u32) -> (Mat4, f32) {
    let dir = sun_dir.normalize_or(Vec3::Y);
    let up = if dir.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
    let texel = 2.0 * distance / map_size as f32;

    let rotation = view::look_to_mat4(Vec3::ZERO, -dir, up);
    let mut center = rotation.transform_point3(camera.position + camera.forward() * distance * 0.5);
    center.x = (center.x / texel).floor() * texel;
    center.y = (center.y / texel).floor() * texel;
    let center = rotation.inverse().transform_point3(center);

    let depth = distance * 4.0;
    let view = view::look_to_mat4(center + dir * depth * 0.5, -dir, up);
    let proj = directx::orthographic(-distance, distance, -distance, distance, 0.0, depth);
    (proj * view, texel)
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
) -> (Globals, Mat4) {
    let view_proj = camera.view_projection(aspect);
    let sun_dir = env.sun.direction.normalize_or(Vec3::Y);
    let (light_view_proj, world_texel) = sun_view_proj(camera, sun_dir, env.shadows.distance.max(1.0), shadow_map_size);

    let mut point_lights = [GpuPointLight::zeroed(); MAX_POINT_LIGHTS];
    let count = lights.len().min(MAX_POINT_LIGHTS);
    if lights.len() > MAX_POINT_LIGHTS {
        log::warn!("{} point lights in view, only the first {MAX_POINT_LIGHTS} are used", lights.len());
    }
    for (gpu, light) in point_lights.iter_mut().zip(lights) {
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
        light_view_proj: light_view_proj.to_cols_array_2d(),
        camera_pos: camera.position.extend(time).into(),
        sun_dir: sun_dir.extend(1.0 / shadow_map_size as f32).into(),
        sun_color: sun_color.extend(env.shadows.strength.clamp(0.0, 1.0)).into(),
        ambient_sky: (lin(env.ambient.sky) * ambient).into(),
        ambient_ground: (lin(env.ambient.ground) * ambient).into(),
        fog_color: srgb_to_linear_vec(env.fog.color).extend(env.fog.density.max(0.0)).into(),
        fog_params: [env.fog.height_falloff.max(0.0), env.fog.base_height, env.fog.sun_scatter, world_texel],
        sky_zenith: lin(env.sky.zenith).into(),
        sky_horizon: lin(env.sky.horizon).into(),
        sky_ground: lin(env.sky.ground).into(),
        sun_disc: [env.sky.sun_size.to_radians().cos(), env.sky.sun_glow, 0.0, 0.0],
        counts: [count as u32, u32::from(env.shadows.enabled), 0, 0],
        point_lights,
    };
    (globals, light_view_proj)
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
