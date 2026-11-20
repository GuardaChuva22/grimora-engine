//! Death as a ragdoll: a body killed standing falls, lands on the floor without going through
//! it, keeps its joints together and comes to rest.

use grimora::prelude::*;
use grimora::scene::{Scene, SceneDesc};
use grimora::{Push, Ragdoll};

const DT: f32 = 1.0 / 60.0;
const CHARTEST: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/models/chartest/chartest.gltf");

fn world() -> (Physics, World, Entity) {
    let path = CHARTEST.replace('\\', "/");
    let source = format!(
        r#"(objects: [
            (name: "floor", mesh: Cube, position: (0.0, -1.0, 0.0), scale: (100.0, 1.0, 100.0), snap_to_ground: false),
            (name: "victim", mesh: Model(path: "{path}"), position: (0.0, 0.0, 0.0), animation: "sword_idle",
             collider: None, combat: (health: 10.0, team: 1)),
        ])"#
    );
    let mut physics = Physics::new();
    let mut world = World::from_scene(Scene::build(&SceneDesc::parse(&source).unwrap()), &mut physics);
    for _ in 0..10 {
        world.update_animations(DT);
    }
    let victim = world.find("victim").unwrap();
    (physics, world, victim)
}

fn frame(physics: &mut Physics, world: &mut World) {
    world.drive_ragdolls(physics, DT);
    physics.step(DT);
    world.pose_ragdolls(physics);
    world.update_animations(DT);
}

fn joint(world: &World, entity: Entity, name: &str) -> Vec3 {
    world.joint_transform(entity, name).unwrap().position
}

fn fall(push: Vec3) {
    let (mut physics, mut world, victim) = world();
    let chest = joint(&world, victim, "chest");
    let killed = world.damage_from(victim, 100.0, &mut physics, Some(Push { at: chest, impulse: push }));
    assert_eq!(killed, Some(true));
    assert!(world.is_ragdoll(victim));
    let arm = joint(&world, victim, "forearm.L").distance(joint(&world, victim, "upper_arm.L"));
    let mut lowest = f32::MAX;
    for step in 0..(6.0 / DT) as u32 {
        frame(&mut physics, &mut world);
        for name in ["head", "hand.L", "hand.R", "foot.L", "foot.R", "hips", "chest"] {
            let at = joint(&world, victim, name);
            assert!(at.is_finite(), "{name} went to {at} at step {step}");
            lowest = lowest.min(at.y);
        }
        let stretched = joint(&world, victim, "forearm.L").distance(joint(&world, victim, "upper_arm.L"));
        assert!((stretched - arm).abs() < 0.02, "the arm went from {arm} to {stretched} long at step {step}");
    }
    let (hips, head) = (joint(&world, victim, "hips"), joint(&world, victim, "head"));
    println!("push {push}: hips {hips}, head {head}, lowest joint {lowest}");
    assert!(hips.y < 0.45, "still up: hips at {hips}");
    assert!(head.y < 0.4, "head still up at {head}");
    assert!(lowest > -0.08, "a joint went {lowest} into the floor");
    assert!(hips.distance(Vec3::ZERO) < 2.5, "thrown to {hips}");
    assert!(world.get::<Ragdoll>(victim).is_some());
}

#[test]
fn a_body_pushed_back_falls_on_its_back() {
    fall(Vec3::new(0.0, 0.0, 60.0));
}

#[test]
fn a_body_pushed_forward_falls_on_its_face() {
    fall(Vec3::new(0.0, 0.0, -60.0));
}

#[test]
fn a_body_pushed_aside_falls_aside() {
    fall(Vec3::new(70.0, 0.0, 0.0));
}

#[test]
fn a_ragdoll_goes_with_its_entity() {
    let (mut physics, mut world, victim) = world();
    let bodies = physics.collider_count();
    world.damage(victim, 100.0, &mut physics);
    assert!(physics.collider_count() > bodies);
    world.despawn(victim, &mut physics);
    assert_eq!(physics.collider_count(), bodies, "limbs left behind");
}

/// How far the hips go from where it stood, pushed back as a killing sword blow does.
#[test]
fn a_killing_blow_carries_the_body_away() {
    let (mut physics, mut world, victim) = world();
    let chest = joint(&world, victim, "chest");
    world.damage_from(victim, 100.0, &mut physics, Some(Push { at: chest, impulse: Vec3::new(0.0, 60.0, 200.0) }));
    for _ in 0..(3.0 / DT) as u32 {
        frame(&mut physics, &mut world);
    }
    let hips = joint(&world, victim, "hips");
    println!("hips ended at {hips}");
    assert!(hips.z > 1.0, "it fell in place: hips at {hips}");
    assert!(hips.z < 4.0, "thrown too far: hips at {hips}");
}

#[test]
fn a_blow_that_does_not_kill_jolts_the_trunk_and_lets_go() {
    let (mut physics, mut world, victim) = world();
    let chest = joint(&world, victim, "chest");
    let still = world.joint_transform(victim, "neck").unwrap().rotation;
    world.damage_from(
        victim,
        1.0,
        &mut physics,
        Some(Push { at: chest + Vec3::X * 0.1, impulse: Vec3::new(0.0, 40.0, 200.0) }),
    );
    assert!(world.get::<grimora::Recoil>(victim).is_some());
    let mut most: f32 = 0.0;
    for _ in 0..(2.0 / DT) as u32 {
        frame(&mut physics, &mut world);
        world.update_recoils(DT);
        most = most.max(world.joint_transform(victim, "neck").unwrap().rotation.angle_between(still));
    }
    println!("the neck turned at most {} degrees", most.to_degrees());
    assert!(most > 10f32.to_radians() && most < 70f32.to_radians());
    assert!(world.get::<grimora::Recoil>(victim).is_none());
}

fn knocked(push: Vec3) {
    let (mut physics, mut world, victim) = world();
    let chest = joint(&world, victim, "chest");
    assert!(world.knock_down(victim, &mut physics, Some(Push { at: chest, impulse: push })));
    let mut thuds = 0;
    let mut up_at = None;
    for step in 0..(12.0 / DT) as u32 {
        frame(&mut physics, &mut world);
        thuds += world.take_thuds().len();
        if up_at.is_none() && !world.is_ragdoll(victim) {
            up_at = Some(step as f32 * DT);
            assert_eq!(world.get::<Animator>(victim).unwrap().current(), Some("get_up"));
        }
        if up_at.is_some_and(|t| step as f32 * DT > t + 2.0) {
            break;
        }
    }
    let up_at = up_at.expect("it never got up");
    let hips = joint(&world, victim, "hips");
    println!("push {push}: up after {up_at:.2} s, {thuds} thuds, hips {hips}");
    assert!(thuds > 0, "a fall nobody heard");
    assert!(hips.y > 0.7, "still down: hips at {hips}");
    assert!(world.get::<Health>(victim).is_some_and(|h| !h.is_dead()));
}

#[test]
fn a_body_knocked_on_its_back_rolls_over_and_gets_up() {
    knocked(Vec3::new(0.0, 30.0, 160.0));
}

#[test]
fn a_body_knocked_on_its_face_gets_up() {
    knocked(Vec3::new(0.0, 30.0, -160.0));
}
