//! How a roll meets the ground, on the skinned mesh in every frame, with the root's travel added: how
//! far the body is from the floor, what touches it, and whether what touches it slides. A body that
//! rolls keeps the point it stands on still; one that spins around its hips sinks or floats.
//! `ROLL=<clip> cargo test --release -p grimora_scene --test roll -- --nocapture` prints the frames,
//! and `RIG=1` the rest joints that `tools/gen_roll_clip.py` builds on.

use glam::{Mat4, Vec3};
use grimora_core::{Control, MeshData, PoseClip, PoseSampler, Rig, Skeleton};
use grimora_scene::{DEFAULT_MAX_TEXTURE_SIZE, load_model, load_pose_clips};

const MODEL: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/models/chartest/chartest.gltf");
const FPS: f32 = 60.0;
/// Meters of the mesh allowed under the floor: skin pressing on the ground.
const SINK: f32 = 0.03;
/// Meters above the floor that still count as touching it.
const CONTACT: f32 = 0.05;
/// Meters per second the trunk can slide, on average, while it rolls over the floor.
const MOST_SLIDE: f32 = 0.6;
/// Meters the hands stay from the chest while upside down: closer is a ball.
const LEAST_HAND_OUT: f32 = 0.36;
/// The ankle's height resting on the floor.
const ANKLE: f32 = 0.1;

struct Frame {
    time: f32,
    lowest: f32,
    part: String,
    hand_r: f32,
    hand_l: f32,
    feet: [f32; 2],
    tip: f32,
    up: f32,
    /// The lowest point of the trunk alone (a hand may dip, the body rests on this) and how far along
    /// z that same point of the skin moved since the last frame, travel included.
    trunk: f32,
    trunk_slip: f32,
}

fn frames(mesh: &MeshData, skeleton: &Skeleton, rig: &Rig, clip: &PoseClip) -> Vec<Frame> {
    let joint = |name: &str| skeleton.joint(name).unwrap();
    let (weapon, chest, hips) = (joint("weapon.R"), joint("chest"), joint("hips"));
    let (hand_r, hand_l) = (joint("hand.R"), joint("hand.L"));
    let (foot_l, foot_r) = (joint("foot.L"), joint("foot.R"));
    let in_trunk = |j: usize| {
        let name = skeleton.joints[j].name.as_str();
        ["hips", "spine", "chest", "neck", "head"].contains(&name)
            || name.starts_with("clavicle")
            || name.starts_with("upper_arm")
    };
    let sampler = PoseSampler::new(clip);
    let (mut pose, mut globals) = (Vec::new(), Vec::new());
    let mut before: Vec<Vec3> = Vec::new();
    let mut out = Vec::new();
    for k in 0..=(clip.duration * FPS).round() as usize {
        let time = k as f32 / FPS;
        let controls = sampler.sample(time);
        let solved = rig.solve(&controls);
        rig.local_pose(&solved, &controls, Vec3::ZERO, &mut pose);
        skeleton.global_transforms(&pose, &mut globals);
        let skin: Vec<Mat4> = globals.iter().zip(&skeleton.joints).map(|(g, j)| *g * j.inverse_bind).collect();
        let root = controls[Control::Root];
        let travel = Vec3::new(root.x, 0.0, root.z);
        let (mut lowest, mut part) = (f32::MAX, 0);
        let (mut trunk, mut trunk_vertex) = (f32::MAX, 0);
        let mut now = Vec::with_capacity(mesh.vertices.len());
        for (index, v) in mesh.vertices.iter().enumerate() {
            let mut p = Vec3::ZERO;
            for (j, w) in v.joints.iter().zip(v.weights) {
                p += skin[*j as usize].transform_point3(Vec3::from(v.position)) * (w as f32 / 255.0);
            }
            now.push(p + travel);
            let (i, _) = v.weights.iter().enumerate().max_by_key(|(_, w)| **w).unwrap();
            let owner = v.joints[i] as usize;
            if p.y < lowest {
                (lowest, part) = (p.y, owner);
            }
            if p.y < trunk && in_trunk(owner) {
                (trunk, trunk_vertex) = (p.y, index);
            }
        }
        let trunk_slip = before.get(trunk_vertex).map_or(0.0, |b| now[trunk_vertex].z - b.z);
        before = now;
        let place = |j: usize| globals[j].w_axis.truncate();
        out.push(Frame {
            time,
            lowest,
            part: skeleton.joints[part].name.clone(),
            hand_r: place(hand_r).distance(place(chest)),
            hand_l: place(hand_l).distance(place(chest)),
            feet: [place(foot_l).y, place(foot_r).y],
            tip: globals[weapon].transform_point3(Vec3::new(0.0, 0.8, 0.0)).y,
            up: (place(chest) - place(hips)).normalize().y,
            trunk,
            trunk_slip,
        });
    }
    out
}

