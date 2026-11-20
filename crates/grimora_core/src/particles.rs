//! Particle effects, CPU only: emitters described as data ([`ParticleDesc`]),
//! simulated in world space by a [`ParticleSystem`], handed to the renderer as
//! [`ParticleInstance`]s.

use std::collections::VecDeque;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::{Quat, Vec2, Vec3, Vec4};
use serde::{Deserialize, Serialize};

use crate::environment::Environment;
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
    /// Seconds to wait before starting, to put the parts of an effect in sequence.
    pub delay: f32,
    /// Releases the `burst` again every this many seconds: a heartbeat. It then never ends.
    pub repeat: f32,
    /// Lies flat on the ground instead of facing the camera: shockwaves, rune circles.
    /// `rotation` and `spin` turn it around the up axis.
    pub flat: bool,
    /// Seconds of its path each particle draws behind it, thinning and burning away towards
    /// the tail: energy, comets, the arc of a blade. It takes the particle's look.
    pub trail: f32,
    /// How many pieces that trail is made of; more is smoother around curves.
    pub trail_points: u32,
    /// How much of the emitter's own motion its particles share, `0..1`: 0 stays where it
    /// was released, 1 goes along, as a flame on a carried torch.
    pub local: f32,
    /// Strength of a swirling flow, in m/s². Unlike `turbulence`, neighbors move together,
    /// so it makes curls instead of jitter.
    pub curl: f32,
    /// Size of those curls, in meters.
    pub curl_scale: f32,
    /// Pushes around the emitter's `direction`, in m/s²: a vortex.
    pub orbit: f32,
    /// Pulls towards the emitter, in m/s². Negative pushes away.
    pub attract: f32,
    /// Stands along the world's up, this much taller than wide: 1 is twice as tall. It stands
    /// on its position instead of being centered there, so it stays rooted as it grows.
    pub upright: f32,
    /// How much each particle flickers on its own, `0..1`.
    pub twinkle: f32,
    /// sRGB colors and intensity `(r, g, b, glow)`, from the rim of the particle to its
    /// heart. They replace a flat color, and `colors` then only tints them and sets the
    /// opacity. Remember `emissive` or `lit`, without which they stay black.
    pub ramp: Vec<Vec4>,
    /// How much of the particle has burned away over life, keys evenly spaced, `0..1`. The
    /// heart goes first, then the rest eats in from the rim, along the noise.
    pub erode: Vec<f32>,
    /// How many times the noise repeats across a particle.
    pub noise: f32,
    /// How much the noise bites into the shape, `0..1`.
    pub grain: f32,
    /// How far the noise bends the shape, as a share of the particle's size.
    pub warp: f32,
    /// Particle sizes per second the noise moves by: a flame's runs downwards, so it rises.
    pub flow: Vec2,
    /// Hard steps of color from rim to heart; 0 is a smooth gradient.
    pub bands: f32,
    /// Square pixels per meter, fixed in the world as the particle grows; 0 is smooth.
    pub pixels: f32,
    /// Meters over which it fades into the scenery it touches, instead of cutting through.
    pub soft: f32,
    /// Bends what is behind it instead of drawing, as heat haze: meters of bending at one
    /// meter from the camera. Colors don't count, only the shape, `colors`' opacity and `erode`.
    pub distort: f32,
    /// Beyond this distance from the camera the emitter sleeps and holds no particles.
    pub max_distance: f32,
    /// Mixed with the entity's position; equal emitters don't move in step.
    pub seed: u64,
    /// Radius of a circle around `direction` that particles start on, on top of `area`.
    pub ring: f32,
    /// Radius of a sphere they start on the surface of.
    pub shell: f32,
    /// How much they leave straight away from the emitter's center instead of along
    /// `direction`, `0..1`: a ring that widens, a burst from a shell. Negative goes inwards.
    pub outward: f32,
    /// Multiplies `rate` over the `duration`, keys evenly spaced: a fire that swells and dies.
    pub rates: Vec<f32>,
    /// Each particle takes a color of its own somewhere between these keys, multiplying
    /// `colors`: sparks of many hues, leaves of many greens.
    pub tints: Vec<Vec4>,
    /// Swells and shrinks by this share of its size, `pulse_rate` times a second, each
    /// particle in its own time: breathing lights, fireflies.
    pub pulse: f32,
    pub pulse_rate: f32,
    /// How much of its speed a particle keeps when it hits the ground, `0..1`. 0 falls
    /// through it. A small value lands and stays: leaves, ash, debris.
    pub bounce: f32,
    /// How much the weather's wind carries it: 1 for smoke, less for sparks.
    pub windy: f32,
    /// A light it casts on the scenery, as bright as the particles alive are.
    pub light: Option<ParticleLight>,
    /// Makes this a child of the emitter with this index in the file: it releases nothing
    /// by itself, only where and `when` that one's particles say.
    pub from: Option<u32>,
    pub when: Spawn,
    /// How much of the parent particle's velocity a child starts with.
    pub inherit: f32,
    /// Share of particles, `0..1`, born on the skin of the body the system is given
    /// ([`ParticleSystem::body`]) instead of at `position`; `area` jitters them off it, and
    /// `outward` then means along the skin's normal. Auras emanate this way.
    pub body: f32,
}

/// The light of an emitter: it sits in the middle of the live particles, follows their
/// opacity, and dies away after them instead of going out in one frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ParticleLight {
    /// sRGB.
    pub color: Vec3,
    /// With every particle of the `burst` (or of a steady `rate`) at full opacity.
    pub intensity: f32,
    pub radius: f32,
    pub flicker: f32,
    /// Seconds the light takes to die once the particles are gone.
    pub linger: f32,
}

impl Default for ParticleLight {
    fn default() -> Self {
        Self { color: Vec3::ONE, intensity: 2.0, radius: 6.0, flicker: 0.0, linger: 0.12 }
    }
}

/// When a child emitter (`ParticleDesc::from`) releases particles.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Spawn {
    /// Its `burst` (at least one) where each parent particle dies: fireworks.
    #[default]
    Death,
    /// Its `burst` where each parent particle hits the ground.
    Bounce,
    /// `rate` particles a second behind each parent particle: smoke behind debris.
    Trail,
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
            delay: 0.0,
            repeat: 0.0,
            flat: false,
            trail: 0.0,
            trail_points: 12,
            local: 0.0,
            curl: 0.0,
            curl_scale: 1.0,
            orbit: 0.0,
            attract: 0.0,
            upright: 0.0,
            twinkle: 0.0,
            ramp: Vec::new(),
            erode: Vec::new(),
            noise: 1.0,
            grain: 0.0,
            warp: 0.0,
            flow: Vec2::ZERO,
            bands: 0.0,
            pixels: 0.0,
            soft: 0.0,
            distort: 0.0,
            max_distance: 60.0,
            seed: 0,
            ring: 0.0,
            shell: 0.0,
            outward: 0.0,
            rates: Vec::new(),
            tints: Vec::new(),
            pulse: 0.0,
            pulse_rate: 1.0,
            bounce: 0.0,
            windy: 0.0,
            light: None,
            from: None,
            when: Spawn::Death,
            inherit: 0.0,
            body: 0.0,
        }
    }
}

