use std::collections::HashMap;

use grimora_core::{
    Animator, DayCycle, Environment, MeshData, ParticleEffect, ParticleInstance, ParticleSystem, PointLight,
    SkyTextures, Transform, Vec3,
};
use grimora_physics::{BodyHandle, ColliderHandle, Physics};
use grimora_scene::{PlayerSpawn, Scene, SceneObject, SceneSound, Terrain};
pub use hecs::Entity;

use crate::combat;
use crate::interaction::TriggerState;
use crate::npc::{self, Agent};
use crate::streaming::{Streamer, StreamingStats};

/// Index of a mesh stored in the [`World`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MeshId(pub usize);

// ---- engine components. An entity can have any of these, plus the game's own.

/// The entity's name, from the scene's `name:` (see [`World::find`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Name(pub String);

/// Drawn with a mesh at the entity's [`Transform`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Renderable {
    pub mesh: MeshId,
    /// sRGB tint multiplied with the mesh's colors.
    pub color: Vec3,
    pub emissive: f32,
    pub visible: bool,
}

impl Renderable {
    pub fn new(mesh: MeshId) -> Self {
        Self { mesh, color: Vec3::ONE, emissive: 0.0, visible: true }
    }
}

/// A point light that moves with its entity: `position` is an offset in the
/// entity's frame (rotated with it, not scaled).
#[derive(Debug, Clone, PartialEq)]
pub struct Light(pub PointLight);

/// A dynamic body: after each physics step the engine copies its pose into
/// the entity's [`Transform`] (keeping the scale).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Body(pub BodyHandle);

/// A static collider; [`World::despawn`] removes it from the physics world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Collider(pub ColliderHandle);

/// Position of the entity in the scene file's `objects`; saved games use it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SceneIndex(pub usize);

/// Everything the game can see and change: entities, lights, atmosphere.
///
/// Objects are entities in [`World::ecs`] (a [`hecs::World`]) with engine
/// components ([`Transform`], [`Renderable`], [`Name`], [`Light`],
/// [`Animator`], [`Body`], [`Collider`], [`SceneIndex`], [`Agent`]) and any components
/// of the game's own:
///
/// ```ignore
/// struct Health(u32);
/// let goblin = ctx.world.spawn(mesh, transform);
/// ctx.world.ecs.insert_one(goblin, Health(10))?;
/// for (transform, health) in ctx.world.ecs.query_mut::<(&mut Transform, &Health)>() { ... }
/// ```
///
/// Entity ids stay valid until the entity is despawned, and a despawned id
/// never points at another entity. Meshes added here are uploaded to the GPU
/// automatically before the next frame.
///
/// With streaming (see `grimora_scene::StreamingDesc`), scattered objects and
/// region files become entities only while near the camera.
#[derive(Default)]
pub struct World {
    pub environment: Environment,
    /// The pictures of the sun and moon in the sky.
    pub sky_textures: SkyTextures,
    /// The running day, if the scene has one. It writes the sun, light, fog and
    /// sky of [`World::environment`] every frame, so to change those for good,
    /// change its keys.
    pub day_cycle: Option<DayCycle>,
    /// Lights not attached to an entity (the scene's `lights`, and any the game
    /// adds). Lights on entities are [`Light`] components.
    pub lights: Vec<PointLight>,
    pub ecs: hecs::World,
    pub terrain: Option<Terrain>,
    /// Player start from the scene file, if any.
    pub player_spawn: Option<PlayerSpawn>,
    /// Always-present sounds; streamed ones are added by [`World::active_sounds`].
    pub sounds: Vec<SceneSound>,
    /// Which triggers the player is in and which `once` ones already fired, by
    /// name, so it survives streaming; see [`World::update_triggers`].
    pub trigger_state: TriggerState,
    /// While true, every [`Brain`](crate::Brain) ignores the player: one who just
    /// died, is in a cutscene or can't be seen. Those who had noticed lose interest.
    pub npcs_ignore_player: bool,
    meshes: Vec<MeshData>,
    streamer: Option<Streamer>,
    /// The scene file's objects, to put them back when a saved game loads.
    pub(crate) scene_objects: Vec<SceneObject>,
}

