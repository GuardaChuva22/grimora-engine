//! How a sword blow moves, measured on the baked clip: the body must drive the blade, from the
//! hips out to the tip, and stay on its feet. `cargo test -p grimora_scene --test blows -- --nocapture`
//! prints the numbers. Only blows rewritten against this check are listed; `BLOW=<clip>` measures
//! another without judging it, while it is being written.

use glam::{Quat, Vec3};
use grimora_core::{AnimationClip, Animator, Control, PoseSampler, Rig, Skeleton};
use grimora_scene::{DEFAULT_MAX_TEXTURE_SIZE, load_model, load_pose_clips};

const MODEL: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/models/chartest/chartest.gltf");
const BLOWS: [&str; 6] = ["sword_1", "sword_2", "sword_3", "heavy_1", "heavy_2", "heavy_3"];
/// Blows that follow one another in a combo.
const COMBO: [(&str, &str); 4] =
    [("sword_1", "sword_2"), ("sword_2", "sword_3"), ("heavy_1", "heavy_2"), ("heavy_2", "heavy_3")];
const FPS: f32 = 60.0;
/// From the weapon joint to the tip, at the scale the sword is held.
const TIP: f32 = 0.787;
const HEAVY_TIP: f32 = 1.1;
/// Two hands on the handle pull the right wrist a little further than one.
const WRIST_BEND: f32 = 70.0;
const HEAVY_WRIST_BEND: f32 = 75.0;
/// From the body out: each link turns fastest after the one before, and the tip of the blade,
/// which in a thrust hardly turns at all, moves fastest last.
const CHAIN: [&str; 4] = ["hips", "chest", "upper_arm.R", "forearm.R"];
/// A clip is baked at 60 Hz, so links this many frames apart count as peaking together.
const CHAIN_SLACK: usize = 1;

struct Frame {
    position: Vec<Vec3>,
    rotation: Vec<Quat>,
}

fn frames(skeleton: &Skeleton, clip: &AnimationClip) -> Vec<Frame> {
    let mut out = Vec::new();
    let mut globals = Vec::new();
    let count = (clip.duration * FPS).round() as usize;
    for k in 0..=count {
        let mut pose = skeleton.rest_pose();
        clip.sample(k as f32 / FPS, &mut pose);
        skeleton.global_transforms(&pose, &mut globals);
        let parts: Vec<_> = globals.iter().map(|g| g.to_scale_rotation_translation()).collect();
        out.push(Frame {
            position: parts.iter().map(|p| p.2).collect(),
            rotation: parts.iter().map(|p| p.1.normalize()).collect(),
        });
    }
    out
}

fn event(clip: &AnimationClip, name: &str) -> f32 {
    clip.events.iter().find(|e| e.name == name).unwrap_or_else(|| panic!("{}: no {name}", clip.name)).time
}

/// Degrees per second of a joint between each frame and the next.
fn turning(frames: &[Frame], joint: usize) -> Vec<f32> {
    frames.windows(2).map(|w| w[0].rotation[joint].angle_between(w[1].rotation[joint]).to_degrees() * FPS).collect()
}

/// The point on the segment `a`..`b` nearest to `p`, on the ground.
fn off_segment(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z);
    let (p, a, b) = (flat(p), flat(a), flat(b));
    let along = (p - a).dot(b - a) / (b - a).length_squared().max(1e-6);
    p.distance(a + (b - a) * along.clamp(0.0, 1.0))
}

