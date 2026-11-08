use std::sync::{Arc, Mutex};

use kira::Frame;
use kira::effect::{Effect, EffectBuilder};
use kira::info::Info;
use kira::track::MainTrackBuilder;

use super::*;

/// Writes a short 16-bit mono WAV (a quiet sine) and returns its path.
fn test_wav(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("grimora_audio_test_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let rate = 8000u32;
    let samples: Vec<i16> = (0..rate / 4).map(|i| ((i as f32 * 0.25).sin() * 8000.0) as i16).collect();
    write_wav(&path, rate, &samples);
    path
}

fn write_wav(path: &Path, rate: u32, samples: &[i16]) {
    let data_len = samples.len() as u32 * 2;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    samples.iter().for_each(|s| bytes.extend_from_slice(&s.to_le_bytes()));
    std::fs::write(path, bytes).unwrap();
}

fn scene_sound(path: &Path, position: Option<Vec3>) -> SceneSound {
    SceneSound { path: path.to_path_buf(), position, volume: 1.0, min_distance: 1.0, max_distance: 20.0 }
}

/// Spatial tracks alive in kira's ambient mixer track, after letting the mixer run.
fn kira_emitters(audio: &mut Audio) -> usize {
    // A stopped emitter needs a few mixer passes: fade out, free the sound, free the track.
    for _ in 0..12 {
        audio.update();
    }
    audio.engine.as_ref().unwrap().tracks[Category::Ambient.index()].num_sub_tracks()
}

#[test]
fn silent_audio_accepts_every_call() {
    let mut audio = Audio::silent();
    assert!(!audio.is_enabled());
    audio.play("missing.wav", 1.0);
    audio.play_at("missing.wav", 1.0, Emitter::at(Vec3::X));
    let id = audio.play_loop_at("missing.wav", 1.0, Emitter::default()).unwrap();
    audio.set_position(id, Vec3::ONE);
    audio.follow_camera(&Camera::default());
    audio.set_master_volume(0.5);
    audio.set_volume(Category::Effects, 0.2);
    audio.update();
    assert_eq!(audio.loop_count(), 1);
    audio.stop(id);
    assert_eq!(audio.loop_count(), 0);
    // Nothing is decoded without a device.
    assert!(audio.cache.is_empty());
}

#[test]
fn headless_audio_plays_and_caches() {
    let path = test_wav("tone.wav");
    let mut audio = Audio::headless();
    assert!(audio.is_enabled());
    assert!(audio.preload(&path));
    for _ in 0..5 {
        audio.play_pitched(&path, 0.7, 1.1);
        audio.play_at(&path, 1.0, Emitter::at(Vec3::new(3.0, 0.0, 0.0)));
    }
    assert_eq!(audio.cache.len(), 1);
    let ambience = audio.play_loop(&path, 0.5).unwrap();
    let fire = audio.play_loop_at(&path, 1.0, Emitter::at(Vec3::Z)).unwrap();
    assert_ne!(ambience, fire);
    audio.set_loop_volume(fire, 0.3);
    audio.follow_camera(&Camera::new(Vec3::new(0.0, 2.0, 5.0), 0.3, -0.1));
    assert_eq!(audio.loop_count(), 2);
    assert_eq!(kira_emitters(&mut audio), 1);
    audio.stop_all();
    assert_eq!(audio.loop_count(), 0);
    assert_eq!(kira_emitters(&mut audio), 0);
}

#[test]
fn missing_files_fail_softly() {
    let mut audio = Audio::headless();
    let missing = Path::new("no/such/sound.ogg");
    assert!(!audio.preload(missing));
    audio.play(missing, 1.0);
    assert!(audio.play_loop(missing, 1.0).is_none());
    assert_eq!(audio.loop_count(), 0);
    // The failure is cached, so it is only reported once.
    assert_eq!(audio.cache.len(), 1);
}

#[test]
fn scene_reload_does_not_duplicate_loops() {
    let wind = test_wav("wind.wav");
    let fire = test_wav("fire.wav");
    let mut audio = Audio::headless();
    let game_loop = audio.play_loop(&wind, 0.2).unwrap();
    let torches: Vec<SceneSound> =
        (0..4).map(|i| scene_sound(&fire, Some(Vec3::new(i as f32 * 3.0, 1.0, 0.0)))).collect();
    let mut sounds = vec![scene_sound(&wind, None)];
    sounds.extend(torches.iter().cloned());

    audio.set_scene_sounds(&sounds);
    assert_eq!(audio.loop_count(), 6);
    assert_eq!(kira_emitters(&mut audio), 4);
    let ids: Vec<SoundId> = audio.loops.iter().map(|l| l.id).collect();

    // Reloading the same scene keeps every loop as it is.
    for _ in 0..3 {
        audio.set_scene_sounds(&sounds);
    }
    assert_eq!(audio.loops.iter().map(|l| l.id).collect::<Vec<_>>(), ids);
    assert_eq!(kira_emitters(&mut audio), 4);

    // Moving one torch restarts only that one; dropping the wind stops it.
    sounds.remove(0);
    sounds[1].position = Some(Vec3::new(0.0, 1.0, 9.0));
    audio.set_scene_sounds(&sounds);
    assert_eq!(audio.loop_count(), 5);
    assert_eq!(kira_emitters(&mut audio), 4);
    assert_eq!(audio.loops.iter().filter(|l| !ids.contains(&l.id)).count(), 1);

    // The game's own loop survives scene changes.
    audio.set_scene_sounds(&[]);
    assert_eq!(audio.loop_count(), 1);
    assert_eq!(audio.loops[0].id, game_loop);
    assert_eq!(kira_emitters(&mut audio), 0);
}

#[test]
fn listener_follows_the_camera_convention() {
    for (yaw, pitch) in [(0.0, 0.0), (0.7, 0.0), (-2.0, 0.4), (3.0, -1.2)] {
        let camera = Camera::new(Vec3::ZERO, yaw, pitch);
        let q = camera_orientation(&camera);
        assert!((q * Vec3::NEG_Z).distance(camera.forward()) < 1e-5);
        // Kira puts the right ear on +X; it must match the camera's right.
        assert!((q * Vec3::X).distance(camera.right()) < 1e-5);
    }
}

#[test]
fn emitters_fade_with_distance() {
    let far = Emitter { position: Vec3::new(0.0, 1.0, 0.0), min_distance: 1.0, max_distance: 100.0 };
    let at = |d: f32| far.gain_at(far.position + Vec3::new(d * 0.6, 0.0, d * 0.8));
    assert_eq!(at(0.0), 1.0);
    assert_eq!(at(0.9), 1.0);
    // Inverse distance: about -6 dB each time the distance doubles...
    assert!((at(2.0) - 0.5).abs() < 1e-3 && (at(4.0) - 0.25).abs() < 1e-3);
    // ...and nothing at all past max_distance.
    assert_eq!(at(100.0), 0.0);
    assert_eq!(at(500.0), 0.0);
    let torch = Emitter { max_distance: 16.0, min_distance: 1.5, ..far };
    let gains: Vec<f32> = (0..40).map(|i| torch.gain_at(torch.position + Vec3::X * i as f32 * 0.5)).collect();
    assert!(gains.windows(2).all(|w| w[1] <= w[0]), "{gains:?}");
    // Broken distances still give sane numbers.
    let odd = Emitter { min_distance: 0.0, max_distance: -1.0, ..far };
    assert!(odd.gain_at(Vec3::ZERO).is_finite());
}

/// What came out of the mixer, one ear at a time.
#[derive(Debug, Default, Clone, Copy)]
struct Output {
    left: f32,
    right: f32,
    frames: usize,
    /// The frame in which anything first came out.
    started: Option<usize>,
}

impl Output {
    fn level(&self) -> f32 {
        self.left.max(self.right).sqrt()
    }
}

struct Meter(Arc<Mutex<Output>>);

impl Effect for Meter {
    fn process(&mut self, input: &mut [Frame], _dt: f64, _info: &Info) {
        let mut output = self.0.lock().unwrap();
        for frame in input {
            output.left += frame.left * frame.left;
            output.right += frame.right * frame.right;
            if frame.left.abs().max(frame.right.abs()) > 1e-5 && output.started.is_none() {
                output.started = Some(output.frames);
            }
            output.frames += 1;
        }
    }
}

impl EffectBuilder for Meter {
    type Handle = ();

    fn build(self) -> (Box<dyn Effect>, ()) {
        (Box::new(self), ())
    }
}

/// A rate high enough to carry the tones, and the frames rendered per update at it.
const METER_RATE: u32 = 32000;
const FRAMES_PER_UPDATE: usize = 128;

/// A headless mixer that reports what comes out of it.
fn metered_audio() -> (Audio, Arc<Mutex<Output>>) {
    let output = Arc::new(Mutex::new(Output::default()));
    let mut main_track_builder = MainTrackBuilder::new();
    main_track_builder.add_effect(Meter(output.clone()));
    let audio = Audio::headless_with(AudioManagerSettings {
        backend_settings: MockBackendSettings { sample_rate: METER_RATE },
        main_track_builder,
        ..Default::default()
    });
    (audio, output)
}

/// A quarter of a second of a pure tone.
fn tone(hz: f32) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("grimora_audio_test_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // Each test thread writes its own, as they run at the same time.
    let path = dir.join(format!("tone_{hz}_{:?}.wav", std::thread::current().id()));
    let samples: Vec<i16> = (0..METER_RATE / 4)
        .map(|i| ((i as f32 / METER_RATE as f32 * hz * std::f32::consts::TAU).sin() * 8000.0) as i16)
        .collect();
    write_wav(&path, METER_RATE, &samples);
    path
}

/// Plays `path` from `emitter` for the listener at the origin looking down -Z (or as `turn`
/// says), with `blocked` standing for the walls, and reports what came out.
fn mixed(path: &Path, emitter: Emitter, turn: Quat, blocked: bool) -> Output {
    let (mut audio, output) = metered_audio();
    audio.set_listener(Vec3::ZERO, turn);
    // The listener turns over a short fade.
    for _ in 0..20 {
        audio.update();
    }
    audio.play_at(path, 1.0, emitter);
    audio.update_occlusion(0.016, |_, _| blocked);
    // Long enough for the farthest sound here to arrive and end.
    for _ in 0..200 {
        audio.update();
    }
    *output.lock().unwrap()
}

fn straight(path: &Path, emitter: Emitter) -> Output {
    mixed(path, emitter, Quat::IDENTITY, false)
}

#[test]
fn a_far_sound_comes_out_lower_duller_and_later() {
    let (low, high) = (tone(200.0), tone(5000.0));
    let near = Emitter::loud(Vec3::new(0.0, 0.0, -3.0));
    let far = Emitter::loud(Vec3::new(0.0, 0.0, -60.0));

    let (low_near, low_far) = (straight(&low, near), straight(&low, far));
    let drop = low_far.level() / low_near.level();
    assert!(drop < 0.1, "60 m away should be at least 20 dB lower: {drop}");
    assert!(drop > 0.01, "but still heard: {drop}");

    // The filter takes the high tone far more than the low one.
    let (high_near, high_far) = (straight(&high, near), straight(&high, far));
    let high_drop = high_far.level() / high_near.level();
    assert!(high_drop < drop * 0.25, "highs should go first: {high_drop} against {drop}");

    // It arrives when sound would: 60 m at 343 m/s, give or take a mixer chunk.
    let late = low_far.started.unwrap() - low_near.started.unwrap();
    let expected = (57.0 / SPEED_OF_SOUND * METER_RATE as f32) as usize;
    assert!(late.abs_diff(expected) <= FRAMES_PER_UPDATE * 2, "{late} frames, expected {expected}");
}

#[test]
fn a_sound_comes_from_its_side() {
    let path = tone(200.0);
    let right = Emitter::at(Vec3::new(10.0, 0.0, 0.0));
    let left = Emitter::at(Vec3::new(-10.0, 0.0, 0.0));
    let db = |o: Output| 10.0 * (o.right / o.left).log10();

    assert!(db(straight(&path, right)) > 10.0, "{}", db(straight(&path, right)));
    assert!(db(straight(&path, left)) < -10.0, "{}", db(straight(&path, left)));
    // Turned around, the same sound changes ears.
    let turned = mixed(&path, right, Quat::from_rotation_y(std::f32::consts::PI), false);
    assert!(db(turned) < -10.0, "{}", db(turned));
    // Ahead it's even, and behind it's even and lower.
    let ahead = straight(&path, Emitter::at(Vec3::new(0.0, 0.0, -10.0)));
    let behind = straight(&path, Emitter::at(Vec3::new(0.0, 0.0, 10.0)));
    assert!(db(ahead).abs() < 0.1 && db(behind).abs() < 0.1);
    assert!(behind.level() < ahead.level() * 0.75, "{behind:?} {ahead:?}");
}

#[test]
fn a_sound_right_ahead_is_as_loud_as_a_flat_one() {
    let path = tone(200.0);
    let (mut audio, output) = metered_audio();
    audio.play(&path, 1.0);
    for _ in 0..200 {
        audio.update();
    }
    let flat = output.lock().unwrap().level();
    let ahead = straight(&path, Emitter::at(Vec3::new(0.0, 0.0, -1.9))).level();
    assert!((ahead / flat - 1.0).abs() < 0.1, "{ahead} against {flat}");
}

#[test]
fn distance_takes_more_of_a_sound_the_farther_it_is() {
    let ears = Ears::default();
    let heard: Vec<Heard> = (0..90).map(|i| ears.hear(&Emitter::loud(Vec3::new(0.0, 0.0, -(i as f32))), 0.0)).collect();
    assert!(heard.windows(2).all(|w| w[1].gain <= w[0].gain && w[1].cutoff <= w[0].cutoff && w[1].delay > w[0].delay));
    assert_eq!(heard[3].cutoff, OPEN_CUTOFF, "the air takes nothing from up close");
    assert!(heard[40].cutoff < 2500.0, "{}", heard[40].cutoff);
    assert!(heard[80].cutoff < 1200.0, "{}", heard[80].cutoff);
    assert!((heard[34].delay - 0.1).abs() < 0.01);
    // The presets say how far a sound carries.
    let at = Vec3::new(0.0, 0.0, -25.0);
    let (quiet, usual, loud) = (Emitter::quiet(at), Emitter::at(at), Emitter::loud(at));
    assert_eq!(quiet.gain_at(Vec3::ZERO), 0.0);
    assert!(usual.gain_at(Vec3::ZERO) < loud.gain_at(Vec3::ZERO));
}

#[test]
fn sounds_behind_the_head_lose_their_highs() {
    let ears = Ears::default();
    let ahead = ears.hear(&Emitter::at(Vec3::new(0.0, 0.0, -4.0)), 0.0);
    let side = ears.hear(&Emitter::at(Vec3::new(4.0, 0.0, 0.0)), 0.0);
    let behind = ears.hear(&Emitter::at(Vec3::new(0.0, 0.0, 4.0)), 0.0);
    assert_eq!(ahead.cutoff, side.cutoff);
    assert!(behind.cutoff < ahead.cutoff * 0.5);
}

#[test]
fn distance_comes_from_the_hearing_point_and_direction_from_the_camera() {
    let mut audio = Audio::headless();
    // A camera 5 m behind a character at the origin, both looking down -Z.
    audio.set_listener(Vec3::new(0.0, 0.0, 5.0), Quat::IDENTITY);
    let blow = Emitter::at(Vec3::new(0.0, 0.0, -1.0));
    let from_camera = audio.heard(&blow);
    assert!(from_camera.gain < 0.5);
    audio.set_hearing_point(Some(Vec3::ZERO));
    let from_character = audio.heard(&blow);
    assert_eq!(from_character.gain, 1.0, "a blow on the character is close, wherever the camera is");
    assert!(from_character.delay < 0.01);
    assert_eq!(from_character.pan, from_camera.pan, "the camera still says which side");
    audio.set_hearing_point(None);
    assert_eq!(audio.heard(&blow), from_camera);
}

#[test]
fn a_pan_opens_only_away_from_the_listener() {
    let ears = Ears::default();
    let pan = |d: f32| ears.hear(&Emitter::at(Vec3::new(d, 0.0, 0.0)), 0.0).pan;
    assert_eq!(pan(0.1), 0.0);
    assert!(pan(1.0) > 0.0 && pan(1.0) < pan(3.0));
    assert_eq!(pan(3.0), pan(30.0));
}

#[test]
fn a_wall_makes_a_sound_lower_and_duller() {
    let (low, high) = (tone(200.0), tone(5000.0));
    let beyond = Emitter::at(Vec3::new(0.0, 0.0, -4.0));
    let (low_open, low_walled) =
        (mixed(&low, beyond, Quat::IDENTITY, false), mixed(&low, beyond, Quat::IDENTITY, true));
    let (high_open, high_walled) =
        (mixed(&high, beyond, Quat::IDENTITY, false), mixed(&high, beyond, Quat::IDENTITY, true));
    let low_drop = low_walled.level() / low_open.level();
    let high_drop = high_walled.level() / high_open.level();
    assert!((0.2..0.5).contains(&low_drop), "{low_drop}");
    assert!(high_drop < low_drop * 0.2, "{high_drop} against {low_drop}");
}

#[test]
fn walls_are_looked_for_around_the_sound() {
    let from = Vec3::ZERO;
    let to = Vec3::new(0.0, 0.0, -10.0);
    let mut ends = Vec::new();
    let all = occlusion_between(from, to, &mut |start, end| {
        assert_eq!(start, from);
        ends.push(end);
        true
    });
    assert_eq!(all, 1.0);
    // A sound against a wall comes from that wall, so no ray goes all the way.
    assert!(ends.iter().all(|end| end.z > -10.0), "{ends:?}");
    // A low wall takes only the rays that don't go over it, and a post the middle one.
    let low_wall = occlusion_between(from, to, &mut |_, end| end.y < 1.0);
    assert!(low_wall > 0.0 && low_wall < 1.0, "{low_wall}");
    let post = occlusion_between(from, to, &mut |_, end| end.x.abs() < 0.1 && end.y < 1.0);
    assert!(post > 0.0 && post < low_wall, "{post}");
    // Right next to the sound, nothing is asked.
    assert_eq!(occlusion_between(from, Vec3::X, &mut |_, _| panic!("no need to ask")), 0.0);
}

#[test]
fn a_wall_changes_a_playing_sound_gradually() {
    let path = test_wav("wall.wav");
    let mut audio = Audio::headless();
    let id = audio.play_loop_at(&path, 1.0, Emitter::at(Vec3::new(0.0, 0.0, -5.0))).unwrap();
    audio.update_occlusion(0.016, |_, _| false);
    let blocked = |audio: &Audio| audio.loops.iter().find(|l| l.id == id).unwrap().voice.as_ref().unwrap().blocked;
    assert_eq!(blocked(&audio), 0.0);
    audio.update_occlusion(0.016, |_, _| true);
    let first = blocked(&audio);
    assert!(first > 0.0 && first < 0.2, "{first}");
    for _ in 0..30 {
        audio.update_occlusion(0.016, |_, _| true);
    }
    assert_eq!(blocked(&audio), 1.0);

    // A sound that starts behind a wall doesn't have to fade into it.
    let mut audio = Audio::headless();
    audio.play_loop_at(&path, 1.0, Emitter::at(Vec3::new(0.0, 0.0, -5.0))).unwrap();
    audio.update_occlusion(0.016, |_, _| true);
    assert_eq!(audio.loops[0].voice.as_ref().unwrap().blocked, 1.0);
}

#[test]
fn looking_for_walls_has_a_budget_and_skips_what_cant_be_heard() {
    let path = test_wav("budget.wav");
    let mut audio = Audio::headless();
    let near = |i: usize| Emitter::at(Vec3::new(i as f32 * 0.5, 0.0, -10.0));
    for i in 0..20 {
        audio.play_loop_at(&path, 1.0, near(i)).unwrap();
    }
    audio.play_loop_at(&path, 1.0, Emitter::at(Vec3::new(500.0, 0.0, 0.0))).unwrap();

    let rays = |audio: &mut Audio| {
        let mut count = 0;
        audio.update_occlusion(0.016, |_, _| {
            count += 1;
            true
        });
        count
    };
    // At first everything in earshot gets its answer, and then they take turns.
    assert_eq!(rays(&mut audio), 20 * 4);
    assert_eq!(rays(&mut audio), OCCLUSION_CHECKS * 4);
    assert!(!audio.loops[20].voice.as_ref().unwrap().checked, "out of earshot, it costs nothing");
    assert!(audio.loops[..20].iter().all(|l| l.voice.as_ref().unwrap().checked));

    // Every sound gets its turn: a wall that opens reaches all of them.
    for _ in 0..200 {
        audio.update_occlusion(0.016, |_, _| false);
    }
    assert!(audio.loops[..20].iter().all(|l| l.voice.as_ref().unwrap().blocked == 0.0));
}

#[test]
fn a_loop_follows_the_listener_and_silent_audio_ignores_walls() {
    let path = test_wav("follow.wav");
    let mut audio = Audio::headless();
    let id = audio.play_loop_at(&path, 1.0, Emitter::loud(Vec3::ZERO)).unwrap();
    audio.update();
    let cutoff = |audio: &Audio| audio.loops[0].voice.as_ref().unwrap().heard.cutoff;
    assert_eq!(cutoff(&audio), OPEN_CUTOFF);
    audio.set_listener(Vec3::new(0.0, 0.0, 60.0), Quat::IDENTITY);
    audio.update();
    assert!(cutoff(&audio) < 2000.0, "{}", cutoff(&audio));
    audio.set_position(id, Vec3::new(0.0, 0.0, 59.0));
    audio.update();
    assert_eq!(cutoff(&audio), OPEN_CUTOFF);

    let mut silent = Audio::silent();
    silent.play_pitched_at("missing.wav", 1.0, 1.2, Emitter::default());
    silent.update_occlusion(0.016, |_, _| panic!("nothing to ask about"));
    silent.update();
    assert!(silent.waiting.is_empty());
}

#[test]
fn one_shots_in_the_world_clean_up_after_themselves() {
    let path = tone(200.0);
    let (mut audio, _output) = metered_audio();
    for i in 0..40 {
        audio.play_pitched_at(&path, 0.8, 1.0 + i as f32 * 0.01, Emitter::at(Vec3::new(i as f32, 0.0, 0.0)));
    }
    // Out of earshot it isn't even started.
    audio.play_at(&path, 1.0, Emitter::at(Vec3::new(500.0, 0.0, 0.0)));
    assert_eq!(audio.waiting.len(), 41);
    audio.update();
    assert!(audio.waiting.is_empty());
    assert_eq!(audio.shots.len(), 40);
    assert_eq!(audio.engine.as_ref().unwrap().tracks[Category::Effects.index()].num_sub_tracks(), 40);
    for _ in 0..200 {
        audio.update();
    }
    assert!(audio.shots.is_empty());
    assert_eq!(audio.engine.as_ref().unwrap().tracks[Category::Effects.index()].num_sub_tracks(), 0);
}

#[test]
fn volumes_convert_to_decibels() {
    assert_eq!(gain_to_db(1.0), Decibels::IDENTITY);
    assert_eq!(gain_to_db(0.0), Decibels::SILENCE);
    assert!((gain_to_db(0.5).0 + 6.02).abs() < 0.01);
    assert!((gain_to_db(0.5).as_amplitude() - 0.5).abs() < 1e-4);
}

#[test]
fn sandbox_sounds_decode() {
    let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds"));
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut audio = Audio::headless();
    for entry in entries {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "wav" || e == "ogg") {
            assert!(audio.preload(&path), "{} does not decode", path.display());
        }
    }
}
