//! Sound in the world through the runner: what the scene's colliders do to it. Headless,
//! with a mixer that doesn't open a device.

use std::path::PathBuf;

use grimora::prelude::*;

const DT: f32 = 1.0 / 60.0;
const SOUND: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/step_1.wav");
const SCENE: &str = r#"(objects: [
    (name: "wall", mesh: Cube, position: (0.0, 0.0, -10.0), scale: (40.0, 12.0, 1.0)),
])"#;

struct Idle;

impl Game for Idle {
    fn update(&mut self, _ctx: &mut Context) {}
}

struct Folder(PathBuf);

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn start() -> (Context, Folder) {
    let dir = std::env::temp_dir().join(format!("grimora_audio_world_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("scene.ron"), SCENE).unwrap();
    let config =
        EngineConfig { scene: Some(dir.join("scene.ron")), save_dir: Some(dir.join("saves")), ..Default::default() };
    let mut ctx = Context::headless(config, &mut Idle).unwrap();
    ctx.audio = Audio::headless();
    (ctx, Folder(dir))
}

#[test]
fn a_wall_in_the_scene_muffles_what_is_behind_it() {
    let (mut ctx, _dir) = start();
    ctx.camera = Camera::new(Vec3::new(0.0, 1.6, 0.0), 0.0, 0.0);
    ctx.headless_frame(&mut Idle, DT).unwrap();

    // The same distance away, one behind the wall and one in the open, off to the side.
    let walled = Vec3::new(0.0, 1.0, -14.0);
    let open = Vec3::new(14.0, 1.0, 0.0);
    ctx.audio.play_at(SOUND, 1.0, Emitter::at(walled));
    ctx.audio.play_at(SOUND, 1.0, Emitter::at(open));
    ctx.headless_frame(&mut Idle, DT).unwrap();

    let heard = |at: Vec3| ctx.audio.world_sounds().find(|(emitter, _)| emitter.position == at).unwrap().1;
    let (walled, open) = (heard(walled), heard(open));
    assert!(walled.gain < open.gain * 0.5, "{walled:?} {open:?}");
    assert!(walled.cutoff < open.cutoff * 0.25, "{walled:?} {open:?}");
}

#[test]
fn the_sounds_of_a_frame_start_from_where_the_camera_ended_up() {
    let (mut ctx, _dir) = start();
    let at = Vec3::new(30.0, 1.0, 0.0);
    ctx.camera = Camera::new(Vec3::new(30.0, 1.0, 1.0), 0.0, 0.0);
    ctx.audio.play_at(SOUND, 1.0, Emitter::at(at));
    ctx.headless_frame(&mut Idle, DT).unwrap();
    let heard = ctx.audio.world_sounds().next().unwrap().1;
    assert_eq!(heard.gain, 1.0, "the camera was already there when the sound started");
}
