//! Water in a running world: what floats, and what the camera's picture has to know.

use std::sync::Arc;

use grimora_core::{Quat, Vec2, Vec3, WaterFrame, WaterHull, WaterLevels, WaterPoint, WaterShape};
use grimora_physics::Physics;
use grimora_scene::FloatHull;

use crate::world::{Body, World};

/// A body that the water holds up; see [`World::update_floating`].
#[derive(Debug, Clone)]
pub struct Floating {
    pub hull: Arc<FloatHull>,
    /// Whether any of it touched the water on the last step.
    pub wet: bool,
}

/// Kilograms per cubic meter.
const WATER_DENSITY: f32 = 1000.0;
const GRAVITY: f32 = 9.81;
/// How much the water's drag grows with speed: at this speed, it has doubled.
const DRAG_DOUBLES_AT: f32 = 2.5;
/// How far the push of the water tilts with the surface, which makes things slide off waves.
const SLOPE_PUSH: f32 = 0.5;
/// How quickly the picture goes from dry to underwater, per second.
const SUBMERGE_RATE: f32 = 7.0;
/// Height of the waves that reach the shore, on top of what `shoal` lets through.
const SWASH_REACH: f32 = 0.45;
/// How fast something has to hit the water to splash, and how fast is the biggest splash.
const SPLASH_FROM: f32 = 2.5;
const SPLASH_FULL: f32 = 9.0;
/// Meters past a river's banks, around the camera's stretch of it, that its bottom and shore are
/// drawn as under it.
const RIVER_VIEW_MARGIN: f32 = 6.0;
/// The map of water levels around the camera (`Waters::levels`): cells on a side, meters per
/// cell, and how far the camera goes before it is made again around it.
const LEVELS_SIZE: usize = 512;
const LEVELS_CELL: f32 = 2.0;
const LEVELS_MOVE: f32 = 128.0;
/// How far away a river can be heard, in meters.
const RIVER_HEARD: f32 = 40.0;
/// How high above the sea the waves stop being heard, in meters.
const SEA_HEARD: f32 = 70.0;
/// On a generated world, how far inland the surf carries, and how far out at sea it fades to a
/// murmur, in meters from the coast.
const SURF_INLAND: f32 = 450.0;
const SEA_OFF_SHORE: f32 = 1500.0;

impl World {
    /// The surface of the water over a point, and how it is moving, if there is any.
    pub fn water_at(&self, x: f32, z: f32) -> Option<WaterPoint> {
        self.waters.at(x, z)
    }

    /// The water that reaches a point: the same, but none inside a hollow hull, even under
    /// the level of the water outside. Ask this about a character or a camera.
    pub fn water_around(&self, point: Vec3) -> Option<WaterPoint> {
        if self.waters.in_hull(point) {
            return None;
        }
        self.waters.at(point.x, point.z)
    }

    /// How deep a point is under the water; negative above it, `None` where there isn't any.
    pub fn depth_in_water(&self, point: Vec3) -> Option<f32> {
        Some(self.water_around(point)?.level - point.y)
    }

    /// Pushes every [`Floating`] body with what its volume under the water displaces, piece
    /// by piece: where the pieces are is what rights it, tips it and rocks it on the waves.
    /// The water also slows each piece down, the more the more of it is under.
    pub fn update_floating(&mut self, physics: &mut Physics, time: f32) {
        self.waters.time = time;
        if self.waters.bodies.is_empty() {
            return;
        }
        let mut forces = Vec::new();
        let mut fell_in = Vec::new();
        self.waters.hulls.clear();
        for (Body(body), floating) in self.ecs.query_mut::<(&Body, &mut Floating)>() {
            let hull = &floating.hull;
            let Some((position, rotation)) = physics.body_pose(*body) else { continue };
            if let Some(map) = &hull.map {
                self.waters.hulls.push(WaterHull { map: map.clone(), position, rotation });
            }
            forces.clear();
            let mut column = u32::MAX;
            let mut surface = None;
            for cell in &hull.cells {
                let at = position + rotation * cell.center;
                if cell.column != column {
                    column = cell.column;
                    surface = self.waters.at(at.x, at.z);
                }
                let Some(water) = surface else { continue };
                let under = ((water.level - at.y + cell.half_height) / (2.0 * cell.half_height)).clamp(0.0, 1.0);
                if under <= 0.0 {
                    continue;
                }
                let displaced = cell.volume * under * WATER_DENSITY;
                let up = Vec3::Y.lerp(water.normal, SLOPE_PUSH);
                let mut force = up * displaced * GRAVITY;
                force += drag(physics.point_velocity(*body, at) - water.flow, rotation, hull.drag) * displaced;
                forces.push((force, at));
            }
            physics.set_forces(*body, &forces);
            let wet = !forces.is_empty();
            if wet && !floating.wet {
                let falling = -physics.point_velocity(*body, position).y;
                if falling > SPLASH_FROM {
                    fell_in.push((position, falling / SPLASH_FULL));
                }
            }
            floating.wet = wet;
        }
        for (position, strength) in fell_in {
            self.splash(position, strength);
        }
    }

