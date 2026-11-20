//! The wind: how hard and which way it blows, changing through the day and with the weather,
//! and gusts that travel with it across the land.
//!
//! The gusts are a pattern of stronger and weaker air carried along by the mean wind (frozen
//! turbulence): at a point they come and go as the pattern passes, and turning the wind only
//! turns where the pattern goes, so nothing jumps. [`gust_at`] is the pattern on the CPU, and
//! `gust_at` in `scene.wgsl` the same on the GPU: change both together.

use glam::{DVec2, Vec2};
use serde::{Deserialize, Serialize};

use crate::environment::Wind;

/// Meters after which the gust pattern repeats, both ways. Its waves fit it a whole number of times.
pub const GUST_PERIOD: f64 = 256.0;
/// The gust pattern's waves, in whole turns per [`GUST_PERIOD`] (x, z), and their weight and phase.
pub const GUST_WAVES: [(f32, f32, f32, f32); 3] = [(5.0, 2.0, 0.5, 0.0), (-3.0, 7.0, 0.3, 1.3), (11.0, -4.0, 0.2, 2.1)];
/// Seconds over which the wind settles to what it is heading for.
const SETTLE: f32 = 15.0;
/// Seconds of the slow rise and fall of the wind, and of its turning.
const SWELL_SECONDS: f64 = 240.0;
const TURN_SECONDS: f64 = 1500.0;
/// A stormy weather's wind (as a multiple of the scene's, from the day cycle) from which the
/// wind turns more and faster, and never quite drops.
const STORMY: f32 = 1.5;
/// How the weather's multiple grows into m/s: storms blow much harder than their 2.2.
const WEATHER_POWER: f32 = 1.8;
/// Least pace of the clouds and the mist, as a share of the scene's wind: high up it never stops.
const LEAST_ALOFT: f32 = 0.15;

/// How the wind of a scene changes: saved with the world.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindState {
    /// Seconds of world time it has run.
    pub seconds: f64,
    /// How hard it blows now, in m/s, before gusts.
    pub speed: f32,
    /// How far the air has carried the gusts, in meters (world, not the origin's).
    pub carried: [f64; 2],
}

impl Default for WindState {
    fn default() -> Self {
        Self { seconds: 0.0, speed: -1.0, carried: [0.0, 0.0] }
    }
}

/// Runs the wind of one scene. [`WindSim::update`] writes it into [`Wind`] each frame.
#[derive(Debug, Clone)]
pub struct WindSim {
    pub state: WindState,
    seed: u64,
    /// Held by hand (debug menu): m/s and degrees from +X toward +Z.
    pub held: Option<(f32, f32)>,
}

impl Default for WindSim {
    fn default() -> Self {
        Self::new(0x77696e64)
    }
}

impl WindSim {
    pub fn new(seed: u64) -> Self {
        Self { state: WindState::default(), seed, held: None }
    }

    pub fn set_seed(&mut self, seed: u64) {
        self.seed = seed;
    }

