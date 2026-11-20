//! Fresh water of a generated world: streams that run off the ground and the ponds they end
//! in, found once for the whole window and dug into the ground by [`Relief::height_at`].
//!
//! The rain runs downhill over a coarse grid of the window ([`GRID`] meters): a priority flood
//! from the sea and the window's edge gives every cell the one it drains into, and adding up
//! what drains through each cell, wetted by the biome, gives the streams: cells that gather at
//! least [`STREAM_FROM`]. Deep enough hollows on the way hold a pond. Each stream is then
//! a smooth line at [`STEP`] meters whose water never climbs, and it ends in the sea, in a pond
//! or in a bigger stream. A pure function of the map and the seed.
//!
//! Map rivers (the compendium's) will come in as more [`Stream`]s with their course given
//! instead of found; everything after the course (levels, carving, water bodies) is shared.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use glam::{DVec2, DVec3};

use crate::field::Field;
use crate::noise::{hash, smoothstep, value};
use crate::{METERS_PER_UNIT, Relief};

/// Meters between two cells of the drainage grid.
pub const GRID: f64 = 50.0;
/// Map units the drainage grid reaches past the window on each side: land cut by the window
/// still drains the way it would.
const PAD: f64 = 2.0;
/// Meters between two points of a stream's course.
pub const STEP: f64 = 4.0;
/// Wetted catchment, in km², from which water runs above ground.
const STREAM_FROM: f64 = 2.5;
/// Fewest grid cells in a stream that nothing else runs into: shorter ones are dropped.
const SHORTEST: usize = 6;
/// Half widths of the smallest and the largest stream, in meters, and how the width grows with
/// the catchment (meters per √km²).
const NARROWEST: f64 = 0.8;
const WIDEST: f64 = 18.0;
const WIDENS: f64 = 0.8;
/// Where the water falls this steeply (meters per meter, over [`SLOPE_WINDOW`] meters each
/// way), a stream runs narrow, shallow and fast; where it falls this little, wide and deeper:
/// how much wider and deeper on the flat, and how much narrower and shallower on the steep.
const GENTLE: f64 = 0.003;
const STEEP: f64 = 0.035;
const SLOPE_WINDOW: f64 = 40.0;
const FLAT_WIDER: f64 = 1.25;
const STEEP_NARROWER: f64 = 0.65;
const FLAT_DEEPER: f64 = 1.15;
const STEEP_SHALLOWER: f64 = 0.7;
/// Pools and riffles: meters from one pool to the next, and how much deeper a pool is (and a
/// riffle shallower) and how much narrower (and a riffle wider), as shares.
const POOL_WAVE: f64 = 48.0;
const POOL_DEEPER: f64 = 0.45;
const POOL_NARROWER: f64 = 0.12;
/// Each stream's own character, from its seed: up to this much wider and shallower, or
/// narrower and deeper.
const CHARACTER_WIDTH: f64 = 0.3;
const CHARACTER_DEPTH: f64 = 0.25;
/// The shallowest a stream's middle ever is.
const SHALLOWEST: f64 = 0.3;
/// A fall is where the water drops more than this many meters from one point of its course to
/// the next ([`STEP`] apart); its sheet is this many times as wide as the stream.
const FALL_STEP: f64 = 1.2;
const FALL_SPREADS: f64 = 1.25;
/// The pool a fall digs at its foot: how far it reaches downstream per meter of fall (and its
/// least and most, in meters), how much wider it makes the stream, and how much deeper per meter
/// of fall, up to the most.
const PLUNGE_PER_METER: f64 = 0.9;
const PLUNGE_REACH: (f64, f64) = (8.0, 24.0);
const PLUNGE_WIDER: f64 = 0.7;
const PLUNGE_DEEPER: f64 = 0.12;
const PLUNGE_DEEPEST: f64 = 2.5;
/// Over its first this many meters a stream comes out of the ground: from a trickle
/// `SPRING_WIDTH` meters wide to its own width.
const SPRING_RUN: f64 = 120.0;
const SPRING_WIDTH: f64 = 0.35;
/// The narrowest a stream's water ever is, half across: past a cell's diagonal of a level 0
/// tile (1 m), so the tile has a vertex under the water and the ground it draws there is the
/// true ground, the one plants, feet and the water's caustics go by.
pub const NARROWEST_SHOWN: f64 = 1.2;
/// How much a bank wanders in and out, as a share of the half width, at two lengths in meters.
const BANK_WOBBLE: [(f64, f64); 3] = [(150.0, 0.3), (45.0, 0.24), (14.0, 0.12)];
/// How far a stream's water lies under the ground beside it.
const SUNK: f64 = 0.25;
/// Most a stream's level may stand over the ground under its middle, where it fills a dip;
/// its lip keeps it in.
const FILL_MOST: f64 = 0.5;
/// Height of the lip at the water's edge, and how far it takes to rise.
const LIP: f64 = 0.35;
const LIP_RUN: f64 = 2.5;
/// Under the water by its edge the bed slopes like the lip, meters down per meter in, and gives
/// way to the bowl between these meters from the edge.
const EDGE_SLOPE: f64 = 0.3;
const EDGE_SHELF: (f64, f64) = (1.5, 3.5);
/// How a course finds the valley's floor: points this far apart, looking this far to each side
/// in these steps, a meter of height worth this many meters of moving, this many times.
const VALLEY_STEP: f64 = 10.0;
const VALLEY_LOOK: f64 = 30.0;
const VALLEY_LOOK_STEP: f64 = 3.0;
const VALLEY_COST: f64 = 0.04;
const VALLEY_PASSES: usize = 3;
/// How far a stream winds off the grid, in meters, and the length of a bend.
/// Bends at several scales, as (wavelength in meters, meters to the side, and more per meter of
/// half width): a wide stream winds in wide loops.
const MEANDERS: [(f64, f64, f64); 3] = [(420.0, 16.0, 6.0), (130.0, 6.0, 2.0), (38.0, 1.5, 0.4)];
/// The level at which a stream meets the sea, a little over it, and how deep into the sea its
/// course goes on.
const MOUTH_LEVEL: f64 = -0.25;
const MOUTH_DEPTH: f64 = -1.2;
/// A stream widens into its mouth over its last this many meters, to this many times as wide.
const ESTUARY_LENGTH: f64 = 180.0;
const ESTUARY_WIDENS: f64 = 1.7;
/// Depth in the middle of a stream, by its half width: a brook knee deep, a river deep enough
/// to swim, before the slope, the pools and its character.
const DEEP_FROM: f64 = 0.45;
const DEEP_PER_METER: f64 = 0.24;
const DEEPEST: f64 = 4.5;
/// Over its last this many meters a stream into the sea comes down to the sea's level.
const MOUTH_SINKS: f64 = 260.0;
/// Over its last this many meters a mouth's channel rises to the sea floor.
const MOUTH_SHOALS: f64 = 160.0;
/// A pond's outlet starts this far under its level, so the two sheets never fight.
const OUTLET_UNDER: f64 = 0.03;
/// Meters over which a tributary eases down to the level of the stream it joins.
const CONFLUENCE_RAMP: f64 = 60.0;
/// A hollow becomes a pond when filling it would take this many meters of water.
const POND_FROM: f64 = 3.0;
/// Most ponds in hollows no stream runs through.
const MOST_PONDS: usize = 0;
/// A hollow a stream runs through gets a pond from this depth on.
const CROSSED_POND_FROM: f64 = 0.8;
const POND_RADIUS: (f64, f64) = (18.0, 80.0);
/// How much a pond's shore wanders in and out, as a fraction of its radius.
const POND_WOBBLE: f64 = 0.22;
/// How far beyond the water the banks reach before the ground is its own, in meters: they
/// ease from the water's level up to the ground over it, so a stream along a slope gets
/// a soft valley side rather than a cut.
const BANK_REACH: f64 = 26.0;
/// The widest a stream's bed gets for a coarse tile, in meters from its middle.
pub const WIDEST_SEEN: f64 = 64.0;
/// How much deeper a widened bed goes per meter it was widened.
const SEEN_DEEPER: f64 = 0.16;
/// How far under the water the ground stands just past a stream's edge, where a vertex of a
/// triangle under the water may sit.
const UNDER_EDGE: f64 = 0.12;
/// How far out, times its half width, a stream's bed counts as under water.
const BED_MARGIN: f64 = 1.15;
/// Side of the cells that index the streams' segments, in meters.
const INDEX_CELL: f64 = 64.0;

