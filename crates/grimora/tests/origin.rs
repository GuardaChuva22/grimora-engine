//! Moving the origin changes nothing in the world: the same run, with and without a shift in
//! the middle, ends with everything in the same world positions.

use std::path::Path;

use grimora::prelude::*;
use grimora::scene::{Scene, SceneDesc};

const DT: f32 = 1.0 / 60.0;
/// What the shifted run moves its origin by, halfway: a jump the runner could make.
const DELTA: Vec3 = Vec3::new(512.0, 0.0, -256.0);

/// A bowl of a sea with fish, gulls, crabs, rays and jellyfish, a floating crate, a torch, a
/// sentry on a dry ledge and the player walking on it.
fn scene() -> (World, Physics) {
    let text = r#"SceneDesc(
        terrain: (size: 240.0, resolution: 60, height: 0.0, seed: 1, edge_rise: 34.0, flat_radius: 50.0),
        water: [(position: (0.0, 20.0, 0.0), sea: (swell: 0.5, length: 40.0))],
        day_cycle: (time: 0.0, day_length: 0.0, weather: Clear),
        fauna: [
            (species: "../fauna/sardinha.ron", position: (0.0, 0.0, 0.0), radius: 18.0, count: 120, seed: 7),
            (species: "../fauna/gaivota.ron", position: (0.0, 0.0, 0.0), radius: 90.0, count: 4, seed: 3),
            (species: "../fauna/caranguejo.ron", position: (70.0, 0.0, 0.0), radius: 14.0, count: 10, seed: 4),
            (species: "../fauna/arraia.ron", position: (63.0, 0.0, 0.0), radius: 8.0, count: 2, seed: 5),
            (species: "../fauna/agua_viva.ron", position: (54.0, 0.0, 0.0), radius: 10.0, count: 6, seed: 6),
        ],
        objects: [
            (name: "ledge", mesh: Cube, position: (0.0, 30.0, 150.0), scale: (40.0, 1.0, 40.0), snap_to_ground: false),
            (name: "sentry", mesh: Cube, position: (6.0, 31.0, 150.0), snap_to_ground: false, npc: ()),
            (name: "crate", mesh: Cube, position: (5.0, 21.0, 0.0), scale: (2.0, 1.0, 2.0), snap_to_ground: false,
             body: Dynamic(), float: (fill: 0.5, cell: 0.25, ballast: 0.5)),
            (name: "torch", mesh: Cube, position: (-4.0, 31.0, 150.0), snap_to_ground: false,
             effect: (path: "../effects/torch.ron")),
        ],
    )"#;
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenes");
    let scene = Scene::build_in(&SceneDesc::parse(text).unwrap(), &dir);
    let mut physics = Physics::new();
    let world = World::from_scene(scene, &mut physics);
    (world, physics)
}

struct Run {
    world: World,
    physics: Physics,
    player: ThirdPersonController,
    camera: Camera,
    input: Input,
    time: f32,
    first: WorldPos,
}

impl Run {
    /// The scene with its zero at `origin` in the world and the player starting at `start`.
    fn new(origin: WorldPos, start: Vec3) -> Self {
        let (mut world, physics) = scene();
        world.set_origin(Origin::new(origin));
        let player = ThirdPersonController::new(start, 0.0);
        Self { world, physics, player, camera: Camera::default(), input: Input::default(), time: 0.0, first: origin }
    }

    /// One frame as the engine runs it, with the player walking forward.
    fn frame(&mut self) {
        self.input.key_down(KeyCode::KeyW);
        let feet = self.player.body.position;
        self.player.water = self.world.water_at(feet.x, feet.z);
        self.world.update_water_view(self.camera.position, DT);
        self.world.update_floating(&mut self.physics, self.time);
        self.physics.step(DT);
        self.world.sync_bodies(&self.physics);
        self.player.update(&self.input, &mut self.physics, &mut self.camera, DT);
        self.world.update_npcs(&mut self.physics, self.player.body.position, DT);
        self.world.update_animations(DT);
        self.world.update_particles(DT, self.camera.position);
        self.world.update_fauna(&self.physics, self.camera.position, DT);
        self.world.take_splashes();
        self.input.end_frame();
        self.time += DT;
    }

    fn shift(&mut self, delta: Vec3) {
        self.world.shift_origin(delta, &mut self.physics);
        self.player.shift(-delta);
        self.camera.position -= delta;
    }

    /// Where everything is, in world positions relative to the first origin.
    fn places(&self) -> Vec<(&'static str, Vec3)> {
        let back = (self.world.origin().world() - self.first).as_vec3();
        let mut places = vec![("player", self.player.body.position + back)];
        for name in ["crate", "sentry"] {
            let entity = self.world.find(name).unwrap();
            places.push((name, self.world.transform(entity).unwrap().position + back));
        }
        places.extend(self.world.fauna.positions().map(|p| ("fish", p + back)));
        places.extend(self.world.fauna.bird_positions().map(|p| ("gull", p + back)));
        places.extend(self.world.fauna.crab_positions().map(|p| ("crab", p + back)));
        places.extend(self.world.fauna.ray_positions().map(|p| ("ray", p + back)));
        places.extend(self.world.fauna.jelly_positions().map(|p| ("jelly", p + back)));
        let particles = self.world.active_particles();
        places.extend(particles.iter().map(|p| ("particle", Vec3::from(p.position) + back)));
        places
    }
}

/// Runs the scene twice from `origin`, shifting one run halfway, and compares them.
fn compare(origin: WorldPos, start: Vec3) -> Run {
    let (mut still, mut moved) = (Run::new(origin, start), Run::new(origin, start));
    for _ in 0..90 {
        still.frame();
        moved.frame();
    }
    moved.shift(DELTA);
    assert_eq!(moved.world.origin().world(), origin + DELTA.as_dvec3());
    for _ in 0..90 {
        still.frame();
        moved.frame();
    }
    let (a, b) = (still.places(), moved.places());
    assert_eq!(a.len(), b.len(), "the same things exist");
    // Positions near 600 m keep about 0.06 mm in f32, and a school passes that around for a
    // second and a half. The character controller alone sometimes stands still for a frame,
    // with or without a shift, so a rounding apart can cost it one or two steps of 7.5 cm.
    for ((name, a), (_, b)) in a.iter().zip(&b) {
        let most = if *name == "player" { 0.16 } else { 0.03 };
        assert!(a.distance(*b) < most, "{name} ended {} m away: {a} against {b}", a.distance(*b));
    }
    assert!(still.player.body.position.distance(start) > 2.0, "the player moved");
    still
}

#[test]
fn a_shift_halfway_changes_nothing_in_the_world() {
    compare(WorldPos::ZERO, Vec3::new(0.0, 31.2, 145.0));
}

/// 15 000 km out, swimming in the sea: the waves, the fish and the swimmer behave as at home.
#[test]
fn far_out_at_sea_everything_still_holds() {
    let far = WorldPos::new(15_000_000.0, 0.0, -3_000_000.0);
    let run = compare(far, Vec3::new(10.0, 19.5, 0.0));
    let feet = run.player.body.position;
    let water = run.world.water_at(feet.x, feet.z).expect("still at sea");
    assert!((feet.y - water.level).abs() < 2.0, "swimming at the surface: {feet} in {}", water.level);
}
