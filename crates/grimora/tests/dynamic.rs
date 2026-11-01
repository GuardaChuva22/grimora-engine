//! Dynamic bodies in real scenes: falling, resting, stacking and being pushed by
//! the third-person character. Headless, like `character.rs`.

use grimora::prelude::*;
use grimora::scene::{Scene, SceneDesc};

const SCENE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/scenes/sandbox.ron");
const DT: f32 = 1.0 / 60.0;

/// Steps physics and copies the bodies into the world, like the engine's frame loop.
fn simulate(physics: &mut Physics, world: &mut World, seconds: f32) {
    for _ in 0..(seconds / DT) as u32 {
        physics.step(DT);
        world.sync_bodies(physics);
    }
}

fn sandbox() -> (Physics, World) {
    let scene = Scene::load(SCENE).unwrap();
    let physics = Physics::from_scene(&scene);
    (physics, World::from_scene(scene))
}

fn object_index(world: &World, name: &str) -> usize {
    world.objects.iter().position(|o| o.name.as_deref() == Some(name)).unwrap()
}

#[test]
fn body_dropped_on_terrain_comes_to_rest() {
    let desc = SceneDesc::parse(
        "(terrain: (size: 64.0, resolution: 32, height: 6.0, flat_radius: 4.0, seed: 2),
          objects: [(name: \"box\", mesh: Cube, position: (6.0, 4.0, -5.0), rotation: (30.0, 20.0, 10.0),
                     body: Dynamic(density: 500.0))])",
    )
    .unwrap();
    let scene = Scene::build(&desc);
    let mut physics = Physics::from_scene(&scene);
    let mut world = World::from_scene(scene);
    let index = object_index(&world, "box");
    let start = world.objects[index].transform;

    simulate(&mut physics, &mut world, 5.0);
    let rest = world.objects[index].transform;
    assert!(rest.position.y < start.position.y - 3.0, "did not fall: {}", rest.position);
    assert_eq!(rest.scale, start.scale);
    // The box's center sits half a unit (give or take the tilt of the ground) above the terrain.
    let center = rest.position + rest.rotation * Vec3::Y * 0.5;
    let ground = world.ground_height(center.x, center.z);
    assert!(center.y > ground + 0.3 && center.y < ground + 0.8, "center {center} vs ground {ground}");

    simulate(&mut physics, &mut world, 1.0);
    let later = world.objects[index].transform.position;
    assert!(later.distance(rest.position) < 0.01, "still moving: {} -> {later}", rest.position);
}

#[test]
fn sandbox_bodies_rest_where_placed() {
    let (mut physics, mut world) = sandbox();
    assert!(physics.scene_bodies().len() >= 4, "sandbox should have a crate stack and a stone");
    let start: Vec<Transform> = physics.scene_bodies().iter().map(|b| world.objects[b.object].transform).collect();

    // The crate stack and the stone stay put for a few seconds.
    simulate(&mut physics, &mut world, 5.0);
    for (body, start) in physics.scene_bodies().iter().zip(&start) {
        let now = world.objects[body.object].transform;
        assert!(
            now.position.distance(start.position) < 0.05,
            "{:?} drifted from {} to {}",
            world.objects[body.object].name,
            start.position,
            now.position
        );
        assert!(now.rotation.angle_between(start.rotation) < 0.05);
        let ground = world.ground_height(now.position.x, now.position.z);
        assert!(now.position.y > ground - 0.02, "sank into the ground: {}", now.position);
    }
}

/// Center of a generated prop (origin at the base, 1 unit tall) in world space.
fn center(transform: &Transform) -> Vec3 {
    transform.position + transform.rotation * (transform.scale * Vec3::Y * 0.5)
}

/// Walks the player towards -Z from `(x, z)` for `seconds` and returns how far
/// the named object's center moved. Along the way, checks that the player
/// never bounces back and that the object stays on the ground.
fn push(name: &str, x: f32, z: f32, seconds: f32) -> (Vec3, ThirdPersonController) {
    let (mut physics, mut world) = sandbox();
    simulate(&mut physics, &mut world, 0.5);
    let index = object_index(&world, name);
    let start = center(&world.objects[index].transform);

    let mut player = ThirdPersonController::new(Vec3::new(x, world.ground_height(x, z) + 0.1, z), 0.0);
    let mut camera = Camera::default();
    let mut input = Input::default();
    input.key_down(KeyCode::KeyW);
    let mut previous = player.body.position.z;
    for frame in 0..(seconds / DT) as u32 {
        physics.step(DT);
        world.sync_bodies(&physics);
        player.update(&input, &mut physics, &mut camera, DT);
        input.end_frame();
        let z = player.body.position.z;
        assert!(z <= previous + 1e-3, "player jittered back at frame {frame}: {previous} -> {z}");
        previous = z;
        let c = center(&world.objects[index].transform);
        assert!(c.y < start.y + 0.1, "{name} was thrown up to {c} at frame {frame}");
    }
    (center(&world.objects[index].transform) - start, player)
}

#[test]
fn walking_into_a_crate_pushes_it() {
    let (moved, player) = push("crate", -2.75, 14.0, 3.0);
    assert!(moved.z < -1.0, "crate only moved {moved}");
    // The player kept walking (behind the crate) instead of getting stuck at it.
    assert!(player.body.position.z < 11.0, "player stuck at {}", player.body.position);
}

#[test]
fn walking_into_the_stone_rolls_it() {
    let (moved, player) = push("stone", -1.0, 15.5, 3.0);
    assert!(moved.z < -2.0, "stone only moved {moved}");
    assert!(moved.x.abs() < 0.5, "stone veered off by {moved}");
    assert!(player.body.position.z < 13.0, "player stuck at {}", player.body.position);
}
