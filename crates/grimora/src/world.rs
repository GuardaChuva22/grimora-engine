use std::borrow::Cow;

use grimora_core::{Animator, Environment, MeshData, PointLight, Transform, Vec3};
use grimora_physics::Physics;
use grimora_scene::{PlayerSpawn, Scene, SceneObject, SceneSound, Terrain};

use crate::streaming::{Streamer, StreamingStats};

/// Index of a mesh stored in the [`World`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MeshId(pub usize);

/// Something drawn in the world.
#[derive(Debug, Clone)]
pub struct Object {
    pub name: Option<String>,
    pub mesh: MeshId,
    pub transform: Transform,
    /// sRGB tint.
    pub color: Vec3,
    pub emissive: f32,
    pub visible: bool,
    /// Plays the mesh's animations. Created automatically for meshes with a
    /// skeleton and advanced by the engine every frame after `Game::update`.
    pub animator: Option<Animator>,
}

impl Object {
    /// The world object for a scene object whose mesh is now `mesh`; starts its
    /// animation clip, if any.
    pub(crate) fn from_scene(o: &SceneObject, mesh: MeshId, meshes: &[MeshData]) -> Self {
        let mut animator = animator_for(meshes.get(mesh.0));
        if let (Some(animator), Some(clip)) = (&mut animator, &o.animation)
            && !animator.play(clip, 0.0)
        {
            log::warn!("object {:?}: the model has no animation {clip:?}", o.name);
        }
        Object {
            name: o.name.clone(),
            mesh,
            transform: o.transform,
            color: o.color,
            emissive: o.emissive,
            visible: true,
            animator,
        }
    }
}

/// Everything the game can see and change: objects, lights, atmosphere.
///
/// Meshes added here are uploaded to the GPU automatically before the next frame.
///
/// With streaming (see `grimora_scene::StreamingDesc`), scattered objects and
/// region files are loaded only near the camera. They are not in `objects` or
/// `lights`, which hold what is always there; [`World::all_objects`],
/// [`World::find`] and friends see both.
#[derive(Debug, Default)]
pub struct World {
    pub environment: Environment,
    /// Always-present lights. Streamed ones are added by [`World::active_lights`].
    pub lights: Vec<PointLight>,
    /// Always-present objects: the scene's `objects` (and, without streaming,
    /// its scatter) plus whatever the game spawns. Indices stay valid until the
    /// scene reloads.
    pub objects: Vec<Object>,
    pub terrain: Option<Terrain>,
    /// Player start from the scene file, if any.
    pub player_spawn: Option<PlayerSpawn>,
    /// Always-present sounds; streamed ones are added by [`World::active_sounds`].
    pub sounds: Vec<SceneSound>,
    meshes: Vec<MeshData>,
    streamer: Option<Streamer>,
}

impl World {
    /// Nothing streamed is loaded yet: call [`World::load_around`] (the engine
    /// does, at the camera, when it loads a scene).
    pub fn from_scene(scene: Scene) -> Self {
        let objects = scene.objects.iter().map(|o| Object::from_scene(o, MeshId(o.mesh), &scene.meshes)).collect();
        let streamer = scene.streaming.map(|s| Streamer::new(s, scene.terrain.as_ref()));
        Self {
            environment: scene.environment,
            lights: scene.lights,
            objects,
            terrain: scene.terrain,
            player_spawn: scene.player,
            sounds: scene.sounds,
            meshes: scene.meshes,
            streamer,
        }
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
        streamer.update(focus, &mut self.meshes, physics, wait)
    }

    /// How much of the world is loaded, or `None` without streaming.
    pub fn streaming_stats(&self) -> Option<StreamingStats> {
        self.streamer.as_ref().map(Streamer::stats)
    }

    /// Every object in the world: always-present ones, then streamed ones.
    pub fn all_objects(&self) -> impl Iterator<Item = &Object> {
        self.objects.iter().chain(self.streamer.iter().flat_map(|s| s.chunks()).flat_map(|c| &c.objects))
    }

    pub fn all_objects_mut(&mut self) -> impl Iterator<Item = &mut Object> {
        self.objects
            .iter_mut()
            .chain(self.streamer.iter_mut().flat_map(|s| s.chunks_mut()).flat_map(|c| &mut c.objects))
    }

    /// Lights to render: [`World::lights`] plus those of loaded cells and regions.
    pub fn active_lights(&self) -> Cow<'_, [PointLight]> {
        match &self.streamer {
            None => Cow::Borrowed(&self.lights),
            Some(s) => Cow::Owned(self.lights.iter().chain(s.chunks().flat_map(|c| &c.lights)).cloned().collect()),
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

    /// Adds an object and returns its index in [`World::objects`].
    /// Objects with an animated mesh get an [`Animator`] in the bind pose.
    pub fn spawn(&mut self, mesh: MeshId, transform: Transform, color: Vec3) -> usize {
        let animator = animator_for(self.meshes.get(mesh.0));
        self.objects.push(Object { name: None, mesh, transform, color, emissive: 0.0, visible: true, animator });
        self.objects.len() - 1
    }

    /// Advances every object's animator. The engine calls this after `Game::update`.
    pub fn update_animations(&mut self, dt: f32) {
        for animator in self.all_objects_mut().filter_map(|o| o.animator.as_mut()) {
            animator.update(dt);
        }
    }

    /// The first object called `name`, streamed ones included.
    pub fn find(&self, name: &str) -> Option<&Object> {
        self.all_objects().find(|o| o.name.as_deref() == Some(name))
    }

    pub fn find_mut(&mut self, name: &str) -> Option<&mut Object> {
        self.all_objects_mut().find(|o| o.name.as_deref() == Some(name))
    }

    /// Copies the pose of every scene object simulated by `physics` (see
    /// [`Physics::scene_bodies`], plus streamed ones) into its transform,
    /// keeping its scale. The engine calls this after each physics step.
    pub fn sync_bodies(&mut self, physics: &Physics) {
        let place = |object: Option<&mut Object>, body| {
            if let (Some(object), Some((position, rotation))) = (object, physics.body_pose(body)) {
                object.transform.position = position;
                object.transform.rotation = rotation;
            }
        };
        for scene_body in physics.scene_bodies() {
            place(self.objects.get_mut(scene_body.object), scene_body.body);
        }
        for chunk in self.streamer.iter_mut().flat_map(|s| s.chunks_mut()) {
            for &(body, index) in &chunk.bodies {
                place(chunk.objects.get_mut(index), body);
            }
        }
    }

    /// World transform of an animated object's joint, for attaching things to it.
    pub fn joint_transform(&self, object: usize, joint: &str) -> Option<Transform> {
        let object = self.objects.get(object)?;
        let joint = object.animator.as_ref()?.joint_transform(joint)?;
        let (scale, rotation, position) = (object.transform.matrix() * joint).to_scale_rotation_translation();
        Some(Transform { position, rotation, scale })
    }

    /// Ground height at `(x, z)`, or 0 without terrain.
    pub fn ground_height(&self, x: f32, z: f32) -> f32 {
        self.terrain.as_ref().map_or(0.0, |t| t.height_at(x, z))
    }
}

fn animator_for(mesh: Option<&MeshData>) -> Option<Animator> {
    mesh?.skeleton.clone().map(Animator::new)
}
