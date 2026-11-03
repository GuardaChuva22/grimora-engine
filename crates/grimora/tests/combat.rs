//! Fights in the real world, headless, like `npc.rs`: the wanderer's clips set
//! the timing, and blows land when they fire their events.

use std::path::PathBuf;

use grimora::prelude::*;
use grimora::save::WorldState;
use grimora::scene::{Scene, SceneDesc};

const DT: f32 = 1.0 / 60.0;
const WANDERER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/models/wanderer/wanderer.gltf");
const SANDBOX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/scenes/sandbox.ron");
/// Far enough for nobody to notice.
const NOBODY: Vec3 = Vec3::new(40.0, 1.0, 40.0);

fn fighter_at(name: &str, x: f32, z: f32, yaw: f32, team: u32, weapon: &str) -> String {
    let path = WANDERER.replace('\\', "/");
    format!(
        r#"(name: "{name}", mesh: Model(path: "{path}"), position: ({x}, 0.0, {z}), rotation: (0.0, {yaw}, 0.0),
            animation: "idle", collider: None, combat: (health: 30.0, team: {team}, {weapon}))"#
    )
}

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

/// Runs the world in the engine's order, returning what the fights did.
fn run(physics: &mut Physics, world: &mut World, player: Vec3, seconds: f32) -> Vec<CombatEvent> {
    let mut events = Vec::new();
    for _ in 0..(seconds / DT).round() as u32 {
        physics.step(DT);
        world.update_npcs(physics, player, DT);
        events.extend(world.update_combat(physics, DT));
        world.update_animations(DT);
    }
    events
}

fn hits(events: &[CombatEvent]) -> Vec<(Entity, bool)> {
    events
        .iter()
        .filter_map(|event| match *event {
            CombatEvent::Hit { target, killed, .. } => Some((target, killed)),
            _ => None,
        })
        .collect()
}

fn health(world: &World, entity: Entity) -> f32 {
    world.get::<Health>(entity).unwrap().current
}

fn clip(world: &World, entity: Entity) -> String {
    world.get::<Animator>(entity).unwrap().current().unwrap_or_default().to_owned()
}

/// `a` at the origin looking towards -Z, with a weapon, and `b` in front of it.
fn duel(b_at: (f32, f32), b_team: u32) -> (Physics, World, Entity, Entity) {
    let a = fighter_at("a", 0.0, 0.0, 0.0, 0, "weapon: (damage: 10.0, reach: 1.6, arc: 90.0)");
    let b = fighter_at("b", b_at.0, b_at.1, 0.0, b_team, "");
    let (physics, world) = world(&format!("{a}, {b}"));
    let (ea, eb) = (world.find("a").unwrap(), world.find("b").unwrap());
    (physics, world, ea, eb)
}

#[test]
fn the_blow_lands_when_the_clip_says_not_when_it_starts() {
    let (mut physics, mut world, a, b) = duel((0.0, -1.5), 1);
    assert!(world.get_mut::<Fighter>(a).unwrap().attack());
    assert!(hits(&run(&mut physics, &mut world, NOBODY, 0.1)).is_empty());
    assert_eq!(clip(&world, a), "attack");
    assert!(world.get::<Fighter>(a).unwrap().busy());
    assert_eq!(health(&world, b), 30.0);

    // tools/gen_wanderer.py puts "hit" at 47% of the 0.8 s clip.
    assert!(hits(&run(&mut physics, &mut world, NOBODY, 0.2)).is_empty());
    assert_eq!(hits(&run(&mut physics, &mut world, NOBODY, 0.12)), [(b, false)]);
    assert_eq!(health(&world, b), 20.0);
    assert_eq!(clip(&world, b), "hurt");
    assert!(world.get::<Health>(b).unwrap().staggered());

    // One swing, one hit; the clip ends and the character is free again.
    assert!(hits(&run(&mut physics, &mut world, NOBODY, 0.6)).is_empty());
    assert!(!world.get::<Fighter>(a).unwrap().busy());
    assert!(!world.get::<Health>(b).unwrap().staggered());
}

