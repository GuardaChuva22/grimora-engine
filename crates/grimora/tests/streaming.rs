//! Streaming: scattered objects and region files come and go with the focus
//! point, colliders and lights included. Headless, like `character.rs`.

use std::path::Path;
use std::time::{Duration, Instant};

use grimora::prelude::*;
use grimora::scene::{Scene, SceneDesc};

fn raycast_down(physics: &Physics, at: Vec3) -> bool {
    physics.raycast(Vec3::new(at.x, 5.0, at.z), Vec3::NEG_Y, 10.0).is_some()
}

/// Horizontal distance from the origin of the farthest object in the world.
fn reach(world: &World, from: Vec3) -> f32 {
    world.all_objects().map(|o| (o.transform.position - from).with_y(0.0).length()).fold(0.0, f32::max)
}

#[test]
fn scattered_objects_follow_the_focus() {
    let desc = SceneDesc::parse(
        "(streaming: (cell_size: 20.0, load_distance: 50.0),
          scatter: [(mesh: Cube, count: 400, radius: 200.0, scale: (1.0, 1.0), tilt: 0.0, sink: 0.0, seed: 4)])",
    )
    .unwrap();
    let scene = Scene::build(&desc);
    let streaming = scene.streaming.as_ref().unwrap();
    let streamed: Vec<Vec3> = streaming.cells.values().flat_map(|c| &c.objects).map(|o| o.transform.position).collect();
    assert_eq!(streamed.len(), 400);
    assert!(scene.objects.is_empty(), "scatter goes to cells, not to the always-present objects");
    let far = *streamed.iter().find(|p| p.length() > 150.0).unwrap();

    let mut physics = Physics::from_scene(&scene);
    let mut world = World::from_scene(scene);
    assert_eq!(world.all_objects().count(), 0, "nothing loads before it's asked to");

    world.load_around(Vec3::ZERO, &mut physics);
    let near = world.streaming_stats().unwrap();
    assert!(near.objects > 0 && near.objects < 400, "{near:?}");
    // Loaded cells touch the 50 m circle, so their objects sit within 50 m + a cell diagonal.
    assert!(reach(&world, Vec3::ZERO) < 50.0 + 20.0 * 1.5);
    assert!(!raycast_down(&physics, far), "far cells have no colliders yet");

    // Walking there brings cells in a few per frame and drops the ones behind.
    world.update_streaming(far, &mut physics);
    assert!(world.streaming_stats().unwrap().loaded_cells <= near.loaded_cells + 8);
    for _ in 0..30 {
        world.update_streaming(far, &mut physics);
    }
    physics.refresh();
    assert!(raycast_down(&physics, far), "the cube there is solid now");
    assert!(world.all_objects().all(|o| (o.transform.position - far).with_y(0.0).length() < 50.0 + 20.0 * 1.5 + 10.0));
    assert!(!raycast_down(
        &physics,
        streamed.iter().copied().min_by(|a, b| a.length().total_cmp(&b.length())).unwrap()
    ));
}

/// Calls `update_streaming` until `done` holds, as frames would.
fn stream_until(world: &mut World, physics: &mut Physics, focus: Vec3, done: impl Fn(&World) -> bool) {
    let started = Instant::now();
    while !done(world) {
        assert!(started.elapsed() < Duration::from_secs(10), "timed out streaming around {focus}");
        world.update_streaming(focus, physics);
        std::thread::sleep(Duration::from_millis(2));
    }
    physics.refresh();
}

fn mesh_count(world: &World) -> usize {
    (0..).take_while(|&i| world.mesh(MeshId(i)).is_some()).count()
}

#[test]
fn regions_load_in_the_background_and_unload_when_left() {
    let dir = std::env::temp_dir().join(format!("grimora_regions_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("shrine.ron"),
        "(objects: [(name: \"shrine\", mesh: Pillar(seed: 2), position: (100.0, 0.0, 0.0),
                     light: (position: (0.0, 2.0, 0.0), radius: 8.0))],
          sounds: [(path: \"hum.wav\", position: (100.0, 1.0, 0.0))])",
    )
    .unwrap();
    let desc = SceneDesc::parse(
        "(streaming: (cell_size: 32.0, load_distance: 40.0),
          regions: [(path: \"shrine.ron\", center: (100.0, 0.0, 0.0), radius: 5.0),
                    (path: \"missing.ron\", center: (-100.0, 0.0, 0.0), radius: 5.0)])",
    )
    .unwrap();
    let scene = Scene::build_in(&desc, &dir);
    let mut physics = Physics::from_scene(&scene);
    let mut world = World::from_scene(scene);
    let shrine = Vec3::new(100.0, 0.0, 0.0);

    world.load_around(Vec3::ZERO, &mut physics);
    assert!(world.find("shrine").is_none(), "60 m away from a 40 m reach");
    assert!(world.active_lights().is_empty() && world.active_sounds().is_empty());

    stream_until(&mut world, &mut physics, Vec3::new(70.0, 0.0, 0.0), |w| w.find("shrine").is_some());
    assert_eq!(world.active_lights().len(), 1);
    let sounds = world.active_sounds();
    assert!(sounds.len() == 1 && sounds[0].path.ends_with("hum.wav"), "{sounds:?}");
    assert!(raycast_down(&physics, shrine), "the pillar collides");
    let meshes = mesh_count(&world);

    // Far away it's gone, collider and light included.
    world.update_streaming(Vec3::new(-50.0, 0.0, 0.0), &mut physics);
    physics.refresh();
    assert!(world.find("shrine").is_none());
    assert!(!raycast_down(&physics, shrine));
    assert!(world.active_lights().is_empty());

    // A missing file is logged once and otherwise ignored.
    world.load_around(Vec3::new(-100.0, 0.0, 0.0), &mut physics);
    assert_eq!(world.streaming_stats().unwrap().loaded_regions, 0);

    // Coming back loads it again (right away with load_around), reusing its meshes.
    world.load_around(shrine, &mut physics);
    assert!(world.find("shrine").is_some() && raycast_down(&physics, shrine));
    assert_eq!(mesh_count(&world), meshes);
    std::fs::remove_dir_all(Path::new(&dir)).ok();
}
