//! What a step lands on: `World::ground_under`, for the footstep sounds.

use std::path::Path;

use grimora::prelude::*;
use grimora::scene::{Ground, Scene, SceneDesc};

fn scene(text: &str) -> (World, Physics) {
    let scene = Scene::build(&SceneDesc::parse(text).unwrap());
    let mut physics = Physics::new();
    let world = World::from_scene(scene, &mut physics);
    physics.refresh();
    (world, physics)
}

const TERRAIN: &str = r#"terrain: Some((size: 60.0, resolution: 30, height: 0.0, edge_rise: 0.0, flat_radius: 30.0,
    grounds: (grass: Sand, dirt: Sand, rock: Stone),
    paths: [(points: [(0.0, -20.0), (0.0, 20.0)], width: 3.0, ground: Stone)]))"#;

fn on_ground(world: &World, x: f32, z: f32) -> Vec3 {
    Vec3::new(x, world.ground_height(x, z), z)
}

#[test]
fn the_terrain_its_paths_and_what_stands_on_it_each_sound_their_own() {
    let objects = r#"objects: [
        (mesh: Cube, position: (10.0, 0.0, 0.0), scale: (4.0, 0.5, 4.0), ground: Some(Wood)),
        (mesh: Cube, position: (-10.0, 0.0, 0.0), scale: (4.0, 0.5, 4.0), texture: Some((path: "../textures/placa_de_ferro.png"))),
        (mesh: Cube, position: (0.0, 0.0, 22.0), scale: (4.0, 0.5, 4.0)),
    ]"#;
    let (world, physics) = scene(&format!("SceneDesc({TERRAIN}, {objects})"));
    assert_eq!(world.ground_under(&physics, on_ground(&world, 8.0, 12.0)), Ground::Sand);
    assert_eq!(world.ground_under(&physics, on_ground(&world, 0.5, 5.0)), Ground::Stone, "on the path");
    let deck = on_ground(&world, 10.0, 0.0) + Vec3::Y * 0.5;
    assert_eq!(world.ground_under(&physics, deck), Ground::Wood);
    let plate = on_ground(&world, -10.0, 0.0) + Vec3::Y * 0.5;
    assert_eq!(world.ground_under(&physics, plate), Ground::Metal, "guessed from the picture's name");
    let block = on_ground(&world, 0.0, 22.0) + Vec3::Y * 0.5;
    assert_eq!(world.ground_under(&physics, block), Ground::Stone, "nothing to go by");
}

#[test]
fn rain_turns_grass_and_dirt_to_mud_and_nothing_else() {
    let (mut world, physics) =
        scene("SceneDesc(terrain: Some((size: 40.0, resolution: 20, height: 0.0, edge_rise: 0.0)))");
    let field = on_ground(&world, 5.0, 5.0);
    assert_eq!(world.ground_under(&physics, field), Ground::Grass);
    world.environment.precipitation.wet = 0.9;
    assert_eq!(world.ground_under(&physics, field), Ground::Mud);
    assert_eq!(Ground::Sand.soaked(0.9), Ground::Sand);
    assert_eq!(Ground::Stone.soaked(0.9), Ground::Stone);
}

#[test]
fn every_ground_has_its_steps_in_boots_and_barefoot() {
    let steps = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/sounds/steps");
    for feet in ["bota", "descalco"] {
        for ground in Ground::ALL {
            for take in 1..=10 {
                let file = steps.join(feet).join(ground.folder()).join(format!("passo_{take:02}.wav"));
                assert!(file.exists(), "missing {}", file.display());
            }
        }
    }
}
