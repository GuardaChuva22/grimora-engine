//! Sound for Grimora, built on [kira](https://crates.io/crates/kira).
//!
//! [`Audio`] plays one-shot effects, looping ambience and positional emitters
//! that get quieter with distance and pan with a listener (the camera). Without
//! an audio device it quietly does nothing, so games and CI never fail over sound.
//!
//! ```no_run
//! use grimora_audio::{Audio, Emitter};
//! use grimora_core::Vec3;
//!
//! let mut audio = Audio::new();
//! audio.play("assets/sounds/step_1.wav", 0.8);
//! let fire = audio.play_loop_at("assets/sounds/fire.wav", 1.0, Emitter::at(Vec3::new(4.5, 2.0, 6.0)));
//! # let _ = fire;
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use grimora_core::{Camera, Quat, Vec3};
use grimora_scene::SceneSound;
use kira::backend::mock::{MockBackend, MockBackendSettings};
use kira::listener::ListenerHandle;
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
use kira::track::{MainTrackHandle, SpatialTrackBuilder, SpatialTrackHandle, TrackBuilder, TrackHandle};
use kira::{AudioManager, AudioManagerSettings, Decibels, DefaultBackend, ResourceLimitReached, Tween};

/// Fade used when loops start and stop, so hot reloads don't click.
const LOOP_FADE: Duration = Duration::from_millis(400);
/// Tracks (emitters) each category can hold at once.
const TRACK_CAPACITY: usize = 256;
/// Headless mixing rate: each update renders kira's 128-frame buffer, about 0.13 s of sound.
const MOCK_SAMPLE_RATE: u32 = 1000;

/// Volume groups, each with its own level on top of the master volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    /// Loops: wind, fire, hums. Scene sounds play here.
    Ambient,
    /// One-shots: steps, hits, spells.
    Effects,
}

impl Category {
    const ALL: [Category; 2] = [Category::Ambient, Category::Effects];

    fn index(self) -> usize {
        self as usize
    }
}

/// Identifies a looping sound so it can be moved or stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SoundId(u64);

/// Where a positional sound plays and how it fades with distance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Emitter {
    pub position: Vec3,
    /// Full volume up to this distance, in meters.
    pub min_distance: f32,
    /// Silent beyond this distance.
    pub max_distance: f32,
}

impl Emitter {
    /// An emitter at `position` with the default distances (1 to 25 m).
    pub fn at(position: Vec3) -> Self {
        Self { position, ..Default::default() }
    }

    /// Gain heard from `listener`: 1 within `min_distance`, then the natural
    /// inverse-distance falloff (-6 dB per doubling), eased to silence at `max_distance`.
    pub fn gain_at(&self, listener: Vec3) -> f32 {
        let (min, max) = self.distances();
        let d = self.position.distance(listener);
        let t = ((d - min) / (max - min)).clamp(0.0, 1.0);
        min / d.max(min) * (1.0 - t * t)
    }

    fn distances(&self) -> (f32, f32) {
        let min = self.min_distance.max(0.01);
        (min, self.max_distance.max(min + 0.01))
    }
}

impl Default for Emitter {
    fn default() -> Self {
        Self { position: Vec3::ZERO, min_distance: 1.0, max_distance: 25.0 }
    }
}

/// The engine's sound system. Volumes are linear gains (1 = as recorded).
pub struct Audio {
    engine: Option<Engine>,
    /// Decoded files by path, with the file's modification time. Failures are
    /// cached too, so a missing file logs once instead of on every play.
    cache: HashMap<PathBuf, (Option<SystemTime>, Option<StaticSoundData>)>,
    loops: Vec<Loop>,
    next_id: u64,
    master: f32,
    volumes: [f32; 2],
    listener: Vec3,
}

/// The live kira side; absent when audio is disabled.
struct Engine {
    manager: Manager,
    listener: ListenerHandle,
    /// One mixer track per [`Category`].
    tracks: Vec<TrackHandle>,
}

/// Kira's manager is generic over its backend; this keeps that out of [`Audio`].
enum Manager {
    Device(Box<AudioManager<DefaultBackend>>),
    /// Mixes without an output device (tests and automated runs).
    Mock(Box<AudioManager<MockBackend>>),
}

impl Manager {
    fn add_sub_track(&mut self, builder: TrackBuilder) -> Result<TrackHandle, ResourceLimitReached> {
        match self {
            Manager::Device(m) => m.add_sub_track(builder),
            Manager::Mock(m) => m.add_sub_track(builder),
        }
    }

