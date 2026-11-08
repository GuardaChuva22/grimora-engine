//! Schools of fish in a real sea, without a window: `World::update_fauna` as the engine calls it.

use std::path::Path;
use std::time::Instant;

use grimora::BirdDoing;
use grimora::prelude::*;
use grimora::scene::{Scene, SceneDesc};

/// A bowl of a sea: 40 m of flat floor at the bottom, rising to a beach all around.
fn scene(count: u32, day: &str) -> (World, Physics) {
    let text = format!(
        r#"SceneDesc(
            terrain: (size: 240.0, resolution: 60, height: 0.0, seed: 1, edge_rise: 30.0, flat_radius: 60.0),
            water: [(position: (0.0, 20.0, 0.0), sea: (swell: 0.5, length: 40.0))],
            day_cycle: {day},
            fauna: [(species: "../fauna/sardinha.ron", position: (0.0, 0.0, 0.0), radius: 30.0, count: {count}, seed: 7)],
        )"#
    );
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenes");
    let scene = Scene::build_in(&SceneDesc::parse(&text).unwrap(), &dir);
    let mut physics = Physics::new();
    let world = World::from_scene(scene, &mut physics);
    (world, physics)
}

fn noon() -> &'static str {
    "(time: 12.0, day_length: 0.0, weather: Clear)"
}

fn run(world: &mut World, physics: &Physics, focus: Vec3, seconds: f32) {
    let dt = 1.0 / 60.0;
    for _ in 0..(seconds / dt) as u32 {
        world.update_fauna(physics, focus, dt);
    }
}

fn center(world: &World) -> Vec3 {
    world.fauna.positions().sum::<Vec3>() / world.fauna.population() as f32
}

/// How far the middle half of the fish are from the middle of the school.
fn spread(world: &World) -> f32 {
    let middle = center(world);
    let mut distances: Vec<f32> = world.fauna.positions().map(|p| p.distance(middle)).collect();
    distances.sort_by(f32::total_cmp);
    distances[distances.len() / 2]
}

#[test]
fn every_fish_stays_in_the_water_for_minutes() {
    let (mut world, _physics) = scene(200, noon());
    assert_eq!(world.fauna.population(), 200);
    let level = world.waters.calm_at(0.0, 0.0).unwrap().0;
    for _ in 0..12 {
        run(&mut world, &_physics, Vec3::new(0.0, 25.0, 0.0), 10.0);
        let terrain = world.terrain.as_ref().unwrap();
        for position in world.fauna.positions() {
            assert!(position.y < level, "a fish left the water at {position}");
            let floor = terrain.height_at(position.x, position.z);
            assert!(position.y > floor - 0.01, "a fish went into the ground at {position}, floor {floor}");
            assert!(level - floor > 0.5, "a fish beached itself at {position}");
        }
    }
}

#[test]
fn a_school_starts_at_its_depth_and_all_going_the_same_way() {
    let (mut world, _physics) = scene(200, noon());
    let level = world.waters.calm_at(0.0, 0.0).unwrap().0;
    let depth = level - center(&world).y;
    assert!((depth - 5.0).abs() < 1.0, "it starts {depth} m deep, and the file says 5");

    let focus = Vec3::new(0.0, 25.0, 0.0);
    let before: Vec<Vec3> = world.fauna.positions().collect();
    run(&mut world, &_physics, focus, 0.2);
    let ways = world.fauna.positions().zip(&before).map(|(now, then)| (now - *then).normalize());
    let order = ways.sum::<Vec3>().length() / before.len() as f32;
    assert!(order > 0.7, "only {order:.2} of the school is going the same way");
}

#[test]
fn a_school_holds_together_without_piling_up() {
    let (mut world, _physics) = scene(200, noon());
    run(&mut world, &_physics, Vec3::new(0.0, 25.0, 0.0), 30.0);
    let spread = spread(&world);
    assert!(spread < 8.0, "the school came apart: half of it is over {spread} m from the middle");
    assert!(spread > 0.5, "the school collapsed into a point: {spread} m");
}

/// How much of the school is going the same way, averaged over a few minutes: it falls when
/// the school turns, and that's as it should be.
#[test]
fn fish_swim_together_and_come_up_to_feed() {
    let (mut world, physics) = scene(200, noon());
    let focus = Vec3::new(0.0, 25.0, 0.0);
    let level = world.waters.calm_at(0.0, 0.0).unwrap().0;
    let (mut total, mut shallowest, mut deepest) = (0.0, f32::MAX, 0.0_f32);
    for _ in 0..60 {
        let before: Vec<Vec3> = world.fauna.positions().collect();
        run(&mut world, &physics, focus, 0.5);
        let ways = world.fauna.positions().zip(&before).map(|(now, then)| (now - *then).normalize_or_zero());
        total += ways.sum::<Vec3>().length() / before.len() as f32;
        run(&mut world, &physics, focus, 2.5);
        let depth = level - center(&world).y;
        shallowest = shallowest.min(depth);
        deepest = deepest.max(depth);
    }
    let order = total / 60.0;
    assert!(order > 0.7, "only {order:.2} of the school is going the same way");
    assert!(shallowest < 1.8, "it never came up: {shallowest} m at the shallowest");
    assert!(deepest > 4.0, "it never went back down: {deepest} m at the deepest");
}