/// One point of a stream's middle, in world meters; `at.y` is the water's level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StreamPoint {
    pub at: DVec3,
    pub half_width: f64,
    /// How far under the water its bed is.
    pub depth: f64,
}

/// Where a stream's water goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamEnd {
    Sea,
    /// Into [`Hydrology::ponds`]`[i]`.
    Pond(usize),
    /// Into [`Hydrology::streams`]`[i]`.
    Stream(usize),
    /// Out of the window.
    Edge,
}

/// A stream, from its spring to its end.
#[derive(Debug, Clone, PartialEq)]
pub struct Stream {
    pub points: Vec<StreamPoint>,
    /// Meters per second.
    pub speed: f64,
    pub end: StreamEnd,
}

/// A pond in a hollow, round with a wandering shore.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pond {
    pub center: DVec2,
    /// Its mean radius; the shore is never further than [`Pond::reach`].
    pub radius: f64,
    pub level: f64,
    pub depth: f64,
    seed: u64,
}

impl Pond {
    /// How far out its bank slopes down to it: a pond lies in a hollow of its own.
    fn bank_reach(&self) -> f64 {
        BANK_REACH.max(self.radius * 1.3)
    }

    /// The furthest its water goes from the center.
    pub fn reach(&self) -> f64 {
        self.radius * (1.0 + POND_WOBBLE)
    }

    /// The shore's distance from the center towards `dir` (a unit vector).
    /// Whether a point is on its water, `inward` meters in from the shore.
    pub fn covers(&self, p: DVec2, inward: f64) -> bool {
        let d = p.distance(self.center);
        d < self.shore((p - self.center) / d.max(1e-9)) - inward
    }

    /// Its shore's distance from the center, toward `dir` (a unit vector).
    pub fn shore(&self, dir: DVec2) -> f64 {
        self.radius * (1.0 + POND_WOBBLE * value(dir.x * 1.7 + 9.0, dir.y * 1.7 - 4.0, self.seed))
    }

    /// The ground at a point with this pond dug into `ground`, the ground's own height.
    fn dig(&self, p: DVec2, ground: f64) -> f64 {
        let d = p.distance(self.center);
        if d > self.reach() + self.bank_reach() {
            return ground;
        }
        let shore = self.shore((p - self.center) / d.max(1e-9));
        ground.min(bank(d, shore, self.level, self.depth, ground, self.bank_reach()))
    }
}

/// The ground a body of water makes at `d` meters from its middle, whose water reaches `half`
/// meters out: a bowl under the water, flat at the shore, then a low lip of [`LIP`] that keeps
/// the water in, and a bank that eases up to `ground` and gives way to it by [`BANK_REACH`].
fn bank(d: f64, half: f64, level: f64, depth: f64, ground: f64, reach: f64) -> f64 {
    let ease = |t: f64| {
        let t = t.clamp(0.0, 1.0);
        1.0 - (1.0 - t) * (1.0 - t)
    };
    if d < half {
        // A wide floor and sloping banks.
        let t = d / half;
        let bowl = level - depth * (1.0 - t * t * t);
        // By the shore, a shelf as steep as the lip outside it: the ground is near a straight
        // line across the water's edge, so the tiles' triangles cut it where it really is. A
        // steep bowl against a gentle lip put the edge by the outer vertex of every cell, a
        // staircase along the grid.
        let shelf = level - EDGE_SLOPE * (half - d);
        return shelf + (bowl - shelf) * smoothstep(EDGE_SHELF.0, EDGE_SHELF.1, half - d);
    }
    let out = d - half;
    // Down by the sea the water has no lip to hold it, and the bank never rises out of the sea.
    let lip = LIP * smoothstep(0.2, 1.2, level);
    let near = level + lip * ease(out / LIP_RUN) + (ground - level - lip).max(0.0) * ease(out / (reach * 0.5));
    let keep = smoothstep(reach * 0.5, reach, out);
    if level < 1.0 {
        return (near + (ground - near) * keep).min(ground.max(level - depth * 0.3));
    }
    near + (ground - near) * keep
}

/// The fresh water of a generated world.
#[derive(Debug, Clone, Default)]
pub struct Hydrology {
    pub streams: Vec<Stream>,
    pub ponds: Vec<Pond>,
    /// `(stream, first point of the segment)` by [`INDEX_CELL`].
    index: HashMap<(i64, i64), Vec<(u32, u32)>>,
    coast: Coast,
}

/// How far each cell of the drainage grid is from the coast, in meters: positive on land,
/// negative at sea.
#[derive(Debug, Clone, Default)]
struct Coast {
    min: DVec2,
    nx: usize,
    nz: usize,
    distance: Vec<f32>,
}

impl Coast {
    /// Two chamfer passes from every cell on the other side of the shore.
    fn of(grid: &Grid) -> Self {
        let (nx, nz) = (grid.nx, grid.nz);
        let spread = |source: &dyn Fn(usize) -> bool| {
            let mut d: Vec<f64> = (0..nx * nz).map(|i| if source(i) { 0.0 } else { f64::MAX }).collect();
            let diagonal = GRID * std::f64::consts::SQRT_2;
            let steps = [(-1i64, 0i64, GRID), (0, -1, GRID), (-1, -1, diagonal), (1, -1, diagonal)];
            let mut relax = |x: i64, z: i64, sign: i64| {
                let i = z as usize * nx + x as usize;
                for &(dx, dz, cost) in &steps {
                    let (px, pz) = (x + dx * sign, z + dz * sign);
                    if px >= 0 && pz >= 0 && (px as usize) < nx && (pz as usize) < nz {
                        let j = pz as usize * nx + px as usize;
                        if d[j] + cost < d[i] {
                            d[i] = d[j] + cost;
                        }
                    }
                }
            };
            for z in 0..nz as i64 {
                for x in 0..nx as i64 {
                    relax(x, z, 1);
                }
            }
            for z in (0..nz as i64).rev() {
                for x in (0..nx as i64).rev() {
                    relax(x, z, -1);
                }
            }
            d
        };
        let sea = |i: usize| grid.ground[i] <= 0.0;
        let to_sea = spread(&sea);
        let to_land = spread(&|i| !sea(i));
        // Cell middles are half a cell from the shore between them.
        let half = GRID * 0.5;
        let distance = to_sea
            .iter()
            .zip(&to_land)
            .map(|(&s, &l)| if l == 0.0 { (s.min(1e7) - half) as f32 } else { (half - l.min(1e7)) as f32 })
            .collect();
        Self { min: grid.min, nx, nz, distance }
    }

