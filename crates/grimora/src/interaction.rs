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
use crate::npc::Agent;
use crate::world::{Body, Collider, Name, Renderable};

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

impl World {
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
        let own = match (self.get::<Collider>(entity), self.get::<Body>(entity)) {
            (Some(collider), _) => Some(collider.0),
            (_, Some(body)) => physics.body_collider(body.0),
            _ => self.get::<Agent>(entity).map(|agent| agent.collider()),
        };
        physics.raycast(from, target - from, from.distance(target)).is_none_or(|hit| Some(hit.collider) == own)
    }
}