/// The form of a particle, drawn by the shader. With a `ramp` or `erode` it is a field
/// of heat, from 0 at the outline to 1 inside, rather than a mere outline.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParticleShape {
    /// A blob that fades towards its edge: fire, smoke, mist.
    #[default]
    Soft,
    /// A hard-edged rhombus: sparks, embers, crystal shards.
    Diamond,
    /// A tongue, round at the bottom and pointed at the top. Goes with `upright`.
    Flame,
    /// A circle's outline: shockwaves, runes.
    Ring,
    /// A four-pointed glint.
    Star,
    /// A circle of strokes like an unknown script, between two thin rings. Goes with `flat`.
    Runes,
    /// A shockwave: a sharp front and a long tail dying away behind it. Goes with `flat`.
    Wave,
    /// The arc a blade leaves, thick in the middle and thin at the tips, bulging up.
    Crescent,
    /// A branch of lightning that strikes again many times a second. Goes with `upright`.
    Bolt,
    /// A splinter with three sides, no two alike: debris, glass, ice.
    Shard,
    /// Pointed at both ends: leaves, petals, feathers, drops.
    Leaf,
    /// A whole fire as a volume, raymarched inside the particle's sphere: a round bed of
    /// fuel low in it, with tongues climbing off it through a rising turbulence (`noise`
    /// repeats per meter, `flow` its motion in m/s, `warp` the sway). One particle is the
    /// whole flame: keep it still, and for a fire that never ends emit one every half
    /// `lifetime`, with `colors` going 0, 1, 0, so two always add up to one.
    Volume,
}

impl ParticleShape {
    /// The number `particle_field` in scene.wgsl knows this shape by. 5 is a piece of a
    /// trail, which has no name here because the trail draws it.
    fn index(self) -> f32 {
        match self {
            Self::Soft => 0.0,
            Self::Diamond => 1.0,
            Self::Flame => 2.0,
            Self::Ring => 3.0,
            Self::Star => 4.0,
            Self::Runes => 6.0,
            Self::Wave => 7.0,
            Self::Crescent => 8.0,
            Self::Bolt => 9.0,
            Self::Shard => 10.0,
            Self::Leaf => 11.0,
            Self::Volume => 12.0,
        }
    }
}

impl ParticleDesc {
    /// Keeps emitting for as long as it lives.
    pub fn looping(&self) -> bool {
        self.from.is_none() && ((self.rate > 0.0 && self.duration.is_none()) || self.repeat > 0.0)
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
    /// Radians, on screen. For a piece of a trail, the meters from the head to it.
    pub rotation: f32,
    pub emissive: f32,
    pub lit: f32,
    pub additive: f32,
    /// See [`ParticleShape::index`], plus [`FLAT`] if it lies flat.
    pub shape: f32,
    /// x how much burned away, y pixels per meter, z meters of soft contact, w bending.
    pub look: [f32; 4],
    /// xy the noise's motion, z its repeats, w how far it bends the shape.
    pub flow: [f32; 4],
    /// x hard steps of color, y a random number, z the noise's bite, w 1 to use all of this.
    pub detail: [f32; 4],
    /// sRGB colors and glow, rim to heart.
    pub rim: [f32; 4],
    pub body: [f32; 4],
    pub heart: [f32; 4],
}

impl ParticleInstance {
    /// A flat particle, the fields of the fuller look left out.
    const PLAIN: Self = Self {
        position: [0.0; 3],
        size: 0.0,
        color: [0.0; 4],
        stretch: [0.0; 3],
        rotation: 0.0,
        emissive: 0.0,
        lit: 0.0,
        additive: 0.0,
        shape: 0.0,
        look: [0.0; 4],
        flow: [0.0; 4],
        detail: [0.0; 4],
        rim: [0.0; 4],
        body: [0.0; 4],
        heart: [0.0; 4],
    };

    /// Bends what is behind it, instead of being drawn over it.
    pub fn bends(&self) -> bool {
        self.look[3] > 0.0
    }
}

/// The most raindrops and snowflakes there can be at a time.
const RAINDROPS: usize = 2600;
const SNOWFLAKES: usize = 2200;
/// The specks that hang in the water around a camera under it.
const MOTES: usize = 1400;
/// Half the size of the box around the camera that rain and snow fall in.
const FALL_REACH: Vec3 = Vec3::new(22.0, 10.0, 22.0);
/// How far above the camera the middle of that box is.
const FALL_ABOVE: f32 = 5.0;

/// Rain and snow around the camera, as [`Environment::precipitation`] asks. Each drop
/// has a fixed place in a repeating world-space pattern that falls with the wind and
/// wraps around a box centered on the camera, so none is born or dies, and they cost the
/// same wherever the camera goes.
#[derive(Debug, Clone, Default)]
pub struct WeatherParticles {
    /// x, y, z from 0 to 1, and w a random number of its own.
    places: Vec<Vec4>,
    rain_fall: Vec3,
    snow_fall: Vec3,
    mote_fall: Vec3,
    /// Where the pattern is, as the world's origin moves: see [`WeatherParticles::shift`].
    moved: Vec3,
    clock: f32,
    instances: Vec<ParticleInstance>,
}

impl WeatherParticles {
    /// Keeps every drop where it is in the world as the origin moves and local positions
    /// change by `offset`.
    pub fn shift(&mut self, offset: Vec3) {
        self.moved = (self.moved + offset).rem_euclid(FALL_REACH * 2.0);
    }

    pub fn update(&mut self, dt: f32, camera: Vec3, env: &Environment) {
        self.instances.clear();
        let under = env.water.submerged.clamp(0.0, 1.0);
        let (rain, snow) = (env.precipitation.rain.clamp(0.0, 1.0), env.precipitation.snow.clamp(0.0, 1.0));
        // From under the water, it doesn't rain.
        let (rain, snow) = (rain * (1.0 - under), snow * (1.0 - under));
        if rain <= 0.0 && snow <= 0.0 && under <= 0.0 {
            return;
        }
        if self.places.is_empty() {
            let mut rng = Rng::new(0x7261696e);
            self.places =
                (0..RAINDROPS.max(SNOWFLAKES)).map(|_| Vec4::new(rng.f32(), rng.f32(), rng.f32(), rng.f32())).collect();
        }
        let dir = env.wind.direction.normalize_or(glam::Vec2::X);
        let wind = Vec3::new(dir.x, 0.0, dir.y) * env.wind.strength * env.wind.scale;
        let rain_velocity = wind * 1.6 + Vec3::NEG_Y * 17.0;
        let snow_velocity = wind * 0.7 + Vec3::NEG_Y * 1.3;
        self.rain_fall += rain_velocity * dt;
        self.snow_fall += snow_velocity * dt;
        self.clock += dt;

        let center = camera + Vec3::Y * FALL_ABOVE;
        let drops = (rain * RAINDROPS as f32) as usize;
        for place in &self.places[..drops] {
            let speed = 0.85 + 0.3 * place.w;
            let Some((position, fade)) =
                fall_position(*place, self.rain_fall * speed + self.moved, center, camera, 2.5)
            else {
                continue;
            };
            self.instances.push(ParticleInstance {
                position: position.to_array(),
                size: 0.018 + 0.014 * place.w,
                color: [0.72, 0.8, 0.95, (0.2 + 0.2 * place.w) * fade],
                stretch: (rain_velocity * speed * (0.016 + 0.014 * place.z)).to_array(),
                rotation: 0.0,
                emissive: 0.15,
                lit: 0.85,
                additive: 0.35,
                shape: 0.0,
                ..ParticleInstance::PLAIN
            });
        }
        let flakes = (snow * SNOWFLAKES as f32) as usize;
        for place in &self.places[..flakes] {
            let speed = 0.75 + 0.5 * place.w;
            let phase = place.w * 40.0;
            let sway = Vec3::new((self.clock * 0.7 + phase).sin(), 0.0, (self.clock * 0.53 + phase * 1.7).cos()) * 0.5;
            let Some((position, fade)) =
                fall_position(*place, self.snow_fall * speed + sway + self.moved, center, camera, 0.4)
            else {
                continue;
            };
            self.instances.push(ParticleInstance {
                position: position.to_array(),
                size: 0.09 + 0.09 * place.w,
                color: [0.92, 0.95, 1.0, 0.95 * fade],
                stretch: [0.0; 3],
                rotation: phase,
                emissive: 0.1,
                lit: 0.9,
                additive: 0.1,
                shape: 0.0,
                ..ParticleInstance::PLAIN
            });
        }
        if under <= 0.0 {
            return;
        }
        self.mote_fall += (wind * 0.04 + Vec3::NEG_Y * 0.07) * dt;
        let tint = env.water.look.shallow.lerp(Vec3::ONE, 0.5);
        for place in &self.places[..MOTES] {
            let phase = place.w * 40.0;
            let sway = Vec3::new((self.clock * 0.3 + phase).sin(), (self.clock * 0.2 + phase).cos(), 0.0) * 0.2;
            let Some((position, fade)) = fall_position(*place, self.mote_fall + sway + self.moved, camera, camera, 0.4)
            else {
                continue;
            };
            if position.y > env.water.level {
                continue;
            }
            self.instances.push(ParticleInstance {
                position: position.to_array(),
                size: 0.03 + 0.05 * place.w,
                color: [tint.x, tint.y, tint.z, 0.45 * fade * under],
                stretch: [0.0; 3],
                rotation: phase,
                emissive: 0.2,
                lit: 0.6,
                additive: 0.3,
                shape: 0.0,
                ..ParticleInstance::PLAIN
            });
        }
    }