#[test]
fn the_cooldown_limits_the_blows() {
    let (mut physics, mut world, a, _) = duel((0.0, -1.5), 1);
    let cooldown = world.get::<Fighter>(a).unwrap().weapon.cooldown;
    assert!(world.get_mut::<Fighter>(a).unwrap().attack());
    assert!(!world.get_mut::<Fighter>(a).unwrap().attack(), "one at a time");
    run(&mut physics, &mut world, NOBODY, 1.0);
    assert!(!world.get::<Fighter>(a).unwrap().busy());
    assert!(!world.get_mut::<Fighter>(a).unwrap().attack(), "the clip ended before the cooldown");
    run(&mut physics, &mut world, NOBODY, cooldown - 0.9);
    assert!(world.get_mut::<Fighter>(a).unwrap().attack());
}

#[test]
fn blows_miss_what_is_too_far_behind_or_on_the_same_team() {
    for (name, at, team) in
        [("too far", (0.0, -2.6), 1), ("behind", (0.0, 1.2), 1), ("aside", (1.5, -0.3), 1), ("friend", (0.0, -1.2), 0)]
    {
        let (mut physics, mut world, a, b) = duel(at, team);
        world.get_mut::<Fighter>(a).unwrap().attack();
        let events = run(&mut physics, &mut world, NOBODY, 1.0);
        assert!(hits(&events).is_empty(), "{name}: {events:?}");
        assert_eq!(health(&world, b), 30.0, "{name}");
    }
}

#[test]
fn a_wall_between_them_stops_the_blow() {
    let a = fighter_at("a", 0.0, 0.0, 0.0, 0, "weapon: (reach: 2.5, arc: 90.0)");
    let b = fighter_at("b", 0.0, -2.2, 0.0, 1, "");
    let wall = r#"(name: "wall", mesh: Cube, position: (0.0, 0.0, -1.0), scale: (4.0, 3.0, 0.3))"#;
    let (mut physics, mut world) = world(&format!("{a}, {b}, {wall}"));
    physics.step(DT);
    let attacker = world.find("a").unwrap();
    world.get_mut::<Fighter>(attacker).unwrap().attack();
    assert!(hits(&run(&mut physics, &mut world, NOBODY, 1.0)).is_empty());
}

#[test]
fn a_blow_can_hit_a_solid_thing_itself() {
    let a = fighter_at("a", 0.0, 0.0, 0.0, 0, "weapon: (reach: 1.6, arc: 90.0)");
    let barrel =
        r#"(name: "barrel", mesh: Cube, position: (0.0, 0.0, -1.5), scale: (0.6, 1.5, 0.6), combat: (health: 5.0))"#;
    let (mut physics, mut world) = world(&format!("{a}, {barrel}"));
    physics.step(DT);
    let (attacker, target) = (world.find("a").unwrap(), world.find("barrel").unwrap());
    world.get_mut::<Fighter>(attacker).unwrap().attack();
    let events = run(&mut physics, &mut world, NOBODY, 1.0);

    // No `die` clip: it goes away, collider and all.
    assert_eq!(hits(&events), [(target, true)]);
    assert!(world.find("barrel").is_none());
    physics.step(DT);
    let ray = physics.raycast(Vec3::new(0.0, 0.5, 0.0), Vec3::NEG_Z, 3.0);
    assert!(ray.is_none(), "{ray:?}");
}

