//! The guns' clips in every frame, as the game plays them: over the upper body of `gun_stand`.
//! `cargo test --release -p grimora_scene --test gun_clips -- --nocapture` prints one line per clip
//! and fails on an arm that can't reach or a joint past what a body does. `CLIP=<prefix>` measures
//! other clips without judging them.

use glam::{EulerRot, Mat4, Quat, Vec3};
use grimora_core::{Animator, Control, PoseClip, PoseSampler, Rig, Skeleton};
use grimora_scene::{DEFAULT_MAX_TEXTURE_SIZE, load_model, load_pose_clips};
use std::sync::Arc;

const MODEL: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/models/chartest/chartest.gltf");
const FPS: f32 = 60.0;
const CLIPS: [&str; 8] = [
    "pistol_hold",
    "pistol_aim",
    "pistol_fire",
    "pistol_reload",
    "musket_hold",
    "musket_aim",
    "musket_fire",
    "musket_reload",
];
const BASE: &str = "gun_stand";
const UPPER_BODY: &str = "spine";
/// Of `references/body.md`, with what a hand on a gun allows.
const WRIST: f32 = 70.0;
/// The free hand, shoving a rod or holding on, as a second hand on a sword may.
const WRIST_LEFT: f32 = 75.0;
/// A long gun at the shoulder asks for a little more than a sword does.
const TWIST: f32 = 112.0;
/// A palm turned up under a stock is most of what a forearm can turn, but a forearm can.
const TWIST_LEFT: f32 = 145.0;
/// How straight an arm may get, of its full length; at 100 the elbow locks.
const ARM: f32 = 98.5;
/// How far an idle's chest and gun must travel from the hips in a cycle, in millimeters, or it's a
/// statue on top of the legs. An aim is meant to stay on target.
const ALIVE: f32 = 8.0;
const ALIVE_AIMING: f32 = 3.0;
/// How far the free hand may slide along a gun it holds, in millimeters.
const GRIP_SLIDE: f32 = 12.0;
/// Where a musket's butt is, in the model, and how `items.ron` holds it while loading.
const BUTT: Vec3 = Vec3::new(-0.4, -0.07, 0.0);
const GROUNDED_AT: Vec3 = Vec3::new(0.0, -0.72, 0.03);
const GROUNDED_TURN: Vec3 = Vec3::new(0.0, -90.0, 90.0);

#[derive(Default)]
struct Top {
    value: f32,
    at: f32,
}

impl Top {
    fn see(&mut self, value: f32, at: f32) {
        if value > self.value {
            *self = Top { value, at };
        }
    }
}

struct Report {
    short: Top,
    arm_r: Top,
    arm_l: Top,
    wrist: Top,
    twist: Top,
    wrist_l: Top,
    twist_l: Top,
    chest_travel: f32,
    gun_travel: f32,
    grip_slide: f32,
    gun_speed: Top,
    left_speed: Top,
    butt: (f32, f32),
    butt_slide: f32,
}

fn at(matrix: Mat4) -> Vec3 {
    matrix.w_axis.truncate()
}

fn extent(points: &[Vec3]) -> f32 {
    let (mut low, mut high) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for point in points {
        low = low.min(*point);
        high = high.max(*point);
    }
    if points.is_empty() { 0.0 } else { (high - low).length() * 1000.0 }
}

