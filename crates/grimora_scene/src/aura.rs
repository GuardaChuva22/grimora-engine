//! Auras: the light, fire and power a body gives off, anime style. An aura file
//! (`assets/auras/*.ron`) says what it is made of in a few high level layers (flames,
//! glow, rising streaks, sparks, bolts, orbs, smoke, signs on the ground, rocks lifting,
//! hands, haze and light), and [`AuraDesc::effect`] turns that into ordinary emitters.
//! Every layer is optional; colors are sRGB, `(r, g, b, glow)` in ramps, rim to heart.
//!
//! The body is a column `height` tall and `radius` wide standing on the effect's origin
//! (the feet); every size and speed is measured against it, so `scale` makes a giant's.

use std::path::Path;

use anyhow::{Context as _, Result};
use grimora_core::{
    AuraLook, AuraPattern, ParticleDesc, ParticleEffect, ParticleLight, ParticleShape, Spawn, Vec2, Vec3, Vec4,
};
use serde::{Deserialize, Serialize};

/// Fire's turbulence on the body, in repeats per meter, and how it rises (m/s): every
/// tongue of an aura cuts its shape out of the same field, so they burn as one.
const FIRE_REPEATS: f32 = 1.8;
const FIRE_FLOW: Vec2 = Vec2::new(0.1, -2.2);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AuraDesc {
    /// Shown in menus.
    pub name: String,
    /// Meters of the body it wraps.
    pub height: f32,
    pub radius: f32,
    /// How fierce: multiplies how many particles there are and how far they reach.
    pub power: f32,
    /// How much of the body's motion the aura shares, `0..1`: 1 sticks to it, less
    /// leaves the flames streaming behind a runner.
    pub follow: f32,
    /// The aura's colors, rim to heart. Layers without their own ramp use it.
    pub ramp: Vec<Vec4>,
    /// Hard color steps and pixels per meter, the PSX look, for every layer.
    pub bands: f32,
    pub pixels: f32,
    /// Patterns painted over the body itself (fire, veins, smoke, magma, ghost), one over
    /// the other. See [`Skin`].
    pub skin: Vec<Skin>,
    /// What flows off the skin: tongues of flame, smoke, strands. See [`Wisps`].
    pub wisps: Vec<Wisps>,
    pub flames: Option<Flames>,
    pub glow: Option<Glow>,
    pub streaks: Option<Streaks>,
    pub sparks: Option<Sparks>,
    pub bolts: Option<Bolts>,
    pub orbs: Option<Orbs>,
    pub smoke: Option<Smoke>,
    pub ground: Option<GroundSigns>,
    pub debris: Option<Debris>,
    pub hands: Option<Hands>,
    /// Heat haze over the body, meters of bending at one meter.
    pub haze: f32,
    pub light: Option<ParticleLight>,
    /// What happens the moment it lights.
    pub burst: Option<Burst>,
}

impl Default for AuraDesc {
    fn default() -> Self {
        Self {
            name: String::new(),
            height: 1.8,
            radius: 0.42,
            power: 1.0,
            follow: 0.92,
            ramp: vec![Vec4::new(0.9, 0.3, 0.05, 1.5), Vec4::new(1.0, 0.7, 0.2, 4.0), Vec4::new(1.0, 0.95, 0.8, 9.0)],
            bands: 4.0,
            pixels: 90.0,
            skin: Vec::new(),
            wisps: Vec::new(),
            flames: None,
            glow: None,
            streaks: None,
            sparks: None,
            bolts: None,
            orbs: None,
            smoke: None,
            ground: None,
            debris: None,
            hands: None,
            haze: 0.0,
            light: None,
            burst: None,
        }
    }
}

/// A pattern painted over the body (`Shading::Aura`), moving with its skin. Colors
/// default to the aura's ramp (`heart` its last key, `rim` its second).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Skin {
    pub pattern: AuraPattern,
    /// Noise cells per meter, and meters a second it climbs.
    pub scale: f32,
    pub speed: f32,
    /// How much it curls on itself, like a fluid.
    pub warp: f32,
    /// Light at the outline, and how tight to it.
    pub outline: f32,
    pub edge: f32,
    /// How much of the body it hides, and how bright the cover is (times `rim`).
    pub cover: f32,
    pub body: f32,
    pub glow: f32,
    pub heart: Option<Vec3>,
    pub rim: Option<Vec3>,
}

impl Default for Skin {
    fn default() -> Self {
        Self {
            pattern: AuraPattern::Fire,
            scale: 4.0,
            speed: 1.2,
            warp: 0.35,
            outline: 0.6,
            edge: 2.5,
            cover: 0.0,
            body: 0.0,
            glow: 3.0,
            heart: None,
            rim: None,
        }
    }
}

