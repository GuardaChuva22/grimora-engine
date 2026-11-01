//! Collision and character movement for Grimora, built on Rapier.
//!
//! [`Physics`] holds the collision world (built from a scene's colliders),
//! simulates dynamic bodies and answers queries; [`CharacterBody`] is a capsule
//! that walks, slides along walls, climbs steps and slopes, snaps to the ground
//! and pushes dynamic bodies out of its way.

use grimora_core::{Quat, Transform, Vec3};
use grimora_scene::{BodyDesc, ColliderShape, Scene};
use rapier3d::control::{CharacterAutostep, CharacterCollision, CharacterLength, KinematicCharacterController};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::parry::query::Ray;
use rapier3d::parry::shape::Capsule;
use rapier3d::prelude::{ColliderBuilder, PhysicsWorld, QueryFilter, RigidBodyBuilder, RigidBodyHandle};

pub use rapier3d;
pub use rapier3d::prelude::ColliderHandle;

/// Builds the collider for `shape` with `transform`'s scale baked in, plus its
/// offset from the object's origin in the object's (unrotated) frame.
fn collider(shape: &ColliderShape, transform: &Transform) -> Option<(ColliderBuilder, Vec3)> {
    let (builder, center) = match shape.scaled(transform) {
        ColliderShape::Cuboid { center, half_extents: h } => (Some(ColliderBuilder::cuboid(h.x, h.y, h.z)), center),
        ColliderShape::Cylinder { center, radius, half_height } => {
            (Some(ColliderBuilder::cylinder(half_height, radius)), center)
        }
        ColliderShape::Ball { center, radius } => (Some(ColliderBuilder::ball(radius)), center),
        ColliderShape::ConvexHull { points } => {
            let points: Vec<Vector> = points.into_iter().map(to_rapier).collect();
            (ColliderBuilder::convex_hull(&points), Vec3::ZERO)
        }
        ColliderShape::TriMesh { vertices, indices } => {
            let vertices = vertices.into_iter().map(to_rapier).collect();
            let builder = ColliderBuilder::trimesh(vertices, indices)
                .inspect_err(|e| log::warn!("skipping triangle mesh collider: {e}"))
                .ok();
            (builder, Vec3::ZERO)
        }
    };
    if builder.is_none() {
        log::warn!("skipping degenerate collider at {}", transform.position);
    }
    Some((builder?, center))
}

/// Rapier uses its own copy of glam, so values cross the boundary as plain floats.
fn to_rapier(v: Vec3) -> Vector {
    Vector::new(v.x, v.y, v.z)
}

fn from_rapier(v: Vector) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

fn rotation(q: Quat) -> Rotation {
    Rotation::from_xyzw(q.x, q.y, q.z, q.w)
}

/// A ray hit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayHit {
    pub collider: ColliderHandle,
    pub distance: f32,
    pub point: Vec3,
    pub normal: Vec3,
}

/// A body moved by the simulation, from [`Physics::add_body`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BodyHandle(RigidBodyHandle);

/// A dynamic body created for a scene object by [`Physics::from_scene`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneBody {
    /// Index into `Scene::objects` (the engine's `World::objects` keeps the same order).
    pub object: usize,
    pub body: BodyHandle,
}

/// The collision world.
pub struct Physics {
    world: PhysicsWorld,
    accumulator: f32,
    scene_bodies: Vec<SceneBody>,
}

impl Default for Physics {
    fn default() -> Self {
        Self::new()
    }
}

impl Physics {
    /// Simulation steps run at a fixed rate, independent of the frame rate.
    pub const FIXED_DT: f32 = 1.0 / 60.0;
    const MAX_STEPS_PER_FRAME: u32 = 4;

    pub fn new() -> Self {
        let mut world = PhysicsWorld::new();
        world.integration_parameters.dt = Self::FIXED_DT;
        world.gravity = Vector::new(0.0, -9.81, 0.0);
        Self { world, accumulator: 0.0, scene_bodies: Vec::new() }
    }

