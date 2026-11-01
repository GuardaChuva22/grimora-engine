//! Core types shared by every Grimora crate: time, input, transforms and camera.

mod camera;
pub mod environment;
mod input;
mod mesh;
mod time;
mod transform;

pub use camera::Camera;
pub use environment::{Environment, PointLight};
pub use input::{Input, KeyCode, MouseButton};
pub use mesh::{Material, MeshData, Submesh, TextureData, Vertex};
pub use time::Time;
pub use transform::Transform;

pub use glam;
pub use glam::{EulerRot, Mat3, Mat4, Quat, Vec2, Vec3, Vec4};
