//! Rays: buried by day, out over the bottom when it's dark, and after crabs when hungry.

use std::sync::Arc;

use glam::{Mat3, Quat, Vec3};
use grimora_core::{ParticleEffect, Rng};
use grimora_scene::{SceneFauna, SpeciesDesc};

use super::crab::Colony;
use super::ecology::{Ledger, Population, fed_from, starving_from};
use super::encounter::{BodyPose, Carcass, Wake, laid_flat};
use super::{Place, Threat, flat, pose_index};

/// Beyond this it isn't drawn.
const DRAW_DISTANCE: f32 = 110.0;
/// It will not swim where there's less water than this.
const SHALLOWEST: f32 = 0.35;
/// Further than this, under the sand, nobody can tell one is there.
const UNSEEN: f32 = 30.0;
/// Under the sand it only knows about what's right on top of it, this far around and this far up:
/// it stays still until someone steps on it, which is what makes them dangerous.
const BURIED_NOTICE: f32 = 0.7;
const BURIED_NOTICE_UP: f32 = 2.5;
/// How far from its middle a blow gets it, times its size.
const RADIUS: f32 = 0.3;
/// Under this much daylight it comes out; over the second it goes back down.
const COMES_OUT: f32 = 0.45;
const GOES_IN: f32 = 0.6;
/// Hungry enough, it will hunt by day too.
const STARVING: f32 = 0.8;
const SATED: f32 = 0.25;
/// Seconds between decisions.
const THINK_SECONDS: f32 = 0.5;
/// Where its middle is, buried, from the sand, in lengths of the model: only its back shows.
const BURIED_AT: f32 = -0.035;
/// How fast it comes down to the sand, in meters per second.
const SETTLING: f32 = 0.7;
/// How big it seems to a crab, when it's after one.
const HUNTER_RADIUS: f32 = 1.0;
/// Seconds it will go after one crab.
const CHASE_SECONDS: f32 = 25.0;
/// Radians it leans into the sharpest turn, and how far it noses up or down.
const TURN_ROLL: f32 = 0.45;
const STEEPEST: f32 = 0.4;
/// Seconds of flat-out swimming after a fright.
const BOLT_SECONDS: f32 = 2.5;

/// What a ray is doing, for whoever asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RayDoing {
    Buried,
    Cruising,
    Hunting,
    Bolting,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Doing {
    Buried {
        left: f32,
    },
    /// On its way down: it stirs up the sand when it gets there.
    Settling {
        rest: f32,
    },
    Cruising {
        to: Vec3,
    },
    Hunting {
        at: Vec3,
        left: f32,
    },
    Bolting {
        left: f32,
    },
}

#[derive(Debug)]
pub(super) struct Ray {
    pub position: Vec3,
    pub velocity: Vec3,
    doing: Doing,
    pub rotation: Quat,
    pub phase: f32,
    pub bend: f32,
    pub size: f32,
    full_size: f32,
    pub shade: f32,
    hunger: f32,
    think_in: f32,
    height: f32,
    meals: u32,
    pub drawn: bool,
}

impl Ray {
    pub fn buried(&self) -> bool {
        matches!(self.doing, Doing::Buried { .. })
    }
}

/// Rays of one species around a place.
#[derive(Debug)]
pub(super) struct Rays {
    pub species: Arc<SpeciesDesc>,
    pub poses: usize,
    pub rays: Vec<Ray>,
    home: Vec3,
    rng: Rng,
    puff: Option<ParticleEffect>,
    ledger: Ledger,
}

impl Population for Rays {
    fn shift(&mut self, offset: Vec3) {
        self.home += offset;
        for ray in &mut self.rays {
            ray.position += offset;
            match &mut ray.doing {
                Doing::Cruising { to } => *to += offset,
                Doing::Hunting { at, .. } => *at += offset,
                _ => {}
            }
        }
    }

    fn near(&self, point: Vec3, radius: f32) -> bool {
        self.rays.iter().any(|ray| ray.position.distance(point) < radius)
    }

    fn species(&self) -> &SpeciesDesc {
        &self.species
    }