    /// Everything falling now, for the renderer.
    pub fn collect(&self, out: &mut Vec<ParticleInstance>) {
        out.extend_from_slice(&self.instances);
    }
}

/// Raindrops landing a second at full rain, and how far from the camera they are drawn.
const SPLASHES: f32 = 520.0;
const SPLASH_REACH: f32 = 17.0;
/// Seconds a landing shows, on the ground and on water, where the ring spreads wider.
const SPLASH_LIFE: f32 = 0.32;
const RIPPLE_LIFE: f32 = 0.7;

/// One raindrop landed.
#[derive(Debug, Clone, Copy)]
struct Landing {
    position: Vec3,
    age: f32,
    on_water: bool,
}

/// Where the rain lands around the camera: a ring that spreads on the ground or the water
/// and a little crown of water jumping back up. Whoever owns it tells it where the ground is.
#[derive(Debug, Clone)]
pub struct RainSplashes {
    landings: Vec<Landing>,
    owed: f32,
    rng: Rng,
}

impl Default for RainSplashes {
    fn default() -> Self {
        Self { landings: Vec::new(), owed: 0.0, rng: Rng::new(0x73706c61) }
    }
}

impl RainSplashes {
    /// `ground` gives the height of what the rain lands on at `(x, z)`, and whether that
    /// is water; `None` where nothing lands.
    pub fn update(
        &mut self,
        dt: f32,
        camera: Vec3,
        env: &Environment,
        mut ground: impl FnMut(f32, f32) -> Option<(f32, bool)>,
    ) {
        self.landings.retain_mut(|l| {
            l.age += dt;
            l.age < if l.on_water { RIPPLE_LIFE } else { SPLASH_LIFE }
        });
        let rain = env.precipitation.rain.clamp(0.0, 1.0) * (1.0 - env.water.submerged.clamp(0.0, 1.0));
        if rain <= 0.02 {
            self.owed = 0.0;
            return;
        }
        self.owed += rain * SPLASHES * dt.min(MAX_STEP);
        while self.owed >= 1.0 {
            self.owed -= 1.0;
            // More of them close by, where they can be told apart.
            let (sin, cos) = self.rng.range(0.0, std::f32::consts::TAU).sin_cos();
            let far = self.rng.f32().powf(0.8) * SPLASH_REACH;
            let (x, z) = (camera.x + cos * far, camera.z + sin * far);
            if let Some((height, on_water)) = ground(x, z) {
                self.landings.push(Landing { position: Vec3::new(x, height, z), age: 0.0, on_water });
            }
        }
    }

    pub fn shift(&mut self, offset: Vec3) {
        for landing in &mut self.landings {
            landing.position += offset;
        }
    }

    pub fn collect(&self, out: &mut Vec<ParticleInstance>) {
        const WHITE: [f32; 4] = [1.0; 4];
        for landing in &self.landings {
            let (life, wide) = if landing.on_water { (RIPPLE_LIFE, 0.75) } else { (SPLASH_LIFE, 0.26) };
            let t = landing.age / life;
            let left = (1.0 - t) * (1.0 - t);
            out.push(ParticleInstance {
                position: (landing.position + Vec3::Y * 0.03).to_array(),
                size: 0.05 + wide * (1.0 - left),
                color: [0.8, 0.87, 1.0, 0.3 * left],
                emissive: 0.2,
                lit: 0.9,
                additive: 0.4,
                shape: ParticleShape::Ring.index() + FLAT,
                // Burned down to a thin line, thinner as it spreads.
                look: [0.55 + 0.35 * t, 0.0, 0.0, 0.0],
                detail: [0.0, 0.0, 0.0, 1.0],
                rim: WHITE,
                body: WHITE,
                heart: WHITE,
                ..ParticleInstance::PLAIN
            });
            if t < 0.6 {
                // The crown: up and back down in the first part of it.
                let jump = (t / 0.6 * std::f32::consts::PI).sin();
                out.push(ParticleInstance {
                    position: (landing.position + Vec3::Y * (0.03 + 0.07 * jump)).to_array(),
                    size: 0.03,
                    color: [0.85, 0.9, 1.0, 0.55 * (1.0 - t / 0.6)],
                    stretch: [0.0, 0.05 * jump + 0.001, 0.0],
                    emissive: 0.2,
                    lit: 0.9,
                    additive: 0.4,
                    ..ParticleInstance::PLAIN
                });
            }
        }
    }
}

/// Where a falling particle is in the box around `center`, and how much it shows: it
/// fades out at the sides, so none pop as they wrap, and within `clear` of the camera, where
/// it would look huge.
fn fall_position(place: Vec4, fall: Vec3, center: Vec3, camera: Vec3, clear: f32) -> Option<(Vec3, f32)> {
    let size = FALL_REACH * 2.0;
    let offset = (place.truncate() * size + fall - center + FALL_REACH).rem_euclid(size) - FALL_REACH;
    let edge = (offset.x.abs() / FALL_REACH.x).max(offset.z.abs() / FALL_REACH.z);
    let side_fade = ((1.0 - edge) / 0.2).clamp(0.0, 1.0);
    let position = center + offset;
    let near_fade = ((position.distance(camera) - clear) / 2.0).clamp(0.0, 1.0);
    let fade = side_fade * near_fade;
    (fade > 0.0).then_some((position, fade))
}

/// The part of `ParticleDesc::max_distance`, at its far end, over which an effect fades out.
const DISTANCE_FADE: f32 = 0.25;

/// How much of an effect shows at `distance` from the camera: all of it up to the last
/// `DISTANCE_FADE` of `max_distance`, and nothing from there on, smoothly.
fn distance_fade(distance: f32, max_distance: f32) -> f32 {
    let from = max_distance * (1.0 - DISTANCE_FADE);
    let t = ((distance - from) / (max_distance - from).max(1e-4)).clamp(0.0, 1.0);
    1.0 - t * t * (3.0 - 2.0 * t)
}

const MIN_LIFE: f32 = 0.01;
/// Added to the shape of a particle that lies flat; `vs_particle` takes it off again.
pub const FLAT: f32 = 32.0;
/// How much of its speed along the ground a particle keeps each time it touches it.
const GROUND_GRIP: f32 = 0.6;
/// Below this speed, in m/s, a bounce is a landing.
const REST_SPEED: f32 = 0.6;
/// How far above the ground a particle rests, clear of it.
const FLOOR_CLEAR: f32 = 0.02;
/// The most deaths or bounces of one emitter that its children hear of in a frame.
const MOST_EVENTS: usize = 48;
/// How far an emitter moves before the ground under it is measured again.
const FLOOR_MOVE: f32 = 1.5;
/// Meters per second of a wind of strength 1.
pub const WIND_SPEED: f32 = 2.5;
/// Longest step simulated at once; a hitch must not teleport every particle.
const MAX_STEP: f32 = 0.1;
/// Step of the catch-up that makes a looping effect already burning when it starts.
const PREWARM_STEP: f32 = 1.0 / 20.0;
/// The last part of every particle's life, over which it fades to nothing.
const LAST_FADE: f32 = 0.2;
/// How much of a trail has burned away at its tail, on top of the particle itself.
const TRAIL_BURN: f32 = 0.5;
/// Radians per second of `ParticleDesc::twinkle`.
const TWINKLE_RATE: f32 = 23.0;

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
    /// Its own color, from `ParticleDesc::tints`.
    tint: Vec4,
    /// Where it has been, newest first.
    trail: VecDeque<Vec3>,
    /// Seconds since the last point of the trail.
    trail_clock: f32,
}

