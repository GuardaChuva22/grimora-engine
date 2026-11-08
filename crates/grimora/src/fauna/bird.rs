//! Birds: few enough for each to have its own animator and its own mind.
//!
//! A bird has needs (`hunger`, `tiredness`) and senses (what it sees below it, the dive of
//! another one, whoever comes too close), and what it does comes out of those: it isn't told
//! to hunt. What it catches is really gone from the school.

use std::sync::Arc;

use glam::{Mat3, Mat4, Quat, Vec3};
use grimora_audio::Emitter;
use grimora_core::{Animator, MeshData, Rng, Skeleton, Transform};
use grimora_physics::Physics;
use grimora_render::{DrawItem, MeshHandle};
use grimora_scene::{SceneFauna, SpeciesDesc};

use super::ecology::{Ledger, Population, fed_from, starving_from};
use super::encounter::{BodyPose, Carcass, Wake, laid_flat};
use super::{Place, School, Threat, flat};
use crate::world::MeshId;

/// Beyond this a bird isn't drawn, and its wings stop being worked out.
const DRAW_DISTANCE: f32 = 280.0;
/// Beyond this its pose is worked out every `FAR_POSE_STRIDE` frames.
const CLOSE_DISTANCE: f32 = 50.0;
const FAR_POSE_STRIDE: u32 = 3;
/// Seconds between decisions.
const THINK_SECONDS: f32 = 0.4;
/// Seconds between looks ahead for something to fly into, and how far it looks.
const LOOK_SECONDS: f32 = 0.3;
const LOOK_SECONDS_AHEAD: f32 = 1.6;
/// Seconds a bird gives to dodging what it saw ahead.
const DODGE_SECONDS: f32 = 0.9;
/// How far above something it may start to come down, and how far a call is heard.
const LANDING_DISTANCE: f32 = 9.0;
const CALL_HEARD: f32 = 160.0;
const CALL_SECONDS: f32 = 6.0;
/// How many tries to find somewhere to land, and how close another bird may stand.
const PERCH_TRIES: u32 = 16;
const PERCH_SPACING: f32 = 1.1;
/// Flatter than this it can stand on it: the `y` of the normal.
const PERCH_FLATNESS: f32 = 0.8;
/// Under the first it wants to sleep; it's what a bird calls dark.
const DARK: f32 = 0.12;
/// How much of its heading may point up, in a normal flight.
const CLIMB_MOST: f32 = 0.45;
const DIVE_STEEPEST: f32 = 0.9;
/// How long it goes between calls while it's circling, as a share of the usual.
const CIRCLING_CALLS: f32 = 0.35;
/// Under this much daylight it has nothing to say.
const QUIET_AT_NIGHT: f32 = 0.2;
/// How far from its middle a blow gets it, and how high a dead one lies on its side, times its size.
const RADIUS: f32 = 0.2;
const BODY_LIFT: f32 = 0.06;
/// The height a dive commits at: it stops following the fish.
const COMMIT_HEIGHT: f32 = 3.5;
/// A dive is seen from this high by what's under the water.
const SEEN_HEIGHT: f32 = 5.0;
const SEEN_RADIUS: f32 = 3.5;
/// How strongly the rotation follows, per second.
const TURN_SETTLE: f32 = 9.0;
/// Below this share of hunger it isn't interested in food; over it, it will leave a perch.
const SATED: f32 = 0.2;
const PECKISH: f32 = 0.4;
/// Tiredness under which it's restless, and over which it must rest.
const RESTED: f32 = 0.15;
const EXHAUSTED: f32 = 0.85;

/// What a bird is doing, for whoever asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BirdDoing {
    Standing,
    Flying,
    /// Over something it wants.
    Circling,
    Diving,
    /// On the water.
    Swimming,
    Landing,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Doing {
    Standing { left: f32 },
    Walking { to: Vec3 },
    TakingOff { left: f32 },
    Roaming { to: Vec3 },
    Circling { center: Vec3, school: Option<usize>, left: f32, angle: f32 },
    Diving { at: Vec3, school: usize },
    Under { left: f32, ate: bool },
    Swimming { left: f32 },
    Landing { on: Vec3 },
}

/// How far beyond its range one comes in from, and how far from anyone it's gone for good.
const ARRIVES_FROM: f32 = 60.0;
const GONE_FROM: f32 = 150.0;

#[derive(Debug)]
struct Bird {
    position: Vec3,
    /// Where it's flying, unit length.
    heading: Vec3,
    speed: f32,
    /// How far into a turn, -1 to 1.
    bank: f32,
    rotation: Quat,
    doing: Doing,
    animator: Animator,
    /// `0..1`.
    hunger: f32,
    tiredness: f32,
    think_in: f32,
    look_in: f32,
    dodge: Vec3,
    dodge_left: f32,
    /// Seconds left of beating its wings, or of holding them still if negative.
    beating: f32,
    /// How far along its pose is behind.
    pose_dt: f32,
    close: bool,
    drawn: bool,
    size: f32,
    shade: f32,
    meals: u32,
    dives: u32,
    /// It's had enough of this place: it flies off, and it's gone once nobody can see it.
    leaving: bool,
    /// Seconds until it calls.
    call_in: f32,
}

