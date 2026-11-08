//! The height of the ground anywhere in the world, from the macro map's altitude bands with
//! seeded detail on top. A pure function of the position and the seed.
//!
//! The coast doesn't come from the bands: the map's land mask, in texels of about a kilometer,
//! becomes a smooth signed distance (`shore_field`) and gets fractal bays and points of its own
//! (`Relief::shore_at`). The land rises from it over a beach, the sea floor falls away over a
//! shelf, and the bands only say how high the land gets inland and how deep the sea.

use glam::{DVec2, Vec3};

use crate::field::{Field, blur, signed_distance};
use crate::noise::{fbm, ridged, smoothstep, value};
use crate::{Hydrology, METERS_PER_UNIT, MacroMap};

/// Map units between two band samples: the coast mask's texel.
const SAMPLE: f64 = 0.5;
/// How far the band samples are pushed around, in meters, so their grid never shows.
const WARP: f64 = 420.0;
/// The wavelength of that push, in meters.
const WARP_LENGTH: f64 = 2600.0;
/// The longest wavelength of the mountain crests, in meters.
const RIDGE_LENGTH: f64 = 3400.0;
/// How much detail each band height carries, as (band height, meters up or down). Each stays
/// under half the gap to the next band, so the middle of a map unit stays in its band.
const DETAIL: [(f64, f64); 6] =
    [(0.0, 0.0), (100.0, 55.0), (400.0, 110.0), (800.0, 190.0), (1600.0, 420.0), (3000.0, 850.0)];
/// Above this band height the land starts to have crests.
const RIDGES_FROM: f64 = 500.0;
/// Bumps on the sea floor, in meters.
const SEABED: f64 = 5.0;

/// The shore field: meters between its cells, map units it reaches past the window, and the
/// blur (in cells) that rounds the mask's texels off.
const SHORE_CELL: f64 = 100.0;
const SHORE_PAD: f64 = 2.5;
const SHORE_BLUR: usize = 5;
/// The coast's own detail, added to the distance from the shore: (wavelength, meters), from
/// bays and points down to little coves.
const COAST_DETAIL: [(f64, f64); 4] = [(2600.0, 220.0), (800.0, 80.0), (210.0, 20.0), (55.0, 4.0)];
/// Past this distance from the smooth shore the coast's detail can't reach.
const COAST_REACH: f64 = 1200.0;
/// How steeply a beach rises out of the sea, and the most it rises before the land does.
const BEACH_SLOPE: f64 = 0.055;
const BEACH_TOP: f64 = 3.2;
/// Meters from the shore over which the land climbs to its band, per meter of band height,
/// and the shortest and longest such climb.
const RISE_PER_METER: f64 = 16.0;
const RISE: (f64, f64) = (500.0, 6000.0);
/// How steeply the sea floor goes down off a beach, and meters over which it reaches its band.
const SHELF_SLOPE: f64 = 0.04;
const SHELF: f64 = 2000.0;
/// The land's shapes, as (wavelength in meters, share of the band's detail): broad swells,
/// hills and the small swales between them.
const SWELLS: (f64, f64) = (3600.0, 0.6);
const HILLS: (f64, f64) = (1100.0, 0.4);
const SWALES: (f64, f64) = (240.0, 0.07);
/// Winding valleys: their wavelength, how narrow (a share of the noise), and how deep as a
/// share of the band's detail.
const VALLEY_LENGTH: f64 = 2300.0;
const VALLEY_WIDTH: f64 = 0.16;
const VALLEY_DEPTH: f64 = 0.55;
/// Detail grows in from the shore over this many meters, so the beach stays a beach.
const INLAND: f64 = 700.0;
/// The least the land stands above the sea once it's off the beach: the detail dips toward it
/// and never under, or the sea would show through inland.
const DRY_ABOVE: f64 = 3.0;

/// A rocky table rising off the plain (a kopje): a ring of cliffs `height` meters high around
/// a top that dips a little to the middle, where its springs gather in a small lake.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Highland {
    pub center: DVec2,
    pub radius: f64,
    pub height: f64,
}

