//! What lives on a generated world, by biome: plants, ambience and animals, from the file a
//! [`WorldDesc`](crate::WorldDesc) names in `life`. Only data and placement: where each plant
//! stands and where each group of animals lives, a pure function of the seed and of the place's
//! path (`flora/<plant>/<cell>`), so changing one plant or one cell never moves another.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use glam::{DVec2, DVec3, Quat, Vec3};
use grimora_core::Rng;
use grimora_scene::{MeshDesc, TextureDesc};
use serde::{Deserialize, Serialize};

use crate::noise::{smoothstep, value};
use crate::{METERS_PER_UNIT, Relief, biome_tint};

/// The smallest and largest side of a plant's cells, in meters.
const SMALLEST_CELL: f64 = 16.0;
const LARGEST_CELL: f64 = 256.0;
/// How far apart, in meters, the ground is sampled to find a slope.
const SLOPE_STEP: f64 = 1.5;

/// A generated world's life file. Paths are relative to it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LifeBook {
    pub plants: Vec<PlantDesc>,
    /// Loops that play while the camera is over their biomes.
    pub ambience: Vec<AmbienceDesc>,
    /// Animals of the sea, placed around the coast.
    pub sea_fauna: Vec<SeaFaunaDesc>,
    /// Animals of the ponds deep and wide enough for them.
    pub pond_fauna: Vec<PondFaunaDesc>,
    /// Meters between two places along the coast where the sea's animals live.
    pub coast_spacing: f64,
}

/// One kind of plant (or stone): what it looks like, where it grows and how thick.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PlantDesc {
    /// Names its cells' seeds: renaming a plant moves it, reordering the list doesn't.
    pub name: String,
    pub mesh: MeshDesc,
    /// Several meshes instead of one (models, say): variant `v` is `meshes[v % len]`, seeded.
    pub meshes: Vec<MeshDesc>,
    pub texture: Option<TextureDesc>,
    /// Distinct shapes of a seeded mesh; each copy picks one.
    pub variants: u32,
    /// The map's biome ids it grows in; empty is all of them.
    pub biomes: Vec<String>,
    /// Copies per hectare on open ground, before the water and the patches.
    pub density: f64,
    /// How far from the camera it is drawn, in meters.
    pub reach: f64,
    pub scale: (f32, f32),
    pub color: Vec3,
    /// Random brightness per copy, `0..1`.
    pub color_jitter: f32,
    /// Multiplies `color` by the biome's grass tint, as the ground's grass is.
    pub biome_tint: bool,
    /// Random lean in degrees.
    pub tilt: f32,
    /// How far each copy is pushed into the ground, times its scale.
    pub sink: f32,
    /// Lowest ground it grows on, in meters above the sea: over the beach.
    pub above: f64,
    pub below: f64,
    /// The least the ground may face up (the cosine of its slope).
    pub flat: f64,
    pub water: WaterLiking,
    /// How clumped it grows, `0..1`: 0 is even, 1 is only in patches.
    pub patches: f64,
    /// Meters across a patch.
    pub patch_size: f64,
    /// Grows only this many meters inland of the coast, `(nearest, farthest)`: palms on the
    /// shore. `None` is anywhere on land.
    pub coast: Option<(f64, f64)>,
    /// Lies only at the foot of the highland's cliffs, as fallen boulders do. Nothing grows on
    /// the faces and ledges themselves.
    pub cliff_foot: bool,
    /// Bumps into the player when near: a model's `*_COL` node does it anyway, and this gives
    /// any other mesh its convex hull.
    pub solid: bool,
}

impl Default for PlantDesc {
    fn default() -> Self {
        Self {
            name: String::new(),
            mesh: MeshDesc::Rock { seed: 0 },
            meshes: Vec::new(),
            texture: None,
            variants: 4,
            biomes: Vec::new(),
            density: 10.0,
            reach: 200.0,
            scale: (0.8, 1.2),
            color: Vec3::ONE,
            color_jitter: 0.1,
            biome_tint: false,
            tilt: 4.0,
            sink: 0.05,
            above: 4.5,
            below: 10_000.0,
            flat: 0.8,
            water: WaterLiking::default(),
            patches: 0.0,
            patch_size: 40.0,
            coast: None,
            cliff_foot: false,
            solid: false,
        }
    }
}

