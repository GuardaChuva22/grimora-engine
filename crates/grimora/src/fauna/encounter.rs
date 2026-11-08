//! Where the animals meet everyone else: what a blow does to one, what one does to whoever
//! comes too close, and the bodies and the noise that leaves behind.

use std::f32::consts::TAU;
use std::path::{Path, PathBuf};

use glam::{Mat4, Quat, Vec3};
use grimora_audio::Emitter;
use grimora_core::ParticleEffect;
use grimora_scene::SpeciesDesc;

use super::{Place, Threat, flat};
use crate::world::MeshId;

/// A body shrinks away over these last seconds, not all at once.
const LAST_SECONDS: f32 = 4.0;
/// How long a body lasts, and how many seconds of eating are in it.
const ROTS_IN: f32 = 120.0;
const FOOD: f32 = 20.0;
/// How fast a body falls through the air, and moves through water, up or down.
const GRAVITY: f32 = 9.0;
const IN_WATER: f32 = 0.5;
/// How deep what floats lies in the water.
const FLOATS_AT: f32 = 0.04;
/// How much of the people around it each death frightens.
const DEATH_SCARES: f32 = 6.0;
/// However wide a body is, this is added to what an animal can reach.
const BODY_RADIUS: f32 = 0.35;

/// Something an animal, or the end of one, did for the game to know about.
#[derive(Debug, Clone, PartialEq)]
pub enum FaunaEvent {
    /// It hurt whoever [`World::touch_fauna`](crate::World::touch_fauna) said was there.
    Hurt {
        species: String,
        damage: f32,
        /// Enough to make someone flinch, or only something that burns.
        staggers: bool,
        at: Vec3,
        /// For whoever it happened to.
        message: Option<String>,
    },
    Killed {
        species: String,
        at: Vec3,
    },
}

/// A noise an animal made: the engine plays these.
#[derive(Debug, Clone, PartialEq)]
pub struct FaunaSound {
    pub file: PathBuf,
    pub volume: f32,
    pub pitch: f32,
    pub emitter: Emitter,
}

/// The one body animals can do anything to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct PlayerBody {
    pub feet: Vec3,
    pub height: f32,
}

impl PlayerBody {
    /// Whether an animal here can get at it.
    pub fn touches(&self, at: Vec3, reach: f32) -> bool {
        flat(at - self.feet).length() <= reach + BODY_RADIUS
            && at.y >= self.feet.y - reach
            && at.y <= self.feet.y + self.height + reach
    }
}

/// What a frame of animals leaves behind for the rest of the world.
#[derive(Debug, Default)]
pub(super) struct Wake {
    pub threats: Vec<Threat>,
    pub splashes: Vec<(Vec3, f32)>,
    pub puffs: Vec<(ParticleEffect, Vec3)>,
    pub sounds: Vec<FaunaSound>,
    pub events: Vec<FaunaEvent>,
    /// Who there is to bite, this frame.
    pub player: Option<PlayerBody>,
    /// Where it's heard from.
    focus: Vec3,
}

impl Wake {
    pub fn new(focus: Vec3, player: Option<PlayerBody>) -> Self {
        Self { focus, player, ..Self::default() }
    }

    /// A noise from a species, each one a little different, if anyone is near enough to hear it.
    pub fn play(&mut self, species: &SpeciesDesc, file: Option<&Path>, emitter: Emitter) {
        let Some(file) = file else { return };
        if emitter.position.distance(self.focus) > emitter.max_distance {
            return;
        }
        let at = emitter.position;
        let chance = ((at.x * 91.7 + at.y * 53.3 + at.z * 37.1).sin() * 437.5).fract().abs();
        self.sounds.push(FaunaSound {
            file: file.to_path_buf(),
            volume: species.sounds.volume,
            pitch: 0.9 + chance * 0.2,
            emitter,
        });
    }

    /// It went in or out of the sand.
    pub fn dig(&mut self, species: &SpeciesDesc, puff: Option<&ParticleEffect>, at: Vec3) {
        if let Some(puff) = puff {
            self.puffs.push((puff.clone(), at));
        }
        self.play(species, species.sounds.dig.as_deref(), Emitter::quiet(at));
    }

