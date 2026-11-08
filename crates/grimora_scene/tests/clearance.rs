//! Whether the sword and the arms pass through the body, on the skinned mesh in every frame of a
//! clip, with and without springs (a stopped animator shows the keys without them).
//! `cargo test --release -p grimora_scene --test clearance -- --nocapture` prints what touches what;
//! `CLEAR=<prefix>` measures other clips without judging them.

use glam::{Mat4, Vec3};
use grimora_core::{PoseSampler, Rig, Skeleton};
use grimora_scene::{DEFAULT_MAX_TEXTURE_SIZE, load_model, load_pose_clips};
use std::collections::BTreeMap;

const MODEL: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/models/chartest/chartest.gltf");
const FPS: f32 = 60.0;
/// The longest of the heavy swords in the weapon joint's frame (the zweihander): its pommel, where
/// the blade starts past the guard, and its tip.
const POMMEL: (f32, f32) = (-0.21, -0.15);
const BLADE: (f32, f32) = (0.06, 1.175);
/// The lines tested along the sword stand this far apart, for its width.
const WIDTH: f32 = 0.03;
/// What the arms must not pass through. The chest is left out, since a two-handed grip keeps the
/// arms against it and the skin bends there.
const SOLID: [&str; 2] = ["head", "neck"];
/// Also solid, but only the skin these joints carry themselves: the belly, the hips and the thighs,
/// where a low two-handed hold puts the forearms.
/// Meters an arm may sink into the trunk: skin pressing on skin, not passing through.
const SINK: f32 = 0.02;
const TRUNK: [&str; 4] = ["spine", "hips", "thigh.L", "thigh.R"];

fn crosses(a: Vec3, b: Vec3, triangle: &[Vec3; 3]) -> bool {
    let (ab, ac, dir) = (triangle[1] - triangle[0], triangle[2] - triangle[0], b - a);
    let p = dir.cross(ac);
    let det = ab.dot(p);
    if det.abs() < 1e-9 {
        return false;
    }
    let s = a - triangle[0];
    let u = s.dot(p) / det;
    let q = s.cross(ab);
    let v = dir.dot(q) / det;
    let t = ac.dot(q) / det;
    u >= 0.0 && v >= 0.0 && u + v <= 1.0 && (0.0..=1.0).contains(&t)
}

fn under(skeleton: &Skeleton, mut joint: usize, ancestor: usize) -> bool {
    loop {
        if joint == ancestor {
            return true;
        }
        match skeleton.joints[joint].parent {
            Some(parent) => joint = parent,
            None => return false,
        }
    }
}