#[test]
fn a_roll_stays_on_the_floor_and_does_not_slide_or_ball_up() {
    let mesh = load_model(MODEL.as_ref(), DEFAULT_MAX_TEXTURE_SIZE).unwrap();
    let skeleton = mesh.skeleton.as_ref().unwrap();
    let rig = Rig::new(skeleton).unwrap();
    if std::env::var("RIG").is_ok() {
        for j in &skeleton.joints {
            let at = j.inverse_bind.inverse().w_axis;
            println!("{} {:.4} {:.4} {:.4}", j.name, at.x, at.y, at.z);
        }
    }
    let clips = load_pose_clips(MODEL.as_ref());
    let chosen = std::env::var("ROLL").ok();
    let name = chosen.clone().unwrap_or_else(|| "sword_roll".into());
    let Some((_, clip)) = clips.iter().find(|(n, _)| *n == name) else { panic!("no clip {name}") };
    let all = frames(&mesh, skeleton, &rig, clip);
    if std::env::var("ROLL_DATA").is_ok() {
        for f in &all {
            println!("F {} {} {}", f.time, f.trunk, f.trunk_slip);
        }
        return;
    }
    let mut sunk = 0.0f32;
    let (mut slide, mut slid) = (0.0f32, 0usize);
    for (i, f) in all.iter().enumerate() {
        sunk = sunk.max(-f.lowest);
        let touching = i > 0 && f.trunk < CONTACT;
        if touching {
            slide += f.trunk_slip.abs() * FPS;
            slid += 1;
        }
        if chosen.is_some() {
            println!(
                "{:.3} low {:6.3} {:10} trunk {:6.3} slide {:5.2} hand {:.2} {:.2} feet {:.2} {:.2} tip {:5.2} up {:5.2}",
                f.time,
                f.lowest,
                f.part,
                f.trunk,
                f.trunk_slip * FPS,
                f.hand_r,
                f.hand_l,
                f.feet[0],
                f.feet[1],
                f.tip,
                f.up
            );
        }
    }
    let inverted: Vec<&Frame> = all.iter().filter(|f| f.up < 0.0).collect();
    let closest_r = inverted.iter().map(|f| f.hand_r).fold(f32::MAX, f32::min);
    let closest_l = inverted.iter().map(|f| f.hand_l).fold(f32::MAX, f32::min);
    let floating = inverted.iter().filter(|f| f.trunk > CONTACT).count();
    let average = slide / slid.max(1) as f32;
    let ankle = all.iter().map(|f| f.feet[0].min(f.feet[1])).fold(f32::MAX, f32::min);
    println!(
        "{name}: sinks {:.0} mm, trunk slides {average:.2} m/s over {slid} frames, upside down for {} frames ({floating} in the air), hands from the chest {closest_r:.2} {closest_l:.2}, lowest ankle {ankle:.3}",
        sunk * 1000.0,
        inverted.len(),
    );
    if chosen.is_none() {
        assert!(sunk <= SINK, "{name} goes {:.0} mm into the floor", sunk * 1000.0);
        assert!(average <= MOST_SLIDE, "{name} slides at {average:.2} m/s");
        assert!(floating == 0, "{name} floats upside down for {floating} frames");
        assert!(closest_r >= LEAST_HAND_OUT && closest_l >= LEAST_HAND_OUT, "{name} balls up its hands");
        assert!(ankle >= ANKLE - SINK, "{name} has a foot in the floor");
    }
}

