//! What floats, in real scenes without a window: `World::update_floating` before each step.

use grimora::prelude::*;
use grimora::scene::{Scene, SceneDesc};

const SEA: &str = "sea: (swell: 0.9, length: 40.0, chop: 0.1, steepness: 0.7)";

fn scene(water: &str, objects: &str) -> (World, Physics) {
    let text = format!("SceneDesc(water: [{water}], objects: [{objects}])");
    let scene = Scene::build(&SceneDesc::parse(&text).unwrap());
    let mut physics = Physics::new();
    let world = World::from_scene(scene, &mut physics);
    (world, physics)
}

/// Runs the world as the engine does, from `start` seconds of its clock.
fn run(world: &mut World, physics: &mut Physics, start: f32, seconds: f32) {
    let dt = 1.0 / 60.0;
    for i in 0..(seconds / dt) as u32 {
        world.update_water_view(Vec3::new(0.0, 50.0, 0.0), dt);
        world.update_floating(physics, start + i as f32 * dt);
        physics.step(dt);
        world.sync_bodies(physics);
    }
}

fn pose(world: &World, name: &str) -> Transform {
    let mut query = world.ecs.query::<(&Name, &Transform)>();
    *query.iter().find(|(n, _)| n.0 == name).map(|(_, t)| t).unwrap()
}

#[test]
fn a_box_floats_as_deep_as_it_is_heavy() {
    let (mut world, mut physics) = scene(
        "(position: (0.0, 0.0, 0.0), size: (40.0, 40.0), waves: 0.0)",
        r#"(name: "half", mesh: Cube, position: (0.0, 2.0, 0.0), scale: (2.0, 2.0, 2.0), body: Dynamic(),
            float: (fill: 0.5, cell: 0.25, ballast: 0.5)),
           (name: "light", mesh: Cube, position: (8.0, 2.0, 0.0), body: Dynamic(), float: (fill: 0.2, cell: 0.2, ballast: 0.5))"#,
    );
    run(&mut world, &mut physics, 0.0, 20.0);
    // A cube's origin is the middle of its base: half of 2 meters under the water.
    let half = pose(&world, "half");
    assert!((half.position.y + 1.0).abs() < 0.06, "{}", half.position);
    let light = pose(&world, "light");
    assert!((light.position.y + 0.2).abs() < 0.05, "{}", light.position);
}

#[test]
fn what_is_heavier_than_water_sinks() {
    let (mut world, mut physics) = scene(
        "(position: (0.0, 0.0, 0.0), size: (40.0, 40.0), waves: 0.0)",
        r#"(name: "stone", mesh: Cube, position: (0.0, 1.0, 0.0), body: Dynamic(), float: (fill: 2.0, cell: 0.25))"#,
    );
    run(&mut world, &mut physics, 0.0, 6.0);
    assert!(pose(&world, "stone").position.y < -8.0, "{}", pose(&world, "stone").position);
}

#[test]
fn a_tipped_hull_rights_itself() {
    let (mut world, mut physics) = scene(
        "(position: (0.0, 0.0, 0.0), size: (40.0, 40.0), waves: 0.0)",
        r#"(name: "hull", mesh: Cube, position: (0.0, 0.0, 0.0), rotation: (0.0, 0.0, 35.0), scale: (6.0, 1.0, 2.0),
            body: Dynamic(), float: (fill: 0.4, cell: 0.25, ballast: 0.7))"#,
    );
    run(&mut world, &mut physics, 0.0, 25.0);
    let up = pose(&world, "hull").rotation * Vec3::Y;
    assert!(up.y > 0.99, "{up}");
}