    /// Adds every collider of `scene`: static geometry, or a dynamic body for
    /// objects marked `body: Dynamic(...)` (listed in [`Physics::scene_bodies`]).
    pub fn from_scene(scene: &Scene) -> Self {
        let mut physics = Self::new();
        for (index, object) in scene.objects.iter().enumerate() {
            match &object.collider {
                Some(shape) => {
                    if let Some(body) = physics.add_body(shape, &object.transform, &object.body) {
                        physics.scene_bodies.push(SceneBody { object: index, body });
                    }
                }
                None if object.body.is_dynamic() => {
                    log::warn!("object {:?} is dynamic but has no collider; it will not move", object.name);
                }
                None => {}
            }
        }
        physics.refresh();
        physics
    }

    /// Adds immovable geometry. Queries see it after the next [`Physics::step`]
    /// or [`Physics::refresh`].
    pub fn add_static(&mut self, shape: &ColliderShape, transform: &Transform) -> Option<ColliderHandle> {
        let (builder, center) = collider(shape, transform)?;
        let position = transform.position + transform.rotation * center;
        let pose = Pose::from_parts(to_rapier(position), rotation(transform.rotation));
        Some(self.world.insert_collider(builder.position(pose), None))
    }

    /// Adds an object's collider moving as `body` says. Static bodies become fixed
    /// geometry (like [`Physics::add_static`]) and return `None`; dynamic ones return
    /// a handle for reading their pose back. Triangle meshes can't be dynamic, so
    /// they fall back to their convex hull.
    pub fn add_body(&mut self, shape: &ColliderShape, transform: &Transform, body: &BodyDesc) -> Option<BodyHandle> {
        let BodyDesc::Dynamic { density, mass, friction, restitution, angular_damping } = *body else {
            self.add_static(shape, transform);
            return None;
        };
        let hull;
        let shape = match shape {
            ColliderShape::TriMesh { vertices, .. } => {
                log::warn!("dynamic body at {} can't use a triangle mesh; using its convex hull", transform.position);
                hull = ColliderShape::ConvexHull { points: vertices.clone() };
                &hull
            }
            shape => shape,
        };
        let (builder, center) = collider(shape, transform)?;
        // The body sits at the object's origin, so its pose is the render transform.
        let builder = builder.translation(to_rapier(center)).friction(friction).restitution(restitution);
        let builder = match mass {
            Some(mass) => builder.mass(mass),
            None => builder.density(density),
        };
        let pose = Pose::from_parts(to_rapier(transform.position), rotation(transform.rotation));
        let (handle, _) =
            self.world.insert(RigidBodyBuilder::dynamic().pose(pose).angular_damping(angular_damping), builder);
        Some(BodyHandle(handle))
    }

    /// Position and rotation of a dynamic body's origin (its object's origin).
    pub fn body_pose(&self, body: BodyHandle) -> Option<(Vec3, Quat)> {
        let pose = self.world.bodies.get(body.0)?.position();
        let r = pose.rotation;
        Some((from_rapier(pose.translation), Quat::from_xyzw(r.x, r.y, r.z, r.w)))
    }

    /// Linear and angular velocity of a dynamic body.
    pub fn body_velocity(&self, body: BodyHandle) -> Option<(Vec3, Vec3)> {
        let body = self.world.bodies.get(body.0)?;
        Some((from_rapier(body.linvel()), from_rapier(body.angvel())))
    }

    /// Moves a dynamic body (teleports it, as when loading a saved game) and
    /// sets its velocities. Its object follows on the next `World::sync_bodies`.
    pub fn set_body_state(&mut self, body: BodyHandle, position: Vec3, rotation: Quat, linear: Vec3, angular: Vec3) {
        let Some(body) = self.world.bodies.get_mut(body.0) else { return };
        body.set_position(Pose::from_parts(to_rapier(position), self::rotation(rotation)), true);
        body.set_linvel(to_rapier(linear), true);
        body.set_angvel(to_rapier(angular), true);
    }

    /// Dynamic bodies created by [`Physics::from_scene`], with their object indices.
    pub fn scene_bodies(&self) -> &[SceneBody] {
        &self.scene_bodies
    }

    /// Removes a dynamic body and its collider.
    pub fn remove_body(&mut self, body: BodyHandle) {
        self.world.remove_body(body.0);
        self.scene_bodies.retain(|b| b.body != body);
    }