/// The highland: how much of the island's widest inland distance it may take up, its height,
/// how wide its cliff is, how far its foot slopes out, how deep its top dips and how much more
/// rain stays on it to run off.
const HIGHLAND_SHARE: f64 = 0.3;
const HIGHLAND_RADIUS: (f64, f64) = (450.0, 1100.0);
const HIGHLAND_HEIGHT: f64 = 30.0;
const CLIFF: f64 = 18.0;
const HIGHLAND_FOOT: f64 = 450.0;
const HIGHLAND_DISH: f64 = 3.5;
const HIGHLAND_RAIN: f64 = 5.0;
/// Height of each layer of rock on the cliffs, and how much of it juts out over the next.
const STRATA: f64 = 6.0;
const STRATA_JUT: f64 = 0.85;
/// How far from the cliff's foot the fallen boulders lie.
const BOULDERS_REACH: f64 = 40.0;

/// Where a point is on the highland's cliffs: see [`Relief::cliff_at`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cliff {
    pub face: bool,
    pub foot: f64,
}

struct Rim {
    top: f64,
    lift: f64,
    face: f64,
    foot: f64,
}

/// How far one water sheet reaches into another it joins, in meters, so no gap shows.
const SHEETS_OVERLAP: f64 = 0.6;
/// A stream's own water stops where the sea floor is this deep: it has faded out by then (see
/// `land.rs`), and under the sea's surface the sea is what shows.
const SEA_COVERS: f64 = -0.6;

/// The ground of a generated world.
pub struct Relief {
    map: MacroMap,
    seed: u64,
    water: Hydrology,
    /// Signed meters to the coast mask's edge, smoothed: positive on land.
    shore: Field,
    /// How much each hollow is filled up so the land drains: see [`Relief::basin_depth`].
    basins: Field,
    highland: Option<Highland>,
    village: Option<crate::Village>,
}

impl Relief {
    /// The ground of `map`'s world, with its streams and ponds found and dug in: see
    /// [`Hydrology`].
    pub fn new(map: MacroMap) -> Self {
        let mut relief = Self::dry(map);
        relief.basins = crate::hydro::basins(&relief);
        relief.water = Hydrology::generate(&relief);
        relief.village = crate::village::choose_village(&relief);
        relief
    }

    /// The ground without fresh water: nothing is dug into it, and its hollows aren't filled.
    pub fn dry(map: MacroMap) -> Self {
        let seed = map.desc.seed;
        let shore = shore_field(&map);
        let highland = find_highland(&shore, seed);
        Self { map, seed, water: Hydrology::default(), shore, basins: Field::default(), highland, village: None }
    }

    /// The island's highland, where its longest streams rise.
    pub fn highland(&self) -> Option<Highland> {
        self.highland
    }

    /// The highland's share of a point: 1 on its top, 0 off its foot; and the meters it lifts the
    /// ground there.
    fn highland_at(&self, x: f64, z: f64) -> (f64, f64) {
        let rim = self.rim_at(x, z);
        (rim.top, rim.lift)
    }

    /// Where a point is on the highland's cliffs: on a face or a ledge between them, where
    /// nothing grows, and how much it is at the cliff's foot, where fallen boulders lie.
    pub fn cliff_at(&self, x: f64, z: f64) -> Cliff {
        let rim = self.rim_at(x, z);
        Cliff { face: rim.face > 0.0, foot: rim.foot }
    }

