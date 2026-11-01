//! Saved games: slots on disk and putting the world back as it was saved.
//! Headless, like `dynamic.rs`.

use std::path::PathBuf;

use grimora::prelude::*;
use grimora::save::{SaveFile, WorldState};
use grimora::scene::{Scene, SceneDesc};

const SCENE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/scenes/sandbox.ron");
const DT: f32 = 1.0 / 60.0;

fn sandbox() -> (Physics, World) {
    let scene = Scene::load(SCENE).unwrap();
    let physics = Physics::from_scene(&scene);
    (physics, World::from_scene(scene))
}

fn object_index(world: &World, name: &str) -> usize {
    world.objects.iter().position(|o| o.name.as_deref() == Some(name)).unwrap()
}

/// Steps physics and the player walking towards -Z, like the engine's frame loop.
fn walk(
    physics: &mut Physics,
    world: &mut World,
    player: &mut ThirdPersonController,
    camera: &mut Camera,
    seconds: f32,
) {
    let mut input = Input::default();
    input.key_down(KeyCode::KeyW);
    for _ in 0..(seconds / DT) as u32 {
        physics.step(DT);
        world.sync_bodies(physics);
        player.update(&input, physics, camera, DT);
        input.end_frame();
    }
}

fn settle(physics: &mut Physics, world: &mut World, seconds: f32) {
    for _ in 0..(seconds / DT) as u32 {
        physics.step(DT);
        world.sync_bodies(physics);
    }
}

fn temp_store(name: &str) -> (SaveStore, PathBuf) {
    let dir = std::env::temp_dir().join(format!("grimora_saves_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    (SaveStore::new(&dir), dir)
}

#[test]
fn a_saved_game_puts_pushed_crates_and_the_player_back() {
    let (mut physics, mut world) = sandbox();
    settle(&mut physics, &mut world, 0.5);
    let crate_index = object_index(&world, "crate");
    let stone = object_index(&world, "stone");
    let altar = object_index(&world, "altar");

    // Shove the crate, hide the altar (as if destroyed), then save.
    let mut camera = Camera::default();
    let mut player = ThirdPersonController::new(Vec3::new(-2.75, world.ground_height(-2.75, 14.0) + 0.1, 14.0), 0.0);
    walk(&mut physics, &mut world, &mut player, &mut camera, 2.0);
    settle(&mut physics, &mut world, 2.0);
    world.objects[altar].visible = false;
    let pushed = world.objects[crate_index].transform;
    let stone_at_save = world.objects[stone].transform;
    assert!(pushed.position.z < 10.5, "the crate wasn't pushed: {}", pushed.position);

    let state = WorldState::capture(&world, &physics, &camera);
    assert!(state.objects.iter().any(|o| o.index == crate_index) && state.objects.iter().any(|o| o.index == altar));
    assert!(state.objects.len() < world.objects.len() / 2, "only changed objects are saved: {}", state.objects.len());
    let (store, dir) = temp_store("round_trip");
    let file = SaveFile::new(Some(SCENE.as_ref()), state, player.state());
    let path = store.write("slot_1", &file).unwrap();
    assert!(path.ends_with("slot_1.ron") && !path.with_extension("ron.tmp").exists());
    let read: SaveFile<CharacterState> = store.read("slot_1").unwrap();
    assert_eq!(read, file);
    assert_eq!(read.scene.as_deref(), Some("sandbox.ron"));

    // Keep playing: knock the stone away, bring the altar back, wander off.
    let mut other = ThirdPersonController::new(Vec3::new(-1.0, world.ground_height(-1.0, 15.5) + 0.1, 15.5), 0.0);
    walk(&mut physics, &mut world, &mut other, &mut camera, 2.0);
    world.objects[altar].visible = true;
    assert!(world.objects[stone].transform.position.distance(stone_at_save.position) > 1.0);
    let saved_camera = read.world.camera;
    camera.position += Vec3::new(30.0, 5.0, 0.0);

    // Loading puts everything back, the physics bodies included.
    let unmatched = read.world.restore(&mut world, &mut physics, &mut camera);
    player.restore(&read.game);
    assert_eq!(unmatched, 0);
    assert_eq!(world.objects[crate_index].transform, pushed);
    assert_eq!(world.objects[stone].transform, stone_at_save, "the stone rolled after saving; it goes back");
    assert!(!world.objects[altar].visible);
    assert_eq!(camera.position, saved_camera.position);
    assert_eq!(player.state(), read.game);
    settle(&mut physics, &mut world, 1.0);
    assert!(world.objects[crate_index].transform.position.distance(pushed.position) < 0.05);
    assert!(world.objects[stone].transform.position.distance(stone_at_save.position) < 0.05);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn objects_that_left_the_scene_are_skipped() {
    let (physics, mut world) = sandbox();
    let altar = object_index(&world, "altar");
    let placed = world.objects[altar].transform;
    world.objects[altar].transform.position.y += 3.0;
    let mut state = WorldState::capture(&world, &physics, &Camera::default());
    let saved = state.objects.iter_mut().find(|o| o.index == altar).unwrap();
    saved.name = Some("renamed".into());

    let (mut physics, mut world) = sandbox();
    let unmatched = state.restore(&mut world, &mut physics, &mut Camera::default());
    assert_eq!(unmatched, 1);
    assert_eq!(world.objects[altar].transform, placed);
}

#[test]
fn loading_streams_the_world_in_around_the_saved_camera() {
    let desc = SceneDesc::parse(
        "(streaming: (cell_size: 20.0, load_distance: 40.0),
          scatter: [(mesh: Cube, count: 300, radius: 150.0, scale: (1.0, 1.0), tilt: 0.0, sink: 0.0, seed: 4)])",
    )
    .unwrap();
    let scene = Scene::build(&desc);
    let mut physics = Physics::from_scene(&scene);
    let mut world = World::from_scene(scene);
    let mut camera = Camera { position: Vec3::new(100.0, 2.0, 0.0), ..Default::default() };
    let state = WorldState::capture(&world, &physics, &camera);

    camera.position = Vec3::ZERO;
    world.load_around(camera.position, &mut physics);
    state.restore(&mut world, &mut physics, &mut camera);
    let near = |o: &Object| (o.transform.position - Vec3::new(100.0, 0.0, 0.0)).with_y(0.0).length() < 40.0 + 30.0;
    assert!(world.all_objects().count() > 0 && world.all_objects().all(near));
}

#[test]
fn slots_are_listed_newest_first_and_can_be_deleted() {
    let (store, dir) = temp_store("slots");
    assert!(store.slots().unwrap().is_empty(), "a missing folder has no slots");
    store.write("first", &1u32).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(30));
    store.write("second", &2u32).unwrap();
    let names: Vec<String> = store.slots().unwrap().into_iter().map(|s| s.name).collect();
    assert_eq!(names, ["second", "first"]);
    assert_eq!(store.read::<u32>("first").unwrap(), 1);

    store.write("first", &3u32).unwrap();
    assert_eq!(store.read::<u32>("first").unwrap(), 3, "writing again replaces the slot");
    store.delete("second").unwrap();
    assert!(!store.exists("second") && store.exists("first"));
    assert!(store.read::<u32>("second").is_err());
    assert!(store.write("../escape", &0u32).is_err());

    std::fs::write(dir.join("broken.ron"), "(not valid").unwrap();
    let err = store.read::<u32>("broken").unwrap_err();
    assert!(format!("{err:#}").contains("broken.ron"), "{err:#}");
    std::fs::remove_dir_all(dir).ok();
}