    fn at(&self, p: DVec2) -> f64 {
        if self.distance.is_empty() {
            return -1e7;
        }
        let g = (p - self.min) / GRID - 0.5;
        let (x0, z0) = (g.x.floor(), g.y.floor());
        let (tx, tz) = (g.x - x0, g.y - z0);
        let cell = |x: f64, z: f64| {
            if x < 0.0 || z < 0.0 || x >= self.nx as f64 || z >= self.nz as f64 {
                // Past the grid there is only the sea around the window.
                return -1e7;
            }
            self.distance[z as usize * self.nx + x as usize] as f64
        };
        let top = cell(x0, z0) + (cell(x0 + 1.0, z0) - cell(x0, z0)) * tx;
        let bottom = cell(x0, z0 + 1.0) + (cell(x0 + 1.0, z0 + 1.0) - cell(x0, z0 + 1.0)) * tx;
        top + (bottom - top) * tz
    }
}

/// A cell on the flood's queue, lowest first, then by index so the order is always the same.
#[derive(PartialEq)]
struct Open(f64, usize);

impl Eq for Open {}

impl Ord for Open {
    fn cmp(&self, other: &Self) -> Ordering {
        other.0.total_cmp(&self.0).then(other.1.cmp(&self.1))
    }
}

impl PartialOrd for Open {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

const NONE: usize = usize::MAX;

/// The drainage grid over the window.
struct Grid {
    min: DVec2,
    nx: usize,
    nz: usize,
    ground: Vec<f64>,
    wet: Vec<f64>,
}

impl Grid {
    fn at(&self, i: usize) -> DVec2 {
        self.min + DVec2::new((i % self.nx) as f64 + 0.5, (i / self.nx) as f64 + 0.5) * GRID
    }

    fn neighbours(&self, i: usize) -> impl Iterator<Item = usize> + '_ {
        let (x, z) = ((i % self.nx) as i64, (i / self.nx) as i64);
        [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)].into_iter().filter_map(
            move |(dx, dz)| {
                let (x, z) = (x + dx, z + dz);
                (x >= 0 && z >= 0 && (x as usize) < self.nx && (z as usize) < self.nz)
                    .then(|| z as usize * self.nx + x as usize)
            },
        )
    }

    fn edge(&self, i: usize) -> bool {
        let (x, z) = (i % self.nx, i / self.nx);
        x == 0 || z == 0 || x + 1 == self.nx || z + 1 == self.nz
    }
}