    /// Moves the wind on by `dt` world seconds. `wind.weather` is the weather's multiple of the
    /// scene's wind (the day cycle sets it), `wind.strength` and `wind.direction` the scene's.
    /// `origin` is where the world's origin is (x, z), for the gusts' place in local space.
    pub fn update(&mut self, wind: &mut Wind, origin: DVec2, dt: f32) {
        let base = (wind.strength * crate::particles::WIND_SPEED).max(0.05);
        let weather = wind.weather.max(0.0);
        let t = self.state.seconds;
        let stormy = ((weather - 1.0) / (STORMY - 1.0)).clamp(0.0, 1.0);
        // How hard it heads for: lulls and blows of a few minutes, calms now and then in
        // fair weather, never in a storm.
        let swell = smooth_noise(self.seed, t / SWELL_SECONDS);
        let floor = 0.12 + 0.55 * stormy;
        let target = base * weather.powf(WEATHER_POWER) * (floor + (1.25 - floor) * swell);
        // Which way: around the scene's, wandering over half an hour, faster and wider in a storm.
        let prevailing = wind.direction.y.atan2(wind.direction.x);
        let reach = wind.wander.to_radians() * (1.0 + stormy);
        let turn =
            smooth_noise(self.seed ^ 0x7475726e, t / (TURN_SECONDS / (1.0 + 2.0 * f64::from(stormy)))) * 2.0 - 1.0;
        let mut angle = prevailing + reach * turn;
        let mut speed = if self.state.speed < 0.0 { target } else { self.state.speed };
        if let Some((held, degrees)) = self.held {
            speed = held.max(0.0);
            angle = degrees.to_radians();
        } else {
            speed += (target - speed) * (1.0 - (-dt / SETTLE).exp());
        }
        let way = Vec2::new(angle.cos(), angle.sin());
        self.state.speed = speed;
        self.state.seconds += f64::from(dt);
        let carried = DVec2::from(self.state.carried) + way.as_dvec2() * f64::from(speed * dt);
        self.state.carried = carried.to_array();

        wind.blowing = way;
        wind.speed_ms = speed;
        // The old multiple, for what still reads it: `strength * scale` is m/s over WIND_SPEED.
        wind.scale = speed / base;
        wind.gustiness = (0.3 + 0.18 * stormy) * wind.gusty;
        let aloft = (speed / base).max(LEAST_ALOFT);
        wind.aloft += way * aloft * dt;
        wind.flutter += dt * wind.speed * (0.5 + 0.5 * (speed / base).min(4.0));
        let shift = (origin - carried).rem_euclid(DVec2::splat(GUST_PERIOD));
        wind.gust_shift = shift.as_vec2();
    }
}

/// The gust pattern at a point in local space, 0 to 1, with `shift` from [`Wind::gust_shift`].
pub fn gust_pattern(local: Vec2, shift: Vec2) -> f32 {
    let q = (local + shift) * (std::f32::consts::TAU / GUST_PERIOD as f32);
    let mut sum = 0.0;
    for (x, z, weight, phase) in GUST_WAVES {
        sum += weight * (q.x * x + q.y * z + phase).sin();
    }
    0.5 + 0.5 * sum
}

/// How many times the mean wind blows at a point (local x, z): above 1 in a gust, below in a lull.
pub fn gust_at(wind: &Wind, local: Vec2) -> f32 {
    1.0 + wind.gustiness * (2.0 * gust_pattern(local, wind.gust_shift) - 1.0) * 1.6
}

/// Smooth noise from 0 to 1 along `x`, the same for the same seed.
fn smooth_noise(seed: u64, x: f64) -> f32 {
    let i = x.floor();
    let f = (x - i) as f32;
    let at = |k: f64| {
        let mut h = seed ^ (k as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        h ^= h >> 31;
        h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        h ^= h >> 29;
        (h >> 40) as f32 / (1u64 << 24) as f32
    };
    let s = f * f * (3.0 - 2.0 * f);
    at(i) + (at(i + 1.0) - at(i)) * s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wind_turns_and_changes_without_jumping() {
        let mut sim = WindSim::new(7);
        let mut wind = Wind { weather: 1.0, ..Default::default() };
        let mut last = None;
        let (mut least, mut most) = (f32::MAX, 0.0f32);
        for _ in 0..(3600 * 10) {
            sim.update(&mut wind, DVec2::new(5000.0, -200.0), 0.1);
            if let Some((speed, way)) = last {
                assert!((wind.speed_ms - speed as f32).abs() < 0.05, "speed jumped");
                assert!(wind.blowing.dot(way) > 0.9999, "direction jumped");
            }
            last = Some((wind.speed_ms, wind.blowing));
            least = least.min(wind.speed_ms);
            most = most.max(wind.speed_ms);
        }
        assert!(most > least * 2.0, "fair weather has lulls and blows: {least}..{most}");
    }

    #[test]
    fn storms_blow_harder() {
        let run = |weather: f32| {
            let mut sim = WindSim::new(3);
            let mut wind = Wind { weather, ..Default::default() };
            let mut sum = 0.0;
            for _ in 0..20000 {
                sim.update(&mut wind, DVec2::ZERO, 0.5);
                sum += wind.speed_ms;
            }
            sum / 20000.0
        };
        let (fair, storm) = (run(1.0), run(2.2));
        assert!(storm > fair * 3.0 && storm > 8.0, "fair {fair} m/s, storm {storm} m/s");
    }
}