/// Something that flows off the skin like a fluid: each particle is born on a point of the
/// moving body, pushed out along its normal, carried up and curled by the air, and burns
/// away along its noise. Tongues of fire, wisps of smoke, strands of light.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Wisps {
    pub rate: f32,
    /// Width, meters, and how much taller than wide (`upright`, 0 round).
    pub size: f32,
    pub tall: f32,
    pub life: (f32, f32),
    /// m/s off the skin, and m/s² upward (negative falls).
    pub push: f32,
    pub rise: f32,
    /// Swirl of the air, and the size of its curls.
    pub curl: f32,
    pub curl_scale: f32,
    /// Seconds of path drawn behind each: strands.
    pub trail: f32,
    pub stretch: f32,
    pub shape: ParticleShape,
    pub opacity: f32,
    /// 1 adds light (fire), 0 covers (smoke).
    pub additive: f32,
    /// How much the noise tears and bends it.
    pub grain: f32,
    pub warp: f32,
    /// How much the scene's light falls on it (smoke 1, fire 0).
    pub lit: f32,
    /// How much the weather's wind carries it.
    pub windy: f32,
    pub ramp: Vec<Vec4>,
}

impl Default for Wisps {
    fn default() -> Self {
        Self {
            rate: 60.0,
            size: 0.14,
            tall: 1.2,
            life: (0.4, 0.8),
            push: 0.25,
            rise: 1.6,
            curl: 1.2,
            curl_scale: 0.3,
            trail: 0.0,
            stretch: 0.0,
            shape: ParticleShape::Flame,
            opacity: 0.85,
            additive: 0.8,
            grain: 0.7,
            warp: 0.5,
            lit: 0.0,
            windy: 0.3,
            ramp: Vec::new(),
        }
    }
}

/// Loose tongues of fire coming off the skin, on top of the painted patterns. A hot inner
/// layer goes with them unless `core` is 0.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Flames {
    pub rate: f32,
    /// Width of a tongue, meters.
    pub size: f32,
    /// How fast they rise, m/s²: their height comes from this.
    pub rise: f32,
    /// Seconds each lives, `(min, max)`.
    pub life: (f32, f32),
    /// How tall a tongue is for its width (`upright`).
    pub tall: f32,
    /// Swirl of the air they burn in.
    pub sway: f32,
    /// Noise that bites and bends them.
    pub grain: f32,
    pub warp: f32,
    /// Opacity, and how much they add light instead of covering (`additive`).
    pub opacity: f32,
    pub additive: f32,
    /// Share of the inner, hotter layer: 0 has none.
    pub core: f32,
    /// Turns around the body, m/s²: a spiral of fire.
    pub spiral: f32,
    pub shape: ParticleShape,
    pub ramp: Vec<Vec4>,
}

impl Default for Flames {
    fn default() -> Self {
        Self {
            rate: 70.0,
            size: 0.32,
            rise: 2.2,
            life: (0.45, 0.8),
            tall: 1.3,
            sway: 0.7,
            grain: 0.7,
            warp: 0.4,
            opacity: 0.8,
            additive: 0.75,
            core: 0.6,
            spiral: 0.0,
            shape: ParticleShape::Flame,
            ramp: Vec::new(),
        }
    }
}

/// A soft egg of light around the body that breathes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Glow {
    pub opacity: f32,
    /// Width, as a share of the body's height.
    pub size: f32,
    /// How much it swells and how often.
    pub breath: f32,
    pub breath_rate: f32,
    pub color: Option<Vec4>,
}

impl Default for Glow {
    fn default() -> Self {
        Self { opacity: 0.12, size: 0.75, breath: 0.12, breath_rate: 1.2, color: None }
    }
}

/// The lines that shoot up past the body, the mark of power in anime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Streaks {
    pub rate: f32,
    pub speed: (f32, f32),
    /// Seconds of travel each is stretched by.
    pub length: f32,
    pub width: f32,
    /// Seconds of path drawn behind (0 is a plain stretched streak).
    pub trail: f32,
    pub color: Option<Vec4>,
    pub glow: f32,
}

impl Default for Streaks {
    fn default() -> Self {
        Self { rate: 14.0, speed: (4.0, 7.0), length: 0.06, width: 0.035, trail: 0.0, color: None, glow: 7.0 }
    }
}

/// Embers, motes, glints, leaves: small loose things carried up in curls.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sparks {
    pub rate: f32,
    pub size: f32,
    pub speed: (f32, f32),
    /// Upward pull, m/s²; negative falls (drops, ash).
    pub rise: f32,
    pub curl: f32,
    pub life: (f32, f32),
    pub shape: ParticleShape,
    pub stretch: f32,
    pub twinkle: f32,
    pub spin: f32,
    /// Each takes a hue between these.
    pub tints: Vec<Vec4>,
    pub glow: f32,
}