    fn rim_at(&self, x: f64, z: f64) -> Rim {
        let none = Rim { top: 0.0, lift: 0.0, face: 0.0, foot: 0.0 };
        let Some(h) = self.highland else { return none };
        let p = DVec2::new(x, z);
        let d = p.distance(h.center);
        if d > h.radius * 1.4 + HIGHLAND_FOOT {
            return none;
        }
        // The rim wanders in bays and spurs, so the table isn't round.
        let edge = h.radius
            * (1.0
                + value(x / 420.0, z / 420.0, self.seed.wrapping_add(61)) * 0.22
                + value(x / 130.0, z / 130.0, self.seed.wrapping_add(62)) * 0.06)
            // Buttresses and gullies along the cliff, in meters.
            + value(x / 38.0, z / 38.0, self.seed.wrapping_add(64)) * 18.0
            + value(x / 12.0, z / 12.0, self.seed.wrapping_add(65)) * 5.0;
        // The cliff breaks at a ledge partway down, whose height wanders.
        let ledge = 0.45 + value(x / 160.0, z / 160.0, self.seed.wrapping_add(66)) * 0.15;
        let upper = smoothstep(edge + CLIFF * 0.05, edge - CLIFF * 0.5, d);
        let lower = smoothstep(edge + CLIFF * 0.5, edge + CLIFF * 0.15, d);
        let top = (1.0 - ledge) * upper + ledge * lower;
        let foot = smoothstep(edge + HIGHLAND_FOOT, edge, d);
        let dish = (1.0 - (d / edge).min(1.0).powi(2)) * HIGHLAND_DISH;
        // Broken rock on the faces, none on the top or the plain.
        let face = (upper * (1.0 - upper) + lower * (1.0 - lower)) * 4.0;
        let rubble = (value(x / 6.0, z / 6.0, self.seed.wrapping_add(67)) * 1.6
            + value(x / 17.0, z / 17.0, self.seed.wrapping_add(68)) * 2.4)
            * face;
        // Layers of rock: the face climbs in steps, each layer jutting out over the one below,
        // tilted and wandering a little so they don't read as contour lines.
        let rise = h.height * top + value(x / 90.0, z / 90.0, self.seed.wrapping_add(69)) * 2.5;
        let strata = ((rise / STRATA).fract() - 0.5) * STRATA * STRATA_JUT * face.min(1.0);
        let lift = h.height * (0.88 * top + 0.12 * foot) - dish * upper + rubble - strata;
        let on_face = d > edge - CLIFF * 0.6 && d < edge + CLIFF * 0.6;
        let at_foot = smoothstep(edge + CLIFF * 0.5, edge + CLIFF * 0.65, d)
            * smoothstep(edge + CLIFF * 0.5 + BOULDERS_REACH, edge + CLIFF * 0.5 + BOULDERS_REACH * 0.4, d);
        Rim { top: upper, lift, face: if on_face { 1.0 } else { 0.0 }, foot: at_foot }
    }

    /// How many meters the ground was raised at a point so that every hollow spills into the
    /// sea: a hollow's floor becomes a flat at its rim, as a lake's silt would leave it.
    pub fn basin_depth(&self, p: DVec2) -> f64 {
        self.basins.sample(p).unwrap_or(0.0)
    }