impl Hydrology {
    /// Finds the streams and ponds of `relief`'s window, from its ground without them.
    pub fn generate(relief: &Relief) -> Self {
        let window = relief.map().desc.window;
        if window.width <= 0.0 || window.height <= 0.0 {
            return Self::default();
        }
        let seed = relief.seed().wrapping_add(0x5157);
        let grid = window_grid(relief);
        let n = grid.ground.len();
        let (_, receiver, order) = flood(&grid);
        let cell_km2 = GRID * GRID / 1e6;
        let mut gathered: Vec<f64> = grid.wet.iter().map(|w| w * cell_km2).collect();
        for &cell in order.iter().rev() {
            if receiver[cell] != NONE {
                gathered[receiver[cell]] += gathered[cell];
            }
        }

        let (ponds, hollow_of) = find_ponds(relief, &grid, &gathered, seed);
        let land = |i: usize| grid.ground[i] > 0.0;
        let in_pond = |i: usize| {
            let p = grid.at(i);
            ponds.iter().position(|pond| p.distance(pond.center) < pond.reach())
        };
        let flows = |i: usize| land(i) && gathered[i] >= STREAM_FROM && in_pond(i).is_none() && hollow_of[i] == NONE;
        // Upstream, each stream cell follows the one that brings it most water.
        let mut main = vec![NONE; n];
        for i in 0..n {
            let r = receiver[i];
            if flows(i) && r != NONE && (main[r] == NONE || gathered[i] > gathered[main[r]]) {
                main[r] = i;
            }
        }
        // Each pond drains: from its deepest cell down the flood's way out of its hollow, which
        // passes its rim at the lowest point. The stream that starts there takes that way as
        // its head, so it leaves the pond itself.
        let mut outlets: Vec<(usize, Vec<usize>, usize)> = Vec::new();
        for (k, pond) in ponds.iter().enumerate() {
            let Some(deepest) = (0..n).min_by(|&a, &b| {
                grid.at(a).distance(pond.center).total_cmp(&grid.at(b).distance(pond.center)).then(a.cmp(&b))
            }) else {
                continue;
            };
            let mut head = Vec::new();
            let mut cell = deepest;
            while cell != NONE && (hollow_of[cell] == k || in_pond(cell) == Some(k)) {
                if in_pond(cell) == Some(k) {
                    head.clear();
                }
                head.push(cell);
                cell = receiver[cell];
            }
            if cell != NONE && !head.is_empty() {
                if flows(cell) {
                    main[cell] = NONE;
                }
                outlets.push((k, head, cell));
            }
        }
        let mut owner = vec![NONE; n];
        let mut paths: Vec<(Vec<usize>, StreamEnd, usize)> = Vec::new();
        for source in (0..n).filter(|&i| flows(i) && main[i] == NONE) {
            let id = paths.len();
            let mut path = vec![source];
            let mut cell = source;
            let end = loop {
                owner[cell] = id;
                let next = receiver[cell];
                if next == NONE {
                    break StreamEnd::Edge;
                }
                if !land(next) {
                    path.push(next);
                    break StreamEnd::Sea;
                }
                if let Some(pond) = in_pond(next) {
                    path.push(next);
                    break StreamEnd::Pond(pond);
                }
                // Into a pond's flat: across it to the pond, never up to where it spills.
                if hollow_of[next] != NONE {
                    let pond = hollow_of[next];
                    let mut at = next;
                    path.push(at);
                    let to_pond = |c: usize| grid.at(c).distance(ponds[pond].center);
                    while in_pond(at).is_none() {
                        let lower = grid
                            .neighbours(at)
                            .filter(|&c| hollow_of[c] == pond && to_pond(c) < to_pond(at))
                            .min_by(|&a, &b| to_pond(a).total_cmp(&to_pond(b)).then(a.cmp(&b)));
                        match lower {
                            Some(c) => {
                                at = c;
                                path.push(at);
                            }
                            None => break,
                        }
                    }
                    break StreamEnd::Pond(pond);
                }
                if main[next] != cell || !flows(next) {
                    path.push(next);
                    // Resolved to the stream once every path is known.
                    break StreamEnd::Stream(next);
                }
                path.push(next);
                cell = next;
            };
            paths.push((path, end, cell));
        }
        let mut outlet_of: HashMap<usize, usize> = HashMap::new();
        for (k, head, exit) in outlets {
            if let Some(id) = paths.iter().position(|(path, _, _)| path[0] == exit) {
                paths[id].0.splice(0..0, head);
                outlet_of.insert(id, k);
            } else if !land(exit) {
                let mut path = head;
                path.push(exit);
                outlet_of.insert(paths.len(), k);
                paths.push((path, StreamEnd::Sea, exit));
            }
        }
        let ends_in = |id: usize, paths: &[(Vec<usize>, StreamEnd, usize)]| {
            paths.iter().any(|(_, end, _)| matches!(end, StreamEnd::Stream(c) if owner[*c] == id))
        };
        // A short stream into a pond stays: it is what feeds it.
        let keep: Vec<bool> = (0..paths.len())
            .map(|id| paths[id].0.len() >= SHORTEST || ends_in(id, &paths) || matches!(paths[id].1, StreamEnd::Pond(_)))
            .collect();
        // Rivers first, so a tributary meets a level that is already known.
        let mut ids: Vec<usize> = (0..paths.len()).filter(|&id| keep[id]).collect();
        ids.sort_by(|&a, &b| gathered[paths[b].2].total_cmp(&gathered[paths[a].2]).then(a.cmp(&b)));
        let new_id: HashMap<usize, usize> = ids.iter().enumerate().map(|(new, &old)| (old, new)).collect();

        let mut hydro = Hydrology { streams: Vec::new(), ponds, index: HashMap::new(), coast: Coast::of(&grid) };
        for &id in &ids {
            let (path, end, _) = &paths[id];
            let end = match *end {
                StreamEnd::Stream(cell) => match new_id.get(&owner[cell]) {
                    Some(&into) => StreamEnd::Stream(into),
                    None => StreamEnd::Edge,
                },
                other => other,
            };
            let controls: Vec<DVec2> = path.iter().map(|&c| grid.at(c)).collect();
            let wet: Vec<f64> = path.iter().map(|&c| gathered[c]).collect();
            let cap = outlet_of.get(&id).map(|&k| hydro.ponds[k].level - OUTLET_UNDER);
            let mut stream = hydro.course(relief, &controls, &wet, end, cap, seed.wrapping_add(id as u64 * 7919));
            // An outlet starts at its pond's shore, not under the pond's own water.
            if let Some(&k) = outlet_of.get(&id) {
                let pond = hydro.ponds[k];
                let inside = stream
                    .points
                    .iter()
                    .take_while(|p| p.at.truncate_xz().distance(pond.center) < pond.radius * 0.8)
                    .count();
                stream.points.drain(..inside.min(stream.points.len().saturating_sub(2)));
            }
            // A tributary ends where it reaches the other's water: past it, its meanders would
            // carry it across and out the far side.
            if let StreamEnd::Stream(into) = stream.end {
                let main = &hydro.streams[into].points;
                let meets = stream.points.iter().position(|p| {
                    let at = p.at.truncate_xz();
                    main.iter().any(|q| q.at.truncate_xz().distance(at) < q.half_width * 0.8)
                });
                if let Some(i) = meets {
                    stream.points.truncate((i + 1).max(2));
                }
                // Coming from higher up, it eases down to the other's level over its last
                // `CONFLUENCE_RAMP` meters, a short run of rapids, so where the two meet the
                // water and the bed dug under it have one level, not a step. Taking more off
                // the further along keeps it from ever climbing.
                if let Some(last) = stream.points.last().map(|p| p.at) {
                    let below = main
                        .iter()
                        .min_by(|a, b| {
                            a.at.truncate_xz()
                                .distance(last.truncate_xz())
                                .total_cmp(&b.at.truncate_xz().distance(last.truncate_xz()))
                        })
                        .map_or(last.y, |q| q.at.y);
                    let drop = last.y - below;
                    if drop > 0.0 {
                        let n = stream.points.len();
                        let mut left = 0.0;
                        for k in (0..n).rev() {
                            if k + 1 < n {
                                left += stream.points[k].at.distance(stream.points[k + 1].at);
                            }
                            let t = (1.0 - left / CONFLUENCE_RAMP).clamp(0.0, 1.0);
                            if t <= 0.0 {
                                break;
                            }
                            stream.points[k].at.y -= drop * t * t * (3.0 - 2.0 * t);
                        }
                    }
                }
            }
            hydro.streams.push(stream);
        }
        hydro.build_index();
        hydro
    }

