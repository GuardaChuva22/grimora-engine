//! Jellyfish: no senses and no needs. It beats to stay at its depth, and everything else is
//! what the water does to it. A storm throws it on the sand, where it's food for whatever
//! smells it (`Carcass`).

use std::sync::Arc;

use glam::{Quat, Vec3};
use grimora_core::Rng;
use grimora_scene::{SceneFauna, SpeciesDesc};

use super::ecology::{Ledger, Population};
use super::encounter::{BodyPose, Carcass, Wake};
use super::{Place, pose_index};

/// Beyond this it isn't drawn.
const DRAW_DISTANCE: f32 = 130.0;
/// How much of a beat is the bell closing: the rest is it opening slowly.
const CLOSING: f32 = 0.3;
/// Radians it tilts, at most, towards where it's being carried.
const MOST_TILT: f32 = 0.4;
/// How big the currents are, in meters, and how slowly they change.
const CURRENT_SIZE: f32 = 45.0;
const CURRENT_SECONDS: f32 = 200.0;
/// How many times its group's radius a jellyfish can drift from where the group lives.
const LEASH: f32 = 2.5;
/// A jellyfish can come or go only further than this from whoever is looking.
const UNSEEN: f32 = 60.0;
/// How far from its middle a blow gets it, times its size.
const RADIUS: f32 = 0.15;

#[derive(Debug)]
pub(super) struct Jelly {
    pub position: Vec3,
    velocity: Vec3,
    /// `0..1`, from the bell opening fully.
    pub phase: f32,
    pub size: f32,
    full_size: f32,
    pub shade: f32,
    habit: f32,
    pub rotation: Quat,
    pub drawn: bool,
}

impl Jelly {
    /// Puts it at the depth it wants, whatever `position` said about that.
    fn settle(&mut self, species: &SpeciesDesc, place: &Place) {
        let drift = &species.drift;
        let (x, z) = (self.position.x, self.position.z);
        let Some((level, _)) = place.waters.calm_at(x, z) else { return };
        let depth = drift.night_depth + (drift.day_depth - drift.night_depth) * place.daylight;
        let lowest = place.floor(x, z) + 0.6;
        self.position.y = (level - depth - self.habit).max(lowest).min(level - 0.5);
    }

    /// Flat on whatever it ends up on, the bell open, and it's no longer holding itself up.
    fn body(&self, species: &SpeciesDesc, poses: usize, at: Vec3) -> Carcass {
        let pose = BodyPose::Table { poses, pose: pose_index(0.75, 0, 0.0) };
        Carcass {
            shape: Vec3::new(1.15, 0.45, 1.15),
            left: species.drift.rots_in,
            food: species.drift.food,
            lift: 0.03,
            ..Carcass::new(at, self.rotation, self.size, self.shade, pose)
        }
    }

    /// How closed its bell is now, as a moment in the table of poses: the closing is quick.
    pub fn pose_phase(&self) -> f32 {
        // In the table a bell is fully open at 0.75 and fully closed at 0.25.
        let pose = if self.phase < CLOSING {
            0.75 + self.phase / CLOSING * 0.5
        } else {
            0.25 + (self.phase - CLOSING) / (1.0 - CLOSING) * 0.5
        };
        pose.fract()
    }
}

impl Population for Bloom {
    fn shift(&mut self, offset: Vec3) {
        self.home += offset;
        for jelly in &mut self.jellies {
            jelly.position += offset;
        }
    }

    fn near(&self, point: Vec3, radius: f32) -> bool {
        self.jellies.iter().any(|jelly| jelly.position.distance(point) < radius)
    }

    fn species(&self) -> &SpeciesDesc {
        &self.species
    }

    fn ledger(&mut self) -> &mut Ledger {
        &mut self.ledger
    }

    fn number(&self) -> usize {
        self.jellies.len()
    }

    /// Somewhere with water enough, that nobody is looking at.
    fn add(&mut self, place: &Place, focus: Vec3, young: bool) -> bool {
        let angle = self.rng.range(0.0, std::f32::consts::TAU);
        let far = self.radius * self.rng.f32().sqrt();
        let at = self.home + Vec3::new(angle.cos(), 0.0, angle.sin()) * far;
        let enough = place.depth(at.x, at.z).is_some_and(|depth| depth > self.species.drift.shallowest);
        if !enough || at.distance(focus) < UNSEEN {
            return false;
        }
        let mut jelly = new_jelly(&self.species, &mut self.rng, at);
        if young {
            jelly.size *= self.species.cycle.newborn;
        }
        jelly.settle(&self.species, place);
        self.jellies.push(jelly);
        true
    }