    pub fn remove(&mut self, collider: ColliderHandle) {
        self.world.remove_collider(collider);
    }

    /// Advances the simulation by `dt` seconds in fixed steps.
    pub fn step(&mut self, dt: f32) {
        self.accumulator = (self.accumulator + dt).min(Self::FIXED_DT * Self::MAX_STEPS_PER_FRAME as f32);
        while self.accumulator >= Self::FIXED_DT {
            self.world.step();
            self.accumulator -= Self::FIXED_DT;
        }
    }

    /// Makes newly added colliders visible to queries right away.
    pub fn refresh(&mut self) {
        self.world.step();
    }

    /// First hit along `direction` (need not be normalized) within `max_distance`.
    pub fn raycast(&self, origin: Vec3, direction: Vec3, max_distance: f32) -> Option<RayHit> {
        let direction = direction.normalize_or_zero();
        if direction == Vec3::ZERO {
            return None;
        }
        let ray = Ray::new(to_rapier(origin), to_rapier(direction));
        let (collider, hit) = self.world.cast_ray_and_get_normal(&ray, max_distance, true, QueryFilter::default())?;
        Some(RayHit {
            collider,
            distance: hit.time_of_impact,
            point: origin + direction * hit.time_of_impact,
            normal: from_rapier(hit.normal),
        })
    }

    pub fn collider_count(&self) -> usize {
        self.world.colliders.len()
    }

    /// Applies the character's push to the dynamic bodies it ran into.
    fn push_bodies(&mut self, character: &CharacterBody, shape: &Capsule, collisions: &[CharacterCollision], dt: f32) {
        let world = &mut self.world;
        let mut queries = world.broad_phase.as_query_pipeline_mut(
            world.narrow_phase.query_dispatcher(),
            &mut world.bodies,
            &mut world.colliders,
            QueryFilter::default(),
        );
        character.controller.solve_character_collision_impulses(dt, &mut queries, shape, character.mass, collisions);
    }

    /// The underlying Rapier world, for anything this wrapper doesn't cover yet.
    pub fn rapier(&self) -> &PhysicsWorld {
        &self.world
    }

    pub fn rapier_mut(&mut self) -> &mut PhysicsWorld {
        &mut self.world
    }
}

/// A walking capsule. `position` is at the feet.
#[derive(Debug, Clone)]
pub struct CharacterBody {
    pub position: Vec3,
    pub radius: f32,
    pub height: f32,
    /// Whether the last move ended standing on something.
    pub grounded: bool,
    /// Kilograms; decides how hard the character shoves dynamic bodies. 0 never pushes.
    pub mass: f32,
    pub controller: KinematicCharacterController,
}

impl CharacterBody {
    pub fn new(position: Vec3, radius: f32, height: f32) -> Self {
        let controller = KinematicCharacterController {
            offset: CharacterLength::Absolute(0.02),
            autostep: Some(CharacterAutostep {
                max_height: CharacterLength::Absolute(0.4),
                min_width: CharacterLength::Absolute(0.15),
                include_dynamic_bodies: true,
            }),
            max_slope_climb_angle: 50f32.to_radians(),
            min_slope_slide_angle: 40f32.to_radians(),
            snap_to_ground: Some(CharacterLength::Absolute(0.3)),
            ..Default::default()
        };
        Self { position, radius, height, grounded: false, mass: 80.0, controller }
    }

    /// World position of the capsule's center.
    pub fn center(&self) -> Vec3 {
        self.position + Vec3::Y * self.height * 0.5
    }