impl std::fmt::Debug for World {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("World")
            .field("entities", &self.ecs.len())
            .field("meshes", &self.meshes.len())
            .field("lights", &self.lights.len())
            .field("streaming", &self.streaming_stats())
            .finish_non_exhaustive()
    }
}

impl World {
    /// Spawns an entity per scene object, with its collider or body added to
    /// `physics`. Nothing streamed is loaded yet: call [`World::load_around`]
    /// (the engine does, at the camera, when it loads a scene).
    pub fn from_scene(scene: Scene, physics: &mut Physics) -> Self {
        let mut ecs = hecs::World::new();
        for (index, object) in scene.objects.iter().enumerate() {
            let mut entity = object_builder(object, MeshId(object.mesh), &scene.meshes, physics);
            ecs.spawn(entity.add(SceneIndex(index)).build());
        }
        for trigger in scene.triggers {
            ecs.spawn((trigger.transform, trigger.trigger));
        }
        for (transform, effect) in &scene.effects {
            ecs.spawn((*transform, effect.system(transform)));
        }
        physics.refresh();
        let streamer = scene.streaming.map(|s| Streamer::new(s, scene.terrain.as_ref()));
        let day_cycle = scene.day_cycle.as_ref().map(DayCycle::new);
        let mut environment = scene.environment;
        if let Some(day) = &day_cycle {
            day.apply(&mut environment);
        }
        Self {
            environment,
            sky_textures: scene.sky_textures,
            day_cycle,
            lights: scene.lights,
            ecs,
            terrain: scene.terrain,
            player_spawn: scene.player,
            npcs_ignore_player: false,
            sounds: scene.sounds,
            trigger_state: TriggerState::default(),
            meshes: scene.meshes,
            streamer,
            scene_objects: scene.objects,
        }
    }

    /// Spawns a drawn entity. Animated meshes get an [`Animator`] in the bind pose.
    pub fn spawn(&mut self, mesh: MeshId, transform: Transform) -> Entity {
        let mut entity = hecs::EntityBuilder::new();
        entity.add(transform).add(Renderable::new(mesh));
        if let Some(animator) = animator_for(self.meshes.get(mesh.0)) {
            entity.add(animator);
        }
        self.ecs.spawn(entity.build())
    }

    /// Removes an entity and its collider or body. Returns false if it was
    /// already gone. Streamed entities come back when their cell or region
    /// loads again.
    pub fn despawn(&mut self, entity: Entity, physics: &mut Physics) -> bool {
        despawn(&mut self.ecs, entity, physics)
    }