#[test]
fn a_threat_scares_the_fish_and_the_fear_runs_through_the_school() {
    let (mut world, _physics) = scene(200, noon());
    let focus = Vec3::new(0.0, 25.0, 0.0);
    run(&mut world, &_physics, focus, 10.0);
    assert_eq!(world.fauna.fears().filter(|&fear| fear > 0.0).count(), 0, "nothing frightened them yet");

    // Near the edge of the school, and close enough for only a few to notice. Not the very
    // edge: that fish may be off with a few others, too far to pass anything on.
    let middle = center(&world);
    let mut by_distance: Vec<Vec3> = world.fauna.positions().collect();
    by_distance.sort_by(|a, b| a.distance(middle).total_cmp(&b.distance(middle)));
    let edge = by_distance[by_distance.len() * 4 / 5];
    let threat = edge + (edge - middle).normalize() * 0.5;
    let noticed = world.fauna.positions().filter(|p| p.distance(threat) < 2.0).count();
    assert!(noticed < 100, "too many are close to see the fear spread: {noticed}");
    let start = center(&world);
    for _ in 0..30 {
        world.scare_fauna(threat, 2.0);
        run(&mut world, &_physics, focus, 1.0 / 60.0);
    }
    let scared = world.fauna.fears().filter(|&fear| fear > 0.0).count();
    assert!(scared > noticed * 2, "fear didn't spread: {scared} are scared and {noticed} saw it");

    // Something bigger, that the whole school sees: it leaves.
    let before = center(&world).distance(threat);
    for _ in 0..240 {
        world.scare_fauna(threat, 8.0);
        run(&mut world, &_physics, focus, 1.0 / 60.0);
    }
    let after = center(&world).distance(threat);
    assert!(after > before + 3.0, "the school didn't get away: {before} m to {after} m, from {start}");

    // And it passes.
    run(&mut world, &_physics, focus, 40.0);
    assert_eq!(world.fauna.fears().filter(|&fear| fear > 0.0).count(), 0, "they never calmed down");
}

#[test]
fn a_school_rises_at_night_and_loosens() {
    let level_depth = |world: &World| {
        let level = world.waters.calm_at(0.0, 0.0).unwrap().0;
        level - center(world).y
    };
    let (mut day, _physics) = scene(200, noon());
    let (mut night, _physics_night) = scene(200, "(time: 0.0, day_length: 0.0, weather: Clear)");
    let focus = Vec3::new(0.0, 25.0, 0.0);
    run(&mut day, &_physics, focus, 30.0);
    run(&mut night, &_physics_night, focus, 30.0);
    let (by_day, by_night) = (level_depth(&day), level_depth(&night));
    assert!(by_night < by_day - 1.5, "at {by_day} m by day and {by_night} m by night");
    assert!(spread(&night) > spread(&day) * 1.2, "{} by night, {} by day", spread(&night), spread(&day));
}

#[test]
fn a_school_nobody_can_see_still_travels_and_comes_back_whole() {
    let (mut world, _physics) = scene(100, noon());
    let far_away = Vec3::new(5000.0, 25.0, 5000.0);
    let before: Vec<Vec3> = world.fauna.positions().collect();
    run(&mut world, &_physics, far_away, 60.0);
    let frozen: Vec<Vec3> = world.fauna.positions().collect();
    assert_eq!(before, frozen, "nobody should be spending time on fish no one can see");

    run(&mut world, &_physics, Vec3::new(0.0, 25.0, 0.0), 0.5);
    let moved = world.fauna.positions().zip(&before).map(|(a, b)| a.distance(*b)).sum::<f32>() / 100.0;
    assert!(moved > 1.0, "it should have travelled in the meantime: {moved} m");
    run(&mut world, &_physics, Vec3::new(0.0, 25.0, 0.0), 10.0);
    assert!(spread(&world) < 8.0, "it didn't come back whole: {}", spread(&world));
}

#[test]
fn the_same_seed_makes_the_same_school() {
    let (mut one, _physics) = scene(50, noon());
    let (mut two, _physics_two) = scene(50, noon());
    let focus = Vec3::new(0.0, 25.0, 0.0);
    run(&mut one, &_physics, focus, 5.0);
    run(&mut two, &_physics_two, focus, 5.0);
    assert!(one.fauna.positions().eq(two.fauna.positions()));
}

/// `cargo test --release -p grimora --test fauna cost -- --nocapture` says what it costs.
#[test]
fn cost_of_a_big_school() {
    let (mut world, physics) = scene(500, noon());
    let focus = Vec3::new(0.0, 25.0, 0.0);
    run(&mut world, &physics, focus, 5.0);
    let frames = 600;
    let start = Instant::now();
    for _ in 0..frames {
        world.update_fauna(&physics, focus, 1.0 / 60.0);
    }
    let per_frame = start.elapsed().as_secs_f32() * 1000.0 / frames as f32;
    println!("[fauna] 500 fish: {per_frame:.3} ms per frame");
    if !cfg!(debug_assertions) {
        assert!(per_frame < 1.0, "the school takes {per_frame} ms per frame");
    }
}

/// The same bowl, with land around it and gulls living on the school.
fn gull_scene(gulls: u32, day: &str, seed: u64) -> (World, Physics) {
    let text = format!(
        r#"SceneDesc(
            terrain: (size: 240.0, resolution: 60, height: 0.0, seed: 1, edge_rise: 34.0, flat_radius: 50.0),
            water: [(position: (0.0, 20.0, 0.0), sea: (swell: 0.5, length: 40.0))],
            day_cycle: {day},
            fauna: [
                (species: "../fauna/sardinha.ron", position: (0.0, 0.0, 0.0), radius: 18.0, count: 260, seed: 7),
                (species: "../fauna/gaivota.ron", position: (0.0, 0.0, 0.0), radius: 118.0, count: {gulls}, seed: {seed}),
            ],
        )"#
    );
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenes");
    let scene = Scene::build_in(&SceneDesc::parse(&text).unwrap(), &dir);
    let mut physics = Physics::new();
    let world = World::from_scene(scene, &mut physics);
    (world, physics)
}

fn live(world: &mut World, physics: &Physics, seconds: f32) {
    let focus = Vec3::new(0.0, 30.0, 0.0);
    let dt = 1.0 / 30.0;
    for _ in 0..(seconds / dt) as u32 {
        world.update_fauna(physics, focus, dt);
        // What the engine does with them, in `after_update`.
        world.take_splashes();
    }
}