#[derive(Debug, Clone)]
struct Emitter {
    particles: Vec<Particle>,
    rng: Rng,
    /// Particles owed by `rate` that didn't add up to a whole one yet.
    owed: f32,
    elapsed: f32,
    /// Seconds of `ParticleDesc::delay` gone by.
    waited: f32,
    /// Seconds since the last `ParticleDesc::repeat`.
    since_burst: f32,
    started: bool,
    last_origin: Vec3,
    /// How much it shows at the camera's distance: it fades out before `max_distance`,
    /// where it sleeps, so it never vanishes in one frame.
    fade: f32,
    /// Where particles died and hit the ground this frame, and how fast, for children.
    deaths: Vec<(Vec3, Vec3)>,
    bounces: Vec<(Vec3, Vec3)>,
    /// How bright its light is, and where.
    glow: f32,
    light_at: Vec3,
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
    /// How far the world's origin has moved since this began, so the curls it swirls in stay
    /// fixed in the world: see [`ParticleSystem::shift`].
    moved: Vec3,
    /// The height of the ground under it and where that was measured, for `bounce`.
    floor: Option<(f32, Vec3)>,
    /// The weather's wind, in m/s, for `windy`. Whoever updates the system sets it.
    pub wind: Vec3,
    /// Points on the skin of the body it wraps and their normals, world space, for emitters
    /// with `body`. Whoever moves the system keeps them up to date.
    pub body: Vec<(Vec3, Vec3)>,
    has_children: bool,
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
                waited: 0.0,
                since_burst: 0.0,
                started: false,
                last_origin: Vec3::ZERO,
                fade: 1.0,
                deaths: Vec::new(),
                bounces: Vec::new(),
                glow: 0.0,
                light_at: Vec3::ZERO,
            })
            .collect();
        let has_children = effect.iter().any(|d| d.from.is_some());
        Self {
            effect,
            emitters,
            offset,
            scale: scale.max(0.0),
            emitting: true,
            despawn_when_done: false,
            moved: Vec3::ZERO,
            floor: None,
            wind: Vec3::ZERO,
            body: Vec::new(),
            has_children,
        }
    }

    /// True if some particle bounces and the ground under `position` isn't known yet, or
    /// was measured too far from there.
    pub fn needs_floor(&self, position: Vec3) -> bool {
        self.effect.iter().any(|d| d.bounce > 0.0)
            && self.floor.is_none_or(|(_, at)| ((at - position) * Vec3::new(1.0, 0.0, 1.0)).length() > FLOOR_MOVE)
    }

    /// Tells it the height of the ground under `position`.
    pub fn set_floor(&mut self, height: f32, position: Vec3) {
        self.floor = Some((height, position));
    }

    /// Adds the light of every emitter that has one and is lit.
    pub fn lights(&self, out: &mut Vec<crate::PointLight>) {
        for (desc, emitter) in self.effect.iter().zip(&self.emitters) {
            let Some(light) = &desc.light else { continue };
            let intensity = light.intensity * emitter.glow * emitter.fade;
            if intensity > 0.01 {
                out.push(crate::PointLight {
                    position: emitter.light_at,
                    color: light.color,
                    intensity,
                    radius: light.radius * self.scale,
                    flicker: light.flicker,
                    night_only: false,
                });
            }
        }
    }

    /// Advances the simulation. `allowed` is false for entities that are hidden,
    /// which emit nothing. Effects that loop start out already burning.
    pub fn update(&mut self, transform: &Transform, dt: f32, camera: Vec3, allowed: bool) {
        let dt = dt.min(MAX_STEP);
        let emitting = self.emitting && allowed;
        let base = transform.position + transform.rotation * self.offset;
        for (desc, emitter) in self.effect.iter().zip(&mut self.emitters) {
            let origin = base + transform.rotation * (desc.position * self.scale);
            let floor = self.floor.map_or(origin.y, |(y, _)| y) + FLOOR_CLEAR;
            let step = Step {
                desc,
                scale: self.scale,
                rotation: transform.rotation,
                origin,
                emitting,
                moved: self.moved,
                floor,
                wind: self.wind,
                body: &self.body,
            };
            emitter.fade = distance_fade(origin.distance(camera), desc.max_distance);
            if origin.distance(camera) > desc.max_distance {
                emitter.particles.clear();
                emitter.owed = 0.0;
                // A looping effect starts over when the camera returns; one that ends is just over.
                emitter.started = !desc.looping();
                continue;
            }
            if desc.from.is_none() && emitter.waited < desc.delay {
                emitter.waited += dt;
                continue;
            }
            if !emitter.started {
                emitter.start(&step);
            }
            emitter.step(&step, dt);
        }
        if self.has_children {
            self.release_children(transform, base, dt);
        }
    }

    /// Lets every child emitter release what its parent's particles call for this frame.
    fn release_children(&mut self, transform: &Transform, base: Vec3, dt: f32) {
        for child in 0..self.emitters.len() {
            let desc = &self.effect[child];
            let Some(parent) = desc.from.map(|p| p as usize).filter(|&p| p != child && p < self.emitters.len()) else {
                continue;
            };
            if !self.emitters[child].started {
                continue;
            }
            let mut events = match desc.when {
                Spawn::Death => self.emitters[parent].deaths.clone(),
                Spawn::Bounce => self.emitters[parent].bounces.clone(),
                Spawn::Trail => Vec::new(),
            };
            let mut each = desc.burst.max(1);
            if desc.when == Spawn::Trail {
                each = 1;
                let alive = self.emitters[parent].particles.len();
                self.emitters[child].owed += desc.rate.max(0.0) * dt * alive as f32;
                let whole = self.emitters[child].owed.floor();
                self.emitters[child].owed -= whole;
                for _ in 0..(whole as usize).min(MOST_EVENTS * 4) {
                    let i = (self.emitters[child].rng.range(0.0, alive as f32) as usize).min(alive.saturating_sub(1));
                    if let Some(p) = self.emitters[parent].particles.get(i) {
                        events.push((p.position, p.velocity));
                    }
                }
            }
            let origin = base + transform.rotation * (desc.position * self.scale);
            let step = Step {
                desc,
                scale: self.scale,
                rotation: transform.rotation,
                origin,
                emitting: true,
                moved: self.moved,
                floor: self.floor.map_or(origin.y, |(y, _)| y) + FLOOR_CLEAR,
                wind: self.wind,
                body: &self.body,
            };
            for (at, velocity) in events {
                for _ in 0..each {
                    self.emitters[child].spawn_from(&step, at, velocity * desc.inherit);
                }
            }
        }
    }

    /// Moves every live particle by `offset`, as the world's origin moves the other way.
    pub fn shift(&mut self, offset: Vec3) {
        self.moved -= offset;
        if let Some((height, at)) = &mut self.floor {
            *height += offset.y;
            *at += offset;
        }
        for emitter in &mut self.emitters {
            emitter.last_origin += offset;
            emitter.light_at += offset;
            for p in &mut emitter.particles {
                p.position += offset;
                for point in &mut p.trail {
                    *point += offset;
                }
            }
        }
    }

    /// True once everything has been emitted and has died. Never, while looping.
    pub fn finished(&self) -> bool {
        self.effect.iter().zip(&self.emitters).all(|(desc, emitter)| {
            emitter.started
                && emitter.particles.is_empty()
                && emitter.glow < 0.02
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
            let key = |t| sample(&desc.ramp, t, Vec4::lerp, Vec4::ONE).to_array();
            let (rim, body, heart) = (key(0.0), key(0.5), key(1.0));
            let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
            let full = !desc.ramp.is_empty()
                || !desc.erode.is_empty()
                || desc.grain > 0.0
                || desc.warp > 0.0
                || desc.pixels > 0.0
                || desc.soft > 0.0
                || desc.distort > 0.0
                || !matches!(desc.shape, ParticleShape::Soft | ParticleShape::Diamond);
            let flat = if desc.flat { FLAT } else { 0.0 };
            for p in &emitter.particles {
                let t = p.age / p.life;
                let mut color = sample(&desc.colors, t, Vec4::lerp, Vec4::ONE) * p.tint;
                color.w *= emitter.fade;
                color.w *= 1.0 - desc.twinkle * (0.5 + 0.5 * (p.age * TWINKLE_RATE + p.phase * 5.0).sin());
                color.w *= last_fade(t);
                let mut size = p.size * sample(&desc.sizes, t, lerp, 0.2) * self.scale;
                if desc.pulse != 0.0 {
                    size *= 1.0 + desc.pulse * (p.age * desc.pulse_rate * std::f32::consts::TAU + p.phase).sin();
                }
                let upright = desc.upright > 0.0;
                let stretch = if upright { Vec3::Y * size * 0.5 * desc.upright } else { p.velocity * desc.stretch };
                let rooted = if upright { stretch + Vec3::Y * size * 0.5 } else { Vec3::ZERO };
                let burned = sample(&desc.erode, t, lerp, 0.0);
                let flag = f32::from(u8::from(full));
                let seed = p.phase / std::f32::consts::TAU;
                let head = ParticleInstance {
                    position: (p.position + rooted).into(),
                    size,
                    color: color.into(),
                    stretch: stretch.into(),
                    rotation: if upright { std::f32::consts::FRAC_PI_2 } else { p.rotation },
                    emissive: desc.emissive,
                    lit: desc.lit,
                    additive: desc.additive,
                    shape: desc.shape.index() + flat,
                    look: [burned, desc.pixels, desc.soft * self.scale, desc.distort],
                    flow: [desc.flow.x, desc.flow.y, desc.noise, desc.warp],
                    detail: [desc.bands, seed, desc.grain, flag],
                    rim,
                    body,
                    heart,
                };
                out.push(head);

                let mut from = p.position;
                let mut meters = 0.0;
                let pieces = p.trail.len().max(1) as f32;
                for (i, &to) in p.trail.iter().enumerate() {
                    let s = (i as f32 + 0.5) / pieces;
                    let half = (to - from) * 0.5;
                    let length = half.length();
                    out.push(ParticleInstance {
                        position: (from + half).into(),
                        size: size * (1.0 - s),
                        color: [color.x, color.y, color.z, color.w * (1.0 - s)],
                        stretch: half.into(),
                        rotation: meters + length,
                        shape: 5.0,
                        look: [burned + s * TRAIL_BURN, desc.pixels, desc.soft * self.scale, 0.0],
                        detail: [desc.bands, seed, desc.grain, 1.0],
                        ..head
                    });
                    meters += length * 2.0;
                    from = to;
                }
            }
        }
    }
}