    /// A component of `entity`, e.g. `world.get::<Transform>(e)`.
    pub fn get<T: hecs::Component>(&self, entity: Entity) -> Option<hecs::Ref<'_, T>> {
        self.ecs.get::<&T>(entity).ok()
    }

    /// A component of `entity` to change, e.g. `world.get_mut::<Transform>(e)`.
    pub fn get_mut<T: hecs::Component>(&mut self, entity: Entity) -> Option<&mut T> {
        self.ecs.query_one_mut::<&mut T>(entity).ok()
    }

    /// Copy of the entity's transform.
    pub fn transform(&self, entity: Entity) -> Option<Transform> {
        self.get::<Transform>(entity).map(|t| *t)
    }

    /// The first entity called `name`, streamed ones included.
    pub fn find(&self, name: &str) -> Option<Entity> {
        self.ecs.query::<(Entity, &Name)>().iter().find(|(_, n)| n.0 == name).map(|(e, _)| e)
    }

    /// Streams in what is near `focus` and out what is far, a few cells per
    /// call, with regions built in the background. The engine calls this every
    /// frame with the camera position. Returns whether anything changed.
    pub fn update_streaming(&mut self, focus: Vec3, physics: &mut Physics) -> bool {
        self.stream(focus, physics, false)
    }

    /// Like [`World::update_streaming`], but loads everything within reach
    /// before returning (for the first frame, or after a teleport).
    pub fn load_around(&mut self, focus: Vec3, physics: &mut Physics) -> bool {
        let changed = self.stream(focus, physics, true);
        physics.refresh();
        changed
    }

    fn stream(&mut self, focus: Vec3, physics: &mut Physics, wait: bool) -> bool {
        let Some(streamer) = &mut self.streamer else { return false };
        streamer.update(focus, &mut self.ecs, &mut self.meshes, physics, wait)
    }

    /// Drops everything streamed in and loads what is around `focus` afresh,
    /// as the scene describes it.
    pub(crate) fn reload_streaming(&mut self, focus: Vec3, physics: &mut Physics) {
        if let Some(streamer) = &mut self.streamer {
            streamer.unload_all(&mut self.ecs, physics);
            self.load_around(focus, physics);
        }
    }

    /// The live entity of each scene object, by its index in the scene file.
    pub(crate) fn scene_entities(&self) -> HashMap<usize, Entity> {
        self.ecs.query::<(Entity, &SceneIndex)>().iter().map(|(entity, index)| (index.0, entity)).collect()
    }

    /// Puts every scene object back as the scene file placed it: still visible,
    /// at rest, usable, and respawned if it was despawned. Returns their entities.
    pub(crate) fn reset_scene_objects(&mut self, physics: &mut Physics) -> HashMap<usize, Entity> {
        let mut live = self.scene_entities();
        let mut respawned = false;
        for (index, desc) in self.scene_objects.iter().enumerate() {
            let Some(&entity) = live.get(&index) else {
                let mut entity = object_builder(desc, MeshId(desc.mesh), &self.meshes, physics);
                live.insert(index, self.ecs.spawn(entity.add(SceneIndex(index)).build()));
                respawned = true;
                continue;
            };
            let query = self.ecs.query_one_mut::<(&mut Transform, Option<&mut Renderable>, Option<&Body>)>(entity);
            let Ok((transform, renderable, body)) = query else { continue };
            *transform = desc.transform;
            if let Some(renderable) = renderable {
                renderable.visible = true;
            }
            if let Some(Body(body)) = body {
                physics.set_body_state(*body, desc.transform.position, desc.transform.rotation, Vec3::ZERO, Vec3::ZERO);
            }
            if let Some(interact) = &desc.interact {
                let _ = self.ecs.insert_one(entity, interact.clone());
            }
            if desc.combat.is_some() {
                combat::revive(&mut self.ecs, entity, physics);
                if let Ok(animator) = self.ecs.query_one_mut::<&mut Animator>(entity) {
                    match &desc.animation {
                        Some(clip) => {
                            animator.play(clip, 0.0);
                        }
                        None => animator.stop(),
                    }
                }
            }
        }
        if respawned {
            physics.refresh();
        }
        live
    }

    /// How much of the world is loaded, or `None` without streaming.
    pub fn streaming_stats(&self) -> Option<StreamingStats> {
        self.streamer.as_ref().map(Streamer::stats)
    }

    /// Lights to render: [`World::lights`], those of loaded regions, and every
    /// [`Light`] component placed at its entity.
    pub fn active_lights(&self) -> Vec<PointLight> {
        let free = self.lights.iter().chain(self.streamer.iter().flat_map(|s| s.chunks()).flat_map(|c| &c.lights));
        let mut lights: Vec<PointLight> = free.cloned().collect();
        for (transform, Light(light)) in self.ecs.query::<(&Transform, &Light)>().iter() {
            let position = transform.position + transform.rotation * light.position;
            lights.push(PointLight { position, ..light.clone() });
        }
        // Those of the night come on as the daylight goes.
        let daylight = self.day_cycle.as_ref().map_or(0.0, DayCycle::daylight);
        lights.retain_mut(|light| {
            if light.night_only {
                light.intensity *= 1.0 - daylight;
            }
            light.intensity > 0.0
        });
        lights
    }

    /// The hour of the scene's day, from 0 to 24, if it has one.
    pub fn time_of_day(&self) -> Option<f32> {
        self.day_cycle.as_ref().map(DayCycle::hour)
    }

    /// Moves the clock to `hour`. Does nothing in a scene without a day cycle.
    pub fn set_time_of_day(&mut self, hour: f32) {
        if let Some(day) = &mut self.day_cycle {
            day.set_hour(hour);
            day.apply(&mut self.environment);
        }
    }

    /// Whether the sun has set. Always false in a scene without a day cycle.
    pub fn is_night(&self) -> bool {
        self.day_cycle.as_ref().is_some_and(DayCycle::is_night)
    }

    /// Runs the clock for `dt` seconds. The engine calls this each frame, unless paused.
    pub fn update_day(&mut self, dt: f32) {
        if let Some(day) = &mut self.day_cycle {
            day.advance(dt);
            day.apply(&mut self.environment);
        }
    }

    /// Looping sounds to play: [`World::sounds`] plus those of loaded regions.
    pub fn active_sounds(&self) -> Vec<SceneSound> {
        let streamed = self.streamer.iter().flat_map(|s| s.chunks()).flat_map(|c| &c.sounds);
        self.sounds.iter().chain(streamed).cloned().collect()
    }

    pub fn add_mesh(&mut self, mesh: MeshData) -> MeshId {
        self.meshes.push(mesh);
        MeshId(self.meshes.len() - 1)
    }

    pub fn mesh(&self, id: MeshId) -> Option<&MeshData> {
        self.meshes.get(id.0)
    }

    pub(crate) fn meshes(&self) -> &[MeshData] {
        &self.meshes
    }

    /// Spawns `mesh` held by a joint of `parent`: a sword in a hand. It follows
    /// the joint from then on, and goes away with `parent`.
    pub fn attach(&mut self, parent: Entity, joint: &str, mesh: MeshId, offset: Transform) -> Entity {
        let entity = self.spawn(mesh, Transform::IDENTITY);
        let _ = self.ecs.insert_one(entity, Attached { parent, joint: joint.to_string(), offset });
        self.update_attachments();
        entity
    }

    /// Puts everything [`World::attach`] made where its joint is now. The engine
    /// calls this after [`World::update_animations`].
    pub fn update_attachments(&mut self) {
        let mut orphans = Vec::new();
        let mut placed = Vec::new();
        for (entity, attached) in self.ecs.query::<(Entity, &Attached)>().iter() {
            let mut parent = self.ecs.query_one::<(&Transform, &Animator, Option<&Renderable>)>(attached.parent);
            let held = parent.get().ok().map(|(transform, animator, renderable)| {
                let joint = animator.joint_transform(&attached.joint).unwrap_or_default();
                let (scale, rotation, position) =
                    (transform.matrix() * joint * attached.offset.matrix()).to_scale_rotation_translation();
                (Transform { position, rotation, scale }, renderable.is_none_or(|r| r.visible))
            });
            match held {
                Some(held) => placed.push((entity, held)),
                None => orphans.push(entity),
            }
        }
        for (entity, (at, visible)) in placed {
            if let Some(transform) = self.get_mut::<Transform>(entity) {
                *transform = at;
            }
            if let Some(renderable) = self.get_mut::<Renderable>(entity) {
                renderable.visible = visible;
            }
        }
        for entity in orphans {
            let _ = self.ecs.despawn(entity);
        }
    }

    /// Advances every animator. The engine calls this after `Game::update`.
    pub fn update_animations(&mut self, dt: f32) {
        for animator in self.ecs.query_mut::<&mut Animator>() {
            animator.update(dt);
        }
    }

    /// Advances every [`ParticleSystem`], with `camera` deciding which are near
    /// enough to bother, and removes the finished one-shots. The engine calls
    /// this after `Game::update`.
    pub fn update_particles(&mut self, dt: f32, camera: Vec3) {
        let mut finished = Vec::new();
        for (entity, transform, system, renderable) in
            self.ecs.query_mut::<(Entity, &Transform, &mut ParticleSystem, Option<&Renderable>)>()
        {
            system.update(transform, dt, camera, renderable.is_none_or(|r| r.visible));
            if system.despawn_when_done && system.finished() {
                finished.push(entity);
            }
        }
        for entity in finished {
            let _ = self.ecs.despawn(entity);
        }
    }

    /// Plays `effect` (see `grimora_scene::load_effect`) at `position`. The
    /// entity removes itself when the effect ends; one that loops plays until
    /// the game despawns it.
    pub fn play_effect(&mut self, effect: &ParticleEffect, position: Vec3) -> Entity {
        // Effects played in the same spot still differ, since live entity ids do.
        let seed = self.ecs.len() as u64 ^ u64::from(position.x.to_bits()) ^ u64::from(position.z.to_bits()) << 32;
        let mut system = ParticleSystem::new(effect.clone(), Vec3::ZERO, 1.0, seed);
        system.despawn_when_done = true;
        self.ecs.spawn((Transform::from_position(position), system))
    }

    /// Every live particle, for the renderer.
    pub fn active_particles(&self) -> Vec<ParticleInstance> {
        let mut particles = Vec::new();
        for system in self.ecs.query::<&ParticleSystem>().iter() {
            system.collect(&mut particles);
        }
        particles
    }

    /// Copies the pose of every [`Body`] into its entity's transform, keeping
    /// the scale. The engine calls this after each physics step.
    pub fn sync_bodies(&mut self, physics: &Physics) {
        for (transform, Body(body)) in self.ecs.query_mut::<(&mut Transform, &Body)>() {
            if let Some((position, rotation)) = physics.body_pose(*body) {
                transform.position = position;
                transform.rotation = rotation;
            }
        }
    }

    /// World transform of an animated entity's joint, for attaching things to it.
    pub fn joint_transform(&self, entity: Entity, joint: &str) -> Option<Transform> {
        let mut query = self.ecs.query_one::<(&Transform, &Animator)>(entity);
        let (transform, animator) = query.get().ok()?;
        let joint = animator.joint_transform(joint)?;
        let (scale, rotation, position) = (transform.matrix() * joint).to_scale_rotation_translation();
        Some(Transform { position, rotation, scale })
    }

    /// Ground height at `(x, z)`, or 0 without terrain.
    pub fn ground_height(&self, x: f32, z: f32) -> f32 {
        self.terrain.as_ref().map_or(0.0, |t| t.height_at(x, z))
    }
}

