//! The clock of the world: where the sun and moon are and what the sky, light
//! and fog look like at each hour. [`DayCycle::apply`] writes that into an
//! [`Environment`], leaving alone what the hours don't touch (shadows, most of
//! post-processing...).

use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::environment::Environment;

const HOURS: f32 = 24.0;
/// The light never comes from lower than this (sine of about 8 degrees): near
/// the horizon, shadows would stretch across the whole map and shimmer.
const MIN_LIGHT_ELEVATION: f32 = 0.14;
/// The sun lights the world from this elevation (sine) up, fading in over
/// `LIGHT_FADE`. The moon only comes on as the sun gets as far below the horizon,
/// so at the swap both are dark.
const LIGHT_HORIZON: f32 = 0.01;
const LIGHT_FADE: f32 = 0.1;

/// How a scene's day runs (`day_cycle:` in the scene file).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DayCycleDesc {
    /// The hour the scene starts at, from 0 to 24.
    pub time: f32,
    /// Real seconds a whole day takes. 0 stops the clock.
    pub day_length: f32,
    /// Where the sun rises, in degrees around the vertical: 0 is +X, 90 is -Z.
    pub sunrise: f32,
    /// How far from straight overhead the sun passes at noon, in degrees.
    pub tilt: f32,
    /// What each hour looks like, interpolated between them. Empty uses
    /// [`DayKey::defaults`], a dark fantasy day.
    pub keys: Vec<DayKey>,
}

impl Default for DayCycleDesc {
    fn default() -> Self {
        Self { time: 9.0, day_length: 600.0, sunrise: 20.0, tilt: 35.0, keys: Vec::new() }
    }
}

/// The look of one hour. Colors are sRGB in `0..=1`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DayKey {
    pub hour: f32,
    pub sun_color: Vec3,
    pub sun_intensity: f32,
    pub moon_color: Vec3,
    pub moon_intensity: f32,
    pub ambient_sky: Vec3,
    pub ambient_ground: Vec3,
    pub ambient_intensity: f32,
    pub fog_color: Vec3,
    pub fog_density: f32,
    pub zenith: Vec3,
    pub horizon: Vec3,
    pub ground: Vec3,
    pub exposure: f32,
    /// How visible the stars are, from 0 to 1.
    pub stars: f32,
}

impl Default for DayKey {
    fn default() -> Self {
        Self::defaults().swap_remove(3)
    }
}