fn measure(skeleton: &Arc<Skeleton>, rig: &Rig, name: &str, clip: &PoseClip) -> Report {
    let joint = |joint: &str| skeleton.joint(joint).unwrap_or_else(|| panic!("no joint {joint}"));
    let (upper_r, fore_r, hand_r) = (joint("upper_arm.R"), joint("forearm.R"), joint("hand.R"));
    let (upper_l, fore_l, hand_l) = (joint("upper_arm.L"), joint("forearm.L"), joint("hand.L"));
    let mut report = Report {
        short: Top::default(),
        arm_r: Top::default(),
        arm_l: Top::default(),
        wrist: Top::default(),
        twist: Top::default(),
        wrist_l: Top::default(),
        twist_l: Top::default(),
        chest_travel: 0.0,
        gun_travel: 0.0,
        grip_slide: 0.0,
        gun_speed: Top::default(),
        left_speed: Top::default(),
        butt: (f32::MAX, f32::MIN),
        butt_slide: 0.0,
    };

    let sampler = PoseSampler::new(clip);
    let frames = (clip.duration * FPS).round() as usize;
    for k in 0..=frames {
        let time = k as f32 / FPS;
        let solved = rig.solve(&sampler.sample(time));
        let p = &solved.positions;
        let stretch = |upper: usize, fore: usize, hand: usize| {
            let full = p[upper].distance(p[fore]) + p[fore].distance(p[hand]);
            p[upper].distance(p[hand]) / full * 100.0
        };
        report.short.see(solved.short_hand * 1000.0, time);
        report.arm_r.see(stretch(upper_r, fore_r, hand_r), time);
        report.arm_l.see(stretch(upper_l, fore_l, hand_l), time);
        report.wrist.see(solved.wrist, time);
        report.twist.see(solved.twist, time);
        report.wrist_l.see(solved.wrist_l, time);
        report.twist_l.see(solved.twist_l, time);
        if let Some(every) = std::env::var("TRACE").ok().and_then(|n| n.parse::<usize>().ok())
            && k % every.max(1) == 0
        {
            println!(
                "  {time:.2} wrist {:3.0} twist {:3.0} | left {:3.0} {:3.0} | hand_l {:5.2} {:5.2} {:5.2} elbow_l {:5.2} {:5.2} {:5.2}",
                solved.wrist,
                solved.twist,
                solved.wrist_l,
                solved.twist_l,
                p[hand_l].x,
                p[hand_l].y,
                p[hand_l].z,
                p[fore_l].x,
                p[fore_l].y,
                p[fore_l].z
            );
        }
    }

    // As the game plays it, on the idle's legs. Steady whenever it aims or loads, as `guns.rs` does.
    let mut animator = Animator::new(skeleton.clone());
    assert!(animator.play(BASE, 0.0), "no clip {BASE}");
    assert!(if clip.looping {
        animator.overlay_loop(name, UPPER_BODY, 0.001)
    } else {
        animator.overlay_once(name, UPPER_BODY, 0.001)
    });
    animator.set_overlay_steady(if name.ends_with("_hold") { 0.0 } else { 1.0 });
    let dt = 1.0 / FPS;
    animator.update(dt);
    animator.update(dt);
    let grounded = Mat4::from_rotation_translation(
        Quat::from_euler(
            EulerRot::YXZ,
            GROUNDED_TURN.y.to_radians(),
            GROUNDED_TURN.x.to_radians(),
            GROUNDED_TURN.z.to_radians(),
        ),
        GROUNDED_AT,
    );
    let (mut chest, mut gun, mut slide, mut butts) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut last = None;
    for k in 0..frames {
        animator.update(dt);
        let time = k as f32 / FPS;
        let weapon = animator.joint_transform("weapon.R").unwrap();
        let hand = at(animator.joint_transform("hand.L").unwrap());
        let hips = at(animator.joint_transform("hips").unwrap());
        chest.push(at(animator.joint_transform("chest").unwrap()) - hips);
        gun.push(at(weapon) - hips);
        let (_, rotation, translation) = weapon.to_scale_rotation_translation();
        slide.push(Mat4::from_rotation_translation(rotation, translation).inverse().transform_point3(hand));
        if let Some((gun_was, hand_was)) = last {
            report.gun_speed.see(at(weapon).distance(gun_was) * FPS, time);
            report.left_speed.see(hand.distance(hand_was) * FPS, time);
        }
        last = Some((at(weapon), hand));
        // Only where the barrel stands on the ground.
        if clip.tracks.contains_key(&Control::HandR) && name == "musket_reload" && (0.6..5.0).contains(&time) {
            let butt = Mat4::from_rotation_translation(rotation, translation) * grounded;
            let point = butt.transform_point3(BUTT);
            let height = point.y;
            butts.push(Vec3::new(point.x, 0.0, point.z));
            report.butt = (report.butt.0.min(height), report.butt.1.max(height));
        }
    }
    report.chest_travel = extent(&chest);
    report.gun_travel = extent(&gun);
    report.grip_slide = extent(&slide);
    report.butt_slide = extent(&butts);
    report
}

