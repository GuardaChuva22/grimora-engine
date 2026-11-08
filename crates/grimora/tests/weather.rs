//! The weather during the game: what falls, changing it and saving it. Headless.

use grimora::core::Weather;
use grimora::prelude::*;
use grimora::save::WorldState;
use grimora::scene::{Scene, SceneDesc};

fn world(source: &str) -> (Physics, World) {
    let scene = Scene::build(&SceneDesc::parse(source).unwrap());
    let mut physics = Physics::new();
    let world = World::from_scene(scene, &mut physics);
    (physics, world)
}

fn frame(world: &mut World) {
    world.update_day(0.1);
    world.update_particles(0.1, Vec3::ZERO);
}

#[test]
fn rain_starts_and_stops_with_the_change_of_weather() {
    let (_, mut world) = world("(day_cycle: (time: 12.0, day_length: 0.0))");
    assert_eq!(world.weather(), Some(Weather::Clear));
    frame(&mut world);
    assert!(world.active_particles().is_empty());

    world.change_weather(Weather::Rain, 5.0);
    assert_eq!(world.weather(), Some(Weather::Rain));
    frame(&mut world);
    let starting = world.active_particles().len();
    for _ in 0..60 {
        frame(&mut world);
    }
    let pouring = world.active_particles().len();
    assert!(pouring > starting.max(1000), "{starting} then {pouring}");
    assert!(world.environment.precipitation.wet > 0.0);

    world.change_weather(Weather::Clear, 1.0);
    for _ in 0..12 {
        frame(&mut world);
    }
    assert!(world.active_particles().is_empty());
    assert!(world.environment.precipitation.wet > 0.0, "the ground is still drying");
}

#[test]
fn a_scene_without_a_day_has_no_weather_to_change() {
    let (_, mut world) = world("()");
    world.change_weather(Weather::Snow, 1.0);
    world.set_weather(Weather::Snow);
    frame(&mut world);
    assert_eq!(world.weather(), None);
    assert!(world.active_particles().is_empty());
    assert!(!world.take_thunder());
}

#[test]
fn the_weather_is_saved() {
    let (mut physics, mut world) = world("(day_cycle: (time: 12.0, weather: Overcast))");
    let mut camera = Camera::new(Vec3::ZERO, 0.0, 0.0);
    world.change_weather(Weather::Snow, 5.0);
    let saved = WorldState::capture(&world, &physics, &camera);
    assert_eq!(saved.weather, Some(Weather::Snow));

    world.set_weather(Weather::Clear);
    saved.restore(&mut world, &mut physics, &mut camera);
    assert_eq!(world.weather(), Some(Weather::Snow));
    assert_eq!(world.environment.precipitation.snow_cover, 1.0, "no change under way");

    // Saves from before there was weather keep what the scene has.
    let old = WorldState { weather: None, ..saved };
    world.set_weather(Weather::Rain);
    old.restore(&mut world, &mut physics, &mut camera);
    assert_eq!(world.weather(), Some(Weather::Rain));
}