    /// A stream along grid cells: winding, smoothed, resampled, leveled.
    /// `cap` is the highest its water may stand: a pond's outlet starts at the pond's level.
    fn course(
        &self,
        relief: &Relief,
        controls: &[DVec2],
        wet: &[f64],
        end: StreamEnd,
        cap: Option<f64>,
        seed: u64,
    ) -> Stream {
        let ground = |p: DVec2| self.ponds_dig(p.x, p.y, relief.raw_height_at(p.x, p.y));
        // Round off the grid's eight directions, then let it find the valley's floor, where it
        // needs no cut, and wind it a little there.
        let mut line = chaikin(&chaikin(&relax(controls, 1)));
        line = resample(&line, VALLEY_STEP).0;
        for _ in 0..VALLEY_PASSES {
            line = seek_valley(&line, &ground);
            line = relax(&line, 2);
        }
        // Its width grows with the water it has gathered, which jumps where a tributary joins:
        // eased between the grid cells, so it widens over a few dozen meters, not at once.
        let half_of = |gathered: f64| (NARROWEST + WIDENS * (gathered - STREAM_FROM * 0.8).max(0.0).sqrt()).min(WIDEST);
        let last = wet.len().saturating_sub(2).min(wet.len() - 1);
        let half_at = |f: f64| {
            let x = f.clamp(0.0, 1.0) * last as f64;
            let (i, t) = (x.floor() as usize, x.fract());
            let (a, b) = (half_of(wet[i.min(last)]), half_of(wet[(i + 1).min(last)]));
            a + (b - a) * smoothstep(0.0, 1.0, t)
        };
        let length: f64 = line.windows(2).map(|w| w[0].distance(w[1])).sum::<f64>().max(1e-6);
        // How the ground falls along it, to wind wide on the flat and little down a slope.
        let line_grounds: Vec<f64> = line.iter().map(|p| ground(*p)).collect();
        let reach = (SLOPE_WINDOW / VALLEY_STEP).round().max(1.0) as usize;
        let flat_at = |i: usize| {
            let (a, b) = (i.saturating_sub(reach), (i + reach).min(line.len() - 1));
            let run = line[a].distance(line[b]).max(1.0);
            1.0 - smoothstep(GENTLE, STEEP, ((line_grounds[a] - line_grounds[b]) / run).abs())
        };
        let mut along = 0.0;
        let mut bent = line.clone();
        for i in 1..line.len().saturating_sub(1) {
            along += line[i].distance(line[i - 1]);
            let dir = (line[i + 1] - line[i - 1]).normalize_or_zero();
            let half = half_at(along / length) * (0.5 + 0.9 * flat_at(i));
            let side: f64 = MEANDERS
                .iter()
                .zip(0u64..)
                .map(|(&(wave, meters, per_half), k)| {
                    value(along / wave, 3.5 + k as f64 * 7.3, seed.wrapping_add(k)) * (meters + per_half * half)
                })
                .sum();
            // Bends grow in from the spring and the mouth, which stay where the ground puts them.
            let ends = smoothstep(0.0, 80.0, along) * smoothstep(0.0, 80.0, length - along);
            bent[i] += DVec2::new(-dir.y, dir.x) * side * ends;
        }
        let line = chaikin(&chaikin(&bent));
        let (fine, at) = resample(&line, STEP);
        let floor = match end {
            StreamEnd::Sea => MOUTH_LEVEL,
            StreamEnd::Pond(p) => self.ponds[p].level,
            StreamEnd::Stream(s) => {
                let to = *fine.last().unwrap_or(&DVec2::ZERO);
                self.streams[s]
                    .points
                    .iter()
                    .min_by(|a, b| a.at.truncate_xz().distance(to).total_cmp(&b.at.truncate_xz().distance(to)))
                    .map_or(MOUTH_LEVEL, |p| p.at.y)
            }
            StreamEnd::Edge => f64::MIN,
        };
        let total = at.last().copied().unwrap_or(0.0).max(1e-6);
        let mut grounds: Vec<f64> = Vec::with_capacity(fine.len());
        for p in &fine {
            let h = ground(*p);
            if matches!(end, StreamEnd::Sea) && h < MOUTH_DEPTH && !grounds.is_empty() {
                break;
            }
            grounds.push(h);
        }
        // The level closest to the ground that never climbs, then cut where that would stand
        // too far over it, so the water never floats over the land.
        let wanted: Vec<f64> = grounds.iter().map(|h| h - SUNK).collect();
        let mut levels = descending_fit(&wanted);
        let mut last = f64::MAX;
        for (level, want) in levels.iter_mut().zip(&wanted) {
            *level = level.min(want + FILL_MOST).min(last).max(floor);
            last = *level;
        }
        // What the floor raised must not stand above what comes before it.
        for i in (0..levels.len().saturating_sub(1)).rev() {
            levels[i] = levels[i].max(levels[i + 1]);
        }
        if let Some(cap) = cap {
            for level in &mut levels {
                *level = level.min(cap);
            }
        }
        // Down to the sea's own level over the last stretch, so the mouth never stands over it.
        if matches!(end, StreamEnd::Sea) {
            let last = at.get(levels.len().saturating_sub(1)).copied().unwrap_or(0.0);
            for (level, s) in levels.iter_mut().zip(&at) {
                let t = smoothstep(last - MOUTH_SINKS, last, *s);
                *level = level.min(*level + (MOUTH_LEVEL - *level) * t).max(MOUTH_LEVEL);
            }
        }
        // Into the sea it widens to a mouth, over the last stretch on land and past it.
        let mouth = if matches!(end, StreamEnd::Sea) { at.get(levels.len().saturating_sub(1)).copied() } else { None };
        let spring = cap.is_none();
        let end_at = at.get(levels.len().saturating_sub(1)).copied().unwrap_or(0.0);
        // Its own character: wider and shallower, or narrower and deeper, all along.
        let character = (hash(0, 0, seed ^ 0xC4A2_A27E) >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0;
        // How steeply the water falls at each point, over a few dozen meters each way.
        let window = (SLOPE_WINDOW / STEP).round().max(1.0) as usize;
        let steep_at = |i: usize| {
            let (a, b) = (i.saturating_sub(window), (i + window).min(levels.len().saturating_sub(1)));
            let run = (at[b] - at[a]).max(1.0);
            smoothstep(GENTLE, STEEP, (levels[a] - levels[b]).abs() / run)
        };
        // Falls: where the water drops more than `FALL_STEP` from one point to the next. Each
        // sheet is a little wider than the stream, and digs a plunge pool at its foot.
        let n = levels.len();
        let mut sheet = vec![false; n];
        let mut plunge = vec![(0.0f64, 0.0f64); n];
        let (mut height, mut since) = (0.0f64, f64::MAX);
        for i in 1..n {
            let drop = levels[i - 1] - levels[i];
            if drop > FALL_STEP {
                sheet[i - 1] = true;
                sheet[i] = true;
                height += drop;
                since = 0.0;
                continue;
            }
            since += at[i] - at[i - 1];
            if height > 0.0 {
                let share = 1.0 - since / (height * PLUNGE_PER_METER).clamp(PLUNGE_REACH.0, PLUNGE_REACH.1);
                if share <= 0.0 {
                    height = 0.0;
                } else {
                    plunge[i] = (smoothstep(0.0, 1.0, share), height);
                }
            }
        }
        let points: Vec<StreamPoint> = fine
            .iter()
            .zip(&at)
            .zip(&levels)
            .zip(&grounds)
            .enumerate()
            .map(|(i, (((p, s), &level), &under))| {
                let mut half = half_at(s / total);
                for (k, &(wave, share)) in BANK_WOBBLE.iter().enumerate() {
                    half *= 1.0 + value(s / wave, 9.1 + k as f64 * 4.7, seed.wrapping_add(17 + k as u64)) * share;
                }
                let steep = steep_at(i);
                // Pools, deep and narrow, between riffles, shallow and wide.
                let pool = value(s / POOL_WAVE, 2.7, seed.wrapping_add(31));
                half *= (FLAT_WIDER + (STEEP_NARROWER - FLAT_WIDER) * steep)
                    * (1.0 - POOL_NARROWER * pool)
                    * (1.0 + CHARACTER_WIDTH * character);
                half = half.min(WIDEST);
                let (pooled, fell) = plunge[i];
                if sheet[i] {
                    half *= FALL_SPREADS;
                }
                half *= 1.0 + PLUNGE_WIDER * pooled;
                if spring {
                    let t = smoothstep(0.0, SPRING_RUN, *s);
                    half = SPRING_WIDTH + (half - SPRING_WIDTH) * t;
                }
                half = half.max(NARROWEST_SHOWN);
                if let Some(mouth) = mouth {
                    half *= 1.0 + (ESTUARY_WIDENS - 1.0) * smoothstep(mouth - ESTUARY_LENGTH, mouth, *s).powf(1.6);
                }
                let mut depth = (DEEP_FROM + DEEP_PER_METER * half)
                    * (FLAT_DEEPER + (STEEP_SHALLOWER - FLAT_DEEPER) * steep)
                    * (1.0 + POOL_DEEPER * pool)
                    * (1.0 - CHARACTER_DEPTH * character);
                depth = depth.clamp(SHALLOWEST, DEEPEST) + pooled * (fell * PLUNGE_DEEPER).min(PLUNGE_DEEPEST);
                if mouth.is_some() {
                    // Past the beach the channel shoals onto the sea floor instead of ending in
                    // a step, which shows through the sea as a dark slab.
                    // Already on the beach, so no deep channel shows through the sea.
                    let t = smoothstep(end_at - MOUTH_SHOALS, end_at, *s).max(smoothstep(2.5, -0.2, under));
                    // Under the sea, barely deeper than its floor; on land, still a channel.
                    let least = if under < level { 0.3 } else { 0.6 };
                    depth += ((level - under).max(least) - depth) * t;
                }
                StreamPoint { at: DVec3::new(p.x, level, p.y), half_width: half, depth }
            })
            .collect();
        let drop = points.first().map_or(0.0, |p| p.at.y) - points.last().map_or(0.0, |p| p.at.y);
        let length = (points.len().max(2) - 1) as f64 * STEP;
        let speed = (0.5 + drop / length * 70.0).clamp(0.5, 1.8);
        Stream { points, speed, end }
    }

    fn build_index(&mut self) {
        self.index.clear();
        for (s, stream) in self.streams.iter().enumerate() {
            for (i, pair) in stream.points.windows(2).enumerate() {
                let reach = pair[0].half_width.max(pair[1].half_width).max(WIDEST_SEEN) + BANK_REACH;
                let (a, b) = (pair[0].at.truncate_xz(), pair[1].at.truncate_xz());
                let (lo, hi) = (a.min(b) - DVec2::splat(reach), a.max(b) + DVec2::splat(reach));
                let (x0, z0) = index_cell(lo);
                let (x1, z1) = index_cell(hi);
                for z in z0..=z1 {
                    for x in x0..=x1 {
                        self.index.entry((x, z)).or_default().push((s as u32, i as u32));
                    }
                }
            }
        }
    }

    /// The ground at a point with the ponds dug into it.
    fn ponds_dig(&self, x: f64, z: f64, ground: f64) -> f64 {
        let p = DVec2::new(x, z);
        self.ponds.iter().fold(ground, |h, pond| pond.dig(p, h))
    }

    /// The ground at a point with every pond and stream dug into it.
    pub fn dig(&self, x: f64, z: f64, ground: f64) -> f64 {
        self.dig_wide(x, z, ground, 0.0)
    }

    /// The same, with every stream's bed at least `wide` meters from its middle to its bank:
    /// a coarse tile, whose vertices are far apart, still has one in the bed. At most
    /// [`WIDEST_SEEN`].
    pub fn dig_wide(&self, x: f64, z: f64, ground: f64, wide: f64) -> f64 {
        let wide = wide.min(WIDEST_SEEN);
        let raw = ground;
        let ground = self.ponds_dig(x, z, ground);
        // In a pond, a stream that ends there never fills its deeper bowl back up.
        let in_pond = ground < raw;
        let p = DVec2::new(x, z);
        let Some(segments) = self.index.get(&index_cell(p)) else { return ground };
        // The nearest stream shapes the ground: its banks may also rise over it, into a lip.
        let mut nearest: Option<(f64, f64, f64, f64, f64)> = None;
        for &(s, i) in segments {
            let points = &self.streams[s as usize].points;
            let (a, b) = (points[i as usize], points[i as usize + 1]);
            let (d, t) = segment_distance(p, a.at.truncate_xz(), b.at.truncate_xz());
            let water = a.half_width + (b.half_width - a.half_width) * t;
            let half = water.max(wide);
            let out = d - half;
            if out > BANK_REACH || nearest.is_some_and(|(o, ..)| o <= out) {
                continue;
            }
            let level = a.at.y + (b.at.y - a.at.y) * t;
            let depth = a.depth + (b.depth - a.depth) * t;
            nearest = Some((out, half, water, level, depth));
        }
        match nearest {
            Some((out, half, water, level, depth)) => {
                let d = out + half;
                let h = if d < water {
                    bank(d, water, level, depth + wide * SEEN_DEEPER, ground, BANK_REACH)
                } else if d < half {
                    // Past the water's edge but within reach of a vertex of a triangle the water
                    // lies over: just under the water, so the ground between far-apart vertices
                    // never closes over it. A shallow wet margin up close, unseen from afar.
                    level - UNDER_EDGE - wide * SEEN_DEEPER
                } else {
                    // The bank as it would be at the water's edge, moved out to there.
                    bank(d - half + water, water, level, depth, ground, BANK_REACH)
                };
                if in_pond { h.min(ground) } else { h }
            }
            None => ground,
        }
    }

    /// Meters from a point to the coast, on the drainage grid (about [`GRID`] meters sharp):
    /// positive on land, negative at sea. Far outside the window, very negative.
    pub fn coast_distance(&self, x: f64, z: f64) -> f64 {
        self.coast.at(DVec2::new(x, z))
    }

    /// The middles of the sea cells of the drainage grid that touch the land, west to east
    /// within each row, north to south.
    pub fn coast_cells(&self) -> Vec<DVec2> {
        let c = &self.coast;
        (0..c.distance.len())
            .filter(|&i| (-GRID..0.0).contains(&(c.distance[i] as f64)))
            .map(|i| c.min + DVec2::new((i % c.nx) as f64 + 0.5, (i / c.nx) as f64 + 0.5) * GRID)
            .collect()
    }

    /// Meters from a point to the edge of the nearest stream or pond, as [`Hydrology::is_bed`]
    /// draws it, 0 inside one; infinite
    /// further than about 90 m from any stream (the reach of its index).
    pub fn fresh_distance(&self, x: f64, z: f64) -> f64 {
        let p = DVec2::new(x, z);
        let mut nearest = f64::INFINITY;
        for pond in &self.ponds {
            let d = p.distance(pond.center);
            if d < pond.reach() + 200.0 {
                nearest = nearest.min((d - pond.shore((p - pond.center) / d.max(1e-9)) * BED_MARGIN).max(0.0));
            }
        }
        if let Some(segments) = self.index.get(&index_cell(p)) {
            for &(s, i) in segments {
                let points = &self.streams[s as usize].points;
                let (a, b) = (points[i as usize], points[i as usize + 1]);
                let (d, t) = segment_distance(p, a.at.truncate_xz(), b.at.truncate_xz());
                nearest = nearest.min((d - (a.half_width + (b.half_width - a.half_width) * t) * BED_MARGIN).max(0.0));
            }
        }
        nearest
    }

    /// Whether a point is under fresh water: a stream's bed or a pond's.
    pub fn is_bed(&self, x: f64, z: f64) -> bool {
        let p = DVec2::new(x, z);
        if self.ponds.iter().any(|pond| {
            let d = p.distance(pond.center);
            d < pond.reach() && d < pond.shore((p - pond.center) / d.max(1e-9))
        }) {
            return true;
        }
        self.index.get(&index_cell(p)).is_some_and(|segments| {
            segments.iter().any(|&(s, i)| {
                let points = &self.streams[s as usize].points;
                let (a, b) = (points[i as usize], points[i as usize + 1]);
                let (d, t) = segment_distance(p, a.at.truncate_xz(), b.at.truncate_xz());
                d < (a.half_width + (b.half_width - a.half_width) * t) * BED_MARGIN
            })
        })
    }
}

/// The fresh water over a point, as the one surface that draws every stream and pond: see
/// [`Hydrology::fresh_at`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FreshPoint {
    pub level: f64,
    /// Meters past the edge of the water (below 0 on it).
    pub out: f64,
    /// Meters past the edge of the ground dug for it, `wide` from a stream's middle.
    pub out_dug: f64,
    /// The stream it belongs to, the first point of the segment and how far along it; `None`
    /// on a pond.
    pub stream: Option<(usize, usize, f64)>,
    /// Across the stream, from -1 to 1 at its edges.
    pub across: f64,
}

