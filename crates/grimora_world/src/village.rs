//! Where the village stands: a place picked for a reason the world gives, beside a stream on
//! flat, dry ground, sheltered from the coast. The reason is kept, for the chronicle to come.

use glam::DVec2;

use grimora_scene::{HouseDesc, Trade};

use crate::{GenId, METERS_PER_UNIT, Relief, path_seed};

/// Meters of ground made flat around the village's middle.
pub const VILLAGE_FLAT: f64 = 45.0;
/// Meters past [`VILLAGE_FLAT`] over which the flat eases back into the land.
pub const VILLAGE_EASE: f64 = 40.0;
/// Nothing grows this far from the middle.
pub const VILLAGE_CLEAR: f64 = VILLAGE_FLAT + 10.0;
/// Meters walked along a stream between two places weighed.
const ALONG: f64 = 120.0;
/// Meters from the water's edge to the eased ground's edge: the flat never reaches the bed.
const SHORE_GAP: f64 = 15.0;
/// The most the ground may rise and fall around a place before it is flattened.
const MOST_UNEVEN: f64 = 8.0;
/// Below this the ground is beach; above it, a climb from the water.
const HEIGHTS: (f64, f64) = (4.0, 120.0);
/// Meters from the coast: closer is wind and spray, further is far from the sea's trade.
const COAST: (f64, f64) = (400.0, 6000.0);

/// The village's place.
#[derive(Debug, Clone)]
pub struct Village {
    pub id: GenId,
    /// World x, z in meters.
    pub center: DVec2,
    /// Height of its flat ground.
    pub level: f64,
    /// Why here, in words.
    pub reason: String,
    /// The trail from the village's edge out to the map window's edge (or the beach), world
    /// x, z in meters, a point every [`TRAIL_STEP`].
    pub trail: Vec<DVec2>,
    /// The trail's box, grown by [`TRAIL_WIDTH`]: `(min, max)`.
    pub trail_box: (DVec2, DVec2),
    /// Its houses, streets, well, field and boat.
    pub layout: Layout,
}

/// Meters between two points of the trail.
pub const TRAIL_STEP: f64 = 8.0;
/// Half the trail's width in meters.
pub const TRAIL_WIDTH: f64 = 1.4;

impl Village {
    /// `h`, the ground at `p`, made flat at the village and eased back around it.
    pub fn flatten(&self, p: DVec2, h: f64) -> f64 {
        let d = p.distance(self.center);
        if d >= VILLAGE_FLAT + VILLAGE_EASE {
            return h;
        }
        let t = ((d - VILLAGE_FLAT) / VILLAGE_EASE).clamp(0.0, 1.0);
        let t = t * t * (3.0 - 2.0 * t);
        self.level + (h - self.level) * t
    }

    /// Whether nothing may grow at `p`.
    pub fn clears(&self, p: DVec2) -> bool {
        p.distance(self.center) < VILLAGE_CLEAR || self.trail_distance(p, 2.0) < TRAIL_WIDTH + 1.0
    }

