//! Water that the game can ask about: the same waves `scene.wgsl` draws, so what floats
//! rides the surface the player sees. A sea is a sum of Gerstner waves with fixed lengths,
//! and only their heights change with the weather, so nothing jumps when the wind rises.

use std::sync::Arc;

use glam::{DVec2, Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::{Origin, Rng};

/// How many waves the shader adds up. Mirrors `MAX_WAVES` in scene.wgsl.
pub const MAX_WAVES: usize = 12;
const GRAVITY: f32 = 9.81;
/// How many hollow hulls can keep the water out at a time, the nearest to the camera.
/// Mirrors `MAX_HULLS` in scene.wgsl.
pub const MAX_HULLS: usize = 8;
/// Texels on each side of a [`HullMap`].
pub const HULL_MAP_SIZE: u32 = 512;
/// How deep the water must be for the waves to keep all their height.
const SHOAL_DEPTH: f32 = 7.0;
/// What is left of the waves where the water ends: the shore still sees the tide come and go.
const SHOAL_LEAST: f32 = 0.05;
/// How often the Gerstner offset is undone to find what lies under a point.
const INVERSE_STEPS: usize = 3;
/// How far a crest bends off a straight line, in radians of its wave, and how many wavelengths
/// along the crest one bend takes. Mirrors scene.wgsl.
const CREST_BEND: f32 = 1.6;
const BEND_SPAN: f32 = 6.0;
/// How much a wave swells and fades in its groups, which is what makes its crests end, and how
/// many wavelengths a group takes along the crest and along the wave. Mirrors scene.wgsl.
const GROUP_DEPTH: f32 = 0.45;
const GROUP_ACROSS: f32 = 4.0;
const GROUP_ALONG: f32 = 7.0;

/// The waves of a sea, as written in scene files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SeaDesc {
    /// Height of the largest swell above the mean level, in meters, in a normal wind.
    pub swell: f32,
    /// Length of the largest swell, in meters.
    pub length: f32,
    /// Height of the short waves over the swell, in meters.
    pub chop: f32,
    /// How pointed the crests are, 0 (round) to 1.
    pub steepness: f32,
    /// How far the waves fan out from the wind, in degrees.
    pub spread: f32,
    /// Which way the swell runs, in degrees from +X; `None` follows the wind.
    pub direction: Option<f32>,
    pub seed: u64,
    /// How far the sea is drawn, in meters. Past the waved rings (about 1.8 km), flat coarse
    /// rings go on to here, a little lower; 0 stops at the waved rings.
    pub reach: f32,
}

impl Default for SeaDesc {
    fn default() -> Self {
        Self {
            swell: 0.9,
            length: 62.0,
            chop: 0.16,
            steepness: 0.72,
            spread: 50.0,
            direction: None,
            seed: 7,
            reach: 0.0,
        }
    }
}

/// One Gerstner wave. `k` is its direction times `2π / length`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Wave {
    pub k: Vec2,
    pub omega: f32,
    pub height: f32,
}

/// What the origin adds to each phase of a wave in [`Wave::at`]: the part of `k · x` that
/// local positions leave out, worked out in `f64` and wrapped to one turn, so a wave stays
/// where it was when the origin moves, however far that is. `scene.wgsl` gets the same.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WavePhase {
    pub along: f32,
    pub bend: f32,
    pub group: f32,
}

impl Wave {
    /// The phases the origin adds to this wave.
    pub fn phase(&self, origin: Origin) -> WavePhase {
        let at = DVec2::new(origin.0.x, origin.0.z);
        let across = self.k.perp();
        WavePhase {
            along: crate::origin::phase(at, self.k),
            bend: crate::origin::phase(at, across / BEND_SPAN),
            group: crate::origin::phase(at, across / GROUP_ACROSS + self.k / GROUP_ALONG),
        }
    }

    /// This wave at a point, being number `index` of its sea: where it is in its cycle, how much
    /// of its height it has there, and which way its phase grows, which a straight crest would
    /// have as `k`. Mirrors `wave_at` in scene.wgsl.
    fn at(&self, index: usize, rest: Vec2, time: f32, origin: WavePhase) -> (f32, f32, Vec2) {
        let across = self.k.perp();
        let bend = across.dot(rest) / BEND_SPAN + origin.bend + index as f32 * 2.4;
        let phase = self.k.dot(rest) + origin.along + CREST_BEND * bend.sin() - self.omega * time;
        let group = across.dot(rest) / GROUP_ACROSS + self.k.dot(rest) / GROUP_ALONG + origin.group
            - self.omega * time / (2.0 * GROUP_ALONG)
            + index as f32 * 1.1;
        (phase, 1.0 + GROUP_DEPTH * group.sin(), self.k + across * (CREST_BEND / BEND_SPAN) * bend.cos())
    }
}