/// Birds of a species that live around the same place.
#[derive(Debug)]
pub(super) struct Flock {
    species: Arc<SpeciesDesc>,
    mesh: MeshId,
    birds: Vec<Bird>,
    home: Vec3,
    radius: f32,
    /// Seen diving, and how long ago that will be forgotten.
    calls: Vec<(Vec3, f32)>,
    rng: Rng,
    /// Their own, so what they say changes nothing about what they do.
    voice: Rng,
    tick: u32,
    /// Whether it has looked for places to stand yet.
    placed: bool,
    skeleton: Arc<Skeleton>,
    ledger: Ledger,
}

impl Doing {
    fn shift(&mut self, offset: Vec3) {
        match self {
            Self::Walking { to } | Self::Roaming { to } => *to += offset,
            Self::Circling { center, .. } => *center += offset,
            Self::Diving { at, .. } => *at += offset,
            Self::Landing { on } => *on += offset,
            Self::Standing { .. } | Self::TakingOff { .. } | Self::Under { .. } | Self::Swimming { .. } => {}
        }
    }
}

impl Population for Flock {
    fn shift(&mut self, offset: Vec3) {
        self.home += offset;
        for call in &mut self.calls {
            call.0 += offset;
        }
        for bird in &mut self.birds {
            bird.position += offset;
            bird.doing.shift(offset);
        }
    }

    fn near(&self, point: Vec3, radius: f32) -> bool {
        self.birds.iter().any(|bird| bird.position.distance(point) < radius)
    }

    fn species(&self) -> &SpeciesDesc {
        &self.species
    }

    fn ledger(&mut self) -> &mut Ledger {
        &mut self.ledger
    }

    fn number(&self) -> usize {
        self.birds.iter().filter(|bird| !bird.leaving).count()
    }

    fn fed(&self) -> f32 {
        fed_from(self.birds.iter().filter(|bird| !bird.leaving).map(|bird| bird.hunger))
    }

    fn starving(&self) -> usize {
        starving_from(self.birds.iter().filter(|bird| !bird.leaving).map(|bird| bird.hunger))
    }

    /// It comes in from the far side of its range, already grown, from wherever it was raised.
    fn add(&mut self, place: &Place, focus: Vec3, _young: bool) -> bool {
        let angle = self.rng.range(-1.0, 1.0);
        let away = flat(self.home - focus).normalize_or(Vec3::X);
        let at = self.home + Quat::from_rotation_y(angle) * away * (self.radius * 1.3 + ARRIVES_FROM);
        let altitude = self.rng.range(self.species.flight.altitude.0, self.species.flight.altitude.1);
        let to = self.roam_point(place);
        let mut bird = new_bird(&self.species, &self.skeleton, &mut self.rng);
        bird.position = Vec3::new(at.x, place.surface(at.x, at.z) + altitude, at.z);
        bird.heading = flat(to - bird.position).normalize_or(Vec3::X);
        bird.speed = self.species.flight.cruise;
        bird.doing = Doing::Roaming { to };
        bird.hunger = self.rng.range(0.0, 0.3);
        self.birds.push(bird);
        true
    }

    /// The hungriest gives up on this place. Leaving is a thing it can do in front of anyone.
    fn take_away(&mut self, _focus: Vec3) -> bool {
        let hungriest = self.birds.iter_mut().filter(|bird| !bird.leaving).max_by(|a, b| a.hunger.total_cmp(&b.hunger));
        hungriest.map(|bird| bird.leaving = true).is_some()
    }

    fn truncate(&mut self, number: usize) {
        self.birds.truncate(number);
    }

    /// It comes down however it was, and ends on its side.
    fn strike(&mut self, hits: &mut dyn FnMut(Vec3, f32) -> bool, dead: &mut Vec<Carcass>) {
        let (mesh, body) = (self.mesh, self.species.flight.body_height);
        self.birds.retain(|bird| {
            let struck = hits(bird.position, RADIUS * bird.size);
            if struck {
                let pose = BodyPose::Own { mesh, matrices: bird.animator.skinning_matrices().to_vec() };
                let on_its_side = laid_flat(bird.rotation) * Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
                let origin = bird.position - bird.rotation * Vec3::Y * body * bird.size;
                dead.push(Carcass {
                    lift: BODY_LIFT * bird.size,
                    floats: true,
                    ..Carcass::new(origin, on_its_side, bird.size, bird.shade, pose)
                });
            }
            !struck
        });
    }
}

fn new_bird(species: &SpeciesDesc, skeleton: &Arc<Skeleton>, rng: &mut Rng) -> Bird {
    let angle = rng.range(0.0, std::f32::consts::TAU);
    Bird {
        position: Vec3::ZERO,
        heading: Vec3::new(angle.cos(), 0.0, angle.sin()),
        speed: 0.0,
        bank: 0.0,
        rotation: Quat::IDENTITY,
        doing: Doing::Standing { left: rng.range(1.0, 30.0) },
        animator: Animator::new(skeleton.clone()),
        hunger: rng.range(0.1, 0.6),
        tiredness: rng.range(0.0, 0.5),
        think_in: rng.range(0.0, THINK_SECONDS),
        look_in: rng.range(0.0, LOOK_SECONDS),
        dodge: Vec3::Y,
        dodge_left: 0.0,
        beating: 0.0,
        pose_dt: 0.0,
        close: false,
        drawn: true,
        size: rng.range(species.size.0, species.size.1),
        shade: rng.range(0.92, 1.08),
        meals: 0,
        dives: 0,
        leaving: false,
        // Looks at the generator without moving it: a noise changes nothing about what happens.
        call_in: rng.clone().range(species.sounds.call_every.0, species.sounds.call_every.1),
    }
}

