//! Entities: spawning, game components, despawning with colliders, and lights
//! that move with their entity. Headless, like `character.rs`.

use grimora::prelude::*;
use grimora::scene::{Scene, SceneDesc};

fn world(source: &str) -> (Physics, World) {
    let scene = Scene::build(&SceneDesc::parse(source).unwrap());
    let mut physics = Physics::new();
    let world = World::from_scene(scene, &mut physics);
    (physics, world)
}

fn hits(physics: &Physics, x: f32, z: f32) -> bool {
    physics.raycast(Vec3::new(x, 10.0, z), Vec3::NEG_Y, 20.0).is_some()
}

#[test]
fn scene_objects_are_entities_with_engine_components() {
    let (_, world) = world(
        "(objects: [(name: \"altar\", mesh: Cube, position: (2.0, 0.0, -3.0), color: (0.5, 0.4, 0.3)),
                    (name: \"crate\", mesh: Cube, position: (0.0, 1.0, 0.0), body: Dynamic(density: 300.0))])",
    );
    assert_eq!(world.ecs.len(), 2);
    let altar = world.find("altar").unwrap();
    assert_eq!(world.transform(altar).unwrap().position, Vec3::new(2.0, 0.0, -3.0));
    assert_eq!(world.get::<Renderable>(altar).unwrap().color, Vec3::new(0.5, 0.4, 0.3));
    assert!(world.get::<Collider>(altar).is_some() && world.get::<Body>(altar).is_none());
    let crate_entity = world.find("crate").unwrap();
    assert!(world.get::<Body>(crate_entity).is_some() && world.get::<Collider>(crate_entity).is_none());
}

#[test]
fn games_attach_their_own_components() {
    #[derive(Debug, PartialEq)]
    struct Health(u32);

    let (_, mut world) = world("(objects: [(name: \"goblin\", mesh: Cube), (name: \"rock\", mesh: Cube)])");
    let goblin = world.find("goblin").unwrap();
    world.ecs.insert_one(goblin, Health(10)).unwrap();
    let spawned = world.spawn(MeshId(0), Transform::from_position(Vec3::X));
    world.ecs.insert_one(spawned, Health(3)).unwrap();

    for (_, health) in world.ecs.query_mut::<(&Transform, &mut Health)>() {
        health.0 -= 1;
    }
    assert_eq!(*world.get::<Health>(goblin).unwrap(), Health(9));
    assert_eq!(*world.get::<Health>(spawned).unwrap(), Health(2));
    assert!(world.get::<Health>(world.find("rock").unwrap()).is_none());
}

#[test]
fn despawning_removes_the_collider() {
    let (mut physics, mut world) = world(
        "(objects: [(name: \"wall\", mesh: Cube, position: (5.0, 0.0, 0.0)),
                    (name: \"crate\", mesh: Cube, position: (-5.0, 0.0, 0.0), body: Dynamic(density: 300.0))])",
    );
    assert!(hits(&physics, 5.0, 0.0) && hits(&physics, -5.0, 0.0));
    for name in ["wall", "crate"] {
        let entity = world.find(name).unwrap();
        assert!(world.despawn(entity, &mut physics));
        assert!(world.transform(entity).is_none(), "{name}'s id no longer resolves");
    }
    physics.refresh();
    assert!(!hits(&physics, 5.0, 0.0) && !hits(&physics, -5.0, 0.0));
    assert_eq!(world.ecs.len(), 0);
}

#[test]
fn lights_move_with_their_entity() {
    let (_, mut world) = world(
        "(objects: [(name: \"heart\", mesh: Cube, position: (0.0, 1.0, 0.0),
                     light: (position: (1.0, 0.0, 0.0), radius: 5.0))],
          lights: [(position: (9.0, 9.0, 9.0))])",
    );
    let lights = world.active_lights();
    assert_eq!(lights.len(), 2, "the free light and the heart's");
    assert!(lights.iter().any(|l| l.position.distance(Vec3::new(1.0, 1.0, 0.0)) < 1e-5));

    let heart = world.find("heart").unwrap();
    let transform = world.get_mut::<Transform>(heart).unwrap();
    transform.position = Vec3::new(0.0, 3.0, 0.0);
    transform.rotation = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
    let moved = world.active_lights();
    assert!(moved.iter().any(|l| l.position.distance(Vec3::new(0.0, 3.0, -1.0)) < 1e-5), "{moved:?}");

    world.ecs.remove_one::<Light>(heart).unwrap();
    assert_eq!(world.active_lights().len(), 1);
}

#[test]
fn night_lights_come_on_as_the_daylight_goes() {
    let objects = "objects: [
        (mesh: Cube, light: (intensity: 4.0, radius: 6.0, night_only: true)),
        (mesh: Cube, position: (3.0, 0.0, 0.0), light: (intensity: 2.0, radius: 6.0)),
    ]";
    let (_, mut world) = world(&format!("(day_cycle: (time: 12.0, tilt: 20.0), {objects})"));
    let intensities = |world: &World| world.active_lights().iter().map(|l| l.intensity).collect::<Vec<_>>();

    assert_eq!(intensities(&world), [2.0], "off by day, and not sent to the renderer");
    world.set_time_of_day(0.0);
    let mut night = intensities(&world);
    night.sort_by(f32::total_cmp);
    assert_eq!(night, [2.0, 4.0]);

    world.set_time_of_day(17.7);
    let dusk = intensities(&world);
    assert!(dusk.iter().any(|&i| i > 0.0 && i < 4.0 && i != 2.0), "fading in at dusk: {dusk:?}");

    let (_, world) = self::world(&format!("({objects})"));
    assert_eq!(world.active_lights().len(), 2, "with no day cycle it is always lit");
}
