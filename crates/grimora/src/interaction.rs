//! How the player meets the world: [`Trigger`] areas that report going in and
//! out, and [`Interactable`] objects used with a key. The game passes where the
//! player is and decides what each one does.

use std::collections::{BTreeMap, BTreeSet};

use grimora_core::{Transform, Vec3};
use grimora_physics::Physics;
use grimora_scene::{Interactable, Trigger};
use hecs::Entity;
use serde::{Deserialize, Serialize};

use crate::World;
use crate::combat::Health;
use crate::folk::Villager;
use crate::house::Door;
use crate::items::{Container, Pickup};
use crate::npc::Agent;
use crate::world::{Body, Collider, Name, Renderable};

/// The most things [`World::interactables_near`] looks at, nearest first: each costs a ray.
const MOST_NEARBY: usize = 16;
/// Meters over the top of a thing that its mark floats.
const MARK_OVER: f32 = 0.22;

/// Meters short of an object that the line of sight to it stops.
const SIGHT_MARGIN: f32 = 0.25;

/// What [`World::update_triggers`] noticed this frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerEvent {
    /// The player went into the area with this name.
    Enter(String),
    /// The player left it, or it was unloaded around them. `once` triggers never exit.
    Exit(String),
}

/// Trigger names, not entities: a trigger in a region is a new entity each time
/// the region loads, but it must not fire again. Saved games keep this.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriggerState {
    /// Areas the player is in.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub inside: BTreeSet<String>,
    /// `once` triggers that already fired. Remove a name to arm it again.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub fired: BTreeSet<String>,
}

/// The object the player could use right now (see [`World::interaction_target`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Interaction {
    pub entity: Entity,
    /// The entity's [`Name`], if it has one.
    pub name: Option<String>,
    pub prompt: String,
}

/// What sort of thing an [`Interactable`] is, for the game to draw its mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InteractKind {
    /// A [`Pickup`]: something to take.
    Take,
    /// A [`Villager`] or anyone else with an [`Agent`].
    Talk,
    Door,
    /// A [`Container`].
    Chest,
    Use,
}

/// An [`Interactable`] near the player (see [`World::interactables_near`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Nearby {
    pub entity: Entity,
    pub name: Option<String>,
    pub prompt: String,
    pub kind: InteractKind,
    /// Where its mark goes, world: just over the top of it (a door: its middle).
    pub mark: Vec3,
    /// Within its own `radius` of the player: it could be used now.
    pub in_reach: bool,
    /// Lower is a better pick among those in reach: nearer the middle of the screen, then nearer.
    pub score: f32,
    /// A house's: taking or opening it is theft.
    pub owned: bool,
}

impl World {
    /// Every [`Interactable`] that is alive, shown, within `reach` of `from` (the middle of the
    /// player's body) and not behind anything solid, nearest first, at most [`MOST_NEARBY`].
    /// `eye` and `forward` are the camera's, which make [`Nearby::score`].
    pub fn interactables_near(
        &mut self,
        from: Vec3,
        eye: Vec3,
        forward: Vec3,
        reach: f32,
        physics: &Physics,
    ) -> Vec<Nearby> {
        let forward = forward.normalize_or_zero();
        let mut found: Vec<(f32, Entity, f32, Option<usize>)> = self
            .ecs
            .query::<(Entity, &Interactable, &Transform, Option<&Renderable>, Option<&Health>)>()
            .iter()
            .filter(|(_, _, _, renderable, health)| {
                renderable.is_none_or(|r| r.visible) && health.is_none_or(|h| !h.is_dead())
            })
            .filter_map(|(entity, interactable, transform, renderable, _)| {
                let distance = transform.position.distance(from);
                (distance <= reach.max(interactable.radius))
                    .then(|| (distance, entity, interactable.radius, renderable.map(|r| r.mesh.0)))
            })
            .collect();
        found.sort_by(|a, b| a.0.total_cmp(&b.0));
        found.truncate(MOST_NEARBY);

        let mut out = Vec::with_capacity(found.len());
        for (distance, entity, radius, mesh) in found {
            let Some(transform) = self.transform(entity) else { continue };
            let bounds = mesh.and_then(|mesh| self.bounds_of(mesh));
            let (middle, top) = match bounds {
                Some((lo, hi)) => {
                    let matrix = transform.matrix();
                    let corners = (0..8).map(|i| {
                        let pick = |bit: usize, a: f32, b: f32| if i & bit == 0 { a } else { b };
                        matrix.transform_point3(Vec3::new(
                            pick(1, lo.x, hi.x),
                            pick(2, lo.y, hi.y),
                            pick(4, lo.z, hi.z),
                        ))
                    });
                    let top = corners.fold(f32::MIN, |top, corner| top.max(corner.y));
                    (matrix.transform_point3((lo + hi) * 0.5), top)
                }
                None => (transform.position, transform.position.y + 0.2),
            };
            if !self.sees(from, middle, entity, physics) {
                continue;
            }
            let kind = if self.get::<Door>(entity).is_some() {
                InteractKind::Door
            } else if self.get::<Villager>(entity).is_some() || self.get::<Agent>(entity).is_some() {
                InteractKind::Talk
            } else if self.get::<Pickup>(entity).is_some() {
                InteractKind::Take
            } else if self.get::<Container>(entity).is_some() {
                InteractKind::Chest
            } else {
                InteractKind::Use
            };
            let key = self
                .get::<Pickup>(entity)
                .map(|p| p.key.clone())
                .or_else(|| self.get::<Container>(entity).map(|c| c.key.clone()));
            let mark = match kind {
                InteractKind::Door => middle,
                _ => Vec3::new(middle.x, top + MARK_OVER, middle.z),
            };
            let off_middle = 1.0 - (middle - eye).normalize_or_zero().dot(forward);
            out.push(Nearby {
                entity,
                name: self.get::<Name>(entity).map(|n| n.0.clone()),
                prompt: self.get::<Interactable>(entity).map(|i| i.prompt.clone()).unwrap_or_default(),
                kind,
                mark,
                in_reach: distance <= radius,
                score: off_middle * 6.0 + distance * 0.4,
                owned: key.is_some_and(|key| key.starts_with("house:")),
            });
        }
        out
    }

