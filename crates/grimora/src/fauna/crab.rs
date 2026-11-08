//! Crabs: each has a hole near the waterline, and each foot is planted on the ground as it
//! walks, with the legs in two groups taking turns. The arms come from clips.
//!
//! From far away nobody can tell where a foot is, so past `FEET_DISTANCE` the legs stop being
//! worked out, and past `DRAW_DISTANCE` so does the rest of its pose.

use std::sync::Arc;

use glam::{Mat3, Mat4, Quat, Vec3};
use grimora_audio::Emitter;
use grimora_core::animation::JointTransform;
use grimora_core::{Animator, MeshData, ParticleEffect, Rng, Skeleton, Transform};
use grimora_physics::Physics;
use grimora_render::{DrawItem, MeshHandle};
use grimora_scene::{SceneFauna, SpeciesDesc};

use super::ecology::{Ledger, Population, fed_from};
use super::encounter::{BodyPose, Carcass, Wake};
use super::{Place, Threat, flat};
use crate::world::MeshId;

/// Past this it isn't drawn; past the second its feet aren't worked out.
const DRAW_DISTANCE: f32 = 70.0;
const FEET_DISTANCE: f32 = 28.0;
/// Seconds between decisions, and between looks for something in the way.
const THINK_SECONDS: f32 = 0.45;
const LOOK_SECONDS: f32 = 0.35;
/// How far under the sand its body goes, in its hole, and how fast, in meters per second.
const HOLE_DEPTH: f32 = 0.16;
const DIG_SPEED: f32 = 0.3;
/// Under this much daylight they come out, all of them.
const DUSK: f32 = 0.4;
const SATED: f32 = 0.2;
/// Seconds it stays in its hole after a fright, at least and at most.
const HIDES_FOR: (f32, f32) = (8.0, 25.0);
/// Seconds it holds up its claws, and how much hunger a second of eating takes away.
const FACES_FOR: f32 = 1.8;
const EATS: f32 = 0.05;
/// A foot in the air takes this long, at most, and less the faster it's going.
const SWING_SECONDS: f32 = 0.16;
const SWING_LEAST: f32 = 0.05;
/// A ray can reach a crab under this much water, at least.
const WADING: f32 = 0.5;
/// Seconds before it will square up to another again.
const SPARES_FOR: f32 = 20.0;
/// How much of the way ahead of its middle a foot lands, of the distance it may be left behind.
const STEP_AHEAD: f32 = 0.7;
/// As the model rests its legs are nearly straight, so it stands with its feet drawn in.
const STANCE: f32 = 0.75;
/// How far from its middle a blow gets it, and how high a dead one lies on its back, times its size.
const RADIUS: f32 = 0.2;
const BODY_LIFT: f32 = 0.11;
/// Seconds from when it turns to face something until it can pinch: time to step back.
const PINCH_WINDUP: f32 = 0.5;
/// How scared it must be to pinch: two of them squaring up only threaten each other.
const PINCHES_FROM: f32 = 0.3;

/// What a crab is doing, for whoever asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrabDoing {
    InItsHole,
    Out,
    Eating,
    Fleeing,
    Facing,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Doing {
    Hidden { left: f32 },
    Rising,
    Walking { to: Vec3, then_eat: bool },
    Eating { left: f32 },
    Fleeing,
    Sinking,
    Facing { from: Vec3, left: f32 },
}

/// How one leg of the model is, as it rests.
#[derive(Debug, Clone)]
struct LegRig {
    joints: [usize; 3],
    hip: Vec3,
    /// The two bones that bend.
    upper: f32,
    lower: f32,
    /// From the third joint to the foot: it stays as it is.
    tip: Vec3,
    /// Where the foot rests.
    rest_foot: Vec3,
    rests: [Quat; 3],
    bones: [Vec3; 3],
    /// Which of the two groups that take turns it's in.
    group: usize,
}

#[derive(Debug, Clone)]
struct Foot {
    /// Where it is, in the world.
    at: Vec3,
    from: Vec3,
    to: Vec3,
    /// `0..1` of its way through the air; `None` on the ground.
    swing: Option<f32>,
}

#[derive(Debug)]
struct Crab {
    position: Vec3,
    /// The way its face looks: it walks across this.
    facing: Vec3,
    velocity: Vec3,
    hole: Vec3,
    doing: Doing,
    animator: Animator,
    pose: Vec<JointTransform>,
    matrices: Vec<Mat4>,
    pinch_in: f32,
    feet: Vec<Foot>,
    feet_placed: bool,
    rotation: Quat,
    /// Up, as the ground says here.
    up: Vec3,
    hunger: f32,
    fear: f32,
    think_in: f32,
    look_in: f32,
    spared: f32,
    bold: bool,
    size: f32,
    full_size: f32,
    shade: f32,
    drawn: bool,
    /// How many times the crab ate, and faced another one down.
    meals: u32,
    faced: u32,
    /// Which of the two groups has its feet in the air.
    stepping: usize,
    stride_clock: u32,
}