impl DayKey {
    /// A day under heavy skies: cold nights, long red dawns and dusks, and even noon a bit grey.
    pub fn defaults() -> Vec<Self> {
        let c = Vec3::new;
        let night = Self {
            hour: 0.0,
            sun_color: c(1.0, 0.5, 0.3),
            sun_intensity: 0.0,
            moon_color: c(0.62, 0.7, 1.0),
            moon_intensity: 0.75,
            ambient_sky: c(0.2, 0.25, 0.42),
            ambient_ground: c(0.07, 0.07, 0.12),
            ambient_intensity: 0.65,
            fog_color: c(0.07, 0.09, 0.16),
            fog_density: 0.012,
            zenith: c(0.012, 0.018, 0.05),
            horizon: c(0.07, 0.09, 0.16),
            ground: c(0.03, 0.03, 0.05),
            exposure: 1.3,
            stars: 1.0,
        };
        let dawn = Self {
            hour: 6.2,
            sun_color: c(1.0, 0.52, 0.34),
            sun_intensity: 1.7,
            moon_intensity: 0.0,
            ambient_sky: c(0.38, 0.36, 0.5),
            ambient_ground: c(0.2, 0.15, 0.15),
            ambient_intensity: 0.55,
            fog_color: c(0.5, 0.37, 0.38),
            fog_density: 0.02,
            zenith: c(0.11, 0.15, 0.32),
            horizon: c(0.86, 0.5, 0.38),
            ground: c(0.2, 0.15, 0.16),
            exposure: 1.25,
            stars: 0.1,
            ..night.clone()
        };
        let morning = Self {
            hour: 9.0,
            sun_color: c(1.0, 0.84, 0.66),
            sun_intensity: 2.7,
            ambient_sky: c(0.46, 0.5, 0.62),
            ambient_ground: c(0.25, 0.2, 0.18),
            ambient_intensity: 0.6,
            fog_color: c(0.58, 0.59, 0.62),
            fog_density: 0.01,
            zenith: c(0.21, 0.33, 0.55),
            horizon: c(0.68, 0.66, 0.66),
            ground: c(0.28, 0.27, 0.28),
            exposure: 1.1,
            stars: 0.0,
            ..dawn.clone()
        };
        let noon = Self {
            hour: 13.0,
            sun_color: c(1.0, 0.92, 0.8),
            sun_intensity: 3.1,
            ambient_sky: c(0.5, 0.55, 0.66),
            zenith: c(0.24, 0.38, 0.6),
            horizon: c(0.72, 0.72, 0.73),
            exposure: 1.05,
            ..morning.clone()
        };
        let afternoon = Self {
            hour: 16.5,
            sun_color: c(1.0, 0.72, 0.5),
            sun_intensity: 2.5,
            ambient_sky: c(0.42, 0.42, 0.56),
            ambient_ground: c(0.24, 0.18, 0.17),
            fog_color: c(0.5, 0.44, 0.48),
            fog_density: 0.013,
            zenith: c(0.14, 0.17, 0.34),
            horizon: c(0.7, 0.54, 0.46),
            exposure: 1.2,
            ..noon.clone()
        };
        let dusk = Self {
            hour: 17.9,
            sun_color: c(1.0, 0.46, 0.3),
            sun_intensity: 1.6,
            ambient_sky: c(0.34, 0.33, 0.52),
            ambient_ground: c(0.2, 0.15, 0.16),
            ambient_intensity: 0.55,
            fog_color: c(0.36, 0.28, 0.38),
            fog_density: 0.018,
            zenith: c(0.07, 0.08, 0.2),
            horizon: c(0.74, 0.38, 0.32),
            ground: c(0.15, 0.13, 0.17),
            exposure: 1.25,
            stars: 0.15,
            ..afternoon.clone()
        };
        let evening = Self { hour: 19.8, moon_intensity: 0.75, ..night.clone() };
        let before_dawn = Self {
            hour: 5.1,
            fog_color: c(0.14, 0.14, 0.22),
            horizon: c(0.2, 0.17, 0.25),
            zenith: c(0.02, 0.03, 0.09),
            stars: 0.7,
            moon_intensity: 0.5,
            ..night.clone()
        };
        vec![night, before_dawn, dawn, morning, noon, afternoon, dusk, evening]
    }

    fn lerp(&self, other: &Self, t: f32) -> Self {
        let num = |a: f32, b: f32| a + (b - a) * t;
        Self {
            hour: self.hour,
            sun_color: self.sun_color.lerp(other.sun_color, t),
            sun_intensity: num(self.sun_intensity, other.sun_intensity),
            moon_color: self.moon_color.lerp(other.moon_color, t),
            moon_intensity: num(self.moon_intensity, other.moon_intensity),
            ambient_sky: self.ambient_sky.lerp(other.ambient_sky, t),
            ambient_ground: self.ambient_ground.lerp(other.ambient_ground, t),
            ambient_intensity: num(self.ambient_intensity, other.ambient_intensity),
            fog_color: self.fog_color.lerp(other.fog_color, t),
            fog_density: num(self.fog_density, other.fog_density),
            zenith: self.zenith.lerp(other.zenith, t),
            horizon: self.horizon.lerp(other.horizon, t),
            ground: self.ground.lerp(other.ground, t),
            exposure: num(self.exposure, other.exposure),
            stars: num(self.stars, other.stars),
        }
    }
}

/// A running day. The engine advances it with the world clock, so pausing stops it.
#[derive(Debug, Clone, PartialEq)]
pub struct DayCycle {
    /// Hours since midnight, from 0 to 24.
    hour: f32,
    /// Real seconds a whole day takes. 0 stops the clock.
    pub day_length: f32,
    east: Vec3,
    noon: Vec3,
    /// Sorted by hour.
    keys: Vec<DayKey>,
}

