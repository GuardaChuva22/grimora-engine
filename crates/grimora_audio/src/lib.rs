//! Sound for Grimora, built on [kira](https://crates.io/crates/kira).
//!
//! [`Audio`] plays one-shot effects, looping ambience and sounds from places in the
//! world. Those come from the right side and, the farther they are, the quieter, duller
//! and later they arrive; walls muffle them (`space.rs` has the numbers). Without an
//! audio device it quietly does nothing, so games and CI never fail over sound.
//!
//! ```no_run
//! use grimora_audio::{Audio, Emitter};
//! use grimora_core::Vec3;
//!
//! let mut audio = Audio::new();
//! audio.play("assets/sounds/step_1.wav", 0.8);
//! audio.play_pitched_at("assets/sounds/clang_1.wav", 0.9, 1.1, Emitter::loud(Vec3::new(30.0, 1.0, 0.0)));
//! let fire = audio.play_loop_at("assets/sounds/fire.ogg", 1.0, Emitter::at(Vec3::new(4.5, 2.0, 6.0)));
//! # let _ = fire;
//! ```

mod space;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use grimora_core::{Camera, Quat, Vec3};
use grimora_scene::SceneSound;
use kira::backend::mock::{MockBackend, MockBackendSettings};
use kira::effect::filter::{FilterBuilder, FilterHandle};
use kira::listener::ListenerHandle;
use kira::sound::PlaybackState;
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
use kira::track::{MainTrackHandle, SpatialTrackBuilder, SpatialTrackHandle, TrackBuilder, TrackHandle};
use kira::{AudioManager, AudioManagerSettings, Decibels, DefaultBackend, ResourceLimitReached, StartTime, Tween};

use space::{Ears, OPEN_CUTOFF, occlusion_between};
pub use space::{Emitter, Heard, SPEED_OF_SOUND};

/// How high the filter on each category's track lets sounds through, in hertz.
const UNDERWATER_CUTOFF: f64 = 480.0;

/// How long a sound in the world takes to follow what changed, so changes don't click.
const FOLLOW: Duration = Duration::from_millis(40);
/// Sounds already playing that look again for walls each frame. The rest wait their turn.
const OCCLUSION_CHECKS: usize = 6;
/// How fast a wall appearing or going away takes effect, in full changes per second.
const OCCLUSION_SPEED: f32 = 5.0;

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

/// The engine's sound system. Volumes are linear gains (1 = as recorded).
pub struct Audio {
    engine: Option<Engine>,
    /// Decoded files by path, with the file's modification time. Failures are
    /// cached too, so a missing file logs once instead of on every play.
    cache: HashMap<PathBuf, (Option<SystemTime>, Option<StaticSoundData>)>,
    loops: Vec<Loop>,
    /// One-shots in the world waiting for this frame's look at the walls.
    waiting: Vec<Waiting>,
    /// One-shots in the world still sounding.
    shots: Vec<Shot>,
    next_id: u64,
    master: f32,
    volumes: [f32; 2],
    ears: Ears,
    /// Where the round of [`OCCLUSION_CHECKS`] is.
    occlusion_turn: usize,
}

/// The live kira side; absent when audio is disabled.
struct Engine {
    manager: Manager,
    listener: ListenerHandle,
    /// One mixer track per [`Category`].
    tracks: Vec<TrackHandle>,
    /// One per track, to muffle them.
    filters: Vec<FilterHandle>,
    muffled: f32,
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

/// A sound's own track in the mixer, which puts it in its place in the world.
struct Voice {
    track: SpatialTrackHandle,
    filter: FilterHandle,
    /// What the track is set to now.
    heard: Heard,
    /// How much of the sound is blocked, on its way to `blocked_target`.
    blocked: f32,
    blocked_target: f32,
    /// Whether anyone looked for walls yet: the first answer applies at once.
    checked: bool,
}

impl Voice {
    fn new(engine: &mut Engine, category: Category, emitter: &Emitter, heard: Heard) -> Option<Self> {
        // Kira only pans; the falloff is ours, set as the track volume.
        let mut builder = SpatialTrackBuilder::new()
            .attenuation_function(None)
            .volume(gain_to_db(heard.track_gain()))
            .spatialization_strength(heard.pan)
            .sound_capacity(1)
            .sub_track_capacity(0)
            .persist_until_sounds_finish(true);
        let filter = builder.add_effect(FilterBuilder::new().cutoff(f64::from(heard.cutoff)));
        let track = engine.tracks[category.index()]
            .add_spatial_sub_track(&engine.listener, emitter.position, builder)
            .map_err(|_| log::warn!("Too many sound emitters at once ({TRACK_CAPACITY} per category)"))
            .ok()?;
        Some(Self { track, filter, heard, blocked: 0.0, blocked_target: 0.0, checked: false })
    }