/// A sum of waves frozen in shape; the weather scales only their heights.
#[derive(Debug, Clone, PartialEq)]
pub struct Sea {
    pub waves: Vec<Wave>,
    pub steepness: f32,
    /// `Σ k h` of the waves at their written height.
    slope: f32,
}

impl Sea {
    /// Long swells first, crossing each other on either side of the main direction, since swells
    /// running side by side only add up to long straight lines. Then shorter and more scattered
    /// ones. Each is much lower than its length, so crests never fold over themselves.
    pub fn new(desc: &SeaDesc, wind: Vec2) -> Self {
        const SWELLS: usize = 4;
        const SWELL_SHARE: [f32; SWELLS] = [0.55, 0.42, 0.3, 0.2];
        // Where each swell runs, in `spread`s from the main direction.
        const SWELL_ANGLE: [f32; SWELLS] = [0.2, -0.55, 0.85, -1.15];
        const SHORTEST: f32 = 0.045;
        let mut rng = Rng::new(desc.seed);
        let main = desc.direction.map_or(wind.y.atan2(wind.x), f32::to_radians);
        let mut waves = Vec::with_capacity(MAX_WAVES);
        for i in 0..MAX_WAVES {
            let t = i as f32 / (MAX_WAVES - 1) as f32;
            let length = desc.length.max(1.0) * SHORTEST.powf(t) * (1.0 + rng.jitter(0.09));
            let scatter = rng.jitter(1.0);
            let fan = SWELL_ANGLE.get(i).map_or(1.5 * scatter, |angle| angle + 0.1 * scatter);
            let angle = main + desc.spread.to_radians() * fan;
            let number = std::f32::consts::TAU / length;
            let height = match SWELL_SHARE.get(i) {
                Some(share) => desc.swell * share,
                // The chop: shorter ones are lower, and a little less steep than the swells.
                None => {
                    let left = 1.0 - (i - SWELLS) as f32 / (MAX_WAVES - SWELLS - 1) as f32;
                    desc.chop * (0.1 + 0.9 * left * left)
                }
            };
            waves.push(Wave {
                k: Vec2::new(angle.cos(), angle.sin()) * number,
                omega: (GRAVITY * number).sqrt(),
                height: height.max(0.0),
            });
        }
        let slope = waves.iter().map(|w| w.k.length() * w.height).sum();
        Self { waves, steepness: desc.steepness.clamp(0.0, 1.0), slope }
    }

    /// How much of its height a wave also moves sideways, with the weather making them
    /// `scale` times as high. Crests fold over where `Σ k h lean` reaches 1, so a storm
    /// sharpens them up to there and no further. See `gerstner` in scene.wgsl.
    pub fn lean(&self, scale: f32) -> f32 {
        (self.steepness / self.slope.max(0.05)).min(0.95 / (self.slope * scale * (1.0 + GROUP_DEPTH)).max(0.05))
    }

    /// What the origin adds to each wave; see [`WavePhase`].
    pub fn phases(&self, origin: Origin) -> Vec<WavePhase> {
        self.waves.iter().map(|wave| wave.phase(origin)).collect()
    }

    /// Where the waves move the water that rests at `rest`, a local position whose origin
    /// gave `phases`.
    pub fn offset(&self, rest: Vec2, time: f32, scale: f32, lean: f32, phases: &[WavePhase]) -> Vec3 {
        let mut sum = Vec3::ZERO;
        for (i, wave) in self.waves.iter().enumerate() {
            let (phase, group, _) = wave.at(i, rest, time, phases.get(i).copied().unwrap_or_default());
            let (sin, cos) = phase.sin_cos();
            let height = wave.height * group * scale;
            let side = wave.k.normalize_or_zero() * height * lean * cos;
            sum += Vec3::new(side.x, height * sin, side.y);
        }
        sum
    }

    /// The surface's normal where the water rests at `rest`.
    pub fn normal(&self, rest: Vec2, time: f32, scale: f32, lean: f32, phases: &[WavePhase]) -> Vec3 {
        let mut normal = Vec3::Y;
        for (i, wave) in self.waves.iter().enumerate() {
            let (phase, group, grows) = wave.at(i, rest, time, phases.get(i).copied().unwrap_or_default());
            let (sin, cos) = phase.sin_cos();
            let height = wave.height * group * scale;
            normal -= Vec3::new(grows.x * height * cos, wave.k.length() * height * lean * sin, grows.y * height * cos);
        }
        normal.normalize_or(Vec3::Y)
    }
}