/// Crabs of one species around a place.
#[derive(Debug)]
pub(super) struct Colony {
    pub species: Arc<SpeciesDesc>,
    mesh: MeshId,
    skeleton: Arc<Skeleton>,
    legs: Vec<LegRig>,
    crabs: Vec<Crab>,
    home: Vec3,
    radius: f32,
    rng: Rng,
    puff: Option<ParticleEffect>,
    globals: Vec<Mat4>,
    ledger: Ledger,
}

impl Population for Colony {
    fn shift(&mut self, offset: Vec3) {
        self.home += offset;
        for crab in &mut self.crabs {
            crab.position += offset;
            crab.hole += offset;
            match &mut crab.doing {
                Doing::Walking { to, .. } => *to += offset,
                Doing::Facing { from, .. } => *from += offset,
                _ => {}
            }
            for foot in &mut crab.feet {
                foot.at += offset;
                foot.from += offset;
                foot.to += offset;
            }
        }
    }

    fn near(&self, point: Vec3, radius: f32) -> bool {
        self.crabs.iter().any(|crab| crab.position.distance(point) < radius)
    }

    fn species(&self) -> &SpeciesDesc {
        &self.species
    }

    fn ledger(&mut self) -> &mut Ledger {
        &mut self.ledger
    }

    fn number(&self) -> usize {
        self.crabs.len()
    }

    fn fed(&self) -> f32 {
        fed_from(self.crabs.iter().map(|crab| crab.hunger))
    }

    /// It starts down a new hole, so nobody sees it arrive, wherever they're standing.
    fn add(&mut self, place: &Place, _focus: Vec3, young: bool) -> bool {
        let hole = self.find_hole(place);
        let mut crab = new_crab(&self.species, &self.skeleton, &mut self.rng, hole);
        crab.hole = hole;
        crab.position = hole - Vec3::Y * HOLE_DEPTH;
        if young {
            crab.size *= self.species.cycle.newborn;
        }
        self.crabs.push(crab);
        true
    }

    /// One that's in its hole, or too far to be seen: the hungriest of them.
    fn take_away(&mut self, focus: Vec3) -> bool {
        let unseen = self
            .crabs
            .iter()
            .enumerate()
            .filter(|(_, crab)| {
                matches!(crab.doing, Doing::Hidden { .. }) || crab.position.distance(focus) > DRAW_DISTANCE
            })
            .max_by(|a, b| a.1.hunger.total_cmp(&b.1.hunger));
        let Some((index, _)) = unseen else { return false };
        self.crabs.swap_remove(index);
        true
    }

    fn grow(&mut self, share: f32) {
        for crab in &mut self.crabs {
            crab.size = (crab.size + crab.full_size * share).min(crab.full_size);
        }
    }

    fn truncate(&mut self, number: usize) {
        self.crabs.truncate(number);
    }

    /// Down its hole, nothing can get at it. A dead one ends on its back, legs as they were.
    fn strike(&mut self, hits: &mut dyn FnMut(Vec3, f32) -> bool, dead: &mut Vec<Carcass>) {
        let mesh = self.mesh;
        self.crabs.retain(|crab| {
            let out = !matches!(crab.doing, Doing::Hidden { .. } | Doing::Sinking);
            let struck = out && hits(crab.position, RADIUS * crab.size);
            if struck {
                let pose = BodyPose::Own { mesh, matrices: crab.matrices.clone() };
                let on_its_back = crab.rotation * Quat::from_rotation_z(std::f32::consts::PI);
                dead.push(Carcass {
                    lift: BODY_LIFT * crab.size,
                    ..Carcass::new(crab.position, on_its_back, crab.size, crab.shade, pose)
                });
            }
            !struck
        });
    }
}

