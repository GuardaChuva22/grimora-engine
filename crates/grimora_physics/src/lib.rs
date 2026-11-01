//! Collision and character movement for Grimora, built on Rapier.
//!
//! [`Physics`] holds the collision world (built from a scene's colliders) and
//! answers queries; [`CharacterBody`] is a capsule that walks, slides along
//! walls, climbs steps and slopes, and snaps to the ground.

use grimora_core::{Quat, Transform, Vec3};
use grimora_scene::{ColliderShape, Scene};
use rapier3d::control::{CharacterAutostep, CharacterLength, KinematicCharacterController};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::parry::query::Ray;
use rapier3d::parry::shape::Capsule;
use rapier3d::prelude::{ColliderBuilder, PhysicsWorld, QueryFilter};

pub use rapier3d;
pub use rapier3d::prelude::ColliderHandle;

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

/// The collision world.
pub struct Physics {
    world: PhysicsWorld,
    accumulator: f32,
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
        Self { world, accumulator: 0.0 }
    }

    /// Adds every collider of `scene` as static geometry.
    pub fn from_scene(scene: &Scene) -> Self {
        let mut physics = Self::new();
        for object in &scene.objects {
            if let Some(shape) = &object.collider {
                physics.add_static(shape, &object.transform);
            }
        }
        physics.refresh();
        physics
    }

    /// Adds immovable geometry. Queries see it after the next [`Physics::step`]
    /// or [`Physics::refresh`].
    pub fn add_static(&mut self, shape: &ColliderShape, transform: &Transform) -> Option<ColliderHandle> {
        let offset = |center: Vec3| transform.position + transform.rotation * center;
        let (builder, position) = match shape.scaled(transform) {
            ColliderShape::Cuboid { center, half_extents: h } => {
                (Some(ColliderBuilder::cuboid(h.x, h.y, h.z)), offset(center))
            }
            ColliderShape::Cylinder { center, radius, half_height } => {
                (Some(ColliderBuilder::cylinder(half_height, radius)), offset(center))
            }
            ColliderShape::Ball { center, radius } => (Some(ColliderBuilder::ball(radius)), offset(center)),
            ColliderShape::ConvexHull { points } => {
                let points: Vec<Vector> = points.into_iter().map(to_rapier).collect();
                (ColliderBuilder::convex_hull(&points), transform.position)
            }
            ColliderShape::TriMesh { vertices, indices } => {
                let vertices = vertices.into_iter().map(to_rapier).collect();
                let builder = ColliderBuilder::trimesh(vertices, indices)
                    .inspect_err(|e| log::warn!("skipping triangle mesh collider: {e}"))
                    .ok();
                (builder, transform.position)
            }
        };
        let Some(builder) = builder else {
            log::warn!("skipping degenerate collider at {}", transform.position);
            return None;
        };
        let pose = Pose::from_parts(to_rapier(position), rotation(transform.rotation));
        Some(self.world.insert_collider(builder.position(pose), None))
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
        Self { position, radius, height, grounded: false, controller }
    }

    /// World position of the capsule's center.
    pub fn center(&self) -> Vec3 {
        self.position + Vec3::Y * self.height * 0.5
    }

    /// Tries to move by `translation`, sliding along whatever is in the way.
    /// Returns the movement actually applied.
    pub fn move_and_slide(&mut self, physics: &Physics, translation: Vec3, dt: f32) -> Vec3 {
        let shape = Capsule::new_y((self.height * 0.5 - self.radius).max(0.01), self.radius);
        let movement = self.controller.move_shape(
            dt,
            &physics.world.query_pipeline(),
            &shape,
            &Pose::from_translation(to_rapier(self.center())),
            to_rapier(translation),
            |_| {},
        );
        let applied = from_rapier(movement.translation);
        self.position += applied;
        self.grounded = movement.grounded;
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

    fn simulate(body: &mut CharacterBody, physics: &Physics, velocity: Vec3, seconds: f32) {
        let dt = 1.0 / 60.0;
        for _ in 0..(seconds / dt) as u32 {
            body.move_and_slide(physics, velocity * dt, dt);
        }
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
        simulate(&mut body, &physics, Vec3::new(0.0, -10.0, 0.0), 1.0);
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
        simulate(&mut body, &physics, Vec3::new(2.0, -1.0, -4.0), 2.0);
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
        simulate(&mut body, &physics, Vec3::new(0.0, -2.0, -3.0), 1.8);
        assert!(body.position.y > 0.2, "did not climb the step: {}", body.position);
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
