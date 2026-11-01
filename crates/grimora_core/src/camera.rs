use glam::camera::rh::{proj::directx, view};
use glam::{Mat4, Vec3};

/// A perspective camera described by position and yaw/pitch (radians).
///
/// Yaw 0 looks down -Z; positive pitch looks up. Right-handed, Y up.
#[derive(Debug, Clone, Copy)]
pub struct Camera {
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    /// Vertical field of view in radians.
    pub fov_y: f32,
    pub near: f32,
    pub far: f32,
}

impl Camera {
    pub const MAX_PITCH: f32 = 1.55;

    pub fn new(position: Vec3, yaw: f32, pitch: f32) -> Self {
        Self { position, yaw, pitch, fov_y: 60f32.to_radians(), near: 0.1, far: 1000.0 }
    }

    pub fn forward(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        Vec3::new(-sy * cp, sp, -cy * cp)
    }

    pub fn right(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        Vec3::new(cy, 0.0, -sy)
    }

    /// Points the camera at `target` from its current position.
    pub fn look_at(&mut self, target: Vec3) {
        let d = (target - self.position).normalize_or(Vec3::NEG_Z);
        self.pitch = d.y.asin().clamp(-Self::MAX_PITCH, Self::MAX_PITCH);
        self.yaw = (-d.x).atan2(-d.z);
    }

    pub fn view(&self) -> Mat4 {
        view::look_to_mat4(self.position, self.forward(), Vec3::Y)
    }

    pub fn projection(&self, aspect: f32) -> Mat4 {
        // DirectX conventions match wgpu: NDC y up, depth in 0..1.
        directx::perspective(self.fov_y, aspect.max(1e-4), self.near, self.far)
    }

    pub fn view_projection(&self, aspect: f32) -> Mat4 {
        self.projection(aspect) * self.view()
    }
}

impl Default for Camera {
    fn default() -> Self {
        Self::new(Vec3::new(0.0, 2.0, 8.0), 0.0, 0.0)
    }
}
