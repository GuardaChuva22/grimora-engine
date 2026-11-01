//! Scenes for Grimora, described in human- and AI-editable RON files.
//!
//! A [`SceneDesc`] is what lives on disk; [`Scene::build`] turns it into meshes,
//! object instances, lights and a camera that the engine can render.
//! See `assets/scenes/sandbox.ron` for a commented example.

mod rng;
mod shapes;
mod terrain;

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context as _, Result};
use grimora_core::{Camera, Environment, MeshData, PointLight, Transform, Vec3};
use serde::{Deserialize, Serialize};

pub use rng::{Rng, fbm, value_noise};
pub use shapes::{MeshDesc, blob, cube, tube};
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
}

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
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self::build(&SceneDesc::load(path)?))
    }

    pub fn build(desc: &SceneDesc) -> Self {
        let mut builder = Builder::default();
        let terrain = desc.terrain.clone().map(Terrain::new);
        if let Some(terrain) = &terrain {
            let mesh = builder.meshes.len();
            builder.meshes.push(terrain.mesh());
            builder.objects.push(SceneObject {
                name: Some("terrain".into()),
                mesh,
                transform: Transform::IDENTITY,
                color: Vec3::ONE,
                emissive: 0.0,
            });
        }
        let ground = |x: f32, z: f32| terrain.as_ref().map_or(0.0, |t| t.height_at(x, z));

        let mut lights = desc.lights.clone();
        for obj in &desc.objects {
            let mut position = obj.position;
            if obj.snap_to_ground {
                position.y += ground(position.x, position.z);
            }
            let transform = Transform::from_euler_degrees(position, obj.rotation, obj.scale);
            let mesh = builder.mesh(&obj.mesh);
            builder.objects.push(SceneObject {
                name: obj.name.clone(),
                mesh,
                transform,
                color: obj.color,
                emissive: obj.emissive,
            });
            if let Some(light) = &obj.light {
                lights.push(attached_light(light, &transform));
            }
        }

        for scatter in &desc.scatter {
            let mut rng = Rng::new(scatter.seed);
            let variants: Vec<usize> = (0..scatter.variants.max(1))
                .map(|k| {
                    builder.mesh(&scatter.mesh.with_seed(scatter.seed.wrapping_mul(31).wrapping_add(u64::from(k))))
                })
                .collect();
            let (r0, r1) = (scatter.min_radius.max(0.0), scatter.radius.max(scatter.min_radius));
            for _ in 0..scatter.count {
                // sqrt keeps the density uniform over the ring's area.
                let r = (rng.f32() * (r1 * r1 - r0 * r0) + r0 * r0).sqrt();
                let a = rng.range(0.0, std::f32::consts::TAU);
                let (x, z) = (scatter.center.x + a.cos() * r, scatter.center.z + a.sin() * r);
                let y = ground(x, z) + scatter.center.y - scatter.sink;
                let scale = rng.range(scatter.scale.0, scatter.scale.1);
                let rotation = Vec3::new(rng.jitter(scatter.tilt), rng.range(0.0, 360.0), rng.jitter(scatter.tilt));
                let transform = Transform::from_euler_degrees(Vec3::new(x, y, z), rotation, Vec3::splat(scale));
                let mesh = variants[(rng.next_u64() % variants.len() as u64) as usize];
                builder.objects.push(SceneObject {
                    name: None,
                    mesh,
                    transform,
                    color: scatter.color * (1.0 + rng.jitter(scatter.color_jitter)),
                    emissive: scatter.emissive,
                });
                if let Some(light) = &scatter.light {
                    lights.push(attached_light(light, &transform));
                }
            }
        }

        let c = &desc.camera;
        let mut camera = Camera::new(c.position, c.yaw.to_radians(), c.pitch.to_radians());
        camera.fov_y = c.fov.clamp(10.0, 150.0).to_radians();
        if let Some(target) = c.look_at {
            camera.look_at(target);
        }

        Self {
            environment: desc.environment.clone(),
            camera,
            lights,
            meshes: builder.meshes,
            objects: builder.objects,
            terrain,
        }
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
#[derive(Default)]
struct Builder {
    meshes: Vec<MeshData>,
    objects: Vec<SceneObject>,
    cache: HashMap<String, usize>,
}

impl Builder {
    fn mesh(&mut self, desc: &MeshDesc) -> usize {
        let key = format!("{desc:?}");
        if let Some(&index) = self.cache.get(&key) {
            return index;
        }
        self.meshes.push(desc.build());
        self.cache.insert(key, self.meshes.len() - 1);
        self.meshes.len() - 1
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
    fn sandbox_scene_parses() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/scenes/sandbox.ron");
        if Path::new(path).exists() {
            Scene::load(path).unwrap();
        }
    }
}
