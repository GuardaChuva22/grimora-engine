use grimora_core::{Animator, Environment, MeshData, PointLight, Transform, Vec3};
use grimora_physics::Physics;
use grimora_scene::{PlayerSpawn, Scene, Terrain};

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

/// Everything the game can see and change: objects, lights, atmosphere.
///
/// Meshes added here are uploaded to the GPU automatically before the next frame.
#[derive(Debug, Default)]
pub struct World {
    pub environment: Environment,
    pub lights: Vec<PointLight>,
    pub objects: Vec<Object>,
    pub terrain: Option<Terrain>,
    /// Player start from the scene file, if any.
    pub player_spawn: Option<PlayerSpawn>,
    meshes: Vec<MeshData>,
}

impl World {
    pub fn from_scene(scene: Scene) -> Self {
        let objects = scene
            .objects
            .into_iter()
            .map(|o| {
                let mut animator = animator_for(scene.meshes.get(o.mesh));
                if let (Some(animator), Some(clip)) = (&mut animator, &o.animation)
                    && !animator.play(clip, 0.0)
                {
                    log::warn!("object {:?}: the model has no animation {clip:?}", o.name);
                }
                Object {
                    name: o.name,
                    mesh: MeshId(o.mesh),
                    transform: o.transform,
                    color: o.color,
                    emissive: o.emissive,
                    visible: true,
                    animator,
                }
            })
            .collect();
        Self {
            environment: scene.environment,
            lights: scene.lights,
            objects,
            terrain: scene.terrain,
            player_spawn: scene.player,
            meshes: scene.meshes,
        }
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
        for animator in self.objects.iter_mut().filter_map(|o| o.animator.as_mut()) {
            animator.update(dt);
        }
    }

    pub fn find(&self, name: &str) -> Option<&Object> {
        self.objects.iter().find(|o| o.name.as_deref() == Some(name))
    }

    pub fn find_mut(&mut self, name: &str) -> Option<&mut Object> {
        self.objects.iter_mut().find(|o| o.name.as_deref() == Some(name))
    }

    /// Copies the pose of every scene object simulated by `physics` (see
    /// [`Physics::scene_bodies`]) into its transform, keeping its scale.
    /// The engine calls this after each physics step.
    pub fn sync_bodies(&mut self, physics: &Physics) {
        for scene_body in physics.scene_bodies() {
            let Some((position, rotation)) = physics.body_pose(scene_body.body) else { continue };
            if let Some(object) = self.objects.get_mut(scene_body.object) {
                object.transform.position = position;
                object.transform.rotation = rotation;
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