/// How a plant grows by the fresh water: kept `gap` meters off its edge, `edge` times as thick
/// right past that, easing to `far` times by `reach` meters.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct WaterLiking {
    pub gap: f64,
    pub edge: f64,
    pub reach: f64,
    pub far: f64,
}

impl Default for WaterLiking {
    fn default() -> Self {
        Self { gap: 1.5, edge: 1.0, reach: 30.0, far: 1.0 }
    }
}

impl WaterLiking {
    /// The density's multiplier at `d` meters from the water's edge; `None` too close.
    fn at(&self, d: f64) -> Option<f64> {
        if d < self.gap {
            return None;
        }
        let t = smoothstep(0.0, self.reach.max(0.01), d - self.gap);
        Some(self.edge + (self.far - self.edge) * t)
    }

    fn most(&self) -> f64 {
        self.edge.max(self.far)
    }
}

/// When an ambience loop plays.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum When {
    #[default]
    Always,
    Day,
    Night,
}

/// A loop for some biomes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AmbienceDesc {
    pub sound: PathBuf,
    /// Empty is every biome on land.
    pub biomes: Vec<String>,
    pub volume: f32,
    pub when: When,
    /// Fades in over this many meters from the coast, so the shore is the sea's.
    pub inland: f64,
}

/// A species of the sea, in a group at some of the places along the coast.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SeaFaunaDesc {
    /// A species file, relative to the life file.
    pub species: PathBuf,
    /// Meters out from the coast its group's middle is (negative goes inland).
    pub offshore: f64,
    /// Height of its middle over the sea.
    pub height: f64,
    pub radius: f32,
    pub count: u32,
    /// One group at every `every`-th place along the coast, from the `first`.
    pub every: u32,
    pub first: u32,
}

impl Default for SeaFaunaDesc {
    fn default() -> Self {
        Self { species: PathBuf::new(), offshore: 0.0, height: 0.0, radius: 30.0, count: 10, every: 1, first: 0 }
    }
}

/// A species of the ponds.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PondFaunaDesc {
    pub species: PathBuf,
    /// Only ponds at least this deep and wide, in meters.
    pub depth: f64,
    pub radius: f64,
    pub count: u32,
}

impl LifeBook {
    /// Reads a life file; paths in it are made relative to the current directory.
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let options = ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
        let mut book: Self = options.from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        let dir = path.parent().unwrap_or(Path::new(""));
        for ambience in &mut book.ambience {
            ambience.sound = dir.join(&ambience.sound);
        }
        for sea in &mut book.sea_fauna {
            sea.species = dir.join(&sea.species);
        }
        for pond in &mut book.pond_fauna {
            pond.species = dir.join(&pond.species);
        }
        Ok(book)
    }
}

/// A cell of one plant's grid: its copies are placed by cell, each from its own seed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FloraKey {
    /// Into [`LifeBook::plants`].
    pub plant: u16,
    pub x: i64,
    pub z: i64,
}

/// The stable address of something generated: a hash of its path in the world's hierarchy
/// (seed, kind, cell or place, index in that cell), never of the order it was made in. The same
/// in any load order, on any thread and after the origin moves, so a save can name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GenId(pub u64);

impl GenId {
    /// The `index`-th plant tried in a cell of the plant called `plant`.
    pub fn plant(seed: u64, plant: &str, x: i64, z: i64, index: u32) -> Self {
        Self(path_seed(&[seed, name_hash("flora"), name_hash(plant), x as u64, z as u64, index as u64]))
    }

    /// Something of `kind` known by its name (a group of animals...).
    pub fn named(seed: u64, kind: &str, name: &str) -> Self {
        Self(path_seed(&[seed, name_hash(kind), name_hash(name)]))
    }

    /// Something of `kind` known by where it is (world meters, rounded to the meter): ponds,
    /// streams by their source.
    pub fn place(seed: u64, kind: &str, at: DVec2) -> Self {
        Self(path_seed(&[seed, name_hash(kind), at.x.round() as i64 as u64, at.y.round() as i64 as u64]))
    }
}

/// One copy of a plant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlantSpot {
    /// Its address: the plant, the cell and which try in that cell.
    pub id: GenId,
    /// Which of the plant's variants.
    pub variant: u32,
    /// World meters, on the ground (minus its sink).
    pub at: DVec3,
    pub rotation: Quat,
    pub scale: f32,
    /// sRGB.
    pub color: Vec3,
}