impl Default for Sparks {
    fn default() -> Self {
        Self {
            rate: 16.0,
            size: 0.03,
            speed: (0.3, 1.2),
            rise: 1.2,
            curl: 2.0,
            life: (0.6, 1.4),
            shape: ParticleShape::Diamond,
            stretch: 0.0,
            twinkle: 0.7,
            spin: 0.0,
            tints: Vec::new(),
            glow: 8.0,
        }
    }
}

/// Lightning crawling over the body, each bolt throwing a few sparks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Bolts {
    pub rate: f32,
    pub size: f32,
    /// How far from the body they strike, as a share of `radius`.
    pub reach: f32,
    pub color: Option<Vec4>,
    pub glow: f32,
    /// Sparks thrown where each dies.
    pub sparks: u32,
}

impl Default for Bolts {
    fn default() -> Self {
        Self { rate: 6.0, size: 0.35, reach: 1.1, color: None, glow: 10.0, sparks: 3 }
    }
}

/// Lights circling the body with tails behind them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Orbs {
    pub count: f32,
    /// Radius of the circle, as a share of the body's height.
    pub distance: f32,
    /// m/s around.
    pub speed: f32,
    pub size: f32,
    /// Seconds of tail.
    pub trail: f32,
    /// How much they wander up and down the body.
    pub wander: f32,
    pub shape: ParticleShape,
    pub color: Option<Vec4>,
    pub glow: f32,
}

impl Default for Orbs {
    fn default() -> Self {
        Self {
            count: 5.0,
            distance: 0.5,
            speed: 2.6,
            size: 0.06,
            trail: 0.35,
            wander: 0.5,
            shape: ParticleShape::Soft,
            color: None,
            glow: 6.0,
        }
    }
}

/// Smoke rising off the body: darkness for shadow, soot for fire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Smoke {
    pub rate: f32,
    pub size: f32,
    pub opacity: f32,
    pub rise: f32,
    /// `(r, g, b, glow)`, rim to heart.
    pub ramp: Vec<Vec4>,
}

impl Default for Smoke {
    fn default() -> Self {
        Self {
            rate: 12.0,
            size: 0.45,
            opacity: 0.4,
            rise: 0.8,
            ramp: vec![Vec4::new(0.06, 0.03, 0.08, 1.0), Vec4::new(0.1, 0.05, 0.12, 1.0)],
        }
    }
}

/// What the aura does to the ground it stands on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GroundSigns {
    /// A turning circle of signs, this wide (0 has none).
    pub runes: f32,
    pub runes_spin: f32,
    /// A shockwave every this many seconds (0 has none), this wide.
    pub pulse: f32,
    pub pulse_size: f32,
    /// Dust blown out along the ground, per second.
    pub dust: f32,
    pub dust_color: Vec4,
    pub color: Option<Vec4>,
}

impl Default for GroundSigns {
    fn default() -> Self {
        Self {
            runes: 0.0,
            runes_spin: 25.0,
            pulse: 0.0,
            pulse_size: 3.0,
            dust: 0.0,
            dust_color: Vec4::new(0.55, 0.48, 0.4, 0.5),
            color: None,
        }
    }
}

/// Stones lifting off the ground around the body, as power tears it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Debris {
    pub rate: f32,
    pub size: f32,
    /// How far out they come from, as a share of the body's height.
    pub distance: f32,
    pub rise: f32,
    pub color: Vec4,
}

impl Default for Debris {
    fn default() -> Self {
        Self { rate: 3.0, size: 0.12, distance: 0.9, rise: 0.25, color: Vec4::new(0.45, 0.4, 0.36, 1.0) }
    }
}

/// Fire in the hands, played at each hand joint by the engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Hands {
    pub rate: f32,
    pub size: f32,
    pub sparks: f32,
    pub ramp: Vec<Vec4>,
}

impl Default for Hands {
    fn default() -> Self {
        Self { rate: 22.0, size: 0.12, sparks: 6.0, ramp: Vec::new() }
    }
}

/// The flash of an aura bursting into being.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Burst {
    /// Width of the shockwave on the ground.
    pub wave: f32,
    /// Streaks thrown out.
    pub streaks: u32,
    /// Brightness of the flash of light.
    pub flash: f32,
    pub color: Option<Vec4>,
}

impl Default for Burst {
    fn default() -> Self {
        Self { wave: 6.0, streaks: 40, flash: 12.0, color: None }
    }
}

