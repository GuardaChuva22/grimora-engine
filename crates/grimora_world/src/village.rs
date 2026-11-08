//! Where the village stands: a place picked for a reason the world gives, beside a stream on
//! flat, dry ground, sheltered from the coast. The reason is kept, for the chronicle to come.

use glam::DVec2;

use crate::{GenId, METERS_PER_UNIT, Relief};

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
        (1.0 - ((d - half) / 1.0).clamp(0.0, 1.0)) as f32
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
    // (score, place, stream, stream's half width, unevenness, coast meters)
    let mut found: Vec<(f64, DVec2, usize, f64, f64, f64)> = Vec::new();
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
                let score = -uneven * 2.0 + p.half_width.min(6.0) * 1.5 - (h - 15.0).abs() * 0.05
                    - (coast / 1500.0 - 1.0).abs();
                found.push((score, c, s, p.half_width, uneven, coast));
            }
        }
    }
    // Best first; ties keep the stream's order, so the pick never depends on the sort.
    found.sort_by(|a, b| b.0.total_cmp(&a.0));
    let (_, center, s, half, uneven, coast) = found.into_iter().find(|f| {
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
    let reason = format!("{reason}; trilha de {:.0} m", trail.len() as f64 * TRAIL_STEP);
    Some(Village { id: GenId::place(relief.seed(), "village", center), center, level, reason, trail, trail_box: (lo, hi) })
}

/// From the village's middle toward the nearest edge of the map window, each step picking the
/// gentlest way within a turn of the goal; it stops at the window's edge or the beach.
fn walk_trail(relief: &Relief, center: DVec2) -> Vec<DVec2> {
    let w = &relief.map().desc.window;
    let (lo, hi) = (DVec2::new(w.x, w.y) * METERS_PER_UNIT, DVec2::new(w.x + w.width, w.y + w.height) * METERS_PER_UNIT);
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
