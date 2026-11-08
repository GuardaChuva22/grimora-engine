//! A shot: a straight line, right now, that hurts the first thing on it. What it looks and
//! sounds like is up to the game, which gets a [`Shot`] back to show it.

use glam::Vec3;
use hecs::Entity;

use grimora_physics::{ColliderHandle, Physics};

use crate::combat::Health;
use crate::npc::Agent;
use crate::world::{Body, Collider, World};

/// Meters between looks for the surface of the water, along a shot.
const WATER_STEP: f32 = 1.5;
/// How wide a shot is, for animals, which are small and have no colliders.
const ANIMAL_MARGIN: f32 = 0.1;

/// What a shot is, for the world.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShotDesc {
    pub range: f32,
    pub damage: f32,
    /// Meters of full damage; from here on it fades, down to half at `range`.
    pub full_damage_to: f32,
    /// How hard it shoves what it hits, if that can move, in kilogram meters per second.
    pub impulse: f32,
    /// Doesn't hit this one, nor anyone on its team.
    pub shooter: Option<Entity>,
}

impl ShotDesc {
    fn damage_at(&self, distance: f32) -> f32 {
        let span = (self.range - self.full_damage_to).max(0.01);
        self.damage * (1.0 - ((distance - self.full_damage_to) / span).clamp(0.0, 1.0) * 0.5)
    }
}

/// What a shot ran into.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShotTarget {
    /// It went as far as it goes.
    Nothing,
    Water,
    /// Something that stays where it is.
    Scenery,
    /// Something that moves, or has a health but took no harm from this.
    Thing(Entity),
    /// Someone it hurt: with `killed` and no `die` clip, it's already gone.
    Hurt {
        target: Entity,
        damage: f32,
        killed: bool,
    },
}

/// Where a shot ended, and what happened on the way.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shot {
    pub end: Vec3,
    /// Facing away from what it hit, or back along the shot if it hit nothing.
    pub normal: Vec3,
    pub target: ShotTarget,
    /// Animals dead on the way.
    pub animals: usize,
}

impl World {
    /// Fires along a line, from `from` and in `direction`.
    pub fn shoot(&mut self, physics: &mut Physics, from: Vec3, direction: Vec3, desc: ShotDesc) -> Shot {
        let direction = direction.normalize_or_zero();
        let mut hit = physics.raycast(from, direction, desc.range);
        // Its own body is in the way, or a friend is: go on from just past them.
        for _ in 0..4 {
            let Some(found) = hit else { break };
            let owner = self.collider_owner(found.collider, physics);
            if !owner.is_some_and(|owner| self.spared_by(owner, desc.shooter)) {
                break;
            }
            let past = found.distance + 0.05;
            hit = physics.raycast(from + direction * past, direction, desc.range - past).map(|mut further| {
                further.distance += past;
                further
            });
        }
        let reach = hit.map_or(desc.range, |hit| hit.distance);
        let water = self.water_crossing(from, direction, reach);
        let distance = water.unwrap_or(reach);
        let end = from + direction * distance;
        let animals = self.strike_fauna_by(&mut |at, size| {
            let along = (at - from).dot(direction).clamp(0.0, distance);
            at.distance(from + direction * along) < size + ANIMAL_MARGIN
        });
        if water.is_some() {
            self.splash(end, 0.6);
            return Shot { end, normal: Vec3::Y, target: ShotTarget::Water, animals };
        }
        let Some(hit) = hit else {
            return Shot { end, normal: -direction, target: ShotTarget::Nothing, animals };
        };
        // A shot from inside something has no surface to tell about.
        let normal = if hit.normal.length_squared() > 0.5 { hit.normal } else { -direction };
        let owner = self.collider_owner(hit.collider, physics);
        let Some(owner) = owner else {
            return Shot { end, normal, target: ShotTarget::Scenery, animals };
        };
        if let Some(body) = self.get::<Body>(owner).map(|body| body.0) {
            physics.add_impulse_at(body, direction * desc.impulse, end);
        }
        let damage = desc.damage_at(distance);
        let target = if self.get::<Health>(owner).is_some() {
            match self.damage(owner, damage, physics) {
                Some(killed) => ShotTarget::Hurt { target: owner, damage, killed },
                None => ShotTarget::Thing(owner),
            }
        } else if self.get::<Body>(owner).is_some() {
            ShotTarget::Thing(owner)
        } else {
            ShotTarget::Scenery
        };
        Shot { end, normal, target, animals }
    }

    /// Whichever entity has this collider, if any.
    fn collider_owner(&self, collider: ColliderHandle, physics: &Physics) -> Option<Entity> {
        for (entity, agent, fixed, body) in
            self.ecs.query::<(Entity, Option<&Agent>, Option<&Collider>, Option<&Body>)>().iter()
        {
            let found = agent.is_some_and(|agent| agent.collider() == collider)
                || fixed.is_some_and(|fixed| fixed.0 == collider)
                || body.is_some_and(|body| physics.body_collider(body.0) == Some(collider));
            if found {
                return Some(entity);
            }
        }
        None
    }

    /// Whether a shot passes through `owner`: it's the one firing, or on its team.
    fn spared_by(&self, owner: Entity, shooter: Option<Entity>) -> bool {
        let Some(shooter) = shooter else { return false };
        if owner == shooter {
            return true;
        }
        let team = |entity| self.get::<Health>(entity).map(|health| health.team);
        team(owner).is_some() && team(owner) == team(shooter)
    }

    /// How far along the line it first goes under water, if it starts above it.
    fn water_crossing(&self, from: Vec3, direction: Vec3, reach: f32) -> Option<f32> {
        let under = |point: Vec3| self.water_around(point).is_some_and(|water| point.y < water.level);
        if under(from) {
            return None;
        }
        let mut before = 0.0;
        while before < reach {
            let at = (before + WATER_STEP).min(reach);
            if under(from + direction * at) {
                let (mut low, mut high) = (before, at);
                for _ in 0..6 {
                    let middle = (low + high) * 0.5;
                    if under(from + direction * middle) {
                        high = middle;
                    } else {
                        low = middle;
                    }
                }
                return Some(high);
            }
            before = at;
        }
        None
    }
}