/// The color of a key, at full opacity.
fn rgb(v: Vec4) -> Vec4 {
    Vec4::new(v.x, v.y, v.z, 1.0)
}

fn fade(alpha: f32) -> Vec<Vec4> {
    vec![
        Vec4::new(1.0, 1.0, 1.0, 0.0),
        Vec4::new(1.0, 1.0, 1.0, alpha),
        Vec4::new(1.0, 1.0, 1.0, alpha * 0.7),
        Vec4::new(1.0, 1.0, 1.0, 0.0),
    ]
}

impl AuraDesc {
    fn ramp_or<'a>(&'a self, own: &'a [Vec4]) -> &'a [Vec4] {
        if own.is_empty() { &self.ramp } else { own }
    }

    /// The color at the heart of the aura, `(r, g, b, 1)`.
    fn heart(&self) -> Vec4 {
        self.ramp.last().copied().map_or(Vec4::ONE, rgb)
    }

    /// The color in the middle of the ramp, the aura's own hue.
    fn hue(&self) -> Vec4 {
        self.ramp.get(self.ramp.len() / 2).copied().map_or(Vec4::ONE, rgb)
    }

    /// The looks of the patterns painted on the body, in order.
    pub fn skin_looks(&self) -> Vec<AuraLook> {
        let key = |i: usize| self.ramp.get(i).or(self.ramp.last()).map_or(Vec3::ONE, |c| c.truncate());
        self.skin
            .iter()
            .map(|s| AuraLook {
                pattern: s.pattern,
                heart: s.heart.unwrap_or_else(|| key(self.ramp.len().saturating_sub(1))),
                rim: s.rim.unwrap_or_else(|| key(1)),
                glow: s.glow * self.power,
                thickness: if s.pattern == AuraPattern::Blaze { 0.0 } else { 0.004 },
                scale: s.scale,
                speed: s.speed,
                warp: s.warp,
                outline: s.outline,
                edge: s.edge,
                cover: s.cover,
                body: s.body,
                bands: self.bands,
                pixels: self.pixels,
            })
            .collect()
    }