    /// Meters from `p` to the trail's middle line; anything past `reach` beyond its width may
    /// come back as infinity.
    pub fn trail_distance(&self, p: DVec2, reach: f64) -> f64 {
        let (lo, hi) = self.trail_box;
        let grow = DVec2::splat(reach);
        if p.cmplt(lo - grow).any() || p.cmpgt(hi + grow).any() {
            return f64::INFINITY;
        }
        self.trail
            .windows(2)
            .map(|w| {
                let (a, b) = (w[0], w[1]);
                let ab = b - a;
                let t = ((p - a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
                p.distance(a + ab * t)
            })
            .fold(f64::INFINITY, f64::min)
    }

    /// How much of the ground at `p` is the trail, for a tile with vertices `cell` meters apart:
    /// widened on coarse tiles so it stays in sight.
    pub fn trail_share(&self, p: DVec2, cell: f64) -> f32 {
        let half = TRAIL_WIDTH.max(cell * 0.6);
        let d = self.trail_distance(p, half + 1.0);
        let trail = 1.0 - ((d - half) / 1.0).clamp(0.0, 1.0);
        // Streets keep their own width, grown on coarse tiles so they stay in sight.
        let grown = (cell * 0.6 - STREET_WIDTH).max(0.0);
        let street = 1.0 - ((self.street_distance(p) - grown) / 1.0).clamp(0.0, 1.0);
        trail.max(street) as f32
    }
}

/// Picks the village's place in `relief`'s world, the same for the same seed: beside each
/// stream, every [`ALONG`] meters and on both banks, the ground is weighed for how flat, dry and
/// sheltered it is and how much water passes; the best wins.
pub fn choose_village(relief: &Relief) -> Option<Village> {
    let water = relief.water();
    let ring = |c: DVec2, r: f64| -> (f64, f64) {
        let mut low = f64::INFINITY;
        let mut high = f64::NEG_INFINITY;
        for k in 0..12 {
            let a = k as f64 * std::f64::consts::TAU / 12.0;
            let h = relief.height_at(c.x + a.cos() * r, c.y + a.sin() * r);
            low = low.min(h);
            high = high.max(h);
        }
        (low, high)
    };
    // (score, place, stream, stream's half width, unevenness, coast meters, bank and along)
    #[allow(clippy::type_complexity)]
    let mut found: Vec<(f64, DVec2, usize, f64, f64, f64, (DVec2, DVec2))> = Vec::new();
    for (s, stream) in water.streams.iter().enumerate() {
        let points = &stream.points;
        let mut walked = ALONG;
        for i in 1..points.len().saturating_sub(1) {
            let (a, b) = (points[i - 1].at, points[i].at);
            walked += DVec2::new(b.x - a.x, b.z - a.z).length();
            if walked < ALONG {
                continue;
            }
            walked = 0.0;
            let next = points[i + 1].at;
            let along = DVec2::new(next.x - a.x, next.z - a.z).normalize_or_zero();
            let side = DVec2::new(-along.y, along.x);
            let p = &points[i];
            let off = p.half_width + SHORE_GAP + VILLAGE_FLAT + VILLAGE_EASE;
            for sign in [1.0, -1.0] {
                let c = DVec2::new(p.at.x, p.at.z) + side * off * sign;
                let h = relief.height_at(c.x, c.y);
                let coast = water.coast_distance(c.x, c.y);
                if h < HEIGHTS.0 || h > HEIGHTS.1 || coast < COAST.0 || coast > COAST.1 {
                    continue;
                }
                if relief.cliff_at(c.x, c.y).face || relief.cliff_at(c.x, c.y).foot > 0.0 {
                    continue;
                }
                let (low, high) = ring(c, VILLAGE_FLAT);
                let uneven = high - low;
                if uneven > MOST_UNEVEN {
                    continue;
                }
                // Flat ground first, then a stream big enough to live by, then shelter.
                let score = -uneven * 2.0 + p.half_width.min(6.0) * 1.5
                    - (h - 15.0).abs() * 0.05
                    - (coast / 1500.0 - 1.0).abs();
                let bank = DVec2::new(p.at.x, p.at.z) + side * sign * (p.half_width + 1.5);
                found.push((score, c, s, p.half_width, uneven, coast, (bank, along)));
            }
        }
    }
    // Best first; ties keep the stream's order, so the pick never depends on the sort.
    found.sort_by(|a, b| b.0.total_cmp(&a.0));
    let (_, center, s, half, uneven, coast, bank) = found.into_iter().find(|f| {
        // No pond or other stream under the flat.
        water.fresh_distance(f.1.x, f.1.y) > VILLAGE_FLAT + VILLAGE_EASE
    })?;
    let level = relief.height_at(center.x, center.y);
    let sample = relief.map().sample_map(center / METERS_PER_UNIT);
    let band = sample.band.map_or("?", |b| b.name.as_str());
    let biome = sample.biome.map_or("?", |b| b.name.as_str());
    let stream_id = relief.stream_id(s).map_or(0, |id| id.0);
    let reason = format!(
        "beira do riacho {stream_id:016x} ({:.0} m de largura), chão plano ({uneven:.1} m de desnível em {VILLAGE_FLAT:.0} m), \
         {level:.0} m acima do mar, a {coast:.0} m da costa, {band}, {biome}",
        half * 2.0
    );
    let trail = walk_trail(relief, center);
    let grow = DVec2::splat(TRAIL_WIDTH);
    let lo = trail.iter().fold(center, |m, p| m.min(*p)) - grow;
    let hi = trail.iter().fold(center, |m, p| m.max(*p)) + grow;
    let count = house_count(half, coast);
    let reason = format!("{reason}; trilha de {:.0} m; {count} casas", trail.len() as f64 * TRAIL_STEP);
    let id = GenId::place(relief.seed(), "village", center);
    let layout = lay_out(id, center, &trail, count, Some(bank));
    Some(Village { id, center, level, reason, trail, trail_box: (lo, hi), layout })
}

/// From the village's middle toward the nearest edge of the map window, each step picking the
/// gentlest way within a turn of the goal; it stops at the window's edge or the beach.
fn walk_trail(relief: &Relief, center: DVec2) -> Vec<DVec2> {
    let w = &relief.map().desc.window;
    let (lo, hi) =
        (DVec2::new(w.x, w.y) * METERS_PER_UNIT, DVec2::new(w.x + w.width, w.y + w.height) * METERS_PER_UNIT);
    let goals = [
        (center.x - lo.x, DVec2::NEG_X),
        (hi.x - center.x, DVec2::X),
        (center.y - lo.y, DVec2::NEG_Y),
        (hi.y - center.y, DVec2::Y),
    ];
    let goal = goals.iter().fold(goals[0], |best, g| if g.0 < best.0 { *g } else { best }).1;
    let water = relief.water();
    let mut at = center;
    let mut heading = goal;
    let mut trail = vec![at];
    for _ in 0..4000 {
        let here = relief.height_at(at.x, at.y);
        let mut best = (f64::INFINITY, at, heading);
        for k in -4..=4 {
            let turn = k as f64 * 0.15;
            let dir = DVec2::from_angle(turn).rotate(heading);
            if dir.dot(goal) < 0.35 {
                continue;
            }
            let next = at + dir * TRAIL_STEP;
            let climb = (relief.height_at(next.x, next.y) - here).abs();
            let wet = if water.is_bed(next.x, next.y) { 4.0 } else { 0.0 };
            let cost = climb * 3.0 + wet + (1.0 - dir.dot(goal)) * 2.0 + (k as f64).abs() * 0.1;
            if cost < best.0 {
                best = (cost, next, dir);
            }
        }
        if !best.0.is_finite() {
            break;
        }
        (at, heading) = (best.1, best.2);
        trail.push(at);
        if at.cmplt(lo).any() || at.cmpgt(hi).any() || water.coast_distance(at.x, at.y) < 20.0 {
            break;
        }
    }
    trail
}

/// Half the width of the main street, and of a lane to a door.
pub const STREET_WIDTH: f64 = 1.6;
const LANE_WIDTH: f64 = 0.9;
/// How far the main street runs each way from the village's middle, meters.
const STREET_REACH: f64 = 38.0;
/// The largo's middle radius and how much its edge wanders.
const LARGO: (f64, f64) = (8.0, 2.5);
/// The fewest and most houses.
const HOUSES: (usize, usize) = (7, 11);

/// A house's lot.
#[derive(Debug, Clone)]
pub struct Lot {
    pub id: GenId,
    pub trade: Trade,
    /// The house's plan, the same for the same seed.
    pub desc: HouseDesc,
    /// World x, z of the house's middle.
    pub center: DVec2,
    /// Its turn about +Y (its front, +Z, looks at its street).
    pub yaw: f64,
    /// World x, z in front of the door, where its lane starts.
    pub door: DVec2,
}

/// The fenced field: middle, half sizes along its own x and z, and its turn about +Y.
#[derive(Debug, Clone, Copy)]
pub struct Field {
    pub center: DVec2,
    pub half: DVec2,
    pub yaw: f64,
}

/// What else a village has, past its houses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtraKind {
    /// The graveyard: graves in rows, a low fence round them.
    Cemetery,
    /// The beasts' pen, by the herder's house.
    Pen,
}

/// A fenced place of the village: middle, half sizes along its own x and z, turn about +Y.
#[derive(Debug, Clone, Copy)]
pub struct Extra {
    pub kind: ExtraKind,
    pub center: DVec2,
    pub half: DVec2,
    pub yaw: f64,
}

impl Extra {
    pub fn name(&self) -> &'static str {
        match self.kind {
            ExtraKind::Cemetery => "Cemitério",
            ExtraKind::Pen => "Curral",
        }
    }

