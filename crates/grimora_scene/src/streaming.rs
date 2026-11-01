//! Streaming: what a large scene loads only near the camera.
//!
//! Scattered objects are grouped into square grid cells, and region files
//! (scene fragments such as a ruin or a village) are listed with the area they
//! cover. The engine decides what to load; this module only holds the data.

use std::collections::BTreeMap;
use std::path::PathBuf;

use grimora_core::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::SceneObject;

/// Streaming settings in the scene file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StreamingDesc {
    /// Side of the square grid cells scattered objects are grouped into, in meters.
    pub cell_size: f32,
    /// Cells and regions closer than this to the camera are loaded. Past about
    /// `4 / fog.density` the fog hides everything anyway.
    pub load_distance: f32,
}

impl Default for StreamingDesc {
    fn default() -> Self {
        Self { cell_size: 64.0, load_distance: 200.0 }
    }
}

/// A scene fragment in its own file, loaded in the background when the camera
/// comes within `load_distance` of the circle (`center`, `radius`). Only its
/// `objects`, `scatter`, `lights`, `sounds`, `triggers` and `effects` are used;
/// positions are in world space and, as usual, measured from the main scene's terrain.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RegionDesc {
    /// Relative to the scene file.
    pub path: String,
    /// Only `x` and `z` count.
    pub center: Vec3,
    pub radius: f32,
}

/// Grid coordinates of a streaming cell.
pub type CellCoord = (i32, i32);

/// The resolved streaming content of a scene.
#[derive(Debug, Clone, Default)]
pub struct SceneStreaming {
    pub cell_size: f32,
    pub load_distance: f32,
    /// Scattered objects by cell.
    pub cells: BTreeMap<CellCoord, SceneCell>,
    pub regions: Vec<SceneRegion>,
}

#[derive(Debug, Clone, Default)]
pub struct SceneCell {
    pub objects: Vec<SceneObject>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneRegion {
    /// Resolved against the scene's directory.
    pub path: PathBuf,
    pub center: Vec2,
    pub radius: f32,
}

impl SceneStreaming {
    pub fn cell_of(&self, position: Vec3) -> CellCoord {
        ((position.x / self.cell_size).floor() as i32, (position.z / self.cell_size).floor() as i32)
    }

    /// Horizontal distance from `point` to the nearest point of `cell` (0 inside).
    pub fn cell_distance(&self, cell: CellCoord, point: Vec3) -> f32 {
        let min = Vec2::new(cell.0 as f32, cell.1 as f32) * self.cell_size;
        let p = Vec2::new(point.x, point.z);
        p.distance(p.clamp(min, min + Vec2::splat(self.cell_size)))
    }
}

impl SceneRegion {
    /// Horizontal distance from `point` to the region's circle (0 inside).
    pub fn distance(&self, point: Vec3) -> f32 {
        (Vec2::new(point.x, point.z).distance(self.center) - self.radius).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_and_distances() {
        let s = SceneStreaming { cell_size: 10.0, ..Default::default() };
        assert_eq!(s.cell_of(Vec3::new(5.0, 3.0, -0.5)), (0, -1));
        assert_eq!(s.cell_of(Vec3::new(-10.0, 0.0, 19.9)), (-1, 1));
        assert_eq!(s.cell_distance((0, 0), Vec3::new(5.0, 100.0, 5.0)), 0.0);
        assert_eq!(s.cell_distance((2, 0), Vec3::new(5.0, 0.0, 5.0)), 15.0);
        assert!((s.cell_distance((1, 1), Vec3::ZERO) - 200f32.sqrt()).abs() < 1e-5);

        let region = SceneRegion { path: PathBuf::new(), center: Vec2::new(10.0, 0.0), radius: 4.0 };
        assert_eq!(region.distance(Vec3::new(10.0, 50.0, 1.0)), 0.0);
        assert_eq!(region.distance(Vec3::new(30.0, 0.0, 0.0)), 16.0);
    }
}