    /// It got whoever was there.
    pub fn harm(&mut self, species: &SpeciesDesc, at: Vec3) {
        let Some(harm) = &species.harm else { return };
        self.events.push(FaunaEvent::Hurt {
            species: species.name.clone(),
            damage: harm.damage,
            staggers: harm.staggers,
            at,
            message: harm.message.clone(),
        });
        self.play(species, species.sounds.harm.as_deref(), Emitter::at(at));
    }

    /// One less, and everyone near saw it.
    pub fn death(&mut self, species: &SpeciesDesc, at: Vec3) {
        self.events.push(FaunaEvent::Killed { species: species.name.clone(), at });
        self.threats.push(Threat { position: at, radius: DEATH_SCARES });
        self.play(species, species.sounds.death.as_deref(), Emitter::at(at));
    }
}

/// What a body looks like: it stays as it was at the end.
#[derive(Debug, Clone)]
pub(super) enum BodyPose {
    /// From a species' table of poses.
    Table {
        poses: usize,
        pose: usize,
    },
    Own {
        mesh: MeshId,
        matrices: Vec<Mat4>,
    },
}

/// Something dead, until it's eaten or there's nothing left.
#[derive(Debug, Clone)]
pub(super) struct Carcass {
    /// Where its model has its origin.
    pub position: Vec3,
    pub rotation: Quat,
    pub size: f32,
    /// How much of `size` goes each way: nothing holds it up any more.
    pub shape: Vec3,
    pub shade: f32,
    pub pose: BodyPose,
    /// Seconds left, whether it's eaten or not.
    pub left: f32,
    /// Seconds of eating left in it.
    pub food: f32,
    /// It stays on top of the water.
    pub floats: bool,
    /// How high its origin stays above the ground.
    pub lift: f32,
    pub speed: f32,
    pub resting: bool,
}

impl Carcass {
    pub fn new(position: Vec3, rotation: Quat, size: f32, shade: f32, pose: BodyPose) -> Self {
        Self {
            position,
            rotation,
            size,
            shape: Vec3::ONE,
            shade,
            pose,
            left: ROTS_IN,
            food: FOOD,
            floats: false,
            lift: 0.0,
            speed: 0.0,
            resting: false,
        }
    }

    /// Falls, sinks or comes up, and rots. False once there's nothing left.
    pub fn update(&mut self, place: &Place, dt: f32) -> bool {
        self.left -= dt;
        let (x, z) = (self.position.x, self.position.z);
        let floor = place.floor(x, z);
        let level = place.waters.calm_at(x, z).map(|(level, _)| level).filter(|level| *level > floor);
        let rest = match level {
            Some(level) if self.floats => level - FLOATS_AT,
            _ if floor.is_finite() => floor + self.lift,
            _ => self.position.y,
        };
        let in_water = level.is_some_and(|level| self.position.y < level);
        self.speed = if in_water { IN_WATER } else { self.speed + GRAVITY * dt };
        let step = self.speed * dt;
        self.position.y += (rest - self.position.y).clamp(-step, step);
        self.resting = (rest - self.position.y).abs() < 1.0e-3;
        self.left > 0.0 && self.food > 0.0
    }

    /// Whether anything on the ground can get at it.
    pub fn eatable(&self) -> bool {
        self.food > 0.0 && self.resting && !self.floats
    }

    /// Its scale now: it shrinks away at the end, and as it's eaten.
    pub fn scale(&self) -> Vec3 {
        self.shape * self.size * (self.left.min(self.food) / LAST_SECONDS).clamp(0.0, 1.0)
    }
}

/// The same way round, flat on the ground.
pub(super) fn laid_flat(rotation: Quat) -> Quat {
    let forward = flat(rotation * Vec3::Z).normalize_or(Vec3::Z);
    Quat::from_rotation_y(forward.x.atan2(forward.z).rem_euclid(TAU))
}