fn print(name: &str, r: &Report) {
    println!(
        "[clip] {name:14} short {:3.0}@{:.2} arm {:3.0}@{:.2} {:3.0}@{:.2} | wrist {:3.0}@{:.2} twist {:3.0}@{:.2} | left {:3.0}@{:.2} {:3.0}@{:.2} | moves chest {:4.0} gun {:4.0} mm, left on gun {:4.0} mm | speed gun {:4.1}@{:.2} left {:4.1}@{:.2}{}",
        r.short.value,
        r.short.at,
        r.arm_r.value,
        r.arm_r.at,
        r.arm_l.value,
        r.arm_l.at,
        r.wrist.value,
        r.wrist.at,
        r.twist.value,
        r.twist.at,
        r.wrist_l.value,
        r.wrist_l.at,
        r.twist_l.value,
        r.twist_l.at,
        r.chest_travel,
        r.gun_travel,
        r.grip_slide,
        r.gun_speed.value,
        r.gun_speed.at,
        r.left_speed.value,
        r.left_speed.at,
        if r.butt.0 < f32::MAX {
            format!(" | butt {:.3}..{:.3} m, slides {:.0} mm", r.butt.0, r.butt.1, r.butt_slide)
        } else {
            String::new()
        },
    );
}

#[test]
fn gun_clips_fit_the_body() {
    let mesh = load_model(MODEL.as_ref(), DEFAULT_MAX_TEXTURE_SIZE).unwrap();
    let skeleton = mesh.skeleton.as_ref().unwrap();
    let rig = Rig::new(skeleton).unwrap();
    let clips = load_pose_clips(MODEL.as_ref());
    let draft = std::env::var("CLIP").ok();
    let mut wrong = Vec::new();
    for (name, clip) in &clips {
        let listed = CLIPS.contains(&name.as_str());
        let wanted = draft.as_ref().map_or(listed, |prefix| name.starts_with(prefix));
        if !wanted {
            continue;
        }
        let r = measure(skeleton, &rig, name, clip);
        print(name, &r);
        if draft.is_some() {
            continue;
        }
        let mut check = |ok: bool, what: String| {
            if !ok {
                wrong.push(format!("{name}: {what}"));
            }
        };
        check(r.short.value < 1.0, format!("an arm is {:.0} mm short at {:.2}", r.short.value, r.short.at));
        check(r.arm_r.value < ARM, format!("the right elbow locks at {:.2}", r.arm_r.at));
        check(r.arm_l.value < ARM, format!("the left elbow locks at {:.2}", r.arm_l.at));
        check(r.wrist.value < WRIST, format!("the right wrist bends {:.0} at {:.2}", r.wrist.value, r.wrist.at));
        check(r.twist.value < TWIST, format!("the right wrist twists {:.0} at {:.2}", r.twist.value, r.twist.at));
        check(
            r.wrist_l.value < WRIST_LEFT,
            format!("the left wrist bends {:.0} at {:.2}", r.wrist_l.value, r.wrist_l.at),
        );
        check(
            r.twist_l.value < TWIST_LEFT,
            format!("the left wrist twists {:.0} at {:.2}", r.twist_l.value, r.twist_l.at),
        );
        if clip.looping {
            let alive = if name.ends_with("_aim") { ALIVE_AIMING } else { ALIVE };
            check(r.chest_travel > alive, format!("a statue: the chest moves {:.0} mm", r.chest_travel));
            check(r.gun_travel > alive, format!("a statue: the gun moves {:.0} mm", r.gun_travel));
        }
        if name.starts_with("musket_") && !name.ends_with("reload") {
            check(r.grip_slide < GRIP_SLIDE, format!("the left hand slides {:.0} mm on the gun", r.grip_slide));
        }
        if r.butt.0 < f32::MAX {
            check(
                r.butt.0 > -0.02 && r.butt.1 < 0.06,
                format!("the butt is {:.3}..{:.3} m off the ground", r.butt.0, r.butt.1),
            );
        }
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}