/// Follows a joint of another entity: see [`World::attach`].
#[derive(Debug, Clone)]
pub struct Attached {
    pub parent: Entity,
    pub joint: String,
    /// From the joint to this entity.
    pub offset: Transform,
}

/// The components of a scene object whose mesh is now `mesh`: its animation
/// clip started, and its collider or body added to `physics`.
pub(crate) fn object_builder(
    o: &SceneObject,
    mesh: MeshId,
    meshes: &[MeshData],
    physics: &mut Physics,
) -> hecs::EntityBuilder {
    let mut entity = hecs::EntityBuilder::new();
    entity.add(o.transform).add(Renderable { mesh, color: o.color, emissive: o.emissive, visible: true });
    if let Some(name) = &o.name {
        entity.add(Name(name.clone()));
    }
    if let Some(light) = &o.light {
        entity.add(Light(light.clone()));
    }
    if let Some(interact) = &o.interact {
        entity.add(interact.clone());
    }
    if let Some(effect) = &o.effect {
        entity.add(effect.system(&o.transform));
    }
    if let Some(mut animator) = animator_for(meshes.get(mesh.0)) {
        if let Some(clip) = &o.animation
            && !animator.play(clip, 0.0)
        {
            log::warn!("object {:?}: the model has no animation {clip:?}", o.name);
        }
        entity.add(animator);
    }
    if let Some(desc) = &o.npc {
        let (agent, brain) = npc::from_desc(desc, &o.transform, physics);
        entity.add(agent).add(brain);
    }
    if let Some(desc) = &o.combat {
        let (health, fighter) = combat::components(desc);
        entity.add(health);
        if let Some(fighter) = fighter {
            entity.add(fighter);
        }
    }
    match &o.collider {
        Some(shape) if o.body.is_dynamic() => {
            if let Some(body) = physics.add_body(shape, &o.transform, &o.body) {
                entity.add(Body(body));
            }
        }
        Some(shape) => {
            if let Some(collider) = physics.add_static(shape, &o.transform) {
                entity.add(Collider(collider));
            }
        }
        None if o.body.is_dynamic() => {
            log::warn!("object {:?} is dynamic but has no collider; it will not move", o.name);
        }
        None => {}
    }
    entity
}

/// Despawns `entity` and removes its collider or body from `physics`.
pub(crate) fn despawn(ecs: &mut hecs::World, entity: Entity, physics: &mut Physics) -> bool {
    if let Ok(body) = ecs.get::<&Body>(entity) {
        physics.remove_body(body.0);
    }
    if let Ok(collider) = ecs.get::<&Collider>(entity) {
        physics.remove(collider.0);
    }
    if let Ok(agent) = ecs.get::<&Agent>(entity) {
        physics.remove(agent.collider());
    }
    ecs.despawn(entity).is_ok()
}

fn animator_for(mesh: Option<&MeshData>) -> Option<Animator> {
    mesh?.skeleton.clone().map(Animator::new)
}