/// A grown crab down its hole, out here at `position` until it's given one.
fn new_crab(species: &SpeciesDesc, skeleton: &Arc<Skeleton>, rng: &mut Rng, position: Vec3) -> Crab {
    let angle = rng.range(0.0, std::f32::consts::TAU);
    let mut animator = Animator::new(skeleton.clone());
    animator.play("idle", 0.0);
    let size = rng.range(species.size.0, species.size.1);
    Crab {
        position,
        facing: Vec3::new(angle.cos(), 0.0, angle.sin()),
        velocity: Vec3::ZERO,
        hole: position,
        doing: Doing::Hidden { left: rng.range(0.5, 20.0) },
        animator,
        pose: skeleton.rest_pose(),
        matrices: Vec::new(),
        pinch_in: 0.0,
        feet: Vec::new(),
        feet_placed: false,
        rotation: Quat::IDENTITY,
        up: Vec3::Y,
        hunger: rng.range(0.1, 0.7),
        fear: 0.0,
        think_in: rng.range(0.0, THINK_SECONDS),
        look_in: rng.range(0.0, LOOK_SECONDS),
        spared: 0.0,
        bold: rng.f32() < species.crawl.bold,
        size,
        full_size: size,
        shade: rng.range(0.9, 1.1),
        drawn: false,
        meals: 0,
        faced: 0,
        stepping: 0,
        stride_clock: rng.next_u64() as u32,
    }
}

impl Colony {
    pub(super) fn new(group: &SceneFauna, meshes: &[MeshData]) -> Option<Self> {
        let skeleton = meshes.get(group.mesh)?.skeleton.clone()?;
        let species = group.species.clone();
        let legs = leg_rigs(&skeleton);
        if legs.is_empty() {
            log::error!("{}: its model has no legs the way a crab has them", species.name);
        }
        let mut rng = Rng::new(group.seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xC4AB);
        let crabs = (0..group.count).map(|_| new_crab(&species, &skeleton, &mut rng, group.position)).collect();
        Some(Self {
            species,
            mesh: MeshId(group.mesh),
            skeleton,
            legs,
            crabs,
            home: group.position,
            radius: group.radius,
            rng,
            puff: group.puff.clone(),
            globals: Vec::new(),
            ledger: Ledger::of(group),
        })
    }

    pub(super) fn population(&self) -> usize {
        self.crabs.len()
    }

