//! Particle effects, CPU only: emitters described as data ([`ParticleDesc`]),
//! simulated in world space by a [`ParticleSystem`], handed to the renderer as
//! [`ParticleInstance`]s.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::{Quat, Vec3, Vec4};
use serde::{Deserialize, Serialize};

use crate::{Rng, Transform};

/// One emitter of an effect, as written in effect files. Colors are sRGB 0..1,
/// angles degrees, distances meters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ParticleDesc {
    /// Particles per second. 0 with a `burst` makes a one-shot.
    pub rate: f32,
    /// Particles released at once when the emitter starts.
    pub burst: u32,
    /// Stop emitting after this many seconds; `None` keeps going forever.
    pub duration: Option<f32>,
    /// Most particles alive at a time; the emitter waits when it hits this.
    pub max_particles: u32,
    /// Seconds each particle lives, `(min, max)`.
    pub lifetime: (f32, f32),
    /// Where this emitter sits, from the effect's position.
    pub position: Vec3,
    /// Half sizes of the box particles appear in.
    pub area: Vec3,
    /// Direction particles leave in, turned with the entity.
    pub direction: Vec3,
    /// Half angle of the cone around `direction`, up to 180.
    pub spread: f32,
    /// Meters per second at birth, `(min, max)`.
    pub speed: (f32, f32),
    /// World-space acceleration: down for gravity, up for rising heat.
    pub acceleration: Vec3,
    /// Slows particles down: the share of speed lost per second, roughly.
    pub drag: f32,
    /// Strength of a random wander, in m/s².
    pub turbulence: f32,
    /// Diameter over life: keys evenly spaced from birth to death.
    pub sizes: Vec<f32>,
    /// Random variation of each particle's size, `0..1`.
    pub size_jitter: f32,
    /// sRGB color and opacity over life, keys evenly spaced like `sizes`.
    pub colors: Vec<Vec4>,
    /// Glow, multiplying the color. Above 1 it feeds the bloom.
    pub emissive: f32,
    /// How much the scene's light falls on it, `0..1`. Fire 0, smoke 1.
    pub lit: f32,
    /// 0 covers what is behind (smoke), 1 adds light to it (fire), or anything between.
    pub additive: f32,
    /// Random spin, in degrees per second either way.
    pub spin: f32,
    /// Stretches particles along their motion by this many seconds of travel (sparks).
    pub stretch: f32,
    pub shape: ParticleShape,
    /// Beyond this distance from the camera the emitter sleeps and holds no particles.
    pub max_distance: f32,
    /// Mixed with the entity's position; equal emitters don't move in step.
    pub seed: u64,
}

impl Default for ParticleDesc {
    fn default() -> Self {
        Self {
            rate: 10.0,
            burst: 0,
            duration: None,
            max_particles: 256,
            lifetime: (1.0, 1.0),
            position: Vec3::ZERO,
            area: Vec3::ZERO,
            direction: Vec3::Y,
            spread: 180.0,
            speed: (0.0, 0.0),
            acceleration: Vec3::ZERO,
            drag: 0.0,
            turbulence: 0.0,
            sizes: vec![0.2],
            size_jitter: 0.0,
            colors: vec![Vec4::ONE],
            emissive: 0.0,
            lit: 0.0,
            additive: 0.0,
            spin: 0.0,
            stretch: 0.0,
            shape: ParticleShape::Soft,
            max_distance: 60.0,
            seed: 0,
        }
    }
}

/// What a particle looks like; no textures, so the look stays low poly and cheap.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParticleShape {
    /// A blob that fades towards its edge: fire, smoke, mist.
    #[default]
    Soft,
    /// A hard-edged rhombus: sparks, embers, crystal shards.
    Diamond,
}

impl ParticleDesc {
    /// Keeps emitting for as long as it lives.
    pub fn looping(&self) -> bool {
        self.rate > 0.0 && self.duration.is_none()
    }

    fn longest_life(&self) -> f32 {
        self.lifetime.0.max(self.lifetime.1).max(MIN_LIFE)
    }
}