    fn add_listener(&mut self) -> Result<ListenerHandle, ResourceLimitReached> {
        match self {
            Manager::Device(m) => m.add_listener(Vec3::ZERO, Quat::IDENTITY),
            Manager::Mock(m) => m.add_listener(Vec3::ZERO, Quat::IDENTITY),
        }
    }

    fn main_track(&mut self) -> &mut MainTrackHandle {
        match self {
            Manager::Device(m) => m.main_track(),
            Manager::Mock(m) => m.main_track(),
        }
    }
}

struct Loop {
    id: SoundId,
    /// `None` when audio is disabled; the loop is still tracked so ids stay valid.
    sound: Option<StaticSoundHandle>,
    /// Positional loops: where they are, and their own spatial track whose
    /// volume follows the distance to the listener.
    emitter: Option<Emitter>,
    track: Option<SpatialTrackHandle>,
    /// Set for loops started by [`Audio::set_scene_sounds`].
    scene: Option<SceneSound>,
}

impl Loop {
    fn follow_listener(&mut self, listener: Vec3) {
        if let (Some(emitter), Some(track)) = (&self.emitter, &mut self.track) {
            track.set_volume(gain_to_db(emitter.gain_at(listener)), Tween::default());
        }
    }

    /// Fades the sound out; drop the loop afterwards.
    fn stop(&mut self) {
        if let Some(sound) = &mut self.sound {
            sound.stop(Tween { duration: LOOP_FADE, ..Default::default() });
        }
        // The track was built with `persist_until_sounds_finish`, so dropping it
        // afterwards still lets the fade-out play.
    }
}

impl Default for Audio {
    fn default() -> Self {
        Self::new()
    }
}

impl Audio {
    /// Opens the default output device, or falls back to [`Audio::silent`] with a warning.
    pub fn new() -> Self {
        match AudioManager::<DefaultBackend>::new(AudioManagerSettings::default()) {
            Ok(manager) => {
                log::info!("Audio started on the default output device");
                Self::with_manager(Manager::Device(Box::new(manager)))
            }
            Err(e) => {
                log::warn!("No audio: {e}. Continuing without sound.");
                Self::silent()
            }
        }
    }

    /// No sound at all. Every call still works and nothing is loaded.
    pub fn silent() -> Self {
        Self {
            engine: None,
            cache: HashMap::new(),
            loops: Vec::new(),
            next_id: 0,
            master: 1.0,
            volumes: [1.0; 2],
            listener: Vec3::ZERO,
        }
    }

    /// Loads and mixes everything like [`Audio::new`], but never opens a device.
    /// For tests and automated runs (screenshots).
    pub fn headless() -> Self {
        // A low mixing rate keeps the fake output cheap: kira steps through every
        // source frame, so at 1 Hz (the mock's default) each update costs millions of steps.
        let settings = AudioManagerSettings {
            backend_settings: MockBackendSettings { sample_rate: MOCK_SAMPLE_RATE },
            ..Default::default()
        };
        match AudioManager::<MockBackend>::new(settings) {
            Ok(manager) => Self::with_manager(Manager::Mock(Box::new(manager))),
            Err(()) => Self::silent(),
        }
    }

    fn with_manager(mut manager: Manager) -> Self {
        let mut audio = Self::silent();
        let tracks: Result<Vec<_>, _> = Category::ALL
            .iter()
            .map(|_| manager.add_sub_track(TrackBuilder::new().sub_track_capacity(TRACK_CAPACITY)))
            .collect();
        match (tracks, manager.add_listener()) {
            (Ok(tracks), Ok(listener)) => audio.engine = Some(Engine { manager, listener, tracks }),
            _ => log::warn!("No audio: could not create mixer tracks"),
        }
        audio
    }

    /// Whether sound is actually being mixed (false when silent).
    pub fn is_enabled(&self) -> bool {
        self.engine.is_some()
    }

    /// Decodes a file into the cache ahead of time. Returns false if it can't be loaded.
    pub fn preload(&mut self, path: impl AsRef<Path>) -> bool {
        self.engine.is_none() || self.sound(path.as_ref()).is_some()
    }

    /// Plays a sound once, not positioned (UI, the player's own steps).
    pub fn play(&mut self, path: impl AsRef<Path>, volume: f32) {
        self.play_pitched(path, volume, 1.0);
    }

    /// Like [`Audio::play`], with a playback rate (1 = normal, 2 = an octave up).
    /// Small random pitch changes keep repeated sounds from feeling mechanical.
    pub fn play_pitched(&mut self, path: impl AsRef<Path>, volume: f32, pitch: f32) {
        if self.engine.is_none() {
            return;
        }
        let Some(data) = self.sound(path.as_ref()) else { return };
        let data = data.volume(gain_to_db(volume)).playback_rate(f64::from(pitch.max(0.01)));
        let Some(engine) = &mut self.engine else { return };
        if let Err(e) = engine.tracks[Category::Effects.index()].play(data) {
            log::warn!("Can't play {}: {e}", path.as_ref().display());
        }
    }