/// Depth of the water under a sea, in a texture the shader reads too. Positive under the
/// water and negative on land, so waves can rise and break as the shore comes up.
#[derive(Debug, Clone, PartialEq)]
pub struct WaterBed {
    /// World position of the first sample, and of the last.
    pub min: Vec2,
    pub max: Vec2,
    pub width: u32,
    pub height: u32,
    pub depths: Vec<f32>,
}

impl WaterBed {
    /// Depth at a point; deep water beyond the map's edge.
    pub fn depth_at(&self, x: f32, z: f32) -> f32 {
        let size = self.max - self.min;
        let uv = (Vec2::new(x, z) - self.min) / size;
        if self.depths.is_empty() || uv.min_element() < 0.0 || uv.max_element() > 1.0 {
            return f32::MAX;
        }
        let at = uv * Vec2::new((self.width - 1) as f32, (self.height - 1) as f32);
        let (i, j) = (at.x as usize, at.y as usize);
        let (fx, fz) = (at.x - i as f32, at.y - j as f32);
        let (w, h) = (self.width as usize, self.height as usize);
        let sample = |i: usize, j: usize| self.depths[j.min(h - 1) * w + i.min(w - 1)];
        let near = sample(i, j) + (sample(i + 1, j) - sample(i, j)) * fx;
        let far = sample(i, j + 1) + (sample(i + 1, j + 1) - sample(i, j + 1)) * fx;
        near + (far - near) * fz
    }
}

/// How much of its height a wave keeps over water this deep. Mirrors `shoal` in scene.wgsl.
pub fn shoal(depth: f32) -> f32 {
    let t = (depth / SHOAL_DEPTH).clamp(0.0, 1.0);
    SHOAL_LEAST + (1.0 - SHOAL_LEAST) * t * t * (3.0 - 2.0 * t)
}

/// How a body of water looks, as written in scene files. All colors are sRGB.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WaterLook {
    /// The color of deep water, seen from above and from inside.
    pub deep: Vec3,
    /// The color it takes where it is thin over the bottom.
    pub shallow: Vec3,
    /// How many meters of water you can see through.
    pub clarity: f32,
    /// How much light the water itself gives back: 0 is glassy, 1 is milky or muddy.
    pub murk: f32,
    /// How much the surface bends what is under it.
    pub refraction: f32,
    /// How much foam: crests, shores and around what touches the water.
    pub foam: f32,
    pub foam_color: Vec3,
    /// How strong the light patterns on the bottom are.
    pub caustics: f32,
}

impl WaterLook {
    pub fn shallow_linear(&self) -> Vec3 {
        self.shallow.to_array().map(srgb_to_linear).into()
    }

    pub fn foam_linear(&self) -> Vec3 {
        self.foam_color.to_array().map(srgb_to_linear).into()
    }
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

impl Default for WaterLook {
    fn default() -> Self {
        Self {
            deep: Vec3::new(0.02, 0.2, 0.32),
            shallow: Vec3::new(0.2, 0.62, 0.6),
            clarity: 14.0,
            murk: 0.3,
            refraction: 0.6,
            foam: 1.0,
            foam_color: Vec3::new(0.9, 0.96, 0.95),
            caustics: 1.0,
        }
    }
}

/// What the renderer needs to know about water this frame: the waves of the sea, and the
/// body nearest the camera, whose bottom gets light patterns and whose shore gets wet.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WaterFrame {
    /// Whether there is any body of water near.
    pub active: bool,
    pub waves: [Wave; MAX_WAVES],
    pub lean: f32,
    /// Height of the nearest body's surface, without waves.
    pub level: f32,
    /// Where it is: everything between these two, in x and z.
    pub min: Vec2,
    pub max: Vec2,
    pub look: WaterLook,
    /// How far the camera is under the surface, in meters; negative above it.
    pub camera_depth: f32,
    /// From 0 to 1: how far the camera's picture has gone from dry to underwater.
    pub submerged: f32,
    /// How high the waves come up the beach above `level`, for the wet band of sand.
    pub reach: f32,
}

/// One point along a river's middle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RiverPoint {
    pub position: Vec3,
    /// Half the river's width here.
    pub half_width: f32,
}

/// Where a body of water is.
#[derive(Debug, Clone, PartialEq)]
pub enum WaterShape {
    /// Everywhere at this level, without end.
    Sea(Arc<Sea>),
    /// A level sheet.
    Sheet { center: Vec2, half: Vec2 },
    /// A strip that flows down its points, at `speed` meters per second. `bounds` is the box
    /// (min, max on X and Z) its water stays in: see [`WaterShape::river`].
    River { points: Vec<RiverPoint>, speed: f32, bounds: [Vec2; 2] },
}