impl Hydrology {
    /// The water a point belongs to, if it is within `reach` meters of where the ground is dug
    /// for it (out to `wide` from a stream's middle, as [`Hydrology::dig_wide`]): the stream or
    /// pond whose water it is furthest into, so where two meet each point has one level.
    pub fn fresh_at(&self, p: DVec2, wide: f64, reach: f64) -> Option<FreshPoint> {
        let mut best: Option<FreshPoint> = None;
        for pond in &self.ponds {
            let d = p.distance(pond.center);
            if d > pond.reach() + reach {
                continue;
            }
            let out = d - pond.shore((p - pond.center) / d.max(1e-9));
            if out < reach && best.is_none_or(|b| out < b.out) {
                best = Some(FreshPoint { level: pond.level, out, out_dug: out, stream: None, across: 0.0 });
            }
        }
        if let Some(segments) = self.index.get(&index_cell(p)) {
            for &(s, i) in segments {
                let points = &self.streams[s as usize].points;
                let (a, b) = (points[i as usize], points[i as usize + 1]);
                let (from, to) = (a.at.truncate_xz(), b.at.truncate_xz());
                let (d, t) = segment_distance(p, from, to);
                let water = a.half_width + (b.half_width - a.half_width) * t;
                let (out, out_dug) = (d - water, d - water.max(wide.min(WIDEST_SEEN)));
                if out_dug >= reach || best.is_some_and(|b| b.out <= out) {
                    continue;
                }
                let along = (to - from).normalize_or_zero();
                let side = along.perp_dot(p - from).signum();
                best = Some(FreshPoint {
                    level: a.at.y + (b.at.y - a.at.y) * t,
                    out,
                    out_dug,
                    stream: Some((s as usize, i as usize, t)),
                    across: side * d / water.max(0.01),
                });
            }
        }
        best
    }
}

