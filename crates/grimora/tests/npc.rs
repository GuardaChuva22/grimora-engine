//! Characters walking the real physics world, headless, like `character.rs`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use grimora::physics::RayHit;
use grimora::prelude::*;
use grimora::save::WorldState;
use grimora::scene::{Scene, SceneDesc};

const DT: f32 = 1.0 / 60.0;
const SANDBOX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/scenes/sandbox.ron");
/// Far enough from everything that nobody notices or avoids them.
const NOBODY: Vec3 = Vec3::new(50.0, 1.0, 50.0);

/// A world with a flat 100×100 floor at y = 0 plus the given objects.
fn world(objects: &str) -> (Physics, World) {
    let source = format!(
        r#"(objects: [
            (name: "floor", mesh: Cube, position: (0.0, -1.0, 0.0), scale: (100.0, 1.0, 100.0), snap_to_ground: false),
            {objects}
        ])"#
    );
    let mut physics = Physics::new();
    let world = World::from_scene(Scene::build(&SceneDesc::parse(&source).unwrap()), &mut physics);
    (physics, world)
}

/// Runs the world for a while, as the engine would, returning every event.
fn run(physics: &mut Physics, world: &mut World, player: Vec3, seconds: f32) -> Vec<NpcEvent> {
    let mut events = Vec::new();
    for _ in 0..(seconds / DT).round() as u32 {
        physics.step(DT);
        events.extend(world.update_npcs(physics, player, DT));
        world.update_animations(DT);
    }
    events
}

fn position(world: &World, name: &str) -> Vec3 {
    world.transform(world.find(name).unwrap_or_else(|| panic!("no {name}"))).unwrap().position
}

fn flat_distance(a: Vec3, b: Vec3) -> f32 {
    (a.x - b.x).hypot(a.z - b.z)
}

fn agent_at(world: &mut World, name: &str) -> Entity {
    let entity = world.find(name).unwrap();
    // These tests give the orders themselves.
    world.ecs.remove_one::<Brain>(entity).unwrap();
    entity
}

