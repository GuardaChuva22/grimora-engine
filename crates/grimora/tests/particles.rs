//! Particle effects in the world: on objects, on their own, streamed, played
//! once. Headless, like `character.rs`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use grimora::core::ParticleInstance;
use grimora::prelude::*;
use grimora::scene::{Scene, SceneDesc, parse_effect};

const DT: f32 = 1.0 / 60.0;
const SANDBOX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/scenes/sandbox.ron");

/// A folder with `fire.ron`, deleted at the end of the test.
struct Assets(PathBuf);

impl Assets {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("grimora_particles_{name}_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("fire.ron"), "[(rate: 60.0, lifetime: (1.0, 1.0), max_distance: 50.0)]").unwrap();
        Self(dir)
    }

    fn world(&self, source: &str) -> (Physics, World) {
        let mut physics = Physics::new();
        let world = World::from_scene(Scene::build_in(&SceneDesc::parse(source).unwrap(), self.dir()), &mut physics);
        (physics, world)
    }

    fn dir(&self) -> &Path {
        &self.0
    }
}

impl Drop for Assets {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(world: &mut World, camera: Vec3, seconds: f32) {
    for _ in 0..(seconds / DT).round() as u32 {
        world.update_particles(DT, camera);
    }
}

#[test]
fn effects_on_objects_follow_them_and_free_ones_stand_alone() {
    let assets = Assets::new("placed");
    let (_, mut world) = assets.world(
        r#"(objects: [(name: "torch", mesh: Cube, position: (2.0, 0.0, 0.0), effect: (path: "fire.ron", position: (0.0, 1.0, 0.0)))],
            effects: [(path: "fire.ron", position: (-2.0, 0.0, 0.0))])"#,
    );
    assert_eq!(world.ecs.len(), 2);
    assert!(world.active_particles().is_empty(), "nothing before the first update");

    world.update_particles(DT, Vec3::ZERO);
    let particles = world.active_particles();
    assert!(particles.len() > 100, "both already burning: {}", particles.len());
    assert!(particles.iter().any(|p| p.position == [2.0, 1.0, 0.0]));
    assert!(particles.iter().any(|p| p.position == [-2.0, 0.0, 0.0]));

    let torch = world.find("torch").unwrap();
    world.get_mut::<Transform>(torch).unwrap().position = Vec3::new(2.0, 0.0, 9.0);
    run(&mut world, Vec3::ZERO, 0.1);
    assert!(world.active_particles().iter().any(|p| p.position == [2.0, 1.0, 9.0]), "new ones come from where it is");
}

#[test]
fn equal_effects_do_not_move_in_step_but_replay_the_same() {
    let assets = Assets::new("seeds");
    std::fs::write(assets.dir().join("spray.ron"), "[(rate: 30.0, speed: (1.0, 2.0))]").unwrap();
    let scene = r#"(effects: [(path: "spray.ron", position: (1.0, 0.0, 0.0)), (path: "spray.ron", position: (2.0, 0.0, 0.0))])"#;
    let once = || {
        let (_, mut world) = assets.world(scene);
        world.update_particles(DT, Vec3::ZERO);
        world.active_particles()
    };
    let particles = once();
    let half = particles.len() / 2;
    let offset = |p: &ParticleInstance, x: f32| Vec3::from(p.position) - Vec3::X * x;
    assert_ne!(offset(&particles[0], 1.0), offset(&particles[half], 2.0));
    assert_eq!(particles, once(), "same scene, same particles");
}