    /// Plays a sound once from a point in the world.
    pub fn play_at(&mut self, path: impl AsRef<Path>, volume: f32, emitter: Emitter) {
        if self.engine.is_none() {
            return;
        }
        let gain = volume * emitter.gain_at(self.listener);
        if gain <= 0.0 {
            return; // out of earshot
        }
        let Some(data) = self.sound(path.as_ref()) else { return };
        let Some(engine) = &mut self.engine else { return };
        // The track goes away by itself once the sound ends.
        let track = spatial_track(engine, Category::Effects, &emitter, gain_to_db(gain));
        if let Some(mut track) = track
            && let Err(e) = track.play(data)
        {
            log::warn!("Can't play {}: {e}", path.as_ref().display());
        }
    }

    /// Starts a loop heard everywhere at the same volume (ambience, music).
    pub fn play_loop(&mut self, path: impl AsRef<Path>, volume: f32) -> Option<SoundId> {
        self.start_loop(path.as_ref(), volume, None, None)
    }

    /// Starts a loop at a point in the world: louder when close, panned by direction.
    pub fn play_loop_at(&mut self, path: impl AsRef<Path>, volume: f32, emitter: Emitter) -> Option<SoundId> {
        self.start_loop(path.as_ref(), volume, Some(emitter), None)
    }

    /// Moves a positional loop (e.g. one carried by a character).
    pub fn set_position(&mut self, id: SoundId, position: Vec3) {
        let Some(l) = self.loops.iter_mut().find(|l| l.id == id) else { return };
        if let Some(emitter) = &mut l.emitter {
            emitter.position = position;
        }
        if let Some(track) = &mut l.track {
            track.set_position(position, Tween::default());
        }
        l.follow_listener(self.listener);
    }

    /// Changes a loop's volume with a short fade.
    pub fn set_loop_volume(&mut self, id: SoundId, volume: f32) {
        if let Some(sound) = self.loops.iter_mut().find(|l| l.id == id).and_then(|l| l.sound.as_mut()) {
            sound.set_volume(gain_to_db(volume), Tween { duration: Duration::from_millis(100), ..Default::default() });
        }
    }

    /// Fades out and removes a loop. Unknown ids are ignored.
    pub fn stop(&mut self, id: SoundId) {
        if let Some(index) = self.loops.iter().position(|l| l.id == id) {
            self.loops.remove(index).stop();
        }
    }

    /// Fades out every loop, scene ones included.
    pub fn stop_all(&mut self) {
        self.loops.drain(..).for_each(|mut l| l.stop());
    }

    /// How many loops are playing (scene and game ones).
    pub fn loop_count(&self) -> usize {
        self.loops.len()
    }

    /// Replaces the scene's loops with `sounds`. Loops that didn't change keep
    /// playing (a hot reload doesn't restart them); loops started by the game
    /// through [`Audio::play_loop`] are left alone.
    pub fn set_scene_sounds(&mut self, sounds: &[SceneSound]) {
        // Scene sounds not matched by a playing loop yet, in file order.
        let mut pending: Vec<Option<&SceneSound>> = sounds.iter().map(Some).collect();
        self.loops.retain_mut(|l| {
            let Some(current) = &l.scene else { return true };
            let matched = pending.iter_mut().find(|s| **s == Some(current));
            let keep = matched.map(|slot| slot.take()).is_some();
            if !keep {
                l.stop();
            }
            keep
        });
        let kept = sounds.len() - pending.iter().flatten().count();
        let mut started = 0;
        for sound in pending.into_iter().flatten() {
            let emitter = sound.position.map(|position| Emitter {
                position,
                min_distance: sound.min_distance,
                max_distance: sound.max_distance,
            });
            started += usize::from(self.start_loop(&sound.path, sound.volume, emitter, Some(sound.clone())).is_some());
        }
        if !sounds.is_empty() {
            log::info!("Scene sounds: {started} started, {kept} kept playing, {} loops in total", self.loops.len());
        }
    }

    /// Places the listener: sounds to its right play in the right ear.
    /// `orientation` turns -Z (forward) into the facing direction, Y up.
    pub fn set_listener(&mut self, position: Vec3, orientation: Quat) {
        self.listener = position;
        if let Some(engine) = &mut self.engine {
            engine.listener.set_position(position, Tween::default());
            engine.listener.set_orientation(orientation, Tween::default());
        }
        self.loops.iter_mut().for_each(|l| l.follow_listener(position));
    }