fn doing(world: &World, what: BirdDoing) -> usize {
    world.fauna.bird_doings().filter(|&doing| doing == what).count()
}

#[test]
fn gulls_get_hungry_hunt_and_what_they_catch_is_gone() {
    let (mut world, physics) = gull_scene(12, noon(), 3);
    let fish = world.fauna.fish_count();
    let (mut most_diving, mut circled) = (0, false);
    for _ in 0..1200 {
        live(&mut world, &physics, 0.5);
        most_diving = most_diving.max(doing(&world, BirdDoing::Diving));
        circled |= doing(&world, BirdDoing::Circling) > 0;
    }
    let meals = world.fauna.bird_meals() as usize;
    let dives = world.fauna.bird_dives() as usize;
    println!("[gulls] 10 minutes: {dives} dives, {meals} fish, most diving at once {most_diving}");
    assert!(circled, "no gull ever went after the school");
    assert!(meals > 0, "{dives} dives and nothing caught");
    assert!(meals < dives, "every one of {dives} dives caught something");
    assert_eq!(world.fauna.fish_count(), fish - meals, "what's eaten is gone from the school");
    let hunger = world.fauna.bird_hungers().sum::<f32>() / 12.0;
    assert!(hunger < 0.9, "they should be managing to eat: {hunger}");
}

#[test]
fn gulls_stay_in_the_air_or_on_something_and_never_inside_it() {
    let (mut world, physics) = gull_scene(12, noon(), 4);
    for _ in 0..480 {
        live(&mut world, &physics, 0.5);
        let terrain = world.terrain.as_ref().unwrap();
        for position in world.fauna.bird_positions() {
            let floor = terrain.height_at(position.x, position.z);
            assert!(position.y > floor - 0.05, "a gull went into the ground at {position}, floor {floor}");
            assert!(position.y < floor.max(20.0) + 60.0, "a gull flew off at {position}");
            assert!(position.x.abs() < 200.0 && position.z.abs() < 200.0, "a gull left at {position}");
        }
    }
}

#[test]
fn gulls_sleep_at_night_and_in_a_storm() {
    for day in ["(time: 1.0, day_length: 0.0, weather: Clear)", "(time: 12.0, day_length: 0.0, weather: Storm)"] {
        let (mut world, physics) = gull_scene(12, day, 5);
        live(&mut world, &physics, 90.0);
        let standing = doing(&world, BirdDoing::Standing);
        assert_eq!(standing, 12, "{day}: only {standing} of 12 have come down");
        let level = world.waters.calm_at(0.0, 0.0).unwrap().0;
        assert!(world.fauna.bird_positions().all(|p| p.y > level), "{day}: one is standing on the water");
    }
}

#[test]
fn someone_coming_close_puts_gulls_in_the_air() {
    let (mut world, physics) = gull_scene(4, "(time: 1.0, day_length: 0.0, weather: Clear)", 6);
    live(&mut world, &physics, 90.0);
    assert_eq!(doing(&world, BirdDoing::Standing), 4);
    let near = world.fauna.bird_positions().next().unwrap();
    world.scare_fauna(near + Vec3::X * 2.0, 2.0);
    live(&mut world, &physics, 1.0 / 30.0);
    assert!(doing(&world, BirdDoing::Standing) < 4, "nothing took off");
}

#[test]
fn the_same_seed_makes_the_same_gulls() {
    let (mut one, physics_one) = gull_scene(6, noon(), 8);
    let (mut two, physics_two) = gull_scene(6, noon(), 8);
    live(&mut one, &physics_one, 60.0);
    live(&mut two, &physics_two, 60.0);
    assert!(one.fauna.bird_positions().eq(two.fauna.bird_positions()));
    assert_eq!(one.fauna.fish_count(), two.fauna.fish_count());
}

/// `cargo test --release -p grimora --test fauna cost_of_gulls -- --nocapture`
#[test]
fn cost_of_gulls_and_their_fish() {
    let (mut world, physics) = gull_scene(14, noon(), 9);
    live(&mut world, &physics, 20.0);
    let frames = 600;
    let start = Instant::now();
    for _ in 0..frames {
        world.update_fauna(&physics, Vec3::new(0.0, 30.0, 0.0), 1.0 / 60.0);
        world.take_splashes();
    }
    let per_frame = start.elapsed().as_secs_f32() * 1000.0 / frames as f32;
    println!("[fauna] 14 gulls and 260 fish: {per_frame:.3} ms per frame");
    if !cfg!(debug_assertions) {
        assert!(per_frame < 2.0, "the fauna takes {per_frame} ms per frame");
    }
}

/// The same bowl of a sea, with its beach: `x` of where the land comes out of the water on +X.
fn beach_x() -> f32 {
    let (world, _physics) = gull_scene(0, noon(), 1);
    let level = world.waters.calm_at(0.0, 0.0).unwrap().0;
    let terrain = world.terrain.as_ref().unwrap();
    (0..120).map(|x| x as f32).find(|x| terrain.height_at(*x, 0.0) > level).unwrap()
}

/// Crabs, rays and jellyfish on the beach at `+X`.
fn coast_scene(crabs: u32, rays: u32, jellies: u32, day: &str) -> (World, Physics) {
    let beach = beach_x();
    let text = format!(
        r#"SceneDesc(
            terrain: (size: 240.0, resolution: 60, height: 0.0, seed: 1, edge_rise: 34.0, flat_radius: 50.0),
            water: [(position: (0.0, 20.0, 0.0), sea: (swell: 0.5, length: 40.0))],
            day_cycle: {day},
            fauna: [
                (species: "../fauna/caranguejo.ron", position: ({beach}, 0.0, 0.0), radius: 14.0, count: {crabs}, seed: 4),
                (species: "../fauna/arraia.ron", position: ({ray_x}, 0.0, 0.0), radius: 8.0, count: {rays}, seed: 5),
                (species: "../fauna/agua_viva.ron", position: ({jelly_x}, 0.0, 0.0), radius: 10.0, count: {jellies}, seed: 6),
            ],
        )"#,
        ray_x = beach - 7.0,
        jelly_x = beach - 16.0,
    );
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenes");
    let scene = Scene::build_in(&SceneDesc::parse(&text).unwrap(), &dir);
    let mut physics = Physics::new();
    let world = World::from_scene(scene, &mut physics);
    (world, physics)
}