impl WaterShape {
    /// A river along `points`, with the box around it that lets a query skip it.
    pub fn river(points: Vec<RiverPoint>, speed: f32) -> Self {
        let mut bounds = [Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)];
        for point in &points {
            let at = Vec2::new(point.position.x, point.position.z);
            bounds[0] = bounds[0].min(at - Vec2::splat(point.half_width));
            bounds[1] = bounds[1].max(at + Vec2::splat(point.half_width));
        }
        Self::River { points, speed, bounds }
    }
}

fn in_bounds(bounds: &[Vec2; 2], x: f32, z: f32) -> bool {
    x >= bounds[0].x && z >= bounds[0].y && x <= bounds[1].x && z <= bounds[1].y
}

/// A body of water the game can ask about.
#[derive(Debug, Clone, PartialEq)]
pub struct WaterBody {
    pub shape: WaterShape,
    pub level: f32,
    /// Height of a sheet's ripples, in meters.
    pub ripples: f32,
    pub look: WaterLook,
}

impl WaterBody {
    /// Moves it by `offset`, as the world's origin moves the other way.
    pub fn shift(&mut self, offset: Vec3) {
        let flat = Vec2::new(offset.x, offset.z);
        self.level += offset.y;
        match &mut self.shape {
            WaterShape::Sea(_) => {}
            WaterShape::Sheet { center, .. } => *center += flat,
            WaterShape::River { points, bounds, .. } => {
                for point in points {
                    point.position += offset;
                }
                for corner in bounds {
                    *corner += flat;
                }
            }
        }
    }
}

/// The surface above or below a point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaterPoint {
    /// Height of the surface.
    pub level: f32,
    pub normal: Vec3,
    /// How the water itself is moving.
    pub flow: Vec3,
    /// Which of [`Waters::bodies`].
    pub body: usize,
}

/// The underside of a hollow hull, seen from above in its own frame: for each spot, the lowest
/// point of the mesh. Water can't be above that and under the deck, which keeps it out of
/// an open boat or a hold, however the mesh is made inside.
#[derive(Debug, Clone, PartialEq)]
pub struct HullMap {
    /// The corners of the map, in meters along the hull's X and Z.
    pub min: Vec2,
    pub max: Vec2,
    pub deck: f32,
    /// [`HULL_MAP_SIZE`] squared, row by row along Z; [`HullMap::OFF`] where there's no hull.
    pub low: Vec<f32>,
}

impl HullMap {
    pub const OFF: f32 = 1e6;

    /// Whether a point in the hull's frame is inside it. Mirrors `in_hull` in scene.wgsl.
    pub fn holds(&self, local: Vec3) -> bool {
        if local.y >= self.deck {
            return false;
        }
        let at = (Vec2::new(local.x, local.z) - self.min) / (self.max - self.min).max(Vec2::splat(1e-5));
        if at.min_element() < 0.0 || at.max_element() >= 1.0 {
            return false;
        }
        let size = HULL_MAP_SIZE as usize;
        let (x, z) = ((at.x * size as f32) as usize, (at.y * size as f32) as usize);
        local.y > self.low[z.min(size - 1) * size + x.min(size - 1)]
    }
}

/// A hollow hull as it is now.
#[derive(Debug, Clone)]
pub struct WaterHull {
    pub map: Arc<HullMap>,
    pub position: Vec3,
    pub rotation: Quat,
}

/// Every body of water in a scene, and the moment they are at.
#[derive(Debug, Clone, Default)]
pub struct Waters {
    pub bodies: Vec<WaterBody>,
    pub bed: Option<Arc<WaterBed>>,
    /// The hollow hulls, where they are now.
    pub hulls: Vec<WaterHull>,
    pub time: f32,
    /// Height of the waves as a multiple of the scene's: the weather's wind.
    pub wave_scale: f32,
    pub wind: Vec2,
    pub wind_speed: f32,
    /// The world's origin, for the waves: see [`WavePhase`].
    pub origin: Origin,
    /// The level of every body around the camera, for the renderer: see [`Waters::levels`].
    /// Kept up to date by the world; a shift of the origin drops it.
    pub levels: Option<Arc<WaterLevels>>,
}

impl Waters {
    /// Moves every body, the bed and the hulls by `offset`, as the world's origin moves the
    /// other way: see [`Origin`].
    pub fn shift(&mut self, offset: Vec3, origin: Origin) {
        self.origin = origin;
        let flat = Vec2::new(offset.x, offset.z);
        for body in &mut self.bodies {
            body.shift(offset);
        }
        if let Some(bed) = &mut self.bed {
            let bed = Arc::make_mut(bed);
            bed.min += flat;
            bed.max += flat;
        }
        for hull in &mut self.hulls {
            hull.position += offset;
        }
        self.levels = None;
    }

    /// The surface over a point, if any body of water has it, the first listed first.
    pub fn in_hull(&self, point: Vec3) -> bool {
        self.hulls.iter().any(|hull| hull.map.holds(hull.rotation.inverse() * (point - hull.position)))
    }