    /// The emitters of the aura around the body, looping until stopped. Those that come
    /// off the body (`ParticleDesc::body`) need its bones in `ParticleSystem::body`.
    pub fn effect(&self) -> ParticleEffect {
        let (h, r, p) = (self.height, self.radius, self.power.max(0.0));
        let (bands, pixels, follow) = (self.bands, self.pixels, self.follow);
        let mut out: Vec<ParticleDesc> = Vec::new();
        let base = ParticleDesc { local: follow, max_distance: 80.0, ..Default::default() };

        if let Some(g) = &self.glow {
            let color = g.color.map_or(self.hue(), rgb);
            let size = h * g.size;
            out.push(ParticleDesc {
                rate: 6.0,
                lifetime: (0.7, 1.0),
                position: Vec3::new(0.0, -h * 0.05, 0.0),
                area: Vec3::new(r * 0.15, 0.0, r * 0.15),
                sizes: vec![size * 0.85, size, size * 0.95],
                size_jitter: 0.08,
                upright: 0.7,
                colors: fade(g.opacity * p.min(1.5)).iter().map(|c| color * *c).collect(),
                pulse: g.breath,
                pulse_rate: g.breath_rate,
                emissive: 1.6,
                additive: 1.0,
                soft: 0.6,
                light: self.light.clone(),
                ..base.clone()
            });
        } else if let Some(light) = &self.light {
            // A light needs particles to sit in: an invisible one.
            out.push(ParticleDesc {
                rate: 4.0,
                lifetime: (0.6, 0.6),
                position: Vec3::new(0.0, h * 0.5, 0.0),
                colors: vec![Vec4::new(1.0, 1.0, 1.0, 1.0)],
                sizes: vec![0.0],
                light: Some(light.clone()),
                ..base.clone()
            });
        }

        if let Some(s) = &self.smoke {
            out.push(ParticleDesc {
                rate: s.rate * p,
                lifetime: (0.9, 1.6),
                position: Vec3::new(0.0, h * 0.35, 0.0),
                area: Vec3::splat(r * 0.3),
                body: 1.0,
                speed: (0.1, 0.4),
                acceleration: Vec3::Y * s.rise,
                drag: 0.6,
                curl: 1.2,
                curl_scale: 0.5,
                sizes: vec![s.size * 0.4, s.size, s.size * 1.5],
                size_jitter: 0.3,
                colors: fade(s.opacity),
                ramp: s.ramp.clone(),
                erode: vec![0.3, 0.1, 0.3, 0.9],
                noise: 1.4,
                flow: Vec2::new(0.0, -0.5),
                warp: 0.4,
                grain: 0.8,
                bands: (bands * 0.5).round(),
                pixels: pixels * 0.6,
                soft: 0.2,
                lit: 0.2,
                emissive: 0.4,
                local: follow * 0.6,
                windy: 0.4,
                ..base.clone()
            });
        }

        for w in &self.wisps {
            let ramp = self.ramp_or(&w.ramp).to_vec();
            out.push(ParticleDesc {
                rate: w.rate * p,
                lifetime: w.life,
                max_particles: 700,
                position: Vec3::new(0.0, h * 0.5, 0.0),
                area: Vec3::splat(0.015),
                body: 1.0,
                outward: 1.0,
                spread: 30.0,
                speed: (w.push * 0.4, w.push),
                acceleration: Vec3::Y * w.rise * p.sqrt(),
                drag: 1.4,
                curl: w.curl,
                curl_scale: w.curl_scale,
                sizes: vec![w.size * 0.35, w.size, w.size * 0.85, w.size * 0.45],
                size_jitter: 0.45,
                upright: w.tall,
                shape: w.shape,
                spin: if w.tall > 0.0 { 0.0 } else { 60.0 },
                colors: vec![
                    Vec4::new(1.0, 1.0, 1.0, w.opacity),
                    Vec4::new(1.0, 1.0, 1.0, w.opacity),
                    Vec4::new(1.0, 1.0, 1.0, w.opacity * 0.7),
                    Vec4::new(1.0, 1.0, 1.0, 0.0),
                ],
                ramp,
                erode: vec![0.05, 0.0, 0.3, 0.75, 1.0 + w.grain],
                noise: if w.shape == ParticleShape::Flame { FIRE_REPEATS } else { 1.4 },
                flow: if w.shape == ParticleShape::Flame { FIRE_FLOW } else { Vec2::new(0.05, -1.3) },
                warp: w.warp,
                grain: w.grain,
                bands: if w.shape == ParticleShape::Flame { 0.0 } else { bands },
                pixels,
                soft: 0.05,
                emissive: if w.lit > 0.0 { 0.4 } else { 1.0 },
                lit: w.lit,
                additive: w.additive,
                trail: w.trail,
                trail_points: 12,
                stretch: w.stretch,
                windy: w.windy,
                ..base.clone()
            });
        }

        if let Some(f) = &self.flames {
            let ramp = self.ramp_or(&f.ramp).to_vec();
            let flame = ParticleDesc {
                rate: f.rate * p,
                lifetime: f.life,
                max_particles: 400,
                position: Vec3::new(0.0, h * 0.5, 0.0),
                area: Vec3::splat(r * 0.3),
                body: 1.0,
                spread: 10.0,
                speed: (0.1, 0.4),
                acceleration: Vec3::Y * f.rise * p.sqrt(),
                attract: 0.0,
                orbit: f.spiral,
                drag: 1.2,
                curl: f.sway,
                curl_scale: 0.35,
                sizes: vec![f.size * 0.5, f.size, f.size * 0.75, f.size * 0.2],
                size_jitter: 0.35,
                upright: f.tall,
                shape: f.shape,
                colors: vec![
                    Vec4::new(1.0, 1.0, 1.0, f.opacity),
                    Vec4::new(1.0, 1.0, 1.0, f.opacity),
                    Vec4::new(1.0, 1.0, 1.0, f.opacity * 0.6),
                ],
                ramp: ramp.clone(),
                erode: vec![0.0, 0.05, 0.3, 0.85],
                noise: if f.shape == ParticleShape::Flame { FIRE_REPEATS } else { 1.3 },
                flow: if f.shape == ParticleShape::Flame { FIRE_FLOW } else { Vec2::new(0.05, -1.5) },
                warp: f.warp,
                grain: f.grain,
                bands: if f.shape == ParticleShape::Flame { 0.0 } else { bands },
                pixels,
                soft: 0.1,
                emissive: 1.0,
                additive: f.additive,
                ..base.clone()
            };
            if f.core > 0.0 {
                // Hotter and tighter, close to the skin: the heat comes from within.
                let hot: Vec<Vec4> = ramp.iter().map(|c| Vec4::new(c.x, c.y, c.z, c.w * 1.6)).skip(1).collect();
                out.push(ParticleDesc {
                    rate: f.rate * p * f.core,
                    lifetime: (f.life.0 * 0.6, f.life.1 * 0.6),
                    area: Vec3::splat(r * 0.12),
                    sizes: vec![f.size * 0.3, f.size * 0.6, f.size * 0.2],
                    ramp: if hot.len() >= 2 { hot } else { ramp.clone() },
                    erode: vec![0.0, 0.1, 0.75],
                    additive: (f.additive + 0.2).min(1.0),
                    seed: 17,
                    ..flame.clone()
                });
            }
            out.push(flame);
        }

        if let Some(s) = &self.streaks {
            let color = s.color.map_or(self.heart(), rgb);
            out.push(ParticleDesc {
                rate: s.rate * p,
                lifetime: (0.25, 0.45),
                position: Vec3::new(0.0, h * 0.1, 0.0),
                area: Vec3::splat(r * 0.35),
                body: 0.85,
                spread: 3.0,
                speed: s.speed,
                acceleration: Vec3::Y * 4.0,
                sizes: vec![s.width, s.width, s.width * 0.3],
                stretch: s.length,
                trail: s.trail,
                trail_points: 8,
                shape: ParticleShape::Diamond,
                colors: vec![color.with_w(0.0), color, color.with_w(0.8), color.with_w(0.0)],
                emissive: s.glow,
                additive: 1.0,
                local: follow.max(0.95),
                ..base.clone()
            });
        }

        if let Some(s) = &self.sparks {
            let heart = self.heart();
            out.push(ParticleDesc {
                rate: s.rate * p,
                lifetime: s.life,
                position: Vec3::new(0.0, h * 0.45, 0.0),
                area: Vec3::splat(r * 0.3),
                body: 1.0,
                speed: s.speed,
                acceleration: Vec3::Y * s.rise,
                drag: 0.9,
                curl: s.curl,
                curl_scale: 0.4,
                sizes: vec![s.size, s.size * 0.8, s.size * 0.3],
                size_jitter: 0.4,
                colors: vec![heart, self.hue(), self.hue().with_w(0.0)],
                tints: s.tints.clone(),
                shape: s.shape,
                stretch: s.stretch,
                twinkle: s.twinkle,
                spin: s.spin,
                emissive: s.glow,
                additive: 1.0,
                local: follow * 0.5,
                windy: 0.3,
                ..base.clone()
            });
        }

        if let Some(b) = &self.bolts {
            let color = b.color.map_or(self.heart(), rgb);
            let index = out.len() as u32;
            out.push(ParticleDesc {
                rate: b.rate * p,
                lifetime: (0.08, 0.2),
                position: Vec3::new(0.0, h * 0.15, 0.0),
                area: Vec3::new(0.0, h * 0.35, 0.0),
                body: 1.0,
                ring: r * (b.reach - 1.0).max(0.0) + 0.05,
                sizes: vec![b.size, b.size * 1.1],
                size_jitter: 0.4,
                upright: 1.5,
                shape: ParticleShape::Bolt,
                colors: vec![color, color, color.with_w(0.0)],
                emissive: b.glow,
                additive: 1.0,
                local: 1.0,
                ..base.clone()
            });
            if b.sparks > 0 {
                out.push(ParticleDesc {
                    rate: 0.0,
                    burst: b.sparks,
                    from: Some(index),
                    when: Spawn::Death,
                    lifetime: (0.15, 0.35),
                    speed: (1.0, 3.0),
                    acceleration: Vec3::Y * -6.0,
                    drag: 1.5,
                    sizes: vec![0.02, 0.0],
                    stretch: 0.03,
                    shape: ParticleShape::Diamond,
                    colors: vec![color, color.with_w(0.0)],
                    emissive: b.glow,
                    additive: 1.0,
                    ..base.clone()
                });
            }
        }

        if let Some(o) = &self.orbs {
            let color = o.color.map_or(self.heart(), rgb);
            let radius = h * o.distance;
            // A circle holds when the pull is v² / r: see `ParticleDesc::orbit`.
            let drag = 2.0;
            out.push(ParticleDesc {
                rate: o.count / 2.0,
                lifetime: (1.6, 2.4),
                position: Vec3::new(0.0, h * 0.5, 0.0),
                area: Vec3::new(0.0, h * 0.3 * o.wander, 0.0),
                ring: radius,
                speed: (0.0, 0.3),
                orbit: o.speed * drag,
                attract: o.speed * o.speed / radius.max(0.05),
                drag,
                turbulence: o.wander * 2.0,
                sizes: vec![0.0, o.size, o.size, 0.0],
                trail: o.trail,
                trail_points: 14,
                shape: o.shape,
                spin: 90.0,
                colors: vec![color.with_w(0.0), color, color, color.with_w(0.0)],
                twinkle: 0.3,
                emissive: o.glow,
                additive: 1.0,
                local: 1.0,
                ..base.clone()
            });
        }

        if let Some(g) = &self.ground {
            let color = g.color.map_or(self.hue(), rgb);
            if g.runes > 0.0 {
                out.push(ParticleDesc {
                    rate: 1.0,
                    lifetime: (2.0, 2.0),
                    position: Vec3::new(0.0, 0.04, 0.0),
                    flat: true,
                    shape: ParticleShape::Runes,
                    spin: g.runes_spin,
                    sizes: vec![g.runes * 0.97, g.runes, g.runes * 1.02],
                    colors: fade(1.0),
                    ramp: self.ramp.clone(),
                    erode: vec![0.4, 0.0, 0.0, 0.5],
                    noise: 3.0,
                    grain: 0.4,
                    pixels: pixels * 0.3,
                    emissive: 1.0,
                    additive: 1.0,
                    local: 1.0,
                    ..base.clone()
                });
            }
            if g.pulse > 0.0 {
                out.push(ParticleDesc {
                    rate: 0.0,
                    burst: 1,
                    repeat: g.pulse,
                    lifetime: (0.9, 0.9),
                    position: Vec3::new(0.0, 0.05, 0.0),
                    flat: true,
                    shape: ParticleShape::Wave,
                    sizes: vec![0.4, g.pulse_size * 0.7, g.pulse_size * 0.9, g.pulse_size],
                    colors: vec![color, color.with_w(0.7), color.with_w(0.25), color.with_w(0.0)],
                    ramp: self.ramp.clone(),
                    erode: vec![0.0, 0.2, 0.6, 1.4],
                    noise: 2.0,
                    grain: 0.5,
                    bands,
                    pixels: pixels * 0.3,
                    emissive: 1.0,
                    additive: 1.0,
                    local: 1.0,
                    ..base.clone()
                });
            }
            if g.dust > 0.0 {
                out.push(ParticleDesc {
                    rate: g.dust * p,
                    lifetime: (0.7, 1.3),
                    position: Vec3::new(0.0, 0.08, 0.0),
                    ring: r * 1.2,
                    outward: 1.0,
                    spread: 10.0,
                    direction: Vec3::Y,
                    speed: (1.5, 3.5),
                    acceleration: Vec3::Y * 0.3,
                    drag: 2.5,
                    curl: 0.6,
                    sizes: vec![0.15, 0.45, 0.7],
                    size_jitter: 0.3,
                    colors: fade(g.dust_color.w).iter().map(|c| rgb(g.dust_color) * *c).collect(),
                    noise: 1.2,
                    grain: 0.6,
                    erode: vec![0.2, 0.1, 0.4, 0.95],
                    pixels: pixels * 0.6,
                    lit: 1.0,
                    soft: 0.3,
                    local: 0.0,
                    windy: 0.6,
                    ..base.clone()
                });
            }
        }

        if let Some(d) = &self.debris {
            out.push(ParticleDesc {
                rate: d.rate * p,
                lifetime: (1.8, 3.2),
                position: Vec3::new(0.0, 0.05, 0.0),
                area: Vec3::new(h * d.distance * 0.3, 0.0, h * d.distance * 0.3),
                ring: h * d.distance * 0.7,
                spread: 15.0,
                speed: (0.2, 0.6),
                acceleration: Vec3::Y * d.rise,
                turbulence: 0.3,
                sizes: vec![d.size * 0.6, d.size, d.size, d.size * 0.5],
                size_jitter: 0.5,
                shape: ParticleShape::Shard,
                spin: 120.0,
                colors: vec![rgb(d.color).with_w(0.0), rgb(d.color), rgb(d.color), rgb(d.color).with_w(0.0)],
                lit: 1.0,
                emissive: 0.0,
                local: 0.0,
                ..base.clone()
            });
        }

        if self.haze > 0.0 {
            out.push(ParticleDesc {
                rate: 10.0,
                lifetime: (0.6, 1.0),
                position: Vec3::new(0.0, h * 0.6, 0.0),
                area: Vec3::new(r * 0.5, h * 0.3, r * 0.5),
                speed: (0.4, 0.9),
                spread: 10.0,
                sizes: vec![0.4, 0.7, 0.9],
                colors: vec![Vec4::new(1.0, 1.0, 1.0, 0.0), Vec4::ONE, Vec4::new(1.0, 1.0, 1.0, 0.0)],
                noise: 0.8,
                flow: Vec2::new(0.0, -1.0),
                soft: 0.1,
                distort: self.haze,
                max_distance: 30.0,
                ..base.clone()
            });
        }

        out.into()
    }