fn midnight() -> &'static str {
    "(time: 0.0, day_length: 0.0, weather: Clear)"
}

/// A few seconds of the coast with someone watching from nearby.
fn watch(world: &mut World, physics: &Physics, seconds: f32) {
    let focus = Vec3::new(beach_x(), 21.0, 0.0);
    let dt = 1.0 / 30.0;
    for _ in 0..(seconds / dt) as u32 {
        world.update_fauna(physics, focus, dt);
        world.take_splashes();
    }
}

fn crabs_doing(world: &World, doing: grimora::CrabDoing) -> usize {
    world.fauna.crab_doings().filter(|what| *what == doing).count()
}

#[test]
fn crabs_stay_in_by_day_and_come_out_at_night() {
    let (mut world, physics) = coast_scene(30, 0, 0, noon());
    watch(&mut world, &physics, 40.0);
    let hidden = crabs_doing(&world, grimora::CrabDoing::InItsHole);
    assert!(hidden >= 15, "by day only {hidden} of 30 stay in, and it should be most of them");

    let (mut world, physics) = coast_scene(30, 0, 0, midnight());
    watch(&mut world, &physics, 40.0);
    let hidden = crabs_doing(&world, grimora::CrabDoing::InItsHole);
    assert!(hidden <= 8, "at night {hidden} of 30 are still in");
}

#[test]
fn a_crab_walks_with_its_feet_on_the_ground() {
    let (mut world, physics) = coast_scene(20, 0, 0, midnight());
    let focus = Vec3::new(beach_x(), 21.0, 0.0);
    let dt = 1.0 / 60.0;
    let (mut off, mut stretch) = (0.0_f32, 0.0_f32);
    for _ in 0..(40.0 / dt) as u32 {
        world.update_fauna(&physics, focus, dt);
        world.take_splashes();
        let (a, b) = world.fauna.crab_feet(world.terrain.as_ref(), &world.waters);
        off = off.max(a);
        stretch = stretch.max(b);
    }
    println!("[crab] worst foot {off:.3} m off the ground, legs at {stretch:.2} of their length");
    assert!(off < 0.002, "a planted foot is {off} m off the ground");
    assert!(stretch < 1.3, "a leg is asked to reach {stretch} of its length");
}

#[test]
fn a_frightened_crab_runs_to_its_hole() {
    let (mut world, physics) = coast_scene(30, 0, 0, midnight());
    watch(&mut world, &physics, 40.0);
    let out = 30 - crabs_doing(&world, grimora::CrabDoing::InItsHole);
    assert!(out >= 20);
    let middle = Vec3::new(beach_x(), 20.0, 0.0);
    for _ in 0..(14.0 * 30.0) as u32 {
        world.scare_fauna(middle, 40.0);
        world.update_fauna(&physics, middle, 1.0 / 30.0);
        world.take_splashes();
    }
    let hidden = crabs_doing(&world, grimora::CrabDoing::InItsHole);
    assert!(hidden >= 26, "only {hidden} of 30 made it in");
}

#[test]
fn crabs_square_up_over_their_sand() {
    let (mut world, physics) = coast_scene(30, 0, 0, midnight());
    watch(&mut world, &physics, 120.0);
    println!("[crab] {} standoffs and {} meals in 2 minutes", world.fauna.crab_standoffs(), world.fauna.crab_meals());
    assert!(world.fauna.crab_standoffs() > 0, "nobody faced anybody down");
    assert!(world.fauna.crab_meals() > 0, "nobody ate");
}

#[test]
fn rays_lie_buried_by_day_and_hunt_crabs_at_night() {
    use grimora::RayDoing;
    let (mut world, physics) = coast_scene(0, 5, 0, noon());
    watch(&mut world, &physics, 30.0);
    let buried = world.fauna.ray_doings().filter(|what| *what == RayDoing::Buried).count();
    assert_eq!(buried, 5, "they should all be down at noon");

    let (mut world, physics) = coast_scene(40, 5, 0, midnight());
    let before = world.fauna.crab_count();
    let level = world.waters.calm_at(0.0, 0.0).unwrap().0;
    let mut most_out = 0;
    for _ in 0..30 {
        watch(&mut world, &physics, 20.0);
        let out = world.fauna.ray_doings().filter(|what| *what != RayDoing::Buried).count();
        most_out = most_out.max(out);
        for position in world.fauna.ray_positions() {
            assert!(position.y < level, "a ray left the water at {position}");
        }
    }
    println!(
        "[ray] at most {most_out} of 5 out; {} crabs eaten in 10 minutes, {} left",
        world.fauna.ray_meals(),
        world.fauna.crab_count()
    );
    assert!(most_out >= 3, "only {most_out} of them came out");
    assert!(world.fauna.ray_meals() > 0, "no ray caught anything in 10 minutes");
    assert_eq!(before - world.fauna.crab_count(), world.fauna.ray_meals() as usize);
    assert!(world.fauna.crab_count() * 2 >= before, "they ate {} of {before} crabs", world.fauna.ray_meals());
}