#[test]
fn without_a_clip_the_blow_lands_right_away_and_without_the_event_at_the_end() {
    let target = fighter_at("b", 0.0, -1.5, 0.0, 1, "");
    let (mut physics, mut world) = world(&format!(
        r#"(name: "a", mesh: Cube, collider: None, combat: (team: 0, weapon: (reach: 1.6))), {target},
        {},
        {}"#,
        fighter_at("c", 20.0, 0.0, 0.0, 0, r#"weapon: (reach: 1.6, event: "nothing")"#),
        fighter_at("d", 20.0, -1.5, 0.0, 1, ""),
    ));
    let (a, b, c, d) =
        (world.find("a").unwrap(), world.find("b").unwrap(), world.find("c").unwrap(), world.find("d").unwrap());
    world.get_mut::<Fighter>(a).unwrap().attack();
    let events = world.update_combat(&mut physics, DT);
    assert_eq!(hits(&events), [(b, false)]);
    assert!(!world.get::<Fighter>(a).unwrap().busy());

    world.get_mut::<Fighter>(c).unwrap().attack();
    assert!(hits(&run(&mut physics, &mut world, NOBODY, 0.7)).is_empty());
    assert_eq!(hits(&run(&mut physics, &mut world, NOBODY, 0.2)), [(d, false)]);
}

#[test]
fn a_flinch_cuts_a_blow_short() {
    let (mut physics, mut world, a, b) = duel((0.0, -1.5), 1);
    world.get_mut::<Fighter>(a).unwrap().attack();
    run(&mut physics, &mut world, NOBODY, 0.2);
    assert_eq!(world.damage(a, 1.0, &mut physics), Some(false));
    assert_eq!(clip(&world, a), "hurt");
    assert!(hits(&run(&mut physics, &mut world, NOBODY, 1.0)).is_empty());
    assert_eq!(health(&world, b), 30.0);
    assert!(!world.get::<Fighter>(a).unwrap().busy());
}

#[test]
fn the_dead_stay_down_cannot_fight_and_cannot_be_hit() {
    let a = fighter_at("a", 0.0, 0.0, 0.0, 0, "weapon: (reach: 1.6, arc: 360.0)");
    let path = WANDERER.replace('\\', "/");
    let b = format!(
        r#"(name: "b", mesh: Model(path: "{path}"), position: (0.0, 0.0, -1.5), animation: "idle",
            npc: (notice: (radius: 8.0, reaction: Attack)),
            combat: (health: 5.0, team: 1, weapon: (reach: 1.6)))"#
    );
    let (mut physics, mut world) = world(&format!("{a}, {b}"));
    let (a, b) = (world.find("a").unwrap(), world.find("b").unwrap());
    world.get_mut::<Fighter>(a).unwrap().attack();
    let events = run(&mut physics, &mut world, NOBODY, 0.6);
    assert_eq!(hits(&events), [(b, true)]);
    assert_eq!(clip(&world, b), "die");
    assert!(world.get::<Health>(b).unwrap().is_dead());
    assert_eq!(world.damage(b, 1.0, &mut physics), None);

    // It lies there, however close the player stands.
    let at = world.transform(b).unwrap().position;
    let player = at + Vec3::new(1.0, 1.0, 0.0);
    run(&mut physics, &mut world, player, 2.0);
    assert_eq!(world.transform(b).unwrap().position, at);
    assert_eq!(clip(&world, b), "die");
    assert!(world.get::<Animator>(b).unwrap().finished());
    assert!(!world.get::<Brain>(b).unwrap().noticed());

    // Its capsule is gone, so nobody bumps into the body.
    physics.step(DT);
    assert!(physics.raycast(at + Vec3::new(-2.0, 1.0, 0.0), Vec3::X, 4.0).is_none());
}

#[test]
fn an_attacker_chases_the_player_and_strikes_in_reach() {
    let path = WANDERER.replace('\\', "/");
    let (mut physics, mut world) = world(&format!(
        r#"(name: "h", mesh: Model(path: "{path}"), animation: "idle",
            npc: (walk_speed: 2.0, run_speed: 4.0, clip_speeds: (4.5, 8.5), notice: (radius: 12.0, reaction: Attack)),
            combat: (health: 30.0, team: 1, weapon: (damage: 5.0, reach: 1.7, arc: 90.0, cooldown: 1.2)))"#
    ));
    let hostile = world.find("h").unwrap();
    // The player as a real target: a still entity on another team, 7 m away.
    let player_at = Vec3::new(7.0, 0.0, 0.0);
    let target = world.ecs.spawn((Transform::from_position(player_at), Health::new(50.0, 0)));
    let player = player_at + Vec3::Y * 0.9;

    let mut events = Vec::new();
    let mut seconds = 0.0;
    while hits(&events).is_empty() && seconds < 12.0 {
        events.extend(run(&mut physics, &mut world, player, 0.1));
        seconds += 0.1;
    }
    assert_eq!(hits(&events), [(target, false)], "after {seconds} s");
    let from = world.transform(hostile).unwrap().position;
    assert!(from.distance(player_at) < 1.8, "{from}");
    assert_eq!(health(&world, target), 45.0);

    // And goes on, a blow per cooldown.
    let more = run(&mut physics, &mut world, player, 3.0);
    assert!((2..=3).contains(&hits(&more).len()), "{more:?}");

    // When the player stops counting, it goes back to its routine.
    world.npcs_ignore_player = true;
    run(&mut physics, &mut world, player, 1.5);
    assert!(!world.get::<Brain>(hostile).unwrap().noticed());
    assert!(hits(&run(&mut physics, &mut world, player, 3.0)).is_empty());
}

