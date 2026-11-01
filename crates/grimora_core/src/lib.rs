//! Core types shared by every Grimora crate: time, input, transforms and camera.

pub mod animation;
mod bounds;
mod camera;
pub mod environment;
mod input;
mod mesh;
pub mod particles;
mod rng;
mod time;
mod transform;

pub use animation::{AnimationClip, Animator, Skeleton};
pub use bounds::{BoundingSphere, Frustum};
pub use camera::Camera;
pub use environment::{Environment, PointLight};
pub use input::{Input, KeyCode, MouseButton};
pub use mesh::{AlphaMode, Material, MeshData, MeshLod, Submesh, TextureData, Vertex};
pub use particles::{ParticleDesc, ParticleEffect, ParticleInstance, ParticleShape, ParticleSystem};
pub use rng::Rng;
pub use time::Time;
pub use transform::Transform;

pub use glam;
pub use glam::{EulerRot, Mat3, Mat4, Quat, Vec2, Vec3, Vec4};
