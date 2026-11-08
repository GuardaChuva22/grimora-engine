//! The hand on the handle, measured on the skinned mesh: the skin must close around the handle
//! without sinking into it. `cargo test -p grimora_scene --test grip -- --nocapture` prints it.

use glam::{EulerRot, Mat4, Quat, Vec3};
use grimora_core::Animator;
use grimora_scene::{DEFAULT_MAX_TEXTURE_SIZE, load_model};

const MODEL: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/models/chartest/chartest.gltf");
/// The handle of the sword the model holds, at the scale it is held: from the pommel to the guard.
const RADIUS: f32 = 0.0237;
const HANDLE: (f32, f32) = (-0.078, 0.076);

#[test]
fn hand_closes_on_the_handle() {
    let mesh = load_model(MODEL.as_ref(), DEFAULT_MAX_TEXTURE_SIZE).unwrap();
    let skeleton = mesh.skeleton.as_ref().unwrap();
    let hand = skeleton.joint("hand.R").unwrap();
    let weapon = skeleton.joint("weapon.R").unwrap();
    let under_hand = |mut joint: usize| loop {
        if joint == weapon {
            return false;
        }
        if joint == hand {
            return true;
        }
        match skeleton.joints[joint].parent {
            Some(parent) => joint = parent,
            None => return false,
        }
    };

    let clip = skeleton.clips.iter().find(|clip| clip.name == "sword_idle").unwrap();
    let mut pose = skeleton.rest_pose();
    clip.sample(0.5, &mut pose);
    let mut globals = Vec::new();
    skeleton.global_transforms(&pose, &mut globals);
    let skin: Vec<Mat4> = globals.iter().zip(&skeleton.joints).map(|(g, j)| *g * j.inverse_bind).collect();
    let handle = globals[weapon].inverse();

    // Every vertex the hand carries, in the handle's frame: y along it, towards the blade.
    let mut sunk = 0.0f32;
    let mut sunk_count = 0;
    let mut covered = [false; 36];
    let mut per_joint = std::collections::BTreeMap::<String, (f32, usize)>::new();
    for vertex in &mesh.vertices {
        let (k, &w) = vertex.weights.iter().enumerate().max_by_key(|(_, w)| **w).unwrap();
        let joint = vertex.joints[k] as usize;
        if w < 128 || !under_hand(joint) {
            continue;
        }
        let mut at = Vec3::ZERO;
        for (j, w) in vertex.joints.iter().zip(vertex.weights) {
            at += skin[*j as usize].transform_point3(Vec3::from(vertex.position)) * (w as f32 / 255.0);
        }
        let local = handle.transform_point3(at);
        let radial = (local.x * local.x + local.z * local.z).sqrt();
        if local.y < HANDLE.0 || local.y > HANDLE.1 {
            continue;
        }
        let gap = radial - RADIUS;
        if gap < -0.002 {
            sunk = sunk.max(-gap);
            sunk_count += 1;
        }
        if gap < 0.012 {
            let angle = local.z.atan2(local.x).to_degrees().rem_euclid(360.0);
            covered[(angle / 10.0) as usize % 36] = true;
        }
        let entry = per_joint.entry(skeleton.joints[joint].name.clone()).or_insert((f32::MAX, 0));
        entry.0 = entry.0.min(gap);
        entry.1 += 1;
    }
    let around = covered.iter().filter(|c| **c).count() * 10;
    let map: String = covered.iter().map(|c| if *c { '#' } else { '.' }).collect();
    println!("[grip] skin within 12 mm of the handle around {around} degrees: {map}");
    println!("[grip] sunk into the handle: {sunk_count} vertices, deepest {:.1} mm", sunk * 1000.0);
    for (name, (gap, count)) in &per_joint {
        println!("[grip]   {name:20} closest {:+.1} mm ({count} vertices by the handle)", gap * 1000.0);
    }
    // Every clip holds the weapon the same way (`grip::apply_fit`), so one pose speaks for all.
    // These fingers are too short to go all the way round: the thumb closes the rest.
    assert!(around >= 180, "the hand wraps only {around} degrees of the handle");
    assert!(sunk < 0.003, "the skin sinks {:.1} mm into the handle", sunk * 1000.0);
    let palm = per_joint.get("hand.R").map_or(f32::MAX, |(gap, _)| *gap);
    assert!(palm < 0.006, "the palm is {:.1} mm off the handle", palm * 1000.0);
}

