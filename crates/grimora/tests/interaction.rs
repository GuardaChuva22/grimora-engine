//! Triggers and usable objects: going in and out, streaming and saved games.
//! Headless, like `character.rs`.

use std::time::{Duration, Instant};

use grimora::prelude::*;
use grimora::save::WorldState;
use grimora::scene::{Scene, SceneDesc};

const SANDBOX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/scenes/sandbox.ron");
const FAR: Vec3 = Vec3::new(100.0, 0.0, 100.0);

fn world(source: &str) -> (Physics, World) {
    let mut physics = Physics::new();
    let world = World::from_scene(Scene::build(&SceneDesc::parse(source).unwrap()), &mut physics);
    physics.refresh();
    (physics, world)
}

fn enter(name: &str) -> TriggerEvent {
    TriggerEvent::Enter(name.into())
}

fn exit(name: &str) -> TriggerEvent {
    TriggerEvent::Exit(name.into())
}

/// The name (or prompt, if nameless) of what a player at `from` looking along -Z would use.
fn target(world: &World, physics: &Physics, from: Vec3) -> Option<String> {
    let found = world.interaction_target(from, Vec3::NEG_Z, physics)?;
    Some(found.name.unwrap_or(found.prompt))
}

#[test]
fn triggers_report_going_in_and_out_once_per_change() {
    let (_, mut world) = world(
        r#"(triggers: [(name: "gate", shape: Ball(radius: 2.0)),
                       (name: "hall", shape: Box(half_extents: (2.0, 2.0, 2.0)), position: (10.0, 0.0, 0.0))])"#,
    );
    assert_eq!(world.ecs.len(), 2);
    assert!(world.find("gate").is_none(), "triggers aren't named objects");

    assert!(world.update_triggers(FAR).is_empty());
    assert_eq!(world.update_triggers(Vec3::ZERO), [enter("gate")]);
    assert!(world.update_triggers(Vec3::X).is_empty(), "still inside");
    assert!(world.trigger_state.inside.contains("gate"));
    assert_eq!(world.update_triggers(Vec3::new(10.0, 0.0, 0.0)), [exit("gate"), enter("hall")]);
    assert_eq!(world.update_triggers(FAR), [exit("hall")]);
    assert_eq!(world.update_triggers(Vec3::ZERO), [enter("gate")], "and it fires again");
}

#[test]
fn triggers_sharing_a_name_are_one_area() {
    let (_, mut world) = world(
        r#"(triggers: [(name: "road", shape: Ball(radius: 2.0)),
                       (name: "road", shape: Ball(radius: 2.0), position: (3.0, 0.0, 0.0))])"#,
    );
    assert_eq!(world.update_triggers(Vec3::ZERO), [enter("road")]);
    assert!(world.update_triggers(Vec3::new(1.5, 0.0, 0.0)).is_empty(), "in both");
    assert!(world.update_triggers(Vec3::new(4.0, 0.0, 0.0)).is_empty(), "only in the second now");
    assert_eq!(world.update_triggers(FAR), [exit("road")]);
}