    fn ledger(&mut self) -> &mut Ledger {
        &mut self.ledger
    }

    fn number(&self) -> usize {
        self.rays.len()
    }

    fn fed(&self) -> f32 {
        fed_from(self.rays.iter().map(|ray| ray.hunger))
    }

    fn starving(&self) -> usize {
        starving_from(self.rays.iter().map(|ray| ray.hunger))
    }

    /// It starts buried, somewhere far enough that nobody saw it get there.
    fn add(&mut self, place: &Place, focus: Vec3, young: bool) -> bool {
        let Some(spot) = self.spot(place) else { return false };
        if spot.distance(focus) < UNSEEN {
            return false;
        }
        let mut ray = new_ray(&self.species, &mut self.rng, spot);
        if young {
            ray.size *= self.species.cycle.newborn;
        }
        ray.position.y = place.floor(spot.x, spot.z) + BURIED_AT * ray.size;
        ray.rotation = flat_look(ray.velocity.normalize_or(Vec3::Z));
        self.rays.push(ray);
        true
    }

    /// One too far to be seen: the hungriest of them.
    fn take_away(&mut self, focus: Vec3) -> bool {
        let unseen = self
            .rays
            .iter()
            .enumerate()
            .filter(|(_, ray)| ray.position.distance(focus) > if ray.buried() { UNSEEN } else { DRAW_DISTANCE })
            .max_by(|a, b| a.1.hunger.total_cmp(&b.1.hunger));
        let Some((index, _)) = unseen else { return false };
        self.rays.swap_remove(index);
        true
    }

    fn grow(&mut self, share: f32) {
        for ray in &mut self.rays {
            ray.size = (ray.size + ray.full_size * share).min(ray.full_size);
        }
    }

    fn truncate(&mut self, number: usize) {
        self.rays.truncate(number);
    }

    /// The sand keeps a buried one from any blow.
    fn strike(&mut self, hits: &mut dyn FnMut(Vec3, f32) -> bool, dead: &mut Vec<Carcass>) {
        let poses = self.poses;
        self.rays.retain(|ray| {
            let struck = !ray.buried() && hits(ray.position, RADIUS * ray.size);
            if struck {
                let pose = BodyPose::Table { poses, pose: pose_index(ray.phase, 0, 0.0) };
                dead.push(Carcass {
                    lift: 0.03,
                    ..Carcass::new(ray.position, laid_flat(ray.rotation), ray.size, ray.shade, pose)
                });
            }
            !struck
        });
    }
}

/// A grown ray lying buried, or about to be.
fn new_ray(species: &SpeciesDesc, rng: &mut Rng, position: Vec3) -> Ray {
    let angle = rng.range(0.0, std::f32::consts::TAU);
    let size = rng.range(species.size.0, species.size.1);
    Ray {
        position,
        velocity: Vec3::new(angle.cos(), 0.0, angle.sin()) * species.swim.cruise,
        doing: Doing::Buried { left: rng.range(1.0, 20.0) },
        rotation: Quat::IDENTITY,
        phase: rng.f32(),
        bend: 0.0,
        size,
        full_size: size,
        shade: rng.range(0.92, 1.08),
        hunger: rng.range(0.0, 0.5),
        think_in: rng.range(0.0, THINK_SECONDS),
        height: rng.range(species.glide.over_floor.0, species.glide.over_floor.1),
        meals: 0,
        drawn: true,
    }
}

impl Rays {
    pub(super) fn new(group: &SceneFauna, poses: usize) -> Self {
        let species = group.species.clone();
        let mut rng = Rng::new(group.seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xA77A);
        let rays = (0..group.count).map(|_| new_ray(&species, &mut rng, group.position)).collect();
        Self { species, poses, rays, home: group.position, rng, puff: group.puff.clone(), ledger: Ledger::of(group) }
    }