/// How much is left of a particle `t` (0..1) through its life. A particle dying while it can
/// still be seen would blink out, so a file that forgets to fade one doesn't get to show that.
fn last_fade(t: f32) -> f32 {
    let left = ((1.0 - t) / LAST_FADE).clamp(0.0, 1.0);
    left * left * (3.0 - 2.0 * left)
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

/// A flow that neither gathers nor spreads, so particles carried by it swirl around each
/// other: the curl of a few sine waves that slowly move. `p` is in units of the curls' size.
fn curl_flow(p: Vec3, t: f32) -> Vec3 {
    // Derivatives of the potential (a, b, c), where each depends on the other two axes.
    let (ay, az) = ((p.y * 1.1 + t * 0.6).cos() * 1.1, (p.z * 2.3 - t * 0.4).cos() * 1.15);
    let (bz, bx) = ((p.z * 1.3 + t * 0.5).cos() * 1.3, (p.x * 2.1 + t * 0.3).cos() * 1.05);
    let (cx, cy) = ((p.x * 0.9 - t * 0.7).cos() * 0.9, (p.y * 1.9 + t * 0.2).cos() * 0.95);
    Vec3::new(cy - bz, az - cx, bx - ay)
}

/// What one emitter needs to know this frame.
struct Step<'a> {
    desc: &'a ParticleDesc,
    scale: f32,
    rotation: Quat,
    origin: Vec3,
    emitting: bool,
    /// [`ParticleSystem::moved`].
    moved: Vec3,
    /// The height particles that `bounce` land at.
    floor: f32,
    wind: Vec3,
    body: &'a [(Vec3, Vec3)],
}