#[test]
fn blows_are_driven_by_the_body() {
    let mesh = load_model(MODEL.as_ref(), DEFAULT_MAX_TEXTURE_SIZE).unwrap();
    let skeleton = mesh.skeleton.as_ref().unwrap();
    let joint = |name: &str| skeleton.joint(name).unwrap_or_else(|| panic!("no joint {name}"));
    let draft = std::env::var("BLOW").ok();
    let names = draft.clone().map_or_else(|| BLOWS.map(String::from).to_vec(), |name| vec![name]);
    for name in &names {
        let clip = &skeleton.clips[skeleton.clip(name).unwrap()];
        let all = frames(skeleton, clip);
        let (swing, on, off) = (event(clip, "swing"), event(clip, "hit_on"), event(clip, "hit_off"));
        let window = ((swing - 0.08) * FPS).max(0.0) as usize..((off * FPS) as usize).min(all.len() - 2);

        // The chain: each link at its fastest later than the one before.
        let mut last = 0;
        let mut in_order = true;
        for link in CHAIN {
            let speed = turning(&all, joint(link));
            let peak = window.clone().max_by(|a, b| speed[*a].total_cmp(&speed[*b])).unwrap();
            println!("[blow] {name} {link:12} fastest {:5.0} deg/s at {:.3}", speed[peak], peak as f32 / FPS);
            in_order &= peak + CHAIN_SLACK >= last;
            last = peak;
        }

        // The tip: slow up, fast down.
        let weapon = joint("weapon.R");
        let heavy = name.starts_with("heavy");
        let length = if heavy { HEAVY_TIP } else { TIP };
        let tip: Vec<Vec3> = all.iter().map(|f| f.position[weapon] + f.rotation[weapon] * Vec3::Y * length).collect();
        let tip_speed: Vec<f32> = tip.windows(2).map(|w| w[0].distance(w[1]) * FPS).collect();
        let before = tip_speed[..(swing * FPS) as usize].iter().cloned().fold(0.0, f32::max);
        let cut = tip_speed[window.clone()].iter().cloned().fold(0.0, f32::max);
        let peak = window.clone().max_by(|a, b| tip_speed[*a].total_cmp(&tip_speed[*b])).unwrap();
        println!("[blow] {name} tip fastest at {:.3}", peak as f32 / FPS);
        in_order &= peak + CHAIN_SLACK >= last;
        println!("[blow] {name} tip up to {before:.1} m/s winding up, {cut:.1} m/s cutting");

        // The arc: how flat is the path of the tip while the blade can hit.
        let (a, b) = ((on * FPS) as usize, (off * FPS) as usize);
        let (first, middle, end) = (tip[a], tip[(a + b) / 2], tip[b]);
        let normal = (middle - first).cross(end - first).normalize_or_zero();
        let bend = tip[a..=b].iter().map(|p| (*p - first).dot(normal).abs()).fold(0.0, f32::max);
        let slope = normal.dot(Vec3::Y).abs().acos().to_degrees();
        println!("[blow] {name} cutting plane {slope:.0} deg from flat, tip leaves it by {:.0} mm", bend * 1000.0);

        // The feet: on the ground they stay put, and the body stays over them.
        let (foot_l, foot_r, hips, chest) = (joint("foot.L"), joint("foot.R"), joint("hips"), joint("chest"));
        let lowest = |foot: usize| all.iter().map(|f| f.position[foot].y).fold(f32::MAX, f32::min);
        let mut slide = 0.0f32;
        let mut slide_at = (true, 0.0);
        for foot in [foot_l, foot_r] {
            let low = lowest(foot) + 0.006;
            for (k, w) in all.windows(2).enumerate() {
                if w[0].position[foot].y < low && w[1].position[foot].y < low {
                    let moved = (w[1].position[foot] - w[0].position[foot]) * Vec3::new(1.0, 0.0, 1.0);
                    if moved.length() * FPS > slide {
                        slide = moved.length() * FPS;
                        slide_at = (foot == foot_l, k as f32 / FPS);
                    }
                }
            }
        }
        let off_feet = all
            .iter()
            .map(|f| {
                let center = (f.position[hips] + f.position[chest]) * 0.5;
                off_segment(center, f.position[foot_l], f.position[foot_r])
            })
            .fold(0.0, f32::max);
        println!(
            "[blow] {name} planted foot slides {slide:.2} m/s ({} at {:.2} s), body up to {:.0} mm off the feet",
            if slide_at.0 { "left" } else { "right" },
            slide_at.1,
            off_feet * 1000.0
        );

        // The wrist: how far the hand turns on the forearm.
        let (forearm, hand) = (joint("forearm.R"), joint("hand.R"));
        let rest_hand = skeleton.rest_pose()[hand].rotation;
        let (mut bend, mut twist) = ((0.0f32, 0.0), (0.0f32, 0.0));
        for (k, f) in all.iter().enumerate() {
            let turn = (rest_hand.inverse() * f.rotation[forearm].inverse() * f.rotation[hand]).normalize();
            let about_bone = Quat::from_xyzw(0.0, turn.y, 0.0, turn.w).normalize();
            let (swing, spin) =
                ((turn * about_bone.inverse()).angle_between(Quat::IDENTITY), about_bone.angle_between(Quat::IDENTITY));
            let at = k as f32 / FPS;
            if swing.to_degrees() > bend.0 {
                bend = (swing.to_degrees(), at);
            }
            if spin.to_degrees() > twist.0 {
                twist = (spin.to_degrees(), at);
            }
        }
        println!(
            "[blow] {name} wrist bends {:.0} deg at {:.3}, twists {:.0} deg at {:.3}",
            bend.0, bend.1, twist.0, twist.1
        );

        for k in (((swing - 0.1) * FPS).max(0.0) as usize..((off + 0.06) * FPS) as usize).step_by(2) {
            println!("[blow]   {:.3} tip {:5.1} m/s  height {:.2}", k as f32 / FPS, tip_speed[k], tip[k].y);
        }

        if draft.is_some() {
            continue;
        }
        assert!(in_order, "{name}: the chain peaks out of order");
        assert!(cut > before * 1.5, "{name}: the cut is hardly faster than the wind-up");
        assert!(cut < 45.0, "{name}: the tip reaches {cut:.0} m/s");
        let limit = if heavy { HEAVY_WRIST_BEND } else { WRIST_BEND };
        assert!(bend.0 < limit, "{name}: the wrist bends {:.0} deg", bend.0);
        assert!(twist.0 < 105.0, "{name}: the wrist twists {:.0} deg", twist.0);
        assert!(slide < 0.3, "{name}: a planted foot slides at {slide:.2} m/s");
        assert!(off_feet < 0.2, "{name}: the body is {:.0} mm off the feet", off_feet * 1000.0);
    }
}

