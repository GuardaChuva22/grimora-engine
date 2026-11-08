//! 3D pictures inside the [`Ui`](crate::render::Ui): someone's face in a frame or their bust over
//! a dialogue box, posed as they are right now. The game asks for them each frame, in
//! [`Context::portraits`](crate::Context::portraits), with room from
//! [`Ui::reserve_image`](crate::render::Ui::reserve_image).

use grimora_core::{Animator, Camera, Environment, Mat4, Transform, Vec3};
use grimora_render::{DrawItem, ImageId, MeshHandle, Portrait};
use hecs::Entity;

use crate::world::{Attached, MeshId, Renderable, World, Worn, push_skin};

/// What a portrait shows.
#[derive(Debug, Clone, Copy)]
pub enum Sitter {
    /// As it stands in the world, with what's attached to it.
    Entity(Entity),
    /// A mesh on its own, in its rest pose.
    Mesh(MeshId, Transform),
}

/// See [`Context::portraits`](crate::Context::portraits).
#[derive(Debug, Clone)]
pub struct PortraitShot {
    pub image: ImageId,
    pub sitter: Sitter,
    pub camera: Camera,
    pub environment: Environment,
}

impl PortraitShot {
    /// `image` showing `sitter` from `camera`, lit from the side of the camera.
    pub fn new(image: ImageId, sitter: Sitter, camera: Camera) -> Self {
        Self { image, sitter, camera, environment: portrait_light(&camera) }
    }

    pub(crate) fn resolve<'a>(&'a self, world: &World, handles: &[MeshHandle]) -> Option<Portrait<'a>> {
        let mut draws = Vec::new();
        let mut joint_matrices: Vec<Mat4> = Vec::new();
        match self.sitter {
            Sitter::Entity(entity) => {
                let mut push = |entity: Entity| {
                    let mut query =
                        world.ecs.query_one::<(&Transform, &Renderable, Option<&Animator>, Option<&Worn>)>(entity);
                    let (transform, renderable, animator, worn) = query.get().ok()?;
                    let mesh = *handles.get(renderable.mesh.0)?;
                    let joints = push_skin(&world.ecs, animator, worn, &mut joint_matrices);
                    draws.push(DrawItem {
                        mesh,
                        transform: *transform,
                        color: renderable.color,
                        emissive: renderable.emissive,
                        joints,
                    });
                    Some(())
                };
                push(entity)?;
                let attached: Vec<Entity> = world
                    .ecs
                    .query::<(Entity, &Attached)>()
                    .iter()
                    .filter(|(_, attached)| attached.parent == entity)
                    .map(|(child, _)| child)
                    .collect();
                let worn: Vec<Entity> = world
                    .ecs
                    .query::<(Entity, &Worn)>()
                    .iter()
                    .filter(|(_, worn)| worn.parent == entity)
                    .map(|(child, _)| child)
                    .collect();
                for child in attached.into_iter().chain(worn) {
                    push(child);
                }
            }
            Sitter::Mesh(mesh, transform) => {
                let mesh = *handles.get(mesh.0)?;
                draws.push(DrawItem { mesh, transform, color: Vec3::ONE, emissive: 0.0, joints: None });
            }
        }
        Some(Portrait { image: self.image, camera: self.camera, environment: &self.environment, draws, joint_matrices })
    }
}

/// The light of a portrait: warm from the upper left of the camera and cold from below, with no
/// fog or shadows, whatever the time and the weather are where the sitter stands.
pub fn portrait_light(camera: &Camera) -> Environment {
    let (forward, right) = (camera.forward(), camera.right());
    let mut environment = Environment::default();
    environment.shadows.enabled = false;
    environment.fog.density = 0.0;
    environment.fog.sun_scatter = 0.0;
    environment.sun.direction = (-right * 0.7 + Vec3::Y * 0.65 - forward * 0.55).normalize();
    environment.sun.color = Vec3::new(1.0, 0.86, 0.68);
    environment.sun.intensity = 2.2;
    environment.ambient.sky = Vec3::new(0.46, 0.44, 0.72);
    environment.ambient.ground = Vec3::new(0.3, 0.2, 0.34);
    environment.ambient.intensity = 0.85;
    environment.post.exposure = 1.0;
    environment
}

/// A camera `distance` meters in front of `point`, on the side the `yaw` faces, looking back at
/// it. A narrow lens, which keeps a face from bulging.
pub fn camera_facing(point: Vec3, yaw: f32, distance: f32, fov_degrees: f32) -> Camera {
    let front = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
    let position = point + front * distance;
    let mut camera = Camera::new(position, yaw, 0.0);
    camera.fov_y = fov_degrees.to_radians();
    camera.near = 0.05;
    camera.far = 50.0;
    camera.look_at(point);
    camera
}

/// Which way an entity is looking, as a yaw.
pub fn yaw_of(transform: &Transform) -> f32 {
    let forward = transform.rotation * Vec3::NEG_Z;
    (-forward.x).atan2(-forward.z)
}