/// A set of emitters that play together, loaded once and shared.
pub type ParticleEffect = Arc<[ParticleDesc]>;

/// A particle ready to draw. Mirrors `ParticleIn` in scene.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct ParticleInstance {
    pub position: [f32; 3],
    /// Diameter.
    pub size: f32,
    /// sRGB color and opacity.
    pub color: [f32; 4],
    /// Length and direction to stretch along; zero for a round particle.
    pub stretch: [f32; 3],
    /// Radians, on screen.
    pub rotation: f32,
    pub emissive: f32,
    pub lit: f32,
    pub additive: f32,
    /// 0 [`ParticleShape::Soft`], 1 [`ParticleShape::Diamond`].
    pub shape: f32,
}

const MIN_LIFE: f32 = 0.01;
/// Longest step simulated at once; a hitch must not teleport every particle.
const MAX_STEP: f32 = 0.1;
/// Step of the catch-up that makes a looping effect already burning when it starts.
const PREWARM_STEP: f32 = 1.0 / 20.0;

#[derive(Debug, Clone)]
struct Particle {
    position: Vec3,
    velocity: Vec3,
    age: f32,
    life: f32,
    size: f32,
    rotation: f32,
    spin: f32,
    /// Offsets its wander from the other particles'.
    phase: f32,
}

#[derive(Debug, Clone)]
struct Emitter {
    particles: Vec<Particle>,
    rng: Rng,
    /// Particles owed by `rate` that didn't add up to a whole one yet.
    owed: f32,
    elapsed: f32,
    started: bool,
    last_origin: Vec3,
}

/// The running effect of an entity, which it follows through its
/// [`Transform`] (position and rotation, not scale). Particles stay where they
/// were released, so a moving entity leaves a trail. The engine updates and
/// draws these; see `World::play_effect` for effects that play once.
#[derive(Debug, Clone)]
pub struct ParticleSystem {
    effect: ParticleEffect,
    emitters: Vec<Emitter>,
    /// Offset from the entity, turned with it.
    offset: Vec3,
    /// Multiplies sizes, speeds and distances.
    scale: f32,
    /// False stops new particles; those alive finish their lives.
    pub emitting: bool,
    /// Remove the entity once nothing is left (only for effects that end).
    pub despawn_when_done: bool,
}

impl ParticleSystem {
    pub fn new(effect: ParticleEffect, offset: Vec3, scale: f32, seed: u64) -> Self {
        let emitters = effect
            .iter()
            .enumerate()
            .map(|(i, desc)| Emitter {
                particles: Vec::new(),
                rng: Rng::new(seed.wrapping_add(desc.seed).wrapping_add(i as u64)),
                owed: 0.0,
                elapsed: 0.0,
                started: false,
                last_origin: Vec3::ZERO,
            })
            .collect();
        Self { effect, emitters, offset, scale: scale.max(0.0), emitting: true, despawn_when_done: false }
    }

    /// Advances the simulation. `allowed` is false for entities that are hidden,
    /// which emit nothing. Effects that loop start out already burning.
    pub fn update(&mut self, transform: &Transform, dt: f32, camera: Vec3, allowed: bool) {
        let dt = dt.min(MAX_STEP);
        let emitting = self.emitting && allowed;
        let base = transform.position + transform.rotation * self.offset;
        for (desc, emitter) in self.effect.iter().zip(&mut self.emitters) {
            let origin = base + transform.rotation * (desc.position * self.scale);
            let step = Step { desc, scale: self.scale, rotation: transform.rotation, origin, emitting };
            if origin.distance(camera) > desc.max_distance {
                emitter.particles.clear();
                emitter.owed = 0.0;
                // A looping effect starts over when the camera returns; one that ends is just over.
                emitter.started = !desc.looping();
                continue;
            }
            if !emitter.started {
                emitter.start(&step);
            }
            emitter.step(&step, dt);
        }
    }