impl DayCycle {
    pub fn new(desc: &DayCycleDesc) -> Self {
        let mut keys = if desc.keys.is_empty() { DayKey::defaults() } else { desc.keys.clone() };
        keys.sort_by(|a, b| a.hour.total_cmp(&b.hour));
        let (sin, cos) = desc.sunrise.to_radians().sin_cos();
        let east = Vec3::new(cos, 0.0, -sin);
        let south = Vec3::new(sin, 0.0, cos);
        let (sin, cos) = desc.tilt.clamp(0.0, 80.0).to_radians().sin_cos();
        let mut day =
            Self { hour: 0.0, day_length: desc.day_length.max(0.0), east, noon: Vec3::Y * cos + south * sin, keys };
        day.set_hour(desc.time);
        day
    }

    pub fn hour(&self) -> f32 {
        self.hour
    }

    /// Moves the clock to `hour`; values past 24 wrap around.
    pub fn set_hour(&mut self, hour: f32) {
        self.hour = hour.rem_euclid(HOURS);
    }

    /// Runs the clock for `dt` real seconds.
    pub fn advance(&mut self, dt: f32) {
        if self.day_length > 0.0 {
            self.set_hour(self.hour + dt / self.day_length * HOURS);
        }
    }

    /// From the ground towards the sun. It rises at 6, peaks at 12 and sets at 18.
    pub fn sun_position(&self) -> Vec3 {
        let (sin, cos) = ((self.hour - 6.0) / HOURS * std::f32::consts::TAU).sin_cos();
        (self.east * cos + self.noon * sin).normalize_or(Vec3::Y)
    }

    /// From the ground towards the moon: across the sky from the sun, a little off.
    pub fn moon_position(&self) -> Vec3 {
        (-self.sun_position() + self.noon.cross(self.east) * 0.18).normalize_or(Vec3::Y)
    }

    /// How much the sun is lighting the world, from 0 at night to 1 once it is up.
    pub fn daylight(&self) -> f32 {
        light_fade(self.sun_position().y)
    }

    /// Whether the sun has set, for whatever comes out at night.
    pub fn is_night(&self) -> bool {
        self.daylight() <= 0.0
    }

    /// The hour's look, between the two keys around it.
    pub fn key(&self) -> DayKey {
        let keys = &self.keys;
        let Some(first) = keys.first() else { return DayKey::default() };
        let after = keys.iter().position(|key| key.hour > self.hour);
        let (a, b, span, into) = match after {
            Some(0) | None => {
                // Around midnight: from the last key of the day to the first of the next.
                let last = &keys[keys.len() - 1];
                let span = first.hour + HOURS - last.hour;
                let into = if self.hour >= last.hour { self.hour - last.hour } else { self.hour + HOURS - last.hour };
                (last, first, span, into)
            }
            Some(i) => (&keys[i - 1], &keys[i], keys[i].hour - keys[i - 1].hour, self.hour - keys[i - 1].hour),
        };
        let t = if span > 0.0 { (into / span).clamp(0.0, 1.0) } else { 0.0 };
        a.lerp(b, t * t * (3.0 - 2.0 * t))
    }

    /// Writes this hour into `env`. The directional light is the sun by day and
    /// the moon by night, both fading out near the horizon, where they swap.
    pub fn apply(&self, env: &mut Environment) {
        let key = self.key();
        let (sun, moon) = (self.sun_position(), self.moon_position());
        let sun_light = light_fade(sun.y);
        let (light, color, intensity) = if sun_light > 0.0 {
            (sun, key.sun_color, key.sun_intensity * sun_light)
        } else {
            (moon, key.moon_color, key.moon_intensity * light_fade(-sun.y) * light_fade(moon.y))
        };
        env.sun.direction = Vec3::new(light.x, light.y.max(MIN_LIGHT_ELEVATION), light.z).normalize_or(Vec3::Y);
        env.sun.color = color;
        env.sun.intensity = intensity;
        env.ambient.sky = key.ambient_sky;
        env.ambient.ground = key.ambient_ground;
        env.ambient.intensity = key.ambient_intensity;
        env.fog.color = key.fog_color;
        env.fog.density = key.fog_density;
        env.sky.zenith = key.zenith;
        env.sky.horizon = key.horizon;
        env.sky.ground = key.ground;
        env.sky.stars = key.stars;
        env.sky.sun_position = Some(sun);
        env.sky.moon_position = Some(moon);
        env.sky.light_position = Some(light);
        env.post.exposure = key.exposure;
    }
}