impl Flock {
    pub(super) fn new(group: &SceneFauna, meshes: &[MeshData]) -> Option<Self> {
        let skeleton = meshes.get(group.mesh)?.skeleton.clone()?;
        let species = group.species.clone();
        let mut rng = Rng::new(group.seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xB14D);
        let birds = (0..group.count)
            .map(|_| {
                let mut bird = new_bird(&species, &skeleton, &mut rng);
                bird.position = group.position;
                bird
            })
            .collect();
        Some(Self {
            mesh: MeshId(group.mesh),
            birds,
            home: group.position,
            radius: group.radius,
            calls: Vec::new(),
            voice: Rng::new(group.seed ^ 0x5EA6),
            rng,
            tick: 0,
            placed: false,
            ledger: Ledger::of(group),
            skeleton,
            species,
        })
    }

    pub(super) fn population(&self) -> usize {
        self.birds.len()
    }

    pub(super) fn positions(&self) -> impl Iterator<Item = Vec3> + '_ {
        self.birds.iter().map(|bird| bird.position)
    }

    pub(super) fn doings(&self) -> impl Iterator<Item = BirdDoing> + '_ {
        self.birds.iter().map(|bird| match bird.doing {
            Doing::Standing { .. } | Doing::Walking { .. } => BirdDoing::Standing,
            Doing::TakingOff { .. } | Doing::Roaming { .. } => BirdDoing::Flying,
            Doing::Circling { .. } => BirdDoing::Circling,
            Doing::Diving { .. } | Doing::Under { .. } => BirdDoing::Diving,
            Doing::Swimming { .. } => BirdDoing::Swimming,
            Doing::Landing { .. } => BirdDoing::Landing,
        })
    }

    pub(super) fn hungers(&self) -> impl Iterator<Item = f32> + '_ {
        self.birds.iter().map(|bird| bird.hunger)
    }

    pub(super) fn meals(&self) -> u32 {
        self.birds.iter().map(|bird| bird.meals).sum()
    }

    pub(super) fn dives(&self) -> u32 {
        self.birds.iter().map(|bird| bird.dives).sum()
    }

    /// Puts the birds somewhere to stand, and some of them in the air.
    pub(super) fn place(&mut self, place: &Place, physics: &Physics) {
        let SpeciesDesc { flight, .. } = &*self.species.clone();
        for i in 0..self.birds.len() {
            let perch = self.find_perch(place, physics, &[]);
            let up_there = self.rng.f32() < 0.5 && place.daylight > DARK;
            let (x, z) = (self.home.x, self.home.z);
            let spread = self.radius * 0.6;
            let air = Vec3::new(x + self.rng.range(-spread, spread), 0.0, z + self.rng.range(-spread, spread));
            let altitude = self.rng.range(flight.altitude.0, flight.altitude.1);
            let to = self.roam_point(place);
            let bird = &mut self.birds[i];
            match perch {
                Some(perch) if !up_there => {
                    bird.position = perch + Vec3::Y * flight.body_height * bird.size;
                    bird.doing = Doing::Standing { left: bird.think_in * 40.0 };
                }
                _ => {
                    bird.position = Vec3::new(air.x, place.surface(air.x, air.z) + altitude, air.z);
                    bird.speed = flight.cruise;
                    bird.doing = Doing::Roaming { to };
                }
            }
        }
        self.placed = true;
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn update(
        &mut self,
        place: &Place,
        physics: &Physics,
        schools: &mut [School],
        threats: &[Threat],
        focus: Vec3,
        dt: f32,
        wake: &mut Wake,
    ) {
        if !self.placed {
            self.place(place, physics);
        }
        self.tick = self.tick.wrapping_add(1);
        self.calls.retain_mut(|call| {
            call.1 -= dt;
            call.1 > 0.0
        });
        let species = self.species.clone();
        // A school too far from anyone isn't being worked out, so nothing can catch it. That's
        // no reason to go hungry: it's only that nobody is there to see the hunting.
        let can_hunt = schools
            .iter()
            .filter(|school| species.diet.iter().any(|food| *food == school.species.name))
            .any(School::huntable);
        for i in 0..self.birds.len() {
            let thinks = {
                let bird = &mut self.birds[i];
                bird.feel(&species, can_hunt || species.diet.is_empty(), dt);
                bird.think_in -= dt;
                let thinks = bird.think_in <= 0.0;
                if thinks {
                    bird.think_in += THINK_SECONDS;
                }
                thinks
            };
            if thinks {
                self.think(i, place, physics, schools);
            }
            self.flee(i, threats);
            self.act(i, place, physics, schools, dt, wake);
            self.call(i, place, dt, wake);
            self.show(i, focus, dt);
        }
        let (home, far) = (self.home, self.radius * 1.3 + GONE_FROM);
        self.birds.retain(|bird| {
            !bird.leaving || (bird.position.distance(focus) < GONE_FROM && flat(bird.position - home).length() < far)
        });
    }

    /// What it does next, if it isn't in the middle of something it can't stop.
    fn think(&mut self, i: usize, place: &Place, physics: &Physics, schools: &[School]) {
        let flight = &self.species.flight;
        let position = self.birds[i].position;
        if self.birds[i].leaving {
            let away = flat(position - self.home).normalize_or(Vec3::X);
            let to = self.home + away * (self.radius * 1.3 + GONE_FROM * 2.0);
            let to = to.with_y(position.y.max(place.surface(to.x, to.z) + flight.altitude.1));
            match self.birds[i].doing {
                Doing::Standing { .. } | Doing::Walking { .. } | Doing::Swimming { .. } => {
                    self.birds[i].doing = Doing::TakingOff { left: flight.take_off };
                }
                Doing::Roaming { .. } | Doing::Circling { .. } | Doing::Landing { .. } => {
                    self.birds[i].doing = Doing::Roaming { to };
                }
                _ => {}
            }
            return;
        }
        let (hunger, tiredness) = (self.birds[i].hunger, self.birds[i].tiredness);
        let wants_rest = place.daylight < DARK || place.storm || tiredness > EXHAUSTED;
        let idle = hunger < SATED && tiredness > RESTED;
        match self.birds[i].doing {
            Doing::Standing { left } => {
                let restless = !wants_rest && (hunger > PECKISH || (left <= 0.0 && tiredness < RESTED));
                if restless {
                    self.birds[i].doing = Doing::TakingOff { left: flight.take_off };
                } else if left <= 0.0 {
                    // Nothing to do. A few steps, maybe.
                    let angle = self.rng.range(0.0, std::f32::consts::TAU);
                    let far = self.rng.range(0.8, 2.5);
                    let to = position + Vec3::new(angle.cos(), 0.0, angle.sin()) * far;
                    let wait = self.rng.range(4.0, 20.0);
                    let ground = self.ground_under(place, physics, to);
                    let foot = position.y - flight.body_height * self.birds[i].size;
                    match ground {
                        Some(ground) if self.rng.f32() < 0.6 && (ground - foot).abs() < 0.25 => {
                            self.birds[i].doing =
                                Doing::Walking { to: Vec3::new(to.x, ground + position.y - foot, to.z) };
                        }
                        _ => self.birds[i].doing = Doing::Standing { left: wait },
                    }
                }
            }
            Doing::Swimming { left } => {
                if left <= 0.0 {
                    self.birds[i].doing = Doing::TakingOff { left: flight.take_off };
                }
            }
            Doing::Roaming { .. } | Doing::Circling { .. } => {
                if wants_rest || idle {
                    self.go_land(i, place, physics);
                    return;
                }
                self.hunt(i, place, schools);
            }
            Doing::Landing { on } => {
                // The place may have been taken, or it may have changed its mind.
                let taken = self.birds.iter().enumerate().any(|(other, bird)| {
                    other != i
                        && matches!(bird.doing, Doing::Standing { .. } | Doing::Walking { .. })
                        && flat(bird.position - on).length() < PERCH_SPACING * 0.7
                });
                if taken {
                    self.go_land(i, place, physics);
                }
            }
            _ => {}
        }
    }

    /// Looks for something to eat, and comes down on it.
    fn hunt(&mut self, i: usize, place: &Place, schools: &[School]) {
        let SpeciesDesc { flight, diet, .. } = &*self.species;
        let bird = &self.birds[i];
        let position = bird.position;
        let hungry = bird.hunger >= SATED;
        let in_range = |point: Vec3| flat(point - self.home).length() < self.radius * 1.3;

        // Food it can see: the school nearest it, close enough to the surface.
        let seen = hungry
            .then(|| {
                schools
                    .iter()
                    .enumerate()
                    .filter(|(_, school)| diet.iter().any(|food| *food == school.species.name))
                    .filter(|(_, school)| school.visible_from_above(flight.sees_down))
                    .map(|(index, school)| (index, school.center_now()))
                    .filter(|(_, center)| in_range(*center))
                    .filter(|(_, center)| flat(*center - position).length() < flight.sight)
                    .min_by(|a, b| flat(a.1 - position).length().total_cmp(&flat(b.1 - position).length()))
            })
            .flatten();
        // Or another one going down: there must be something there.
        let heard = hungry.then(|| self.calls.iter().find(|(at, _)| at.distance(position) < CALL_HEARD)).flatten();

        match self.birds[i].doing {
            Doing::Roaming { .. } => {
                let goal = seen.map(|(index, center)| (Some(index), center)).or(heard.map(|(at, _)| (None, *at)));
                if let Some((school, center)) = goal {
                    let angle = self.rng.range(0.0, std::f32::consts::TAU);
                    let left = self.rng.range(12.0, 30.0);
                    self.birds[i].doing = Doing::Circling { center, school, left, angle };
                }
            }
            Doing::Circling { school, left, angle, .. } => {
                let school = seen.map(|(index, _)| index).or(school);
                if left <= 0.0 || !hungry {
                    let to = self.roam_point(place);
                    self.birds[i].doing = Doing::Roaming { to };
                    return;
                }
                let center = seen.map(|(_, center)| center).or_else(|| school.map(|s| schools[s].center_now()));
                if let Doing::Circling { center: old, .. } = self.birds[i].doing {
                    let center = center.unwrap_or(old);
                    self.birds[i].doing = Doing::Circling { center, school, left, angle };
                }
                // Over one close to the surface, it might just go.
                let Some(index) = school else { return };
                let Some(fish) = schools[index].fish_near_surface(position, flight.dive_reach, flight.sees_down) else {
                    return;
                };
                if self.rng.f32() < flight.dive_chance {
                    self.birds[i].doing = Doing::Diving { at: fish, school: index };
                    self.birds[i].beating = 0.0;
                    self.birds[i].dives += 1;
                    self.birds[i].call_in = 0.0;
                    self.calls.push((fish, CALL_SECONDS));
                }
            }
            _ => {}
        }
    }

    /// Now and then it says something, and more often when there's something below it.
    fn call(&mut self, i: usize, place: &Place, dt: f32, wake: &mut Wake) {
        let bird = &mut self.birds[i];
        bird.call_in -= dt;
        if bird.call_in > 0.0 {
            return;
        }
        let sounds = &self.species.sounds;
        let (soonest, latest) = sounds.call_every;
        let excited = if matches!(bird.doing, Doing::Circling { .. }) { CIRCLING_CALLS } else { 1.0 };
        bird.call_in = self.voice.range(soonest, latest).max(1.0) * excited;
        if place.daylight > QUIET_AT_NIGHT && !sounds.calls.is_empty() {
            let call = &sounds.calls[self.voice.next_u64() as usize % sounds.calls.len()];
            wake.play(&self.species, Some(call), Emitter::loud(bird.position));
        }
    }

    /// Whoever comes too close to something on the ground or the water scares it into the air.
    fn flee(&mut self, i: usize, threats: &[Threat]) {
        let SpeciesDesc { flight, senses, .. } = &*self.species;
        let bird = &mut self.birds[i];
        if !matches!(bird.doing, Doing::Standing { .. } | Doing::Walking { .. } | Doing::Swimming { .. }) {
            return;
        }
        let noticed =
            threats.iter().find(|threat| threat.position.distance(bird.position) < threat.radius * senses.notice);
        if let Some(threat) = noticed {
            bird.heading = flat(bird.position - threat.position).normalize_or(bird.heading);
            bird.doing = Doing::TakingOff { left: flight.take_off };
            bird.tiredness = bird.tiredness.min(0.5);
        }
    }

    /// Picks somewhere to come down, or keeps flying if there's nowhere.
    fn go_land(&mut self, i: usize, place: &Place, physics: &Physics) {
        let others: Vec<Vec3> = self
            .birds
            .iter()
            .enumerate()
            .filter(|(other, _)| *other != i)
            .filter_map(|(_, bird)| match bird.doing {
                Doing::Landing { on } => Some(on),
                Doing::Standing { .. } | Doing::Walking { .. } => Some(bird.position),
                _ => None,
            })
            .collect();
        if let Some(on) = self.find_perch(place, physics, &others) {
            self.birds[i].doing = Doing::Landing { on };
        } else if !matches!(self.birds[i].doing, Doing::Roaming { .. }) {
            let to = self.roam_point(place);
            self.birds[i].doing = Doing::Roaming { to };
        }
    }

    fn find_perch(&mut self, place: &Place, physics: &Physics, others: &[Vec3]) -> Option<Vec3> {
        for _ in 0..PERCH_TRIES {
            let angle = self.rng.range(0.0, std::f32::consts::TAU);
            let far = self.radius * self.rng.f32().sqrt();
            let point = self.home + Vec3::new(angle.cos(), 0.0, angle.sin()) * far;
            let top = Vec3::new(point.x, 400.0, point.z);
            let Some(hit) = physics.raycast_scenery(top, Vec3::NEG_Y, 800.0) else { continue };
            let level = place.waters.calm_at(point.x, point.z).map_or(f32::NEG_INFINITY, |(level, _)| level);
            if hit.normal.y > PERCH_FLATNESS
                && hit.point.y > level + 0.3
                && others.iter().all(|other| flat(*other - hit.point).length() > PERCH_SPACING)
            {
                return Some(hit.point);
            }
        }
        None
    }

    fn ground_under(&self, place: &Place, physics: &Physics, point: Vec3) -> Option<f32> {
        let hit = physics.raycast_scenery(point + Vec3::Y * 1.5, Vec3::NEG_Y, 4.0)?;
        let level = place.waters.calm_at(point.x, point.z).map_or(f32::NEG_INFINITY, |(level, _)| level);
        (hit.normal.y > PERCH_FLATNESS && hit.point.y > level + 0.3).then_some(hit.point.y)
    }

    fn roam_point(&mut self, place: &Place) -> Vec3 {
        let flight = &self.species.flight;
        let angle = self.rng.range(0.0, std::f32::consts::TAU);
        let far = self.radius * self.rng.f32().sqrt();
        let point = self.home + Vec3::new(angle.cos(), 0.0, angle.sin()) * far;
        let altitude = self.rng.range(flight.altitude.0, flight.altitude.1);
        Vec3::new(point.x, place.surface(point.x, point.z) + altitude, point.z)
    }

    /// Carries out what it's doing, this frame.
    fn act(&mut self, i: usize, place: &Place, physics: &Physics, schools: &mut [School], dt: f32, wake: &mut Wake) {
        let species = self.species.clone();
        let flight = &species.flight;
        let body = flight.body_height * self.birds[i].size;
        match self.birds[i].doing {
            Doing::Standing { left } => {
                let bird = &mut self.birds[i];
                bird.doing = Doing::Standing { left: left - dt };
                bird.speed = 0.0;
            }
            Doing::Walking { to } => {
                let bird = &mut self.birds[i];
                let way = flat(to - bird.position);
                if way.length() < 0.1 {
                    bird.doing = Doing::Standing { left: 8.0 };
                } else {
                    bird.heading = bird.heading.lerp(way.normalize(), (dt * 6.0).min(1.0)).normalize_or(bird.heading);
                    bird.speed = flight.walk;
                    bird.position += way.normalize() * flight.walk * dt;
                    bird.position.y += (to.y - bird.position.y) * (dt * 8.0).min(1.0);
                }
            }
            Doing::TakingOff { left } => {
                let bird = &mut self.birds[i];
                bird.speed = (bird.speed + flight.cruise / flight.take_off.max(0.1) * dt).min(flight.cruise);
                let way = flat(bird.heading).normalize_or(Vec3::NEG_Z);
                bird.heading = (way + Vec3::Y * 0.6).normalize();
                // It takes off gently at first, as it isn't going anywhere yet.
                bird.position += bird.heading * bird.speed.max(1.0) * dt;
                bird.doing = Doing::TakingOff { left: left - dt };
                if left <= 0.0 {
                    let to = self.roam_point(place);
                    self.birds[i].doing = Doing::Roaming { to };
                }
            }
            Doing::Roaming { to } => {
                if flat(to - self.birds[i].position).length() < 12.0 {
                    let to = self.roam_point(place);
                    self.birds[i].doing = Doing::Roaming { to };
                } else {
                    self.fly(i, to, flight.cruise, physics, place, dt);
                }
            }
            Doing::Circling { center, school, left, angle } => {
                let around = (flight.altitude.0 * 0.8).max(6.0);
                let bird = &self.birds[i];
                let radius = flight.circle;
                let angle = angle + bird.speed / radius * dt;
                let height = place.surface(center.x, center.z) + around + (angle * 0.4).sin() * 1.5;
                let to = Vec3::new(center.x + angle.cos() * radius, height, center.z + angle.sin() * radius);
                self.birds[i].doing = Doing::Circling { center, school, left: left - dt, angle };
                self.fly(i, to, flight.cruise * 0.85, physics, place, dt);
            }
            Doing::Diving { at, school } => self.dive(i, at, school, place, schools, dt, wake),
            Doing::Under { left, ate } => {
                let bird = &mut self.birds[i];
                let level = place.waters.calm_at(bird.position.x, bird.position.z).map_or(bird.position.y, |w| w.0);
                bird.speed = 0.0;
                let under = level - flight.plunge;
                bird.position.y += (under - bird.position.y) * (dt * 10.0).min(1.0);
                bird.doing = Doing::Under { left: left - dt, ate };
                if left <= 0.0 {
                    wake.splashes.push((bird.position, 0.25));
                    bird.position.y = level + body * 0.4;
                    if ate {
                        bird.animator.replay_once("swallow", 0.15);
                    }
                    let left = if ate { flight.swim.1 } else { flight.swim.0 };
                    bird.doing = Doing::Swimming { left };
                }
            }
            Doing::Swimming { left } => {
                let bird = &mut self.birds[i];
                let point = place.waters.at(bird.position.x, bird.position.z);
                let level = point.map_or(bird.position.y, |water| water.level);
                let target = level + body * 0.4;
                bird.position.y += (target - bird.position.y) * (dt * 6.0).min(1.0);
                bird.speed = 0.0;
                bird.doing = Doing::Swimming { left: left - dt };
            }
            Doing::Landing { on } => self.land(i, on, body, physics, place, dt),
        }
        // A slope can come up to meet it, leaving the ground or landing on it.
        if !matches!(self.birds[i].doing, Doing::Standing { .. } | Doing::Walking { .. }) {
            let bird = &mut self.birds[i];
            bird.position.y = bird.position.y.max(place.floor(bird.position.x, bird.position.z) + body * 0.5);
        }
    }

    fn fly(&mut self, i: usize, to: Vec3, speed: f32, physics: &Physics, place: &Place, dt: f32) {
        let flight = &self.species.flight;
        let bird = &mut self.birds[i];

        // Something ahead?
        bird.look_in -= dt;
        if bird.look_in <= 0.0 {
            bird.look_in = LOOK_SECONDS;
            let ahead = bird.speed.max(flight.cruise) * LOOK_SECONDS_AHEAD;
            if let Some(hit) = physics.raycast_scenery(bird.position, bird.heading, ahead) {
                bird.dodge = (hit.normal + Vec3::Y * 1.2).normalize_or(Vec3::Y);
                bird.dodge_left = DODGE_SECONDS;
            }
        }
        let mut wanted = (to - bird.position).normalize_or(bird.heading);
        if bird.dodge_left > 0.0 {
            bird.dodge_left -= dt;
            wanted = (wanted * 0.2 + bird.dodge).normalize_or(wanted);
        }
        wanted.y = wanted.y.clamp(-CLIMB_MOST, CLIMB_MOST);
        let turned = steer(bird.heading, wanted.normalize_or(bird.heading), flight.turn, dt);
        let yaw = f32::atan2(bird.heading.cross(turned).y, bird.heading.dot(turned));
        bird.bank += ((yaw / (flight.turn * dt).max(1e-4)).clamp(-1.0, 1.0) - bird.bank) * (dt * 4.0).min(1.0);
        bird.heading = turned;
        bird.speed += (speed - bird.speed).clamp(-flight.cruise * dt, flight.cruise * dt);
        bird.position += bird.heading * bird.speed * dt;
        // Never into what's under it, whatever it's after.
        let lowest = place.surface(bird.position.x, bird.position.z) + 1.0;
        bird.position.y = bird.position.y.max(lowest);

        // It beats its wings to climb, and in bouts when it needs speed, and otherwise glides.
        bird.beating -= dt;
        let climbing = bird.heading.y > 0.12 || bird.speed < speed * 0.8;
        if climbing && bird.beating < 0.0 {
            bird.beating = 1.2;
        } else if bird.beating < -flight.glide {
            bird.beating = flight.beat_bout;
        }
    }

    /// All the way down on a fish, and then whatever's there.
    #[allow(clippy::too_many_arguments)]
    fn dive(
        &mut self,
        i: usize,
        at: Vec3,
        index: usize,
        place: &Place,
        schools: &mut [School],
        dt: f32,
        wake: &mut Wake,
    ) {
        let species = self.species.clone();
        let flight = &species.flight;
        let level = place.waters.calm_at(at.x, at.z).map_or(at.y, |w| w.0);
        let bird = &mut self.birds[i];
        let height = bird.position.y - level;

        // It follows the one it picked until it can't turn any more.
        let mut at = at;
        if height > COMMIT_HEIGHT
            && let Some(fish) = schools[index].fish_near_surface(bird.position, flight.dive_reach, flight.sees_down)
        {
            at = fish;
        }
        bird.doing = Doing::Diving { at, school: index };
        let wanted = (Vec3::new(at.x, level, at.z) - bird.position).normalize_or(Vec3::NEG_Y);
        let mut wanted = wanted;
        wanted.y = wanted.y.max(-DIVE_STEEPEST);
        let turned = steer(bird.heading, wanted.normalize_or(bird.heading), flight.turn * 2.5, dt);
        bird.heading = turned;
        bird.bank *= 1.0 - (dt * 4.0).min(1.0);
        bird.speed += (flight.dive_speed - bird.speed).clamp(-20.0 * dt, 20.0 * dt);
        bird.position += bird.heading * bird.speed * dt;
        if height < SEEN_HEIGHT {
            wake.threats
                .push(Threat { position: Vec3::new(bird.position.x, level, bird.position.z), radius: SEEN_RADIUS });
        }
        if bird.position.y > level {
            return;
        }
        // In the water: it has this one chance.
        let entry = Vec3::new(bird.position.x, level, bird.position.z);
        wake.splashes.push((entry, 0.45));
        let ate = schools[index].take_fish_at(entry, flight.catch_radius, flight.plunge + 0.4);
        bird.heading = flat(bird.heading).normalize_or(Vec3::NEG_Z);
        if ate {
            bird.hunger = (bird.hunger - flight.meal).max(0.0);
            bird.meals += 1;
        }
        bird.doing = Doing::Under { left: flight.under, ate };
    }

    fn land(&mut self, i: usize, on: Vec3, body: f32, physics: &Physics, place: &Place, dt: f32) {
        let flight = &self.species.flight;
        let spot = on + Vec3::Y * body;
        let bird = &self.birds[i];
        let far = bird.position.distance(spot);
        if far > LANDING_DISTANCE {
            // Far off it flies over to it, high enough to come down on it.
            let high = spot + Vec3::Y * (far * 0.3).min(flight.altitude.1);
            self.fly(i, high, flight.cruise, physics, place, dt);
            return;
        }
        let bird = &mut self.birds[i];
        let t = (far / LANDING_DISTANCE).clamp(0.0, 1.0);
        bird.speed += (flight.walk.max(1.5) + (flight.cruise - flight.walk.max(1.5)) * t - bird.speed)
            .clamp(-14.0 * dt, 14.0 * dt);
        let way = (spot - bird.position).normalize_or(bird.heading);
        // Slow and low, it can turn as it likes.
        bird.heading = steer(bird.heading, way, flight.turn * (1.0 + 4.0 * (1.0 - t)), dt);
        bird.bank *= 1.0 - (dt * 5.0).min(1.0);
        bird.position += bird.heading.lerp(way, 1.0 - t).normalize_or(way) * bird.speed * dt;
        if bird.position.distance(spot) < (bird.speed * dt * 1.5).max(0.12) {
            bird.position = spot;
            bird.heading = flat(bird.heading).normalize_or(Vec3::NEG_Z);
            bird.speed = 0.0;
            bird.doing = Doing::Standing { left: 10.0 };
        }
    }

    /// Picks its clip and works out how it looks.
    fn show(&mut self, i: usize, focus: Vec3, dt: f32) {
        let species = self.species.clone();
        let flight = &species.flight;
        let tick = self.tick;
        let bird = &mut self.birds[i];
        let distance = bird.position.distance(focus);
        bird.drawn = distance < DRAW_DISTANCE;
        bird.close = distance < CLOSE_DISTANCE;

        let flare = match bird.doing {
            Doing::Landing { on } => {
                let far = bird.position.distance(on);
                (1.0 - far / LANDING_DISTANCE).clamp(0.0, 1.0)
            }
            Doing::TakingOff { .. } => 0.6,
            _ => 0.0,
        };

        // Flat on what it's standing on, nose wherever it's flying otherwise.
        let heading = match bird.doing {
            Doing::Standing { .. } | Doing::Walking { .. } | Doing::Swimming { .. } | Doing::Under { .. } => {
                flat(bird.heading).normalize_or(Vec3::NEG_Z)
            }
            Doing::Landing { .. } => {
                let level = flat(bird.heading).normalize_or(Vec3::NEG_Z);
                level.lerp(bird.heading, 1.0 - flare).normalize_or(level)
            }
            _ => bird.heading,
        };
        let right = heading.cross(Vec3::Y).normalize_or(Vec3::X);
        let up = right.cross(heading);
        let look = Quat::from_mat3(&Mat3::from_cols(right, up, -heading));
        let lean = Quat::from_axis_angle(heading, -bird.bank * flight.bank.to_radians());
        let pitch = Quat::from_axis_angle(right, flare * flight.flare.to_radians());
        let facing = Quat::from_rotation_y(f32::atan2(species.forward.x, -species.forward.z));
        let target = lean * pitch * look * facing;
        bird.rotation = bird.rotation.slerp(target, (dt * TURN_SETTLE).min(1.0));

        if !bird.drawn {
            return;
        }
        let clip = match bird.doing {
            Doing::Standing { .. } => "idle",
            Doing::Walking { .. } => "walk",
            Doing::TakingOff { .. } => "flap_hard",
            Doing::Roaming { .. } | Doing::Circling { .. } => {
                if bird.beating > 0.0 {
                    "flap"
                } else {
                    "glide"
                }
            }
            Doing::Diving { .. } | Doing::Under { .. } => "dive",
            Doing::Swimming { .. } => "float",
            Doing::Landing { .. } => {
                if flare > 0.15 {
                    "land"
                } else if bird.beating > 0.0 {
                    "flap"
                } else {
                    "glide"
                }
            }
        };
        let swallowing = bird.animator.current() == Some("swallow") && !bird.animator.finished();
        if !swallowing {
            bird.animator.play(clip, if clip == "dive" { 0.12 } else { 0.28 });
        }
        bird.pose_dt += dt;
        let stride = if bird.close { 1 } else { FAR_POSE_STRIDE };
        if (tick + i as u32).is_multiple_of(stride) {
            bird.animator.update(bird.pose_dt);
            bird.pose_dt = 0.0;
        }
    }

    pub(super) fn push_draws(&self, handles: &[MeshHandle], draws: &mut Vec<DrawItem>, joints: &mut Vec<Mat4>) {
        let Some(&mesh) = handles.get(self.mesh.0) else { return };
        let body = self.species.flight.body_height;
        for bird in self.birds.iter().filter(|bird| bird.drawn) {
            let matrices = bird.animator.skinning_matrices();
            joints.extend_from_slice(matrices);
            draws.push(DrawItem {
                mesh,
                transform: Transform {
                    // `position` is its body, and the model has its origin under its feet.
                    position: bird.position - bird.rotation * Vec3::Y * body * bird.size,
                    rotation: bird.rotation,
                    scale: Vec3::splat(bird.size),
                },
                color: Vec3::splat(bird.shade),
                emissive: 0.0,
                joints: Some((joints.len() - matrices.len()) as u32),
            });
        }
    }
}

impl Bird {
    fn feel(&mut self, species: &SpeciesDesc, gets_hungry: bool, dt: f32) {
        let needs = &species.needs;
        if gets_hungry {
            self.hunger = (self.hunger + dt / needs.hungry_after.max(1.0)).min(1.0);
        }
        let resting = matches!(self.doing, Doing::Standing { .. } | Doing::Walking { .. } | Doing::Swimming { .. });
        let tiring = if resting { -dt / needs.rested_after.max(1.0) } else { dt / needs.tired_after.max(1.0) };
        self.tiredness = (self.tiredness + tiring).clamp(0.0, 1.0);
    }
}

/// `from` turned towards `to`, no more than `turn` radians per second.
fn steer(from: Vec3, to: Vec3, turn: f32, dt: f32) -> Vec3 {
    let angle = from.angle_between(to);
    let most = turn * dt;
    if angle <= most {
        return to;
    }
    let t = most / angle;
    from.lerp(to, t).normalize_or(from)
}