    pub(super) fn doings(&self) -> impl Iterator<Item = RayDoing> + '_ {
        self.rays.iter().map(|ray| match ray.doing {
            Doing::Buried { .. } => RayDoing::Buried,
            Doing::Cruising { .. } | Doing::Settling { .. } => RayDoing::Cruising,
            Doing::Hunting { .. } => RayDoing::Hunting,
            Doing::Bolting { .. } => RayDoing::Bolting,
        })
    }

    /// What the crabs have to get away from: whoever is after one, not one going by.
    pub(super) fn hunters(&self) -> impl Iterator<Item = Threat> + '_ {
        self.rays
            .iter()
            .filter(|ray| matches!(ray.doing, Doing::Hunting { .. }))
            .map(|ray| Threat { position: ray.position, radius: HUNTER_RADIUS })
    }

    pub(super) fn meals(&self) -> u32 {
        self.rays.iter().map(|ray| ray.meals).sum()
    }

    /// Each lies buried somewhere deep enough, near its home.
    pub(super) fn place(&mut self, place: &Place) {
        for i in 0..self.rays.len() {
            let spot = self.spot(place).unwrap_or(self.home);
            let ray = &mut self.rays[i];
            let buried = place.floor(spot.x, spot.z) + BURIED_AT * ray.size;
            ray.position = Vec3::new(spot.x, buried, spot.z);
            ray.rotation = flat_look(ray.velocity.normalize_or(Vec3::Z));
        }
    }

    /// Somewhere with water enough for it, in its range.
    fn spot(&mut self, place: &Place) -> Option<Vec3> {
        let glide = &self.species.glide;
        for _ in 0..16 {
            let angle = self.rng.range(0.0, std::f32::consts::TAU);
            let far = glide.roam * self.rng.f32().sqrt();
            let point = self.home + Vec3::new(angle.cos(), 0.0, angle.sin()) * far;
            if place.depth(point.x, point.z).is_some_and(|depth| depth >= glide.shallowest) {
                return Some(point);
            }
        }
        None
    }

    pub(super) fn update(
        &mut self,
        place: &Place,
        colonies: &mut [Colony],
        threats: &[Threat],
        focus: Vec3,
        dt: f32,
        wake: &mut Wake,
    ) {
        let species = self.species.clone();
        let SpeciesDesc { glide, swim, needs, senses, diet, .. } = &*species;
        for i in 0..self.rays.len() {
            let spot = self.spot(place);
            let ray = &mut self.rays[i];
            ray.hunger = (ray.hunger + dt / needs.hungry_after.max(1.0)).min(1.0);
            ray.think_in -= dt;
            let thinks = ray.think_in <= 0.0;
            if thinks {
                ray.think_in += THINK_SECONDS;
            }

            // Anything close is a reason to go, and a buried one bursts out of the sand.
            let buried = ray.buried();
            let scare = threats
                .iter()
                .find(|threat| {
                    if buried {
                        flat(threat.position - ray.position).length() < BURIED_NOTICE
                            && (threat.position.y - ray.position.y).abs() < BURIED_NOTICE_UP
                    } else {
                        threat.position.distance(ray.position) < threat.radius * senses.notice
                    }
                })
                .copied();
            // Stepped on, it stings first and goes after.
            let stepped_on = wake.player.filter(|player| {
                buried && species.harm.as_ref().is_some_and(|harm| player.touches(ray.position, harm.reach))
            });
            if stepped_on.is_some() {
                wake.harm(&species, ray.position);
            }
            let scare = stepped_on.map(|player| Threat { position: player.feet, radius: 0.0 }).or(scare);
            if let Some(threat) = scare
                && !matches!(ray.doing, Doing::Bolting { .. })
            {
                if buried {
                    wake.dig(&species, self.puff.as_ref(), ray.position);
                }
                let away = flat(ray.position - threat.position).normalize_or(Vec3::X);
                ray.velocity = away * swim.sprint.min(ray.velocity.length() + swim.sprint);
                ray.doing = Doing::Bolting { left: BOLT_SECONDS };
            }

            let dark = place.daylight < COMES_OUT;
            let light = place.daylight > GOES_IN;
            let starving = ray.hunger > STARVING;
            match ray.doing {
                Doing::Buried { left } => {
                    ray.doing = Doing::Buried { left: left - dt };
                    ray.velocity = Vec3::ZERO;
                    if thinks && left <= 0.0 && (dark || starving) {
                        wake.dig(&species, self.puff.as_ref(), ray.position);
                        let to = spot.unwrap_or(self.home);
                        ray.velocity = (to - ray.position).normalize_or(Vec3::Z) * swim.cruise * 0.5;
                        ray.doing = Doing::Cruising { to };
                    }
                }
                Doing::Cruising { to } => {
                    if thinks && light && !starving {
                        ray.doing = Doing::Settling { rest: self.rng.range(glide.rest.0, glide.rest.1) };
                    } else if thinks
                        && ray.hunger > SATED
                        && let Some(crab) = smell_crab(place, colonies, diet, ray.position, glide.smell)
                    {
                        ray.doing = Doing::Hunting { at: crab, left: CHASE_SECONDS };
                    } else {
                        let to = if flat(to - ray.position).length() < 2.0 { spot.unwrap_or(to) } else { to };
                        ray.doing = Doing::Cruising { to };
                        ray.glide(place, to, swim.cruise, swim.turn, dt);
                    }
                }
                Doing::Settling { rest } => {
                    let floor = place.floor(ray.position.x, ray.position.z);
                    let lying = floor + BURIED_AT * ray.size;
                    let next = ray.position + flat(ray.velocity) * dt;
                    if place.depth(next.x, next.z).is_some_and(|depth| depth > SHALLOWEST) {
                        ray.position = next;
                    }
                    ray.position.y = (ray.position.y - SETTLING * dt).max(lying);
                    ray.velocity *= 1.0 - (dt * 1.5).min(1.0);
                    ray.bend *= 1.0 - (dt * 3.0).min(1.0);
                    let heading = flat(ray.rotation * Vec3::Z);
                    ray.rotation = ray.rotation.slerp(flat_look(heading), (dt * 4.0).min(1.0));
                    if ray.position.y <= lying + 0.001 {
                        wake.dig(&species, self.puff.as_ref(), Vec3::new(ray.position.x, floor, ray.position.z));
                        ray.velocity = Vec3::ZERO;
                        ray.doing = Doing::Buried { left: rest };
                    }
                }
                Doing::Hunting { at, left } => {
                    let at = smell_crab(place, colonies, diet, at, glide.smell * 0.5).unwrap_or(at);
                    ray.doing = Doing::Hunting { at, left: left - dt };
                    ray.glide(place, at, swim.cruise * 1.9, swim.turn * 1.8, dt);
                    let reach = glide.catch_radius * ray.size;
                    if colonies.iter_mut().any(|colony| colony.take_near(ray.position, reach)) {
                        ray.hunger = (ray.hunger - glide.meal).max(0.0);
                        ray.meals += 1;
                        ray.doing = Doing::Settling { rest: self.rng.range(glide.rest.0, glide.rest.1) };
                    } else if thinks && (left <= 0.0 || flat(at - ray.position).length() < 0.4) {
                        // It's not there any more, or it can't be reached.
                        ray.doing = Doing::Cruising { to: spot.unwrap_or(self.home) };
                    }
                }
                Doing::Bolting { left } => {
                    let way = ray.position + ray.velocity.normalize_or(Vec3::Z) * 5.0;
                    ray.glide(place, way, swim.sprint, swim.turn * 2.0, dt);
                    ray.doing = Doing::Bolting { left: left - dt };
                    if left <= 0.0 {
                        ray.doing = Doing::Cruising { to: spot.unwrap_or(self.home) };
                    }
                }
            }
            if !ray.buried() {
                let speed = ray.velocity.length();
                ray.phase = (ray.phase + (swim.idle_beat + swim.beats_per_meter * speed) * dt).fract();
            }
            ray.drawn = ray.position.distance(focus) < DRAW_DISTANCE;
        }
    }
}