#[test]
fn a_boat_rides_the_waves_it_is_drawn_on() {
    let (mut world, mut physics) = scene(
        &format!("({SEA}, position: (0.0, 0.0, 0.0))"),
        r#"(name: "hull", mesh: Cube, position: (0.0, 0.0, 0.0), scale: (6.0, 1.2, 2.0),
            body: Dynamic(), float: (fill: 0.4, cell: 0.3, ballast: 0.6))"#,
    );
    run(&mut world, &mut physics, 0.0, 15.0);
    let (mut lowest, mut highest, mut most_tipped) = (f32::MAX, f32::MIN, 1.0f32);
    let mut gap = 0.0f32;
    for i in 0..60 {
        run(&mut world, &mut physics, 15.0 + i as f32 * 0.5, 0.5);
        let hull = pose(&world, "hull");
        lowest = lowest.min(hull.position.y);
        highest = highest.max(hull.position.y);
        most_tipped = most_tipped.min((hull.rotation * Vec3::Y).y);
        let surface = world.water_at(hull.position.x, hull.position.z).unwrap().level;
        // Its base stays about as far under as a still sea would leave it.
        gap = gap.max((hull.position.y + 0.48 - surface).abs());
    }
    assert!(highest - lowest > 0.5, "it should go up and down: {lowest} {highest}");
    assert!(most_tipped > 0.82, "it should rock, not capsize: {most_tipped}");
    assert!(gap < 0.75, "it should follow the surface: {gap}");
}

#[test]
fn the_ships_of_the_sea_scene_float_upright() {
    let scene = Scene::load("../../assets/scenes/sea.ron").unwrap();
    let mut physics = Physics::new();
    let mut world = World::from_scene(scene, &mut physics);
    run(&mut world, &mut physics, 0.0, 30.0);
    // How far under the water each one's origin sits, and how far its deck stays out of it.
    for (name, deck, least_out) in [("chalupa", 1.85, 0.5), ("bote", 0.88, 0.2)] {
        let (mut out, mut most_tipped) = (f32::MAX, 1.0f32);
        for i in 0..80 {
            run(&mut world, &mut physics, 30.0 + i as f32 * 0.5, 0.5);
            let ship = pose(&world, name);
            let surface = world.water_at(ship.position.x, ship.position.z).unwrap().level;
            out = out.min(ship.position.y + deck - surface);
            most_tipped = most_tipped.min((ship.rotation * Vec3::Y).y);
        }
        println!("{name}: {} out {out} tipped {most_tipped}", pose(&world, name).position);
        assert!(out > least_out, "{name} ships water: {out}");
        assert!(out < deck + 1.0, "{name} barely touches it: {out}");
        assert!(most_tipped > 0.9, "{name} rolls too far: {most_tipped}");
    }
}

#[test]
fn the_river_scene_has_water_where_it_should() {
    let scene = Scene::load("../../assets/scenes/river.ron").unwrap();
    let mut physics = Physics::new();
    let mut world = World::from_scene(scene, &mut physics);
    let (rim, pond) = (world.ground_height(14.0, 22.0), world.ground_height(14.0, 10.0));
    let level = world.water_at(14.0, 10.0).unwrap().level;
    println!("pond: rim {rim} level {level} bottom {pond}");
    assert!(pond < level - 1.8 && level < rim, "the pond should be deep, and held in");
    // The stones in the river stand on its bed and break the surface.
    let river = world.water_at(-8.0, -22.0).unwrap();
    println!("river at the stone: level {} ground {}", river.level, world.ground_height(-8.0, -22.0));
    assert!(river.flow.length() > 1.5);
    assert!(world.ground_height(-8.0, -22.0) < river.level - 1.0, "the river should dig its bed");

    let start = pose(&world, "tronco").position;
    run(&mut world, &mut physics, 0.0, 12.0);
    let log = pose(&world, "tronco").position;
    assert!(start.distance(log) > 12.0, "the river should take it: {log}");
    let bote = pose(&world, "bote").position;
    assert!(bote.distance(Vec3::new(14.0, level, 10.0)) < 3.0, "the pond should keep it: {bote}");
}