fn index_cell(p: DVec2) -> (i64, i64) {
    ((p.x / INDEX_CELL).floor() as i64, (p.y / INDEX_CELL).floor() as i64)
}

/// Distance from `p` to the segment `a`-`b`, and how far along it the nearest point is.
fn segment_distance(p: DVec2, a: DVec2, b: DVec2) -> (f64, f64) {
    let along = b - a;
    let t = ((p - a).dot(along) / along.length_squared().max(1e-9)).clamp(0.0, 1.0);
    ((a + along * t).distance(p), t)
}

/// The window's ground on the drainage grid, and how wet each cell's climate is.
/// How deep each hollow of the ground stands under where it would spill, on the drainage grid.
/// [`Relief`] fills its ground up by this, so all of it drains: hollows become flats, and the
/// deepest of those that a stream crosses get a pond.
pub(crate) fn basins(relief: &Relief) -> Field {
    let window = relief.map().desc.window;
    if window.width <= 0.0 || window.height <= 0.0 {
        return Field::default();
    }
    let grid = window_grid(relief);
    let (fill, _, _) = flood(&grid);
    let values = fill.iter().zip(&grid.ground).map(|(f, g)| f - g).collect();
    Field { min: grid.min, cell: GRID, nx: grid.nx, nz: grid.nz, values }
}

/// The drainage grid over the window and [`PAD`] around it.
fn window_grid(relief: &Relief) -> Grid {
    let window = relief.map().desc.window;
    let corner = DVec2::new(window.x, window.y) - DVec2::splat(PAD);
    sample_grid(relief, corner, DVec2::new(window.width, window.height) + DVec2::splat(PAD * 2.0))
}

/// Priority flood from the sea and the edge: each cell drains into the one that reached it
/// (`receiver`), `fill` is how high the water would stand in a hollow before it spills, and
/// `order` is the order the cells were reached in, from the sea up.
fn flood(grid: &Grid) -> (Vec<f64>, Vec<usize>, Vec<usize>) {
    let n = grid.ground.len();
    let mut fill = grid.ground.clone();
    let mut receiver = vec![NONE; n];
    let mut seen = vec![false; n];
    let mut open = BinaryHeap::new();
    for i in 0..n {
        if grid.ground[i] <= 0.0 || grid.edge(i) {
            seen[i] = true;
            open.push(Open(fill[i], i));
        }
    }
    let mut order = Vec::with_capacity(n);
    while let Some(Open(level, cell)) = open.pop() {
        order.push(cell);
        for next in grid.neighbours(cell) {
            if !seen[next] {
                seen[next] = true;
                fill[next] = grid.ground[next].max(level + 1e-4);
                receiver[next] = cell;
                open.push(Open(fill[next], next));
            }
        }
    }
    (fill, receiver, order)
}