    /// Whether `p` is inside it, grown by `grow`.
    pub fn contains(&self, p: DVec2, grow: f64) -> bool {
        turn(p - self.center, -self.yaw).abs().cmplt(self.half + grow).all()
    }
}

/// The largo: an irregular widening of the street, its edge `radii` meters from `center`,
/// evenly around.
#[derive(Debug, Clone, Default)]
pub struct Largo {
    pub center: DVec2,
    pub radii: Vec<f64>,
}

impl Largo {
    /// Meters from `p` to the largo's edge, negative inside.
    fn distance(&self, p: DVec2) -> f64 {
        if self.radii.is_empty() {
            return f64::INFINITY;
        }
        let to = p - self.center;
        let n = self.radii.len();
        let at = (to.y.atan2(to.x) / std::f64::consts::TAU).rem_euclid(1.0) * n as f64;
        let (i, t) = (at.floor() as usize % n, at.fract());
        let t = t * t * (3.0 - 2.0 * t);
        to.length() - (self.radii[i] + (self.radii[(i + 1) % n] - self.radii[i]) * t)
    }
}

/// Where the houses, streets and the rest go.
#[derive(Debug, Clone, Default)]
pub struct Layout {
    pub lots: Vec<Lot>,
    /// The main street and each door's lane, as (from, to, half width).
    pub streets: Vec<(DVec2, DVec2, f64)>,
    pub largo: Largo,
    pub well: DVec2,
    /// The small standing stone on the largo's edge.
    pub marker: DVec2,
    pub field: Option<Field>,
    /// The boat on the stream's bank, and its turn about +Y.
    pub boat: Option<(DVec2, f64)>,
    /// The graveyard and the pen, where they fit.
    pub extras: Vec<Extra>,
}