    pub fn at(&self, x: f32, z: f32) -> Option<WaterPoint> {
        for (body, water) in self.bodies.iter().enumerate() {
            if let Some(point) = self.surface(water, body, x, z) {
                return Some(point);
            }
        }
        None
    }

    /// The level and flow over a point, without the waves: cheap enough to ask for each of
    /// hundreds of fish.
    pub fn calm_at(&self, x: f32, z: f32) -> Option<(f32, Vec3)> {
        self.bodies.iter().find_map(|water| match &water.shape {
            WaterShape::Sea(_) => Some((water.level, Vec3::ZERO)),
            WaterShape::Sheet { center, half } => {
                let d = Vec2::new(x, z) - *center;
                (d.x.abs() <= half.x && d.y.abs() <= half.y).then_some((water.level, Vec3::ZERO))
            }
            WaterShape::River { points, speed, bounds } => {
                if !in_bounds(bounds, x, z) {
                    return None;
                }
                river_at(points, x, z).map(|(level, flow)| (level, flow * *speed))
            }
        })
    }

    fn surface(&self, water: &WaterBody, body: usize, x: f32, z: f32) -> Option<WaterPoint> {
        match &water.shape {
            WaterShape::Sea(sea) => {
                let depth = self.bed.as_ref().map_or(f32::MAX, |bed| bed.depth_at(x, z));
                let scale = self.wave_scale * shoal(depth);
                let lean = sea.lean(self.wave_scale);
                let phases = sea.phases(self.origin);
                // What rests at `rest` moves over the point: undo the offset, from there.
                let target = Vec2::new(x, z);
                let mut rest = target;
                for _ in 0..INVERSE_STEPS {
                    let off = sea.offset(rest, self.time, scale, lean, &phases);
                    rest = target - Vec2::new(off.x, off.z);
                }
                let height = sea.offset(rest, self.time, scale, lean, &phases).y;
                let normal = sea.normal(rest, self.time, scale, lean, &phases);
                Some(WaterPoint { level: water.level + height, normal, flow: Vec3::ZERO, body })
            }
            WaterShape::Sheet { center, half } => {
                let d = Vec2::new(x, z) - *center;
                if d.x.abs() > half.x || d.y.abs() > half.y {
                    return None;
                }
                let (height, normal) = self.ripple(d.x, d.y, water.ripples);
                Some(WaterPoint { level: water.level + height, normal, flow: Vec3::ZERO, body })
            }
            WaterShape::River { points, speed, bounds } => {
                if !in_bounds(bounds, x, z) {
                    return None;
                }
                let (level, flow) = river_at(points, x, z)?;
                Some(WaterPoint { level, normal: Vec3::Y, flow: flow * *speed, body })
            }
        }
    }

    /// The little waves of a sheet at `x, z` from its middle, as `vs_water` moves them. They
    /// are fixed to the sheet, not to the world, so they don't care where the origin is.
    fn ripple(&self, x: f32, z: f32, height: f32) -> (f32, Vec3) {
        let t = self.time * self.wind_speed.max(0.2);
        let along = x * self.wind.x + z * self.wind.y;
        let across = -x * self.wind.y + z * self.wind.x;
        let h = height * self.wave_scale;
        let side = along * 0.7 + across * 2.1 + t * 0.8;
        let level = h * ((along * 1.3 - t * 1.1).sin() + 0.6 * side.sin());
        let rise_along = h * (1.3 * (along * 1.3 - t * 1.1).cos() + 0.42 * side.cos());
        let rise_across = h * 1.26 * side.cos();
        let slope = self.wind * rise_along + Vec2::new(-self.wind.y, self.wind.x) * rise_across;
        (level, Vec3::new(-slope.x, 1.0, -slope.y).normalize())
    }

    /// The scene's sea and its mean level.
    pub fn sea(&self) -> Option<(&Sea, f32)> {
        self.bodies.iter().find_map(|water| match &water.shape {
            WaterShape::Sea(sea) => Some((&**sea, water.level)),
            _ => None,
        })
    }
}

/// The level of the water around a spot, for each cell of a square grid fixed in local space:
/// every body at once, so the wet shores and the light under the water of each one show
/// wherever the camera is, not only those of the body nearest it. A cell up to `margin` from a
/// body takes its level too, which is what wets the bank above it. The renderer reads it as a
/// texture; past its edge, there is only the sea.
#[derive(Debug, Clone, PartialEq)]
pub struct WaterLevels {
    /// Where the first cell's corner is.
    pub min: Vec2,
    pub cell: f32,
    /// Cells on each side.
    pub size: usize,
    /// For each cell, row by row along +Z: level, clarity, how strong the caustics are, and
    /// how far the water is (0 inside it, up to `margin` on its bank). A cell with no water
    /// has a level of [`WaterLevels::NONE`].
    pub cells: Vec<[f32; 4]>,
    /// The sea's level, if there is one, for everywhere past the edge.
    pub sea: Option<f32>,
    /// What the bodies were when this was made, for whoever keeps it to tell when to redo it.
    pub key: u64,
}

