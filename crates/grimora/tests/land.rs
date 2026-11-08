use std::path::PathBuf;

use glam::{DVec3, Vec3};
use grimora::{Land, World};
use grimora_core::{Origin, Transform};
use grimora_physics::Physics;

fn island() -> Land {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/world/island.ron");
    Land::load(&path).expect("island loads")
}

/// The ground under a local point, in world meters, by the colliders.
fn ground(world: &World, physics: &Physics, at: Vec3) -> f64 {
    let hit = physics.raycast(at + Vec3::Y * 2000.0, Vec3::NEG_Y, 4000.0).expect("ground under");
    world.origin().to_world(hit.point).y
}

#[test]
fn tiles_and_colliders_stay_put_when_the_origin_moves() {
    let land = island();
    let start = land.start(None);
    let mut world = World::default();
    let mut physics = Physics::new();
    world.set_origin(Origin::new(DVec3::new(start.x, 0.0, start.z)));
    world.set_land(land);
    let eye = Vec3::new(0.0, start.y as f32 + 2.0, 0.0);
    let first = world.update_land(eye, eye, &mut physics);
    assert!(first.tiles > 50 && first.colliders == 25, "{first:?}");
    let probe = Vec3::new(37.0, 0.0, -51.0);
    let before = ground(&world, &physics, probe);
    let expected = world.ground().unwrap().height_at(world.origin().to_world(probe));
    assert!((before - expected).abs() < 0.6, "collider {before}, relief {expected}");
    let tile_world: Vec<DVec3> = tile_positions(&world);

    let delta = Vec3::new(2048.0, 0.0, -4096.0);
    world.shift_origin(delta, &mut physics);
    physics.refresh();
    let again = world.update_land(eye - delta, eye - delta, &mut physics);
    assert_eq!(again.built, 0, "nothing rebuilt: the keys are in the world");
    let moved = tile_positions(&world);
    assert_eq!(moved.len(), tile_world.len());
    for (a, b) in moved.iter().zip(&tile_world) {
        assert!(a.distance(*b) < 0.01, "{a} was {b}");
    }
    let after = ground(&world, &physics, probe - delta);
    assert!((after - before).abs() < 1e-3, "{before} then {after}");
}

#[test]
fn walking_away_swaps_tiles_and_reuses_meshes() {
    let land = island();
    let start = land.start(None);
    let mut world = World::default();
    let mut physics = Physics::new();
    world.set_origin(Origin::new(DVec3::new(start.x, 0.0, start.z)));
    world.set_land(land);
    let mut eye = Vec3::new(0.0, start.y as f32 + 2.0, 0.0);
    world.update_land(eye, eye, &mut physics);
    let meshes = (0..).take_while(|&i| world.mesh(grimora::MeshId(i)).is_some()).count();
    for _ in 0..20 {
        eye.x += 150.0;
        world.update_land(eye, eye, &mut physics);
    }
    let after = (0..).take_while(|&i| world.mesh(grimora::MeshId(i)).is_some()).count();
    assert!(after < meshes + meshes / 4, "{meshes} meshes, then {after}");
    let colliders = world.ground().unwrap().stats().colliders;
    // 5 × 5 around the player, and those one tile further stay until it goes on.
    assert!((25..=49).contains(&colliders), "{colliders} colliders");
}

fn tile_positions(world: &World) -> Vec<DVec3> {
    let mut out: Vec<DVec3> = world
        .ecs
        .query::<(&Transform, &grimora::Name)>()
        .iter()
        .filter(|(_, n)| n.0 == "terrain")
        .map(|(t, _)| world.origin().to_world(t.position))
        .collect();
    out.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.z.total_cmp(&b.z)));
    out
}

