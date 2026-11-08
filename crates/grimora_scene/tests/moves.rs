//! What the keys don't show about a move, in every frame as it plays, springs included: whether the
//! body takes part in a blow, where it stops dead, and what it asks of the joints.
//! `cargo test --release -p grimora_scene --test moves -- --nocapture` prints a line per clip and
//! judges the great swords. `CLIP=<prefix>` measures others without judging them, and `TRACE=<n>` prints
//! a line every n frames.

use glam::Vec3;
use grimora_core::{Control, PoseClip, PoseSampler, Rig, Skeleton};
use grimora_scene::{DEFAULT_MAX_TEXTURE_SIZE, load_model, load_pose_clips};

const MODEL: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/models/chartest/chartest.gltf");
const FPS: f32 = 60.0;
const JUDGED: [&str; 7] = ["heavy_1", "heavy_2", "heavy_3", "heavy_idle", "heavy_carry", "heavy_guard", "heavy_block"];
/// The great sword's tip, from the weapon joint.
const TIP: f32 = 1.1;
const WRIST: f32 = 75.0;
const TWIST: f32 = 110.0;
const ARM: f32 = 99.0;
/// Of the hand's way through a blow, how much must come from the body, not the arms.
const BODY_SHARE: f32 = 0.35;
/// A part slower than this has stopped dead, in degrees per second.
const DEAD_TURN: f32 = 12.0;
/// Frames the hips or the chest may stop dead between `swing` and `hit_off`.
const DEAD_FRAMES: usize = 2;
/// Wind-up seconds per second of the swing itself. Low on purpose: quick and snappy is the look
/// wanted, and a long wind-up is what made the big swords read as slow.
const CONTRAST: f32 = 1.3;
/// Seconds the tip keeps moving after the blade stops hitting: what weight looks like.
const FOLLOW_THROUGH: f32 = 0.12;
/// How big a blow must be, from the one the user turned down as "arms glued together, the sword
/// always in front": arms covering 0.30-0.44 m seen from the chest, the blade in front 65% of the clip.
/// The great swords are held in both hands, so the hands never get farther apart than this, meters.
const TWO_HANDS: f32 = 0.3;
/// Meters the right and left hand must cover seen from the chest.
const COVER_RIGHT: f32 = 0.6;
const COVER_LEFT: f32 = 0.6;
/// Degrees between the blade's two farthest directions.
const BLADE_SWEEP: f32 = 150.0;
/// The most of a clip the blade may spend pointing to the front.
const IN_FRONT: f32 = 0.5;
/// A loop's chest and hand must move this far from the hips, in millimeters.
const ALIVE: f32 = 8.0;

#[derive(Clone, Copy)]
struct Frame {
    time: f32,
    hips: Vec3,
    hips_turn: glam::Quat,
    chest: Vec3,
    chest_turn: glam::Quat,
    hand: Vec3,
    hand_l: Vec3,
    blade: Vec3,
    tip: Vec3,
}

#[derive(Default)]
struct Report {
    short: f32,
    short_foot: f32,
    arm_r: f32,
    arm_l: f32,
    wrist: (f32, f32),
    twist: (f32, f32),
    wrist_l: (f32, f32),
    twist_l: (f32, f32),
    lowest_tip: f32,
    blow: Option<Blow>,
    chest_travel: f32,
    hand_travel: f32,
    reach: Reach,
}

/// How big a move is: what the eye reads as stiff or free, whatever the joints say.
#[derive(Default)]
struct Reach {
    /// Meters, the most the hands get apart.
    apart: f32,
    /// Meters the right and left hand cover as seen from the chest: the arms' own move.
    right: f32,
    left: f32,
    /// Degrees, the widest two directions of the blade in the clip.
    blade: f32,
    /// Of the clip's frames, how many have the blade pointing to the front.
    front: f32,
    /// Meters the tip covers, each axis.
    tip: Vec3,
}