    /// Whichever is furthest, if that's too far to be seen.
    fn take_away(&mut self, focus: Vec3) -> bool {
        let furthest = self
            .jellies
            .iter()
            .enumerate()
            .map(|(index, jelly)| (index, jelly.position.distance(focus)))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        match furthest {
            Some((index, far)) if far > UNSEEN => {
                self.jellies.swap_remove(index);
                true
            }
            _ => false,
        }
    }

    fn grow(&mut self, share: f32) {
        for jelly in &mut self.jellies {
            jelly.size = (jelly.size + jelly.full_size * share).min(jelly.full_size);
        }
    }

    fn truncate(&mut self, number: usize) {
        self.jellies.truncate(number);
    }

    fn strike(&mut self, hits: &mut dyn FnMut(Vec3, f32) -> bool, dead: &mut Vec<Carcass>) {
        let (species, poses) = (&self.species, self.poses);
        self.jellies.retain(|jelly| {
            let struck = hits(jelly.position, RADIUS * jelly.size);
            if struck {
                dead.push(jelly.body(species, poses, jelly.position));
            }
            !struck
        });
    }
}

fn new_jelly(species: &SpeciesDesc, rng: &mut Rng, position: Vec3) -> Jelly {
    let size = rng.range(species.size.0, species.size.1);
    Jelly {
        position,
        velocity: Vec3::ZERO,
        phase: rng.f32(),
        size,
        full_size: size,
        shade: rng.range(0.9, 1.1),
        habit: rng.range(-1.0, 1.0),
        rotation: Quat::from_rotation_y(rng.range(0.0, std::f32::consts::TAU)),
        drawn: true,
    }
}

/// Jellyfish of one species around a place.
#[derive(Debug)]
pub(super) struct Bloom {
    pub species: Arc<SpeciesDesc>,
    pub poses: usize,
    pub jellies: Vec<Jelly>,
    home: Vec3,
    radius: f32,
    /// Further than this from `home`, it's on its way back.
    leash: f32,
    rng: Rng,
    ledger: Ledger,
    time: f32,
    /// Seconds until it can sting anyone again.
    stung_in: f32,
}

impl Bloom {
    pub(super) fn new(group: &SceneFauna, poses: usize) -> Self {
        let species = group.species.clone();
        let mut rng = Rng::new(group.seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x7E11);
        let radius = group.radius;
        let jellies = (0..group.count)
            .map(|_| {
                let angle = rng.range(0.0, std::f32::consts::TAU);
                let far = radius * rng.f32().sqrt();
                let at = group.position + Vec3::new(angle.cos(), 0.0, angle.sin()) * far;
                new_jelly(&species, &mut rng, at)
            })
            .collect();
        Self {
            species,
            poses,
            jellies,
            home: group.position,
            radius,
            ledger: Ledger::of(group),
            leash: group.radius * LEASH,
            time: rng.range(0.0, 1000.0),
            stung_in: 0.0,
            rng,
        }
    }

    /// Puts them at the depth they want, out of whatever `position` left them in.
    pub(super) fn place(&mut self, place: &Place) {
        for jelly in &mut self.jellies {
            jelly.settle(&self.species, place);
        }
    }