#[test]
fn jellyfish_keep_off_the_beach_until_a_storm() {
    let (mut world, physics) = coast_scene(0, 0, 25, noon());
    let level = world.waters.calm_at(0.0, 0.0).unwrap().0;
    let start: Vec<Vec3> = world.fauna.jelly_positions().collect();
    watch(&mut world, &physics, 300.0);
    assert_eq!(world.fauna.jelly_count(), 25, "{} died on a calm day", 25 - world.fauna.jelly_count());
    let moved = world.fauna.jelly_positions().zip(&start).map(|(a, b)| a.distance(*b)).sum::<f32>() / 25.0;
    println!("[jelly] carried {moved:.1} m on average in 5 minutes");
    assert!(moved > 3.0, "they barely moved: {moved} m");
    for position in world.fauna.jelly_positions() {
        assert!(position.y < level, "a jellyfish came out of the water at {position}");
    }
    let depth = level - world.fauna.jelly_positions().map(|p| p.y).sum::<f32>() / 25.0;
    println!("[jelly] {depth:.1} m deep on average, and the file says 5 by day");
    assert!((3.0..7.5).contains(&depth), "they're {depth} m deep");

    // The wind goes towards +X: right at the beach.
    let (mut world, physics) = coast_scene(0, 0, 25, "(time: 12.0, day_length: 0.0, weather: Storm)");
    watch(&mut world, &physics, 120.0);
    println!("[jelly] after a storm: {} alive, {} on the sand", world.fauna.jelly_count(), world.fauna.carcass_count());
    assert!(world.fauna.carcass_count() >= 15, "the storm only stranded {}", world.fauna.carcass_count());
}

#[test]
fn what_a_storm_leaves_rots_away() {
    let (mut world, physics) = coast_scene(20, 0, 25, "(time: 0.0, day_length: 0.0, weather: Storm)");
    watch(&mut world, &physics, 90.0);
    assert!(world.fauna.carcass_count() > 0);
    world.set_weather(grimora::core::Weather::Clear);
    watch(&mut world, &physics, 300.0);
    assert_eq!(world.fauna.carcass_count(), 0, "what's on the sand should be gone in 4 minutes");
}

#[test]
fn the_coast_costs_little() {
    let (mut world, physics) = coast_scene(26, 5, 28, midnight());
    watch(&mut world, &physics, 40.0);
    let focus = Vec3::new(beach_x(), 21.0, 0.0);
    let start = Instant::now();
    let frames = 600;
    for _ in 0..frames {
        world.update_fauna(&physics, focus, 1.0 / 60.0);
        world.take_splashes();
    }
    let ms = start.elapsed().as_secs_f32() * 1000.0 / frames as f32;
    println!("[coast] {ms:.3} ms per frame with everyone near");
    if cfg!(not(debug_assertions)) {
        assert!(ms < 1.0, "the coast takes {ms} ms per frame");
    }
}

fn number_of(world: &World, species: &str) -> usize {
    world.fauna.census().iter().filter(|group| group.species == species).map(|group| group.number).sum()
}

/// How many are left of a species, in a scene with a single group of it.
fn set_number(world: &mut World, species: &str, number: usize) {
    let mut census = world.fauna.census();
    census.iter_mut().filter(|group| group.species == species).for_each(|group| group.number = number);
    world.restore_fauna(&census);
}

#[test]
fn a_group_grows_until_there_is_no_room_and_stops() {
    let (mut world, _physics) = coast_scene(20, 0, 0, midnight());
    set_number(&mut world, "caranguejo", 4);
    assert_eq!(number_of(&world, "caranguejo"), 4);
    let nobody = Vec3::new(0.0, 500.0, 0.0);
    world.pass_fauna_days(4.0, nobody);
    let soon = number_of(&world, "caranguejo");
    world.pass_fauna_days(40.0, nobody);
    let later = number_of(&world, "caranguejo");
    println!("[ecology] from 4 crabs: {soon} after 4 days, {later} after 44");
    assert!(soon > 4, "nobody was born in 4 days");
    assert!(later > 16, "it only got back to {later}");
    assert!(later <= 30, "there's room for 30, and there are {later}");
}

#[test]
fn what_vanished_from_a_place_slowly_comes_back_from_elsewhere() {
    let (mut world, _physics) = coast_scene(20, 5, 20, midnight());
    let nobody = Vec3::new(0.0, 500.0, 0.0);
    for species in ["caranguejo", "arraia", "agua_viva"] {
        set_number(&mut world, species, 0);
        assert_eq!(number_of(&world, species), 0);
    }
    world.pass_fauna_days(0.25, nobody);
    assert_eq!(number_of(&world, "arraia"), 0, "rays came back in a quarter of a day");
    world.pass_fauna_days(30.0, nobody);
    for species in ["caranguejo", "arraia", "agua_viva"] {
        assert!(number_of(&world, species) > 0, "no {species} came back in a month");
    }
    println!("[ecology] a month after they vanished: {:?}", world.fauna.census());
}

#[test]
fn nothing_comes_or_goes_in_front_of_someone() {
    // Rays and jellyfish, with someone standing right where they live.
    let (mut world, physics) = coast_scene(0, 5, 12, midnight());
    let here = Vec3::new(beach_x() - 10.0, 18.0, 0.0);
    world.update_fauna(&physics, here, 1.0 / 30.0);
    let before = world.fauna.census();
    world.pass_fauna_days(30.0, here);
    assert_eq!(world.fauna.census(), before);
    // And they do, as soon as nobody is there.
    world.pass_fauna_days(30.0, Vec3::new(0.0, 500.0, 0.0));
    assert_ne!(world.fauna.census(), before);
}

