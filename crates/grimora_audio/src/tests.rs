use super::*;

/// Writes a short 16-bit mono WAV (a quiet sine) and returns its path.
fn test_wav(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("grimora_audio_test_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let rate = 8000u32;
    let samples: Vec<i16> = (0..rate / 4).map(|i| ((i as f32 * 0.25).sin() * 8000.0) as i16).collect();
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
    std::fs::write(&path, bytes).unwrap();
    path
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