    /// Everyone moves, and whoever ends up on the sand goes in `carcasses`.
    pub(super) fn update(
        &mut self,
        place: &Place,
        focus: Vec3,
        dt: f32,
        carcasses: &mut Vec<Carcass>,
        wake: &mut Wake,
    ) {
        self.time += dt;
        let species = self.species.clone();
        let drift = &species.drift;
        let wanted_depth = drift.night_depth + (drift.day_depth - drift.night_depth) * place.daylight;
        let wind = Vec3::new(place.waters.wind.x, 0.0, place.waters.wind.y).normalize_or(Vec3::X);
        let time = self.time;
        let poses = self.poses;
        let (home, leash) = (self.home, self.leash);
        let origin = place.waters.origin.world();
        self.jellies.retain_mut(|jelly| {
            let position = jelly.position;
            let (level, flow) = place.waters.calm_at(position.x, position.z).unwrap_or((position.y, Vec3::ZERO));
            let floor = place.floor(position.x, position.z);
            let water = level - floor;
            if water < drift.stranded {
                carcasses.push(jelly.body(&species, poses, Vec3::new(position.x, floor + 0.03, position.z)));
                return false;
            }

            // Beating faster if it's fallen too deep, and hardly at all if it's come up too far.
            let target = level - wanted_depth.min(water - 0.5) - jelly.habit;
            let urge = ((position.y - target) * -0.6).clamp(-0.7, 1.5);
            let beat = drift.beat * (1.0 + urge) * (1.0 + jelly.habit * 0.15);
            jelly.phase = (jelly.phase + beat * dt).fract();
            let up = jelly.rotation * Vec3::Y;
            if jelly.phase < CLOSING {
                // The whole of `thrust`, over one beat's closing: more beats, more thrust.
                jelly.velocity += up * drift.thrust * beat * dt / CLOSING;
            }
            jelly.velocity.y -= drift.sink * drift.drag * dt;
            jelly.velocity *= 1.0 - (drift.drag * dt).min(1.0);

            // The current: slow, wide and always changing.
            // Drawn from the world position, in `f64`, so it is the same wherever the origin is.
            let world = origin + position.as_dvec3();
            let (a, b) = (world.x / f64::from(CURRENT_SIZE), world.z / f64::from(CURRENT_SIZE));
            let t = f64::from(time / CURRENT_SECONDS);
            let turn = |v: f64| v.rem_euclid(std::f64::consts::TAU) as f32;
            let current = Vec3::new(
                turn(b + t).sin() + turn(a * 0.6 - t * 0.7).cos() * 0.5,
                0.0,
                turn(a - t * 1.3).sin() + turn(b * 0.7).cos() * 0.5,
            );
            let mut carried = current * drift.current * 0.7 + flow;
            // No open sea to lose them in: whatever carried them off, another current brings them back.
            let from_home = Vec3::new(home.x - position.x, 0.0, home.z - position.z);
            if from_home.length() > leash {
                carried += from_home.normalize_or(Vec3::ZERO) * drift.current * 3.0;
            } else if place.storm {
                carried += wind * drift.storm_push;
            } else if water < drift.shallowest {
                // Away from the shore, with the little it can do.
                let slope = Vec3::new(
                    place.floor(position.x - 3.0, position.z) - place.floor(position.x + 3.0, position.z),
                    0.0,
                    place.floor(position.x, position.z - 3.0) - place.floor(position.x, position.z + 3.0),
                );
                carried += slope.normalize_or(Vec3::ZERO) * drift.current * 2.0;
            }
            jelly.position += (jelly.velocity + carried) * dt;
            jelly.position.y = jelly.position.y.min(level - 0.15).max(floor + 0.1);

            // Tilted by the water it's being carried in, and turning slowly.
            let tilt = Quat::from_rotation_arc(Vec3::Y, (Vec3::Y + carried * MOST_TILT * 2.0).normalize());
            let spin = Quat::from_rotation_y(time * 0.1 + jelly.habit * 40.0);
            jelly.rotation = jelly.rotation.slerp(tilt * spin, (dt * 2.0).min(1.0));
            jelly.drawn = jelly.position.distance(focus) < DRAW_DISTANCE;
            true
        });

        // It doesn't mean anything by it: it's what its tentacles do to whatever they touch.
        self.stung_in -= dt;
        if self.stung_in <= 0.0
            && let (Some(player), Some(harm)) = (wake.player, &species.harm)
            && let Some(jelly) = self.jellies.iter().find(|jelly| player.touches(jelly.position, harm.reach))
        {
            self.stung_in = harm.every;
            wake.harm(&species, jelly.position);
        }
    }
}

/// How much it glows at this hour, and this moment of its beat.
pub(super) fn glow(glow: f32, daylight: f32, phase: f32) -> f32 {
    let beat = 0.65 + 0.35 * (phase * std::f32::consts::TAU).cos();
    glow * (1.0 - daylight).clamp(0.0, 1.0) * beat
}