/// Degrees a joint can turn in a frame, in the frame of its parent: more is a joint flipping over, not
/// a move. Played the way the game plays it, on the baked clip.
const MOST_FLIP: f32 = 30.0;
/// The fade the game uses to walk or stand out of a roll: `LOCOMOTION` in `blows.rs`.
const LEAVING: f32 = 0.2;

#[test]
fn the_arms_do_not_flip_in_a_roll() {
    let mesh = load_model(MODEL.as_ref(), DEFAULT_MAX_TEXTURE_SIZE).unwrap();
    let skeleton = mesh.skeleton.as_ref().unwrap();
    let mut animator = grimora_core::Animator::new(skeleton.clone());
    let clip = std::env::var("ROLL").unwrap_or_else(|_| "sword_roll".into());
    assert!(animator.replay_once(&clip, 0.0));
    animator.update(1.0 / FPS);
    let joints = ["upper_arm.R", "forearm.R", "hand.R", "upper_arm.L", "forearm.L", "hand.L"];
    let parents = ["chest", "upper_arm.R", "forearm.R", "chest", "upper_arm.L", "forearm.L"];
    let turn = |animator: &grimora_core::Animator, joint: &str, parent: &str| {
        let (j, p) = (animator.joint_transform(joint).unwrap(), animator.joint_transform(parent).unwrap());
        glam::Quat::from_mat4(&(p.inverse() * j).mul_scalar(1.0)).normalize()
    };
    let mut before: Vec<glam::Quat> = joints.iter().zip(&parents).map(|(j, p)| turn(&animator, j, p)).collect();
    let mut worst = vec![(0.0f32, 0.0f32); joints.len()];
    let mut time = 1.0 / FPS;
    while !animator.finished() {
        animator.update(1.0 / FPS);
        time += 1.0 / FPS;
        for (i, (j, p)) in joints.iter().zip(&parents).enumerate() {
            let now = turn(&animator, j, p);
            let degrees = before[i].angle_between(now).to_degrees();
            if degrees > worst[i].0 {
                worst[i] = (degrees, time);
            }
            if std::env::var("ROLL_TURNS").is_ok() && degrees > 8.0 {
                println!("{time:.3} {j} {degrees:.0}");
            }
            before[i] = now;
        }
    }
    for (j, (degrees, time)) in joints.iter().zip(&worst) {
        println!("[flip] {clip} {j}: {degrees:.0} deg in a frame at {time:.2}");
    }
    assert!(worst.iter().all(|w| w.0 <= MOST_FLIP), "{clip} flips a joint");
    // Nor on the way out, into what the game plays next.
    for next in ["sword_idle", "heavy_idle", "sword_run"] {
        let mut animator = grimora_core::Animator::new(skeleton.clone());
        assert!(animator.replay_once(&clip, 0.0));
        while !animator.finished() {
            animator.update(1.0 / FPS);
        }
        assert!(animator.replay_once(next, LEAVING));
        let mut top = 0.0f32;
        let mut before: Vec<glam::Quat> = joints.iter().zip(&parents).map(|(j, p)| turn(&animator, j, p)).collect();
        for _ in 0..(2.0 * LEAVING * FPS) as usize {
            animator.update(1.0 / FPS);
            for (i, (j, p)) in joints.iter().zip(&parents).enumerate() {
                let now = turn(&animator, j, p);
                top = top.max(before[i].angle_between(now).to_degrees());
                before[i] = now;
            }
        }
        println!("[flip] {clip} into {next}: {top:.0} deg in a frame");
        assert!(top <= MOST_FLIP, "{clip} flips a joint going into {next}");
    }
}