/// Every generated stream and pond is in `Waters`, deep enough to swim in its middle, and
/// stays where it is in the world when the origin moves.
#[test]
fn generated_water_answers_where_it_is_drawn() {
    let land = island();
    let start = land.start(None);
    let streams = land.relief().water().streams.clone();
    let ponds = land.relief().water().ponds.clone();
    assert!(!streams.is_empty() && !ponds.is_empty());
    let mut world = World::default();
    let mut physics = Physics::new();
    world.set_origin(Origin::new(DVec3::new(start.x, 0.0, start.z)));
    world.set_land(land);
    let swim = |world: &World, at: DVec3| {
        let local = world.origin().to_local(at);
        let water = world.water_at(local.x, local.z).unwrap_or_else(|| panic!("no water at {at}"));
        let level = world.origin().to_world(Vec3::new(local.x, water.level, local.z)).y;
        let ground = world.ground().unwrap().height_at(at);
        (level, level - ground, water.flow)
    };
    for (s, stream) in streams.iter().enumerate() {
        let middle = stream.points[stream.points.len() / 2];
        let (level, depth, flow) = swim(&world, middle.at);
        assert!((level - middle.at.y).abs() < 0.05, "stream {s}: water at {level}, course at {}", middle.at.y);
        // Spring-fed streams are narrow, and a narrow bed is shallower.
        assert!(depth > 1.2, "stream {s} is {depth} m deep in the middle");
        assert!(flow.length() > 0.4, "stream {s} flows");
        let (a, b) = (stream.points[stream.points.len() / 3].at, stream.points[stream.points.len() / 3 + 4].at);
        let side = DVec3::new(b.z - a.z, 0.0, a.x - b.x).normalize() * 14.0;
        let bank = (a + side) / 2109.704641350211;
        println!("stream {s}: --at {:.4},{:.4} water {:.1} dir {:.2},{:.2}", bank.x, bank.z, a.y, (b - a).x, (b - a).z);
    }
    for (p, pond) in ponds.iter().enumerate() {
        let (level, depth, _) = swim(&world, DVec3::new(pond.center.x, 0.0, pond.center.y));
        assert!((level - pond.level).abs() < 0.2, "pond {p}");
        assert!(depth > 1.4, "pond {p} is {depth} m deep");
    }
    // The sea still answers off the coast, under the fresh water in the list.
    let sea_probe = world.origin().to_local(DVec3::new(start.x + 20_000.0, 0.0, start.z));
    assert!(world.water_at(sea_probe.x, sea_probe.z).is_none(), "no sea in this test's world");

    let delta = Vec3::new(3072.0, 0.0, -1024.0);
    world.shift_origin(delta, &mut physics);
    for stream in &streams {
        let middle = stream.points[stream.points.len() / 2];
        let (level, depth, _) = swim(&world, middle.at);
        assert!((level - middle.at.y).abs() < 0.05 && depth > 1.2, "after the shift: {level} {depth}");
    }
}

/// The plants of a generated world, as world positions.
fn plant_positions(world: &World) -> Vec<DVec3> {
    world
        .ecs
        .query::<(&Transform, &grimora::Renderable)>()
        .without::<&grimora::Name>()
        .iter()
        .map(|(t, _)| world.origin().to_world(t.position))
        .collect()
}