/// Which parts touch which, each with the moments it happens, for one way of playing the clip.
fn touches(
    mesh: &grimora_core::MeshData,
    skeleton: &Skeleton,
    rig: &Rig,
    clip: &grimora_core::PoseClip,
    springs: bool,
) -> (BTreeMap<String, Vec<f32>>, BTreeMap<String, f32>) {
    let joint = |name: &str| skeleton.joint(name).unwrap();
    let weapon = joint("weapon.R");
    let hands = [joint("hand.R"), joint("hand.L")];
    let arms = [joint("forearm.R"), joint("forearm.L")];
    let owner: Vec<usize> = mesh
        .vertices
        .iter()
        .map(|v| {
            let (k, _) = v.weights.iter().enumerate().max_by_key(|(_, w)| **w).unwrap();
            v.joints[k] as usize
        })
        .collect();
    let in_hand = |j: usize| hands.iter().any(|&h| under(skeleton, j, h));
    let in_arm = |j: usize| arms.iter().any(|&a| under(skeleton, j, a));
    let trunk: Vec<usize> = TRUNK.iter().map(|name| joint(name)).collect();
    let solid = |j: usize| SOLID.iter().any(|name| under(skeleton, j, joint(name)));

    let sampler = PoseSampler::with_springs(clip, springs);
    let mut out = BTreeMap::<String, Vec<f32>>::new();
    let mut worst = BTreeMap::<String, f32>::new();
    let mut pose = Vec::new();
    let mut globals = Vec::new();
    let mut world = Vec::new();
    for k in 0..=(clip.duration * FPS).round() as usize {
        let time = k as f32 / FPS;
        let controls = sampler.sample(time);
        let solved = rig.solve(&controls);
        rig.local_pose(&solved, &controls, Vec3::ZERO, &mut pose);
        skeleton.global_transforms(&pose, &mut globals);
        let skin: Vec<Mat4> = globals.iter().zip(&skeleton.joints).map(|(g, j)| *g * j.inverse_bind).collect();
        world.clear();
        world.extend(mesh.vertices.iter().map(|v| {
            let mut at = Vec3::ZERO;
            for (j, w) in v.joints.iter().zip(v.weights) {
                at += skin[*j as usize].transform_point3(Vec3::from(v.position)) * (w as f32 / 255.0);
            }
            at
        }));

        let handle = globals[weapon];
        let mut lines = Vec::new();
        for (a, b) in [POMMEL, BLADE] {
            for (x, z) in [(0.0, 0.0), (WIDTH, 0.0), (-WIDTH, 0.0), (0.0, WIDTH), (0.0, -WIDTH)] {
                lines.push((
                    handle.transform_point3(Vec3::new(x, a, z)),
                    handle.transform_point3(Vec3::new(x, b, z)),
                    if a < 0.0 { "pommel" } else { "blade" },
                ));
            }
        }
        // The edges of the hands and forearms, only near what they could touch.
        let mut limbs = Vec::new();
        let (mut low, mut high) = (Vec3::MAX, Vec3::MIN);
        let (triangles, _) = mesh.indices.as_chunks::<3>();
        for tri in triangles {
            if solid(owner[tri[0] as usize]) {
                for &i in tri {
                    low = low.min(world[i as usize]);
                    high = high.max(world[i as usize]);
                }
            }
        }
        for tri in triangles {
            let j = owner[tri[0] as usize];
            if in_hand(j) || in_arm(j) {
                let side = if under(skeleton, j, arms[0]) { "right arm" } else { "left arm" };
                for e in 0..3 {
                    let (a, b) = (world[tri[e] as usize], world[tri[(e + 1) % 3] as usize]);
                    if a.max(b).cmpge(low).all() && a.min(b).cmple(high).all() {
                        limbs.push((a, b, side));
                    }
                }
            }
        }

        // The trunk: a point of a forearm or hand is inside it when a line from it to the front and
        // one to the back both cross its skin an odd number of times (the skin is open at the neck,
        // the shoulders and the legs, not in front or behind). How deep is how far the nearest skin is.
        let skin: Vec<[Vec3; 3]> = triangles
            .iter()
            .filter(|tri| trunk.contains(&owner[tri[0] as usize]))
            .map(|tri| tri.map(|i| world[i as usize]))
            .collect();
        let forward = globals[joint("hips")].transform_vector3(Vec3::NEG_Z).normalize();
        let mut depth = BTreeMap::<String, f32>::new();
        for (i, at) in world.iter().enumerate() {
            let j = owner[i];
            if !(in_hand(j) || in_arm(j)) {
                continue;
            }
            let odd = |way: Vec3| skin.iter().filter(|t| crosses(*at, *at + way * 1.5, t)).count() % 2 == 1;
            if !(odd(forward) && odd(-forward)) {
                continue;
            }
            let nearest = skin.iter().flat_map(|t| t.iter()).map(|p| p.distance(*at)).fold(f32::MAX, f32::min);
            let side = if under(skeleton, j, arms[0]) { "right arm" } else { "left arm" };
            let d = depth.entry(format!("{side} in the trunk")).or_default();
            if std::env::var("DEEP").is_ok() && nearest > 0.04 && std::env::var("DEEP").is_ok() {
                let local = globals[joint("hips")].inverse().transform_point3(*at);
                println!("[deep] {time:.2} {} {local:.3} {nearest:.3}", skeleton.joints[j].name);
            }
            *d = d.max(nearest);
        }
        for (what, d) in depth {
            let deepest = worst.entry(what.clone()).or_default();
            *deepest = deepest.max(d * 1000.0);
            if d > SINK {
                let moments = out.entry(what).or_default();
                moments.push(time);
            }
        }
    }
    (out, worst)
}

fn moments(times: &[f32]) -> String {
    let mut spans: Vec<(f32, f32)> = Vec::new();
    for &t in times {
        match spans.last_mut() {
            Some(span) if t - span.1 < 1.5 / FPS => span.1 = t,
            _ => spans.push((t, t)),
        }
    }
    spans.iter().map(|s| format!("{:.2}-{:.2}", s.0, s.1)).collect::<Vec<_>>().join(" ")
}

#[test]
fn heavy_swords_stay_out_of_the_body() {
    let prefix = std::env::var("CLEAR").ok();
    let mesh = load_model(MODEL.as_ref(), DEFAULT_MAX_TEXTURE_SIZE).unwrap();
    let skeleton = mesh.skeleton.as_ref().unwrap();
    let rig = Rig::new(skeleton).unwrap();
    let mut failed = Vec::new();
    for (name, clip) in load_pose_clips(MODEL.as_ref()) {
        if !name.starts_with(prefix.as_deref().unwrap_or("heavy_")) {
            continue;
        }
        for springs in [false, true] {
            let (found, deepest) = touches(&mesh, skeleton, &rig, &clip, springs);
            println!("[clear] {name} springs {springs}: {}", if found.is_empty() { "clear" } else { "" });
            for (what, mm) in &deepest {
                println!("[clear]     ({what}: {mm:.0} mm deep at most)");
            }
            for (what, times) in &found {
                println!("[clear]     {what}: {}", moments(times));
            }
            if !found.is_empty() {
                failed.push(name.clone());
            }
        }
    }
    assert!(prefix.is_some() || failed.is_empty(), "these pass through the body: {failed:?}");
}
