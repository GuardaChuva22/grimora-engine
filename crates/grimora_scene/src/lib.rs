//! Scenes for Grimora, described in human- and AI-editable RON files.
//!
//! A [`SceneDesc`] is what lives on disk; [`Scene::build`] turns it into meshes,
//! object instances, lights and a camera that the engine can render.
//! See `assets/scenes/sandbox.ron` for a commented example.

mod collider;
mod model;
mod rng;
mod shapes;
mod sound;
mod streaming;
mod terrain;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use grimora_core::{Camera, Environment, MeshData, PointLight, Transform, Vec2, Vec3};
use serde::{Deserialize, Serialize};

pub use collider::{BodyDesc, ColliderDesc, ColliderShape};
pub use model::{DEFAULT_MAX_TEXTURE_SIZE, load_model};
pub use rng::{Rng, fbm, value_noise};
pub use shapes::{MeshDesc, blob, cube, tube};
pub use sound::{SceneSound, SoundDesc};
pub use streaming::{CellCoord, RegionDesc, SceneCell, SceneRegion, SceneStreaming, StreamingDesc};
pub use terrain::{Terrain, TerrainDesc};

/// The on-disk scene format.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SceneDesc {
    pub environment: Environment,
    pub camera: CameraDesc,
    pub terrain: Option<TerrainDesc>,
    pub objects: Vec<ObjectDesc>,
    pub scatter: Vec<ScatterDesc>,
    pub lights: Vec<PointLight>,
    /// Where the player character starts, if the game has one.
    pub player: Option<PlayerDesc>,
    /// Looping ambience and positional emitters.
    pub sounds: Vec<SoundDesc>,
    /// Load scattered objects only near the camera (see [`StreamingDesc`]).
    /// Implied, with default settings, when there are `regions`.
    pub streaming: Option<StreamingDesc>,
    /// Scene fragments in their own files, loaded in the background when the
    /// camera gets close.
    pub regions: Vec<RegionDesc>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerDesc {
    /// With a terrain, `y` is measured from the ground.
    pub position: Vec3,
    /// Facing in degrees. 0 looks towards -Z.
    pub yaw: f32,
}

/// Resolved player start in world space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerSpawn {
    pub position: Vec3,
    /// Radians.
    pub yaw: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraDesc {
    pub position: Vec3,
    /// When set, overrides `yaw`/`pitch`.
    pub look_at: Option<Vec3>,
    /// Degrees. 0 looks towards -Z.
    pub yaw: f32,
    /// Degrees. Positive looks up.
    pub pitch: f32,
    /// Vertical field of view in degrees.
    pub fov: f32,
}

impl Default for CameraDesc {
    fn default() -> Self {
        Self { position: Vec3::new(0.0, 3.0, 10.0), look_at: None, yaw: 0.0, pitch: -10.0, fov: 60.0 }
    }
}

/// A single placed object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ObjectDesc {
    pub name: Option<String>,
    pub mesh: MeshDesc,
    pub position: Vec3,
    /// Euler angles in degrees (yaw around Y applied first).
    pub rotation: Vec3,
    pub scale: Vec3,
    /// sRGB tint multiplied with the mesh colors.
    pub color: Vec3,
    pub emissive: f32,
    /// With a terrain, `position.y` is measured from the ground surface.
    pub snap_to_ground: bool,
    /// Optional light attached to the object; its position is an offset in
    /// world units, rotated with the object (not scaled).
    pub light: Option<PointLight>,
    pub collider: ColliderDesc,
    /// `Dynamic(...)` makes the object fall and get pushed around; needs a collider.
    pub body: BodyDesc,
    /// Animated models: the clip looped from the start (e.g. `"idle"`).
    pub animation: Option<String>,
    /// Optional looping sound attached to the object; its position is an offset
    /// rotated with the object, like `light`.
    pub sound: Option<SoundDesc>,
}

impl Default for ObjectDesc {
    fn default() -> Self {
        Self {
            name: None,
            mesh: MeshDesc::Cube,
            position: Vec3::ZERO,
            rotation: Vec3::ZERO,
            scale: Vec3::ONE,
            color: Vec3::ONE,
            emissive: 0.0,
            snap_to_ground: true,
            light: None,
            collider: ColliderDesc::Auto,
            body: BodyDesc::Static,
            animation: None,
            sound: None,
        }
    }
}