#[test]
fn once_triggers_fire_a_single_time() {
    let (_, mut world) = world(r#"(triggers: [(name: "omen", shape: Ball(radius: 2.0), once: true)])"#);
    assert_eq!(world.update_triggers(Vec3::ZERO), [enter("omen")]);
    assert!(world.update_triggers(Vec3::ZERO).is_empty());
    assert!(world.update_triggers(FAR).is_empty(), "no exit either");
    assert!(world.update_triggers(Vec3::ZERO).is_empty());

    world.trigger_state.fired.remove("omen");
    assert_eq!(world.update_triggers(Vec3::ZERO), [enter("omen")], "the game can arm it again");
}

#[test]
fn triggers_on_entities_follow_them() {
    let (_, mut world) = world(r#"(objects: [(name: "ghost", mesh: Cube)])"#);
    let ghost = world.find("ghost").unwrap();
    world.ecs.insert_one(ghost, Trigger::new("chill", TriggerShape::Ball { radius: 2.0 })).unwrap();
    assert_eq!(world.update_triggers(Vec3::ZERO), [enter("chill")]);
    world.get_mut::<Transform>(ghost).unwrap().position = FAR;
    assert_eq!(world.update_triggers(Vec3::ZERO), [exit("chill")]);
    assert_eq!(world.update_triggers(FAR), [enter("chill")]);
}

#[test]
fn a_trigger_in_a_region_comes_and_goes_with_it_but_once_stays_spent() {
    let dir = std::env::temp_dir().join(format!("grimora_interaction_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("shrine.ron"),
        r#"(triggers: [(name: "omen", shape: Ball(radius: 5.0), position: (100.0, 0.0, 0.0), once: true),
                       (name: "shrine", shape: Ball(radius: 5.0), position: (100.0, 0.0, 0.0))],
            objects: [(name: "crystal", mesh: Cube, position: (100.0, 0.0, 0.0), interact: (prompt: "tocar"))])"#,
    )
    .unwrap();
    let main = r#"(streaming: (load_distance: 20.0),
                   regions: [(path: "shrine.ron", center: (100.0, 0.0, 0.0), radius: 5.0)])"#;
    let scene = Scene::build_in(&SceneDesc::parse(main).unwrap(), &dir);
    let mut physics = Physics::new();
    let mut world = World::from_scene(scene, &mut physics);
    let at_shrine = Vec3::new(100.0, 1.0, 0.0);

    world.load_around(Vec3::ZERO, &mut physics);
    assert!(world.update_triggers(at_shrine).is_empty(), "not loaded yet");

    world.load_around(at_shrine, &mut physics);
    assert_eq!(world.update_triggers(at_shrine), [enter("omen"), enter("shrine")]);
    assert_eq!(world.interaction_target(at_shrine, Vec3::NEG_Z, &physics).unwrap().prompt, "tocar");

    // Unloaded with the player still inside (the camera flew away): that's an exit.
    let deadline = Instant::now() + Duration::from_secs(10);
    while world.streaming_stats().unwrap().loaded_regions > 0 && Instant::now() < deadline {
        world.update_streaming(Vec3::ZERO, &mut physics);
    }
    assert_eq!(world.update_triggers(at_shrine), [exit("shrine")]);
    assert!(world.interaction_target(at_shrine, Vec3::NEG_Z, &physics).is_none());

    world.load_around(at_shrine, &mut physics);
    assert_eq!(world.update_triggers(at_shrine), [enter("shrine")], "omen already fired");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_nearest_usable_object_in_reach_is_the_target() {
    let (physics, world) = world(
        r#"(objects: [(name: "near", mesh: Cube, position: (0.0, 0.0, -3.0), interact: (prompt: "abrir")),
                      (name: "far", mesh: Cube, position: (0.0, 0.0, -7.0), interact: (prompt: "ler", radius: 6.0)),
                      (name: "plain", mesh: Cube, position: (0.0, 0.0, -1.0), collider: None)])"#,
    );
    assert_eq!(target(&world, &physics, Vec3::new(0.0, 1.0, 5.0)), None);
    assert_eq!(target(&world, &physics, Vec3::new(5.0, 1.0, -7.0)).as_deref(), Some("far"));
    let found = world.interaction_target(Vec3::new(1.5, 1.0, -3.0), Vec3::NEG_Z, &physics).unwrap();
    assert_eq!((found.name.as_deref(), found.prompt.as_str()), (Some("near"), "abrir"));
    assert_eq!(found.entity, world.find("near").unwrap());
}

#[test]
fn what_lies_ahead_wins_over_what_is_slightly_closer_behind() {
    let (physics, world) = world(
        r#"(objects: [(name: "ahead", mesh: Cube, position: (0.0, 0.0, -2.0), collider: None, interact: (radius: 3.0)),
                      (name: "behind", mesh: Cube, position: (0.0, 0.0, 1.5), collider: None, interact: (radius: 3.0))])"#,
    );
    assert_eq!(target(&world, &physics, Vec3::ZERO).as_deref(), Some("ahead"));
}