impl WaterLevels {
    pub const NONE: f32 = -1e9;

    /// What [`WaterLevels::cells`] say at a point: the nearest cell, or the sea past the edge.
    pub fn level_at(&self, x: f32, z: f32) -> Option<f32> {
        let at = (Vec2::new(x, z) - self.min) / self.cell;
        if at.min_element() < 0.0 || at.max_element() >= self.size as f32 {
            return self.sea;
        }
        let level = self.cells[at.y as usize * self.size + at.x as usize][0];
        (level > Self::NONE).then_some(level)
    }

    /// The cell grid covering a square of `size` cells centered near `center`. The grid
    /// itself doesn't move with `center`, only which of its cells are in.
    pub fn place(center: Vec2, size: usize, cell: f32) -> Vec2 {
        ((center / cell).floor() - Vec2::splat((size / 2) as f32)) * cell
    }
}

impl Waters {
    /// The level of every body around `center` (see [`WaterLevels`]).
    pub fn levels(&self, center: Vec2, size: usize, cell: f32, margin: f32) -> WaterLevels {
        let min = WaterLevels::place(center, size, cell);
        let sea = self.sea().map(|(_, level)| level);
        let sea_cell = self.bodies.iter().find(|b| matches!(b.shape, WaterShape::Sea(_)));
        let base = match sea_cell {
            Some(body) => [body.level, body.look.clarity, body.look.caustics, 0.0],
            None => [WaterLevels::NONE, 1.0, 0.0, margin],
        };
        let mut cells = vec![base; size * size];
        // How near the best water so far is, measured past its edge: below 0 inside it.
        let mut nearest = vec![f32::MAX; size * size];
        let span = size as f32 * cell;
        // The cells whose middles fall in a box, clipped to the grid.
        let range = |lo: f32, hi: f32, from: f32| {
            let first = (((lo - from) / cell - 0.5).ceil().max(0.0)) as usize;
            let last = ((hi - from) / cell - 0.5).floor().min(size as f32 - 1.0);
            (first, if last < 0.0 { None } else { Some(last as usize) })
        };
        for body in &self.bodies {
            let look = [body.look.clarity, body.look.caustics];
            let mut mark = |i: usize, j: usize, gap: f32, level: f32| {
                let k = j * size + i;
                if gap <= margin && gap < nearest[k] {
                    nearest[k] = gap;
                    cells[k] = [level, look[0], look[1], gap.max(0.0)];
                }
            };
            match &body.shape {
                WaterShape::Sea(_) => {}
                WaterShape::Sheet { center, half } => {
                    let (lo, hi) = (*center - *half - margin, *center + *half + margin);
                    if hi.x < min.x || hi.y < min.y || lo.x > min.x + span || lo.y > min.y + span {
                        continue;
                    }
                    let ((i0, i1), (j0, j1)) = (range(lo.x, hi.x, min.x), range(lo.y, hi.y, min.y));
                    let (Some(i1), Some(j1)) = (i1, j1) else { continue };
                    for j in j0..=j1 {
                        for i in i0..=i1 {
                            let p = min + (Vec2::new(i as f32, j as f32) + 0.5) * cell;
                            let out = ((p - *center).abs() - *half).max_element();
                            mark(i, j, out, body.level);
                        }
                    }
                }
                WaterShape::River { points, bounds, .. } => {
                    let (lo, hi) = (bounds[0] - margin, bounds[1] + margin);
                    if hi.x < min.x || hi.y < min.y || lo.x > min.x + span || lo.y > min.y + span {
                        continue;
                    }
                    for pair in points.windows(2) {
                        let (a, b) = (&pair[0], &pair[1]);
                        let (from, to) = (Vec2::new(a.position.x, a.position.z), Vec2::new(b.position.x, b.position.z));
                        let reach = a.half_width.max(b.half_width) + margin;
                        let (lo, hi) = (from.min(to) - reach, from.max(to) + reach);
                        let ((i0, i1), (j0, j1)) = (range(lo.x, hi.x, min.x), range(lo.y, hi.y, min.y));
                        let (Some(i1), Some(j1)) = (i1, j1) else { continue };
                        let along = to - from;
                        for j in j0..=j1 {
                            for i in i0..=i1 {
                                let p = min + (Vec2::new(i as f32, j as f32) + 0.5) * cell;
                                let t = ((p - from).dot(along) / along.length_squared().max(1e-5)).clamp(0.0, 1.0);
                                let half = a.half_width + (b.half_width - a.half_width) * t;
                                let gap = (from + along * t).distance(p) - half;
                                mark(i, j, gap, a.position.y + (b.position.y - a.position.y) * t);
                            }
                        }
                    }
                }
            }
        }
        WaterLevels { min, cell, size, cells, sea, key: self.bodies_key() }
    }

