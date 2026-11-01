use glam::{EulerRot, Mat3, Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};

/// Position, rotation and scale of an object in world space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Transform {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Transform {
    pub const IDENTITY: Self = Self { position: Vec3::ZERO, rotation: Quat::IDENTITY, scale: Vec3::ONE };

    pub fn from_position(position: Vec3) -> Self {
        Self { position, ..Self::IDENTITY }
    }

    /// Builds a transform from Euler angles in degrees (yaw around Y first).
    pub fn from_euler_degrees(position: Vec3, degrees: Vec3, scale: Vec3) -> Self {
        let r = degrees * (std::f32::consts::PI / 180.0);
        Self { position, rotation: Quat::from_euler(EulerRot::YXZ, r.y, r.x, r.z), scale }
    }

    pub fn with_scale(mut self, scale: Vec3) -> Self {
        self.scale = scale;
        self
    }

    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.position)
    }

    /// Inverse-transpose of the upper 3x3, used to transform normals correctly
    /// under non-uniform scale.
    pub fn normal_matrix(&self) -> Mat3 {
        Mat3::from_mat4(self.matrix()).inverse().transpose()
    }

    pub fn forward(&self) -> Vec3 {
        self.rotation * Vec3::NEG_Z
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}