    /// The fire in one hand, or `None` if this aura has none.
    pub fn hand_effect(&self) -> Option<ParticleEffect> {
        let hands = self.hands.as_ref()?;
        let ramp = self.ramp_or(&hands.ramp).to_vec();
        let s = hands.size;
        let mut out = vec![ParticleDesc {
            rate: hands.rate * self.power,
            lifetime: (0.2, 0.4),
            area: Vec3::splat(s * 0.2),
            spread: 25.0,
            speed: (0.05, 0.25),
            acceleration: Vec3::Y * 1.5,
            drag: 1.5,
            curl: 0.6,
            curl_scale: 0.2,
            sizes: vec![s * 0.6, s, s * 0.3],
            size_jitter: 0.3,
            upright: 0.9,
            shape: ParticleShape::Flame,
            ramp,
            erode: vec![0.0, 0.1, 0.8],
            noise: FIRE_REPEATS * 1.6,
            flow: FIRE_FLOW,
            warp: 0.35,
            grain: 0.6,
            bands: 0.0,
            pixels: self.pixels * 1.3,
            emissive: 1.0,
            additive: 0.8,
            local: 1.0,
            max_distance: 60.0,
            ..Default::default()
        }];
        if hands.sparks > 0.0 {
            let heart = self.heart();
            out.push(ParticleDesc {
                rate: hands.sparks,
                lifetime: (0.3, 0.7),
                area: Vec3::splat(s * 0.3),
                speed: (0.2, 0.8),
                acceleration: Vec3::Y * 1.0,
                drag: 1.0,
                curl: 1.5,
                curl_scale: 0.3,
                sizes: vec![0.02, 0.0],
                colors: vec![heart, self.hue().with_w(0.0)],
                shape: ParticleShape::Diamond,
                twinkle: 0.6,
                emissive: 8.0,
                additive: 1.0,
                local: 0.5,
                max_distance: 60.0,
                ..Default::default()
            });
        }
        Some(out.into())
    }

