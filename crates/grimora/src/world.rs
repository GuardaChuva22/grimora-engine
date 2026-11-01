use grimora_core::{Environment, MeshData, PointLight, Transform, Vec3};
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
            .map(|o| Object {
                name: o.name,
                mesh: MeshId(o.mesh),
                transform: o.transform,
                color: o.color,
                emissive: o.emissive,
                visible: true,
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
    pub fn spawn(&mut self, mesh: MeshId, transform: Transform, color: Vec3) -> usize {
        self.objects.push(Object { name: None, mesh, transform, color, emissive: 0.0, visible: true });
        self.objects.len() - 1
    }

    pub fn find(&self, name: &str) -> Option<&Object> {
        self.objects.iter().find(|o| o.name.as_deref() == Some(name))
    }

    pub fn find_mut(&mut self, name: &str) -> Option<&mut Object> {
        self.objects.iter_mut().find(|o| o.name.as_deref() == Some(name))
    }

    /// Ground height at `(x, z)`, or 0 without terrain.
    pub fn ground_height(&self, x: f32, z: f32) -> f32 {
        self.terrain.as_ref().map_or(0.0, |t| t.height_at(x, z))
    }
}