    /// Something fell into the water here, `0..1` how hard: it throws water up and is heard.
    pub fn splash(&mut self, position: Vec3, strength: f32) {
        let Some(water) = self.waters.at(position.x, position.z) else { return };
        let at = Vec3::new(position.x, water.level, position.z);
        if let Some(effect) = self.splash_effect.clone() {
            self.play_effect(&effect, at);
        }
        self.splashes.push((at, strength.clamp(0.0, 1.0)));
    }

    /// Where something fell in since the last call, and how hard. The engine plays the
    /// sounds from these itself, so a game has no need to call this.
    pub fn take_splashes(&mut self) -> Vec<(Vec3, f32)> {
        std::mem::take(&mut self.splashes)
    }

    /// How loud, `0..1`, the sea and the rivers are to someone at `at`.
    pub(crate) fn water_noise(&self, at: Vec3) -> (f32, f32) {
        let mut sea = 0.0;
        if let Some((_, level)) = self.waters.sea() {
            let height = (1.0 - (at.y - level).abs() / SEA_HEARD).clamp(0.0, 1.0);
            // Louder over the shallows, and fading inland as the ground rises over the sea.
            // On a generated world, by how far the coast is.
            let coast = self.land.as_ref().map(|land| {
                let at = self.origin().to_world(at);
                land.relief().water().coast_distance(at.x, at.z) as f32
            });
            let shore = if let Some(d) = coast {
                if d < 0.0 {
                    (1.0 + d / SEA_OFF_SHORE).clamp(0.1, 1.0)
                } else {
                    (1.0 - d / SURF_INLAND).clamp(0.0, 1.0).powi(2)
                }
            } else {
                self.waters.bed.as_ref().map_or(0.5, |bed| {
                    let depth = bed.depth_at(at.x, at.z);
                    if depth < 0.0 {
                        (1.0 + depth / 12.0).clamp(0.05, 1.0)
                    } else {
                        (1.0 - depth / 40.0).clamp(0.4, 1.0)
                    }
                })
            };
            sea = height * shore;
        }
        let mut river = f32::MAX;
        for body in &self.waters.bodies {
            if let WaterShape::River { points, .. } = &body.shape {
                for point in points {
                    river = river.min(point.position.distance(at) - point.half_width);
                }
            }
        }
        let river = (1.0 - river.max(0.0) / RIVER_HEARD).clamp(0.0, 1.0);
        (sea, river * river)
    }