struct Blow {
    wind_up: f32,
    swing: f32,
    tip_up: f32,
    tip_cut: f32,
    body_share: f32,
    dead_hips: usize,
    dead_chest: usize,
    follow: f32,
    kink: f32,
}

fn top(seen: &mut (f32, f32), value: f32, time: f32) {
    if value > seen.0 {
        *seen = (value, time);
    }
}

fn measure(skeleton: &Skeleton, rig: &Rig, clip: &PoseClip) -> Report {
    let joint = |name: &str| skeleton.joint(name).unwrap_or_else(|| panic!("no joint {name}"));
    let (hips, chest, weapon) = (joint("hips"), joint("chest"), joint("weapon.R"));
    let (upper_r, fore_r, hand_r) = (joint("upper_arm.R"), joint("forearm.R"), joint("hand.R"));
    let (upper_l, fore_l, hand_l) = (joint("upper_arm.L"), joint("forearm.L"), joint("hand.L"));
    let sampler = PoseSampler::new(clip);
    let mut report = Report { lowest_tip: f32::MAX, ..Default::default() };
    let mut frames = Vec::new();
    for k in 0..=(clip.duration * FPS).round() as usize {
        let time = k as f32 / FPS;
        let controls = sampler.sample(time);
        let solved = rig.solve(&controls);
        let (p, turns) = (&solved.positions, &solved.turns);
        let stretch = |upper: usize, fore: usize, hand: usize| {
            p[upper].distance(p[hand]) / (p[upper].distance(p[fore]) + p[fore].distance(p[hand])) * 100.0
        };
        report.short = report.short.max(solved.short_hand * 1000.0);
        report.short_foot = report.short_foot.max(solved.short_foot * 1000.0);
        report.arm_r = report.arm_r.max(stretch(upper_r, fore_r, hand_r));
        report.arm_l = report.arm_l.max(stretch(upper_l, fore_l, hand_l));
        if std::env::var("TRACE").is_ok_and(|every| k % every.parse::<usize>().unwrap_or(3) == 0) {
            println!(
                "  {time:.2} short {:3.0} foot {:3.0} arm {:3.0} {:3.0} wrist {:3.0} twist {:3.0} left {:3.0} {:3.0}",
                solved.short_hand * 1000.0,
                solved.short_foot * 1000.0,
                stretch(upper_r, fore_r, hand_r),
                stretch(upper_l, fore_l, hand_l),
                solved.wrist,
                solved.twist,
                solved.wrist_l,
                solved.twist_l
            );
        }
        top(&mut report.wrist, solved.wrist, time);
        top(&mut report.twist, solved.twist, time);
        top(&mut report.wrist_l, solved.wrist_l, time);
        top(&mut report.twist_l, solved.twist_l, time);
        let root = Vec3::new(controls[Control::Root].x, 0.0, controls[Control::Root].z);
        let tip = p[weapon] + controls[Control::Blade].normalize() * TIP;
        report.lowest_tip = report.lowest_tip.min(tip.y);
        frames.push(Frame {
            time,
            hips: p[hips] + root,
            hips_turn: turns[hips],
            chest: p[chest] + root,
            chest_turn: turns[chest],
            hand: p[hand_r] + root,
            hand_l: p[hand_l] + root,
            blade: controls[Control::Blade].normalize(),
            tip: tip + root,
        });
    }

    let extent = |pick: &dyn Fn(&Frame) -> Vec3| {
        let (mut low, mut high) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for frame in &frames {
            let point = pick(frame);
            low = low.min(point);
            high = high.max(point);
        }
        (high - low).length() * 1000.0
    };
    report.chest_travel = extent(&|f| f.chest - f.hips);
    report.hand_travel = extent(&|f| f.hand - f.hips);
    let seen_from_chest = |f: &Frame, at: Vec3| f.chest_turn.inverse() * (at - f.chest);
    let box_of = |pick: &dyn Fn(&Frame) -> Vec3| {
        let (mut low, mut high) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for frame in &frames {
            let point = pick(frame);
            low = low.min(point);
            high = high.max(point);
        }
        high - low
    };
    let mut widest = 0.0f32;
    for a in &frames {
        for b in &frames {
            widest = widest.max(a.blade.angle_between(b.blade).to_degrees());
        }
    }
    report.reach = Reach {
        apart: frames.iter().map(|f| f.hand.distance(f.hand_l)).fold(0.0, f32::max),
        right: extent(&|f| seen_from_chest(f, f.hand)) / 1000.0,
        left: extent(&|f| seen_from_chest(f, f.hand_l)) / 1000.0,
        blade: widest,
        front: frames.iter().filter(|f| f.blade.z < -0.3).count() as f32 / frames.len() as f32,
        tip: box_of(&|f| f.tip),
    };

    let event = |name: &str| clip.events.iter().find(|(_, n)| n == name).map(|(t, _)| *t);
    if let (Some(hit_on), Some(hit_off)) = (event("hit_on"), event("hit_off")) {
        let swing = event("swing").unwrap_or(hit_on);
        let during: Vec<&[Frame]> = frames.windows(2).filter(|w| w[0].time >= swing && w[1].time <= hit_off).collect();
        let speed = |w: &[Frame]| w[0].tip.distance(w[1].tip) * FPS;
        let tip_cut = during.iter().map(|w| speed(w)).fold(0.0, f32::max);
        let tip_up = frames.windows(2).filter(|w| w[1].time <= swing).map(speed).fold(0.0, f32::max);
        // The hand's way, and what's left of it as seen from the chest.
        let (mut world, mut arms) = (0.0, 0.0);
        for w in &during {
            world += w[0].hand.distance(w[1].hand);
            let seen = |f: &Frame| f.chest_turn.inverse() * (f.hand - f.chest);
            arms += seen(&w[0]).distance(seen(&w[1]));
        }
        let turning = |a: glam::Quat, b: glam::Quat| a.angle_between(b).to_degrees() * FPS;
        let dead_hips = during.iter().filter(|w| turning(w[0].hips_turn, w[1].hips_turn) < DEAD_TURN).count();
        let dead_chest = during.iter().filter(|w| turning(w[0].chest_turn, w[1].chest_turn) < DEAD_TURN).count();
        let settled =
            frames.windows(2).find(|w| w[0].time > hit_off && speed(w) < 0.5).map_or(clip.duration, |w| w[0].time);
        let speeds: Vec<f32> = frames.windows(2).map(speed).collect();
        let kink = speeds.windows(2).map(|s| (s[1] - s[0]).abs()).fold(0.0, f32::max);
        report.blow = Some(Blow {
            wind_up: swing,
            swing: hit_off - swing,
            tip_up,
            tip_cut,
            body_share: 1.0 - arms / f32::max(world, 1e-3),
            dead_hips,
            dead_chest,
            follow: settled - hit_off,
            kink,
        });
    }
    report
}

