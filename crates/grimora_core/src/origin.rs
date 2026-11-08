//! Where things are in a world too large for `f32`: positions that last (saves, generated
//! places) are [`WorldPos`] in `f64`, and everything that runs each frame (render, physics,
//! animation, particles, sound) keeps `f32` positions relative to an [`Origin`] that jumps to
//! stay near the player. A [`Transform::position`](crate::Transform) is such a local position.

use glam::{DVec2, DVec3, Quat, Vec2, Vec3};

use crate::Transform;
use serde::{Deserialize, Serialize};

/// A position in the world, in meters, as `f64`: about 2 nm of precision at 17 000 km.
pub type WorldPos = DVec3;

/// Every pattern drawn from world positions (noise, ripples, foam) that has no period of its
/// own repeats within this many meters, so `origin mod WRAP` stands in for the origin there.
/// A multiple of 10 000 makes any frequency written with up to four decimals line up with it.
pub const WRAP: f64 = 10_000.0;

/// How far the player may get from the origin, in meters, before it moves to them.
pub const SHIFT_DISTANCE: f32 = 2048.0;
/// The origin always lands on this grid, so shifts are whole meters.
pub const SHIFT_GRID: f64 = 256.0;

/// A [`Transform`] whose position is a [`WorldPos`], for what is kept between sessions. Its
/// fields are named as `Transform`'s, so a save written with one reads as the other.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorldTransform {
    pub position: WorldPos,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Default for WorldTransform {
    fn default() -> Self {
        Self { position: WorldPos::ZERO, rotation: Quat::IDENTITY, scale: Vec3::ONE }
    }
}

/// The world position that local position zero stands for.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Origin(pub WorldPos);

impl Origin {
    pub fn new(world: WorldPos) -> Self {
        Self(world)
    }

    pub fn world(&self) -> WorldPos {
        self.0
    }

    /// The local position of a world position: only precise within a few km of the origin.
    pub fn to_local(&self, world: WorldPos) -> Vec3 {
        (world - self.0).as_vec3()
    }

    pub fn to_world(&self, local: Vec3) -> WorldPos {
        self.0 + local.as_dvec3()
    }

    pub fn transform_to_world(&self, t: &Transform) -> WorldTransform {
        WorldTransform { position: self.to_world(t.position), rotation: t.rotation, scale: t.scale }
    }

    pub fn transform_to_local(&self, t: &WorldTransform) -> Transform {
        Transform { position: self.to_local(t.position), rotation: t.rotation, scale: t.scale }
    }

    /// The origin `local` should move to, if it is too far: on the grid, under it. The offset
    /// is what every local position then loses.
    pub fn shift_for(&self, local: Vec3) -> Option<Vec3> {
        let flat = Vec2::new(local.x, local.z);
        if flat.abs().max_element() <= SHIFT_DISTANCE {
            return None;
        }
        let snap = |v: f32| (f64::from(v) / SHIFT_GRID).round() * SHIFT_GRID;
        Some(Vec3::new(snap(local.x) as f32, 0.0, snap(local.z) as f32))
    }

    /// The origin wrapped by [`WRAP`]: what the shaders and the CPU mirrors of them add to a local
    /// position to draw world-fixed patterns.
    pub fn wrapped(&self) -> Vec3 {
        wrap(self.0, WRAP)
    }

    /// The origin wrapped by `period` meters, for a pattern of that period.
    pub fn wrapped_by(&self, period: f64) -> Vec3 {
        wrap(self.0, period)
    }

    /// `k · origin`, in radians, wrapped to one turn, for a wave whose phase grows as `k`.
    pub fn phase(&self, k: Vec2) -> f32 {
        phase(DVec2::new(self.0.x, self.0.z), k)
    }
}

/// `k · at` in radians, wrapped to one turn.
pub fn phase(at: DVec2, k: Vec2) -> f32 {
    at.dot(k.as_dvec2()).rem_euclid(std::f64::consts::TAU) as f32
}

fn wrap(p: DVec3, period: f64) -> Vec3 {
    if period <= 0.0 {
        return Vec3::ZERO;
    }
    DVec3::new(p.x.rem_euclid(period), p.y.rem_euclid(period), p.z.rem_euclid(period)).as_vec3()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn far_positions_stay_precise_near_the_origin() {
        let origin = Origin::new(DVec3::new(15_000_000.0, 0.0, -7_300_000.0));
        let world = origin.0 + DVec3::new(0.123_456, 2.0, -0.000_5);
        let local = origin.to_local(world);
        assert!((origin.to_world(local) - world).length() < 1e-6);
        assert!(origin.shift_for(local).is_none());
        let shift = origin.shift_for(Vec3::new(2100.0, 5.0, -10.0)).unwrap();
        assert_eq!(shift, Vec3::new(2048.0, 0.0, 0.0));
        assert_eq!(origin.wrapped(), Vec3::ZERO);
    }
}