#[test]
fn being_hit_wakes_it_up_and_knocks_it_back() {
    let a = fighter_at("a", 0.0, 0.0, 0.0, 0, "weapon: (reach: 1.6, arc: 90.0)");
    let path = WANDERER.replace('\\', "/");
    let b = format!(
        r#"(name: "b", mesh: Model(path: "{path}"), position: (0.0, 0.0, -1.5), animation: "idle",
            npc: (notice: (radius: 0.1, reaction: Follow(distance: 1.0))),
            combat: (health: 30.0, team: 1))"#
    );
    let (mut physics, mut world) = world(&format!("{a}, {b}"));
    let (a, b) = (world.find("a").unwrap(), world.find("b").unwrap());
    let before = world.transform(b).unwrap().position;
    assert!(!world.get::<Brain>(b).unwrap().noticed());
    world.get_mut::<Fighter>(a).unwrap().attack();
    let player = Vec3::new(0.0, 1.0, 0.0);
    let mut events = Vec::new();
    while hits(&events).is_empty() {
        events.extend(run(&mut physics, &mut world, player, DT));
    }
    assert!(world.get::<Brain>(b).unwrap().noticed());
    run(&mut physics, &mut world, player, 0.3);
    let after = world.transform(b).unwrap().position;
    assert!(after.z < before.z - 0.15, "pushed away from the attacker: {before} -> {after}");
}

fn saved(world: &World, physics: &Physics) -> WorldState {
    WorldState::capture(world, physics, &Camera::default())
}

#[test]
fn health_and_death_go_into_the_save() {
    let path = WANDERER.replace('\\', "/");
    let (mut physics, mut world) = world(&format!(
        r#"(name: "hurt", mesh: Model(path: "{path}"), position: (3.0, 0.0, 0.0), animation: "idle", combat: (health: 30.0)),
           (name: "dead", mesh: Model(path: "{path}"), position: (-3.0, 0.0, 0.0), animation: "idle",
            npc: (), combat: (health: 30.0)),
           (name: "whole", mesh: Model(path: "{path}"), position: (0.0, 0.0, 3.0), animation: "idle", combat: (health: 30.0)),
           (name: "barrel", mesh: Cube, position: (0.0, 0.0, -3.0), combat: (health: 5.0))"#
    ));
    let (hurt, dead, barrel) =
        (world.find("hurt").unwrap(), world.find("dead").unwrap(), world.find("barrel").unwrap());
    let at = world.transform(dead).unwrap().position;
    let mut camera = Camera::default();
    let before = saved(&world, &physics);
    assert!(before.objects.is_empty(), "{:?}", before.objects);

    world.damage(hurt, 12.0, &mut physics);
    world.damage(dead, 99.0, &mut physics);
    world.damage(barrel, 99.0, &mut physics);
    run(&mut physics, &mut world, NOBODY, 2.0);
    let state = saved(&world, &physics);
    let of = |name: &str| state.objects.iter().find(|o| o.name.as_deref() == Some(name));
    assert_eq!(of("hurt").unwrap().health, Some(18.0));
    assert_eq!(of("dead").unwrap().health, Some(0.0));
    assert!(of("barrel").unwrap().removed);
    assert!(of("whole").is_none());

    // Loading the state from before the fight undoes all of it.
    assert_eq!(before.restore(&mut world, &mut physics, &mut camera), 0);
    let dead = world.find("dead").unwrap();
    assert_eq!(health(&world, world.find("hurt").unwrap()), 30.0);
    assert_eq!(health(&world, dead), 30.0);
    assert_eq!(clip(&world, dead), "idle");
    assert!(world.find("barrel").is_some());
    physics.step(DT);
    assert!(physics.raycast(at + Vec3::new(-2.0, 1.0, 0.0), Vec3::X, 4.0).is_some(), "its capsule is back");

    // And loading the later one puts it back, the dead one already down.
    assert_eq!(state.restore(&mut world, &mut physics, &mut camera), 0);
    assert_eq!(health(&world, world.find("hurt").unwrap()), 18.0);
    assert!(world.get::<Health>(dead).unwrap().is_dead());
    assert!(world.find("barrel").is_none());
    let animator = world.get::<Animator>(dead).unwrap();
    assert_eq!(animator.current(), Some("die"));
    assert!(animator.finished());
    drop(animator);
    physics.step(DT);
    assert!(physics.raycast(at + Vec3::new(-2.0, 1.0, 0.0), Vec3::X, 4.0).is_none());
    run(&mut physics, &mut world, NOBODY, 0.5);
    assert_eq!(clip(&world, dead), "die");
}