#[test]
fn a_save_keeps_how_many_there_are() {
    use grimora::save::WorldState;
    let (mut world, mut physics) = coast_scene(20, 5, 20, midnight());
    set_number(&mut world, "caranguejo", 7);
    set_number(&mut world, "agua_viva", 0);
    let state = WorldState::capture(&world, &physics, &Camera::default());

    let (mut fresh, mut fresh_physics) = coast_scene(20, 5, 20, midnight());
    state.restore(&mut fresh, &mut fresh_physics, &mut Camera::default());
    assert_eq!(number_of(&fresh, "caranguejo"), 7);
    assert_eq!(number_of(&fresh, "agua_viva"), 0);

    // A save from before there were numbers leaves them as they are.
    let mut old = state.clone();
    old.fauna.clear();
    old.restore(&mut world, &mut physics, &mut Camera::default());
    assert_eq!(number_of(&world, "caranguejo"), 7);

    // More than there are now: they get made.
    let mut more = state.clone();
    more.fauna.iter_mut().for_each(|group| group.number = 12);
    more.restore(&mut world, &mut physics, &mut Camera::default());
    assert_eq!(number_of(&world, "agua_viva"), 12);
    for _ in 0..120 {
        world.update_fauna(&physics, Vec3::new(0.0, 25.0, 0.0), 1.0 / 30.0);
        world.take_splashes();
    }
}

/// A day of the sea lasts as long as in `sea.ron`.
fn day_passing() -> &'static str {
    "(time: 8.0, day_length: 480.0, weather: Clear)"
}

#[test]
#[cfg_attr(debug_assertions, ignore = "many game days: run it with --release")]
fn gulls_with_nothing_to_eat_go_away() {
    let text = format!(
        r#"SceneDesc(
            terrain: (size: 240.0, resolution: 60, height: 0.0, seed: 1, edge_rise: 34.0, flat_radius: 50.0),
            water: [(position: (0.0, 20.0, 0.0), sea: (swell: 0.5, length: 40.0))],
            day_cycle: {day},
            fauna: [
                (species: "../fauna/sardinha.ron", position: (0.0, 0.0, 0.0), radius: 18.0, count: 40, seed: 7),
                (species: "../fauna/gaivota.ron", position: (0.0, 0.0, 0.0), radius: 80.0, count: 12, seed: 3),
            ],
        )"#,
        day = day_passing(),
    );
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenes");
    let scene = Scene::build_in(&SceneDesc::parse(&text).unwrap(), &dir);
    let mut physics = Physics::new();
    let mut world = World::from_scene(scene, &mut physics);
    let focus = Vec3::new(0.0, 30.0, 0.0);
    let dt = 1.0 / 15.0;
    let frames = (480.0 * 6.0 / dt) as u32;
    let mut least = 12;
    for frame in 0..frames {
        // Nothing to catch, and the sardines keep arriving: take them away again.
        if frame % 900 == 0 {
            set_number(&mut world, "sardinha", 0);
        }
        world.update_fauna(&physics, focus, dt);
        world.take_splashes();
        least = least.min(number_of(&world, "gaivota"));
    }
    println!("[ecology] 12 gulls with no fish: {least} at the lowest in 6 days");
    assert!(least <= 6, "{least} of them are still hanging around with nothing to eat");
}

#[test]
#[cfg_attr(debug_assertions, ignore = "many game days: run it with --release")]
fn the_coast_goes_on_for_days_with_everyone_eating_everyone() {
    let beach = beach_x();
    let text = format!(
        r#"SceneDesc(
            terrain: (size: 240.0, resolution: 60, height: 0.0, seed: 1, edge_rise: 34.0, flat_radius: 50.0),
            water: [(position: (0.0, 20.0, 0.0), sea: (swell: 0.5, length: 40.0))],
            day_cycle: {day},
            fauna: [
                (species: "../fauna/sardinha.ron", position: ({fish}, 0.0, 0.0), radius: 18.0, count: 300, seed: 7),
                (species: "../fauna/gaivota.ron", position: ({beach}, 0.0, 0.0), radius: 80.0, count: 12, seed: 3),
                (species: "../fauna/caranguejo.ron", position: ({beach}, 0.0, 0.0), radius: 14.0, count: 26, seed: 4),
                (species: "../fauna/arraia.ron", position: ({ray}, 0.0, 0.0), radius: 8.0, count: 5, seed: 5),
                (species: "../fauna/agua_viva.ron", position: ({fish}, 0.0, 0.0), radius: 12.0, count: 25, seed: 6),
            ],
        )"#,
        day = day_passing(),
        fish = beach - 30.0,
        ray = beach - 7.0,
    );
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenes");
    let scene = Scene::build_in(&SceneDesc::parse(&text).unwrap(), &dir);
    let mut physics = Physics::new();
    let mut world = World::from_scene(scene, &mut physics);
    let start = world.fauna.census();
    // Near enough to be worked out, and too far to get in the way of anyone coming or going.
    let focus = Vec3::new(beach - 95.0, 25.0, 0.0);
    let dt = 1.0 / 15.0;
    for day in 1..=12 {
        for _ in 0..(480.0 / dt) as u32 {
            world.update_fauna(&physics, focus, dt);
            world.take_splashes();
        }
        let numbers: Vec<String> = world.fauna.census().iter().map(|g| format!("{} {}", g.species, g.number)).collect();
        println!(
            "[ecology] day {day}: {} ({} fish caught, {} crabs caught)",
            numbers.join(", "),
            world.fauna.bird_meals(),
            world.fauna.ray_meals()
        );
    }
    for (now, before) in world.fauna.census().iter().zip(&start) {
        let share = now.number as f32 / before.number as f32;
        assert!((0.35..=1.55).contains(&share), "{} went from {} to {}", now.species, before.number, now.number);
    }
}

/// Someone standing somewhere, as the game says it every frame.
fn stand(world: &mut World, physics: &Physics, feet: Vec3, seconds: f32, scares: bool) -> Vec<FaunaEvent> {
    let focus = Vec3::new(beach_x(), 21.0, 0.0);
    let dt = 1.0 / 30.0;
    let mut events = Vec::new();
    for _ in 0..(seconds / dt) as u32 {
        world.touch_fauna(feet, 1.8);
        if scares {
            world.scare_fauna(feet + Vec3::Y, 5.0);
        }
        world.update_fauna(physics, focus, dt);
        world.take_splashes();
        events.extend(world.fauna.take_events());
    }
    events
}

