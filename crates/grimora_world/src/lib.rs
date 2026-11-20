//! Generated worlds. For now, the Nyraldar macro map: [`MacroMap`] answers, at any
//! [`WorldPos`], the altitude band, the biome, land or water, the nation, the water body and
//! the points nearby, read from the category grids in `assets/world/` (written by
//! `tools/import_world_map.py`). A [`WorldDesc`] `.ron` says which map, window and seed to use.
//!
//! Categories are never blended: every answer is the nearest texel's. Where the generator needs
//! a softer border, it draws one with noise over these answers.

mod desc;
mod fall;
mod field;
pub mod folk;
mod hydro;
mod life;
mod map;
pub mod noise;
mod relief;
mod surface;
mod tile;
mod village;

pub use desc::{CoastDesc, FreshWaterLook, GroundLook, MapWindow, WorldDesc};
pub use fall::{Curtain, curtains};
pub use folk::{Bond, Day, Doing, Kin, Post, Resident, body_shape, bond, clothes, hair, hat, residents};
pub use hydro::{FreshPoint, Hydrology, Pond, Stream, StreamEnd, StreamPoint};
pub use life::{
    AmbienceDesc, FaunaPlace, FloraKey, GenId, LifeBook, PlantDesc, PlantSpot, PondFaunaDesc, SeaFaunaDesc,
    WaterLiking, When, coast_places, fauna_places, flora_around, path_seed, place_plants,
};
pub use map::{Band, Biome, Layer, LayerKind, Legend, MacroMap, MapPoint, MapSample, Nation, PointGroup, WaterBody};
pub use relief::{Cliff, Highland, Relief, biome_tint, biome_wetness};
pub use surface::{FRESH_TOP_LEVEL, FreshCourses, fresh_courses, fresh_tile};
pub use tile::{
    BASE_TILE, NEAR_CELLS, SPLIT, TILE_CELLS, TOP_LEVEL, TileKey, TileLook, build_tile, ground_at as tile_ground,
    mesh_hash, tile_collider, tile_heights, tiles_around,
};
pub use village::{
    Extra, ExtraKind, Field, Largo, Layout, Lot, VILLAGE_CLEAR, VILLAGE_EASE, VILLAGE_FLAT, Village, choose_village,
    turn,
};

use glam::DVec2;
use grimora_core::WorldPos;

/// Meters between two points of a generated stream's course.
pub fn hydro_step() -> f64 {
    hydro::STEP
}

/// Meters in one map unit: the compendium's ruler, 2000 km over 948 units.
pub const METERS_PER_UNIT: f64 = 2_000_000.0 / 948.0;

/// The world position of a map point (x east, y south), on the ground plane: map zero is world
/// zero, map x is +X and map y is +Z, since -Z is north.
pub fn map_to_world(map: DVec2) -> WorldPos {
    WorldPos::new(map.x * METERS_PER_UNIT, 0.0, map.y * METERS_PER_UNIT)
}

/// The map point (x east, y south) under a world position.
pub fn world_to_map(world: WorldPos) -> DVec2 {
    DVec2::new(world.x / METERS_PER_UNIT, world.z / METERS_PER_UNIT)
}