impl Ray {
    /// Swims towards a point, at its height over the bottom.
    fn glide(&mut self, place: &Place, to: Vec3, speed: f32, turn: f32, dt: f32) {
        let old = self.velocity.normalize_or(Vec3::Z);
        let floor = place.floor(self.position.x, self.position.z);
        let (level, _) = place.waters.calm_at(self.position.x, self.position.z).unwrap_or((floor + 2.0, Vec3::ZERO));
        // Close to something on the bottom it comes right down on it.
        let close = flat(to - self.position).length() < 2.5;
        let height = if close { 0.12 } else { self.height };
        let wanted_y = (floor + height).min(level - 0.25);
        let mut wanted = flat(to - self.position).normalize_or(old);
        // Not up the beach: it turns back to deeper water.
        let ahead = self.position + old * 1.5;
        let shallow = place.depth(ahead.x, ahead.z).is_none_or(|depth| depth < self.species_shallowest());
        if shallow {
            let slope = Vec3::new(
                place.floor(self.position.x - 3.0, self.position.z)
                    - place.floor(self.position.x + 3.0, self.position.z),
                0.0,
                place.floor(self.position.x, self.position.z - 3.0)
                    - place.floor(self.position.x, self.position.z + 3.0),
            );
            wanted = (wanted + slope.normalize_or(-old) * 2.0).normalize_or(wanted);
        }
        wanted.y = ((wanted_y - self.position.y) * 0.6).clamp(-STEEPEST, STEEPEST);
        let wanted = wanted.normalize_or(old);
        let most = turn * dt;
        let angle = old.angle_between(wanted);
        let heading = if angle > most { old.lerp(wanted, most / angle).normalize_or(old) } else { wanted };
        let was = self.velocity.length();
        let speed_now = was + (speed - was).clamp(-speed * dt, speed * 2.0 * dt);
        self.velocity = heading * speed_now;
        // Whatever it was after, it doesn't go up on the sand.
        let next = self.position + self.velocity * dt;
        if place.depth(next.x, next.z).is_none_or(|depth| depth < SHALLOWEST) {
            self.velocity = downhill(place, self.position) * speed_now;
        }
        self.position += self.velocity * dt;
        let floor = place.floor(self.position.x, self.position.z);
        self.position.y = self.position.y.max(floor + 0.05).min(level - 0.15);

        let turning = f32::atan2(old.cross(heading).y, old.dot(heading)) / (turn * dt).max(1e-4);
        self.bend += (turning.clamp(-1.0, 1.0) - self.bend) * (dt * 5.0).min(1.0);
        let target = Quat::from_axis_angle(heading, -self.bend * TURN_ROLL) * look_along(heading);
        self.rotation = self.rotation.slerp(target, (dt * 6.0).min(1.0));
    }