    /// The one-shot flash of the aura lighting, or `None`.
    pub fn burst_effect(&self) -> Option<ParticleEffect> {
        let b = self.burst.as_ref()?;
        let (h, r) = (self.height, self.radius);
        let color = b.color.map_or(self.heart(), rgb);
        let hue = self.hue();
        let mut out = vec![
            // a sphere of light that swells past the body and is gone
            ParticleDesc {
                rate: 0.0,
                burst: 1,
                lifetime: (0.45, 0.45),
                position: Vec3::new(0.0, h * 0.5, 0.0),
                sizes: vec![h * 0.3, h * 1.6, h * 2.0],
                colors: vec![color.with_w(0.7), hue.with_w(0.35), hue.with_w(0.0)],
                emissive: 3.0,
                additive: 1.0,
                soft: 0.6,
                light: Some(ParticleLight {
                    color: hue.truncate(),
                    intensity: b.flash,
                    radius: 14.0,
                    flicker: 0.0,
                    linger: 0.5,
                }),
                ..Default::default()
            },
            // the shockwave across the ground
            ParticleDesc {
                rate: 0.0,
                burst: 1,
                lifetime: (0.8, 0.8),
                position: Vec3::new(0.0, 0.06, 0.0),
                flat: true,
                shape: ParticleShape::Wave,
                sizes: vec![0.5, b.wave * 0.75, b.wave * 0.95, b.wave],
                colors: vec![color, hue.with_w(0.8), hue.with_w(0.3), hue.with_w(0.0)],
                ramp: self.ramp.clone(),
                erode: vec![0.0, 0.15, 0.6, 1.4],
                noise: 2.0,
                grain: 0.5,
                bands: self.bands,
                pixels: self.pixels * 0.3,
                emissive: 1.0,
                additive: 1.0,
                ..Default::default()
            },
        ];
        if b.streaks > 0 {
            out.push(ParticleDesc {
                rate: 0.0,
                burst: b.streaks,
                lifetime: (0.25, 0.5),
                position: Vec3::new(0.0, h * 0.5, 0.0),
                shell: r,
                outward: 1.0,
                speed: (6.0, 12.0),
                drag: 3.0,
                sizes: vec![0.05, 0.03, 0.0],
                stretch: 0.05,
                shape: ParticleShape::Diamond,
                colors: vec![color, hue, hue.with_w(0.0)],
                emissive: 9.0,
                additive: 1.0,
                ..Default::default()
            });
        }
        Some(out.into())
    }
}

/// Reads an aura file.
pub fn load_aura(path: impl AsRef<Path>) -> Result<AuraDesc> {
    let path = path.as_ref();
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let options = ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
    options.from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_aura_in_the_assets_loads() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/auras");
        let mut count = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let aura = super::load_aura(&path).unwrap_or_else(|e| panic!("{e:#}"));
            assert!(!aura.effect().is_empty(), "{}", path.display());
            count += 1;
        }
        assert!(count > 0);
    }
}