    pub(super) fn positions(&self) -> impl Iterator<Item = Vec3> + '_ {
        self.crabs.iter().map(|crab| crab.position)
    }

    pub(super) fn doings(&self) -> impl Iterator<Item = CrabDoing> + '_ {
        self.crabs.iter().map(|crab| match crab.doing {
            Doing::Hidden { .. } | Doing::Sinking | Doing::Rising => CrabDoing::InItsHole,
            Doing::Walking { .. } => CrabDoing::Out,
            Doing::Eating { .. } => CrabDoing::Eating,
            Doing::Fleeing => CrabDoing::Fleeing,
            Doing::Facing { .. } => CrabDoing::Facing,
        })
    }

    pub(super) fn meals(&self) -> u32 {
        self.crabs.iter().map(|crab| crab.meals).sum()
    }

    pub(super) fn standoffs(&self) -> u32 {
        self.crabs.iter().map(|crab| crab.faced).sum()
    }

    /// How much of the length of a leg the worst foot asks for: over 1, it doesn't reach.
    pub(super) fn longest_stretch(&self) -> f32 {
        self.crabs
            .iter()
            .filter(|crab| crab.feet_placed && crab.feet.len() == self.legs.len())
            .flat_map(|crab| {
                self.legs.iter().zip(&crab.feet).map(|(leg, foot)| {
                    let here = crab.rotation.inverse() * (foot.at - crab.position) / crab.size;
                    (here - leg.tip - leg.hip).length() / (leg.upper + leg.lower)
                })
            })
            .fold(0.0, f32::max)
    }

    /// How far the worst planted foot is from the ground under it.
    pub(super) fn feet_off_ground(&self, place: &Place) -> f32 {
        self.crabs
            .iter()
            .filter(|crab| crab.feet_placed)
            .flat_map(|crab| crab.feet.iter())
            .filter(|foot| foot.swing.is_none())
            .map(|foot| (foot.at.y - place.floor(foot.at.x, foot.at.z)).abs())
            .fold(0.0, f32::max)
    }

    /// Gives each its hole.
    pub(super) fn place(&mut self, place: &Place) {
        for i in 0..self.crabs.len() {
            let hole = self.find_hole(place);
            let crab = &mut self.crabs[i];
            crab.hole = hole;
            crab.position = hole - Vec3::Y * HOLE_DEPTH;
        }
    }

    /// Somewhere between the water and the dry sand.
    fn find_hole(&mut self, place: &Place) -> Vec3 {
        let shore = self.species.crawl.shore;
        for _ in 0..40 {
            let angle = self.rng.range(0.0, std::f32::consts::TAU);
            let far = self.radius * self.rng.f32().sqrt();
            let point = self.home + Vec3::new(angle.cos(), 0.0, angle.sin()) * far;
            let floor = place.floor(point.x, point.z);
            let level = place.waters.calm_at(point.x, point.z).map_or(floor, |(level, _)| level);
            if (shore.0..=shore.1).contains(&(floor - level)) {
                return Vec3::new(point.x, floor, point.z);
            }
        }
        Vec3::new(self.home.x, place.floor(self.home.x, self.home.z), self.home.z)
    }

    /// A crab out in the water near a point, if any: what a ray can get at.
    pub(super) fn exposed_in_water(&self, place: &Place, from: Vec3, within: f32) -> Option<Vec3> {
        self.crabs
            .iter()
            .filter(|crab| !matches!(crab.doing, Doing::Hidden { .. } | Doing::Sinking))
            .filter(|crab| flat(crab.position - from).length() < within)
            .filter(|crab| place.depth(crab.position.x, crab.position.z).is_some_and(|depth| depth > WADING))
            .map(|crab| crab.position)
            .min_by(|a, b| flat(*a - from).length().total_cmp(&flat(*b - from).length()))
    }

    /// Something took the crab closest to a point, if one is out and within reach.
    pub(super) fn take_near(&mut self, at: Vec3, reach: f32) -> bool {
        let caught = self
            .crabs
            .iter()
            .enumerate()
            .filter(|(_, crab)| !matches!(crab.doing, Doing::Hidden { .. } | Doing::Sinking))
            .map(|(index, crab)| (index, flat(crab.position - at).length()))
            .filter(|(_, far)| *far < reach)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let Some((index, _)) = caught else { return false };
        self.crabs.swap_remove(index);
        true
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn update(
        &mut self,
        place: &Place,
        physics: &Physics,
        carcasses: &mut [Carcass],
        threats: &[Threat],
        focus: Vec3,
        dt: f32,
        wake: &mut Wake,
    ) {
        let species = self.species.clone();
        for i in 0..self.crabs.len() {
            let crab = &mut self.crabs[i];
            // Down its hole it isn't doing anything to get hungry.
            if !matches!(crab.doing, Doing::Hidden { .. }) {
                crab.hunger = (crab.hunger + dt / species.needs.hungry_after.max(1.0)).min(1.0);
            }
            crab.fear = (crab.fear - dt / species.senses.calm_seconds.max(0.1)).max(0.0);
            crab.spared = (crab.spared - dt).max(0.0);
            crab.think_in -= dt;
            // Something near doesn't wait for it to look up.
            let position = crab.position;
            let startled =
                threats.iter().any(|threat| threat.position.distance(position) < threat.radius * species.senses.notice);
            if crab.think_in <= 0.0 || startled {
                crab.think_in = THINK_SECONDS;
                let facing = matches!(crab.doing, Doing::Facing { .. });
                self.think(i, place, carcasses, threats);
                if !facing && matches!(self.crabs[i].doing, Doing::Facing { .. }) {
                    self.crabs[i].pinch_in = PINCH_WINDUP;
                    wake.play(&species, species.sounds.facing.as_deref(), Emitter::quiet(position));
                }
            }
            self.act(i, place, physics, carcasses, dt, wake);
            self.pinch(i, dt, wake);
            self.show(i, place, focus, dt);
        }
    }

    /// Cornered, it gets whatever comes within reach of its claws.
    fn pinch(&mut self, i: usize, dt: f32, wake: &mut Wake) {
        let crab = &mut self.crabs[i];
        crab.pinch_in -= dt;
        if crab.pinch_in <= 0.0
            && crab.fear > PINCHES_FROM
            && matches!(crab.doing, Doing::Facing { .. })
            && let (Some(player), Some(harm)) = (wake.player, &self.species.harm)
            && player.touches(crab.position, harm.reach)
        {
            crab.pinch_in = harm.every;
            wake.harm(&self.species, crab.position);
        }
    }

    fn think(&mut self, i: usize, place: &Place, carcasses: &[Carcass], threats: &[Threat]) {
        let species = self.species.clone();
        let crawl = &species.crawl;
        let position = self.crabs[i].position;

        // Danger first.
        let danger = threats
            .iter()
            .filter(|threat| threat.position.distance(position) < threat.radius * species.senses.notice)
            .min_by(|a, b| a.position.distance(position).total_cmp(&b.position.distance(position)))
            .copied();
        let out = !matches!(self.crabs[i].doing, Doing::Hidden { .. } | Doing::Sinking);
        if let Some(danger) = danger {
            let crab = &mut self.crabs[i];
            crab.fear = 1.0;
            if out {
                let to_hole = flat(crab.hole - position).length();
                let to_it = flat(danger.position - position).length();
                if to_it < crawl.cornered && to_hole > to_it * 2.0 {
                    crab.doing = Doing::Facing { from: danger.position, left: FACES_FOR };
                } else if !matches!(crab.doing, Doing::Facing { .. }) {
                    crab.doing = Doing::Fleeing;
                }
            } else if let Doing::Hidden { left } = crab.doing {
                // It knows, down there, and stays in a while longer.
                crab.doing = Doing::Hidden { left: left.max(HIDES_FOR.0) };
            }
            return;
        }

        let dark = place.daylight < DUSK;
        let bold = self.crabs[i].bold;
        let wants_out = !place.storm && (dark || bold) && self.crabs[i].fear <= 0.0;
        match self.crabs[i].doing {
            Doing::Hidden { left } if left <= 0.0 && wants_out => {
                self.crabs[i].doing = Doing::Rising;
            }
            Doing::Walking { .. } | Doing::Eating { .. } if !wants_out => {
                self.crabs[i].doing = Doing::Fleeing;
            }
            Doing::Walking { then_eat, .. } => {
                // Another too close to its patch of sand: they square up.
                let spared = self.crabs[i].spared > 0.0;
                let crowded = self.crabs.iter().enumerate().find(|(other, crab)| {
                    !spared
                        && *other != i
                        && !matches!(crab.doing, Doing::Hidden { .. } | Doing::Sinking | Doing::Rising)
                        && flat(crab.position - position).length() < crawl.territory
                });
                if let Some((_, other)) = crowded {
                    let from = other.position;
                    self.crabs[i].doing = Doing::Facing { from, left: FACES_FOR };
                    self.crabs[i].faced += 1;
                    self.crabs[i].spared = SPARES_FOR;
                    return;
                }
                // Something dead, within smell.
                let hungry = self.crabs[i].hunger > SATED;
                let food = carcasses
                    .iter()
                    .filter(|carcass| carcass.eatable())
                    .map(|carcass| carcass.position)
                    .filter(|at| flat(*at - position).length() < crawl.smell)
                    .min_by(|a, b| flat(*a - position).length().total_cmp(&flat(*b - position).length()));
                if let (true, Some(food), false) = (hungry, food, then_eat) {
                    self.crabs[i].doing = Doing::Walking { to: food, then_eat: true };
                }
            }
            _ => {}
        }
    }

    fn wander_point(&mut self, i: usize, place: &Place) -> Vec3 {
        let crawl = &self.species.crawl;
        let hole = self.crabs[i].hole;
        for _ in 0..8 {
            let angle = self.rng.range(0.0, std::f32::consts::TAU);
            let far = crawl.roam * self.rng.f32().sqrt();
            let point = hole + Vec3::new(angle.cos(), 0.0, angle.sin()) * far;
            let floor = place.floor(point.x, point.z);
            let level = place.waters.calm_at(point.x, point.z).map_or(floor, |(level, _)| level);
            if (crawl.shore.0 - 0.5..=crawl.shore.1 + 0.5).contains(&(floor - level)) {
                return Vec3::new(point.x, floor, point.z);
            }
        }
        hole
    }

    fn act(&mut self, i: usize, place: &Place, physics: &Physics, carcasses: &mut [Carcass], dt: f32, wake: &mut Wake) {
        let species = self.species.clone();
        let crawl = &species.crawl;
        match self.crabs[i].doing {
            Doing::Hidden { left } => {
                let crab = &mut self.crabs[i];
                crab.velocity = Vec3::ZERO;
                crab.doing = Doing::Hidden { left: left - dt };
            }
            Doing::Rising => {
                let crab = &mut self.crabs[i];
                crab.position.y += DIG_SPEED * dt;
                crab.velocity = Vec3::ZERO;
                if crab.position.y >= crab.hole.y {
                    crab.position.y = crab.hole.y;
                    crab.feet_placed = false;
                    wake.dig(&self.species, self.puff.as_ref(), crab.hole);
                    let to = self.wander_point(i, place);
                    self.crabs[i].doing = Doing::Walking { to, then_eat: false };
                }
            }
            Doing::Sinking => {
                let crab = &mut self.crabs[i];
                crab.velocity = Vec3::ZERO;
                crab.position.y -= DIG_SPEED * dt;
                if crab.position.y <= crab.hole.y - HOLE_DEPTH {
                    let left = self.rng.range(HIDES_FOR.0, HIDES_FOR.1);
                    self.crabs[i].doing = Doing::Hidden { left };
                }
            }
            Doing::Walking { to, then_eat } => {
                let arrived = self.walk(i, to, crawl.walk, place, physics, dt);
                if arrived {
                    let crab = &mut self.crabs[i];
                    let hungry = crab.hunger > SATED;
                    if then_eat || hungry {
                        let left = self.rng.range(4.0, 9.0);
                        self.crabs[i].doing = Doing::Eating { left };
                    } else {
                        let to = self.wander_point(i, place);
                        self.crabs[i].doing = Doing::Walking { to, then_eat: false };
                    }
                }
            }
            Doing::Eating { left } => {
                let crab = &mut self.crabs[i];
                crab.velocity = Vec3::ZERO;
                crab.hunger = (crab.hunger - EATS * dt).max(0.0);
                // What's dead is much better than what it picks out of the sand, and it runs out.
                let position = crab.position;
                let eaten = carcasses
                    .iter_mut()
                    .find(|carcass| carcass.eatable() && flat(carcass.position - position).length() < 0.7);
                if let Some(carcass) = eaten {
                    carcass.food -= dt;
                    crab.hunger = (crab.hunger - EATS * 3.0 * dt).max(0.0);
                }
                crab.doing = Doing::Eating { left: left - dt };
                if left <= 0.0 {
                    crab.meals += 1;
                    let to = self.wander_point(i, place);
                    self.crabs[i].doing = Doing::Walking { to, then_eat: false };
                }
            }
            Doing::Fleeing => {
                let hole = self.crabs[i].hole;
                if self.walk(i, hole, crawl.run, place, physics, dt) {
                    let crab = &mut self.crabs[i];
                    wake.dig(&self.species, self.puff.as_ref(), crab.hole);
                    crab.doing = Doing::Sinking;
                }
            }
            Doing::Facing { from, left } => {
                let crab = &mut self.crabs[i];
                crab.velocity = Vec3::ZERO;
                let toward = flat(from - crab.position).normalize_or(crab.facing);
                crab.facing = turned(crab.facing, toward, crawl.turn, dt);
                crab.doing = Doing::Facing { from, left: left - dt };
                if left <= 0.0 {
                    // It backs down if it's scared, and goes about its business if it isn't.
                    if crab.fear > 0.3 {
                        crab.doing = Doing::Fleeing;
                    } else {
                        let to = self.wander_point(i, place);
                        self.crabs[i].doing = Doing::Walking { to, then_eat: false };
                    }
                }
            }
        }
    }

    /// Walks towards a point, sideways. True when it gets there.
    fn walk(&mut self, i: usize, to: Vec3, speed: f32, place: &Place, physics: &Physics, dt: f32) -> bool {
        let crawl = &self.species.crawl;
        let crab = &mut self.crabs[i];
        let way = flat(to - crab.position);
        let far = way.length();
        if far < 0.12 {
            crab.velocity = Vec3::ZERO;
            return true;
        }
        let way = way / far;
        // Whichever of its sides is nearer: it never bothers to turn all the way around.
        let across = Vec3::new(crab.facing.z, 0.0, -crab.facing.x);
        let side = if across.dot(way) >= 0.0 { across } else { -across };
        let turn = f32::atan2(side.cross(way).y, side.dot(way));
        crab.facing = Quat::from_rotation_y((crawl.turn * dt).min(turn.abs()) * turn.signum()) * crab.facing;

        // Something in the way: it edges around it.
        crab.look_in -= dt;
        let mut walking = way;
        if crab.look_in <= 0.0 {
            crab.look_in = LOOK_SECONDS;
            let body = crab.position + crab.up * 0.08;
            if physics.raycast_scenery(body, way, 0.45).is_some_and(|hit| hit.normal.y < 0.6) {
                walking = Vec3::new(way.z, 0.0, -way.x);
            }
        }
        // Young ones have short legs.
        let speed = speed * crab.size / crab.full_size;
        crab.velocity = crab.velocity.lerp(walking * speed, (dt * 8.0).min(1.0));
        crab.position += crab.velocity * dt;
        let floor = place.floor(crab.position.x, crab.position.z);
        crab.position.y = floor;
        false
    }

    /// Its pose: the arms from a clip, and each foot where it is on the ground.
    fn show(&mut self, i: usize, place: &Place, focus: Vec3, dt: f32) {
        let species = self.species.clone();
        let crawl = &species.crawl;
        let crab = &mut self.crabs[i];
        let distance = crab.position.distance(focus);
        crab.drawn = distance < DRAW_DISTANCE && !matches!(crab.doing, Doing::Hidden { .. });
        if !crab.drawn {
            crab.feet_placed = false;
            return;
        }

        // Lying along the ground under it.
        let (x, z) = (crab.position.x, crab.position.z);
        let d = 0.18;
        let slope_x = place.floor(x + d, z) - place.floor(x - d, z);
        let slope_z = place.floor(x, z + d) - place.floor(x, z - d);
        let ground_up = Vec3::new(-slope_x, 2.0 * d, -slope_z).normalize_or(Vec3::Y);
        crab.up = crab.up.lerp(ground_up, (dt * 8.0).min(1.0)).normalize_or(Vec3::Y);
        let forward = (crab.facing - crab.up * crab.facing.dot(crab.up)).normalize_or(crab.facing);
        let left = crab.up.cross(forward).normalize_or(Vec3::X);
        crab.rotation = Quat::from_mat3(&Mat3::from_cols(left, crab.up, forward));

        let clip = match crab.doing {
            Doing::Facing { .. } => "threat",
            Doing::Eating { .. } => "feed",
            _ => "idle",
        };
        crab.animator.play(clip, 0.2);
        crab.animator.update(dt);
        crab.pose.clear();
        crab.pose.extend_from_slice(crab.animator.pose());

        if distance < FEET_DISTANCE
            && !self.legs.is_empty()
            && matches!(
                crab.doing,
                Doing::Walking { .. } | Doing::Fleeing | Doing::Eating { .. } | Doing::Facing { .. }
            )
        {
            step_feet(crab, &self.legs, crawl.step, crawl.lift, place, dt);
            pose_legs(crab, &self.legs, &self.skeleton);
        } else {
            crab.feet_placed = false;
        }

        self.skeleton.global_transforms(&crab.pose, &mut self.globals);
        crab.matrices.clear();
        crab.matrices.extend(self.globals.iter().zip(&self.skeleton.joints).map(|(g, j)| *g * j.inverse_bind));
        crab.stride_clock = crab.stride_clock.wrapping_add(1);
    }

    pub(super) fn push_draws(&self, handles: &[MeshHandle], draws: &mut Vec<DrawItem>, joints: &mut Vec<Mat4>) {
        let Some(&mesh) = handles.get(self.mesh.0) else { return };
        for crab in self.crabs.iter().filter(|crab| crab.drawn && !crab.matrices.is_empty()) {
            joints.extend_from_slice(&crab.matrices);
            draws.push(DrawItem {
                mesh,
                transform: Transform {
                    position: crab.position,
                    rotation: crab.rotation,
                    scale: Vec3::splat(crab.size),
                },
                color: Vec3::splat(crab.shade),
                emissive: 0.0,
                joints: Some((joints.len() - crab.matrices.len()) as u32),
            });
        }
    }
}