    /// True once everything has been emitted and has died. Never, while looping.
    pub fn finished(&self) -> bool {
        self.effect.iter().zip(&self.emitters).all(|(desc, emitter)| {
            emitter.started
                && emitter.particles.is_empty()
                && !desc.looping()
                && desc.duration.is_none_or(|duration| desc.rate <= 0.0 || emitter.elapsed >= duration)
        })
    }

    /// How many particles are alive.
    pub fn particle_count(&self) -> usize {
        self.emitters.iter().map(|e| e.particles.len()).sum()
    }

    /// Adds every live particle to `out`, as it looks now.
    pub fn collect(&self, out: &mut Vec<ParticleInstance>) {
        for (desc, emitter) in self.effect.iter().zip(&self.emitters) {
            let shape = match desc.shape {
                ParticleShape::Soft => 0.0,
                ParticleShape::Diamond => 1.0,
            };
            out.extend(emitter.particles.iter().map(|p| {
                let t = p.age / p.life;
                let color = sample(&desc.colors, t, Vec4::lerp, Vec4::ONE);
                ParticleInstance {
                    position: p.position.into(),
                    size: p.size * sample(&desc.sizes, t, |a, b, t| a + (b - a) * t, 0.2) * self.scale,
                    color: color.into(),
                    stretch: (p.velocity * desc.stretch).into(),
                    rotation: p.rotation,
                    emissive: desc.emissive,
                    lit: desc.lit,
                    additive: desc.additive,
                    shape,
                }
            }));
        }
    }
}

/// The value at `t` (0..1) of `keys` evenly spaced over 0..1.
fn sample<T: Copy>(keys: &[T], t: f32, lerp: impl Fn(T, T, f32) -> T, empty: T) -> T {
    let Some(last) = keys.len().checked_sub(1) else { return empty };
    let at = t.clamp(0.0, 1.0) * last as f32;
    let i = (at as usize).min(last.saturating_sub(1));
    match keys.get(i + 1) {
        Some(&next) => lerp(keys[i], next, at - i as f32),
        None => keys[i],
    }
}

/// What one emitter needs to know this frame.
struct Step<'a> {
    desc: &'a ParticleDesc,
    scale: f32,
    rotation: Quat,
    origin: Vec3,
    emitting: bool,
}

impl Emitter {
    fn start(&mut self, step: &Step<'_>) {
        self.started = true;
        self.elapsed = 0.0;
        self.owed = 0.0;
        self.last_origin = step.origin;
        if !step.emitting {
            return;
        }
        for _ in 0..step.desc.burst {
            self.spawn(step, step.origin);
        }
        if step.desc.looping() {
            for _ in 0..(step.desc.longest_life() / PREWARM_STEP).ceil() as u32 {
                self.step(step, PREWARM_STEP);
            }
        }
    }

    fn step(&mut self, step: &Step<'_>, dt: f32) {
        let desc = step.desc;
        let wander = desc.turbulence * step.scale * dt;
        let friction = (-desc.drag * dt).exp();
        let acceleration = desc.acceleration * step.scale * dt;
        self.particles.retain_mut(|p| {
            p.age += dt;
            if p.age >= p.life {
                return false;
            }
            let (a, phase) = (p.age, p.phase);
            let drift =
                Vec3::new((a * 4.1 + phase).sin(), (a * 3.3 + phase * 2.0).sin(), (a * 5.3 + phase * 3.0).cos());
            p.velocity = (p.velocity + acceleration + drift * wander) * friction;
            p.position += p.velocity * dt;
            p.rotation += p.spin * dt;
            true
        });

        self.elapsed += dt;
        if !step.emitting || desc.duration.is_some_and(|duration| self.elapsed > duration) {
            self.owed = 0.0;
        } else {
            self.owed += desc.rate.max(0.0) * dt;
            // Spread along the way the emitter moved, so a fast one doesn't leave clumps.
            let whole = self.owed.floor();
            for i in 0..whole as u32 {
                let along = (i as f32 + 1.0) / whole;
                self.spawn(step, self.last_origin.lerp(step.origin, along));
            }
            self.owed -= whole;
        }
        self.last_origin = step.origin;
    }