fn hurts(events: &[FaunaEvent]) -> Vec<(f32, bool)> {
    events
        .iter()
        .filter_map(|event| match event {
            FaunaEvent::Hurt { damage, staggers, .. } => Some((*damage, *staggers)),
            _ => None,
        })
        .collect()
}

/// Whether one of these noises came from a file of this name, and every one of them exists.
fn made_noise(world: &mut World, name: &str) -> bool {
    let sounds = world.fauna.take_sounds();
    for sound in &sounds {
        assert!(sound.file.exists(), "{} isn't there", sound.file.display());
    }
    sounds.iter().any(|sound| sound.file.ends_with(name))
}

#[test]
fn a_blow_kills_what_is_out_and_leaves_a_body() {
    use grimora::CrabDoing;
    let (mut world, physics) = coast_scene(30, 0, 0, midnight());
    watch(&mut world, &physics, 40.0);
    let out: Vec<Vec3> = world
        .fauna
        .crab_positions()
        .zip(world.fauna.crab_doings())
        .filter(|(_, doing)| *doing == CrabDoing::Out)
        .map(|(position, _)| position)
        .collect();
    assert!(!out.is_empty(), "nobody came out");
    let (before, bodies) = (world.fauna.crab_count(), world.fauna.carcass_count());
    let killed = world.strike_fauna(out[0], 0.05);
    assert!(killed >= 1, "the blow missed");
    assert_eq!(world.fauna.crab_count(), before - killed);
    assert_eq!(world.fauna.carcass_count(), bodies + killed);
    assert!(made_noise(&mut world, "crunch.wav"));
    let events = world.fauna.take_events();
    assert!(events.iter().any(|event| matches!(event, FaunaEvent::Killed { species, .. } if species == "caranguejo")));

    // It stays where it fell, on the ground, and the others want nothing to do with that spot.
    let lying = world.fauna.carcass_positions().last().unwrap();
    watch(&mut world, &physics, 2.0);
    let now = world.fauna.carcass_positions().last().unwrap();
    assert!(now.distance(lying) < 0.3, "the body went from {lying} to {now}");
}

#[test]
fn nothing_gets_at_what_is_under_the_sand() {
    use grimora::{CrabDoing, RayDoing};
    let (mut world, physics) = coast_scene(30, 5, 0, noon());
    watch(&mut world, &physics, 10.0);
    let hidden: Vec<Vec3> = world
        .fauna
        .crab_positions()
        .zip(world.fauna.crab_doings())
        .filter(|(_, doing)| *doing == CrabDoing::InItsHole)
        .map(|(position, _)| position)
        .collect();
    assert!(!hidden.is_empty());
    let before = world.fauna.census();
    for at in hidden.iter().copied().chain(world.fauna.ray_positions().collect::<Vec<_>>()) {
        world.strike_fauna_by(&mut |other, _| other == at);
    }
    let buried = world.fauna.ray_doings().filter(|what| *what == RayDoing::Buried).count();
    assert_eq!(buried, 5);
    assert_eq!(world.fauna.census(), before);
}

#[test]
fn a_jellyfish_stings_whoever_touches_it_and_gives_them_time() {
    let (mut world, physics) = coast_scene(0, 0, 20, midnight());
    watch(&mut world, &physics, 3.0);
    let jelly = world.fauna.jelly_positions().next().unwrap();
    let feet = jelly - Vec3::Y * 0.9;
    let events = stand(&mut world, &physics, feet, 3.0, false);
    let stings = hurts(&events);
    println!("[harm] in 3 s next to a jellyfish: {stings:?}");
    assert!((2..=3).contains(&stings.len()), "{} stings in 3 seconds", stings.len());
    assert!(stings.iter().all(|(damage, staggers)| *damage > 0.0 && !*staggers));
    assert!(made_noise(&mut world, "jelly_sting.wav"));

    let far = stand(&mut world, &physics, jelly + Vec3::new(0.0, 40.0, 0.0), 2.0, false);
    assert!(hurts(&far).is_empty(), "stung from far away");
}

#[test]
fn a_ray_under_the_sand_stays_there_until_it_is_stepped_on() {
    use grimora::RayDoing;
    let (mut world, physics) = coast_scene(0, 5, 0, noon());
    watch(&mut world, &physics, 5.0);
    let rays: Vec<Vec3> = world.fauna.ray_positions().collect();
    let on_it = rays[0];
    // Near it, and not next to any other one.
    let near = (0..16)
        .map(|k| on_it + Quat::from_rotation_y(k as f32 * 0.4) * Vec3::X * 2.5)
        .find(|at| rays.iter().all(|ray| ray.distance(*at) > 1.5))
        .unwrap();
    let events = stand(&mut world, &physics, near, 3.0, true);
    assert!(hurts(&events).is_empty(), "stung from 2.5 m away");
    let buried = world.fauna.ray_doings().filter(|what| *what == RayDoing::Buried).count();
    assert_eq!(buried, 5, "it should have stayed where it was, with someone walking by");

    let events = stand(&mut world, &physics, on_it, 0.2, true);
    let stings = hurts(&events);
    println!("[harm] stepping on a ray: {stings:?}");
    assert_eq!(stings.len(), 1);
    assert!(stings[0].1, "that should stop anyone in their tracks");
    assert!(world.fauna.ray_doings().any(|what| what == RayDoing::Bolting));
    assert!(made_noise(&mut world, "spine_sting.wav"));
    // It's gone: nobody gets stung twice.
    let events = stand(&mut world, &physics, on_it, 1.0, true);
    assert!(hurts(&events).is_empty());
}

/// Whichever crab that's out is nearest, as seen by someone following it around.
fn nearest_crab_out(world: &World, to: Vec3) -> Option<Vec3> {
    world
        .fauna
        .crab_positions()
        .zip(world.fauna.crab_doings())
        .filter(|(_, doing)| *doing != grimora::CrabDoing::InItsHole)
        .map(|(position, _)| position)
        .min_by(|a, b| a.distance(to).total_cmp(&b.distance(to)))
}