/// Turns `from` towards `to`, on the ground, no more than `turn` radians per second.
fn turned(from: Vec3, to: Vec3, turn: f32, dt: f32) -> Vec3 {
    let angle = f32::atan2(from.cross(to).y, from.dot(to));
    Quat::from_rotation_y((turn * dt).min(angle.abs()) * angle.signum()) * from
}

/// Works out the legs of a model whose joints go `Leg<n>.Hip.<side>`, `.Shin.`, `.Tip.`.
fn leg_rigs(skeleton: &Skeleton) -> Vec<LegRig> {
    let mut globals = Vec::new();
    skeleton.global_transforms(&skeleton.rest_pose(), &mut globals);
    let head = |joint: usize| globals[joint].w_axis.truncate();
    let rotation = |joint: usize| globals[joint].to_scale_rotation_translation().1;
    let mut legs = Vec::new();
    for (side_index, side) in ["L", "R"].iter().enumerate() {
        for leg in 1..=4 {
            let named = |part: &str| skeleton.joint(&format!("Leg{leg}.{part}.{side}"));
            let (Some(hip), Some(shin), Some(tip)) = (named("Hip"), named("Shin"), named("Tip")) else {
                continue;
            };
            let rests = [rotation(hip), rotation(shin), rotation(tip)];
            let bones = rests.map(|rest| rest * Vec3::Y);
            // The foot is where the last bone reaches the ground, the model's origin being there.
            let down = -bones[2].y.min(-0.01);
            let rest_foot = head(tip) + bones[2] * (head(tip).y / down);
            let stance = head(hip) + (rest_foot - head(hip)) * STANCE;
            legs.push(LegRig {
                joints: [hip, shin, tip],
                hip: head(hip),
                upper: head(hip).distance(head(shin)),
                lower: head(shin).distance(head(tip)),
                tip: rest_foot - head(tip),
                rest_foot: stance.with_y(0.0),
                rests,
                bones,
                group: (leg + side_index) % 2,
            });
        }
    }
    legs
}