/// Randomly places many copies of a mesh in a ring around `center`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScatterDesc {
    pub mesh: MeshDesc,
    /// Distinct shapes generated for seeded meshes (each copy picks one).
    pub variants: u32,
    pub count: u32,
    pub center: Vec3,
    pub radius: f32,
    /// Keeps the inside of the ring empty.
    pub min_radius: f32,
    /// Uniform scale range `(min, max)`.
    pub scale: (f32, f32),
    pub color: Vec3,
    /// Random brightness variation per copy, `0..1`.
    pub color_jitter: f32,
    pub emissive: f32,
    /// Random tilt in degrees, so things don't look stamped.
    pub tilt: f32,
    /// How far each copy is pushed into the ground.
    pub sink: f32,
    pub seed: u64,
    /// Optional light attached to every copy; its position is an offset.
    pub light: Option<PointLight>,
    pub collider: ColliderDesc,
}

impl Default for ScatterDesc {
    fn default() -> Self {
        Self {
            mesh: MeshDesc::Rock { seed: 0 },
            variants: 4,
            count: 10,
            center: Vec3::ZERO,
            radius: 50.0,
            min_radius: 0.0,
            scale: (0.8, 1.2),
            color: Vec3::ONE,
            color_jitter: 0.1,
            emissive: 0.0,
            tilt: 4.0,
            sink: 0.05,
            seed: 1,
            light: None,
            collider: ColliderDesc::Auto,
        }
    }
}

/// A placed mesh instance in a built scene.
#[derive(Debug, Clone)]
pub struct SceneObject {
    pub name: Option<String>,
    /// Index into [`Scene::meshes`].
    pub mesh: usize,
    pub transform: Transform,
    pub color: Vec3,
    pub emissive: f32,
    /// Collision shape in local space; the transform (scale included) still applies.
    pub collider: Option<ColliderShape>,
    /// Dynamic objects are moved by the physics simulation.
    pub body: BodyDesc,
    /// Clip to loop when the mesh has a skeleton.
    pub animation: Option<String>,
}

/// A scene ready to render: unique meshes plus the instances that use them.
#[derive(Debug, Clone)]
pub struct Scene {
    pub environment: Environment,
    pub camera: Camera,
    pub lights: Vec<PointLight>,
    pub meshes: Vec<MeshData>,
    pub objects: Vec<SceneObject>,
    pub terrain: Option<Terrain>,
    pub player: Option<PlayerSpawn>,
    /// Looping sounds, positions in world space and paths resolved from the scene directory.
    pub sounds: Vec<SceneSound>,
    /// With streaming on, scattered objects live here (by cell) instead of in
    /// `objects`, along with the region files to load.
    pub streaming: Option<SceneStreaming>,
}

/// Terrain render chunks are this many grid cells on a side.
const TERRAIN_CHUNK: usize = 32;

impl SceneDesc {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Self> {
        Ok(ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME).from_str(text)?)
    }

    pub fn to_ron(&self) -> Result<String> {
        Ok(ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())?)
    }
}