    fn spawn(&mut self, step: &Step<'_>, origin: Vec3) {
        let desc = step.desc;
        if self.particles.len() >= desc.max_particles as usize {
            return;
        }
        let rng = &mut self.rng;
        let area = desc.area * step.scale;
        let box_offset = Vec3::new(rng.jitter(area.x), rng.jitter(area.y), rng.jitter(area.z));
        let direction = cone_direction(rng, step.rotation * desc.direction.normalize_or(Vec3::Y), desc.spread);
        let spin = rng.jitter(desc.spin).to_radians();
        let rotation = rng.range(0.0, std::f32::consts::TAU);
        self.particles.push(Particle {
            position: origin + step.rotation * box_offset,
            velocity: direction * rng.range(desc.speed.0, desc.speed.1) * step.scale,
            age: 0.0,
            life: rng.range(desc.lifetime.0, desc.lifetime.1).max(MIN_LIFE),
            size: 1.0 + rng.jitter(desc.size_jitter.clamp(0.0, 1.0)),
            rotation,
            spin,
            phase: rng.range(0.0, std::f32::consts::TAU),
        });
    }
}

/// A random direction within `spread` degrees of `axis` (a unit vector),
/// uniform over that part of the sphere.
fn cone_direction(rng: &mut Rng, axis: Vec3, spread: f32) -> Vec3 {
    let cos_spread = spread.clamp(0.0, 180.0).to_radians().cos();
    let cos = rng.range(cos_spread, 1.0);
    let sin = (1.0 - cos * cos).max(0.0).sqrt();
    let (s, c) = rng.range(0.0, std::f32::consts::TAU).sin_cos();
    let (a, b) = axis.any_orthonormal_pair();
    axis * cos + (a * c + b * s) * sin
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;
    const NEAR: Vec3 = Vec3::ZERO;

    fn system(desc: ParticleDesc) -> ParticleSystem {
        ParticleSystem::new(Arc::from([desc]), Vec3::ZERO, 1.0, 7)
    }

    fn run(system: &mut ParticleSystem, at: &Transform, seconds: f32) {
        for _ in 0..(seconds / DT).round() as u32 {
            system.update(at, DT, NEAR, true);
        }
    }

    fn instances(system: &ParticleSystem) -> Vec<ParticleInstance> {
        let mut out = Vec::new();
        system.collect(&mut out);
        out
    }

    #[test]
    fn looping_effects_start_already_burning_and_hold_steady() {
        let mut fire = system(ParticleDesc { rate: 60.0, lifetime: (1.0, 1.0), ..Default::default() });
        fire.update(&Transform::IDENTITY, DT, NEAR, true);
        assert!((55..=65).contains(&fire.particle_count()), "{}", fire.particle_count());
        run(&mut fire, &Transform::IDENTITY, 5.0);
        assert!((55..=65).contains(&fire.particle_count()), "{}", fire.particle_count());
        assert!(!fire.finished(), "it never ends");
    }

    #[test]
    fn bursts_play_once_and_finish() {
        let mut burst = system(ParticleDesc { rate: 0.0, burst: 12, lifetime: (0.5, 0.5), ..Default::default() });
        assert!(!burst.finished(), "hasn't started");
        burst.update(&Transform::IDENTITY, DT, NEAR, true);
        assert_eq!(burst.particle_count(), 12);
        assert!(!burst.finished());
        run(&mut burst, &Transform::IDENTITY, 0.6);
        assert_eq!(burst.particle_count(), 0);
        assert!(burst.finished());
    }

    #[test]
    fn durations_stop_the_emission() {
        let desc = ParticleDesc { rate: 100.0, duration: Some(0.5), lifetime: (0.2, 0.2), ..Default::default() };
        let mut puff = system(desc);
        run(&mut puff, &Transform::IDENTITY, 0.4);
        assert!(puff.particle_count() > 0 && !puff.finished());
        run(&mut puff, &Transform::IDENTITY, 0.5);
        assert_eq!(puff.particle_count(), 0);
        assert!(puff.finished());
    }

    #[test]
    fn emitting_and_hidden_entities_stop_new_particles_only() {
        let mut fire = system(ParticleDesc { rate: 60.0, lifetime: (1.0, 1.0), ..Default::default() });
        run(&mut fire, &Transform::IDENTITY, 0.5);
        let before = fire.particle_count();
        fire.emitting = false;
        fire.update(&Transform::IDENTITY, DT, NEAR, true);
        assert!(fire.particle_count() <= before && fire.particle_count() > 0, "the live ones stay");
        run(&mut fire, &Transform::IDENTITY, 1.1);
        assert_eq!(fire.particle_count(), 0);
        assert!(!fire.finished(), "switched off, not over");

        fire.emitting = true;
        fire.update(&Transform::IDENTITY, DT, NEAR, false);
        assert_eq!(fire.particle_count(), 0, "hidden");
        fire.update(&Transform::IDENTITY, DT, NEAR, true);
        assert!(fire.particle_count() <= 2, "comes back gradually");
    }

    #[test]
    fn far_emitters_sleep_and_wake_up_burning() {
        let desc = ParticleDesc { rate: 60.0, lifetime: (1.0, 1.0), max_distance: 30.0, ..Default::default() };
        let mut fire = system(desc);
        let far = Vec3::new(31.0, 0.0, 0.0);
        fire.update(&Transform::IDENTITY, DT, far, true);
        assert_eq!(fire.particle_count(), 0);
        fire.update(&Transform::IDENTITY, DT, NEAR, true);
        assert!(fire.particle_count() > 50);
        fire.update(&Transform::IDENTITY, DT, far, true);
        assert_eq!(fire.particle_count(), 0);
        fire.update(&Transform::IDENTITY, DT, NEAR, true);
        assert!(fire.particle_count() > 50, "prewarmed again");
    }

    #[test]
    fn a_far_one_shot_is_simply_over() {
        let desc = ParticleDesc { rate: 0.0, burst: 5, max_distance: 10.0, ..Default::default() };
        let mut burst = system(desc);
        burst.update(&Transform::from_position(Vec3::X * 20.0), DT, NEAR, true);
        assert!(burst.finished());
    }

    #[test]
    fn particles_stay_in_the_world_as_the_entity_moves() {
        let desc = ParticleDesc { rate: 60.0, lifetime: (10.0, 10.0), duration: Some(0.1), ..Default::default() };
        let mut trail = system(desc);
        run(&mut trail, &Transform::IDENTITY, 0.1);
        let left_behind = trail.particle_count();
        assert!(left_behind > 0);
        run(&mut trail, &Transform::from_position(Vec3::X * 50.0), 0.5);
        assert_eq!(trail.particle_count(), left_behind);
        assert!(instances(&trail).iter().all(|p| p.position[0].abs() < 1e-3), "they didn't follow");
    }

    #[test]
    fn a_moving_emitter_leaves_an_even_trail() {
        let desc = ParticleDesc { rate: 600.0, lifetime: (10.0, 10.0), duration: Some(1.0), ..Default::default() };
        let mut trail = system(desc);
        trail.update(&Transform::IDENTITY, DT, NEAR, true);
        // One big step, 10 m at once: particles land all along the way.
        trail.update(&Transform::from_position(Vec3::X * 10.0), 0.1, NEAR, true);
        let xs: Vec<f32> = instances(&trail).iter().map(|p| p.position[0]).collect();
        assert!(xs.iter().any(|x| (4.0..6.0).contains(x)), "{xs:?}");
    }

    #[test]
    fn hitches_do_not_teleport_particles() {
        let desc =
            ParticleDesc { rate: 0.0, burst: 1, lifetime: (100.0, 100.0), speed: (1.0, 1.0), ..Default::default() };
        let mut one = system(desc);
        one.update(&Transform::IDENTITY, 5.0, NEAR, true);
        let p = instances(&one)[0];
        assert!(Vec3::from(p.position).length() <= MAX_STEP * 1.01, "{:?}", p.position);
    }

    #[test]
    fn the_cap_holds() {
        let desc = ParticleDesc { rate: 1000.0, lifetime: (10.0, 10.0), max_particles: 50, ..Default::default() };
        let mut capped = system(desc);
        run(&mut capped, &Transform::IDENTITY, 1.0);
        assert_eq!(capped.particle_count(), 50);
    }

    #[test]
    fn forces_move_particles() {
        let fall = ParticleDesc {
            rate: 0.0,
            burst: 1,
            lifetime: (10.0, 10.0),
            acceleration: Vec3::NEG_Y * 10.0,
            ..Default::default()
        };
        let mut stone = system(fall.clone());
        run(&mut stone, &Transform::IDENTITY, 1.0);
        let y = instances(&stone)[0].position[1];
        assert!((-5.5..-4.5).contains(&y), "fell {y}");

        // Drag keeps it from falling as far.
        let mut feather = system(ParticleDesc { drag: 3.0, ..fall });
        run(&mut feather, &Transform::IDENTITY, 1.0);
        let slower = instances(&feather)[0].position[1];
        assert!(slower > y + 1.0, "{slower} vs {y}");
    }

    #[test]
    fn direction_turns_with_the_entity_and_spread_limits_it() {
        let desc = ParticleDesc {
            rate: 0.0,
            burst: 64,
            lifetime: (10.0, 10.0),
            speed: (1.0, 1.0),
            spread: 10.0,
            direction: Vec3::Y,
            stretch: 1.0,
            ..Default::default()
        };
        let mut jet = system(desc);
        let turned = Transform { rotation: Quat::from_rotation_z(-std::f32::consts::FRAC_PI_2), ..Transform::IDENTITY };
        jet.update(&turned, DT, NEAR, true);
        for p in instances(&jet) {
            let along = Vec3::from(p.stretch);
            assert!((along.length() - 1.0).abs() < 1e-4);
            assert!(along.x >= 10f32.to_radians().cos() - 1e-4, "points along +X: {along}");
        }
    }

    #[test]
    fn scale_multiplies_sizes_speeds_and_offsets() {
        let desc = ParticleDesc {
            rate: 0.0,
            burst: 1,
            lifetime: (10.0, 10.0),
            position: Vec3::Y,
            sizes: vec![0.5],
            ..Default::default()
        };
        let mut big = ParticleSystem::new(Arc::from([desc]), Vec3::ZERO, 4.0, 1);
        big.update(&Transform::IDENTITY, DT, NEAR, true);
        let p = instances(&big)[0];
        assert_eq!(p.size, 2.0);
        assert_eq!(p.position, [0.0, 4.0, 0.0]);
    }

    #[test]
    fn keys_are_evenly_spaced_over_life() {
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let keys = [0.0, 1.0, 3.0];
        assert_eq!(sample(&keys, 0.0, lerp, 9.0), 0.0);
        assert_eq!(sample(&keys, 0.25, lerp, 9.0), 0.5);
        assert_eq!(sample(&keys, 0.75, lerp, 9.0), 2.0);
        assert_eq!(sample(&keys, 1.0, lerp, 9.0), 3.0);
        assert_eq!(sample(&keys, 7.0, lerp, 9.0), 3.0);
        assert_eq!(sample(&[4.0], 0.5, lerp, 9.0), 4.0);
        assert_eq!(sample(&[], 0.5, lerp, 9.0), 9.0);
    }

    #[test]
    fn the_same_seed_gives_the_same_particles() {
        let desc = ParticleDesc { rate: 30.0, speed: (1.0, 2.0), turbulence: 1.0, ..Default::default() };
        let (mut a, mut b) = (system(desc.clone()), system(desc.clone()));
        run(&mut a, &Transform::IDENTITY, 1.0);
        run(&mut b, &Transform::IDENTITY, 1.0);
        assert_eq!(instances(&a), instances(&b));
        let mut other = ParticleSystem::new(Arc::from([desc]), Vec3::ZERO, 1.0, 8);
        run(&mut other, &Transform::IDENTITY, 1.0);
        assert_ne!(instances(&a), instances(&other));
    }
}