    /// Where the village stands and why: see [`crate::choose_village`].
    pub fn village(&self) -> Option<&crate::Village> {
        self.village.as_ref()
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// The address of pond `p` of [`Relief::water`], by its middle.
    pub fn pond_id(&self, p: usize) -> Option<crate::GenId> {
        let pond = self.water().ponds.get(p)?;
        Some(crate::GenId::place(self.seed, "pond", pond.center))
    }

    /// The address of stream `s` of [`Relief::water`], by its source.
    pub fn stream_id(&self, s: usize) -> Option<crate::GenId> {
        let first = self.water().streams.get(s)?.points.first()?.at;
        Some(crate::GenId::place(self.seed, "stream", DVec2::new(first.x, first.z)))
    }

    /// The streams and ponds dug into this ground.
    /// The points of stream `s` whose water is drawn as its own sheet: not where it lies in a
    /// pond or in the stream it joins, whose water is already there at the same level, nor
    /// past the coast, where the sea covers it. Two sheets over each other show their edges.
    pub fn drawn_course(&self, s: usize) -> std::ops::Range<usize> {
        let water = &self.water;
        let points = &water.streams[s].points;
        let covered = |i: usize| {
            let p = points[i].at;
            let at = DVec2::new(p.x, p.z);
            if water.ponds.iter().any(|pond| pond.covers(at, SHEETS_OVERLAP)) {
                return true;
            }
            if let crate::StreamEnd::Stream(into) = water.streams[s].end {
                let joined = &water.streams[into].points;
                if joined.iter().any(|q| {
                    DVec2::new(q.at.x, q.at.z).distance(at) < q.half_width - SHEETS_OVERLAP && i + 40 > points.len()
                }) {
                    return true;
                }
            }
            water.streams[s].end == crate::StreamEnd::Sea && self.raw_height_at(p.x, p.z) < SEA_COVERS
        };
        let mut start = 0;
        while start + 2 < points.len() && covered(start) {
            start += 1;
        }
        let mut end = points.len();
        while end > start + 2 && covered(end - 1) {
            end -= 1;
        }
        start..end
    }

    pub fn water(&self) -> &Hydrology {
        &self.water
    }

    /// Whether a point lies under fresh water.
    pub fn is_bed(&self, x: f64, z: f64) -> bool {
        self.water.is_bed(x, z)
    }

    pub fn map(&self) -> &MacroMap {
        &self.map
    }

    /// Height of the ground in meters above the sea, at world `x`, `z` (meters, `f64`), with
    /// the beds of the streams and ponds dug in.
    pub fn height_at(&self, x: f64, z: f64) -> f64 {
        self.water.dig(x, z, self.raw_height_at(x, z))
    }

    /// The ground as a tile with vertices `cell` meters apart shows it: the streams' beds
    /// widened so that it has a vertex in them, which keeps them in sight from far away.
    pub fn height_seen(&self, x: f64, z: f64, cell: f64) -> f64 {
        // Every vertex of a triangle the water's middle lies over is within a cell's diagonal of
        // it: dug under the water out to there, the ground never closes over the stream.
        let wide = cell * std::f64::consts::SQRT_2 + 0.1;
        self.water.dig_wide(x, z, self.raw_height_at(x, z), wide)
    }

    /// Meters from the shore, before the fresh water: positive on land. The map's coast,
    /// smoothed, with fractal bays and points of its own.
    pub fn shore_at(&self, x: f64, z: f64) -> f64 {
        let smooth = match self.shore.sample(DVec2::new(x, z)) {
            Some(d) => d,
            None if self.map.band_at(DVec2::new(x, z) / METERS_PER_UNIT).0 => 1e5,
            None => -1e5,
        };
        if smooth.abs() > COAST_REACH {
            return smooth;
        }
        // Warped, so the bays don't line up.
        let (wx, wz) = (x / 900.0, z / 900.0);
        let (px, pz) = (
            x + fbm(wx, wz, 2, self.seed.wrapping_add(41)) * 180.0,
            z + fbm(wx + 7.1, wz - 3.3, 2, self.seed.wrapping_add(42)) * 180.0,
        );
        let detail: f64 = COAST_DETAIL
            .iter()
            .zip(43u64..)
            .map(|(&(length, meters), k)| value(px / length, pz / length, self.seed.wrapping_add(k)) * meters)
            .sum();
        // Fades out toward the reach, so there's no step there.
        smooth + detail * smoothstep(COAST_REACH, COAST_REACH * 0.6, smooth.abs())
    }

    /// The ground as the shore, the bands and the noise make it, before any water digs into it.
    pub fn raw_height_at(&self, x: f64, z: f64) -> f64 {
        let (wx, wz) = (x / WARP_LENGTH, z / WARP_LENGTH);
        let push = DVec2::new(
            fbm(wx, wz, 2, self.seed.wrapping_add(11)),
            fbm(wx + 31.7, wz - 17.3, 2, self.seed.wrapping_add(12)),
        ) * WARP;
        let (bx, bz) = (x + push.x, z + push.y);
        let d = self.shore_at(x, z);
        if d <= 0.0 {
            // Down off the beach, then over the shelf to the sea's bands.
            let off = -d;
            let floor = self.base_at(bx, bz, false).min(-4.0);
            let slope = (-off * SHELF_SLOPE).max(floor);
            let h = slope + (floor - slope) * smoothstep(0.0, SHELF, off);
            let bumps = fbm(x / 600.0, z / 600.0, 3, self.seed.wrapping_add(21)) * SEABED;
            return h + bumps * smoothstep(60.0, 600.0, off);
        }
        let band = self.base_at(bx, bz, true);
        let filled = self.basin_depth(DVec2::new(x, z));
        let (_, lift) = self.highland_at(x, z);
        // Some stretches of the coast stand up as low bluffs; the rest are beaches.
        let bluff = smoothstep(0.25, 0.6, value(x / 3200.0, z / 3200.0, self.seed.wrapping_add(51)));
        let beach = (d * BEACH_SLOPE * (1.0 + 5.0 * bluff)).min(BEACH_TOP * (1.0 + 1.5 * bluff));
        let rise = (band * RISE_PER_METER).clamp(RISE.0, RISE.1) * (1.0 - 0.6 * bluff);
        let ground = beach + (band - beach).max(0.0) * smoothstep(0.0, rise, d);
        let mut detail = self.detail(x, z, band) * smoothstep(0.0, INLAND, d);
        // Dips ease toward the least height instead of going under it.
        let room = ground - DRY_ABOVE;
        if detail < 0.0 {
            detail = if room > 0.0 { -room * (1.0 - (detail / room).exp()) } else { 0.0 };
        }
        let h = ground + detail + lift + filled;
        self.village.as_ref().map_or(h, |v| v.flatten(DVec2::new(x, z), h))
    }

    /// The bands' heights, smoothed between the samples: each sample's own height at its
    /// middle, with a flat slope there. `land` asks the land's bands only (a sea sample counts
    /// as the lowest land) or the sea's only (a land sample counts as the coastal sea).
    fn base_at(&self, x: f64, z: f64, land: bool) -> f64 {
        let g = DVec2::new(x, z) / METERS_PER_UNIT / SAMPLE - 0.5;
        let cell = g.floor();
        let t = g - cell;
        let (tx, tz) = (smoothstep(0.0, 1.0, t.x), smoothstep(0.0, 1.0, t.y));
        let at = |i: f64, j: f64| {
            self.band_height(DVec2::new((cell.x + i + 0.5) * SAMPLE, (cell.y + j + 0.5) * SAMPLE), land)
        };
        let top = at(0.0, 0.0) + (at(1.0, 0.0) - at(0.0, 0.0)) * tx;
        let bottom = at(0.0, 1.0) + (at(1.0, 1.0) - at(0.0, 1.0)) * tx;
        top + (bottom - top) * tz
    }

    fn band_height(&self, map: DVec2, land: bool) -> f64 {
        match self.map.band_at(map) {
            (true, Some(band)) if land && band.elevation > 0.0 => band.elevation,
            (false, Some(band)) if !land && band.elevation < 0.0 => band.elevation,
            _ if land => 100.0,
            _ => -20.0,
        }
    }

    fn detail(&self, x: f64, z: f64, base: f64) -> f64 {
        let amplitude = detail_amplitude(base);
        let wave = |(length, share): (f64, f64), octaves: u32, k: u64| {
            fbm(x / length, z / length, octaves, self.seed.wrapping_add(k)) * amplitude * share
        };
        let mut h = wave(SWELLS, 3, 22) + wave(HILLS, 4, 24) + wave(SWALES, 3, 25);
        // Valleys wind along where a slow noise crosses zero; the streams find them.
        let (vx, vz) = (x / VALLEY_LENGTH, z / VALLEY_LENGTH);
        let bend = fbm(vx * 1.7, vz * 1.7, 2, self.seed.wrapping_add(26)) * 0.35;
        let v = fbm(vx + bend, vz - bend, 3, self.seed.wrapping_add(27)).abs();
        let floor = 1.0 - smoothstep(0.0, VALLEY_WIDTH, v);
        h -= floor * floor * amplitude * VALLEY_DEPTH;
        let crests = smoothstep(RIDGES_FROM, RIDGES_FROM * 3.0, base);
        if crests <= 0.0 {
            return h;
        }
        // Crests run along a slow turn, so ranges keep a direction for a while.
        let ridge = ridged(x / RIDGE_LENGTH, z / RIDGE_LENGTH, 5, self.seed.wrapping_add(23)) * 2.0 - 1.0;
        h * (1.0 - crests) + ridge * crests * amplitude
    }

    /// Grass color by the map's biome, sRGB: a tint over the ground picture. It changes
    /// only from one band sample (half a map unit) to the next; see [`Relief::biome_cell`].
    pub fn biome_tint_at(&self, x: f64, z: f64) -> Vec3 {
        let map = DVec2::new(x, z) / METERS_PER_UNIT;
        biome_tint(self.map.sample_map(map).biome.map(|b| b.id.as_str()).unwrap_or(""))
    }

    /// The band sample a point belongs to, which [`Relief::biome_tint_at`] is the same over.
    pub fn biome_cell(x: f64, z: f64) -> (i64, i64) {
        let cell = METERS_PER_UNIT * SAMPLE;
        ((x / cell).floor() as i64, (z / cell).floor() as i64)
    }

    /// How much of the rain stays to run off, by the map's biome: 1 for a temperate forest.
    pub fn wetness_at(&self, x: f64, z: f64) -> f64 {
        let map = DVec2::new(x, z) / METERS_PER_UNIT;
        let on_top = self.highland_at(x, z).0;
        biome_wetness(self.map.sample_map(map).biome.map(|b| b.id.as_str()).unwrap_or(""))
            * (1.0 + (HIGHLAND_RAIN - 1.0) * on_top)
    }

    /// A slow wash of light and dark, around 1, so a large biome isn't one flat color.
    pub fn wash(&self, x: f64, z: f64) -> f32 {
        1.0 + value(x / 900.0, z / 900.0, self.seed.wrapping_add(31)) as f32 * 0.08
    }
}

fn detail_amplitude(base: f64) -> f64 {
    for pair in DETAIL.windows(2) {
        let ((h0, a0), (h1, a1)) = (pair[0], pair[1]);
        if base <= h1 {
            return a0 + (a1 - a0) * ((base - h0) / (h1 - h0)).clamp(0.0, 1.0);
        }
    }
    DETAIL[DETAIL.len() - 1].1
}

/// How much water each biome's climate leaves on the ground to run off.
pub fn biome_wetness(id: &str) -> f64 {
    match id {
        "floresta_tropical" | "oceanico" => 1.3,
        "floresta_subtropical" | "floresta_temperada_decidua" | "sub_artico" => 1.0,
        "mediterraneo" | "savana_tropical" | "floresta_seca_tropical" | "tundra" => 0.7,
        "estepe_fria" | "estepe_quente" => 0.4,
        "deserto_quente" | "deserto_frio" | "pico_congelado" | "calota_polar" => 0.15,
        _ => 0.8,
    }
}

/// How each biome tints grass, sRGB.
pub fn biome_tint(id: &str) -> Vec3 {
    match id {
        "oceanico" => Vec3::new(0.95, 1.05, 0.95),
        "floresta_temperada_decidua" => Vec3::new(0.85, 1.0, 0.8),
        "floresta_subtropical" | "floresta_tropical" => Vec3::new(0.8, 1.1, 0.75),
        "mediterraneo" => Vec3::new(1.15, 1.05, 0.72),
        "floresta_seca_tropical" | "savana_tropical" => Vec3::new(1.15, 1.0, 0.65),
        "estepe_fria" => Vec3::new(1.0, 1.0, 0.8),
        "estepe_quente" | "deserto_quente" => Vec3::new(1.3, 1.05, 0.62),
        "deserto_frio" => Vec3::new(1.15, 1.05, 0.85),
        "tundra" | "sub_artico" => Vec3::new(0.85, 0.95, 1.0),
        "pico_congelado" | "calota_polar" => Vec3::new(1.4, 1.45, 1.5),
        _ => Vec3::ONE,
    }
}

/// The coast mask as a smooth signed distance, in meters, over the window and around it.
fn shore_field(map: &MacroMap) -> Field {
    let window = map.desc.window;
    if window.width <= 0.0 || window.height <= 0.0 {
        return Field::default();
    }
    let min = (DVec2::new(window.x, window.y) - DVec2::splat(SHORE_PAD)) * METERS_PER_UNIT;
    let span = (DVec2::new(window.width, window.height) + DVec2::splat(SHORE_PAD * 2.0)) * METERS_PER_UNIT;
    let (nx, nz) = ((span.x / SHORE_CELL).ceil() as usize, (span.y / SHORE_CELL).ceil() as usize);
    let mut field = Field { min, cell: SHORE_CELL, nx, nz, values: Vec::new() };
    let land: Vec<bool> = (0..nx * nz).map(|i| map.band_at(field.at(i) / METERS_PER_UNIT).0).collect();
    let mut values = signed_distance(&land, nx, nz, SHORE_CELL);
    blur(&mut values, nx, nz, SHORE_BLUR);
    field.values = values;
    field
}

/// The highland goes where the land is widest, a little off the very middle by the seed.
fn find_highland(shore: &Field, seed: u64) -> Option<Highland> {
    let (best, inland) = shore.values.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1).then(b.0.cmp(&a.0)))?;
    if *inland < 2000.0 {
        return None;
    }
    let radius = (inland * HIGHLAND_SHARE).clamp(HIGHLAND_RADIUS.0, HIGHLAND_RADIUS.1);
    let angle = value(seed as f64 * 0.37, 1.3, seed.wrapping_add(63)) * std::f64::consts::PI * 2.0;
    let off = DVec2::new(angle.cos(), angle.sin()) * inland * 0.25;
    let center = shore.at(best) + off;
    // Keep it well inland even after the push.
    let center = if shore.sample(center).is_some_and(|d| d > radius + 1200.0) { center } else { shore.at(best) };
    Some(Highland { center, radius, height: HIGHLAND_HEIGHT })
}