    fn follow(&mut self, ears: &Ears, emitter: &Emitter) {
        let heard = ears.hear(emitter, self.blocked);
        if heard == self.heard {
            return;
        }
        self.heard = heard;
        let tween = Tween { duration: FOLLOW, ..Default::default() };
        self.track.set_volume(gain_to_db(heard.track_gain()), tween);
        self.track.set_spatialization_strength(heard.pan, tween);
        self.filter.set_cutoff(f64::from(heard.cutoff), tween);
    }

    fn look_for_walls(
        &mut self,
        emitter: &Emitter,
        from: Vec3,
        due: bool,
        dt: f32,
        blocked: &mut impl FnMut(Vec3, Vec3) -> bool,
    ) {
        // Out of earshot it doesn't matter, and it costs no rays.
        if (due || !self.checked) && emitter.gain_at(from) > 0.0 {
            self.blocked_target = occlusion_between(from, emitter.position, blocked);
            if !self.checked {
                self.blocked = self.blocked_target;
                self.checked = true;
            }
        }
        let step = OCCLUSION_SPEED * dt;
        self.blocked += (self.blocked_target - self.blocked).clamp(-step, step);
    }
}

struct Loop {
    id: SoundId,
    /// `None` when audio is disabled; the loop is still tracked so ids stay valid.
    sound: Option<StaticSoundHandle>,
    /// Positional loops: where they are, and their own track.
    emitter: Option<Emitter>,
    voice: Option<Voice>,
    /// Set for loops started by [`Audio::set_scene_sounds`].
    scene: Option<SceneSound>,
}

/// A one-shot in the world that hasn't started yet.
struct Waiting {
    data: StaticSoundData,
    emitter: Emitter,
    blocked: Option<f32>,
}

/// A one-shot in the world. It goes away when the sound ends.
struct Shot {
    emitter: Emitter,
    voice: Voice,
    sound: StaticSoundHandle,
}

impl Loop {
    /// Fades the sound out; drop the loop afterwards.
    fn stop(&mut self) {
        if let Some(sound) = &mut self.sound {
            sound.stop(Tween { duration: LOOP_FADE, ..Default::default() });
        }
        // The voice's track was built with `persist_until_sounds_finish`, so dropping it
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
            waiting: Vec::new(),
            shots: Vec::new(),
            next_id: 0,
            master: 1.0,
            volumes: [1.0; 2],
            ears: Ears::default(),
            occlusion_turn: 0,
        }
    }

    /// Loads and mixes everything like [`Audio::new`], but never opens a device.
    /// For tests and automated runs (screenshots).
    pub fn headless() -> Self {
        // A low mixing rate keeps the fake output cheap: kira steps through every
        // source frame, so at 1 Hz (the mock's default) each update costs millions of steps.
        Self::headless_with(AudioManagerSettings {
            backend_settings: MockBackendSettings { sample_rate: MOCK_SAMPLE_RATE },
            ..Default::default()
        })
    }

    fn headless_with(settings: AudioManagerSettings<MockBackend>) -> Self {
        match AudioManager::<MockBackend>::new(settings) {
            Ok(manager) => Self::with_manager(Manager::Mock(Box::new(manager))),
            Err(()) => Self::silent(),
        }
    }