fn light_fade(elevation: f32) -> f32 {
    let t = ((elevation - LIGHT_HORIZON) / LIGHT_FADE).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day_at(hour: f32) -> DayCycle {
        DayCycle::new(&DayCycleDesc { time: hour, ..Default::default() })
    }

    #[test]
    fn the_sun_rises_peaks_and_sets_on_time() {
        assert!(day_at(6.0).sun_position().y.abs() < 1e-5);
        assert!(day_at(18.0).sun_position().y.abs() < 1e-5);
        assert!(day_at(12.0).sun_position().y > 0.7);
        assert!(day_at(0.0).sun_position().y < -0.7);
        assert!(day_at(9.0).sun_position().y > 0.0 && day_at(21.0).sun_position().y < 0.0);
        assert!(day_at(0.0).moon_position().y > 0.6);
        assert!(day_at(12.0).moon_position().y < -0.6);
        // Rises at the east of the description, sets at the opposite side.
        let east = day_at(6.0).sun_position();
        assert!(east.dot(day_at(18.0).sun_position()) < -0.99);
    }

    #[test]
    fn the_clock_runs_and_wraps() {
        let mut day = DayCycle::new(&DayCycleDesc { time: 23.0, day_length: 240.0, ..Default::default() });
        day.advance(10.0);
        assert!(day.hour().abs() < 1e-3 || (day.hour() - 24.0).abs() < 1e-3);
        day.advance(30.0);
        assert!((day.hour() - 3.0).abs() < 1e-3);
        day.day_length = 0.0;
        day.advance(1000.0);
        assert!((day.hour() - 3.0).abs() < 1e-3);
        day.set_hour(-1.0);
        assert!((day.hour() - 23.0).abs() < 1e-3);
    }

    #[test]
    fn keys_interpolate_and_wrap_around_midnight() {
        let at = |hour, stars| DayKey { hour, stars, ..Default::default() };
        let desc = DayCycleDesc { keys: vec![at(18.0, 1.0), at(6.0, 0.0)], time: 6.0, ..Default::default() };
        let mut day = DayCycle::new(&desc);
        assert_eq!(day.key().stars, 0.0);
        day.set_hour(12.0);
        assert!((day.key().stars - 0.5).abs() < 1e-5);
        day.set_hour(18.0);
        assert_eq!(day.key().stars, 1.0);
        day.set_hour(0.0);
        assert!((day.key().stars - 0.5).abs() < 1e-5, "{}", day.key().stars);
        day.set_hour(3.0);
        assert!(day.key().stars < 0.5 && day.key().stars > 0.0);
    }

    #[test]
    fn night_and_day_differ_and_light_does_not_jump() {
        let mut env = Environment::default();
        day_at(12.0).apply(&mut env);
        let noon = env.clone();
        day_at(0.0).apply(&mut env);
        assert!(noon.sun.intensity > env.sun.intensity * 2.0);
        assert!(noon.ambient.sky.y > env.ambient.sky.y);
        assert!(env.sky.stars > 0.9 && noon.sky.stars < 0.01);
        assert!(day_at(0.0).is_night() && !day_at(12.0).is_night());
        assert!(env.sun.direction.y > 0.1, "the moon lights from above");

        // Sweeping the whole day, the light never jumps: when the source swaps
        // between sun and moon, it is dark.
        let mut before: Option<Vec3> = None;
        for step in 0..=2400 {
            day_at(step as f32 / 100.0).apply(&mut env);
            assert!(env.sun.direction.y >= 0.13);
            let radiance = env.sun.color * env.sun.intensity;
            if let Some(previous) = before {
                let change = (radiance - previous).length();
                assert!(change < 0.15, "jump of {change} at {}", step as f32 / 100.0);
            }
            before = Some(radiance);
        }
    }

    #[test]
    fn apply_leaves_the_rest_of_the_environment_alone() {
        let mut env = Environment::default();
        env.shadows.distance = 77.0;
        env.post.vignette = 0.9;
        day_at(15.0).apply(&mut env);
        assert_eq!((env.shadows.distance, env.post.vignette), (77.0, 0.9));
    }
}
