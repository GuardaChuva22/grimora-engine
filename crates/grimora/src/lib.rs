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

mod combat;
mod config;
mod fly_camera;
mod gamepad;
mod interaction;
mod lock_on;
mod npc;
mod options;
mod runner;
pub mod save;
mod streaming;
mod third_person;
mod world;

pub use combat::{CombatEvent, Fighter, Guard, Health, Stamina};
pub use config::{EngineConfig, USAGE};
pub use fly_camera::FlyCamera;
pub use interaction::{Interaction, TriggerEvent, TriggerState};
pub use npc::{Agent, AgentStatus, Brain, NpcEvent};
pub use options::{Options, RESOLUTIONS, ShadowQuality, UI_SCALES};
pub use runner::{Context, run};
pub use save::SaveStore;
pub use streaming::StreamingStats;
pub use third_person::{CharacterState, ThirdPersonController};
pub use world::{Attached, Body, Collider, Entity, Light, MeshId, Name, Renderable, SceneIndex, World};

pub use grimora_audio as audio;
pub use grimora_core as core;
pub use grimora_physics as physics;
pub use grimora_render as render;
pub use grimora_scene as scene;
/// The entity storage under [`World`]: `world.ecs` is a [`hecs::World`].
pub use hecs;

/// Your game. The engine calls these from its main loop.
pub trait Game: 'static {
    /// Once, after the window, renderer and initial scene are ready.
    fn init(&mut self, _ctx: &mut Context) {}

    /// After the scene file is loaded or hot-reloaded. Every entity was
    /// replaced, so re-find or re-spawn anything you keep here.
    fn scene_loaded(&mut self, _ctx: &mut Context) {}

    /// Every frame, before rendering.
    fn update(&mut self, ctx: &mut Context);
}

pub mod prelude {
    pub use crate::{
        Agent, AgentStatus, Body, Brain, CharacterState, Collider, CombatEvent, Context, EngineConfig, Entity, Fighter,
        FlyCamera, Game, Guard, Health, Interaction, Light, MeshId, Name, NpcEvent, Options, Renderable, SaveStore,
        ShadowQuality, Stamina, ThirdPersonController, TriggerEvent, World,
    };
    pub use grimora_audio::{Audio, Category, Emitter, SoundId};
    pub use grimora_core::{
        ActionMap, Animator, Binding, Camera, Environment, EulerRot, Input, KeyCode, Mat4, MeshData, MouseButton,
        PadAxis, PadButton, ParticleDesc, ParticleEffect, ParticleShape, ParticleSystem, PointLight, Quat, Time,
        Transform, Vec2, Vec3, Vec4, actions,
    };
    pub use grimora_physics::{BodyHandle, CharacterBody, NavPath, NavShape, Physics, RayHit};
    pub use grimora_render::{Anchor, Rect, TextStyle, Theme, Ui, UiInput};
    pub use grimora_scene::{
        Behavior, Blade, BodyDesc, CombatDesc, Interactable, MeshDesc, Notice, Reaction, Trigger, TriggerShape, Weapon,
        load_effect,
    };
}