#[test]
fn walls_and_hiding_block_the_target() {
    let (mut physics, mut world) = world(
        r#"(objects: [(name: "chest", mesh: Cube, position: (0.0, 0.0, -2.0), interact: (radius: 4.0)),
                      (name: "wall", mesh: Cube, position: (0.0, 0.0, 0.0), scale: (6.0, 4.0, 0.2))])"#,
    );
    let (behind_wall, beside_chest) = (Vec3::new(0.0, 1.0, 1.0), Vec3::new(2.0, 1.0, -2.0));
    assert_eq!(target(&world, &physics, behind_wall), None);
    assert_eq!(target(&world, &physics, beside_chest).as_deref(), Some("chest"));

    let chest = world.find("chest").unwrap();
    world.get_mut::<Renderable>(chest).unwrap().visible = false;
    assert_eq!(target(&world, &physics, beside_chest), None);
    world.get_mut::<Renderable>(chest).unwrap().visible = true;

    let wall = world.find("wall").unwrap();
    world.despawn(wall, &mut physics);
    physics.refresh();
    assert_eq!(target(&world, &physics, behind_wall).as_deref(), Some("chest"));
}

#[test]
fn a_dynamic_body_does_not_block_itself() {
    let (mut physics, mut world) = world(
        r#"(objects: [(name: "floor", mesh: Plane(size: 20.0)),
                      (name: "barrel", mesh: Cube, position: (0.0, 0.01, -2.0), body: Dynamic(), interact: (radius: 4.0)),
                      (name: "post", mesh: Cube, position: (2.0, 0.0, 0.0))])"#,
    );
    for _ in 0..30 {
        physics.step(1.0 / 60.0);
        world.sync_bodies(&physics);
    }
    assert_eq!(target(&world, &physics, Vec3::new(0.0, 1.0, 0.0)).as_deref(), Some("barrel"));
    // Seen from the far side of the post, the ray stops at the post first.
    assert_eq!(target(&world, &physics, Vec3::new(3.0, 0.5, 1.0)), None);
}

#[test]
fn saved_games_keep_triggers_and_used_objects() {
    let (mut physics, mut world) = world(
        r#"(triggers: [(name: "omen", shape: Ball(radius: 2.0), once: true), (name: "gate", shape: Ball(radius: 2.0))],
            objects: [(name: "chest", mesh: Cube, position: (5.0, 0.0, 0.0), interact: (prompt: "abrir")),
                      (name: "door", mesh: Cube, position: (-5.0, 0.0, 0.0), interact: (prompt: "abrir"))])"#,
    );
    let mut camera = Camera::default();
    let chest = world.find("chest").unwrap();
    let door = world.find("door").unwrap();

    assert_eq!(world.update_triggers(Vec3::ZERO), [enter("gate"), enter("omen")]);
    world.ecs.remove_one::<Interactable>(chest).unwrap();
    let state = WorldState::capture(&world, &physics, &camera);
    assert_eq!(state.objects.len(), 1, "only the opened chest differs from the scene");
    let text = ron::to_string(&state).unwrap();
    assert_eq!(ron::from_str::<WorldState>(&text).unwrap(), state);

    // Play on: leave the gate, take the door's prompt away too.
    assert_eq!(world.update_triggers(FAR), [exit("gate")]);
    world.ecs.remove_one::<Interactable>(door).unwrap();

    state.restore(&mut world, &mut physics, &mut camera);
    assert!(world.get::<Interactable>(chest).is_none(), "still opened");
    assert_eq!(world.get::<Interactable>(door).unwrap().prompt, "abrir", "back as the scene had it");
    assert!(world.update_triggers(Vec3::ZERO).is_empty(), "already in the gate, and the omen is spent");
    assert_eq!(world.update_triggers(FAR), [exit("gate")]);
}

#[test]
fn saves_from_before_triggers_still_load() {
    let old = "(camera: (position: (1.0, 2.0, 3.0), yaw: 0.0, pitch: 0.0), objects: [])";
    assert_eq!(ron::from_str::<WorldState>(old).unwrap().triggers, Default::default());
}

#[test]
fn the_sandbox_has_a_circle_and_a_usable_heart() {
    let mut physics = Physics::new();
    let mut world = World::from_scene(Scene::load(SANDBOX).unwrap(), &mut physics);
    let spawn = world.player_spawn.unwrap().position + Vec3::Y * 0.9;
    assert!(world.update_triggers(spawn).is_empty(), "the player starts outside the circle");
    let beside_altar = Vec3::new(0.0, world.ground_height(0.0, 2.0) + 0.9, 2.0);
    assert_eq!(world.update_triggers(beside_altar), [enter("circle")]);
    assert_eq!(target(&world, &physics, spawn), None);
    assert_eq!(target(&world, &physics, beside_altar).as_deref(), Some("heart"));
}
