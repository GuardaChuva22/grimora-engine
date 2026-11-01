//! # Grimora Engine
//!
//! A light Rust 3D engine for stylized low-poly worlds.
//!
//! ```no_run
//! use grimora::prelude::*;
//!
//! struct MyGame { fly: FlyCamera }
//!
//! impl Game for MyGame {
//!     fn update(&mut self, ctx: &mut Context) {
//!         let dt = ctx.time.delta();
//!         let looking = self.fly.update(&mut ctx.camera, &ctx.input, dt);
//!         ctx.set_cursor_grab(looking);
//!     }
//! }
//!
//! fn main() -> anyhow::Result<()> {
//!     let config = EngineConfig { scene: Some("assets/scenes/sandbox.ron".into()), ..Default::default() };
//!     grimora::run(config.with_cli()?, MyGame { fly: FlyCamera::default() })
//! }
//! ```

mod config;
mod fly_camera;
mod runner;
mod third_person;
mod world;

pub use config::{EngineConfig, USAGE};
pub use fly_camera::FlyCamera;
pub use runner::{Context, run};
pub use third_person::ThirdPersonController;
pub use world::{MeshId, Object, World};

pub use grimora_audio as audio;
pub use grimora_core as core;
pub use grimora_physics as physics;
pub use grimora_render as render;
pub use grimora_scene as scene;

/// Your game. The engine calls these from its main loop.
pub trait Game: 'static {
    /// Once, after the window, renderer and initial scene are ready.
    fn init(&mut self, _ctx: &mut Context) {}

    /// After the scene file is loaded or hot-reloaded. Objects were replaced,
    /// so re-find anything you keep by name here.
    fn scene_loaded(&mut self, _ctx: &mut Context) {}

    /// Every frame, before rendering.
    fn update(&mut self, ctx: &mut Context);
}

pub mod prelude {
    pub use crate::{Context, EngineConfig, FlyCamera, Game, MeshId, Object, ThirdPersonController, World};
    pub use grimora_audio::{Audio, Category, Emitter, SoundId};
    pub use grimora_core::{
        Animator, Camera, Environment, EulerRot, Input, KeyCode, Mat4, MeshData, MouseButton, PointLight, Quat, Time,
        Transform, Vec2, Vec3, Vec4,
    };
    pub use grimora_physics::{BodyHandle, CharacterBody, Physics, RayHit};
    pub use grimora_scene::{BodyDesc, MeshDesc};
}