    fn with_manager(mut manager: Manager) -> Self {
        let mut audio = Self::silent();
        let mut filters = Vec::new();
        let tracks: Result<Vec<_>, _> = Category::ALL
            .iter()
            .map(|_| {
                let mut track = TrackBuilder::new().sub_track_capacity(TRACK_CAPACITY);
                filters.push(track.add_effect(FilterBuilder::new().cutoff(f64::from(OPEN_CUTOFF))));
                manager.add_sub_track(track)
            })
            .collect();
        match (tracks, manager.add_listener()) {
            (Ok(tracks), Ok(listener)) => {
                audio.engine = Some(Engine { manager, listener, tracks, filters, muffled: 0.0 });
            }
            _ => log::warn!("No audio: could not create mixer tracks"),
        }
        audio
    }

    /// Muffles everything, as heard under water: from 0 (not at all) to 1.
    pub fn set_underwater(&mut self, amount: f32) {
        let Some(engine) = &mut self.engine else { return };
        if (engine.muffled - amount).abs() < 0.005 {
            return;
        }
        engine.muffled = amount;
        // Evenly in pitch, not in hertz, which is how ears hear a filter closing.
        let open = f64::from(OPEN_CUTOFF);
        let cutoff = open * (UNDERWATER_CUTOFF / open).powf(f64::from(amount.clamp(0.0, 1.0)));
        for filter in &mut engine.filters {
            filter.set_cutoff(cutoff, Tween { duration: Duration::from_millis(60), ..Default::default() });
        }
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
        self.play_pitched_at(path, volume, 1.0, emitter);
    }

    /// Like [`Audio::play_at`], with a playback rate. This is the way to play anything that
    /// happens at a place, not to the player. The sound starts at the end of the frame
    /// ([`Audio::update`]), once it's known what's in its way.
    pub fn play_pitched_at(&mut self, path: impl AsRef<Path>, volume: f32, pitch: f32, emitter: Emitter) {
        if self.engine.is_none() {
            return;
        }
        let Some(data) = self.sound(path.as_ref()) else { return };
        let data = data.volume(gain_to_db(volume)).playback_rate(f64::from(pitch.max(0.01)));
        self.waiting.push(Waiting { data, emitter, blocked: None });
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
        if let Some(voice) = &mut l.voice {
            voice.track.set_position(position, Tween::default());
        }
    }

    /// Moves the listener and every sound in the world by `offset`, at once, as the world's
    /// origin moves the other way: nothing is heard to change. Scene loops keep matching
    /// the shifted [`SceneSound`]s, so they keep playing.
    pub fn shift(&mut self, offset: Vec3) {
        let now = Tween { duration: Duration::ZERO, ..Default::default() };
        self.ears.position += offset;
        if let Some(point) = &mut self.ears.hearing_point {
            *point += offset;
        }
        if let Some(engine) = &mut self.engine {
            engine.listener.set_position(self.ears.position, now);
        }
        for l in &mut self.loops {
            if let Some(emitter) = &mut l.emitter {
                emitter.position += offset;
            }
            if let Some(position) = l.scene.as_mut().and_then(|scene| scene.position.as_mut()) {
                *position += offset;
            }
            if let (Some(voice), Some(emitter)) = (&mut l.voice, &l.emitter) {
                voice.track.set_position(emitter.position, now);
            }
        }
        for waiting in &mut self.waiting {
            waiting.emitter.position += offset;
        }
        for shot in &mut self.shots {
            shot.emitter.position += offset;
            shot.voice.track.set_position(shot.emitter.position, now);
        }
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
        self.ears.position = position;
        self.ears.orientation = orientation;
        if let Some(engine) = &mut self.engine {
            engine.listener.set_position(position, Tween::default());
            engine.listener.set_orientation(orientation, Tween::default());
        }
    }

    /// Where distances are measured from, when that isn't the listener. With a camera
    /// behind a character, give it the character's head: direction still comes from the
    /// camera, but what happens next to the character sounds close. `None` is the listener.
    pub fn set_hearing_point(&mut self, point: Option<Vec3>) {
        self.ears.hearing_point = point;
    }

