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

mod aura;
mod combat;
mod config;
mod fauna;
mod fly_camera;
mod folk;
mod gamepad;
pub mod generated;
mod house;
mod interaction;
mod items;
pub mod land;
mod lock_on;
mod npc;
mod options;
pub mod portrait;
mod ragdoll;
mod runner;
pub mod save;
mod shot;
mod society;
pub mod story;
mod streaming;
mod third_person;
mod water;
mod weather_audio;
mod world;

use grimora_core::Vec3;

pub use aura::{Aura, AuraSkin, SkinPoint};
pub use combat::{CombatEvent, Fighter, Frozen, Guard, Health, Stamina};
pub use config::{EngineConfig, USAGE};
pub use fauna::{BirdDoing, CrabDoing, Fauna, FaunaCount, FaunaEvent, FaunaSound, RayDoing, Threat};
pub use fly_camera::FlyCamera;
pub use folk::{FOLK_NEAR, Villager};
pub use generated::{GenId, GenState, Generated};
pub use house::{DOOR_OPENS, Door, Dwelling, Roof, Uses};
pub use interaction::{InteractKind, Interaction, Nearby, TriggerEvent, TriggerState};
pub use items::{Container, DroppedItem, HOTBAR, Inventory, ItemStack, ItemState, Landing, Pickup, Placed};
pub use land::{Land, LandStats};
pub use npc::{Agent, AgentStatus, Brain, NpcEvent};
pub use options::{Options, RESOLUTIONS, ShadowQuality, UI_SCALES};
pub use ragdoll::{Push, Ragdoll, Recoil, Thud};
pub use runner::{Context, run};
pub use save::SaveStore;
pub use shot::{Shot, ShotDesc, ShotTarget};
pub use society::VillagerTalk;
pub use streaming::StreamingStats;
pub use third_person::{CharacterState, Flight, ThirdPersonController};
pub use water::Floating;
pub use world::{
    Attached, AutoWeather, Body, Collider, Entity, Light, MeshId, Name, Renderable, SceneIndex, World, Worn,
};

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

    /// The world's origin just moved, before this frame's `update`: every local position
    /// changed by `offset` (see [`World::shift_origin`]). The engine moved what it owns, the
    /// camera and sound included; move whatever positions the game keeps outside the world,
    /// such as a [`ThirdPersonController`] (with its `shift`).
    fn origin_shifted(&mut self, _ctx: &mut Context, _offset: Vec3) {}
}

pub mod prelude {
    pub use crate::{
        Agent, AgentStatus, Body, Brain, CharacterState, Collider, CombatEvent, Context, EngineConfig, Entity,
        FaunaEvent, Fighter, Flight, Floating, FlyCamera, Frozen, Game, Guard, HOTBAR, Health, Interaction, Inventory,
        ItemStack, Landing, Light, MeshId, Name, NpcEvent, Options, Pickup, Placed, Renderable, SaveStore,
        ShadowQuality, Shot, ShotDesc, ShotTarget, Stamina, ThirdPersonController, TriggerEvent, World, Worn,
    };
    pub use grimora_audio::{Audio, Category, Emitter, SoundId};
    pub use grimora_core::{
        ActionMap, Animator, Binding, Camera, DVec3, Environment, EulerRot, Input, KeyCode, Mat4, MeshData,
        MouseButton, Origin, PadAxis, PadButton, ParticleDesc, ParticleEffect, ParticleShape, ParticleSystem,
        PointLight, Quat, Time, Transform, Vec2, Vec3, Vec4, WaterLook, WaterPoint, WorldPos, WorldTransform, actions,
    };
    pub use grimora_physics::{BodyHandle, CharacterBody, NavPath, NavShape, Physics, RayHit};
    pub use grimora_render::{Anchor, Rect, TextStyle, Theme, Ui, UiInput};
    pub use grimora_scene::{
        Behavior, Blade, BodyDesc, CombatDesc, Ground, Interactable, ItemCatalog, ItemDesc, ItemKind, MeshDesc, Notice,
        Reaction, Trigger, TriggerShape, Weapon, load_effect,
    };
}