/// `POSES=<clip> cargo test -p grimora_scene --test blows key_poses -- --nocapture`: what each
/// key of the sword hand asks of the arms, on the exact keys. Prints nothing without it.
#[test]
fn key_poses() {
    let Ok(name) = std::env::var("POSES") else { return };
    let mesh = load_model(MODEL.as_ref(), DEFAULT_MAX_TEXTURE_SIZE).unwrap();
    let skeleton = mesh.skeleton.as_ref().unwrap();
    let rig = Rig::new(skeleton).unwrap();
    let clips = load_pose_clips(MODEL.as_ref());
    for (clip_name, clip) in clips.iter().filter(|(clip_name, _)| clip_name.starts_with(&name)) {
        let sampler = PoseSampler::with_springs(clip, false);
        for &(time, _) in &clip.tracks[&Control::HandR] {
            let mut controls = sampler.sample(time);
            let solved = rig.solve(&controls);
            // How far to turn the edge around the blade for the wrists to bend the least.
            let blade = controls[Control::Blade].normalize();
            let edge = controls[Control::Edge];
            let strain = |turn: f32, controls: &mut grimora_core::Controls| {
                controls[Control::Edge] = Quat::from_axis_angle(blade, turn.to_radians()) * edge;
                let solved = rig.solve(controls);
                solved.wrist.powi(2) + solved.wrist_l.powi(2) + (solved.twist.max(90.0) - 90.0).powi(2)
            };
            let best = (-18..=18).map(|k| k as f32 * 10.0).fold((0.0, f32::MAX), |best, turn| {
                let value = strain(turn, &mut controls);
                if value < best.1 { (turn, value) } else { best }
            });
            if std::env::var("POSES_TURNS").is_ok() {
                let table: Vec<String> = (-6..6)
                    .map(|k| {
                        let turn = k as f32 * 30.0;
                        controls[Control::Edge] = Quat::from_axis_angle(blade, turn.to_radians()) * edge;
                        let solved = rig.solve(&controls);
                        format!("{turn:.0}:{:.0}/{:.0}/{:.0}", solved.wrist, solved.twist, solved.wrist_l)
                    })
                    .collect();
                println!("[turns] {clip_name} {time:.3} {}", table.join(" "));
            }
            controls[Control::Edge] = Quat::from_axis_angle(blade, best.0.to_radians()) * edge;
            let eased = rig.solve(&controls);
            println!(
                "[pose] {clip_name} {time:.3} short {:3.0} mm foot {:3.0} mm wrist {:3.0} twist {:3.0} left {:3.0} {:3.0} | edge {:4.0}: {:3.0} {:3.0} left {:3.0} {:3.0}",
                solved.short_hand * 1000.0,
                solved.short_foot * 1000.0,
                solved.wrist,
                solved.twist,
                solved.wrist_l,
                solved.twist_l,
                best.0,
                eased.wrist,
                eased.twist,
                eased.wrist_l,
                eased.twist_l
            );
        }
    }
}