    /// Puts the listener where the camera is, facing where it looks.
    pub fn follow_camera(&mut self, camera: &Camera) {
        self.set_listener(camera.position, camera_orientation(camera));
    }

    pub fn master_volume(&self) -> f32 {
        self.master
    }

    /// Scales everything.
    pub fn set_master_volume(&mut self, volume: f32) {
        self.master = volume.max(0.0);
        if let Some(engine) = &mut self.engine {
            engine.manager.main_track().set_volume(gain_to_db(self.master), Tween::default());
        }
    }

    pub fn volume(&self, category: Category) -> f32 {
        self.volumes[category.index()]
    }

    /// Scales one category, on top of the master volume.
    pub fn set_volume(&mut self, category: Category, volume: f32) {
        self.volumes[category.index()] = volume.max(0.0);
        if let Some(engine) = &mut self.engine {
            engine.tracks[category.index()].set_volume(gain_to_db(volume), Tween::default());
        }
    }

    /// Once per frame (the engine does it). Lets the headless mixer free finished sounds.
    pub fn update(&mut self) {
        if let Some(Engine { manager: Manager::Mock(manager), .. }) = &mut self.engine {
            let backend = manager.backend_mut();
            backend.on_start_processing();
            backend.process();
        }
    }

    fn start_loop(
        &mut self,
        path: &Path,
        volume: f32,
        emitter: Option<Emitter>,
        scene: Option<SceneSound>,
    ) -> Option<SoundId> {
        let id = SoundId(self.next_id);
        let mut entry = Loop { id, sound: None, emitter, track: None, scene };
        let listener = self.listener;
        if self.engine.is_some() {
            let data = self.sound(path)?;
            // Spread copies of the same loop over its length so four torches
            // don't crackle in lockstep.
            let offset = (id.0 as f64 * 0.618_034).fract() * data.duration().as_secs_f64();
            let data = data
                .loop_region(..)
                .start_position(offset)
                .volume(gain_to_db(volume))
                .fade_in_tween(Tween { duration: LOOP_FADE, ..Default::default() });
            let engine = self.engine.as_mut()?;
            let played = match emitter {
                Some(emitter) => {
                    let gain = gain_to_db(emitter.gain_at(listener));
                    let mut track = spatial_track(engine, Category::Ambient, &emitter, gain)?;
                    let sound = track.play(data);
                    entry.track = Some(track);
                    sound
                }
                None => engine.tracks[Category::Ambient.index()].play(data),
            };
            match played {
                Ok(sound) => entry.sound = Some(sound),
                Err(e) => {
                    log::warn!("Can't play {}: {e}", path.display());
                    return None;
                }
            }
        }
        self.next_id += 1;
        self.loops.push(entry);
        Some(id)
    }

    /// The decoded file, from the cache when it hasn't changed on disk.
    fn sound(&mut self, path: &Path) -> Option<StaticSoundData> {
        let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        if let Some((stamp, data)) = self.cache.get(path)
            && *stamp == modified
        {
            return data.clone();
        }
        let data = match StaticSoundData::from_file(path) {
            Ok(data) => {
                log::info!("Loaded sound {} ({:.1} s)", path.display(), data.duration().as_secs_f64());
                Some(data)
            }
            Err(e) => {
                log::warn!("Can't load sound {}: {e}", path.display());
                None
            }
        };
        self.cache.insert(path.to_path_buf(), (modified, data.clone()));
        data
    }
}

fn spatial_track(
    engine: &mut Engine,
    category: Category,
    emitter: &Emitter,
    volume: Decibels,
) -> Option<SpatialTrackHandle> {
    // Kira only pans; the distance falloff is ours (`Emitter::gain_at`), set as the track volume.
    let builder =
        SpatialTrackBuilder::new().attenuation_function(None).volume(volume).persist_until_sounds_finish(true);
    engine.tracks[category.index()]
        .add_spatial_sub_track(&engine.listener, emitter.position, builder)
        .map_err(|_| log::warn!("Too many sound emitters at once ({TRACK_CAPACITY} per category)"))
        .ok()
}

/// Linear gain to kira's decibels; 0 is silence.
fn gain_to_db(gain: f32) -> Decibels {
    if gain <= 0.001 { Decibels::SILENCE } else { Decibels(20.0 * gain.log10()) }
}

/// Rotation that turns -Z into the camera's view direction (yaw, then pitch).
pub fn camera_orientation(camera: &Camera) -> Quat {
    Quat::from_rotation_y(camera.yaw) * Quat::from_rotation_x(camera.pitch)
}

#[cfg(test)]
mod tests;