/// The stances of the guard, and how they sit on the wrist: see `tests/blows.rs` for how it's read.
#[test]
fn guard_stances_do_not_wring_the_wrist() {
    let mesh = load_model(MODEL.as_ref(), DEFAULT_MAX_TEXTURE_SIZE).unwrap();
    let skeleton = mesh.skeleton.as_ref().unwrap();
    let joint = |name: &str| skeleton.joint(name).unwrap();
    let (forearm, hand) = (joint("forearm.R"), joint("hand.R"));
    let rest_hand = skeleton.rest_pose()[hand].rotation;
    for name in ["sword_guard_1", "sword_guard_2", "sword_guard_3"] {
        let clip = skeleton.clips.iter().find(|clip| clip.name == name).unwrap();
        let mut pose = skeleton.rest_pose();
        clip.sample(0.5, &mut pose);
        let mut globals = Vec::new();
        skeleton.global_transforms(&pose, &mut globals);
        let turn = |j: usize| globals[j].to_scale_rotation_translation().1.normalize();
        let on_forearm = (rest_hand.inverse() * turn(forearm).inverse() * turn(hand)).normalize();
        let about_bone = Quat::from_xyzw(0.0, on_forearm.y, 0.0, on_forearm.w).normalize();
        let (bend, twist) = (
            (on_forearm * about_bone.inverse()).angle_between(Quat::IDENTITY).to_degrees(),
            about_bone.angle_between(Quat::IDENTITY).to_degrees(),
        );
        println!("[guard] {name}: wrist bends {bend:.0} deg, twists {twist:.0} deg");
        assert!(bend < 70.0, "{name}: the wrist bends {bend:.0} deg");
        assert!(twist < 105.0, "{name}: the wrist twists {twist:.0} deg");
    }
}

/// At rest the blade points where the file says, without wringing the arm to get there.
#[test]
fn the_blade_is_carried_where_the_file_says() {
    let mesh = load_model(MODEL.as_ref(), DEFAULT_MAX_TEXTURE_SIZE).unwrap();
    let skeleton = mesh.skeleton.as_ref().unwrap();
    let joint = |name: &str| skeleton.joint(name).unwrap();
    let (forearm, hand, weapon) = (joint("forearm.R"), joint("hand.R"), joint("weapon.R"));
    let aim = Vec3::new(0.3, -0.34, -0.89).normalize();
    let rest_hand = skeleton.rest_pose()[hand].rotation;
    for name in ["sword_idle", "sword_walk", "sword_run"] {
        let clip = skeleton.clips.iter().find(|clip| clip.name == name).unwrap();
        let mut globals = Vec::new();
        let (mut off, mut bent) = (0.0f32, 0.0f32);
        let mut blade = Vec3::ZERO;
        for k in 0..=20 {
            let mut pose = skeleton.rest_pose();
            clip.sample(clip.duration * k as f32 / 20.0, &mut pose);
            skeleton.global_transforms(&pose, &mut globals);
            let turn = |j: usize| globals[j].to_scale_rotation_translation().1.normalize();
            blade = turn(weapon) * Vec3::Y;
            off = off.max(blade.angle_between(aim).to_degrees());
            bent = bent.max(
                (rest_hand.inverse() * turn(forearm).inverse() * turn(hand)).normalize().to_axis_angle().1.to_degrees(),
            );
        }
        println!("[carry] {name}: blade {blade:.2}, up to {off:.0} deg off, hand turned {bent:.0} deg on the forearm");
        if name == "sword_idle" {
            assert!(off < 15.0, "{name}: the blade strays {off:.0} deg from where it's carried");
        }
        assert!(bent < 32.0, "{name}: the wrist bends {bent:.0} deg just to carry the sword");
    }
}

/// Plays `clip` for a moment with the right hand empty: the animator and the joints' places.
fn empty_handed(clip: &str, placed: bool) -> (Animator, Vec<Mat4>) {
    let mesh = load_model(MODEL.as_ref(), DEFAULT_MAX_TEXTURE_SIZE).unwrap();
    let mut animator = Animator::new(mesh.skeleton.clone().unwrap());
    assert!(animator.play(clip, 0.0), "no clip {clip}");
    animator.set_hand_empty(true);
    animator.set_hand_placed(placed);
    animator.update(1.0);
    let mut globals = Vec::new();
    animator.skeleton().global_transforms(animator.pose(), &mut globals);
    (animator, globals)
}