fn print(name: &str, r: &Report) {
    print!(
        "[move] {name:14} short {:3.0} foot {:3.0} arm {:3.0} {:3.0} | wrist {:3.0}@{:.2} twist {:3.0}@{:.2} left {:3.0}@{:.2} {:3.0}@{:.2} | tip low {:5.2}",
        r.short,
        r.short_foot,
        r.arm_r,
        r.arm_l,
        r.wrist.0,
        r.wrist.1,
        r.twist.0,
        r.twist.1,
        r.wrist_l.0,
        r.wrist_l.1,
        r.twist_l.0,
        r.twist_l.1,
        r.lowest_tip
    );
    match &r.blow {
        Some(b) => println!(
            " | wind-up {:.2} swing {:.2} ({:.1}x) tip {:4.1}/{:4.1} m/s | body {:3.0}% dead hips {:2} chest {:2} | follow {:.2} s kink {:4.1}",
            b.wind_up,
            b.swing,
            b.wind_up / b.swing,
            b.tip_up,
            b.tip_cut,
            b.body_share * 100.0,
            b.dead_hips,
            b.dead_chest,
            b.follow,
            b.kink
        ),
        None => println!(" | moves chest {:3.0} hand {:3.0} mm", r.chest_travel, r.hand_travel),
    }
    let a = &r.reach;
    println!(
        "       reach: hands apart {:.2} m, arms cover {:.2}/{:.2} m, blade {:3.0} deg, in front {:3.0}%, tip box {:.1} {:.1} {:.1} m",
        a.apart,
        a.right,
        a.left,
        a.blade,
        a.front * 100.0,
        a.tip.x,
        a.tip.y,
        a.tip.z
    );
}

