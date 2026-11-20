//! Plays what the weather and the water sound like, from the scene's `weather_sounds`
//! and `water_sounds`.

use std::path::Path;

use grimora_audio::{Audio, Emitter, SoundId};
use grimora_core::Vec3;

use crate::World;

const RAIN_VOLUME: f32 = 0.6;
const SNOW_VOLUME: f32 = 0.35;
/// The wind's loop: heard from `WIND_FROM` m/s, full at `WIND_FULL` (gusts included).
const WIND_VOLUME: f32 = 0.6;
const WIND_FROM: f32 = 7.0;
const WIND_FULL: f32 = 18.0;
const THUNDER_VOLUME: f32 = 0.8;
const SEA_VOLUME: f32 = 0.55;
/// `river_small.wav` is mixed at -24 LUFS and `river_big.wav` at -21: the brook stays the
/// quieter of the two at the same volume.
const BROOK_VOLUME: f32 = 0.75;
const RIVER_VOLUME: f32 = 0.7;
const UNDERWATER_VOLUME: f32 = 0.9;
/// How much of the sea and the rivers is still heard from under the water.
const THROUGH_WATER: f32 = 0.2;
/// How far a splash carries, in meters.
const SPLASH_NEAR: f32 = 4.0;
const SPLASH_FAR: f32 = 70.0;

/// The loops of rain and snow, which follow how hard it falls.
#[derive(Debug, Default)]
pub(crate) struct WeatherAudio {
    rain: Loop,
    snow: Loop,
    wind: Loop,
    thunders: u32,
    sea: Loop,
    river: Loop,
    river_big: Loop,
    underwater: Loop,
    splashes: u32,
    /// A generated world's loops by biome, with how loud each is now.
    ambience: Vec<(Loop, f32)>,
}

/// How fast an ambience loop fades, in volume per second.
const AMBIENCE_FADE: f32 = 0.25;

impl WeatherAudio {
    pub(crate) fn update(&mut self, audio: &mut Audio, world: &mut World, camera: Vec3, dt: f32) {
        self.update_water(audio, world, camera);
        self.update_ambience(audio, world, camera, dt);
        let falling = world.environment.precipitation;
        self.rain.follow(audio, world.weather_sounds.rain.as_deref(), falling.rain * RAIN_VOLUME);
        self.snow.follow(audio, world.weather_sounds.snow.as_deref(), falling.snow * SNOW_VOLUME);
        // How hard it blows here, gust and all; the snow's loop already carries a blizzard's hiss.
        let blowing = world.wind_at(camera).length();
        let t = ((blowing - WIND_FROM) / (WIND_FULL - WIND_FROM)).clamp(0.0, 1.0);
        let under = world.environment.water.submerged;
        let wind = t * t * (3.0 - 2.0 * t) * (1.0 - 0.6 * falling.snow) * (1.0 - under);
        self.wind.follow(audio, world.weather_sounds.wind.as_deref(), wind * WIND_VOLUME);
        if world.take_thunder() && !world.weather_sounds.thunder.is_empty() {
            self.thunders += 1;
            let sounds = &world.weather_sounds.thunder;
            // Every recording in turn, in an order that changes each time around, and never
            // quite at the same pitch.
            let turn = self.thunders as usize;
            let pick = (turn * 3 + turn / sounds.len()) % sounds.len();
            let pitch = 0.94 + 0.04 * (turn * 7 % 4) as f32;
            audio.play_pitched(&sounds[pick], THUNDER_VOLUME, pitch);
        }
    }

    pub(crate) fn stop(&mut self, audio: &mut Audio) {
        self.rain.stop(audio);
        self.snow.stop(audio);
        self.wind.stop(audio);
        self.sea.stop(audio);
        self.river.stop(audio);
        self.river_big.stop(audio);
        self.underwater.stop(audio);
        for (sound, _) in &mut self.ambience {
            sound.stop(audio);
        }
        self.ambience.clear();
    }

    fn update_ambience(&mut self, audio: &mut Audio, world: &World, camera: Vec3, dt: f32) {
        let wanted = world.ambience(camera);
        let under = world.environment.water.submerged;
        self.ambience.resize_with(wanted.len(), Default::default);
        for ((sound, level), (path, volume)) in self.ambience.iter_mut().zip(wanted) {
            let target = volume * (1.0 - under);
            let step = AMBIENCE_FADE * dt;
            *level += (target - *level).clamp(-step, step);
            sound.follow(audio, Some(path), *level);
        }
    }

    fn update_water(&mut self, audio: &mut Audio, world: &mut World, camera: Vec3) {
        let under = world.environment.water.submerged;
        let (sea, brook, river) = world.water_noise(camera);
        let through = 1.0 - under * (1.0 - THROUGH_WATER);
        let splashes = world.take_splashes();
        let sounds = &world.water_sounds;
        self.sea.follow(audio, sounds.sea.as_deref(), sea * through * SEA_VOLUME);
        // Without a wide river's own loop, the brook's plays for both.
        let (brook, river) = match sounds.river_big {
            Some(_) => (brook, river),
            None => (brook.max(river), 0.0),
        };
        self.river.follow(audio, sounds.river.as_deref(), brook * through * BROOK_VOLUME);
        self.river_big.follow(audio, sounds.river_big.as_deref(), river * through * RIVER_VOLUME);
        self.underwater.follow(audio, sounds.underwater.as_deref(), under * UNDERWATER_VOLUME);
        if sounds.splash.is_empty() {
            return;
        }
        for (position, strength) in splashes {
            self.splashes += 1;
            let sound = &sounds.splash[self.splashes as usize % sounds.splash.len()];
            let emitter = Emitter { position, min_distance: SPLASH_NEAR, max_distance: SPLASH_FAR };
            audio.play_at(sound, 0.25 + 0.75 * strength, emitter);
        }
    }
}

#[derive(Debug, Default)]
struct Loop {
    id: Option<SoundId>,
    volume: f32,
}

impl Loop {
    /// Starts the loop when it gets audible and stops it when it no longer is. Small
    /// changes wait, so a slow fade doesn't send a command every frame.
    fn follow(&mut self, audio: &mut Audio, path: Option<&Path>, volume: f32) {
        match (self.id, path) {
            (None, Some(path)) if volume > 0.01 => {
                self.id = audio.play_loop(path, volume);
                self.volume = volume;
            }
            (Some(_), _) if volume <= 0.01 => self.stop(audio),
            (Some(id), _) if (volume - self.volume).abs() > 0.02 => {
                audio.set_loop_volume(id, volume);
                self.volume = volume;
            }
            _ => {}
        }
    }

    fn stop(&mut self, audio: &mut Audio) {
        if let Some(id) = self.id.take() {
            audio.stop(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_loop_runs_only_while_it_can_be_heard() {
        let path = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/wind.ogg"));
        let mut audio = Audio::headless();
        let mut sound = Loop::default();
        sound.follow(&mut audio, Some(path), 0.0);
        assert_eq!(audio.loop_count(), 0);
        sound.follow(&mut audio, Some(path), 0.3);
        assert_eq!(audio.loop_count(), 1);
        sound.follow(&mut audio, Some(path), 0.6);
        assert_eq!(audio.loop_count(), 1, "the same one, louder");
        sound.follow(&mut audio, Some(path), 0.0);
        assert_eq!(audio.loop_count(), 0);
        sound.follow(&mut audio, None, 0.5);
        assert_eq!(audio.loop_count(), 0, "a scene with no such sound stays quiet");
    }
}