fn sample_grid(relief: &Relief, corner: DVec2, size: DVec2) -> Grid {
    let min = corner * METERS_PER_UNIT;
    let span = size * METERS_PER_UNIT;
    let (nx, nz) = ((span.x / GRID).ceil() as usize, (span.y / GRID).ceil() as usize);
    let rows: Vec<usize> = (0..nz).collect();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(nz.max(1));
    let per = nz.div_ceil(threads.max(1)).max(1);
    let parts: Vec<Vec<(f64, f64)>> = std::thread::scope(|s| {
        let handles: Vec<_> = rows
            .chunks(per)
            .map(|part| {
                s.spawn(move || {
                    let mut out = Vec::with_capacity(part.len() * nx);
                    for &z in part {
                        for x in 0..nx {
                            let p = min + DVec2::new(x as f64 + 0.5, z as f64 + 0.5) * GRID;
                            out.push((relief.raw_height_at(p.x, p.y), relief.wetness_at(p.x, p.y)));
                        }
                    }
                    out
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("drainage sampler panicked")).collect()
    });
    let (ground, wet) = parts.into_iter().flatten().unzip();
    Grid { min, nx, nz, ground, wet }
}

/// Ponds in the deepest hollows the relief filled ([`basins`]), and for each cell, the pond whose
/// flat it is in.
fn find_ponds(relief: &Relief, grid: &Grid, gathered: &[f64], seed: u64) -> (Vec<Pond>, Vec<usize>) {
    let n = grid.ground.len();
    let filled: Vec<f64> = (0..n).map(|i| relief.basin_depth(grid.at(i))).collect();
    let hollow = |i: usize| grid.ground[i] > 1.0 && filled[i] > 0.25;
    let mut done = vec![false; n];
    // (how much it would hold, lowest cell, cells, whether a stream runs through it)
    let mut found: Vec<(f64, usize, Vec<usize>, bool)> = Vec::new();
    for start in 0..n {
        if done[start] || !hollow(start) {
            continue;
        }
        let (mut stack, mut cells, mut deepest, mut lowest) = (vec![start], Vec::new(), 0.0f64, start);
        done[start] = true;
        let mut held = 0.0;
        while let Some(cell) = stack.pop() {
            cells.push(cell);
            let depth = filled[cell];
            held += depth;
            if depth > deepest || (depth == deepest && cell < lowest) {
                deepest = depth;
                lowest = cell;
            }
            for next in grid.neighbours(cell) {
                if !done[next] && hollow(next) {
                    done[next] = true;
                    stack.push(next);
                }
            }
        }
        // A stream through a hollow would cut a trench out of it: it gets a pond instead.
        let crossed = gathered[lowest] >= STREAM_FROM;
        if deepest >= if crossed { CROSSED_POND_FROM } else { POND_FROM } {
            found.push((held, lowest, cells, crossed));
        }
    }
    found.sort_by(|a, b| b.3.cmp(&a.3).then(b.0.total_cmp(&a.0)).then(a.1.cmp(&b.1)));
    let mut ponds: Vec<Pond> = Vec::new();
    let mut dry_ponds = 0;
    let mut hollow_of = vec![NONE; n];
    for (_, lowest, cells, crossed) in found {
        if !crossed && dry_ponds == MOST_PONDS {
            break;
        }
        let center = grid.at(lowest);
        let area = cells.len() as f64 * GRID * GRID;
        let radius = ((area / std::f64::consts::PI).sqrt() * 0.6).clamp(POND_RADIUS.0, POND_RADIUS.1);
        if ponds.iter().any(|p| p.center.distance(center) < p.reach() + radius * 1.6 + BANK_REACH * 2.0) {
            continue;
        }
        let mut pond =
            Pond { center, radius, level: 0.0, depth: 1.8 + radius * 0.03, seed: seed.wrapping_add(lowest as u64) };
        // Its level is the lowest ground just outside its shore, and further out where its
        // square sheet of water reaches, so the water never stands over the ground there.
        let mut low = f64::MAX;
        for dir in directions() {
            let shore = pond.shore(dir);
            for k in 0..5 {
                let r = shore + (pond.reach() * 1.5 - shore) * k as f64 / 4.0;
                let p = center + dir * r;
                low = low.min(relief.raw_height_at(p.x, p.y));
            }
        }
        pond.level = low - 0.3;
        if pond.level > 1.0 {
            for &cell in &cells {
                hollow_of[cell] = ponds.len();
            }
            ponds.push(pond);
            dry_ponds += usize::from(!crossed);
        }
    }
    (ponds, hollow_of)
}

/// 64 unit directions around the circle, from square roots alone: see the plan's rule that
/// the generator's math must be the same on every machine.
fn directions() -> Vec<DVec2> {
    let mut dirs = vec![DVec2::X, DVec2::Y, DVec2::NEG_X, DVec2::NEG_Y];
    while dirs.len() < 64 {
        let mut next = Vec::with_capacity(dirs.len() * 2);
        for i in 0..dirs.len() {
            next.push(dirs[i]);
            next.push((dirs[i] + dirs[(i + 1) % dirs.len()]).normalize());
        }
        dirs = next;
    }
    dirs
}

/// Each point moved across the line to the lowest ground nearby, the ends kept.
fn seek_valley(line: &[DVec2], ground: &impl Fn(DVec2) -> f64) -> Vec<DVec2> {
    let last = line.len().saturating_sub(1);
    let steps = (VALLEY_LOOK / VALLEY_LOOK_STEP) as i32;
    (0..line.len())
        .map(|i| {
            if i == 0 || i == last {
                return line[i];
            }
            let dir = (line[i + 1] - line[i - 1]).normalize_or_zero();
            let side = DVec2::new(-dir.y, dir.x);
            (-steps..=steps)
                .map(|k| {
                    let off = f64::from(k) * VALLEY_LOOK_STEP;
                    let p = line[i] + side * off;
                    (ground(p) + off.abs() * VALLEY_COST, p)
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map_or(line[i], |(_, p)| p)
        })
        .collect()
}

/// The non-increasing line closest to `values` (least squares, pooling neighbors that climb).
fn descending_fit(values: &[f64]) -> Vec<f64> {
    let mut blocks: Vec<(f64, usize)> = Vec::new();
    for &v in values {
        blocks.push((v, 1));
        while blocks.len() > 1 {
            let (b, nb) = blocks[blocks.len() - 1];
            let (a, na) = blocks[blocks.len() - 2];
            if b / nb as f64 <= a / na as f64 {
                break;
            }
            blocks.pop();
            let top = blocks.len() - 1;
            blocks[top] = (a + b, na + nb);
        }
    }
    blocks.iter().flat_map(|&(sum, n)| std::iter::repeat_n(sum / n as f64, n)).collect()
}

/// Each point moved to the mean of the `reach` on either side, the ends kept.
fn relax(line: &[DVec2], reach: usize) -> Vec<DVec2> {
    let last = line.len().saturating_sub(1);
    (0..line.len())
        .map(|i| {
            let r = reach.min(i).min(last - i);
            let sum: DVec2 = line[i - r..=i + r].iter().copied().sum();
            sum / (2 * r + 1) as f64
        })
        .collect()
}

/// One round of corner cutting, which keeps the ends.
fn chaikin(line: &[DVec2]) -> Vec<DVec2> {
    if line.len() < 3 {
        return line.to_vec();
    }
    let mut out = Vec::with_capacity(line.len() * 2);
    out.push(line[0]);
    for pair in line.windows(2) {
        out.push(pair[0] * 0.75 + pair[1] * 0.25);
        out.push(pair[0] * 0.25 + pair[1] * 0.75);
    }
    out.push(line[line.len() - 1]);
    out
}

/// Points every `step` meters along a line, and how far along each one is.
fn resample(line: &[DVec2], step: f64) -> (Vec<DVec2>, Vec<f64>) {
    let mut out = vec![line[0]];
    let mut at = vec![0.0];
    let mut travelled = 0.0;
    let mut next = step;
    for pair in line.windows(2) {
        let length = pair[0].distance(pair[1]);
        while next <= travelled + length {
            out.push(pair[0].lerp(pair[1], (next - travelled) / length.max(1e-9)));
            at.push(next);
            next += step;
        }
        travelled += length;
    }
    if travelled > at[at.len() - 1] + step * 0.25 {
        out.push(line[line.len() - 1]);
        at.push(travelled);
    }
    (out, at)
}

trait Xz {
    fn truncate_xz(self) -> DVec2;
}

impl Xz for DVec3 {
    fn truncate_xz(self) -> DVec2 {
        DVec2::new(self.x, self.z)
    }
}
