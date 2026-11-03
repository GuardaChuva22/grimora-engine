//! Characters that move on their own, as written in scene files. The engine
//! brings them to life in `grimora::npc`.

use grimora_core::Vec3;
use serde::{Deserialize, Serialize};

/// Makes an object a character: it walks, keeps to its [`Behavior`] and reacts
/// to the player. Its collider is always a capsule of this size.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NpcDesc {
    /// Meters per second.
    pub walk_speed: f32,
    pub run_speed: f32,
    pub radius: f32,
    pub height: f32,
    /// Speeds the model's `walk` and `run` clips were made for, so the feet keep
    /// up at any pace. Without it, `walk_speed` and `run_speed`.
    pub clip_speeds: Option<(f32, f32)>,
    /// Comes before the names of its `idle`, `walk` and `run` clips: `sword_` for
    /// one with a sword in hand.
    pub clip_prefix: String,
    pub behavior: Behavior,
    pub notice: Option<Notice>,
}

impl Default for NpcDesc {
    fn default() -> Self {
        Self {
            walk_speed: 2.0,
            run_speed: 4.5,
            radius: 0.35,
            height: 1.8,
            clip_speeds: None,
            clip_prefix: String::new(),
            behavior: Behavior::Stand,
            notice: None,
        }
    }
}

impl NpcDesc {
    /// Places what the description holds on the ground: `y` is measured from it,
    /// as for objects.
    pub(crate) fn resolve(&self, ground: impl Fn(f32, f32) -> f32) -> Self {
        let mut npc = self.clone();
        if let Behavior::Patrol { points, .. } = &mut npc.behavior {
            for point in points {
                point.y += ground(point.x, point.z);
            }
        }
        npc
    }
}

/// What a character does while nothing has its attention.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub enum Behavior {
    /// Stays where it is, coming back if something took it away.
    #[default]
    Stand,
    /// Walks to random places around where it started.
    Wander {
        #[serde(default = "default_wander_radius")]
        radius: f32,
        /// Seconds of rest between walks: (shortest, longest).
        #[serde(default = "default_wander_pause")]
        pause: (f32, f32),
    },
    /// Walks these places in a loop.
    Patrol {
        points: Vec<Vec3>,
        /// Seconds of rest at each.
        #[serde(default)]
        pause: f32,
    },
}

fn default_wander_radius() -> f32 {
    6.0
}

fn default_wander_pause() -> (f32, f32) {
    (1.0, 4.0)
}

/// When a character notices the player, and what it does about it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Notice {
    /// It must see the player this close. It loses interest at 1.5 times this,
    /// or some seconds after losing sight of them.
    pub radius: f32,
    pub reaction: Reaction,
}

impl Default for Notice {
    fn default() -> Self {
        Self { radius: 6.0, reaction: Reaction::Watch }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub enum Reaction {
    /// Stops and turns to face the player.
    #[default]
    Watch,
    /// Comes to `distance` meters from the player.
    Follow {
        #[serde(default = "default_follow_distance")]
        distance: f32,
    },
    /// Runs away.
    Flee,
    /// Goes after the player and strikes when in reach. Needs a `weapon` under
    /// the object's `combat`; without one it only watches.
    Attack,
}

fn default_follow_distance() -> f32 {
    2.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Scene, SceneDesc};

    #[test]
    fn npcs_parse_with_defaults_and_stand_on_the_ground() {
        let scene = Scene::build(
            &SceneDesc::parse(
                r#"(terrain: (flat_radius: 100.0, edge_rise: 0.0, height: 3.0),
                    objects: [
                        (name: "a", mesh: Cube),
                        (name: "b", mesh: Cube, npc: (behavior: Wander())),
                        (name: "c", mesh: Cube, position: (2.0, 0.0, 0.0), npc: (
                            walk_speed: 3.0,
                            clip_speeds: (4.5, 8.5),
                            behavior: Patrol(points: [(1.0, 0.5, 2.0), (3.0, 0.0, 4.0)], pause: 2.0),
                            notice: (radius: 9.0, reaction: Follow(distance: 3.0)),
                        )),
                    ])"#,
            )
            .unwrap(),
        );
        let object = |name: &str| scene.objects.iter().find(|o| o.name.as_deref() == Some(name)).unwrap();
        assert!(object("a").npc.is_none());
        assert!(object("a").collider.is_some());

        let b = object("b");
        assert!(b.collider.is_none(), "npcs bring their own capsule");
        assert_eq!(
            b.npc.as_ref().unwrap().behavior,
            Behavior::Wander { radius: default_wander_radius(), pause: default_wander_pause() }
        );

        let c = object("c").npc.as_ref().unwrap();
        assert_eq!(c.clip_speeds, Some((4.5, 8.5)));
        assert_eq!(c.notice.as_ref().unwrap().reaction, Reaction::Follow { distance: 3.0 });
        let Behavior::Patrol { points, pause } = &c.behavior else { panic!("{:?}", c.behavior) };
        assert_eq!(*pause, 2.0);
        let ground = scene.ground_height(1.0, 2.0);
        assert!((points[0].y - (ground + 0.5)).abs() < 1e-4, "{} over {ground}", points[0].y);
    }
}