#[test]
fn the_sandbox_has_someone_to_fight_and_something_to_break() {
    let mut physics = Physics::new();
    let scene = Scene::load(PathBuf::from(SANDBOX)).unwrap();
    let mut world = World::from_scene(scene, &mut physics);
    physics.refresh();
    let hostile = world.find("hostile").expect("the sandbox's enemy");
    assert!(world.get::<Fighter>(hostile).is_some());
    assert!(world.get::<Health>(world.find("barrel").unwrap()).is_some());
    for neutral in ["watcher", "pilgrim", "follower", "timid"] {
        assert!(world.get::<Health>(world.find(neutral).unwrap()).is_none(), "{neutral} can't be hit");
    }

    // A player standing right in front of it, outside the circle, gets noticed and hit.
    let at = world.transform(hostile).unwrap().position;
    let mut target_at = at + Vec3::new(3.0, 0.0, 0.0);
    target_at.y = world.ground_height(target_at.x, target_at.z);
    let target = world.ecs.spawn((Transform::from_position(target_at), Health::new(60.0, 0)));
    let center = target_at + Vec3::Y * 0.9;
    let mut events = Vec::new();
    let mut seconds = 0.0;
    while hits(&events).is_empty() && seconds < 15.0 {
        events.extend(run(&mut physics, &mut world, center, 0.1));
        seconds += 0.1;
    }
    assert_eq!(hits(&events), [(target, false)], "after {seconds} s");
}

const SWORD: &str =
    r#"weapon: (damage: 5.0, cooldown: 0.0, clip: "sword_1", combo: ["sword_2", "sword_3"], blade: ())"#;

/// Clips that move their `root` joint move the character, and not the pose.
#[test]
fn a_clip_with_root_motion_reports_its_travel_and_keeps_the_pose_home() {
    let (_physics, mut world) = {
        let (mut physics, mut world) = world(&fighter_at("a", 0.0, 0.0, 0.0, 0, SWORD));
        world.update_animations(DT);
        physics.step(DT);
        (physics, world)
    };
    let a = world.find("a").unwrap();
    let animator = world.get_mut::<Animator>(a).unwrap();
    assert!(animator.replay_once("sword_roll", 0.0));
    let mut travel = Vec3::ZERO;
    for _ in 0..60 {
        animator.update(DT);
        travel += animator.root_delta();
        let root = animator.joint_transform("root").unwrap();
        assert!(root.w_axis.truncate().length() < 1e-4, "the pose stays home");
    }
    assert!(travel.z < -2.8 && travel.x.abs() < 0.01, "rolled {travel}");
}

/// A blade only hurts what it touches, once a swing.
#[test]
fn a_blade_hits_who_is_in_its_way_once_and_misses_who_is_not() {
    let near = fighter_at("a", 0.0, 0.0, 0.0, 0, SWORD);
    let target = fighter_at("near", 0.0, -1.0, 180.0, 1, "");
    let far = fighter_at("far", 0.0, -5.0, 180.0, 1, "");
    let (mut physics, mut world) = world(&format!("{near}, {target}, {far}"));
    let (a, near, far) = (world.find("a").unwrap(), world.find("near").unwrap(), world.find("far").unwrap());
    assert!(world.get_mut::<Fighter>(a).unwrap().attack());
    let events = run(&mut physics, &mut world, NOBODY, 1.2);
    assert_eq!(hits(&events), vec![(near, false)]);
    assert_eq!(health(&world, near), 25.0);
    assert_eq!(health(&world, far), 30.0);
}