impl Emitter {
    fn start(&mut self, step: &Step<'_>) {
        self.started = true;
        self.elapsed = 0.0;
        self.owed = 0.0;
        self.last_origin = step.origin;
        if !step.emitting || step.desc.from.is_some() {
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
        let carried = (step.origin - self.last_origin) * desc.local;
        let (curl, curl_size) = (desc.curl * step.scale * dt, (desc.curl_scale * step.scale).max(1e-3));
        let (orbit, attract) = (desc.orbit * step.scale * dt, desc.attract * step.scale * dt);
        let axis = step.rotation * desc.direction.normalize_or(Vec3::Y);
        let clock = self.elapsed;
        let wind = step.wind * desc.windy * dt;
        let (bounce, floor) = (desc.bounce, step.floor);
        self.deaths.clear();
        self.bounces.clear();
        let (deaths, bounces) = (&mut self.deaths, &mut self.bounces);
        self.particles.retain_mut(|p| {
            p.age += dt;
            if p.age >= p.life {
                if deaths.len() < MOST_EVENTS {
                    deaths.push((p.position, p.velocity));
                }
                return false;
            }
            let (a, phase) = (p.age, p.phase);
            let drift =
                Vec3::new((a * 4.1 + phase).sin(), (a * 3.3 + phase * 2.0).sin(), (a * 5.3 + phase * 3.0).cos());
            let mut push = acceleration + drift * wander + wind;
            if curl != 0.0 {
                push += curl_flow((p.position + step.moved) / curl_size, clock) * curl;
            }
            if orbit != 0.0 || attract != 0.0 {
                let to_center = (step.origin - p.position).normalize_or_zero();
                push += to_center * attract + to_center.cross(axis).normalize_or_zero() * orbit;
            }
            p.velocity = (p.velocity + push) * friction;
            // Where it was: a point at the head itself would be a piece with no length.
            let before = p.position;
            p.position += p.velocity * dt + carried;
            if bounce > 0.0 && p.position.y < floor && p.velocity.y < 0.0 {
                p.position.y = floor;
                let up = -p.velocity.y * bounce;
                p.velocity = Vec3::new(p.velocity.x * GROUND_GRIP, up, p.velocity.z * GROUND_GRIP);
                if up < REST_SPEED {
                    p.velocity.y = 0.0;
                    p.spin *= GROUND_GRIP;
                } else if bounces.len() < MOST_EVENTS {
                    bounces.push((p.position, p.velocity));
                }
            }
            if desc.trail > 0.0 {
                let points = desc.trail_points.max(1) as usize;
                let spacing = desc.trail / points as f32;
                p.trail_clock += dt;
                if p.trail_clock >= spacing {
                    p.trail_clock %= spacing;
                    p.trail.push_front(before);
                    p.trail.truncate(points);
                }
            }
            p.rotation += p.spin * dt;
            true
        });

        self.elapsed += dt;
        if let Some(light) = &desc.light {
            let (mut sum, mut at) = (0.0, Vec3::ZERO);
            for p in &self.particles {
                let t = p.age / p.life;
                let weight = sample(&desc.colors, t, Vec4::lerp, Vec4::ONE).w * last_fade(t);
                sum += weight;
                at += p.position * weight;
            }
            let mean_life = (desc.lifetime.0 + desc.lifetime.1) * 0.5;
            let full = (desc.burst as f32).max(desc.rate * mean_life).max(1.0);
            if sum > 1e-4 {
                self.light_at = at / sum;
            }
            self.glow = (sum / full).min(1.5).max(self.glow * (-dt / light.linger.max(1e-3)).exp());
        }
        if desc.from.is_some() {
            return;
        }
        if desc.repeat > 0.0 && step.emitting {
            self.since_burst += dt;
            if self.since_burst >= desc.repeat {
                self.since_burst -= desc.repeat;
                for _ in 0..desc.burst {
                    self.spawn(step, step.origin);
                }
            }
        }
        if !step.emitting || desc.duration.is_some_and(|duration| self.elapsed > duration) {
            self.owed = 0.0;
        } else {
            let swell = match desc.duration {
                Some(duration) if !desc.rates.is_empty() => {
                    sample(&desc.rates, self.elapsed / duration.max(1e-3), |a, b, t| a + (b - a) * t, 1.0)
                }
                _ => 1.0,
            };
            self.owed += desc.rate.max(0.0) * swell * dt;
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
        self.spawn_from(step, origin, Vec3::ZERO);
    }

    /// A new particle at `origin`, with `carried` added to the velocity it starts with.
    fn spawn_from(&mut self, step: &Step<'_>, origin: Vec3, carried: Vec3) {
        let desc = step.desc;
        if self.particles.len() >= desc.max_particles as usize {
            return;
        }
        let rng = &mut self.rng;
        let mut origin = origin;
        let mut skin_normal = None;
        if desc.body > 0.0 && !step.body.is_empty() && rng.range(0.0, 1.0) < desc.body {
            let (at, normal) = step.body[(rng.range(0.0, step.body.len() as f32) as usize).min(step.body.len() - 1)];
            origin = at;
            skin_normal = Some(normal);
        }
        let area = desc.area * step.scale;
        let box_offset = Vec3::new(rng.jitter(area.x), rng.jitter(area.y), rng.jitter(area.z));
        let axis = step.rotation * desc.direction.normalize_or(Vec3::Y);
        let mut offset = step.rotation * box_offset;
        if desc.ring > 0.0 {
            let (a, b) = axis.any_orthonormal_pair();
            let (s, c) = rng.range(0.0, std::f32::consts::TAU).sin_cos();
            offset += (a * c + b * s) * desc.ring * step.scale;
        }
        if desc.shell > 0.0 {
            offset += cone_direction(rng, Vec3::Y, 180.0) * desc.shell * step.scale;
        }
        let mut direction = cone_direction(rng, axis, desc.spread);
        if desc.outward != 0.0 {
            let away = skin_normal.unwrap_or_else(|| offset.normalize_or(direction)) * desc.outward.signum();
            direction = direction.lerp(away, desc.outward.abs().min(1.0)).normalize_or(direction);
        }
        let tint = if desc.tints.is_empty() {
            Vec4::ONE
        } else {
            sample(&desc.tints, rng.range(0.0, 1.0), Vec4::lerp, Vec4::ONE)
        };
        let spin = rng.jitter(desc.spin).to_radians();
        let rotation = rng.range(0.0, std::f32::consts::TAU);
        self.particles.push(Particle {
            position: origin + offset,
            velocity: direction * rng.range(desc.speed.0, desc.speed.1) * step.scale + carried,
            age: 0.0,
            life: rng.range(desc.lifetime.0, desc.lifetime.1).max(MIN_LIFE),
            size: 1.0 + rng.jitter(desc.size_jitter.clamp(0.0, 1.0)),
            rotation,
            spin,
            phase: rng.range(0.0, std::f32::consts::TAU),
            tint,
            trail: VecDeque::new(),
            trail_clock: 0.0,
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
    #[test]
    fn what_falls_follows_the_weather_and_the_camera() {
        let falling = |rain: f32, snow: f32, camera: Vec3, seconds: u32| {
            let mut env = Environment::default();
            env.precipitation.rain = rain;
            env.precipitation.snow = snow;
            let mut field = WeatherParticles::default();
            for _ in 0..seconds {
                field.update(1.0, camera, &env);
            }
            let mut out = Vec::new();
            field.collect(&mut out);
            out
        };
        assert!(falling(0.0, 0.0, Vec3::ZERO, 3).is_empty());
        let (light, heavy) = (falling(0.25, 0.0, Vec3::ZERO, 3), falling(1.0, 0.0, Vec3::ZERO, 3));
        assert!(light.len() > 200 && heavy.len() > light.len() * 3, "{} {}", light.len(), heavy.len());
        assert!(heavy.iter().all(|p| p.stretch[1] < 0.0), "rain is drawn along its fall");
        let flakes = falling(0.0, 1.0, Vec3::ZERO, 3);
        assert!(flakes.len() > 500 && flakes.iter().all(|p| p.stretch == [0.0; 3] && p.shape == 0.0));

        let far = Vec3::new(5000.0, 80.0, -3000.0);
        for p in falling(1.0, 1.0, far, 7) {
            let offset = Vec3::from(p.position) - far - Vec3::Y * FALL_ABOVE;
            assert!(offset.abs().cmple(FALL_REACH + Vec3::splat(0.01)).all(), "{offset} is out of the box");
            assert!(p.color[3] > 0.0);
        }
    }

    use super::*;

    #[test]
    fn a_trail_follows_the_path_and_thins_out() {
        let desc = ParticleDesc {
            rate: 0.0,
            burst: 1,
            lifetime: (9.0, 9.0),
            speed: (2.0, 2.0),
            direction: Vec3::X,
            spread: 0.0,
            sizes: vec![0.5],
            trail: 0.5,
            trail_points: 5,
            ..Default::default()
        };
        let mut comet = system(desc);
        run(&mut comet, &Transform::IDENTITY, 2.0);
        let all = instances(&comet);
        assert_eq!(all.len(), 6, "the head and five pieces");
        let (head, pieces) = all.split_first().unwrap();
        assert!(pieces.iter().all(|p| p.shape == 5.0 && p.detail[3] == 1.0));
        assert!(pieces.iter().all(|p| p.position[0] < head.position[0]), "behind it");
        // Each piece starts where the one before it ends, and is thinner and fainter.
        for pair in pieces.windows(2) {
            let end = Vec3::from(pair[0].position) + Vec3::from(pair[0].stretch);
            let start = Vec3::from(pair[1].position) - Vec3::from(pair[1].stretch);
            assert!(end.distance(start) < 1e-4, "{end} {start}");
            assert!(pair[1].size < pair[0].size && pair[1].color[3] < pair[0].color[3]);
            assert!(pair[1].look[0] > pair[0].look[0], "and more burned");
            assert!(pair[1].rotation > pair[0].rotation, "counting the meters from the head");
        }
    }

    #[test]
    fn nothing_blinks_out_at_the_end_of_its_life() {
        // Solid to the end as far as its file goes, and hard edged.
        let desc = ParticleDesc {
            rate: 0.0,
            burst: 1,
            lifetime: (1.0, 1.0),
            sizes: vec![1.0],
            shape: ParticleShape::Ring,
            bands: 3.0,
            ..Default::default()
        };
        let mut wave = system(desc);
        let mut last = 1.0;
        loop {
            wave.update(&Transform::IDENTITY, DT, NEAR, true);
            let Some(now) = instances(&wave).first().map(|p| p.color[3]) else { break };
            assert!(last - now < 0.15, "dropped from {last} to {now} in one frame");
            last = now;
        }
        assert!(last < 0.02, "still at {last} on its last frame");
    }

    #[test]
    fn repeat_beats_until_it_is_switched_off() {
        let desc = ParticleDesc { rate: 0.0, burst: 2, repeat: 0.5, lifetime: (0.1, 0.1), ..Default::default() };
        let mut heart = system(desc);
        let mut beats = 0;
        let mut before = 0;
        for _ in 0..130 {
            heart.update(&Transform::IDENTITY, DT, NEAR, true);
            let now = heart.particle_count();
            beats += usize::from(now > before);
            before = now;
        }
        assert!((4..=7).contains(&beats), "{beats} beats in about two seconds");
        assert!(!heart.finished(), "it never ends");
        heart.emitting = false;
        run(&mut heart, &Transform::IDENTITY, 1.0);
        assert_eq!(heart.particle_count(), 0);
    }

    #[test]
    fn flat_is_marked_on_the_shape() {
        let desc = ParticleDesc { rate: 0.0, burst: 1, flat: true, shape: ParticleShape::Ring, ..Default::default() };
        let mut ring = system(desc);
        ring.update(&Transform::IDENTITY, DT, NEAR, true);
        assert_eq!(instances(&ring)[0].shape, 3.0 + FLAT);
    }

    #[test]
    fn delay_holds_an_emitter_back() {
        let desc = ParticleDesc { rate: 0.0, burst: 3, delay: 0.5, ..Default::default() };
        let mut late = system(desc);
        run(&mut late, &Transform::IDENTITY, 0.4);
        assert_eq!(late.particle_count(), 0);
        assert!(!late.finished(), "hasn't played yet");
        run(&mut late, &Transform::IDENTITY, 0.2);
        assert_eq!(late.particle_count(), 3);
    }

    #[test]
    fn local_particles_go_along_with_the_emitter() {
        let desc = ParticleDesc { rate: 0.0, burst: 1, lifetime: (9.0, 9.0), local: 1.0, ..Default::default() };
        let mut flame = system(desc);
        run(&mut flame, &Transform::IDENTITY, 0.1);
        run(&mut flame, &Transform::from_position(Vec3::X * 5.0), 0.1);
        assert!((instances(&flame)[0].position[0] - 5.0).abs() < 1e-3);
    }

    #[test]
    fn the_curl_flow_turns_without_gathering() {
        // A field without divergence neither piles particles up nor scatters them.
        let e = 1e-2;
        for p in [Vec3::ZERO, Vec3::new(1.3, -0.4, 2.2), Vec3::new(-3.0, 5.0, 0.7)] {
            let d = |axis: Vec3| (curl_flow(p + axis * e, 0.4) - curl_flow(p - axis * e, 0.4)) / (2.0 * e);
            let divergence = d(Vec3::X).x + d(Vec3::Y).y + d(Vec3::Z).z;
            assert!(divergence.abs() < 1e-2, "{divergence} at {p}");
            assert!(curl_flow(p, 0.4).length() > 0.0);
        }
    }

    #[test]
    fn orbit_circles_and_attract_gathers() {
        let around = ParticleDesc {
            rate: 0.0,
            burst: 1,
            lifetime: (9.0, 9.0),
            position: Vec3::ZERO,
            area: Vec3::ZERO,
            orbit: 4.0,
            ..Default::default()
        };
        let start = Transform::from_position(Vec3::X);
        let mut vortex = system(around.clone());
        vortex.update(&start, DT, NEAR, true);
        // The emitter moves away, leaving the particle off to one side of it.
        run(&mut vortex, &Transform::IDENTITY, 0.5);
        let p = Vec3::from(instances(&vortex)[0].position);
        assert!(p.z.abs() > 0.05, "pushed sideways: {p}");
        assert!(p.y.abs() < 1e-3, "and around the up axis only: {p}");

        let mut pull = system(ParticleDesc { orbit: 0.0, attract: 4.0, ..around });
        pull.update(&start, DT, NEAR, true);
        run(&mut pull, &Transform::IDENTITY, 0.4);
        assert!(instances(&pull)[0].position[0] < 0.9, "drawn in");
    }

    #[test]
    fn plain_particles_stay_plain_and_a_ramp_turns_on_the_full_look() {
        let plain = system(ParticleDesc { rate: 0.0, burst: 1, ..Default::default() });
        let full = ParticleDesc {
            rate: 0.0,
            burst: 1,
            lifetime: (1.0, 1.0),
            ramp: vec![Vec4::X, Vec4::Y, Vec4::Z],
            erode: vec![0.0, 1.0],
            pixels: 40.0,
            distort: 0.1,
            ..Default::default()
        };
        let mut burn = system(full);
        let mut flat = plain;
        flat.update(&Transform::IDENTITY, DT, NEAR, true);
        assert_eq!(instances(&flat)[0].detail[3], 0.0);
        run(&mut burn, &Transform::IDENTITY, 0.5);
        let p = instances(&burn)[0];
        assert_eq!(p.detail[3], 1.0);
        assert_eq!((p.rim, p.body, p.heart), (Vec4::X.to_array(), Vec4::Y.to_array(), Vec4::Z.to_array()));
        assert!((p.look[0] - 0.5).abs() < 0.05, "half burned at half its life: {}", p.look[0]);
        assert!(p.bends());
    }

    #[test]
    fn upright_particles_stand_up_whatever_their_motion() {
        let desc = ParticleDesc {
            rate: 0.0,
            burst: 1,
            lifetime: (9.0, 9.0),
            speed: (3.0, 3.0),
            direction: Vec3::X,
            spread: 0.0,
            sizes: vec![2.0],
            upright: 1.0,
            spin: 90.0,
            ..Default::default()
        };
        let mut flame = system(desc);
        run(&mut flame, &Transform::IDENTITY, 0.2);
        let p = instances(&flame)[0];
        assert_eq!(p.stretch, [0.0, 1.0, 0.0]);
        assert!(p.position[1] > 1.9, "stands on its position: {}", p.position[1]);
        assert_eq!(p.rotation, std::f32::consts::FRAC_PI_2);
    }

    #[test]
    fn sparks_bounce_and_children_are_born_where_they_land() {
        let spark = ParticleDesc {
            rate: 0.0,
            burst: 4,
            lifetime: (3.0, 3.0),
            acceleration: Vec3::new(0.0, -9.8, 0.0),
            bounce: 0.5,
            light: Some(ParticleLight::default()),
            ..Default::default()
        };
        let puff =
            ParticleDesc { burst: 2, lifetime: (0.3, 0.3), from: Some(0), when: Spawn::Bounce, ..Default::default() };
        let mut sparks = ParticleSystem::new(Arc::from([spark, puff]), Vec3::ZERO, 1.0, 7);
        let above = Transform::from_position(Vec3::Y * 2.0);
        assert!(sparks.needs_floor(above.position));
        sparks.set_floor(0.0, above.position);
        let mut most = 0;
        for _ in 0..150 {
            sparks.update(&above, DT, NEAR, true);
            most = most.max(sparks.particle_count());
            assert!(instances(&sparks).iter().all(|p| p.position[1] >= 0.0), "none under the ground");
        }
        let mut lights = Vec::new();
        sparks.lights(&mut lights);
        assert!(most >= 4 + 8, "each of the four made two more on landing: {most}");
        assert_eq!(lights.len(), 1);
        run(&mut sparks, &above, 2.0);
        assert!(sparks.finished(), "children end with their parent, and the light goes out");
    }

    #[test]
    fn a_ring_that_goes_outward_widens() {
        let desc = ParticleDesc {
            rate: 0.0,
            burst: 12,
            lifetime: (9.0, 9.0),
            ring: 1.0,
            outward: 1.0,
            speed: (2.0, 2.0),
            ..Default::default()
        };
        let mut ring = system(desc);
        run(&mut ring, &Transform::IDENTITY, 1.0);
        for p in instances(&ring) {
            let p = Vec3::from(p.position);
            assert!(p.y.abs() < 1e-3 && (p.length() - 3.0).abs() < 0.1, "{p}");
        }
    }

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
    fn far_emitters_fade_out_before_they_sleep() {
        let desc = ParticleDesc { rate: 60.0, lifetime: (1.0, 1.0), max_distance: 40.0, ..Default::default() };
        let mut fire = system(desc);
        // The most any particle shows, as the camera backs away a step at a time.
        let mut last = 1.0f32;
        for step in 0..=80 {
            let camera = Vec3::new(20.0 + step as f32 * 0.25, 0.0, 0.0);
            fire.update(&Transform::IDENTITY, DT, camera, true);
            let mut out = Vec::new();
            fire.collect(&mut out);
            let shown = out.iter().map(|p| p.color[3]).fold(0.0, f32::max);
            assert!(last - shown < 0.08, "it jumped from {last} to {shown} at {}", camera.x);
            last = shown;
        }
        assert_eq!(last, 0.0, "asleep past its distance");
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

/// The look of a [`BladeTrail`]: a sheet swept by the blade, hot at the edge and at the
/// head, that thins toward the edge and breaks into strands along its tail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BladeTrailDesc {
    /// Seconds a piece of the sheet lasts.
    pub life: f32,
    /// sRGB colors and glow, from the faint rim to the white heart.
    pub rim: [f32; 4],
    pub body: [f32; 4],
    pub heart: [f32; 4],
    pub opacity: f32,
    /// How much the strands bite into it, 0 to 1.
    pub strands: f32,
}

impl Default for BladeTrailDesc {
    fn default() -> Self {
        Self {
            life: 0.2,
            rim: [0.35, 0.42, 0.75, 0.6],
            body: [0.72, 0.82, 1.0, 1.4],
            heart: [1.0, 0.98, 0.92, 3.0],
            opacity: 1.0,
            strands: 0.75,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct BladeSample {
    base: Vec3,
    tip: Vec3,
    age: f32,
    /// Meters the tip has traveled up to here.
    along: f32,
    /// Starts a new stroke: no piece joins it to the one before.
    cut: bool,
}

/// What a blade leaves behind while it cuts: the base and the tip at each frame, drawn as
/// one smooth sheet between them (the frames are joined by curves, since a tip moves half a
/// meter in one), as particles of [`BLADE_RIBBON`].
#[derive(Debug, Clone)]
pub struct BladeTrail {
    pub desc: BladeTrailDesc,
    samples: VecDeque<BladeSample>,
    live: bool,
    seed: f32,
}

/// The shape index of a piece of a blade's sheet.
pub const BLADE_RIBBON: f32 = 13.0;
/// Meters of the tip's path a piece of the sheet covers, and the most pieces between two frames.
const RIBBON_STEP: f32 = 0.035;
const RIBBON_MOST: usize = 24;

impl BladeTrail {
    pub fn new(desc: BladeTrailDesc, seed: u32) -> Self {
        Self { desc, samples: VecDeque::new(), live: false, seed: (seed % 997) as f32 * 0.137 }
    }

    /// `blade` is where the base and the tip are now, while the blade cuts.
    pub fn update(&mut self, dt: f32, blade: Option<(Vec3, Vec3)>) {
        let life = self.desc.life.max(0.01);
        for s in &mut self.samples {
            s.age += dt;
        }
        while self.samples.len() > 1 && self.samples[1].age >= life {
            self.samples.pop_front();
        }
        if self.samples.len() == 1 && self.samples[0].age >= life {
            self.samples.clear();
        }
        match blade {
            Some((base, tip)) => {
                let last = self.samples.back().copied();
                let along = last.map_or(0.0, |l| l.along + l.tip.distance(tip));
                self.samples.push_back(BladeSample { base, tip, age: 0.0, along, cut: !self.live });
                self.live = true;
            }
            None => self.live = false,
        }
    }

    pub fn shift(&mut self, offset: Vec3) {
        for s in &mut self.samples {
            s.base += offset;
            s.tip += offset;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.samples.len() < 2
    }

    pub fn collect(&self, out: &mut Vec<ParticleInstance>) {
        let life = self.desc.life.max(0.01);
        let n = self.samples.len();
        let d = &self.desc;
        let at = |i: usize| self.samples[i];
        for i in 1..n {
            let (a, b) = (at(i - 1), at(i));
            if b.cut || a.age >= life {
                continue;
            }
            // The frames around, for the curve: none across a cut.
            let before = if a.cut || i < 2 { a } else { at(i - 2) };
            let after = if i + 1 < n && !at(i + 1).cut { at(i + 1) } else { b };
            let pieces = ((b.tip.distance(a.tip) / RIBBON_STEP).ceil() as usize).clamp(1, RIBBON_MOST);
            let point = |k: usize| {
                let u = k as f32 / pieces as f32;
                let base = catmull_rom(before.base, a.base, b.base, after.base, u);
                let tip = catmull_rom(before.tip, a.tip, b.tip, after.tip, u);
                (base, tip, a.age + (b.age - a.age) * u, a.along + (b.along - a.along) * u)
            };
            let mut from = point(0);
            for k in 1..=pieces {
                let to = point(k);
                out.push(ParticleInstance {
                    position: from.0.to_array(),
                    size: 0.0,
                    color: [1.0, 1.0, 1.0, d.opacity],
                    stretch: from.1.to_array(),
                    rotation: (from.2 / life).min(1.0),
                    emissive: 1.0,
                    lit: 0.0,
                    additive: 1.0,
                    shape: BLADE_RIBBON,
                    // Below zero, or it would count as bending what is behind it.
                    look: [to.1.x, to.1.y, to.1.z, -1.0 - to.3],
                    flow: [to.0.x, to.0.y, to.0.z, from.3],
                    detail: [(to.2 / life).min(1.0), self.seed, d.strands, 1.0],
                    rim: d.rim,
                    body: d.body,
                    heart: d.heart,
                });
                from = to;
            }
        }
    }
}

fn catmull_rom(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * (2.0 * p1 + (p2 - p0) * t + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2 + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t3)
}