#[test]
fn hidden_objects_stop_emitting() {
    let assets = Assets::new("hidden");
    let (_, mut world) = assets.world(r#"(objects: [(name: "torch", mesh: Cube, effect: (path: "fire.ron"))])"#);
    run(&mut world, Vec3::ZERO, 0.5);
    assert!(!world.active_particles().is_empty());
    let torch = world.find("torch").unwrap();
    world.get_mut::<Renderable>(torch).unwrap().visible = false;
    run(&mut world, Vec3::ZERO, 1.1);
    assert!(world.active_particles().is_empty());
}

#[test]
fn far_effects_cost_nothing_until_the_camera_returns() {
    let assets = Assets::new("far");
    let (_, mut world) = assets.world(r#"(effects: [(path: "fire.ron")])"#);
    world.update_particles(DT, Vec3::new(51.0, 0.0, 0.0));
    assert!(world.active_particles().is_empty());
    world.update_particles(DT, Vec3::new(49.0, 0.0, 0.0));
    assert!(!world.active_particles().is_empty());
}

#[test]
fn one_shots_remove_themselves() {
    let (_, mut world) = {
        let assets = Assets::new("shots");
        assets.world("()")
    };
    let burst = parse_effect("[(rate: 0.0, burst: 10, lifetime: (0.2, 0.2))]").unwrap();
    let entity = world.play_effect(&burst, Vec3::new(1.0, 2.0, 3.0));
    world.update_particles(DT, Vec3::ZERO);
    assert_eq!(world.active_particles().len(), 10);
    assert!(world.active_particles().iter().all(|p| p.position == [1.0, 2.0, 3.0]));
    run(&mut world, Vec3::ZERO, 0.3);
    assert!(world.active_particles().is_empty());
    assert!(world.transform(entity).is_none(), "the entity is gone");
    assert_eq!(world.ecs.len(), 0);

    // Looping ones keep going until the game removes them.
    let flame = parse_effect("[(rate: 30.0)]").unwrap();
    let entity = world.play_effect(&flame, Vec3::ZERO);
    run(&mut world, Vec3::ZERO, 2.0);
    assert!(world.transform(entity).is_some());
}

#[test]
fn games_attach_effects_to_any_entity() {
    let assets = Assets::new("attach");
    let (_, mut world) = assets.world(r#"(objects: [(name: "goblin", mesh: Cube)])"#);
    let goblin = world.find("goblin").unwrap();
    let aura = parse_effect("[(rate: 10.0, lifetime: (1.0, 1.0))]").unwrap();
    world.ecs.insert_one(goblin, ParticleSystem::new(Arc::clone(&aura), Vec3::Y, 2.0, 3)).unwrap();
    world.update_particles(DT, Vec3::ZERO);
    assert!(world.active_particles().iter().all(|p| p.position == [0.0, 1.0, 0.0] && p.size == 0.4));
}

#[test]
fn effects_in_a_region_come_and_go_with_it() {
    let assets = Assets::new("region");
    std::fs::write(
        assets.dir().join("camp.ron"),
        r#"(effects: [(path: "fire.ron", position: (100.0, 0.0, 0.0))],
            objects: [(mesh: Cube, position: (100.0, 0.0, 5.0), effect: (path: "fire.ron"))])"#,
    )
    .unwrap();
    let (mut physics, mut world) = assets.world(
        r#"(streaming: (load_distance: 20.0), regions: [(path: "camp.ron", center: (100.0, 0.0, 0.0), radius: 5.0)])"#,
    );
    let camp = Vec3::new(100.0, 1.0, 0.0);

    world.load_around(Vec3::ZERO, &mut physics);
    world.update_particles(DT, Vec3::ZERO);
    assert!(world.active_particles().is_empty());

    world.load_around(camp, &mut physics);
    world.update_particles(DT, camp);
    assert!(world.active_particles().len() > 100, "{}", world.active_particles().len());

    let deadline = Instant::now() + Duration::from_secs(10);
    while world.streaming_stats().unwrap().loaded_regions > 0 && Instant::now() < deadline {
        world.update_streaming(Vec3::ZERO, &mut physics);
    }
    assert!(world.active_particles().is_empty(), "unloaded with the region");
}

#[test]
fn a_broken_effect_file_does_not_stop_the_scene() {
    let assets = Assets::new("broken");
    std::fs::write(assets.dir().join("bad.ron"), "[(rate: \"lots\")]").unwrap();
    let (_, world) =
        assets.world(r#"(objects: [(mesh: Cube, effect: (path: "bad.ron"))], effects: [(path: "gone.ron")])"#);
    assert_eq!(world.ecs.len(), 1);
    assert!(world.ecs.query::<&ParticleSystem>().iter().next().is_none());
}

#[test]
fn the_sandbox_torches_burn_and_saving_ignores_particles() {
    let mut physics = Physics::new();
    let mut world = World::from_scene(Scene::load(SANDBOX).unwrap(), &mut physics);
    let camera = Camera::new(Vec3::new(0.0, 5.0, 12.0), 0.0, 0.0);
    world.update_particles(DT, camera.position);
    assert!(world.active_particles().len() > 200, "{}", world.active_particles().len());

    let state = grimora::save::WorldState::capture(&world, &physics, &camera);
    // Only the crates settling down: burning changes nothing that should be saved.
    assert!(state.objects.iter().all(|o| o.velocity.is_some()), "{:?}", state.objects);
}