/// Pressing during a blow chains the next one; not pressing lets it end.
#[test]
fn an_attack_asked_for_mid_blow_chains_the_next_clip() {
    let (mut physics, mut world) = world(&fighter_at("a", 0.0, 0.0, 0.0, 0, SWORD));
    let a = world.find("a").unwrap();
    assert!(world.get_mut::<Fighter>(a).unwrap().attack());
    run(&mut physics, &mut world, NOBODY, 0.3);
    assert!(!world.get_mut::<Fighter>(a).unwrap().attack(), "busy, so it waits");
    run(&mut physics, &mut world, NOBODY, 0.4);
    assert_eq!(world.get::<Animator>(a).unwrap().current(), Some("sword_2"));

    run(&mut physics, &mut world, NOBODY, 1.1);
    assert!(!world.get::<Fighter>(a).unwrap().busy(), "nothing more was asked");
    assert!(world.get_mut::<Fighter>(a).unwrap().attack());
    run(&mut physics, &mut world, NOBODY, 0.1);
    assert_eq!(world.get::<Animator>(a).unwrap().current(), Some("sword_1"), "the chain starts over");
}

/// A raised guard stops what comes from the front, once a swing, and nothing from behind.
#[test]
fn a_raised_guard_stops_blows_from_the_front_only() {
    let attacker = fighter_at("a", 0.0, 0.0, 0.0, 0, SWORD);
    let target = fighter_at("b", 0.0, -1.0, 180.0, 1, "");
    let (mut physics, mut world) = world(&format!("{attacker}, {target}"));
    let (a, b) = (world.find("a").unwrap(), world.find("b").unwrap());
    world.ecs.insert_one(b, Guard { raised: true, ..Default::default() }).unwrap();

    assert!(world.get_mut::<Fighter>(a).unwrap().attack());
    let events = run(&mut physics, &mut world, NOBODY, 1.3);
    let rang = |events: &[CombatEvent]| events.iter().filter(|e| matches!(e, CombatEvent::Blocked { .. })).count();
    assert_eq!(rang(&events), 1, "once a swing");
    assert!(hits(&events).is_empty());
    assert_eq!(health(&world, b), 30.0);

    // Turned away, it hurts.
    world.get_mut::<Transform>(b).unwrap().rotation = Quat::IDENTITY;
    assert!(world.get_mut::<Fighter>(a).unwrap().attack());
    let events = run(&mut physics, &mut world, NOBODY, 1.3);
    assert_eq!(rang(&events), 0);
    assert_eq!(hits(&events), vec![(b, false)]);

    // Lowered, too.
    world.get_mut::<Transform>(b).unwrap().rotation = Quat::from_rotation_y(std::f32::consts::PI);
    world.get_mut::<Guard>(b).unwrap().raised = false;
    world.revive(b, &mut physics);
    assert!(world.get_mut::<Fighter>(a).unwrap().attack());
    let events = run(&mut physics, &mut world, NOBODY, 1.3);
    assert_eq!(hits(&events), vec![(b, false)]);
}

/// Each blow on a guard costs stamina, and an empty bar lets the next one through.
#[test]
fn a_guard_takes_stamina_for_each_blow_until_there_is_none() {
    let attacker = fighter_at("a", 0.0, 0.0, 0.0, 0, SWORD);
    let target = fighter_at("b", 0.0, -1.0, 180.0, 1, "");
    let (mut physics, mut world) = world(&format!("{attacker}, {target}"));
    let (a, b) = (world.find("a").unwrap(), world.find("b").unwrap());
    world.ecs.insert(b, (Guard { raised: true, cost: 10.0, ..Default::default() }, Stamina::new(10.0))).unwrap();

    assert!(world.get_mut::<Fighter>(a).unwrap().attack());
    let events = run(&mut physics, &mut world, NOBODY, 0.45);
    assert!(events.iter().any(|e| matches!(e, CombatEvent::Blocked { .. })));
    assert_eq!(world.get::<Stamina>(b).unwrap().current, 0.0, "paid on the clang, and not before");
    assert_eq!(health(&world, b), 30.0);

    run(&mut physics, &mut world, NOBODY, 0.9);
    let stamina = world.get_mut::<Stamina>(b).unwrap();
    stamina.current = 0.0;
    stamina.regen = 0.0;
    assert!(world.get_mut::<Fighter>(a).unwrap().attack());
    let events = run(&mut physics, &mut world, NOBODY, 1.0);
    assert_eq!(hits(&events), vec![(b, false)], "out of stamina, the guard breaks");
}