/// Moves whichever feet it's their turn to step. A foot stays planted where it is until the
/// body has gone too far from it.
fn step_feet(crab: &mut Crab, legs: &[LegRig], step: f32, lift: f32, place: &Place, dt: f32) {
    let place_foot = |crab: &Crab, rest: Vec3| -> Vec3 {
        let at = crab.position + crab.rotation * (rest * crab.size);
        Vec3::new(at.x, place.floor(at.x, at.z), at.z)
    };
    if !crab.feet_placed || crab.feet.len() != legs.len() {
        crab.feet = legs
            .iter()
            .map(|leg| {
                let at = place_foot(crab, leg.rest_foot);
                Foot { at, from: at, to: at, swing: None }
            })
            .collect();
        crab.feet_placed = true;
        return;
    }
    let speed = crab.velocity.length();
    let reach = step * crab.size;
    // Quicker the further behind it's been left: it may be turning, and going nowhere.
    let lag = legs
        .iter()
        .zip(&crab.feet)
        .map(|(leg, foot)| foot.at.distance(place_foot(crab, leg.rest_foot)))
        .fold(0.0_f32, f32::max);
    let hurry = (reach / lag.max(0.001)).min(1.0);
    let swing_time = (SWING_SECONDS * hurry / (1.0 + speed * 3.0)).max(SWING_LEAST);

    // Only one group is ever in the air, so it's always standing on the other.
    let stepping = crab.stepping;
    let in_air = crab.feet.iter().any(|foot| foot.swing.is_some());
    if !in_air {
        let behind = |group: usize| {
            legs.iter()
                .zip(&crab.feet)
                .filter(|(leg, _)| leg.group == group)
                .map(|(leg, foot)| foot.at.distance(place_foot(crab, leg.rest_foot)))
                .fold(0.0_f32, f32::max)
        };
        let (mine, other) = (behind(stepping), behind(1 - stepping));
        let group = if mine >= other { stepping } else { 1 - stepping };
        if mine.max(other) > reach {
            // Where the body will be when it lands, and a bit further on.
            let way = crab.velocity.normalize_or(Vec3::ZERO);
            let ahead = (crab.velocity * swing_time + way * reach * STEP_AHEAD) / crab.size;
            let targets: Vec<_> = legs
                .iter()
                .map(|leg| {
                    let at = place_foot(crab, leg.rest_foot + crab.rotation.inverse() * ahead);
                    (leg.group, at)
                })
                .collect();
            for (foot, (leg_group, to)) in crab.feet.iter_mut().zip(targets) {
                if leg_group == group {
                    foot.from = foot.at;
                    foot.to = to;
                    foot.swing = Some(0.0);
                }
            }
            crab.stepping = 1 - group;
        }
    }
    for foot in &mut crab.feet {
        let Some(swing) = foot.swing else { continue };
        let swing = swing + dt / swing_time;
        if swing >= 1.0 {
            foot.at = foot.to;
            foot.swing = None;
        } else {
            let rise = (swing * std::f32::consts::PI).sin() * lift * crab.size;
            foot.at = foot.from.lerp(foot.to, swing) + Vec3::Y * rise;
            foot.swing = Some(swing);
        }
    }
}

