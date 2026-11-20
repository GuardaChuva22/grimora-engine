//! Core types shared by every Grimora crate: time, input, transforms and camera.

pub mod actions;
pub mod animation;
mod bounds;
mod camera;
mod day_cycle;
pub mod environment;
mod input;
mod mesh;
pub mod origin;
pub mod particles;
pub mod pose;
mod rng;
mod time;
mod transform;
pub mod water;
pub mod wind;

pub use actions::{ActionMap, Binding, PadAxis, PadButton};
pub use animation::{AnimationClip, Animator, ClipEvent, FREE_HAND_CLIP, JointTransform, Skeleton};
pub use bounds::{BoundingSphere, Frustum};
pub use camera::Camera;
pub use day_cycle::{DayCycle, DayCycleDesc, DayKey, Weather};
pub use environment::{Environment, PointLight, Precipitation, RoomBox, SkyTextures};
pub use input::{Input, KeyCode, MouseButton};
pub use mesh::{
    AlphaMode, AuraLook, AuraPattern, CollisionMesh, Material, MeshData, MeshLod, Morph, Shading, Submesh, TextureData,
    Vertex,
};
pub use origin::{Origin, WorldPos, WorldTransform};
pub use particles::{
    BLADE_RIBBON, BladeTrail, BladeTrailDesc, ParticleDesc, ParticleEffect, ParticleInstance, ParticleLight,
    ParticleShape, ParticleSystem, RainSplashes, Spawn, WIND_SPEED, WeatherParticles,
};
pub use pose::{
    Control, Controls, POSE_FPS, PoseClip, PoseKey, PoseSampler, Rig, SECOND_GRIP_CLIP, SolvedPose, TWO_HANDS_APART,
};
pub use rng::Rng;
pub use time::Time;
pub use transform::Transform;
pub use water::{
    HullMap, RiverPoint, Sea, SeaDesc, WaterBed, WaterBody, WaterFrame, WaterHull, WaterLevels, WaterLook, WaterPoint,
    WaterShape, Waters,
};
pub use wind::{WindSim, WindState};

pub use glam;
pub use glam::{DVec2, DVec3, EulerRot, Mat3, Mat4, Quat, Vec2, Vec3, Vec4};