/// The island's savanna grows around the camera, comes back the same after walking away and
/// back, and its animals live in the water around it.
#[test]
fn the_savanna_grows_back_the_same_and_its_animals_keep_to_the_water() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let land = island();
    let start = land.start(None);
    let mut physics = Physics::new();
    let scene = grimora_scene::Scene::load(dir.join("scenes/world.ron")).expect("world scene loads");
    let mut world = World::from_scene(scene, &mut physics);
    world.set_origin(Origin::new(DVec3::new(start.x, 0.0, start.z)));
    world.set_land(land);
    let eye = Vec3::new(0.0, start.y as f32 + 2.0, 0.0);
    let first = world.update_land(eye, eye, &mut physics);
    println!("{first:?}");
    assert!(first.plants > 5000, "a savanna has plants: {first:?}");
    let before = plant_positions(&world);

    // Away far enough for every plant to go, through an origin shift, and back.
    let far = Vec3::new(3500.0, eye.y, 0.0);
    world.update_land(far, far, &mut physics);
    let delta = Vec3::new(2048.0, 0.0, 0.0);
    world.shift_origin(delta, &mut physics);
    let back = world.update_land(eye - delta, eye - delta, &mut physics);
    assert_eq!(back.plants, first.plants);
    let after = plant_positions(&world);
    assert_eq!(after.len(), before.len());
    let mut cells: std::collections::HashMap<(i64, i64), Vec<DVec3>> = std::collections::HashMap::new();
    for b in &before {
        cells.entry((b.x.floor() as i64, b.z.floor() as i64)).or_default().push(*b);
    }
    for a in &after {
        let (x, z) = (a.x.floor() as i64, a.z.floor() as i64);
        let found = (-1..=1)
            .flat_map(|dx| (-1..=1).map(move |dz| (x + dx, z + dz)))
            .filter_map(|cell| cells.get(&cell))
            .flatten()
            .any(|b| a.distance(*b) < 0.01);
        assert!(found, "a plant came back at {a}, where there was none");
    }

    // The animals: everything that swims or drifts is in the sea, over the floor.
    for _ in 0..30 {
        world.update_fauna(&physics, eye - delta, 1.0 / 30.0);
    }
    let population = world.fauna.population();
    println!("{population} animals, {} fish", world.fauna.fish_count());
    assert!(population > 1000 && world.fauna.fish_count() > 1000);
    let ground = |world: &World, p: Vec3| {
        let at = world.origin().to_world(p);
        world.ground().unwrap().height_at(at) as f32 - world.origin().world().y as f32
    };
    for p in world.fauna.jelly_positions().chain(world.fauna.ray_positions()) {
        let (level, _) = world.waters.calm_at(p.x, p.z).unwrap_or_else(|| panic!("no water over {p}"));
        assert!(p.y < level + 0.5 && p.y > ground(&world, p) - 0.5, "{p} out of the water ({level})");
        assert!(ground(&world, p) < level, "{p} over dry land");
    }
}

/// Walks a character at `target` from four meters off and returns how close its middle came:
/// a capsule that passes through gets to about nothing.
fn closest_walk(world: &World, physics: &mut Physics, target: Vec3, from: Vec3) -> f32 {
    let mut start = target + (from - target).with_y(0.0).normalize_or(Vec3::X) * 4.0;
    let ground = world.ground().unwrap().height_at(world.origin().to_world(start));
    start.y = (ground - world.origin().world().y) as f32 + 0.1;
    let mut body = grimora_physics::CharacterBody::new(start, 0.35, 1.8);
    let toward = (target - start).with_y(0.0).normalize();
    let dt = 1.0 / 60.0;
    let mut closest = f32::MAX;
    for _ in 0..180 {
        physics.step(dt);
        body.move_and_slide(physics, (toward * 3.0 + Vec3::NEG_Y * 4.0) * dt, dt);
        closest = closest.min((body.position - target).with_y(0.0).length());
    }
    closest
}