/// Bends each leg so that its foot is where [`step_feet`] put it.
fn pose_legs(crab: &mut Crab, legs: &[LegRig], skeleton: &Skeleton) {
    let inverse = crab.rotation.inverse();
    for (leg, foot) in legs.iter().zip(&crab.feet) {
        // In the model's space.
        let foot_here = inverse * (foot.at - crab.position) / crab.size;
        let ankle = foot_here - leg.tip;
        let reach = ankle - leg.hip;
        let longest = (leg.upper + leg.lower) * 0.995;
        let distance = reach.length().clamp(0.001, longest);
        let along = reach.normalize_or(Vec3::NEG_Y);
        let ankle = leg.hip + along * distance;
        // The knee, up and out: the way a crab's legs arch.
        let out = (leg.rest_foot - leg.hip).with_y(0.0).normalize_or(Vec3::X);
        let pole = (Vec3::Y + out * 0.3) - along * (Vec3::Y + out * 0.3).dot(along);
        let pole = pole.normalize_or(Vec3::Y);
        let a = (leg.upper * leg.upper - leg.lower * leg.lower + distance * distance) / (2.0 * distance);
        let h = (leg.upper * leg.upper - a * a).max(0.0).sqrt();
        let knee = leg.hip + along * a + pole * h;

        let upper = Quat::from_rotation_arc(leg.bones[0], (knee - leg.hip).normalize_or(leg.bones[0])) * leg.rests[0];
        let lower = Quat::from_rotation_arc(leg.bones[1], (ankle - knee).normalize_or(leg.bones[1])) * leg.rests[1];
        let last = leg.rests[2];
        let parent = skeleton.joints[leg.joints[0]].parent;
        let above = parent.map_or(Quat::IDENTITY, |parent| {
            let mut rotation = Quat::IDENTITY;
            let mut at = Some(parent);
            while let Some(joint) = at {
                rotation = crab.pose[joint].rotation * rotation;
                at = skeleton.joints[joint].parent;
            }
            rotation
        });
        crab.pose[leg.joints[0]].rotation = above.inverse() * upper;
        crab.pose[leg.joints[1]].rotation = upper.inverse() * lower;
        crab.pose[leg.joints[2]].rotation = lower.inverse() * last;
    }
}