impl Scene {
    /// Loads and builds a scene file. Model paths are relative to its directory.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        Ok(Self::build_in(&SceneDesc::load(path)?, path.parent().unwrap_or(Path::new(""))))
    }

    /// Builds a scene, resolving model paths from the current directory.
    pub fn build(desc: &SceneDesc) -> Self {
        Self::build_in(desc, Path::new(""))
    }

    /// Builds a scene, resolving model paths from `dir`.
    pub fn build_in(desc: &SceneDesc, dir: &Path) -> Self {
        let terrain = desc.terrain.clone().map(Terrain::new);
        let mut builder = Builder::new(dir, terrain.as_ref());
        if let Some(terrain) = &terrain {
            for chunk in terrain.chunk_meshes(TERRAIN_CHUNK) {
                let collider = Some(ColliderShape::trimesh(&chunk));
                builder.meshes.push(chunk);
                builder.objects.push(SceneObject {
                    name: Some("terrain".into()),
                    mesh: builder.meshes.len() - 1,
                    transform: Transform::IDENTITY,
                    color: Vec3::ONE,
                    emissive: 0.0,
                    collider,
                    body: BodyDesc::Static,
                    animation: None,
                });
            }
        }

        let settings = desc.streaming.clone().or_else(|| (!desc.regions.is_empty()).then(StreamingDesc::default));
        let mut streaming = settings.map(|s| SceneStreaming {
            cell_size: s.cell_size.max(1.0),
            load_distance: s.load_distance.max(0.0),
            ..Default::default()
        });
        builder.add_contents(desc, streaming.as_mut());
        if let Some(streaming) = &mut streaming {
            streaming.regions = desc
                .regions
                .iter()
                .map(|r| SceneRegion {
                    path: dir.join(&r.path),
                    center: Vec2::new(r.center.x, r.center.z),
                    radius: r.radius.max(0.0),
                })
                .collect();
        }

        let c = &desc.camera;
        let mut camera = Camera::new(c.position, c.yaw.to_radians(), c.pitch.to_radians());
        camera.fov_y = c.fov.clamp(10.0, 150.0).to_radians();
        if let Some(target) = c.look_at {
            camera.look_at(target);
        }

        let player = desc.player.as_ref().map(|p| PlayerSpawn {
            position: p.position + Vec3::Y * builder.ground(p.position.x, p.position.z),
            yaw: p.yaw.to_radians(),
        });

        let Builder { meshes, objects, lights, sounds, .. } = builder;
        Self {
            environment: desc.environment.clone(),
            camera,
            lights,
            meshes,
            objects,
            terrain,
            player,
            sounds,
            streaming,
        }
    }

    /// Loads a region file (see [`RegionDesc`]): only its objects, scatter,
    /// lights and sounds, placed on the main scene's `terrain`.
    pub fn load_region(path: &Path, terrain: Option<&Terrain>) -> Result<Self> {
        let desc = SceneDesc::load(path)?;
        if desc.terrain.is_some() || !desc.regions.is_empty() {
            log::warn!("{}: regions can't have a terrain or regions of their own; ignored", path.display());
        }
        let mut builder = Builder::new(path.parent().unwrap_or(Path::new("")), terrain);
        builder.add_contents(&desc, None);
        Ok(Self {
            environment: Environment::default(),
            camera: Camera::default(),
            lights: builder.lights,
            meshes: builder.meshes,
            objects: builder.objects,
            terrain: None,
            player: None,
            sounds: builder.sounds,
            streaming: None,
        })
    }

    /// Ground height at `(x, z)`, or 0 without terrain.
    pub fn ground_height(&self, x: f32, z: f32) -> f32 {
        self.terrain.as_ref().map_or(0.0, |t| t.height_at(x, z))
    }

    pub fn find(&self, name: &str) -> Option<&SceneObject> {
        self.objects.iter().find(|o| o.name.as_deref() == Some(name))
    }

    pub fn triangle_count(&self) -> usize {
        self.objects.iter().map(|o| self.meshes[o.mesh].triangle_count()).sum()
    }
}

/// Places a light relative to its object: rotated with it, but not scaled.
fn attached_light(light: &PointLight, transform: &Transform) -> PointLight {
    PointLight { position: transform.position + transform.rotation * light.position, ..light.clone() }
}

/// Collects unique meshes so identical descriptions share one GPU buffer.
struct Builder<'a> {
    meshes: Vec<MeshData>,
    objects: Vec<SceneObject>,
    lights: Vec<PointLight>,
    sounds: Vec<SceneSound>,
    cache: HashMap<String, usize>,
    /// Directory model paths are relative to.
    dir: PathBuf,
    terrain: Option<&'a Terrain>,
}

impl<'a> Builder<'a> {
    fn new(dir: &Path, terrain: Option<&'a Terrain>) -> Self {
        Self {
            meshes: Vec::new(),
            objects: Vec::new(),
            lights: Vec::new(),
            sounds: Vec::new(),
            cache: HashMap::new(),
            dir: dir.to_path_buf(),
            terrain,
        }
    }

    fn ground(&self, x: f32, z: f32) -> f32 {
        self.terrain.map_or(0.0, |t| t.height_at(x, z))
    }