/// `v` (x, z) turned by `yaw` about +Y, as `Quat::from_rotation_y` turns it.
pub fn turn(v: DVec2, yaw: f64) -> DVec2 {
    let (s, c) = yaw.sin_cos();
    DVec2::new(v.x * c + v.y * s, -v.x * s + v.y * c)
}

/// How many houses the place holds: more beside a big stream and away from the coast.
fn house_count(half_width: f64, coast: f64) -> usize {
    let water = (half_width / 6.0).clamp(0.0, 1.0);
    let inland = ((coast - COAST.0) / 3000.0).clamp(0.0, 1.0);
    let share = 0.6 * water + 0.4 * inland;
    HOUSES.0 + ((HOUSES.1 - HOUSES.0) as f64 * share).round() as usize
}

/// A number from 0 to 1 for these parts, the same every time.
fn unit(parts: &[u64]) -> f64 {
    (path_seed(parts) >> 11) as f64 / (1u64 << 53) as f64
}

/// A rectangle on the ground, turned: middle, half sizes along its own x and z, turn about +Y.
#[derive(Clone, Copy)]
struct Plot {
    center: DVec2,
    half: DVec2,
    yaw: f64,
}

impl Plot {
    fn corners(&self, grow: f64) -> [DVec2; 4] {
        let h = self.half + grow;
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .map(|(x, z)| self.center + turn(DVec2::new(h.x * x, h.y * z), self.yaw))
    }