#[test]
fn great_swords_move_with_the_body() {
    let mesh = load_model(MODEL.as_ref(), DEFAULT_MAX_TEXTURE_SIZE).unwrap();
    let skeleton = mesh.skeleton.as_ref().unwrap();
    let rig = Rig::new(skeleton).unwrap();
    let draft = std::env::var("CLIP").ok();
    let mut clips = load_pose_clips(MODEL.as_ref());
    clips.sort_by(|a, b| a.0.cmp(&b.0));
    let mut wrong = Vec::new();
    for (name, clip) in &clips {
        let judged = JUDGED.contains(&name.as_str());
        if !draft.as_ref().map_or(judged, |prefix| name.starts_with(prefix)) {
            continue;
        }
        let r = measure(skeleton, &rig, clip);
        print(name, &r);
        if draft.is_some() {
            continue;
        }
        let mut check = |ok: bool, what: String| {
            if !ok {
                wrong.push(format!("{name}: {what}"));
            }
        };
        check(r.short < 1.0, format!("an arm is {:.0} mm short", r.short));
        check(r.short_foot < 1.0, format!("a leg is {:.0} mm short", r.short_foot));
        check(r.arm_r < ARM, format!("the right elbow locks ({:.0})", r.arm_r));
        check(r.arm_l < ARM, format!("the left elbow locks ({:.0})", r.arm_l));
        check(r.wrist.0 < WRIST, format!("the right wrist bends {:.0} at {:.2}", r.wrist.0, r.wrist.1));
        check(r.wrist_l.0 < WRIST, format!("the left wrist bends {:.0} at {:.2}", r.wrist_l.0, r.wrist_l.1));
        check(r.twist.0 < TWIST, format!("the right wrist twists {:.0} at {:.2}", r.twist.0, r.twist.1));
        check(r.twist_l.0 < TWIST, format!("the left wrist twists {:.0} at {:.2}", r.twist_l.0, r.twist_l.1));
        if let Some(b) = &r.blow {
            check(
                b.body_share > BODY_SHARE,
                format!("only {:.0}% of the hand's way comes from the body", b.body_share * 100.0),
            );
            check(b.dead_hips <= DEAD_FRAMES, format!("the hips stop dead for {} frames of the blow", b.dead_hips));
            check(b.dead_chest <= DEAD_FRAMES, format!("the chest stops dead for {} frames of the blow", b.dead_chest));
            check(b.wind_up / b.swing > CONTRAST, format!("wind-up only {:.1}x the swing", b.wind_up / b.swing));
            check(b.follow > FOLLOW_THROUGH, format!("the tip stops {:.2} s after the blow", b.follow));
            let a = &r.reach;
            check(a.apart < TWO_HANDS, format!("the left hand lets go of the handle, {:.2} m away", a.apart));
            check(
                a.right > COVER_RIGHT && a.left > COVER_LEFT,
                format!("the arms stay glued, they cover {:.2}/{:.2} m", a.right, a.left),
            );
            check(a.blade > BLADE_SWEEP, format!("the blade only sweeps {:.0} degrees", a.blade));
            check(a.front < IN_FRONT, format!("the blade is in front {:.0}% of the clip", a.front * 100.0));
        } else if clip.looping {
            check(r.chest_travel > ALIVE, format!("a statue: the chest moves {:.0} mm", r.chest_travel));
            check(r.hand_travel > ALIVE, format!("a statue: the hand moves {:.0} mm", r.hand_travel));
        }
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}