    fn mesh(&mut self, desc: &MeshDesc) -> usize {
        let key = format!("{desc:?}");
        if let Some(&index) = self.cache.get(&key) {
            return index;
        }
        self.meshes.push(desc.build_in(&self.dir));
        self.cache.insert(key, self.meshes.len() - 1);
        self.meshes.len() - 1
    }

    /// Adds `desc`'s objects, scatter, lights and sounds. With `streaming`,
    /// scattered copies go into its cells instead.
    fn add_contents(&mut self, desc: &SceneDesc, mut streaming: Option<&mut SceneStreaming>) {
        let dir = self.dir.clone();
        self.lights.extend(desc.lights.iter().cloned());
        for sound in &desc.sounds {
            let sound = sound.resolve(&dir, |x, z| self.ground(x, z));
            self.sounds.push(sound);
        }
        for obj in &desc.objects {
            let mut position = obj.position;
            if obj.snap_to_ground {
                position.y += self.ground(position.x, position.z);
            }
            let transform = Transform::from_euler_degrees(position, obj.rotation, obj.scale);
            let mesh = self.mesh(&obj.mesh);
            let collider = obj.collider.resolve(&obj.mesh, &self.meshes[mesh]);
            self.objects.push(SceneObject {
                name: obj.name.clone(),
                mesh,
                transform,
                color: obj.color,
                emissive: obj.emissive,
                collider,
                body: obj.body,
                animation: obj.animation.clone(),
            });
            if let Some(light) = &obj.light {
                self.lights.push(attached_light(light, &transform));
            }
            if let Some(sound) = &obj.sound {
                self.sounds.push(sound.attached(&dir, &transform));
            }
        }

        for scatter in &desc.scatter {
            let mut rng = Rng::new(scatter.seed);
            let variants: Vec<(usize, Option<ColliderShape>)> = (0..scatter.variants.max(1))
                .map(|k| {
                    let desc = scatter.mesh.with_seed(scatter.seed.wrapping_mul(31).wrapping_add(u64::from(k)));
                    let mesh = self.mesh(&desc);
                    (mesh, scatter.collider.resolve(&desc, &self.meshes[mesh]))
                })
                .collect();
            let (r0, r1) = (scatter.min_radius.max(0.0), scatter.radius.max(scatter.min_radius));
            for _ in 0..scatter.count {
                // sqrt keeps the density uniform over the ring's area.
                let r = (rng.f32() * (r1 * r1 - r0 * r0) + r0 * r0).sqrt();
                let a = rng.range(0.0, std::f32::consts::TAU);
                let (x, z) = (scatter.center.x + a.cos() * r, scatter.center.z + a.sin() * r);
                let y = self.ground(x, z) + scatter.center.y - scatter.sink;
                let scale = rng.range(scatter.scale.0, scatter.scale.1);
                let rotation = Vec3::new(rng.jitter(scatter.tilt), rng.range(0.0, 360.0), rng.jitter(scatter.tilt));
                let transform = Transform::from_euler_degrees(Vec3::new(x, y, z), rotation, Vec3::splat(scale));
                let (mesh, collider) = variants[(rng.next_u64() % variants.len() as u64) as usize].clone();
                let object = SceneObject {
                    name: None,
                    mesh,
                    transform,
                    color: scatter.color * (1.0 + rng.jitter(scatter.color_jitter)),
                    emissive: scatter.emissive,
                    collider,
                    body: BodyDesc::Static,
                    animation: None,
                };
                let light = scatter.light.as_ref().map(|light| attached_light(light, &transform));
                match streaming.as_deref_mut() {
                    Some(streaming) => {
                        let cell = streaming.cells.entry(streaming.cell_of(transform.position)).or_default();
                        cell.objects.push(object);
                        cell.lights.extend(light);
                    }
                    None => {
                        self.objects.push(object);
                        self.lights.extend(light);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
        SceneDesc(
            environment: (sun: (direction: (0.2, 0.5, -0.3), intensity: 2.0)),
            camera: (position: (0.0, 4.0, 12.0), look_at: (0.0, 1.0, 0.0)),
            terrain: (size: 64.0, resolution: 16),
            objects: [
                (name: "altar", mesh: Cube, position: (0.0, 0.0, 0.0), scale: (2.0, 1.0, 1.0)),
                (mesh: Crystal(seed: 3), position: (3.0, 0.0, 0.0), emissive: 2.0,
                 light: (position: (0.0, 1.0, 0.0), color: (0.5, 0.4, 1.0), radius: 6.0)),
            ],
            scatter: [ (mesh: Pine(), count: 20, radius: 25.0, min_radius: 8.0) ],
        )
    "#;

    #[test]
    fn parses_and_builds() {
        let desc = SceneDesc::parse(SAMPLE).unwrap();
        let scene = Scene::build(&desc);
        // terrain + 2 objects + 20 trees
        assert_eq!(scene.objects.len(), 23);
        assert_eq!(scene.lights.len(), 1);
        assert!(scene.find("altar").is_some());
        // Terrain is a triangle mesh; pines get an automatic trunk cylinder.
        assert!(matches!(scene.find("terrain").unwrap().collider, Some(ColliderShape::TriMesh { .. })));
        assert!(scene.objects.iter().any(|o| matches!(o.collider, Some(ColliderShape::Cylinder { .. }))));
        // The altar sits on the terrain surface.
        let altar = scene.find("altar").unwrap();
        assert!((altar.transform.position.y - scene.ground_height(0.0, 0.0)).abs() < 1e-4);
    }

    #[test]
    fn round_trips_through_ron() {
        let desc = SceneDesc::parse(SAMPLE).unwrap();
        let again = SceneDesc::parse(&desc.to_ron().unwrap()).unwrap();
        assert_eq!(desc, again);
    }

    #[test]
    fn parses_dynamic_bodies() {
        let desc = SceneDesc::parse(
            "(objects: [(mesh: Cube), (mesh: Cube, body: Dynamic()), (mesh: Sphere(), body: Dynamic(mass: 40.0))])",
        )
        .unwrap();
        let bodies: Vec<BodyDesc> = Scene::build(&desc).objects.iter().map(|o| o.body).collect();
        assert_eq!(bodies[0], BodyDesc::Static);
        assert_eq!(
            bodies[1],
            BodyDesc::Dynamic { density: 1000.0, mass: None, friction: 0.6, restitution: 0.0, angular_damping: 0.5 }
        );
        assert!(matches!(bodies[2], BodyDesc::Dynamic { mass: Some(40.0), .. }));
    }

    #[test]
    fn parses_sounds() {
        let desc = SceneDesc::parse(
            r#"(
                terrain: (size: 64.0, resolution: 16),
                sounds: [(path: "wind.wav"), (path: "hum.ogg", position: (2.0, 1.0, 3.0), max_distance: 9.0)],
                objects: [(mesh: Cube, position: (4.0, 0.0, 0.0), rotation: (0.0, 90.0, 0.0),
                           sound: (path: "fire.wav", position: (1.0, 0.5, 0.0)))],
            )"#,
        )
        .unwrap();
        assert_eq!(desc.sounds[0], SoundDesc { path: "wind.wav".into(), ..Default::default() });
        let scene = Scene::build_in(&desc, Path::new("scenes"));
        let [wind, hum, fire] = &scene.sounds[..] else { panic!("expected 3 sounds, got {:?}", scene.sounds) };
        assert_eq!(wind.path, Path::new("scenes").join("wind.wav"));
        assert_eq!((wind.position, wind.volume, wind.min_distance, wind.max_distance), (None, 1.0, 1.0, 25.0));
        // Free-standing sounds sit relative to the ground; attached ones follow their object.
        let hum_pos = hum.position.unwrap();
        assert!((hum_pos.y - 1.0 - scene.ground_height(2.0, 3.0)).abs() < 1e-4);
        assert_eq!(hum.max_distance, 9.0);
        let cube = scene.objects.iter().find(|o| o.transform.position.x == 4.0).unwrap();
        let expected = cube.transform.position + Vec3::new(0.0, 0.5, -1.0);
        assert!(fire.position.unwrap().distance(expected) < 1e-4, "{:?} vs {expected}", fire.position);
    }

    #[test]
    fn sandbox_scene_parses() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/scenes/sandbox.ron");
        if Path::new(path).exists() {
            Scene::load(path).unwrap();
        }
    }
}