#[test]
fn a_crab_only_pinches_when_it_has_nowhere_to_go() {
    let (mut world, physics) = coast_scene(40, 0, 0, midnight());
    watch(&mut world, &physics, 40.0);

    // Standing among them with nothing to be afraid of.
    let start = nearest_crab_out(&world, Vec3::new(beach_x(), 0.0, 0.0)).unwrap();
    let events = stand(&mut world, &physics, start, 0.5, false);
    assert!(hurts(&events).is_empty(), "pinched by a crab with no reason to");

    // After one, and another, with hands out.
    let mut chased = start;
    let mut stings = Vec::new();
    for _ in 0..600 {
        let Some(crab) = nearest_crab_out(&world, chased) else {
            watch(&mut world, &physics, 5.0);
            continue;
        };
        chased = crab;
        stings.extend(hurts(&stand(&mut world, &physics, crab - Vec3::Y * 0.1, 0.1, true)));
        if !stings.is_empty() {
            break;
        }
    }
    println!("[harm] chasing crabs: {stings:?}");
    assert!(!stings.is_empty(), "a minute of chasing crabs and none fought back");
    assert!(stings.iter().all(|(_, staggers)| !*staggers));
    assert!(made_noise(&mut world, "pinch.wav"));
}

#[test]
fn gulls_call_by_day_and_are_quiet_at_night() {
    let (mut world, physics) = gull_scene(8, noon(), 3);
    let mut calls = 0;
    for _ in 0..60 {
        live(&mut world, &physics, 1.0);
        let sounds = world.fauna.take_sounds();
        assert!(sounds.iter().all(|sound| sound.file.exists()));
        calls += sounds.iter().filter(|sound| sound.file.to_string_lossy().contains("gull_")).count();
    }
    println!("[sound] 8 gulls in a minute of daylight: {calls} calls");
    assert!((5..=120).contains(&calls), "{calls} calls in a minute");

    let (mut world, physics) = gull_scene(8, midnight(), 3);
    live(&mut world, &physics, 60.0);
    assert!(world.fauna.take_sounds().iter().all(|sound| !sound.file.to_string_lossy().contains("gull_")));
}

#[test]
fn what_is_struck_out_of_the_air_falls_and_is_gone_in_the_end() {
    let (mut world, physics) = gull_scene(8, noon(), 3);
    live(&mut world, &physics, 25.0);
    let highest = world.fauna.bird_positions().max_by(|a, b| a.y.total_cmp(&b.y)).unwrap();
    assert_eq!(world.strike_fauna(highest, 0.05), 1);
    assert_eq!(world.fauna.bird_positions().count(), 7);
    let start = world.fauna.carcass_positions().next().unwrap();
    live(&mut world, &physics, 12.0);
    let end = world.fauna.carcass_positions().next().unwrap();
    println!("[body] a gull fell from {start} to {end}");
    let (level, _) = world.waters.calm_at(end.x, end.z).unwrap_or((f32::NEG_INFINITY, Vec3::ZERO));
    let floor = world.terrain.as_ref().unwrap().height_at(end.x, end.z);
    assert!(end.y < start.y - 1.0, "it stayed in the air");
    assert!((end.y - level.max(floor)).abs() < 0.2, "it's at {} over {}", end.y, level.max(floor));

    live(&mut world, &physics, 125.0);
    assert_eq!(world.fauna.carcass_count(), 0, "it never rotted");
}

#[test]
fn a_sword_kills_what_runs_around_the_feet_of_whoever_swings_it() {
    let wanderer =
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/models/wanderer/wanderer.gltf").replace('\\', "/");
    let beach = beach_x();
    let text = format!(
        r#"SceneDesc(
            terrain: (size: 240.0, resolution: 60, height: 0.0, seed: 1, edge_rise: 34.0, flat_radius: 50.0),
            water: [(position: (0.0, 20.0, 0.0), sea: (swell: 0.5, length: 40.0))],
            day_cycle: {day},
            objects: [
                (name: "swordsman", mesh: Model(path: "{wanderer}"), position: ({beach}, 0.0, 0.0), animation: "idle", collider: None,
                 combat: (health: 30.0, team: 1, weapon: (damage: 5.0, cooldown: 0.0, clip: "sword_1", combo: ["sword_2", "sword_3"], blade: ()))),
            ],
            fauna: [(species: "../fauna/caranguejo.ron", position: ({beach}, 0.0, 0.0), radius: 3.0, count: 60, seed: 4)],
        )"#,
        day = midnight(),
    );
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenes");
    let scene = Scene::build_in(&SceneDesc::parse(&text).unwrap(), &dir);
    let mut physics = Physics::new();
    let mut world = World::from_scene(scene, &mut physics);
    let swordsman = world.ecs.query::<(Entity, &Fighter)>().iter().next().map(|(entity, _)| entity).unwrap();
    let before = world.fauna.crab_count();
    let focus = Vec3::new(beach, 21.0, 0.0);
    let dt = 1.0 / 60.0;
    let mut killed = 0;
    for _ in 0..(180.0 / dt) as u32 {
        physics.step(dt);
        world.get_mut::<Fighter>(swordsman).unwrap().attack();
        world.update_combat(&mut physics, dt);
        world.update_animations(dt);
        world.update_fauna(&physics, focus, dt);
        world.take_splashes();
        world.fauna.take_sounds();
        killed += world.fauna.take_events().iter().filter(|event| matches!(event, FaunaEvent::Killed { .. })).count();
        if killed >= 3 {
            break;
        }
    }
    println!("[harm] the sword got {killed} of {before} crabs");
    assert!(killed >= 1, "three minutes of swinging among crabs and it never touched one");
    assert_eq!(world.fauna.crab_count(), before - killed);
}