/// Trees and rocks near the player are solid, the ones far off are not, and that holds through
/// an origin shift.
#[test]
fn trees_and_rocks_by_the_player_are_solid() {
    let land = island();
    let start = land.start(None);
    let mut world = World::default();
    let mut physics = Physics::new();
    world.set_origin(Origin::new(DVec3::new(start.x, 0.0, start.z)));
    world.set_land(land);
    let eye = Vec3::new(0.0, start.y as f32 + 2.0, 0.0);
    let stats = world.update_land(eye, eye, &mut physics);
    assert!(stats.plant_colliders > 0, "{stats:?}");
    let solid = world.plant_colliders();
    assert!(solid.iter().all(|(_, at)| at.with_y(0.0).length() <= 61.0), "only near the player");
    let nearest = |names: &[&str]| {
        world
            .plant_colliders()
            .into_iter()
            .filter(|(name, _)| names.contains(name))
            .min_by(|a, b| a.1.length().total_cmp(&b.1.length()))
            .map(|(_, at)| at)
            .unwrap_or_else(|| panic!("no {names:?} near the start"))
    };
    let tree = nearest(&["acacia", "baoba", "palmeira", "palmeira_margem"]);
    let rock = nearest(&["kopje", "rocha", "cupinzeiro"]);
    for target in [tree, rock] {
        let closest = closest_walk(&world, &mut physics, target, eye);
        println!("{target}: {closest} m");
        assert!(closest > 0.3, "walked through the plant at {target}: {closest} m from its middle");
    }

    let delta = Vec3::new(2048.0, 0.0, -4096.0);
    world.shift_origin(delta, &mut physics);
    physics.refresh();
    let again = world.update_land(eye - delta, eye - delta, &mut physics);
    assert_eq!(again.plant_colliders, stats.plant_colliders);
    let closest = closest_walk(&world, &mut physics, tree - delta, eye - delta);
    assert!(closest > 0.3, "the tree is not solid after the origin moved: {closest}");

    let far = Vec3::new(1500.0, eye.y, 0.0) - delta;
    world.update_land(far, far, &mut physics);
    assert!(world.plant_colliders().iter().all(|(_, at)| (*at - far).with_y(0.0).length() <= 61.0));
}

/// A plant cut down stays down: saved by its `GenId`, through a walk away, an origin shift, the
/// way back and a load; and the save holds only that.
#[test]
fn a_cut_plant_stays_cut_after_saving_walking_away_and_loading() {
    use grimora::save::WorldState;
    use grimora::{GenId, Generated};
    let land = island();
    let start = land.start(None);
    let mut world = World::default();
    let mut physics = Physics::new();
    world.set_origin(Origin::new(DVec3::new(start.x, 0.0, start.z)));
    world.set_land(land);
    let eye = Vec3::new(0.0, start.y as f32 + 2.0, 0.0);
    let first = world.update_land(eye, eye, &mut physics);
    let near = |world: &World| -> Vec<(GenId, grimora::Entity)> {
        let mut found: Vec<_> = world
            .ecs
            .query::<(grimora::Entity, &Transform, &Generated)>()
            .iter()
            .filter(|(_, t, _)| (t.position - eye).length() < 40.0)
            .map(|(e, _, g)| (g.id, e))
            .collect();
        found.sort();
        found
    };
    let (cut, entity) = near(&world)[0];
    assert!(world.despawn(entity, &mut physics));
    assert!(world.generated_entity(cut).is_none());

    let mut camera = grimora_core::Camera { position: eye, ..Default::default() };
    let saved = WorldState::capture(&world, &physics, &camera);
    let text = ron::to_string(&saved.generated).unwrap();
    println!("generated diff in the save: {} bytes", text.len());
    assert_eq!(saved.generated.removed.len(), 1);
    assert!(text.len() < 64, "{text}");

    let far = Vec3::new(3500.0, eye.y, 0.0);
    world.update_land(far, far, &mut physics);
    let delta = Vec3::new(2048.0, 0.0, 0.0);
    world.shift_origin(delta, &mut physics);
    let back = world.update_land(eye - delta, eye - delta, &mut physics);
    assert_eq!(back.plants, first.plants - 1);
    assert!(world.generated_entity(cut).is_none(), "came back with its cell");

    // A fresh world loads the save.
    let mut fresh = World::default();
    let mut physics = Physics::new();
    fresh.set_origin(Origin::new(DVec3::new(start.x, 0.0, start.z)));
    fresh.set_land(island());
    fresh.update_land(eye, eye, &mut physics);
    assert!(fresh.generated_entity(cut).is_some());
    saved.restore(&mut fresh, &mut physics, &mut camera);
    let loaded = fresh.update_land(eye, eye, &mut physics);
    assert_eq!(loaded.plants, first.plants - 1);
    assert!(fresh.generated_entity(cut).is_none(), "came back after loading");
}