impl PlantDesc {
    /// The side of its cells: about a quarter of its reach, a power of two.
    pub fn cell(&self) -> f64 {
        let wanted = (self.reach / 4.0).clamp(SMALLEST_CELL, LARGEST_CELL);
        2f64.powf(wanted.log2().round())
    }
}

/// The cells of every plant whose middle is within its reach of `eye` (world meters, flat).
pub fn flora_around(book: &LifeBook, eye: DVec2) -> Vec<FloraKey> {
    let mut keys = Vec::new();
    for (i, plant) in book.plants.iter().enumerate() {
        let cell = plant.cell();
        let reach = plant.reach + cell * 0.71;
        let (x0, x1) = (((eye.x - reach) / cell).floor() as i64, ((eye.x + reach) / cell).floor() as i64);
        let (z0, z1) = (((eye.y - reach) / cell).floor() as i64, ((eye.y + reach) / cell).floor() as i64);
        for z in z0..=z1 {
            for x in x0..=x1 {
                let middle = DVec2::new(x as f64 + 0.5, z as f64 + 0.5) * cell;
                if middle.distance(eye) <= reach {
                    keys.push(FloraKey { plant: i as u16, x, z });
                }
            }
        }
    }
    keys
}

/// FNV-1a over a name, for seeds that follow names rather than order.
fn name_hash(name: &str) -> u64 {
    name.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

/// A seed from a path in the world's hierarchy: each part mixed into the last.
pub fn path_seed(parts: &[u64]) -> u64 {
    parts.iter().fold(0x9e37_79b9_7f4a_7c15, |h, &p| {
        let mut z = (h ^ p).wrapping_add(0x9e37_79b9_7f4a_7c15);
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    })
}

fn unit(rng: &mut Rng) -> f64 {
    (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64
}

/// The copies of a plant in one of its cells: the same for the same seed, in any order.
pub fn place_plants(relief: &Relief, book: &LifeBook, key: FloraKey) -> Vec<PlantSpot> {
    let Some(plant) = book.plants.get(key.plant as usize) else { return Vec::new() };
    let seed = path_seed(&[relief.seed(), name_hash("flora"), name_hash(&plant.name), key.x as u64, key.z as u64]);
    let mut rng = Rng::new(seed);
    let cell = plant.cell();
    let corner = DVec2::new(key.x as f64, key.z as f64) * cell;
    let most = plant.water.most().max(1e-6);
    let tries = plant.density / 10_000.0 * cell * cell * most;
    // A whole number of tries, with the fraction left to chance.
    let count = tries.floor() as usize + usize::from(unit(&mut rng) < tries.fract());
    let patch_seed = path_seed(&[relief.seed(), name_hash(&plant.name), 7]);
    let water = relief.water();
    let mut spots = Vec::new();
    for index in 0..count {
        // Every try spends the same numbers, kept or not, so one never moves the next.
        let p = corner + DVec2::new(unit(&mut rng), unit(&mut rng)) * cell;
        let chance = unit(&mut rng);
        let variant = (rng.next_u64() % plant.variants.max(1) as u64) as u32;
        let yaw = rng.range(0.0, std::f32::consts::TAU);
        let lean = Vec3::new(rng.jitter(1.0), 0.0, rng.jitter(1.0)).normalize_or(Vec3::X);
        let tilt = rng.range(0.0, plant.tilt.to_radians());
        let size = rng.range(plant.scale.0, plant.scale.1);
        let shade = 1.0 + rng.jitter(plant.color_jitter);

        let mut keep = 1.0;
        if plant.patches > 0.0 {
            let n = value(p.x / plant.patch_size, p.y / plant.patch_size, patch_seed) * 0.5 + 0.5;
            keep *= 1.0 - plant.patches + plant.patches * smoothstep(0.45, 0.65, n);
        }
        let Some(by_water) = plant.water.at(water.fresh_distance(p.x, p.y)) else { continue };
        keep *= by_water / most;
        let inland = water.coast_distance(p.x, p.y);
        if chance >= keep || inland <= 0.0 || plant.coast.is_some_and(|(near, far)| inland < near || inland > far) {
            continue;
        }
        let biome = relief.map().sample_map(p / METERS_PER_UNIT).biome;
        let biome_id = biome.map_or("", |b| b.id.as_str());
        if !plant.biomes.is_empty() && !plant.biomes.iter().any(|b| b == biome_id) {
            continue;
        }
        let cliff = relief.cliff_at(p.x, p.y);
        if cliff.face || (plant.cliff_foot && chance >= cliff.foot) || relief.village().is_some_and(|v| v.clears(p)) {
            continue;
        }
        let ground = relief.height_at(p.x, p.y);
        if ground < plant.above || ground > plant.below {
            continue;
        }
        if plant.flat > 0.0 {
            let dx = relief.height_at(p.x + SLOPE_STEP, p.y) - ground;
            let dz = relief.height_at(p.x, p.y + SLOPE_STEP) - ground;
            let up = SLOPE_STEP / (dx * dx + dz * dz + SLOPE_STEP * SLOPE_STEP).sqrt();
            if up < plant.flat {
                continue;
            }
        }
        let mut color = plant.color * shade;
        if plant.biome_tint {
            color *= biome_tint(biome_id) * relief.wash(p.x, p.y);
        }
        spots.push(PlantSpot {
            id: GenId::plant(relief.seed(), &plant.name, key.x, key.z, index as u32),
            variant,
            at: DVec3::new(p.x, ground - (plant.sink * size) as f64, p.y),
            rotation: Quat::from_axis_angle(lean, tilt) * Quat::from_rotation_y(yaw),
            scale: size,
            color,
        });
    }
    spots
}

/// A group of animals to put into the world.
#[derive(Debug, Clone, PartialEq)]
pub struct FaunaPlace {
    /// Names the group, for saves: `<species file stem> <n>`.
    pub name: String,
    /// Its address, from the name.
    pub id: GenId,
    pub species: PathBuf,
    /// World meters.
    pub at: DVec3,
    pub radius: f32,
    pub count: u32,
    pub seed: u64,
}

/// Places along the coast, [`LifeBook::coast_spacing`] apart, each with the direction out to
/// sea: the sea cells next to the land, walked in a fixed order.
pub fn coast_places(relief: &Relief, spacing: f64) -> Vec<(DVec2, DVec2)> {
    let water = relief.water();
    let mut places: Vec<(DVec2, DVec2)> = Vec::new();
    for at in water.coast_cells() {
        if places.iter().any(|(p, _)| p.distance(at) < spacing) {
            continue;
        }
        const STEP: f64 = 40.0;
        let d = |x: f64, z: f64| water.coast_distance(x, z);
        let grad = DVec2::new(d(at.x + STEP, at.y) - d(at.x - STEP, at.y), d(at.x, at.y + STEP) - d(at.x, at.y - STEP));
        // The distance grows inland, so the sea is the other way.
        places.push((at, (-grad).normalize_or(DVec2::X)));
    }
    places
}

/// Where the book's animals live in `relief`'s world.
pub fn fauna_places(relief: &Relief, book: &LifeBook) -> Vec<FaunaPlace> {
    let mut out = Vec::new();
    let stem = |path: &Path| path.file_stem().map_or(String::new(), |s| s.to_string_lossy().into_owned());
    if !book.sea_fauna.is_empty() && book.coast_spacing > 0.0 {
        let places = coast_places(relief, book.coast_spacing);
        for desc in &book.sea_fauna {
            let every = desc.every.max(1) as usize;
            for (i, (at, out_to_sea)) in places.iter().enumerate().skip(desc.first as usize).step_by(every) {
                let flat = *at + *out_to_sea * desc.offshore;
                let name = format!("{} {}", stem(&desc.species), i);
                out.push(FaunaPlace {
                    id: GenId::named(relief.seed(), "fauna", &name),
                    seed: path_seed(&[relief.seed(), name_hash("fauna"), name_hash(&name)]),
                    name,
                    species: desc.species.clone(),
                    at: DVec3::new(flat.x, desc.height, flat.y),
                    radius: desc.radius,
                    count: desc.count,
                });
            }
        }
    }
    for desc in &book.pond_fauna {
        for (i, pond) in relief.water().ponds.iter().enumerate() {
            if pond.depth < desc.depth || pond.radius < desc.radius {
                continue;
            }
            let name = format!("{} pond {}", stem(&desc.species), i);
            out.push(FaunaPlace {
                id: GenId::named(relief.seed(), "fauna", &name),
                seed: path_seed(&[relief.seed(), name_hash("fauna"), name_hash(&name)]),
                name,
                species: desc.species.clone(),
                at: DVec3::new(pond.center.x, pond.level - pond.depth * 0.4, pond.center.y),
                radius: (pond.radius * 0.6) as f32,
                count: desc.count,
            });
        }
    }
    out
}