    /// A mesh's bounds, measured the first time they're asked for.
    pub(crate) fn bounds_of(&mut self, mesh: usize) -> Option<(Vec3, Vec3)> {
        if let Some(bounds) = self.mesh_bounds.get(&mesh) {
            return Some(*bounds);
        }
        let bounds = self.meshes.get(mesh)?.bounds()?;
        self.mesh_bounds.insert(mesh, bounds);
        Some(bounds)
    }

    /// Checks `point` (usually the center of the player's body) against every
    /// [`Trigger`] and returns what changed since the last call, exits first.
    /// Call it once per frame from `Game::update`.
    pub fn update_triggers(&mut self, point: Vec3) -> Vec<TriggerEvent> {
        // Name to `once`. Triggers sharing a name count as one area.
        let mut now = BTreeMap::new();
        for (transform, trigger) in self.ecs.query::<(&Transform, &Trigger)>().iter() {
            if !self.trigger_state.fired.contains(&trigger.name) && trigger.contains(transform, point) {
                let once = now.entry(trigger.name.clone()).or_insert(trigger.once);
                *once |= trigger.once;
            }
        }

        let state = &mut self.trigger_state;
        let mut events = Vec::new();
        state.inside.retain(|name| {
            let stay = now.contains_key(name);
            if !stay {
                events.push(TriggerEvent::Exit(name.clone()));
            }
            stay
        });
        for (name, once) in now {
            if once {
                state.fired.insert(name.clone());
                events.push(TriggerEvent::Enter(name));
            } else if !state.inside.contains(&name) {
                state.inside.insert(name.clone());
                events.push(TriggerEvent::Enter(name));
            }
        }
        events
    }

    /// The [`Interactable`] the player at `from` (usually the center of their
    /// body) would use: within its `radius` of the object's origin, visible and
    /// not behind anything solid. The nearest wins, favoring those along
    /// `forward` (usually the camera's direction).
    pub fn interaction_target(&self, from: Vec3, forward: Vec3, physics: &Physics) -> Option<Interaction> {
        let forward = forward.normalize_or_zero();
        let mut query = self.ecs.query::<(Entity, &Interactable, &Transform, Option<&Renderable>, Option<&Health>)>();
        let mut candidates: Vec<(f32, Entity)> = query
            .iter()
            .filter(|(_, _, _, renderable, health)| {
                renderable.is_none_or(|r| r.visible) && health.is_none_or(|h| !h.is_dead())
            })
            .filter_map(|(entity, interactable, transform, _, _)| {
                let to = transform.position - from;
                let distance = to.length();
                // Straight behind counts as twice as far as straight ahead.
                (distance <= interactable.radius)
                    .then(|| (distance * (3.0 - to.normalize_or_zero().dot(forward)), entity))
            })
            .collect();
        candidates.sort_by(|a, b| a.0.total_cmp(&b.0));

        let (_, entity) = candidates.into_iter().find(|&(_, entity)| self.in_sight(from, entity, physics))?;
        Some(Interaction {
            entity,
            name: self.get::<Name>(entity).map(|n| n.0.clone()),
            prompt: self.get::<Interactable>(entity)?.prompt.clone(),
        })
    }

    /// Whether a ray from `from` to the middle of `entity` reaches it, or hits nothing but it.
    fn in_sight(&self, from: Vec3, entity: Entity, physics: &Physics) -> bool {
        let Some(transform) = self.transform(entity) else { return false };
        let mesh = self.get::<Renderable>(entity).and_then(|r| self.mesh(r.mesh));
        let target = mesh.map_or(transform.position, |m| m.bounding_sphere().transformed(&transform).center);
        self.sees(from, target, entity, physics)
    }

    /// Whether a ray from `from` to `target`, on `entity`, hits nothing but it.
    fn sees(&self, from: Vec3, target: Vec3, entity: Entity, physics: &Physics) -> bool {
        let own = match (self.get::<Collider>(entity), self.get::<Body>(entity)) {
            (Some(collider), _) => Some(collider.0),
            (_, Some(body)) => physics.body_collider(body.0),
            _ => self.get::<Agent>(entity).map(|agent| agent.collider()),
        };
        // Stops short of it: something lying on the ground is about as far as the ground.
        let reach = (from.distance(target) - SIGHT_MARGIN).max(0.0);
        physics.raycast(from, target - from, reach).is_none_or(|hit| Some(hit.collider) == own)
    }
}