    /// Points spread over it: corners, edge middles and the middle.
    fn samples(&self) -> Vec<DVec2> {
        let mut points = Vec::new();
        for x in [-1.0, -0.5, 0.0, 0.5, 1.0] {
            for z in [-1.0, 0.0, 1.0] {
                points.push(self.center + turn(DVec2::new(self.half.x * x, self.half.y * z), self.yaw));
            }
        }
        points
    }

    fn contains(&self, p: DVec2, grow: f64) -> bool {
        let local = turn(p - self.center, -self.yaw);
        local.abs().cmplt(self.half + grow).all()
    }

    /// Whether the two overlap with `gap` between them (separating axes).
    fn overlaps(&self, other: &Plot, gap: f64) -> bool {
        let (a, b) = (self.corners(gap * 0.5), other.corners(gap * 0.5));
        let axes =
            [turn(DVec2::X, self.yaw), turn(DVec2::Y, self.yaw), turn(DVec2::X, other.yaw), turn(DVec2::Y, other.yaw)];
        axes.iter().all(|axis| {
            let span = |c: &[DVec2; 4]| {
                c.iter().map(|p| p.dot(*axis)).fold((f64::INFINITY, f64::NEG_INFINITY), |m, v| (m.0.min(v), m.1.max(v)))
            };
            let ((a0, a1), (b0, b1)) = (span(&a), span(&b));
            a0 < b1 && b0 < a1
        })
    }
}

