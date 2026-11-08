//! What became of the generated world's things: each has a [`GenId`] (its path, not the order
//! it was made in), and [`GenState`] keeps only what differs from what the generator makes:
//! removed (cut, broken, taken), moved, used (harvested...). When a cell comes back, its
//! differences are put on top of it again, as [`crate::ItemState`] does for streamed scenes.

use std::collections::{BTreeMap, BTreeSet};

use grimora_core::WorldTransform;
pub use grimora_world::GenId;
use serde::{Deserialize, Serialize};

/// The component of a generated entity: its address and where the generator put it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Generated {
    pub id: GenId,
    pub made: WorldTransform,
}

/// The differences from the generated world, by [`GenId`]; saved in
/// [`WorldState::generated`](crate::save::WorldState::generated).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GenState {
    /// Gone for good: despawned with [`crate::World::despawn`].
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub removed: BTreeSet<GenId>,
    /// Where those that were moved are now, in the world.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub moved: BTreeMap<GenId, WorldTransform>,
    /// Still there, but the game used them up (a bush already picked...); only the game
    /// knows what that means.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub used: BTreeSet<GenId>,
}

impl GenState {
    pub fn is_removed(&self, id: GenId) -> bool {
        self.removed.contains(&id)
    }

    pub fn is_used(&self, id: GenId) -> bool {
        self.used.contains(&id)
    }

    pub fn set_used(&mut self, id: GenId) {
        self.used.insert(id);
    }

    pub fn is_empty(&self) -> bool {
        self.removed.is_empty() && self.moved.is_empty() && self.used.is_empty()
    }

    /// Notes where `id` is if that isn't where it was made, and forgets an old move otherwise.
    pub(crate) fn note_place(&mut self, id: GenId, made: &WorldTransform, now: WorldTransform) {
        if same_place(made, &now) {
            self.moved.remove(&id);
        } else {
            self.moved.insert(id, now);
        }
    }
}

/// Within a millimeter and a hair of a degree: what an origin shift leaves of a transform.
fn same_place(a: &WorldTransform, b: &WorldTransform) -> bool {
    a.position.distance_squared(b.position) < 1e-6
        && a.rotation.dot(b.rotation).abs() > 1.0 - 1e-6
        && (a.scale - b.scale).abs().max_element() < 1e-4
}
