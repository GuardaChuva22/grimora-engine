//! Areas that notice the player and objects the player can use, as plain data.
//! The engine (`grimora::World`) checks them and tells the game what happened.

use grimora_core::{Transform, Vec3};
use serde::{Deserialize, Serialize};

/// The volume of a trigger, in world units around its position (not scaled).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TriggerShape {
    /// Sphere centered on the position.
    Ball { radius: f32 },
    /// Box given by its half sizes, centered on the position and turned with it.
    Box { half_extents: Vec3 },
    /// Upright cylinder resting on the position.
    Cylinder { radius: f32, height: f32 },
}

impl TriggerShape {
    /// Whether `point`, in the trigger's local space, is inside.
    pub fn contains(&self, point: Vec3) -> bool {
        match *self {
            Self::Ball { radius } => point.length_squared() <= radius * radius,
            Self::Box { half_extents } => point.abs().cmple(half_extents).all(),
            Self::Cylinder { radius, height } => {
                (0.0..=height).contains(&point.y) && point.with_y(0.0).length_squared() <= radius * radius
            }
        }
    }
}

/// An area in the scene file; the game hears about it when the player goes in or out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TriggerDesc {
    /// How the game recognizes it. Triggers with the same name act as one area.
    pub name: String,
    pub shape: TriggerShape,
    pub position: Vec3,
    /// Euler angles in degrees, like objects. Only boxes care.
    pub rotation: Vec3,
    /// With a terrain, `position.y` is measured from the ground surface.
    pub snap_to_ground: bool,
    /// Fires the first time only, even after its region reloads or a game is loaded.
    pub once: bool,
}

impl Default for TriggerDesc {
    fn default() -> Self {
        Self {
            name: String::new(),
            shape: TriggerShape::Ball { radius: 1.0 },
            position: Vec3::ZERO,
            rotation: Vec3::ZERO,
            snap_to_ground: true,
            once: false,
        }
    }
}

/// An area as the engine checks it. On an entity it follows the [`Transform`]
/// (position and rotation, not scale).
#[derive(Debug, Clone, PartialEq)]
pub struct Trigger {
    pub name: String,
    pub shape: TriggerShape,
    pub once: bool,
}

impl Trigger {
    pub fn new(name: impl Into<String>, shape: TriggerShape) -> Self {
        Self { name: name.into(), shape, once: false }
    }

    /// Whether the world position `point` is inside the trigger placed at `transform`.
    pub fn contains(&self, transform: &Transform, point: Vec3) -> bool {
        self.shape.contains(transform.rotation.inverse() * (point - transform.position))
    }
}

/// A trigger placed in a built scene.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneTrigger {
    pub trigger: Trigger,
    pub transform: Transform,
}

impl TriggerDesc {
    /// `ground` gives the terrain height at `(x, z)`.
    pub(crate) fn resolve(&self, ground: impl Fn(f32, f32) -> f32) -> SceneTrigger {
        let mut position = self.position;
        if self.snap_to_ground {
            position.y += ground(position.x, position.z);
        }
        SceneTrigger {
            trigger: Trigger { name: self.name.clone(), shape: self.shape, once: self.once },
            transform: Transform::from_euler_degrees(position, self.rotation, Vec3::ONE),
        }
    }
}

/// Something the player can use with a key: in the scene file as an object's
/// `interact`, and on entities as a component.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Interactable {
    /// What using it does, for the game to show (e.g. `"abrir"`).
    pub prompt: String,
    /// How close the player must be to the object's origin, in meters.
    pub radius: f32,
}

impl Interactable {
    pub fn new(prompt: impl Into<String>) -> Self {
        Self { prompt: prompt.into(), ..Default::default() }
    }
}

impl Default for Interactable {
    fn default() -> Self {
        Self { prompt: String::new(), radius: 2.5 }
    }
}

#[cfg(test)]
mod tests {
    use grimora_core::Quat;

    use super::*;

    #[test]
    fn shapes_contain_points() {
        let ball = TriggerShape::Ball { radius: 2.0 };
        assert!(ball.contains(Vec3::new(0.0, 2.0, 0.0)) && !ball.contains(Vec3::new(1.5, 1.5, 0.0)));

        let cuboid = TriggerShape::Box { half_extents: Vec3::new(1.0, 2.0, 3.0) };
        assert!(cuboid.contains(Vec3::new(-1.0, 2.0, -3.0)) && !cuboid.contains(Vec3::new(0.0, 2.5, 0.0)));

        // A cylinder stands on its position, unlike the other two.
        let cylinder = TriggerShape::Cylinder { radius: 2.0, height: 3.0 };
        assert!(cylinder.contains(Vec3::new(0.0, 3.0, 2.0)));
        assert!(!cylinder.contains(Vec3::new(0.0, -0.1, 0.0)) && !cylinder.contains(Vec3::new(2.0, 1.0, 2.0)));
    }

    #[test]
    fn triggers_follow_their_transform() {
        let long = Trigger::new("hall", TriggerShape::Box { half_extents: Vec3::new(4.0, 1.0, 1.0) });
        let turned = Transform {
            position: Vec3::new(10.0, 0.0, 0.0),
            rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            scale: Vec3::splat(100.0),
        };
        assert!(long.contains(&turned, Vec3::new(10.0, 0.0, 3.0)), "the long side now runs along Z");
        assert!(!long.contains(&turned, Vec3::new(13.0, 0.0, 0.0)), "and scale doesn't count");
    }
}