    /// How a sound from `emitter` would reach the listener now, walls aside.
    pub fn heard(&self, emitter: &Emitter) -> Heard {
        self.ears.hear(emitter, 0.0)
    }

    /// The one-shots now sounding in the world, and how each one reaches the listener.
    pub fn world_sounds(&self) -> impl Iterator<Item = (Emitter, Heard)> + '_ {
        self.shots.iter().map(|shot| (shot.emitter, shot.voice.heard))
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

    /// Finds what stands between the hearing point and the sounds in the world, once per
    /// frame before [`Audio::update`] (the engine does it). `blocked` answers whether
    /// anything is between two points. Sounds about to start always get an answer, and the
    /// ones already playing take turns.
    pub fn update_occlusion(&mut self, dt: f32, mut blocked: impl FnMut(Vec3, Vec3) -> bool) {
        if self.engine.is_none() {
            return;
        }
        let from = self.ears.hearing_from();
        for waiting in &mut self.waiting {
            if waiting.emitter.gain_at(from) > 0.0 {
                waiting.blocked = Some(occlusion_between(from, waiting.emitter.position, &mut blocked));
            }
        }
        let voices = (self.loops.iter().filter(|l| l.voice.is_some()).count() + self.shots.len()).max(1);
        let turn = self.occlusion_turn % voices;
        let loops = self.loops.iter_mut().filter_map(|l| Some((l.emitter?, l.voice.as_mut()?)));
        let shots = self.shots.iter_mut().map(|s| (s.emitter, &mut s.voice));
        for (i, (emitter, voice)) in loops.chain(shots).enumerate() {
            let due = (i + voices - turn) % voices < OCCLUSION_CHECKS;
            voice.look_for_walls(&emitter, from, due, dt, &mut blocked);
        }
        self.occlusion_turn = (turn + OCCLUSION_CHECKS) % voices;
    }

    /// Once per frame (the engine does it): starts the sounds played this frame, makes the
    /// ones in the world follow the listener and lets the headless mixer free finished sounds.
    pub fn update(&mut self) {
        self.start_waiting();
        self.shots.retain(|shot| shot.sound.state() != PlaybackState::Stopped);
        let ears = self.ears;
        for shot in &mut self.shots {
            shot.voice.follow(&ears, &shot.emitter);
        }
        for l in &mut self.loops {
            if let (Some(emitter), Some(voice)) = (&l.emitter, &mut l.voice) {
                voice.follow(&ears, emitter);
            }
        }
        if let Some(Engine { manager: Manager::Mock(manager), .. }) = &mut self.engine {
            let backend = manager.backend_mut();
            backend.on_start_processing();
            backend.process();
        }
    }

    fn start_waiting(&mut self) {
        let ears = self.ears;
        for waiting in std::mem::take(&mut self.waiting) {
            let Waiting { data, emitter, blocked } = waiting;
            let heard = ears.hear(&emitter, blocked.unwrap_or(0.0));
            let Some(engine) = &mut self.engine else { return };
            if heard.gain <= 0.0 {
                continue; // out of earshot
            }
            let Some(mut voice) = Voice::new(engine, Category::Effects, &emitter, heard) else { continue };
            voice.blocked = blocked.unwrap_or(0.0);
            voice.blocked_target = voice.blocked;
            voice.checked = blocked.is_some();
            let delay = Duration::try_from_secs_f32(heard.delay).unwrap_or_default();
            match voice.track.play(data.start_time(StartTime::Delayed(delay))) {
                Ok(sound) => self.shots.push(Shot { emitter, voice, sound }),
                Err(e) => log::warn!("Can't play a sound: {e}"),
            }
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
        let mut entry = Loop { id, sound: None, emitter, voice: None, scene };
        let ears = self.ears;
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
                    let mut voice = Voice::new(engine, Category::Ambient, &emitter, ears.hear(&emitter, 0.0))?;
                    let sound = voice.track.play(data);
                    entry.voice = Some(voice);
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
