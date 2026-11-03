//! Core types shared by every Grimora crate: time, input, transforms and camera.

pub mod actions;
pub mod animation;
mod bounds;
mod camera;
mod day_cycle;
pub mod environment;
mod input;
mod mesh;
pub mod particles;
pub mod pose;
mod rng;
mod time;
mod transform;

pub use actions::{ActionMap, Binding, PadAxis, PadButton};
pub use animation::{AnimationClip, Animator, ClipEvent, Skeleton};
pub use bounds::{BoundingSphere, Frustum};
pub use camera::Camera;
pub use day_cycle::{DayCycle, DayCycleDesc, DayKey};
pub use environment::{Environment, PointLight, SkyTextures};
pub use input::{Input, KeyCode, MouseButton};
pub use mesh::{AlphaMode, Material, MeshData, MeshLod, Shading, Submesh, TextureData, Vertex};
pub use particles::{ParticleDesc, ParticleEffect, ParticleInstance, ParticleShape, ParticleSystem};
pub use pose::{Control, Controls, POSE_FPS, PoseClip, PoseKey, PoseSampler, Rig, SolvedPose};
pub use rng::Rng;
pub use time::Time;
pub use transform::Transform;

pub use glam;
pub use glam::{EulerRot, Mat3, Mat4, Quat, Vec2, Vec3, Vec4};