#[test]
fn a_river_carries_what_floats() {
    let (mut world, mut physics) = {
        let text = r#"SceneDesc(
            rivers: [(points: [(0.0, 0.0, 0.0), (60.0, 0.0, 0.0)], width: 10.0, speed: 2.0)],
            objects: [(name: "log", mesh: Cube, position: (5.0, 0.2, 0.0), body: Dynamic(), float: (fill: 0.4, cell: 0.25))],
        )"#;
        let scene = Scene::build(&SceneDesc::parse(text).unwrap());
        let mut physics = Physics::new();
        let world = World::from_scene(scene, &mut physics);
        (world, physics)
    };
    run(&mut world, &mut physics, 0.0, 8.0);
    assert!(pose(&world, "log").position.x > 12.0, "{}", pose(&world, "log").position);
}

/// Runs a character in the water, without a window, with these keys held.
fn swim(world: &World, physics: &mut Physics, player: &mut ThirdPersonController, keys: &[KeyCode], seconds: f32) {
    let dt = 1.0 / 60.0;
    let mut camera = Camera::default();
    let mut input = Input::default();
    for key in keys {
        input.key_down(*key);
    }
    for _ in 0..(seconds / dt) as u32 {
        player.water = world.water_at(player.body.position.x, player.body.position.z);
        player.update(&input, physics, &mut camera, dt);
        physics.step(dt);
    }
}

const POOL: &str = "(position: (0.0, 0.0, 0.0), size: (300.0, 300.0), waves: 0.0)";

#[test]
fn a_character_floats_dives_and_comes_back_up() {
    let (world, mut physics) = scene(POOL, "");
    let mut player = ThirdPersonController::new(Vec3::new(0.0, 3.0, 0.0), 0.0);
    swim(&world, &mut physics, &mut player, &[], 4.0);
    assert!(player.swimming(), "{}", player.body.position);
    assert!(!player.head_under(), "{}", player.body.position);
    assert!((player.body.position.y + 1.45).abs() < 0.05, "{}", player.body.position);

    // Forward along the surface, without sinking.
    let start = player.body.position;
    swim(&world, &mut physics, &mut player, &[KeyCode::KeyW], 3.0);
    assert!(start.distance(player.body.position) > 4.5, "{}", player.body.position);
    assert!((player.body.position.y + 1.45).abs() < 0.05, "{}", player.body.position);

    swim(&world, &mut physics, &mut player, &[KeyCode::ControlLeft], 3.0);
    assert!(player.diving());
    assert!(player.head_under());
    assert!(player.body.position.y < -6.0, "{}", player.body.position);

    // Left alone, it only drifts up slowly.
    let deep = player.body.position.y;
    swim(&world, &mut physics, &mut player, &[], 2.0);
    assert!(player.body.position.y > deep, "{}", player.body.position);
    assert!(player.head_under());

    swim(&world, &mut physics, &mut player, &[KeyCode::Space], 6.0);
    assert!(!player.diving(), "{}", player.body.position);
    assert!(!player.head_under(), "{}", player.body.position);
}

#[test]
fn a_diver_swims_where_the_camera_looks() {
    let (world, mut physics) = scene(POOL, "");
    let mut player = ThirdPersonController::new(Vec3::new(0.0, -6.0, 0.0), 0.0);
    swim(&world, &mut physics, &mut player, &[KeyCode::ControlLeft], 0.5);
    let start = player.body.position;
    player.camera_pitch = -0.9;
    swim(&world, &mut physics, &mut player, &[KeyCode::KeyW], 2.0);
    assert!(player.body.position.y < start.y - 1.5, "{}", player.body.position);
    player.camera_pitch = 0.9;
    let low = player.body.position.y;
    swim(&world, &mut physics, &mut player, &[KeyCode::KeyW], 2.0);
    assert!(player.body.position.y > low + 1.5, "{}", player.body.position);
}