    /// Water it won't go shallower than: a little less than where it lies.
    fn species_shallowest(&self) -> f32 {
        0.6
    }
}

/// The way to deeper water from a point.
fn downhill(place: &Place, at: Vec3) -> Vec3 {
    let (x, z) = (at.x, at.z);
    let slope = Vec3::new(
        place.floor(x - 3.0, z) - place.floor(x + 3.0, z),
        0.0,
        place.floor(x, z - 3.0) - place.floor(x, z + 3.0),
    );
    slope.normalize_or(Vec3::X)
}

fn smell_crab(place: &Place, colonies: &[Colony], diet: &[String], from: Vec3, smell: f32) -> Option<Vec3> {
    colonies
        .iter()
        .filter(|colony| diet.iter().any(|food| *food == colony.species.name))
        .filter_map(|colony| colony.exposed_in_water(place, from, smell))
        .min_by(|a, b| flat(*a - from).length().total_cmp(&flat(*b - from).length()))
}

/// A rotation that points the model's +Z along `heading`, level.
fn look_along(heading: Vec3) -> Quat {
    let forward = heading.normalize_or(Vec3::Z);
    let left = Vec3::Y.cross(forward).normalize_or(Vec3::X);
    let up = forward.cross(left);
    Quat::from_mat3(&Mat3::from_cols(left, up, forward))
}

fn flat_look(heading: Vec3) -> Quat {
    look_along(flat(heading).normalize_or(Vec3::Z))
}
