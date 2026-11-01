//! Drives the third-person controller through the real sandbox scene with
//! simulated key presses — no window or GPU needed.

use grimora::prelude::*;
use grimora::scene::Scene;

const SCENE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/scenes/sandbox.ron");
const DT: f32 = 1.0 / 60.0;

fn setup() -> (Scene, Physics, ThirdPersonController, Camera) {
    let scene = Scene::load(SCENE).unwrap();
    let physics = Physics::from_scene(&scene);
    let spawn = scene.player.expect("sandbox defines a player spawn");
    let player = ThirdPersonController::new(spawn.position + Vec3::Y * 0.1, spawn.yaw);
    (scene, physics, player, Camera::default())
}

fn run(
    player: &mut ThirdPersonController,
    physics: &mut Physics,
    camera: &mut Camera,
    input: &mut Input,
    seconds: f32,
) {
    for _ in 0..(seconds / DT) as u32 {
        physics.step(DT);
        player.update(input, physics, camera, DT);
        input.end_frame();
    }
}

#[test]
fn settles_on_the_terrain() {
    let (scene, mut physics, mut player, mut camera) = setup();
    run(&mut player, &mut physics, &mut camera, &mut Input::default(), 1.0);
    let p = player.body.position;
    assert!(player.body.grounded);
    assert!(
        (p.y - scene.ground_height(p.x, p.z)).abs() < 0.1,
        "feet {} vs ground {}",
        p.y,
        scene.ground_height(p.x, p.z)
    );
}

#[test]
fn walking_forward_stops_at_the_altar() {
    let (scene, mut physics, mut player, mut camera) = setup();
    let mut input = Input::default();
    input.key_down(KeyCode::KeyW);
    run(&mut player, &mut physics, &mut camera, &mut input, 6.0);

    let p = player.body.position;
    // The altar is 1.5 deep centered on z = 0; the capsule radius is 0.35.
    assert!(p.z > 0.75 && p.z < 2.0, "player at {p}, expected just in front of the altar");
    assert!(p.x.abs() < 1.0, "drifted sideways to {p}");
    assert!((p.y - scene.ground_height(p.x, p.z)).abs() < 0.15);
    // The camera stays behind the player, above the ground.
    assert!(camera.position.z > p.z);
    assert!(camera.position.y > scene.ground_height(camera.position.x, camera.position.z));
}

#[test]
fn jumps_and_lands() {
    let (_, mut physics, mut player, mut camera) = setup();
    let mut input = Input::default();
    run(&mut player, &mut physics, &mut camera, &mut input, 0.5);
    let ground = player.body.position.y;

    input.key_down(KeyCode::Space);
    let mut peak = ground;
    for _ in 0..90 {
        physics.step(DT);
        player.update(&input, &mut physics, &mut camera, DT);
        input.end_frame();
        peak = peak.max(player.body.position.y);
    }
    assert!(peak - ground > 0.8, "jump only reached {}", peak - ground);
    assert!(player.body.grounded && (player.body.position.y - ground).abs() < 0.1);
}

#[test]
fn camera_is_pulled_in_front_of_walls() {
    let (_, mut physics, mut player, mut camera) = setup();
    // Stand right in front of the altar facing away from it, so the camera's
    // default spot (5.5 units behind) is inside/behind the altar.
    player.body.position = Vec3::new(0.0, player.body.position.y, 1.5);
    player.facing = std::f32::consts::PI;
    player.camera_yaw = std::f32::consts::PI;
    player.camera_pitch = -0.05;
    run(&mut player, &mut physics, &mut camera, &mut Input::default(), 1.0);
    let distance = (camera.position - (player.body.position + Vec3::Y * player.camera_height)).length();
    assert!(distance < 4.0, "camera stayed {distance} away, inside the altar");
}