#[test]
fn shallow_water_is_waded_through() {
    let (world, mut physics) = scene(
        POOL,
        r#"(name: "shoal", mesh: Cube, position: (0.0, -2.8, 0.0), scale: (80.0, 2.0, 80.0), collider: Hull)"#,
    );
    physics.refresh();
    let mut player = ThirdPersonController::new(Vec3::new(0.0, -0.75, 0.0), 0.0);
    swim(&world, &mut physics, &mut player, &[], 1.0);
    assert!(!player.swimming(), "{}", player.body.position);
    assert!(player.body.grounded);
    let start = player.body.position;
    swim(&world, &mut physics, &mut player, &[KeyCode::KeyW], 2.0);
    let waded = start.distance(player.body.position);
    assert!(waded < player.walk_speed * 2.0 * 0.8, "it should be slower than on land: {waded}");
    assert!(waded > 1.0, "{waded}");
}

#[test]
fn a_current_carries_a_swimmer() {
    let text =
        r#"SceneDesc(rivers: [(points: [(0.0, 0.0, 0.0), (80.0, 0.0, 0.0)], width: 14.0, depth: 4.0, speed: 2.0)])"#;
    let scene = Scene::build(&SceneDesc::parse(text).unwrap());
    let mut physics = Physics::new();
    let world = World::from_scene(scene, &mut physics);
    let level = world.water_at(10.0, 0.0).unwrap().level;
    let mut player = ThirdPersonController::new(Vec3::new(10.0, level - 1.45, 0.0), 0.0);
    swim(&world, &mut physics, &mut player, &[], 5.0);
    assert!(player.swimming());
    assert!(player.body.position.x > 16.0, "{}", player.body.position);
}

#[test]
fn something_falling_in_splashes_once() {
    let (mut world, mut physics) = scene(
        POOL,
        r#"(name: "crate", mesh: Cube, position: (0.0, 4.0, 0.0), body: Dynamic(), float: (fill: 0.4, cell: 0.25))"#,
    );
    run(&mut world, &mut physics, 0.0, 4.0);
    assert_eq!(world.take_splashes().len(), 1);
}

/// How a point is seen by what asks about water, on a boat that has settled.
#[test]
fn no_water_gets_into_a_hollow_hull() {
    let scene = Scene::load("../../assets/scenes/sea.ron").unwrap();
    let mut physics = Physics::new();
    let mut world = World::from_scene(scene, &mut physics);
    run(&mut world, &mut physics, 0.0, 10.0);
    assert_eq!(world.waters.hulls.len(), 2, "the chalupa and the bote");
    for (name, inside) in [("chalupa", Vec3::new(0.0, 0.7, 0.0)), ("bote", Vec3::new(0.0, 0.25, 0.0))] {
        let ship = pose(&world, name);
        let point = ship.position + ship.rotation * inside;
        assert!(world.waters.in_hull(point), "{name}: {point}");
        assert!(world.water_around(point).is_none(), "{name}");
        // Well under the keel and off to the side, the water is back.
        for away in [Vec3::new(0.0, -6.0, 0.0), Vec3::new(0.0, 0.0, 14.0)] {
            assert!(!world.waters.in_hull(ship.position + away), "{name}: {away}");
            assert!(world.water_around(ship.position + away).is_some(), "{name}: {away}");
        }
    }
}

#[test]
fn a_river_falls_off_high_ground() {
    let scene = Scene::load("../../assets/scenes/river.ron").unwrap();
    let (effects, sounds) = (scene.effects.len(), scene.sounds.len());
    assert!(effects >= 1 && sounds >= 1, "the fall should have its mist and its roar: {effects} {sounds}");
    let mut physics = Physics::new();
    let world = World::from_scene(scene, &mut physics);
    let (above, below) = (world.water_at(-85.0, -52.0).unwrap().level, world.water_at(-50.0, -33.0).unwrap().level);
    println!("river: {above} above the fall, {below} below it");
    assert!(above - below > 8.0, "{above} {below}");
}