    /// Tells the renderer about the waves and about the water nearest `camera`, whose
    /// bottom and shore it shows, and how far under it the camera is.
    pub fn update_water_view(&mut self, camera: Vec3, dt: f32) {
        let wind = &self.environment.wind;
        self.waters.wave_scale = wind.scale.max(0.0);
        self.waters.wind = wind.direction.normalize_or(Vec2::X);
        self.waters.wind_speed = wind.speed;
        let submerged = self.environment.water.submerged;
        if self.waters.bodies.is_empty() {
            self.waters.levels = None;
            return;
        }
        self.update_water_levels(camera);
        let here = self.waters.at(camera.x, camera.z);
        let index = here.map_or_else(|| self.nearest_water(camera), |point| point.body);
        let body = &self.waters.bodies[index];
        let mut frame = WaterFrame { active: true, level: body.level, look: body.look, ..Default::default() };
        match &body.shape {
            WaterShape::Sea(_) => {
                frame.min = Vec2::splat(-1e7);
                frame.max = Vec2::splat(1e7);
            }
            WaterShape::Sheet { center, half } => {
                frame.min = *center - *half;
                frame.max = *center + *half;
            }
            WaterShape::River { points, .. } => {
                // Only around the stretch nearest the camera: a river's level changes along it,
                // and over its whole box, ground lower than that level here would look drowned.
                let p = Vec2::new(camera.x, camera.z);
                let near = points
                    .iter()
                    .min_by(|a, b| {
                        let d =
                            |q: &grimora_core::RiverPoint| Vec2::new(q.position.x, q.position.z).distance_squared(p);
                        d(a).total_cmp(&d(b))
                    })
                    .copied();
                if let Some(point) = near {
                    let at = Vec2::new(point.position.x, point.position.z);
                    let half = Vec2::splat(point.half_width * 1.5 + RIVER_VIEW_MARGIN);
                    frame.min = at - half;
                    frame.max = at + half;
                    frame.level = here.map_or(point.position.y, |here| here.level);
                }
            }
        }
        let scale = self.waters.wave_scale;
        if let Some((sea, _)) = self.waters.sea() {
            frame.lean = sea.lean(scale);
            for (slot, wave) in frame.waves.iter_mut().zip(&sea.waves) {
                *slot = grimora_core::water::Wave { height: wave.height * scale, ..*wave };
            }
            frame.reach =
                SWASH_REACH + frame.waves.iter().map(|w| w.height).sum::<f32>() * grimora_core::water::shoal(0.0);
        }
        // In a hold, the camera is dry.
        frame.camera_depth =
            if self.waters.in_hull(camera) { -1.0 } else { here.map_or(-1.0, |point| point.level - camera.y) };
        let target = if frame.camera_depth > 0.0 { 1.0f32 } else { 0.0 };
        let step = SUBMERGE_RATE * dt;
        frame.submerged = submerged + (target - submerged).clamp(-step, step);
        self.environment.water = frame;
    }

    /// Keeps `Waters::levels` around the camera, making it again once the camera has gone
    /// `LEVELS_MOVE` or the bodies have changed.
    fn update_water_levels(&mut self, camera: Vec3) {
        let at = Vec2::new(camera.x, camera.z);
        let min = WaterLevels::place(at, LEVELS_SIZE, LEVELS_CELL);
        let key = self.waters.bodies_key();
        let current = self.waters.levels.as_ref().is_some_and(|l| l.key == key && l.min.distance(min) < LEVELS_MOVE);
        if !current {
            let levels = self.waters.levels(at, LEVELS_SIZE, LEVELS_CELL, RIVER_VIEW_MARGIN);
            self.waters.levels = Some(Arc::new(levels));
        }
    }

    /// The body of water closest to a point that none of them cover.
    fn nearest_water(&self, at: Vec3) -> usize {
        let p = Vec2::new(at.x, at.z);
        let distance = |shape: &WaterShape| match shape {
            WaterShape::Sea(_) => 0.0,
            WaterShape::Sheet { center, half } => ((p - *center).abs() - *half).max(Vec2::ZERO).length(),
            WaterShape::River { points, .. } => points
                .iter()
                .map(|point| Vec2::new(point.position.x, point.position.z).distance(p))
                .fold(f32::MAX, f32::min),
        };
        let bodies = &self.waters.bodies;
        (0..bodies.len())
            .min_by(|&a, &b| distance(&bodies[a].shape).total_cmp(&distance(&bodies[b].shape)))
            .unwrap_or(0)
    }
}

/// The force per kilogram of water that slows something moving through it, along its own
/// axes: a hull cuts ahead more easily than it goes sideways.
fn drag(velocity: Vec3, rotation: Quat, per_axis: Vec3) -> Vec3 {
    let local = rotation.inverse() * velocity;
    rotation * (-local * per_axis * (1.0 + local.length() / DRAG_DOUBLES_AT))
}
