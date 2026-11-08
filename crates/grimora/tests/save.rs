//! Saved games: slots on disk and putting the world back as it was saved.
//! Headless, like `dynamic.rs`.

use std::path::PathBuf;

use grimora::SceneIndex;
use grimora::prelude::*;
use grimora::save::{SaveFile, WorldState};
use grimora::scene::{Scene, SceneDesc};

const SCENE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/scenes/sandbox.ron");
const DT: f32 = 1.0 / 60.0;

fn sandbox() -> (Physics, World) {
    let mut physics = Physics::new();
    let world = World::from_scene(Scene::load(SCENE).unwrap(), &mut physics);
    (physics, world)
}

fn entity(world: &World, name: &str) -> Entity {
    world.find(name).unwrap()
}

fn scene_index(world: &World, entity: Entity) -> usize {
    world.get::<SceneIndex>(entity).unwrap().0
}

fn transform(world: &World, entity: Entity) -> Transform {
    world.transform(entity).unwrap()
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
    let crate_entity = entity(&world, "crate");
    let stone = entity(&world, "stone");
    let altar = entity(&world, "altar");

    // Shove the crate, hide the altar (as if destroyed), then save.
    let mut camera = Camera::default();
    let mut player = ThirdPersonController::new(Vec3::new(-2.75, world.ground_height(-2.75, 14.0) + 0.1, 14.0), 0.0);
    walk(&mut physics, &mut world, &mut player, &mut camera, 2.0);
    settle(&mut physics, &mut world, 2.0);
    world.get_mut::<Renderable>(altar).unwrap().visible = false;
    let pushed = transform(&world, crate_entity);
    let stone_at_save = transform(&world, stone);
    assert!(pushed.position.z < 10.5, "the crate wasn't pushed: {}", pushed.position);

    let state = WorldState::capture(&world, &physics, &camera);
    let saved = |e| state.objects.iter().any(|o| o.index == scene_index(&world, e));
    assert!(saved(crate_entity) && saved(altar));
    let scene_objects = world.ecs.query::<&SceneIndex>().iter().count();
    assert!(state.objects.len() < scene_objects / 2, "only changed objects are saved: {}", state.objects.len());
    let (store, dir) = temp_store("round_trip");
    let file = SaveFile::new(Some(SCENE.as_ref()), state, player.state(world.origin()));
    let path = store.write("slot_1", &file).unwrap();
    assert!(path.ends_with("slot_1.ron") && !path.with_extension("ron.tmp").exists());
    let read: SaveFile<CharacterState> = store.read("slot_1").unwrap();
    assert_eq!(read, file);
    assert_eq!(read.scene.as_deref(), Some("sandbox.ron"));

    // Keep playing: knock the stone away, bring the altar back, wander off.
    let mut other = ThirdPersonController::new(Vec3::new(-1.0, world.ground_height(-1.0, 15.5) + 0.1, 15.5), 0.0);
    walk(&mut physics, &mut world, &mut other, &mut camera, 2.0);
    world.get_mut::<Renderable>(altar).unwrap().visible = true;
    assert!(transform(&world, stone).position.distance(stone_at_save.position) > 1.0);
    let saved_camera = read.world.camera;
    camera.position += Vec3::new(30.0, 5.0, 0.0);

    // Loading puts everything back, the physics bodies included.
    let unmatched = read.world.restore(&mut world, &mut physics, &mut camera);
    player.restore(&read.game, world.origin());
    assert_eq!(unmatched, 0);
    assert_eq!(transform(&world, crate_entity), pushed, "entities keep their ids across a load");
    assert_eq!(transform(&world, stone), stone_at_save, "the stone rolled after saving; it goes back");
    assert!(!world.get::<Renderable>(altar).unwrap().visible);
    assert_eq!(world.origin().to_world(camera.position), saved_camera.position);
    assert_eq!(player.state(world.origin()), read.game);
    settle(&mut physics, &mut world, 1.0);
    assert!(transform(&world, crate_entity).position.distance(pushed.position) < 0.05);
    assert!(transform(&world, stone).position.distance(stone_at_save.position) < 0.05);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn objects_that_left_the_scene_are_skipped() {
    let (physics, mut world) = sandbox();
    let altar = entity(&world, "altar");
    let placed = transform(&world, altar);
    world.get_mut::<Transform>(altar).unwrap().position.y += 3.0;
    let mut state = WorldState::capture(&world, &physics, &Camera::default());
    let saved = state.objects.iter_mut().find(|o| o.index == scene_index(&world, altar)).unwrap();
    saved.name = Some("renamed".into());

    let (mut physics, mut world) = sandbox();
    let unmatched = state.restore(&mut world, &mut physics, &mut Camera::default());
    assert_eq!(unmatched, 1);
    assert_eq!(transform(&world, entity(&world, "altar")), placed);
}

#[test]
fn despawned_scene_objects_stay_gone_and_come_back_with_older_saves() {
    let (mut physics, mut world) = sandbox();
    // Loading streams the world in around the camera; start with the same cells.
    world.load_around(Camera::default().position, &mut physics);
    let colliders = physics.collider_count();
    let before = WorldState::capture(&world, &physics, &Camera::default());
    let altar = entity(&world, "altar");
    let index = scene_index(&world, altar);
    assert!(world.despawn(altar, &mut physics));
    assert!(!world.despawn(altar, &mut physics), "already gone");
    assert_eq!(physics.collider_count(), colliders - 1, "its collider went with it");
    let after = WorldState::capture(&world, &physics, &Camera::default());
    assert!(after.objects.iter().any(|o| o.index == index && o.removed));

    // A save from before the altar was destroyed brings it back, collider included.
    before.restore(&mut world, &mut physics, &mut Camera::default());
    let respawned = world.find("altar").expect("respawned");
    assert_ne!(respawned, altar, "a despawned id is never reused");
    assert_eq!(physics.collider_count(), colliders);
    assert!(world.get::<Name>(altar).is_none());

    after.restore(&mut world, &mut physics, &mut Camera::default());
    assert!(world.find("altar").is_none());
    assert_eq!(physics.collider_count(), colliders - 1);
}

#[test]
fn loading_streams_the_world_in_around_the_saved_camera() {
    let desc = SceneDesc::parse(
        "(streaming: (cell_size: 20.0, load_distance: 40.0),
          scatter: [(mesh: Cube, count: 300, radius: 150.0, scale: (1.0, 1.0), tilt: 0.0, sink: 0.0, seed: 4)])",
    )
    .unwrap();
    let scene = Scene::build(&desc);
    let mut physics = Physics::new();
    let mut world = World::from_scene(scene, &mut physics);
    let mut camera = Camera { position: Vec3::new(100.0, 2.0, 0.0), ..Default::default() };
    let state = WorldState::capture(&world, &physics, &camera);

    camera.position = Vec3::ZERO;
    world.load_around(camera.position, &mut physics);
    state.restore(&mut world, &mut physics, &mut camera);
    let near = |t: &Transform| (t.position - Vec3::new(100.0, 0.0, 0.0)).with_y(0.0).length() < 40.0 + 30.0;
    assert!(!world.ecs.is_empty() && world.ecs.query::<&Transform>().iter().all(near));
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

#[test]
fn a_saved_game_keeps_the_hour() {
    let (mut physics, mut world) = sandbox();
    let mut camera = Camera::default();
    world.set_time_of_day(3.0);
    let state = WorldState::capture(&world, &physics, &camera);
    assert_eq!(state.time_of_day, Some(3.0));

    world.set_time_of_day(15.0);
    state.restore(&mut world, &mut physics, &mut camera);
    assert_eq!(world.time_of_day(), Some(3.0));
    assert!(world.is_night());

    // A save from before the day cycle leaves the clock where it is.
    let old = WorldState { time_of_day: None, ..state };
    world.set_time_of_day(15.0);
    old.restore(&mut world, &mut physics, &mut camera);
    assert_eq!(world.time_of_day(), Some(15.0));
}

#[test]
fn saves_hold_world_positions_whatever_the_origin() {
    let (mut physics, mut world) = sandbox();
    settle(&mut physics, &mut world, 0.5);
    let crate_entity = entity(&world, "crate");
    let mut camera = Camera { position: Vec3::new(3.0, 6.0, 20.0), ..Camera::default() };
    let mut player = ThirdPersonController::new(Vec3::new(-2.75, world.ground_height(-2.75, 14.0) + 0.1, 14.0), 0.0);
    walk(&mut physics, &mut world, &mut player, &mut camera, 2.0);
    settle(&mut physics, &mut world, 2.0);
    let pushed = world.origin().to_world(transform(&world, crate_entity).position);

    // The origin goes a long way off, as it would after a long walk, and everything goes with it.
    let delta = Vec3::new(4096.0, 0.0, -2048.0);
    world.shift_origin(delta, &mut physics);
    camera.position -= delta;
    player.shift(-delta);
    let state = WorldState::capture(&world, &physics, &camera);
    let saved = player.state(world.origin());
    let crate_index = scene_index(&world, crate_entity);
    let crate_state = state.objects.iter().find(|o| o.index == crate_index).unwrap();
    assert!(crate_state.transform.position.distance(pushed) < 1e-3, "{} {pushed}", crate_state.transform.position);

    // A fresh world, with the origin at zero again, puts it all where it was in the world.
    let (mut physics, mut world) = sandbox();
    let mut camera = Camera::default();
    state.restore(&mut world, &mut physics, &mut camera);
    let mut player = ThirdPersonController::new(Vec3::ZERO, 0.0);
    player.restore(&saved, world.origin());
    let crate_entity = entity(&world, "crate");
    assert!(world.origin().to_world(transform(&world, crate_entity).position).distance(pushed) < 1e-3);
    assert!(world.origin().to_world(camera.position).distance(state.camera.position) < 1e-3);
    assert!(world.origin().to_world(player.body.position).distance(saved.position) < 1e-3);
}