#[test]
fn walks_to_where_it_is_told_and_stands_on_the_floor() {
    let (mut physics, mut world) = world(r#"(name: "a", mesh: Cube, npc: ())"#);
    let entity = agent_at(&mut world, "a");
    let to = Vec3::new(6.0, 0.0, -3.0);
    world.get_mut::<Agent>(entity).unwrap().go_to(to);
    run(&mut physics, &mut world, NOBODY, 0.5);
    assert_eq!(world.get::<Agent>(entity).unwrap().status(), AgentStatus::Moving);

    run(&mut physics, &mut world, NOBODY, 6.0);
    let at = position(&world, "a");
    assert_eq!(world.get::<Agent>(entity).unwrap().status(), AgentStatus::Arrived);
    assert!(flat_distance(at, to) < 0.6, "{at}");
    assert!(at.y.abs() < 0.05, "{at}");
    // It ends up facing the way it walked: mostly +X, so a yaw near -90 degrees or so.
    let forward = world.transform(entity).unwrap().rotation * Vec3::NEG_Z;
    assert!(forward.dot(to.normalize()) > 0.8, "{forward}");
}

#[test]
fn goes_around_walls_and_never_through_them() {
    let (mut physics, mut world) = world(
        r#"(name: "a", mesh: Cube, npc: ()),
           (mesh: Cube, position: (0.0, 0.0, -4.0), scale: (10.0, 2.0, 0.5))"#,
    );
    let entity = agent_at(&mut world, "a");
    let to = Vec3::new(0.0, 0.0, -8.0);
    world.get_mut::<Agent>(entity).unwrap().go_to(to);
    for _ in 0..(14.0 / DT) as u32 {
        physics.step(DT);
        world.update_npcs(&mut physics, NOBODY, DT);
        let at = position(&world, "a");
        assert!(!(at.x.abs() < 5.0 && (at.z + 4.0).abs() < 0.25), "inside the wall at {at}");
    }
    assert_eq!(world.get::<Agent>(entity).unwrap().status(), AgentStatus::Arrived);
    assert!(flat_distance(position(&world, "a"), to) < 0.6);
}

#[test]
fn reports_when_there_is_no_way() {
    let (mut physics, mut world) = world(
        r#"(name: "a", mesh: Cube, npc: ()),
           (mesh: Cube, position: (0.0, 0.0, -4.0), scale: (2.0, 2.0, 0.5)),
           (mesh: Cube, position: (0.0, 0.0, 4.0), scale: (2.0, 2.0, 0.5)),
           (mesh: Cube, position: (-1.0, 0.0, 0.0), scale: (0.5, 2.0, 8.5)),
           (mesh: Cube, position: (1.0, 0.0, 0.0), scale: (0.5, 2.0, 8.5))"#,
    );
    let entity = agent_at(&mut world, "a");
    world.get_mut::<Agent>(entity).unwrap().go_to(Vec3::new(10.0, 0.0, 0.0));
    run(&mut physics, &mut world, NOBODY, 8.0);
    assert_eq!(world.get::<Agent>(entity).unwrap().status(), AgentStatus::Failed);
}

#[test]
fn stop_stops_and_a_new_order_replaces_the_old() {
    let (mut physics, mut world) = world(r#"(name: "a", mesh: Cube, npc: ())"#);
    let entity = agent_at(&mut world, "a");
    world.get_mut::<Agent>(entity).unwrap().go_to(Vec3::new(20.0, 0.0, 0.0));
    run(&mut physics, &mut world, NOBODY, 1.0);
    world.get_mut::<Agent>(entity).unwrap().stop();
    run(&mut physics, &mut world, NOBODY, 0.5);
    let stopped = position(&world, "a");
    assert_eq!(world.get::<Agent>(entity).unwrap().status(), AgentStatus::Idle);
    run(&mut physics, &mut world, NOBODY, 1.0);
    assert!(position(&world, "a").distance(stopped) < 0.01);

    let back = Vec3::new(-3.0, 0.0, 0.0);
    world.get_mut::<Agent>(entity).unwrap().go_to(back);
    run(&mut physics, &mut world, NOBODY, 6.0);
    assert!(flat_distance(position(&world, "a"), back) < 0.6);
}

#[test]
fn patrols_its_points_in_a_loop() {
    let (mut physics, mut world) = world(
        r#"(name: "guard", mesh: Cube, npc: (walk_speed: 4.0, behavior: Patrol(points: [(5.0, 0.0, 0.0), (5.0, 0.0, 5.0), (0.0, 0.0, 5.0)])))"#,
    );
    let mut visited = [0u32; 3];
    let mut near = None;
    for _ in 0..(20.0 / DT) as u32 {
        physics.step(DT);
        world.update_npcs(&mut physics, NOBODY, DT);
        let at = position(&world, "guard");
        let points = [Vec3::new(5.0, 0.0, 0.0), Vec3::new(5.0, 0.0, 5.0), Vec3::new(0.0, 0.0, 5.0)];
        let here = points.iter().position(|&p| flat_distance(at, p) < 0.6);
        if let Some(point) = here
            && here != near
        {
            visited[point] += 1;
        }
        near = here;
    }
    assert!(visited.iter().all(|&n| n >= 2), "went round at least twice: {visited:?}");
}

#[test]
fn wandering_stays_around_home_and_repeats_with_the_scene() {
    let trail = || {
        let (mut physics, mut world) = world(
            r#"(name: "w", mesh: Cube, position: (3.0, 0.0, 2.0),
                npc: (walk_speed: 4.0, behavior: Wander(radius: 5.0, pause: (0.1, 0.3))))"#,
        );
        let mut trail = Vec::new();
        for _ in 0..12 {
            run(&mut physics, &mut world, NOBODY, 1.0);
            trail.push(position(&world, "w"));
        }
        trail
    };
    let first = trail();
    let home = Vec3::new(3.0, 0.0, 2.0);
    assert!(first.iter().all(|&p| flat_distance(p, home) < 5.7), "{first:?}");
    let farthest = first.iter().map(|&p| flat_distance(p, home)).fold(0.0, f32::max);
    assert!(farthest > 1.0, "it did go somewhere: {first:?}");
    assert_eq!(first, trail(), "same scene, same walk");
}

#[test]
fn watchers_turn_to_the_player_and_lose_interest() {
    let (mut physics, mut world) = world(
        r#"(name: "w", mesh: Cube, rotation: (0.0, 180.0, 0.0),
            npc: (notice: (radius: 6.0, reaction: Watch)))"#,
    );
    let entity = world.find("w").unwrap();
    let far = Vec3::new(0.0, 0.9, -20.0);
    assert!(run(&mut physics, &mut world, far, 1.0).is_empty());

    let near = Vec3::new(0.0, 0.9, -4.0);
    let events = run(&mut physics, &mut world, near, 2.0);
    assert_eq!(events, [NpcEvent::Noticed(entity)]);
    let forward = world.transform(entity).unwrap().rotation * Vec3::NEG_Z;
    assert!(forward.dot(Vec3::NEG_Z) > 0.95, "it looks at the player: {forward}");
    assert!(flat_distance(position(&world, "w"), Vec3::ZERO) < 0.01, "and stays put");

    // Between `radius` and 1.5 times it, nothing changes.
    assert!(run(&mut physics, &mut world, Vec3::new(0.0, 0.9, -8.0), 3.0).is_empty());
    let events = run(&mut physics, &mut world, far, 3.0);
    assert_eq!(events, [NpcEvent::Lost(entity)]);
    // Back to how the scene left it, facing +Z.
    run(&mut physics, &mut world, far, 2.0);
    let forward = world.transform(entity).unwrap().rotation * Vec3::NEG_Z;
    assert!(forward.dot(Vec3::Z) > 0.95, "{forward}");
}