    /// Changes when a body comes, goes or moves.
    pub fn bodies_key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        for body in &self.bodies {
            body.level.to_bits().hash(&mut hash);
            match &body.shape {
                WaterShape::Sea(_) => 0u8.hash(&mut hash),
                WaterShape::Sheet { center, half } => {
                    1u8.hash(&mut hash);
                    [center.x, center.y, half.x, half.y].map(f32::to_bits).hash(&mut hash);
                }
                WaterShape::River { points, .. } => {
                    2u8.hash(&mut hash);
                    points.len().hash(&mut hash);
                    for p in points.first().into_iter().chain(points.last()) {
                        p.position.to_array().map(f32::to_bits).hash(&mut hash);
                    }
                }
            }
        }
        hash.finish()
    }
}

/// The level and the direction of the flow where a river passes over a point.
fn river_at(points: &[RiverPoint], x: f32, z: f32) -> Option<(f32, Vec3)> {
    let p = Vec2::new(x, z);
    let mut best: Option<(f32, f32, Vec3)> = None;
    for pair in points.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let (from, to) = (Vec2::new(a.position.x, a.position.z), Vec2::new(b.position.x, b.position.z));
        let along = to - from;
        let t = ((p - from).dot(along) / along.length_squared().max(1e-5)).clamp(0.0, 1.0);
        let gap = (from + along * t).distance(p);
        let half = a.half_width + (b.half_width - a.half_width) * t;
        if gap <= half && best.is_none_or(|(g, ..)| gap < g) {
            let level = a.position.y + (b.position.y - a.position.y) * t;
            best = Some((gap, level, (b.position - a.position).normalize_or_zero()));
        }
    }
    best.map(|(_, level, flow)| (level, flow))
}