fn segment_distance(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

fn polyline_distance(p: DVec2, line: &[DVec2]) -> f64 {
    line.windows(2).map(|w| segment_distance(p, w[0], w[1])).fold(f64::INFINITY, f64::min)
}

/// The nearest point of `line` to `p`.
fn nearest_on(p: DVec2, line: &[DVec2]) -> DVec2 {
    let mut best = (f64::INFINITY, p);
    for w in line.windows(2) {
        let ab = w[1] - w[0];
        let t = ((p - w[0]).dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
        let q = w[0] + ab * t;
        if p.distance(q) < best.0 {
            best = (p.distance(q), q);
        }
    }
    best.1
}

/// A village that grew along its street: the main street follows the stream's way and bends,
/// widening into a largo with the well and the tavern; houses stand on both sides with their
/// own setbacks and turns, a few behind them on lanes; the fisher on the stream's side, the
/// smith at an end, the field behind the houses.
fn lay_out(id: GenId, center: DVec2, trail: &[DVec2], count: usize, bank: Option<(DVec2, DVec2)>) -> Layout {
    let seed = id.0;
    let (along, bank_at) = match bank {
        Some((at, along)) => (along.normalize_or(DVec2::X), at),
        None => (DVec2::X, center + DVec2::Y),
    };
    let normal = DVec2::new(-along.y, along.x);
    // The side the stream is on.
    let near = if normal.dot(bank_at - center) >= 0.0 { 1.0 } else { -1.0 };
    let phase = unit(&[seed, 1]) * std::f64::consts::TAU;
    let phase2 = unit(&[seed, 2]) * std::f64::consts::TAU;
    let bend = |s: f64| 3.2 * (s / 15.0 + phase).sin() + 1.4 * (s / 6.5 + phase2).sin();
    let street_at = |s: f64| center + along * s + normal * (bend(s) - bend(0.0));
    let street: Vec<DVec2> = (-19..=19).map(|k| street_at(k as f64 * STREET_REACH / 19.0)).collect();
    // The largo, swelling out on one side of the street near the middle.
    let largo_s = (unit(&[seed, 3]) - 0.5) * 10.0;
    let largo_side = if unit(&[seed, 4]) < 0.5 { 1.0 } else { -1.0 };
    let largo_center = street_at(largo_s) + normal * largo_side * 3.0;
    let radii = (0..12).map(|k| LARGO.0 + (unit(&[seed, 5, k]) - 0.5) * 2.0 * LARGO.1).collect();
    let largo = Largo { center: largo_center, radii };

    // The trades a village always has, then the ones a bigger one adds, then homes.
    let mut trades = vec![
        Trade::Tavern,
        Trade::Smith,
        Trade::Healer,
        Trade::Fisher,
        Trade::Chief,
        Trade::Herder,
        Trade::Carpenter,
        Trade::Sexton,
        Trade::Home,
    ];
    trades.truncate(count.max(7));
    trades.resize(count.max(trades.len()), Trade::Home);
    let behind_from = trades.len() - if trades.len() >= 8 { 2 } else { 1 };
    let mut plots: Vec<Plot> = Vec::new();
    let mut lots = Vec::new();
    let mut streets: Vec<(DVec2, DVec2, f64)> = street.windows(2).map(|w| (w[0], w[1], STREET_WIDTH)).collect();
    let main = street.clone();
    let clear_of_ways = |plot: &Plot| {
        plot.samples().iter().all(|&p| {
            polyline_distance(p, &main) > STREET_WIDTH + 0.6
                && largo.distance(p) > 0.6
                && polyline_distance(p, trail) > TRAIL_WIDTH + 0.6
                && p.distance(center) < VILLAGE_FLAT - 3.0
        })
    };
    for (i, &trade) in trades.iter().enumerate() {
        let desc = HouseDesc::for_trade(trade, path_seed(&[seed, i as u64]));
        let half = desc.footprint().as_dvec2();
        let gap = 1.5 + unit(&[seed, 10, i as u64]) * 3.0;
        let behind = i >= behind_from;
        // Every place it could go, best first for its trade.
        let mut tries: Vec<(f64, f64, f64)> = Vec::new(); // (key, side, s)
        let mut s = -STREET_REACH + 4.0;
        while s <= STREET_REACH - 4.0 {
            for side in [1.0, -1.0] {
                let key = match trade {
                    Trade::Tavern => (s - largo_s).abs() + if side == largo_side { 0.0 } else { 100.0 },
                    Trade::Smith => -s.abs(),
                    Trade::Fisher => (s - largo_s).abs() * 0.3 + if side == near { 0.0 } else { 100.0 },
                    Trade::Chief => (s - largo_s).abs() + if side == near { 100.0 } else { 0.0 },
                    _ => unit(&[seed, 20, i as u64, s.to_bits(), side.to_bits()]),
                };
                tries.push((key, side, s));
            }
            s += 1.0;
        }
        tries.sort_by(|a, b| a.0.total_cmp(&b.0));
        for &(_, side, s) in &tries {
            let jitter = |k: u64| unit(&[seed, 30, i as u64, s.to_bits(), side.to_bits(), k]) - 0.5;
            let foot = street_at(s);
            let ahead = (street_at(s + 0.5) - street_at(s - 0.5)).normalize_or(along);
            let out = DVec2::new(-ahead.y, ahead.x) * side;
            let setback =
                1.5 + (jitter(0) + 0.5) * 3.0 + if behind { 2.0 * half.y + 6.0 + (jitter(3) + 0.5) * 3.0 } else { 0.0 };
            let (anchor, edge) = if trade == Trade::Tavern {
                // On the largo's far edge, looking into it.
                let dir = (foot - largo.center + out * 0.01).normalize_or(out);
                let dir = (dir + out * 2.0).normalize_or(out);
                let edge = largo.center + dir * (largo.distance(largo.center + dir * 50.0) * -1.0 + 50.0);
                (edge, dir)
            } else {
                (foot + out * STREET_WIDTH, out)
            };
            let mid = anchor + edge * (setback + half.y);
            let look = -edge;
            let yaw = look.x.atan2(look.y) + jitter(1) * 0.45;
            let plot = Plot { center: mid, half, yaw };
            if !clear_of_ways(&plot) || plots.iter().any(|p| p.overlaps(&plot, gap)) {
                continue;
            }
            let door = desc.front_door().as_dvec3();
            let door = mid + turn(DVec2::new(door.x, door.z + 1.4), yaw);
            // The lane from the door to the street, clear of the other houses.
            let to = if trade == Trade::Tavern { largo.center } else { nearest_on(door, &main) };
            let blocked = (0..=20).any(|k| {
                let p = door.lerp(to, k as f64 / 20.0);
                plots.iter().any(|q| q.contains(p, LANE_WIDTH + 0.3))
            });
            if blocked {
                continue;
            }
            streets.push((door, to, LANE_WIDTH));
            plots.push(plot);
            lots.push(Lot { id: GenId::place(seed, trade.name(), mid), trade, desc, center: mid, yaw, door });
            break;
        }
    }
    // The well on the largo, off its middle, and the marker on its edge.
    let a = unit(&[seed, 6]) * std::f64::consts::TAU;
    let well = largo.center + DVec2::new(a.cos(), a.sin()) * 2.5;
    let b = a + std::f64::consts::PI * (0.7 + unit(&[seed, 7]) * 0.6);
    let marker_dir = DVec2::new(b.cos(), b.sin());
    let marker = largo.center + marker_dir * (largo.distance(largo.center + marker_dir * 50.0) * -1.0 + 50.0 - 1.0);
    // The field behind the houses, on the side away from the stream, where it fits.
    let half = DVec2::new(7.0, 5.0);
    let mut field = None;
    let mut s: f64 = 0.0;
    for k in 0..60 {
        s = if k % 2 == 0 { s.abs() + 1.5 } else { -s };
        let foot = street_at(s.clamp(-STREET_REACH, STREET_REACH));
        let out = normal * -near;
        let depth = 14.0 + unit(&[seed, 40, k]) * 4.0;
        let at = foot + out * (STREET_WIDTH + depth + half.y);
        let yaw = out.x.atan2(out.y) + (unit(&[seed, 41, k]) - 0.5) * 0.4;
        let plot = Plot { center: at, half, yaw };
        if plot.corners(0.0).iter().all(|c| c.distance(center) < VILLAGE_FLAT + 4.0)
            && clear_of_ways(&Plot { center: at, half: half * 0.98, yaw })
            && !plots.iter().any(|p| p.overlaps(&plot, 2.0))
        {
            field = Some(Field { center: at, half, yaw });
            break;
        }
    }
    if let Some(f) = field {
        plots.push(Plot { center: f.center, half: f.half, yaw: f.yaw });
    }
    let mut extras = Vec::new();
    // The pen behind the herder's house, its gate toward it.
    // As big as fits, for the herd: it sleeps in there.
    'pen: for half in [DVec2::new(6.0, 4.5), DVec2::new(5.2, 4.0), DVec2::new(4.5, 3.5)] {
        let Some(herder) = lots.iter().find(|l| l.trade == Trade::Herder) else { break };
        let back = turn(DVec2::new(0.0, -1.0), herder.yaw);
        let side = turn(DVec2::new(1.0, 0.0), herder.yaw);
        let house = herder.desc.footprint().as_dvec2();
        // Behind the house first, then beside it, farther out each round.
        let mut tries = Vec::new();
        for deeper in [0.0, 3.0, 6.0, 9.0] {
            for sway in [0.0, 2.5, -2.5, 5.0, -5.0, 7.5, -7.5] {
                tries.push(herder.center + back * (house.y + half.y + 2.5 + deeper) + side * sway);
            }
            for s in [1.0, -1.0] {
                tries.push(herder.center + side * s * (house.x + half.x + 2.5 + deeper) + back * 2.0);
            }
        }
        let clear = |plot: &Plot| {
            plot.samples().iter().all(|&p| {
                polyline_distance(p, &main) > STREET_WIDTH + 0.6
                    && largo.distance(p) > 0.6
                    && polyline_distance(p, trail) > TRAIL_WIDTH + 0.6
            })
        };
        for at in tries {
            let plot = Plot { center: at, half, yaw: herder.yaw };
            if plot.corners(0.0).iter().all(|c| c.distance(center) < VILLAGE_FLAT + 4.0)
                && clear(&plot)
                && !plots.iter().any(|p| p.overlaps(&plot, 1.2))
            {
                plots.push(plot);
                extras.push(Extra { kind: ExtraKind::Pen, center: at, half, yaw: herder.yaw });
                break 'pen;
            }
        }
    }
    // The graveyard out at the edge of the flat ground, at an end of the street, away from the
    // stream: the dead are kept apart from the water and the houses.
    let half = DVec2::new(6.5, 5.5);
    'grave: for ring in [34.0, 30.0, 38.0, 26.0] {
        for k in 0..24 {
            let a = (k as f64 / 24.0 + unit(&[seed, 50])) * std::f64::consts::TAU;
            let dir = DVec2::new(a.cos(), a.sin());
            // Away from the stream first.
            if k < 16 && dir.dot(normal * near) > 0.2 {
                continue;
            }
            let at = center + dir * ring;
            let yaw = dir.x.atan2(dir.y) + (unit(&[seed, 51, k]) - 0.5) * 0.3;
            let plot = Plot { center: at, half, yaw };
            if plot.corners(0.0).iter().all(|c| c.distance(center) < VILLAGE_FLAT + 3.0)
                && clear_of_ways(&Plot { center: at, half: half * 0.98, yaw })
                && !plots.iter().any(|p| p.overlaps(&plot, 3.0))
            {
                // A path from its gate (its -Z side, toward the village) to the main street.
                let gate = at + turn(DVec2::new(0.0, -half.y - 0.6), yaw);
                streets.push((gate, nearest_on(gate, &main), LANE_WIDTH));
                // Inside, one path from the gate up the middle to the cross at the back.
                let inner = (at + turn(DVec2::new(0.0, -half.y), yaw), at + turn(DVec2::new(0.0, half.y - 1.5), yaw));
                streets.push((inner.0, inner.1, 0.6));
                extras.push(Extra { kind: ExtraKind::Cemetery, center: at, half, yaw });
                break 'grave;
            }
        }
    }
    let boat = bank.map(|(at, along)| (at, (-along.y).atan2(along.x)));
    Layout { lots, streets, largo, well, marker, field, boat, extras }
}

impl Village {
    /// Meters from `p` to the nearest street's or lane's edge, negative inside one, the largo
    /// or the field; infinity away from the village.
    fn street_distance(&self, p: DVec2) -> f64 {
        if p.distance(self.center) > VILLAGE_FLAT + 10.0 {
            return f64::INFINITY;
        }
        if let Some(field) = self.layout.field {
            let local = turn(p - field.center, -field.yaw);
            if local.abs().cmplt(field.half).all() {
                return -1.0;
            }
        }
        if self.layout.extras.iter().any(|e| e.kind == ExtraKind::Pen && e.contains(p, 0.0)) {
            return -1.0;
        }
        self.layout
            .streets
            .iter()
            .map(|&(a, b, width)| segment_distance(p, a, b) - width)
            .fold(self.layout.largo.distance(p), f64::min)
    }
}