/// A clip that follows, with its fade and how long until the next change.
type Next = (&'static str, f32, f32);

/// The ways the sandbox goes from one clip to another: what plays and for how long, then what follows.
const SWITCHES: [(&str, &str, f32, &[Next]); 20] = [
    ("stepping to the right", "sword_idle", 0.5, &[("sword_strafe_r", LOCOMOTION, 0.5)]),
    ("stepping to the left", "sword_idle", 0.5, &[("sword_strafe_l", LOCOMOTION, 0.5)]),
    ("stepping back", "sword_idle", 0.5, &[("sword_back", LOCOMOTION, 0.5)]),
    ("from one side to the other", "sword_strafe_l", 0.33, &[("sword_strafe_r", LOCOMOTION, 0.5)]),
    ("raising the guard", "sword_idle", 0.5, &[("sword_guard_1", GUARD_UP, 0.4)]),
    ("raising the guard on a run", "sword_run", 0.33, &[("sword_guard_1", GUARD_UP, 0.4)]),
    ("first blow on the guard", "sword_guard_1", 0.5, &[("sword_block_2", BLOCK, 0.6)]),
    ("second blow on the guard", "sword_guard_2", 0.5, &[("sword_block_1", BLOCK, 0.6)]),
    ("walking out of sword_1", "sword_1", 0.54, &[("sword_walk", OUT_OF_BLOW, 0.5)]),
    ("walking out of sword_2", "sword_2", 0.46, &[("sword_walk", OUT_OF_BLOW, 0.5)]),
    ("running out of sword_3", "sword_3", 0.68, &[("sword_run", OUT_OF_BLOW, 0.5)]),
    (
        "a change of mind out of sword_1",
        "sword_1",
        0.54,
        &[("sword_idle", OUT_OF_BLOW, 0.05), ("sword_walk", LOCOMOTION, 0.05), ("sword_run", LOCOMOTION, 0.5)],
    ),
    ("starting to run", "sword_idle", 0.5, &[("sword_run", LOCOMOTION, 0.5)]),
    ("stopping a run", "sword_run", 0.5, &[("sword_walk", LOCOMOTION, 0.15), ("sword_idle", LOCOMOTION, 0.5)]),
    ("jumping from a run", "sword_run", 0.33, &[("jump", INTO_AIR, 0.3)]),
    ("landing", "jump_fall", 0.3, &[("jump_land", LANDING, 0.3)]),
    ("from a walk into a blow", "sword_walk", 0.5, &[("sword_1", INTO_BLOW, 0.2)]),
    ("from a run into a blow", "sword_run", 0.33, &[("sword_1", INTO_BLOW, 0.2)]),
    ("flinching out of a blow", "sword_1", 0.2, &[("hurt", INTO_BLOW, 0.3)]),
    ("rolling out of a blow", "sword_1", 0.54, &[("sword_roll", INTO_ROLL, 0.3)]),
];
/// The fades the game uses: `animate` and `update_dodge` in the sandbox, `FADE_IN` in combat.
const LOCOMOTION: f32 = 0.2;
const OUT_OF_BLOW: f32 = 0.3;
const INTO_BLOW: f32 = 0.12;
const INTO_ROLL: f32 = 0.1;
const INTO_AIR: f32 = 0.06;
const LANDING: f32 = 0.045;
const GUARD_UP: f32 = 0.16;
const BLOCK: f32 = 0.05;

/// Changing clips can't throw the hand and the blade across the screen in a frame.
#[test]
fn changing_clips_does_not_jump() {
    let mesh = load_model(MODEL.as_ref(), DEFAULT_MAX_TEXTURE_SIZE).unwrap();
    let skeleton = mesh.skeleton.as_ref().unwrap();
    let dt = 1.0 / FPS;
    for (name, first, played, next) in SWITCHES {
        let mut animator = Animator::new(skeleton.clone());
        assert!(animator.replay_once(first, 0.0));
        for _ in 0..(played * FPS).round() as usize {
            animator.update(dt);
        }
        let place = |animator: &Animator| {
            let weapon = animator.joint_transform("weapon.R").unwrap();
            let hand = animator.joint_transform("hand.R").unwrap();
            (hand.w_axis.truncate(), weapon.w_axis.truncate() + weapon.y_axis.truncate().normalize() * TIP)
        };
        let (mut hand_top, mut tip_top) = (0.0f32, 0.0f32);
        let mut last = place(&animator);
        for &(clip, fade, lasts) in next {
            assert!(animator.replay_once(clip, fade), "no clip {clip}");
            for _ in 0..(lasts * FPS).round().max(1.0) as usize {
                animator.update(dt);
                let now = place(&animator);
                hand_top = hand_top.max(now.0.distance(last.0) * FPS);
                tip_top = tip_top.max(now.1.distance(last.1) * FPS);
                last = now;
            }
        }
        println!("[switch] {name}: hand up to {hand_top:.1} m/s, tip up to {tip_top:.1} m/s");
        // No faster than the blows themselves are allowed to go.
        assert!(tip_top < 45.0, "{name}: the tip jumps at {tip_top:.0} m/s");
        assert!(hand_top < 15.0, "{name}: the hand jumps at {hand_top:.0} m/s");
    }
}

/// Going from one gait to another keeps the point of the cycle, so they must step at the same points.
#[test]
fn gaits_step_at_the_same_points() {
    let mesh = load_model(MODEL.as_ref(), DEFAULT_MAX_TEXTURE_SIZE).unwrap();
    let skeleton = mesh.skeleton.as_ref().unwrap();
    for name in ["sword_walk", "sword_run", "sword_back", "sword_strafe_l", "sword_strafe_r"] {
        let clip = &skeleton.clips[skeleton.clip(name).unwrap()];
        let steps: Vec<f32> = clip.events.iter().filter(|e| e.name == "step").map(|e| e.time / clip.duration).collect();
        assert!(steps.len() == 2 && steps[0] < 0.02 && (steps[1] - 0.5).abs() < 0.02, "{name} steps at {steps:?}");
    }
    // Each foot lands at the same point: at its furthest along the way the gait goes.
    let gaits = [
        ("sword_walk", "foot.L", 0.5, Vec3::NEG_Z),
        ("sword_run", "foot.L", 0.5, Vec3::NEG_Z),
        ("sword_back", "foot.L", 0.5, Vec3::Z),
        ("sword_back", "foot.R", 0.0, Vec3::Z),
        ("sword_strafe_r", "foot.R", 0.0, Vec3::X),
        ("sword_strafe_l", "foot.L", 0.5, Vec3::NEG_X),
    ];
    for (name, foot, due, way) in gaits {
        let all = frames(skeleton, &skeleton.clips[skeleton.clip(name).unwrap()]);
        let foot = skeleton.joint(foot).unwrap();
        let along = |k: usize| all[k].position[foot].dot(way);
        let landing = (0..all.len()).fold(0, |best, k| if along(k) > along(best) { k } else { best });
        let at = landing as f32 / (all.len() - 1) as f32;
        let late = (at - due).rem_euclid(1.0).min((due - at).rem_euclid(1.0));
        println!("[gait] {name}: foot lands at {at:.2} of the cycle");
        assert!(late < 0.07, "{name}: the foot lands at {at:.2}, not {due}");
    }
    // A planted foot goes back at the speed the character goes forward, or it slides.
    for (name, speed) in [("sword_walk", 1.2), ("sword_back", 0.95), ("sword_strafe_r", 0.8), ("sword_strafe_l", 0.8)] {
        let all = frames(skeleton, &skeleton.clips[skeleton.clip(name).unwrap()]);
        let mut speeds: Vec<f32> = Vec::new();
        for foot in ["foot.L", "foot.R"].map(|foot| skeleton.joint(foot).unwrap()) {
            let low = all.iter().map(|f| f.position[foot].y).fold(f32::MAX, f32::min) + 0.004;
            for (k, w) in all.windows(2).enumerate() {
                if w[0].position[foot].y < low && w[1].position[foot].y < low {
                    speeds
                        .push(((w[1].position[foot] - w[0].position[foot]) * Vec3::new(1.0, 0.0, 1.0)).length() * FPS);
                }
            }
        }
        speeds.sort_by(f32::total_cmp);
        let middle = speeds[speeds.len() / 2];
        let top = speeds[speeds.len() * 9 / 10];
        println!("[gait] {name}: planted foot at {middle:.2} m/s, up to {top:.2}, written for {speed}");
        assert!((middle - speed).abs() < speed * 0.2, "{name}: the foot slides at {middle:.2} m/s, not {speed}");
    }
}

/// A combo starts its next blow at the last one's `combo` event, so the body must be there already.
#[test]
fn combos_start_where_the_last_blow_is() {
    let mesh = load_model(MODEL.as_ref(), DEFAULT_MAX_TEXTURE_SIZE).unwrap();
    let skeleton = mesh.skeleton.as_ref().unwrap();
    let joint = |name: &str| skeleton.joint(name).unwrap_or_else(|| panic!("no joint {name}"));
    let (root, hand, weapon, foot_l, foot_r) =
        (joint("root"), joint("hand.R"), joint("weapon.R"), joint("foot.L"), joint("foot.R"));
    for (from, to) in COMBO {
        let clip = &skeleton.clips[skeleton.clip(from).unwrap()];
        let last = frames(skeleton, clip);
        let last = &last[((event(clip, "combo") * FPS) as usize).min(last.len() - 1)];
        let next = frames(skeleton, &skeleton.clips[skeleton.clip(to).unwrap()]);
        let next = &next[0];
        let apart =
            |j: usize| (last.position[j] - last.position[root]).distance(next.position[j] - next.position[root]);
        let blade = (last.rotation[weapon] * Vec3::Y).angle_between(next.rotation[weapon] * Vec3::Y).to_degrees();
        let (hands, feet) = (apart(hand), apart(foot_l).max(apart(foot_r)));
        println!(
            "[combo] {from} to {to}: hand {:.0} mm, blade {blade:.0} deg, feet {:.0} mm",
            hands * 1000.0,
            feet * 1000.0
        );
        assert!(hands < 0.1, "{from} to {to}: the hand jumps {:.0} mm", hands * 1000.0);
        assert!(blade < 15.0, "{from} to {to}: the blade jumps {blade:.0} deg");
        assert!(feet < 0.08, "{from} to {to}: a foot jumps {:.0} mm", feet * 1000.0);
    }
}