    /// Tries to move by `translation`, sliding along whatever is in the way and
    /// pushing the dynamic bodies it runs into. Returns the movement actually applied.
    pub fn move_and_slide(&mut self, physics: &mut Physics, translation: Vec3, dt: f32) -> Vec3 {
        let shape = Capsule::new_y((self.height * 0.5 - self.radius).max(0.01), self.radius);
        let mut pushes = Vec::new();
        let movement = self.controller.move_shape(
            dt,
            &physics.world.query_pipeline(),
            &shape,
            &Pose::from_translation(to_rapier(self.center())),
            to_rapier(translation),
            |hit| {
                // Only colliders attached to a body can move; static ones have no parent.
                if physics.world.colliders.get(hit.handle).is_some_and(|c| c.parent().is_some()) {
                    pushes.push(hit);
                }
            },
        );
        let applied = from_rapier(movement.translation);
        self.position += applied;
        self.grounded = movement.grounded;
        if self.mass > 0.0 && !pushes.is_empty() {
            physics.push_bodies(self, &shape, &pushes, dt);
        }
        applied
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 40×40 floor whose top is at y = 0, plus whatever else the test adds.
    fn floor() -> Physics {
        let mut physics = Physics::new();
        let floor = ColliderShape::Cuboid { center: Vec3::Y * -0.5, half_extents: Vec3::new(20.0, 0.5, 20.0) };
        physics.add_static(&floor, &Transform::IDENTITY);
        physics
    }

    fn simulate(body: &mut CharacterBody, physics: &mut Physics, velocity: Vec3, seconds: f32) {
        let dt = 1.0 / 60.0;
        for _ in 0..(seconds / dt) as u32 {
            physics.step(dt);
            body.move_and_slide(physics, velocity * dt, dt);
        }
    }

    fn run(physics: &mut Physics, seconds: f32) {
        for _ in 0..(seconds * 60.0) as u32 {
            physics.step(Physics::FIXED_DT);
        }
    }

    const CUBE: ColliderShape =
        ColliderShape::Cuboid { center: Vec3::new(0.0, 0.5, 0.0), half_extents: Vec3::splat(0.5) };
    const WOOD: BodyDesc =
        BodyDesc::Dynamic { density: 500.0, mass: None, friction: 0.6, restitution: 0.0, angular_damping: 0.5 };

    /// A 0.8 m wooden crate with its base at `position`.
    fn crate_at(physics: &mut Physics, position: Vec3) -> BodyHandle {
        let transform = Transform { position, rotation: Quat::IDENTITY, scale: Vec3::splat(0.8) };
        physics.add_body(&CUBE, &transform, &WOOD).unwrap()
    }

    #[test]
    fn raycast_hits_floor() {
        let mut physics = floor();
        physics.refresh();
        let hit = physics.raycast(Vec3::new(1.0, 5.0, 2.0), Vec3::NEG_Y, 100.0).unwrap();
        assert!((hit.distance - 5.0).abs() < 1e-3);
        assert!(hit.normal.y > 0.99);
        assert!(physics.raycast(Vec3::new(1.0, 5.0, 2.0), Vec3::Y, 100.0).is_none());
    }

    #[test]
    fn character_lands_on_floor() {
        let mut physics = floor();
        physics.refresh();
        let mut body = CharacterBody::new(Vec3::new(0.0, 3.0, 0.0), 0.35, 1.8);
        simulate(&mut body, &mut physics, Vec3::new(0.0, -10.0, 0.0), 1.0);
        assert!(body.grounded);
        assert!(body.position.y.abs() < 0.05, "feet at {}", body.position.y);
    }

    #[test]
    fn wall_blocks_and_character_slides() {
        let mut physics = floor();
        let wall = ColliderShape::Cuboid { center: Vec3::Y, half_extents: Vec3::new(5.0, 1.0, 0.25) };
        physics.add_static(&wall, &Transform::from_position(Vec3::new(0.0, 0.0, -3.0)));
        physics.refresh();
        let mut body = CharacterBody::new(Vec3::ZERO, 0.35, 1.8);
        // Walk diagonally into the wall: blocked in -Z, still sliding along +X.
        simulate(&mut body, &mut physics, Vec3::new(2.0, -1.0, -4.0), 2.0);
        assert!(body.position.z > -3.0 + 0.25, "went through the wall: {}", body.position);
        assert!(body.position.x > 3.0, "did not slide: {}", body.position);
    }

    #[test]
    fn climbs_small_steps() {
        let mut physics = floor();
        let step = ColliderShape::Cuboid { center: Vec3::Y * 0.125, half_extents: Vec3::new(2.0, 0.125, 2.0) };
        physics.add_static(&step, &Transform::from_position(Vec3::new(0.0, 0.0, -4.0)));
        physics.refresh();
        let mut body = CharacterBody::new(Vec3::ZERO, 0.35, 1.8);
        simulate(&mut body, &mut physics, Vec3::new(0.0, -2.0, -3.0), 1.8);
        assert!(body.position.y > 0.2, "did not climb the step: {}", body.position);
    }

    #[test]
    fn dynamic_box_falls_and_rests() {
        let mut physics = floor();
        let tilted = Transform::from_euler_degrees(Vec3::new(1.0, 3.0, 0.0), Vec3::new(20.0, 30.0, 10.0), Vec3::ONE);
        let body = physics.add_body(&CUBE, &tilted, &WOOD).unwrap();
        run(&mut physics, 3.0);
        let (position, rotation) = physics.body_pose(body).unwrap();
        // Landed flat on a face: the box's up axis lines up with a world axis again.
        let up = rotation * Vec3::Y;
        assert!(up.abs().max_element() > 0.99, "not resting flat: up = {up}");
        let center_height = (position + rotation * Vec3::Y * 0.5).y;
        assert!((center_height - 0.5).abs() < 0.03, "center at {center_height}");
        run(&mut physics, 0.5);
        assert!(physics.body_pose(body).unwrap().0.distance(position) < 1e-3, "still moving");
    }

    #[test]
    fn stack_stays_put() {
        let mut physics = floor();
        let bodies: Vec<_> = (0..4).map(|i| crate_at(&mut physics, Vec3::new(0.0, i as f32 * 0.8, 0.0))).collect();
        run(&mut physics, 5.0);
        for (i, &body) in bodies.iter().enumerate() {
            let (position, _) = physics.body_pose(body).unwrap();
            let start = Vec3::new(0.0, i as f32 * 0.8, 0.0);
            assert!(position.distance(start) < 0.05, "crate {i} moved from {start} to {position}");
        }
    }

    #[test]
    fn triangle_mesh_bodies_fall_back_to_hull() {
        let mut physics = floor();
        let mesh = ColliderShape::TriMesh {
            vertices: vec![Vec3::ZERO, Vec3::X, Vec3::Z, Vec3::Y],
            indices: vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
        };
        let body = physics.add_body(&mesh, &Transform::from_position(Vec3::Y * 2.0), &WOOD).unwrap();
        run(&mut physics, 2.0);
        assert!(physics.body_pose(body).unwrap().0.y.abs() < 0.05);
    }

    #[test]
    fn character_pushes_crates() {
        let mut physics = floor();
        let body = crate_at(&mut physics, Vec3::new(0.0, 0.0, -2.0));
        physics.refresh();
        let mut character = CharacterBody::new(Vec3::ZERO, 0.35, 1.8);
        simulate(&mut character, &mut physics, Vec3::new(0.0, -1.0, -3.0), 2.0);
        let (position, _) = physics.body_pose(body).unwrap();
        assert!(position.z < -3.5, "crate only moved to {position}");
        assert!(position.y.abs() < 0.05, "crate left the floor: {position}");
        // The character kept walking behind it instead of getting stuck.
        assert!(character.position.z < -2.5, "character stuck at {}", character.position);
        assert!(character.position.z > position.z, "character went through the crate");
    }

    #[test]
    fn scaled_rotated_cylinder_blocks() {
        let mut physics = floor();
        let pillar = ColliderShape::Cylinder { center: Vec3::Y * 0.5, radius: 0.5, half_height: 0.5 };
        let transform = Transform::from_euler_degrees(
            Vec3::new(0.0, 0.0, -3.0),
            Vec3::new(0.0, 30.0, 0.0),
            Vec3::new(2.0, 3.0, 2.0),
        );
        physics.add_static(&pillar, &transform);
        physics.refresh();
        // Radius is 1 after scaling, so a ray from the side stops 1 unit before the axis.
        let hit = physics.raycast(Vec3::new(0.0, 1.0, 0.0), Vec3::NEG_Z, 10.0).unwrap();
        assert!((hit.distance - 2.0).abs() < 0.05, "hit at {}", hit.distance);
    }
}
