use std::collections::HashMap;
use std::sync::Arc;

use grimora_core::{
    Animator, DayCycle, Environment, MeshData, Origin, ParticleEffect, ParticleInstance, ParticleSystem, PointLight,
    SkyTextures, Transform, Vec3, WaterBody, Waters, Weather, WeatherParticles,
};
use grimora_physics::{BodyHandle, ColliderHandle, Physics};
use grimora_scene::{
    Ground, ItemCatalog, ObjectItems, PlayerSpawn, Scene, SceneObject, SceneSound, Terrain, WaterSounds, WeatherSounds,
};
pub use hecs::Entity;

/// How far above the terrain a hit still counts as the terrain: its collider and
/// `height_at` agree, but a step lands a little over it.
const TERRAIN_SLACK: f32 = 0.08;

use crate::combat::{self, Fighter};
use crate::fauna::Fauna;
use crate::interaction::TriggerState;
use crate::items::{Container, ItemState, Pickup};
use crate::npc::{self, Agent, Brain};
use crate::streaming::{Streamer, StreamingStats};
use crate::water::Floating;

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
    pub weather_sounds: WeatherSounds,
    pub water_sounds: WaterSounds,
    /// What `water_sounds` says plays when something falls in.
    pub(crate) splash_effect: Option<ParticleEffect>,
    /// Where something fell in since the last frame, and how hard.
    pub(crate) splashes: Vec<(Vec3, f32)>,
    falling: WeatherParticles,
    /// Which triggers the player is in and which `once` ones already fired, by
    /// name, so it survives streaming; see [`World::update_triggers`].
    pub trigger_state: TriggerState,
    /// While true, every [`Brain`](crate::Brain) ignores the player: one who just
    /// died, is in a cutscene or can't be seen. Those who had noticed lose interest.
    pub npcs_ignore_player: bool,
    pub(crate) meshes: Vec<MeshData>,
    /// Meshes whose data changed since the renderer last looked; see [`World::replace_mesh`].
    replaced_meshes: Vec<MeshId>,
    /// The ground of a generated world (`--world`); see [`crate::land`].
    pub(crate) land: Option<crate::land::Land>,
    streamer: Option<Streamer>,
    /// The scene file's objects, to put them back when a saved game loads.
    pub(crate) scene_objects: Vec<SceneObject>,
    /// Every item there is: the scene's `items` file.
    pub items: Arc<ItemCatalog>,
    /// What became of the items in the world; see [`crate::items`].
    pub item_state: ItemState,
    /// What differs from the generated world (`--world`): removed, moved or used plants and
    /// the like, by [`GenId`](crate::GenId); see [`crate::generated`].
    pub generated: crate::GenState,
    /// Each item's mesh, once something asked for it.
    pub(crate) item_meshes: HashMap<String, MeshId>,
    /// Every body of water, for the game to ask about with [`World::water_at`]. The first
    /// [`World::streamed_waters`] are those of the streamed regions loaded now, then the
    /// generated world's, then the scene's, with its sea last.
    pub waters: Waters,
    pub(crate) streamed_waters: usize,
    /// Every animal; see [`World::update_fauna`].
    pub fauna: Fauna,
    /// What local position zero is in the world: every [`Transform`] here, and every other
    /// position the engine runs on, is relative to it. See [`World::shift_origin`].
    pub(crate) origin: Origin,
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
            let mut entity = object_builder(object, MeshId(object.mesh), &scene.meshes, physics, Vec3::ZERO);
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
        let fauna = Fauna::new(scene.fauna, &scene.meshes);
        let mut world =
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
                weather_sounds: scene.weather_sounds,
                splash_effect: scene.water_sounds.splash_effect.as_ref().and_then(|path| {
                    grimora_scene::load_effect(path).map_err(|err| eprintln!("[grimora] {err:#}")).ok()
                }),
                water_sounds: scene.water_sounds,
                splashes: Vec::new(),
                falling: WeatherParticles::default(),
                trigger_state: TriggerState::default(),
                meshes: scene.meshes,
                replaced_meshes: Vec::new(),
                land: None,
                streamed_waters: 0,
                streamer,
                scene_objects: scene.objects,
                items: scene.items,
                item_state: ItemState::default(),
                generated: crate::GenState::default(),
                item_meshes: HashMap::new(),
                waters: scene.waters,
                fauna,
                origin: Origin::default(),
            };
        world.settle_fauna(physics);
        world
    }

    /// What local position zero is in the world.
    pub fn origin(&self) -> Origin {
        self.origin
    }

    /// Says where in the world what is loaded now is, without moving any of it: a scene's
    /// coordinates start out as local ones, around this origin. For the moment after loading.
    pub fn set_origin(&mut self, origin: Origin) {
        self.origin = origin;
        self.environment.origin = origin;
        self.waters.origin = origin;
    }

    /// Moves the origin by `delta`, a local position, and everything loaded by `-delta` in the
    /// same frame, so nothing moves in the world: entities, bodies and colliders, particles,
    /// lights, sounds of the scene, agents and their paths, triggers, water, animals, the
    /// terrain's queries, weather and streaming. The camera, the sound and whatever the game
    /// keeps are the engine's and the game's to move: [`crate::Context::shift_origin`] does.
    pub fn shift_origin(&mut self, delta: Vec3, physics: &mut Physics) {
        if delta == Vec3::ZERO {
            return;
        }
        let offset = -delta;
        self.set_origin(Origin::new(self.origin.world() + delta.as_dvec3()));
        physics.shift(offset);
        for transform in self.ecs.query_mut::<&mut Transform>() {
            transform.position += offset;
        }
        for system in self.ecs.query_mut::<&mut ParticleSystem>() {
            system.shift(offset);
        }
        for agent in self.ecs.query_mut::<&mut Agent>() {
            agent.shift(offset);
        }
        for brain in self.ecs.query_mut::<&mut Brain>() {
            brain.home += offset;
        }
        for fighter in self.ecs.query_mut::<&mut Fighter>() {
            fighter.shift(offset);
        }
        for light in &mut self.lights {
            light.position += offset;
        }
        for sound in &mut self.sounds {
            if let Some(position) = &mut sound.position {
                *position += offset;
            }
        }
        for splash in &mut self.splashes {
            splash.0 += offset;
        }
        for object in &mut self.scene_objects {
            object.transform.position += offset;
        }
        if let Some(spawn) = &mut self.player_spawn {
            spawn.position += offset;
        }
        if let Some(terrain) = &mut self.terrain {
            terrain.shift(offset);
        }
        if let Some(streamer) = &mut self.streamer {
            streamer.shift(offset);
        }
        self.falling.shift(offset);
        self.waters.shift(offset, self.origin);
        self.fauna.shift(offset);
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
    /// A generated entity (a plant of the land) stays gone: it is noted in
    /// [`World::generated`] and never comes back with its cell.
    pub fn despawn(&mut self, entity: Entity, physics: &mut Physics) -> bool {
        let generated = self.ecs.get::<&crate::Generated>(entity).ok().map(|g| g.id);
        if let Some(id) = generated {
            self.generated.removed.insert(id);
            self.generated.moved.remove(&id);
            if let Some(land) = &mut self.land {
                land.forget(entity, physics);
            }
        }
        despawn(&mut self.ecs, entity, physics)
    }

    /// Despawns the generated entity (a plant) whose base is nearest `center`, flat, within
    /// `radius` meters: it stays gone, as [`World::despawn`] says. Returns its id.
    pub fn cut_generated_near(&mut self, center: Vec3, radius: f32, physics: &mut Physics) -> Option<crate::GenId> {
        let flat = |p: Vec3| glam::Vec2::new(p.x - center.x, p.z - center.z).length();
        let (entity, id) = self
            .ecs
            .query::<(Entity, &Transform, &crate::Generated)>()
            .iter()
            .map(|(e, t, g)| (e, g.id, flat(t.position)))
            .filter(|&(_, _, d)| d <= radius)
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(e, id, _)| (e, id))?;
        self.despawn(entity, physics);
        Some(id)
    }

    /// The generated entity at `id`, if its cell is shown and it is still there.
    pub fn generated_entity(&self, id: crate::GenId) -> Option<Entity> {
        self.ecs.query::<(Entity, &crate::Generated)>().iter().find(|(_, g)| g.id == id).map(|(e, _)| e)
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
        let changed = streamer.update(focus, &mut self.ecs, &mut self.meshes, physics, wait);
        if changed {
            let loaded: Vec<WaterBody> = streamer.chunks().flat_map(|c| c.waters.iter().cloned()).collect();
            self.waters.bodies.splice(..self.streamed_waters, loaded.iter().cloned());
            self.streamed_waters = loaded.len();
            self.settle_pickups(physics);
        }
        changed
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
                let mut entity = object_builder(desc, MeshId(desc.mesh), &self.meshes, physics, Vec3::ZERO);
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
            if let Some(ObjectItems::Pickup { key, item, count }) = &desc.items {
                let _ = self.ecs.insert_one(entity, Pickup::new(key.clone(), item.clone(), *count));
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

    /// The weather of the scene's day, if it has one.
    pub fn weather(&self) -> Option<Weather> {
        self.day_cycle.as_ref().map(DayCycle::weather)
    }

    /// Changes what the scene's day is like, at once, with the ground already wet or
    /// under snow. Does nothing in a scene without a day cycle.
    pub fn set_weather(&mut self, weather: Weather) {
        if let Some(day) = &mut self.day_cycle {
            day.set_weather(weather);
            day.apply(&mut self.environment);
        }
    }

    /// Turns what the scene's day is like into `weather` over `seconds`. The ground gets
    /// wet or white at its own pace. Does nothing in a scene without a day cycle.
    pub fn change_weather(&mut self, weather: Weather, seconds: f32) {
        if let Some(day) = &mut self.day_cycle {
            day.change_weather(weather, seconds);
        }
    }

    /// Whether a lightning strike's thunder has just arrived. It only says so once.
    pub fn take_thunder(&mut self) -> bool {
        self.day_cycle.as_mut().is_some_and(DayCycle::take_thunder)
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

    /// Puts new data behind a mesh that is already uploaded, for meshes that come and go while
    /// the world runs: the renderer uploads it in place of the old one.
    pub fn replace_mesh(&mut self, id: MeshId, mesh: MeshData) {
        if let Some(slot) = self.meshes.get_mut(id.0) {
            *slot = mesh;
            self.replaced_meshes.push(id);
        }
    }

    pub(crate) fn take_replaced_meshes(&mut self) -> Vec<MeshId> {
        std::mem::take(&mut self.replaced_meshes)
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
        let _ = self.ecs.insert_one(entity, Attached { parent, joint: joint.to_string(), offset, hidden: false });
        self.update_attachments();
        entity
    }

    /// Spawns `mesh` on `parent`'s skin: clothes made on the same skeleton, in the same pose,
    /// which bend with the body and don't have a pose of their own. It goes away with `parent`.
    pub fn dress(&mut self, parent: Entity, mesh: MeshId) -> Entity {
        let transform = self.transform(parent).unwrap_or_default();
        self.ecs.spawn((transform, Renderable::new(mesh), Worn { parent }))
    }

    /// Puts everything [`World::attach`] made where its joint is now, and what
    /// [`World::dress`] made where its wearer is. The engine calls this after
    /// [`World::update_animations`].
    pub fn update_attachments(&mut self) {
        let mut worn = Vec::new();
        let mut gone = Vec::new();
        for (entity, piece) in self.ecs.query::<(Entity, &Worn)>().iter() {
            let mut parent = self.ecs.query_one::<(&Transform, &Renderable)>(piece.parent);
            match parent.get() {
                Ok((transform, renderable)) => worn.push((entity, *transform, renderable.visible)),
                Err(_) => gone.push(entity),
            }
        }
        for (entity, at, visible) in worn {
            if let Some(transform) = self.get_mut::<Transform>(entity) {
                *transform = at;
            }
            if let Some(renderable) = self.get_mut::<Renderable>(entity) {
                renderable.visible = visible;
            }
        }
        for entity in gone {
            let _ = self.ecs.despawn(entity);
        }
        let mut orphans = Vec::new();
        let mut placed = Vec::new();
        for (entity, attached) in self.ecs.query::<(Entity, &Attached)>().iter() {
            let mut parent = self.ecs.query_one::<(&Transform, &Animator, Option<&Renderable>)>(attached.parent);
            let held = parent.get().ok().map(|(transform, animator, renderable)| {
                let joint = animator.joint_transform(&attached.joint).unwrap_or_default();
                let (scale, rotation, position) =
                    (transform.matrix() * joint * attached.offset.matrix()).to_scale_rotation_translation();
                (Transform { position, rotation, scale }, renderable.is_none_or(|r| r.visible) && !attached.hidden)
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
            animator.set_hand_empty(true);
        }
        // A hand with something on it holds on.
        for attached in self.ecs.query::<&Attached>().iter() {
            if let Ok(mut animator) = self.ecs.get::<&mut Animator>(attached.parent)
                && animator.in_free_hand(&attached.joint)
            {
                animator.set_hand_empty(false);
            }
        }
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
        self.falling.update(dt, camera, &self.environment);
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
        self.falling.collect(&mut particles);
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

    /// What a step at `foot` lands on: whatever is just below it (an object's
    /// `ground`, stone if it names none) or the terrain there, made mud by rain.
    pub fn ground_under(&self, physics: &Physics, foot: Vec3) -> Ground {
        const ABOVE: f32 = 0.4;
        const BELOW: f32 = 0.8;
        let terrain = self.terrain.as_ref();
        let on_terrain = |point: Vec3| {
            if let Some(land) = &self.land {
                let at = self.origin().to_world(point);
                return (at.y < land.height_at(at) + f64::from(TERRAIN_SLACK)).then(|| land.ground_at(at));
            }
            terrain
                .filter(|t| point.y < t.height_at(point.x, point.z) + TERRAIN_SLACK)
                .map(|t| t.ground_at(point.x, point.z))
        };
        let hit = physics.raycast_scenery(foot + Vec3::Y * ABOVE, Vec3::NEG_Y, ABOVE + BELOW);
        let ground = match hit {
            Some(hit) => on_terrain(hit.point).unwrap_or_else(|| {
                let mut owners = self.ecs.query::<(&Collider, Option<&Ground>)>();
                let owner = owners.iter().find(|(collider, _)| collider.0 == hit.collider);
                owner.and_then(|(_, ground)| ground.copied()).unwrap_or(Ground::Stone)
            }),
            None => terrain.map_or(Ground::Dirt, |t| t.ground_at(foot.x, foot.z)),
        };
        ground.soaked(self.environment.precipitation.wet)
    }

    /// Ground height at `(x, z)`, or 0 without terrain.
    pub fn ground_height(&self, x: f32, z: f32) -> f32 {
        if let Some(land) = &self.land {
            let at = self.origin().to_world(Vec3::new(x, 0.0, z));
            return (land.height_at(at) - self.origin().world().y) as f32;
        }
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
    /// It doesn't show, whether the parent does or not, as in a pocket.
    pub hidden: bool,
}

/// Bends with another entity's skeleton: see [`World::dress`].
#[derive(Debug, Clone, Copy)]
pub struct Worn {
    pub parent: Entity,
}

/// Adds the matrices that bend an entity to `matrices`, its own or its wearer's if it's clothes,
/// and returns where they start.
pub(crate) fn push_skin(
    ecs: &hecs::World,
    animator: Option<&Animator>,
    worn: Option<&Worn>,
    matrices: &mut Vec<grimora_core::Mat4>,
) -> Option<u32> {
    let start = matrices.len() as u32;
    if let Some(animator) = animator {
        matrices.extend_from_slice(animator.skinning_matrices());
        return Some(start);
    }
    let wearer = ecs.get::<&Animator>(worn?.parent).ok()?;
    matrices.extend_from_slice(wearer.skinning_matrices());
    Some(start)
}

/// The components of a scene object whose mesh is now `mesh`: its animation
/// clip started, and its collider or body added to `physics`. It goes `offset` from where the
/// scene placed it, which is how far the world's origin has moved the scene since.
pub(crate) fn object_builder(
    o: &SceneObject,
    mesh: MeshId,
    meshes: &[MeshData],
    physics: &mut Physics,
    offset: Vec3,
) -> hecs::EntityBuilder {
    let mut entity = hecs::EntityBuilder::new();
    let placed = Transform { position: o.transform.position + offset, ..o.transform };
    entity.add(placed).add(Renderable { mesh, color: o.color, emissive: o.emissive, visible: true });
    if let Some(name) = &o.name {
        entity.add(Name(name.clone()));
    }
    if let Some(light) = &o.light {
        entity.add(Light(light.clone()));
    }
    if let Some(interact) = &o.interact {
        entity.add(interact.clone());
    }
    if let Some(ground) = o.ground {
        entity.add(ground);
    }
    if let Some(effect) = &o.effect {
        entity.add(effect.system(&o.transform));
    }
    match &o.items {
        Some(ObjectItems::Pickup { key, item, count }) => {
            entity.add(Pickup::new(key.clone(), item.clone(), *count));
        }
        Some(ObjectItems::Container { key, desc }) => {
            entity.add(Container { key: key.clone(), desc: desc.clone() });
        }
        None => {}
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
        let (agent, brain) = npc::from_desc(desc, &o.transform, offset, physics);
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
            if let Some(body) = physics.add_body(shape, &placed, &o.body) {
                entity.add(Body(body));
                if let Some(hull) = &o.float {
                    physics.set_mass_properties(body, hull.center_of_mass, hull.mass, hull.inertia);
                    entity.add(Floating { hull: hull.clone(), wet: false });
                }
            }
        }
        Some(shape) => {
            if let Some(collider) = physics.add_static(shape, &placed) {
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
