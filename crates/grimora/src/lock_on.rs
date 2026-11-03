//! Who a character locks its camera onto. The camera itself is
//! `ThirdPersonController::lock_target`; holding the lock is the game's business.

use grimora_core::{Camera, Transform, Vec3};
use grimora_physics::Physics;
use hecs::Entity;

use crate::World;
use crate::combat::{Fighter, Health};

/// How far away a lock can start, in meters.
const LOCK_RANGE: f32 = 16.0;
/// How far away a lock holds.
const HOLD_RANGE: f32 = 22.0;
/// Cosine of the widest angle from where the camera looks at which a lock can start.
const LOCK_CONE: f32 = 0.5;
/// Where on a character the camera looks, up from the feet.
const LOCK_HEIGHT: f32 = 1.25;

impl World {
    fn lock_candidates<'a>(
        &'a self,
        from: Vec3,
        team: u32,
        physics: &'a Physics,
    ) -> impl Iterator<Item = (Entity, Vec3)> + 'a {
        let list: Vec<(Entity, Vec3)> = self
            .ecs
            .query::<(Entity, &Transform, &Health, &Fighter)>()
            .iter()
            .filter(|&(_, _, health, _)| !health.is_dead() && health.team != team)
            .map(|(entity, transform, _, _)| (entity, transform.position + Vec3::Y * LOCK_HEIGHT))
            .collect();
        list.into_iter().filter(move |&(_, point)| {
            let to = point - from;
            to.length() <= LOCK_RANGE && physics.raycast_scenery(from, to, to.length()).is_none()
        })
    }

    /// The enemy of `team` nearest to where `camera` looks, seen from `from`.
    pub fn find_lock_target(&self, camera: &Camera, from: Vec3, team: u32, physics: &Physics) -> Option<Entity> {
        let forward = camera.forward();
        self.lock_candidates(from, team, physics)
            .filter_map(|(entity, point)| {
                let to = point - camera.position;
                let facing = forward.dot(to.normalize_or_zero());
                (facing >= LOCK_CONE).then_some((entity, (1.0 - facing) * 3.0 + (point - from).length() / LOCK_RANGE))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(entity, _)| entity)
    }

    /// The next enemy to one side of `current`: the camera's right if `side` is
    /// positive. `None` if there is nobody there.
    pub fn next_lock_target(
        &self,
        current: Entity,
        camera: &Camera,
        from: Vec3,
        team: u32,
        side: f32,
        physics: &Physics,
    ) -> Option<Entity> {
        let right = camera.right() * side.signum();
        let forward = camera.forward();
        let across = |point: Vec3| {
            let to = (point - camera.position).normalize_or_zero();
            (forward.dot(to) > 0.0).then_some(right.dot(to))
        };
        let here = across(self.lock_point(current)?).unwrap_or(0.0);
        self.lock_candidates(from, team, physics)
            .filter(|&(entity, _)| entity != current)
            .filter_map(|(entity, point)| across(point).map(|x| (entity, x - here)))
            .filter(|&(_, x)| x > 0.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(entity, _)| entity)
    }

    /// Where the camera looks on a locked character.
    pub fn lock_point(&self, target: Entity) -> Option<Vec3> {
        Some(self.transform(target)?.position + Vec3::Y * LOCK_HEIGHT)
    }

    /// Whether a lock on `target` from `from` still holds: alive and near enough.
    pub fn lock_holds(&self, target: Entity, from: Vec3) -> bool {
        self.get::<Health>(target).is_some_and(|health| !health.is_dead())
            && self.lock_point(target).is_some_and(|point| point.distance(from) <= HOLD_RANGE)
    }
}