/// How high the waves stand for a wind of `wind` times the scene's (`Wind::scale`): 1 in the
/// scene's own wind, a calm sea in still air and a little over twice in a storm, since much of
/// a sea's swell comes from afar and the waves grow slower than the wind.
pub fn wave_scale(wind: f32) -> f32 {
    (0.35 + 0.65 * wind.max(0.0).powf(0.6)).clamp(0.3, 2.6)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sea() -> Waters {
        let sea = Sea::new(&SeaDesc::default(), Vec2::X);
        Waters {
            bodies: vec![WaterBody {
                shape: WaterShape::Sea(Arc::new(sea)),
                level: 2.0,
                ripples: 0.0,
                look: WaterLook::default(),
            }],
            wave_scale: 1.0,
            wind: Vec2::X,
            wind_speed: 1.0,
            ..Default::default()
        }
    }

    #[test]
    fn every_body_keeps_its_level_wherever_the_camera_is() {
        // A sea at 2 m, a river beside a beach and a pond inland.
        let mut waters = sea();
        let look = WaterLook { clarity: 3.0, ..WaterLook::default() };
        let river = (0..20)
            .map(|i| RiverPoint { position: Vec3::new(i as f32 * 10.0, 9.0 - i as f32 * 0.3, 40.0), half_width: 2.0 });
        waters
            .bodies
            .insert(0, WaterBody { shape: WaterShape::river(river.collect(), 1.0), level: 9.0, ripples: 0.0, look });
        let pond = WaterShape::Sheet { center: Vec2::new(100.0, -60.0), half: Vec2::splat(15.0) };
        waters.bodies.insert(1, WaterBody { shape: pond, level: 12.0, ripples: 0.0, look });
        // Seen from by the river, from by the pond and from the sea, each place says the same.
        for camera in [Vec2::new(50.0, 40.0), Vec2::new(100.0, -60.0), Vec2::new(-90.0, 10.0)] {
            let levels = waters.levels(camera, 256, 2.0, 6.0);
            let at = |x: f32, z: f32| levels.level_at(x, z).unwrap();
            assert!((at(50.0, 40.0) - 7.5).abs() < 0.2, "in the river, from {camera}");
            assert!((at(50.0, 46.5) - 7.5).abs() < 0.2, "on its bank, from {camera}");
            assert_eq!(at(100.0, -60.0), 12.0, "in the pond, from {camera}");
            assert_eq!(at(100.0, -78.0), 12.0, "on its shore, from {camera}");
            assert_eq!(at(50.0, -20.0), 2.0, "the beach goes by the sea, from {camera}");
        }
    }

    #[test]
    fn waves_fill_the_shader_and_do_not_fold() {
        let waters = sea();
        let WaterShape::Sea(sea) = &waters.bodies[0].shape else { unreachable!() };
        assert_eq!(sea.waves.len(), MAX_WAVES);
        for scale in [1.0, 3.0] {
            let folding: f32 = sea.waves.iter().map(|w| w.k.length() * w.height * scale * sea.lean(scale)).sum();
            assert!(folding <= 1.0, "{folding}");
        }
        // The largest swell comes first.
        assert!(sea.waves[0].k.length() < sea.waves[MAX_WAVES - 1].k.length());
    }

    #[test]
    fn the_surface_found_is_the_surface_drawn() {
        let mut waters = sea();
        waters.time = 12.5;
        let level = 2.0;
        let WaterShape::Sea(sea) = waters.bodies[0].shape.clone() else { unreachable!() };
        for rest in [Vec2::ZERO, Vec2::new(14.0, -3.0), Vec2::new(-80.0, 41.0)] {
            // Where the vertex resting at `rest` ends up, as `vs_water` moves it...
            let moved = Vec3::new(rest.x, 0.0, rest.y) + sea.offset(rest, waters.time, 1.0, sea.lean(1.0), &[]);
            // ...is how high the query says the water is there.
            let found = waters.at(moved.x, moved.z).unwrap();
            assert!((found.level - (level + moved.y)).abs() < 0.03, "{found:?} vs {moved}");
            assert!(found.normal.y > 0.5);
        }
    }

    #[test]
    fn waves_stay_put_when_the_origin_moves() {
        let mut near = sea();
        near.time = 7.25;
        near.bodies.insert(
            0,
            WaterBody {
                shape: WaterShape::Sheet { center: Vec2::new(30.0, 0.0), half: Vec2::splat(10.0) },
                level: 1.0,
                ripples: 0.2,
                look: WaterLook::default(),
            },
        );
        let check = |a: &Waters, b: &Waters, step: Vec3| {
            for p in [Vec2::new(0.0, 0.0), Vec2::new(31.0, 2.0), Vec2::new(-120.0, 47.5)] {
                let here = a.at(p.x, p.y).unwrap();
                let there = b.at(p.x - step.x, p.y - step.z).unwrap();
                assert_eq!(here.body, there.body);
                let close = (here.level - there.level).abs() < 2e-3 && here.normal.distance(there.normal) < 5e-3;
                assert!(close, "{here:?} {there:?}");
            }
        };
        // A jump of a few km, the way the player's walk makes them.
        let step = Vec3::new(2048.0, 0.0, -1280.0);
        let mut shifted = near.clone();
        shifted.shift(-step, Origin::new(step.as_dvec3()));
        check(&near, &shifted, step);
        // And the same far out in the world.
        let far = glam::DVec3::new(15_000_000.0, 0.0, -2_000_000.0);
        let mut there = near.clone();
        there.origin = Origin::new(far);
        let mut beyond = there.clone();
        beyond.shift(-step, Origin::new(far + step.as_dvec3()));
        check(&there, &beyond, step);
    }

    #[test]
    fn waves_die_down_at_the_shore() {
        assert!(shoal(100.0) > 0.99);
        assert!(shoal(-1.0) <= SHOAL_LEAST + 1e-4);
        let mut waters = sea();
        waters.bed = Some(Arc::new(WaterBed {
            min: Vec2::splat(-10.0),
            max: Vec2::splat(10.0),
            width: 2,
            height: 2,
            depths: vec![0.0; 4],
        }));
        let (mut shore, mut open) = (0.0f32, 0.0f32);
        for i in 0..40 {
            waters.time = i as f32 * 0.5;
            shore = shore.max((waters.at(0.0, 0.0).unwrap().level - 2.0).abs());
            open = open.max((waters.at(400.0, 0.0).unwrap().level - 2.0).abs());
        }
        assert!(shore < open * 0.25, "{shore} {open}");
    }

    #[test]
    fn rivers_flow_down_their_points() {
        let river = WaterBody {
            shape: WaterShape::river(
                vec![
                    RiverPoint { position: Vec3::new(0.0, 5.0, 0.0), half_width: 2.0 },
                    RiverPoint { position: Vec3::new(10.0, 3.0, 0.0), half_width: 2.0 },
                ],
                2.0,
            ),
            level: 0.0,
            ripples: 0.0,
            look: WaterLook::default(),
        };
        let waters = Waters { bodies: vec![river], wave_scale: 1.0, ..Default::default() };
        let point = waters.at(5.0, 1.0).unwrap();
        assert!((point.level - 4.0).abs() < 1e-4 && point.flow.x > 1.9);
        assert!(waters.at(5.0, 3.0).is_none());
    }
}