#[test]
fn walls_hide_the_player() {
    let (mut physics, mut world) = world(
        r#"(name: "w", mesh: Cube, npc: (notice: (radius: 8.0))),
           (mesh: Cube, position: (0.0, 0.0, -3.0), scale: (6.0, 3.0, 0.5))"#,
    );
    assert!(run(&mut physics, &mut world, Vec3::new(0.0, 0.9, -5.0), 1.0).is_empty(), "behind the wall");
    let events = run(&mut physics, &mut world, Vec3::new(5.0, 0.9, 0.0), 1.0);
    assert_eq!(events.len(), 1, "in the open: {events:?}");
}

#[test]
fn followers_come_close_but_keep_their_distance_and_go_home() {
    let (mut physics, mut world) = world(
        r#"(name: "f", mesh: Cube, npc: (walk_speed: 3.0, notice: (radius: 8.0, reaction: Follow(distance: 2.0))))"#,
    );
    let player = Vec3::new(0.0, 0.9, -7.0);
    run(&mut physics, &mut world, player, 5.0);
    let distance = flat_distance(position(&world, "f"), player);
    assert!((1.7..2.8).contains(&distance), "{distance}");

    // Round a corner of open ground, it keeps up.
    let player = Vec3::new(5.0, 0.9, -8.0);
    run(&mut physics, &mut world, player, 4.0);
    let distance = flat_distance(position(&world, "f"), player);
    assert!((1.7..2.8).contains(&distance), "{distance}");

    run(&mut physics, &mut world, Vec3::new(40.0, 0.9, 40.0), 12.0);
    assert!(flat_distance(position(&world, "f"), Vec3::ZERO) < 1.1, "{}", position(&world, "f"));
}