/// Stepping between blows doesn't lose the chain, unless the step goes on too long.
#[test]
fn an_attack_soon_after_walking_off_still_chains() {
    let (mut physics, mut world) = world(&fighter_at("a", 0.0, 0.0, 0.0, 0, SWORD));
    let a = world.find("a").unwrap();
    let walk_off = |physics: &mut Physics, world: &mut World| {
        assert!(world.get_mut::<Fighter>(a).unwrap().attack());
        run(physics, world, NOBODY, 0.1);
        world.get_mut::<Fighter>(a).unwrap().walk_off();
        assert!(!world.get::<Fighter>(a).unwrap().busy());
    };
    walk_off(&mut physics, &mut world);
    run(&mut physics, &mut world, NOBODY, 0.2);
    assert!(world.get_mut::<Fighter>(a).unwrap().attack());
    run(&mut physics, &mut world, NOBODY, 0.1);
    assert_eq!(world.get::<Animator>(a).unwrap().current(), Some("sword_2"));

    world.get_mut::<Fighter>(a).unwrap().interrupt();
    walk_off(&mut physics, &mut world);
    run(&mut physics, &mut world, NOBODY, 0.6);
    assert!(world.get_mut::<Fighter>(a).unwrap().attack());
    run(&mut physics, &mut world, NOBODY, 0.1);
    assert_eq!(world.get::<Animator>(a).unwrap().current(), Some("sword_1"), "too late, the chain starts over");
}

#[test]
fn stamina_pays_for_blows_and_comes_back() {
    let (mut physics, mut world) = world(&fighter_at("a", 0.0, 0.0, 0.0, 0, SWORD));
    let a = world.find("a").unwrap();
    world.get_mut::<Fighter>(a).unwrap().weapon.stamina = 10.0;
    let mut stamina = Stamina::new(30.0);
    stamina.current = 0.0;
    world.ecs.insert_one(a, stamina).unwrap();
    assert!(world.get_mut::<Fighter>(a).unwrap().attack());
    run(&mut physics, &mut world, NOBODY, 0.1);
    assert!(!world.get::<Fighter>(a).unwrap().busy(), "out of stamina, the blow doesn't start");
    run(&mut physics, &mut world, NOBODY, 1.5);
    let stamina = world.get::<Stamina>(a).unwrap().current;
    assert!(stamina > 5.0, "it should refill, has {stamina}");
    assert!(world.get_mut::<Fighter>(a).unwrap().attack());
    run(&mut physics, &mut world, NOBODY, 0.1);
    assert!(world.get::<Fighter>(a).unwrap().busy());
}

#[test]
fn nothing_hurts_a_character_while_it_is_untouchable() {
    let (mut physics, mut world) = world(&fighter_at("a", 0.0, 0.0, 0.0, 0, ""));
    let a = world.find("a").unwrap();
    world.get_mut::<Health>(a).unwrap().make_untouchable(0.2);
    assert_eq!(world.damage(a, 5.0, &mut physics), None);
    run(&mut physics, &mut world, NOBODY, 0.3);
    assert_eq!(world.damage(a, 5.0, &mut physics), Some(false));
    assert_eq!(health(&world, a), 25.0);
}

/// What `World::attach` made follows the joint, and leaves with its parent.
#[test]
fn an_attached_mesh_follows_its_joint_and_goes_with_its_parent() {
    let (mut physics, mut world) = world(&fighter_at("a", 2.0, 0.0, 0.0, 0, ""));
    let a = world.find("a").unwrap();
    let mesh = world.get::<Renderable>(a).unwrap().mesh;
    let held = world.attach(a, "hand.R", mesh, Transform::IDENTITY);
    let at = world.joint_transform(a, "hand.R").unwrap().position;
    assert!(world.transform(held).unwrap().position.distance(at) < 1e-4);
    world.despawn(a, &mut physics);
    world.update_attachments();
    assert!(world.transform(held).is_none());
}