#[test]
fn an_empty_hand_lets_go_and_hangs_in_line_with_the_forearm() {
    for clip in ["sword_idle", "sword_walk", "sword_back", "sword_strafe_l"] {
        let (mut animator, globals) = empty_handed(clip, false);
        let skeleton = animator.skeleton().clone();
        let at = |name: &str| globals[skeleton.joint(name).unwrap()].w_axis.truncate();
        let bend = (at("hand.R") - at("forearm.R")).angle_between(at("fingers.R") - at("hand.R")).to_degrees();
        println!("[empty] {clip}: the wrist bends {bend:.0} deg");
        assert!(bend < 22.0, "{clip}: the empty hand bends {bend:.0} deg at the wrist");

        // Something in it: the fingers close again, the way the clip has them.
        let fingers = skeleton.joint("fingers.R").unwrap();
        let open = animator.pose()[fingers].rotation;
        animator.set_hand_empty(false);
        animator.update(1.0);
        let closed = animator.pose()[fingers].rotation;
        assert!(open.angle_between(closed).to_degrees() > 20.0, "{clip}: the hand doesn't close");
        assert!(closed.angle_between(skeleton.joints[fingers].rest.rotation).to_degrees() < 1.0);
    }
}

#[test]
fn the_reading_hand_lies_on_the_page_with_both_elbows_bent() {
    /// The open book at this scale, on the left hand, from `items.ron`.
    const SCALE: f32 = 1.25;
    /// In the book's own units: the top of the pages, and the right page.
    const PAGE_TOP: f32 = 0.027;
    const PAGE_ACROSS: (f32, f32) = (-0.086, 0.087);
    const PAGE_DEEP: f32 = 0.122;
    let (animator, globals) = empty_handed("book_hold", true);
    let skeleton = animator.skeleton();
    let joint = |name: &str| skeleton.joint(name).unwrap();
    let at = |name: &str| globals[joint(name)].w_axis.truncate();
    for (side, upper, forearm, hand) in
        [("right", "upper_arm.R", "forearm.R", "hand.R"), ("left", "upper_arm.L", "forearm.L", "hand.L")]
    {
        let elbow = (at(upper) - at(forearm)).angle_between(at(hand) - at(forearm)).to_degrees();
        println!("[book] {side} elbow {elbow:.0} deg");
        assert!((70.0..135.0).contains(&elbow), "the {side} elbow is at {elbow:.0} deg");
    }

    let hold = Mat4::from_scale_rotation_translation(
        Vec3::splat(SCALE),
        Quat::from_euler(EulerRot::YXZ, -121.3f32.to_radians(), 74.8f32.to_radians(), 98.2f32.to_radians()),
        Vec3::new(-0.069, 0.133, -0.091),
    );
    let to_book = (globals[joint("hand.L")] * hold).inverse();
    let skin: Vec<Mat4> = globals.iter().zip(&skeleton.joints).map(|(g, j)| *g * j.inverse_bind).collect();
    let (hand, mesh) = (joint("hand.R"), load_model(MODEL.as_ref(), DEFAULT_MAX_TEXTURE_SIZE).unwrap());
    let under_hand = |mut j: usize| loop {
        if j == hand {
            return true;
        }
        match skeleton.joints[j].parent {
            Some(parent) => j = parent,
            None => return false,
        }
    };
    let (mut lowest, mut on_page, mut total) = (f32::INFINITY, 0, 0);
    for vertex in &mesh.vertices {
        let (k, _) = vertex.weights.iter().enumerate().max_by_key(|(_, w)| **w).unwrap();
        if !under_hand(vertex.joints[k] as usize) {
            continue;
        }
        let mut point = Vec3::ZERO;
        for (j, w) in vertex.joints.iter().zip(vertex.weights) {
            point += skin[*j as usize].transform_point3(Vec3::from(vertex.position)) * (w as f32 / 255.0);
        }
        let local = to_book.transform_point3(point);
        total += 1;
        if local.x > PAGE_ACROSS.0 && local.x < PAGE_ACROSS.1 && local.z.abs() < PAGE_DEEP {
            on_page += 1;
            lowest = lowest.min((local.y - PAGE_TOP) * SCALE);
        }
    }
    println!(
        "[book] {on_page} of {total} hand vertices over the right page, the lowest {:.1} mm off it",
        lowest * 1000.0
    );
    assert!(on_page * 2 > total, "the hand isn't over the page: {on_page} of {total}");
    assert!(lowest > -0.006, "the hand sinks {:.1} mm into the page", -lowest * 1000.0);
    assert!(lowest < 0.02, "the hand floats {:.1} mm over the page", lowest * 1000.0);
}