#[test]
fn the_timid_run_away() {
    let (mut physics, mut world) = world(r#"(name: "t", mesh: Cube, npc: (notice: (radius: 6.0, reaction: Flee)))"#);
    let player = Vec3::new(0.0, 0.9, -4.0);
    run(&mut physics, &mut world, player, 3.0);
    let distance = flat_distance(position(&world, "t"), player);
    assert!(distance > 8.5, "{distance}");
}

#[test]
fn the_player_cannot_walk_through_them_and_they_do_not_walk_through_the_player() {
    let (mut physics, mut world) = world(r#"(name: "a", mesh: Cube, position: (0.0, 0.0, -4.0), npc: ())"#);
    physics.refresh();
    run(&mut physics, &mut world, NOBODY, 0.2);

    let mut camera = Camera::default();
    let mut input = Input::default();
    let mut player = ThirdPersonController::new(Vec3::new(0.0, 0.1, 0.0), 0.0);
    input.key_down(KeyCode::KeyW);
    let npc = position(&world, "a");
    let mut closest = f32::MAX;
    for _ in 0..(3.0 / DT) as u32 {
        physics.step(DT);
        player.update(&input, &mut physics, &mut camera, DT);
        world.update_npcs(&mut physics, player.body.center(), DT);
        input.end_frame();
        closest = closest.min(flat_distance(player.body.position, npc));
    }
    // Two capsules of 0.35: it stops against this one, then slides around it like a pillar.
    assert!(closest >= 0.69, "went through it: {closest}");
    assert!(player.body.position.z < -5.0, "got past: {}", player.body.position);
    assert!(flat_distance(position(&world, "a"), npc) < 0.01, "and doesn't shove it aside");
    input.key_up(KeyCode::KeyW);

    // Now it comes straight at the player, and goes around.
    let entity = agent_at(&mut world, "a");
    let at = player.body.position;
    let beyond = at + (at - npc).normalize() * 4.0;
    world.get_mut::<Agent>(entity).unwrap().go_to(beyond);
    let mut closest = f32::MAX;
    for _ in 0..(10.0 / DT) as u32 {
        physics.step(DT);
        world.update_npcs(&mut physics, player.body.center(), DT);
        closest = closest.min(flat_distance(position(&world, "a"), at));
    }
    assert!(closest >= 0.69, "keeps clear of the player: {closest}");
    assert_eq!(world.get::<Agent>(entity).unwrap().status(), AgentStatus::Arrived);
}

#[test]
fn two_of_them_meeting_head_on_get_past_each_other() {
    let (mut physics, mut world) = world(
        r#"(name: "a", mesh: Cube, position: (0.0, 0.0, -5.0), npc: ()),
           (name: "b", mesh: Cube, position: (0.0, 0.0, 5.0), npc: ())"#,
    );
    let (a, b) = (agent_at(&mut world, "a"), agent_at(&mut world, "b"));
    let (to_a, to_b) = (Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -5.0));
    world.get_mut::<Agent>(a).unwrap().go_to(to_a);
    world.get_mut::<Agent>(b).unwrap().go_to(to_b);
    run(&mut physics, &mut world, NOBODY, 9.0);
    assert_eq!(world.get::<Agent>(a).unwrap().status(), AgentStatus::Arrived);
    assert_eq!(world.get::<Agent>(b).unwrap().status(), AgentStatus::Arrived);
    assert!(flat_distance(position(&world, "a"), to_a) < 0.6);
}

#[test]
fn a_crowd_does_not_search_too_many_paths_at_once() {
    let objects: Vec<String> = (0..30)
        .map(|i| format!(r#"(name: "n{i}", mesh: Cube, position: ({}.0, 0.0, 0.0), npc: ())"#, i * 2 - 30))
        .collect();
    let (mut physics, mut world) = world(&objects.join(",\n"));
    let entities: Vec<Entity> = (0..30).map(|i| agent_at(&mut world, &format!("n{i}"))).collect();
    for &entity in &entities {
        let x = world.transform(entity).unwrap().position.x;
        world.get_mut::<Agent>(entity).unwrap().go_to(Vec3::new(x, 0.0, -6.0));
    }
    let planned =
        |world: &World| entities.iter().filter(|&&e| !world.get::<Agent>(e).unwrap().path().is_empty()).count();
    world.update_npcs(&mut physics, NOBODY, DT);
    assert_eq!(planned(&world), 2);
    run(&mut physics, &mut world, NOBODY, 1.0);
    assert_eq!(planned(&world), 30, "the rest got theirs in the next frames");
}

#[test]
fn far_ones_wait_until_the_player_comes_near() {
    let (mut physics, mut world) =
        world(r#"(name: "w", mesh: Cube, position: (0.0, 0.0, 0.0), npc: (behavior: Wander(pause: (0.0, 0.1))))"#);
    run(&mut physics, &mut world, Vec3::new(200.0, 1.0, 0.0), 3.0);
    let entity = world.find("w").unwrap();
    assert_eq!(world.get::<Agent>(entity).unwrap().status(), AgentStatus::Idle);
    assert_eq!(position(&world, "w").x, 0.0);
    run(&mut physics, &mut world, NOBODY, 3.0);
    assert!(flat_distance(position(&world, "w"), Vec3::ZERO) > 0.1);
}

#[test]
fn despawning_takes_the_capsule_too() {
    let (mut physics, mut world) = world(r#"(name: "a", mesh: Cube, npc: ())"#);
    let colliders = physics.collider_count();
    let entity = world.find("a").unwrap();
    physics.refresh();
    let from = Vec3::new(0.0, 1.0, 3.0);
    let hit = |physics: &Physics| -> Option<RayHit> { physics.raycast(from, Vec3::NEG_Z, 10.0) };
    assert!(hit(&physics).is_some());
    assert!(physics.raycast_scenery(from, Vec3::NEG_Z, 10.0).is_none(), "cameras see through them");

    world.despawn(entity, &mut physics);
    physics.refresh();
    assert_eq!(physics.collider_count(), colliders - 1);
    assert!(hit(&physics).is_none());
}

#[test]
fn they_can_be_used_like_any_object() {
    let (mut physics, world) = world(r#"(name: "a", mesh: Cube, npc: (), interact: (prompt: "falar"))"#);
    physics.refresh();
    let target = world.interaction_target(Vec3::new(0.0, 0.9, 1.5), Vec3::NEG_Z, &physics).unwrap();
    assert_eq!(target.prompt, "falar");
    assert_eq!(target.name.as_deref(), Some("a"));
}

#[test]
fn saved_games_put_them_back_and_they_carry_on() {
    let (mut physics, mut world) = world(r#"(name: "a", mesh: Cube, npc: (walk_speed: 3.0))"#);
    let entity = agent_at(&mut world, "a");
    let mut camera = Camera::default();
    world.get_mut::<Agent>(entity).unwrap().go_to(Vec3::new(20.0, 0.0, 0.0));
    run(&mut physics, &mut world, NOBODY, 2.0);
    let saved_at = position(&world, "a");
    assert!(saved_at.x > 3.0, "{saved_at}");
    let state = WorldState::capture(&world, &physics, &camera);

    run(&mut physics, &mut world, NOBODY, 2.0);
    assert!(position(&world, "a").x > saved_at.x + 3.0);

    state.restore(&mut world, &mut physics, &mut camera);
    assert_eq!(position(&world, "a"), saved_at);
    assert_eq!(world.get::<Agent>(entity).unwrap().status(), AgentStatus::Idle);
    run(&mut physics, &mut world, NOBODY, 0.2);
    assert!(position(&world, "a").distance(saved_at) < 0.05, "stays there, without snapping back");
    physics.refresh();
    let hit = physics.raycast(saved_at + Vec3::new(2.0, 1.0, 0.0), Vec3::NEG_X, 3.0);
    assert!(hit.is_some(), "its capsule came along");
}

#[test]
fn regions_bring_their_characters_and_take_them_away() {
    let dir: PathBuf = std::env::temp_dir().join(format!("grimora_npc_region_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("camp.ron"),
        r#"(objects: [(name: "keeper", mesh: Cube, position: (100.0, 0.0, 0.0), npc: ())])"#,
    )
    .unwrap();
    let source =
        r#"(streaming: (load_distance: 20.0), regions: [(path: "camp.ron", center: (100.0, 0.0, 0.0), radius: 5.0)])"#;
    let mut physics = Physics::new();
    let mut world = World::from_scene(Scene::build_in(&SceneDesc::parse(source).unwrap(), &dir), &mut physics);
    let empty = physics.collider_count();

    let camp = Vec3::new(100.0, 1.0, 0.0);
    world.load_around(camp, &mut physics);
    assert!(world.find("keeper").is_some());
    assert_eq!(physics.collider_count(), empty + 1);

    let deadline = Instant::now() + Duration::from_secs(10);
    while world.streaming_stats().unwrap().loaded_regions > 0 && Instant::now() < deadline {
        world.update_streaming(Vec3::ZERO, &mut physics);
    }
    assert!(world.find("keeper").is_none());
    assert_eq!(physics.collider_count(), empty);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn in_the_sandbox_the_pilgrim_walks_around_the_altar() {
    let mut physics = Physics::new();
    let scene = Scene::load(SANDBOX).unwrap();
    let mut world = World::from_scene(scene, &mut physics);
    let camera = Vec3::new(0.0, 5.0, 12.0);
    world.load_around(camera, &mut physics);
    let pilgrim = world.find("pilgrim").expect("the sandbox has a pilgrim");

    let start = position(&world, "pilgrim");
    let far = Vec3::new(0.0, 60.0, 90.0);
    let mut longest_stop = 0.0f32;
    let mut stopped = 0.0f32;
    let mut last = start;
    let mut walked = 0.0;
    for _ in 0..(40.0 / DT) as u32 {
        physics.step(DT);
        world.sync_bodies(&physics);
        world.update_npcs(&mut physics, far, DT);
        let at = position(&world, "pilgrim");
        let moved = flat_distance(at, last);
        walked += moved;
        stopped = if moved < 0.001 { stopped + DT } else { 0.0 };
        longest_stop = longest_stop.max(stopped);
        last = at;
        // The altar's cube is 2.4 wide and 1.5 deep, around the origin.
        assert!(!(at.x.abs() < 1.2 && at.z.abs() < 0.75), "walked into the altar at {at}");
        let ground = world.ground_height(at.x, at.z);
        assert!((at.y - ground).abs() < 0.3, "off the ground at {at}: {ground}");
    }
    assert!(walked > 30.0, "only {walked} m");
    assert!(longest_stop < 5.0, "stood still for {longest_stop} s");
    assert_ne!(world.get::<Agent>(pilgrim).unwrap().status(), AgentStatus::Failed);
    // The animation follows: it walks, on a skeleton with that clip.
    assert_eq!(world.get::<Animator>(pilgrim).unwrap().current(), Some("sword_walk"));
}
