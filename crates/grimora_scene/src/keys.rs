//! Clips for models that aren't people: the file `<model>_keys.ron` beside the model. The poses
//! of [`grimora_core::pose`] are for a humanoid. These only say where each joint points, in the
//! model's space, so they work on any skeleton: a wing is three joints pointing somewhere.
//!
//! Poses have names, and a key of a clip is a list of them, one on top of the other: the wings
//! of one, the legs of another. Each pose also becomes a clip of one frame, `pose:<name>`, to
//! look at it on its own (`animation: "pose:top"` on an object).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use glam::{EulerRot, Quat, Vec3, Vec4};
use grimora_core::animation::{Channel, Interpolation, Keyframes};
use grimora_core::{AnimationClip, ClipEvent, MeshData, Skeleton};
use serde::Deserialize;

/// Frames per second clips are baked at.
const KEY_FPS: f32 = 30.0;

/// Where one joint goes in a pose. A joint in no pose keeps following its parent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct JointKey {
    /// The way its bone points, in the model's space.
    pub aim: Option<Vec3>,
    /// Or: degrees around x, y and z of the model's space, from where it rests.
    pub turn: Option<Vec3>,
    /// Degrees around its own bone, after `aim`.
    pub twist: f32,
}

impl JointKey {
    /// The same on the other side of the body.
    fn mirrored(self) -> Self {
        Self {
            aim: self.aim.map(|a| Vec3::new(-a.x, a.y, a.z)),
            turn: self.turn.map(|t| Vec3::new(t.x, -t.y, -t.z)),
            twist: -self.twist,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct KeyClip {
    /// Seconds.
    pub duration: f32,
    /// The last key leads back into the first.
    pub looping: bool,
    /// (seconds, names of the poses).
    pub keys: Vec<(f32, Vec<String>)>,
    /// (seconds, name), as in [`AnimationClip::events`].
    pub events: Vec<(f32, String)>,
}

/// A `<model>_keys.ron` file. In a pose, a joint ending in `.L` also moves its `.R`, mirrored,
/// unless that pose names the `.R` too.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct KeyFile {
    pub poses: BTreeMap<String, BTreeMap<String, JointKey>>,
    pub clips: BTreeMap<String, KeyClip>,
}

/// The keys file of a model: `Gaivota_keys.ron` for `Gaivota.glb`.
pub(crate) fn keys_path(model: &Path) -> PathBuf {
    let stem = model.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    model.with_file_name(format!("{stem}_keys.ron"))
}

/// Reads a keys file.
pub fn parse_keys(text: &str) -> Result<KeyFile> {
    let options = ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
    Ok(options.from_str(text)?)
}

/// Bakes a model's keys into its skeleton. A broken file only logs its error.
pub(crate) fn add_key_clips(model: &Path, mesh: &mut MeshData) {
    let path = keys_path(model);
    if !path.exists() {
        return;
    }
    let Some(skeleton) = &mesh.skeleton else { return };
    let read = std::fs::read_to_string(&path)
        .map_err(anyhow::Error::from)
        .and_then(|text| parse_keys(&text))
        .with_context(|| format!("reading {}", path.display()));
    match read {
        Ok(file) => {
            let mut skeleton = Skeleton::clone(skeleton);
            let clips = bake_keys(&skeleton, &file);
            for clip in clips {
                match skeleton.clips.iter_mut().find(|c| c.name == clip.name) {
                    Some(old) => *old = clip,
                    None => skeleton.clips.push(clip),
                }
            }
            mesh.skeleton = Some(Arc::new(skeleton));
        }
        Err(err) => log::error!("{err:#}"),
    }
}

/// Every clip of a file, and a clip of one frame for each of its poses.
pub fn bake_keys(skeleton: &Skeleton, file: &KeyFile) -> Vec<AnimationClip> {
    let rig = RestRig::new(skeleton);
    let mut clips = Vec::new();
    for (name, clip) in &file.clips {
        clips.push(rig.bake(skeleton, file, name, clip));
    }
    for name in file.poses.keys() {
        let still = KeyClip { duration: 1.0 / KEY_FPS, keys: vec![(0.0, vec![name.clone()])], ..Default::default() };
        clips.push(rig.bake(skeleton, file, &format!("pose:{name}"), &still));
    }
    clips
}

/// The skeleton as it rests, in the model's space.
struct RestRig {
    rotations: Vec<Quat>,
}

impl RestRig {
    fn new(skeleton: &Skeleton) -> Self {
        let mut globals = Vec::new();
        skeleton.global_transforms(&skeleton.rest_pose(), &mut globals);
        Self { rotations: globals.iter().map(|g| g.to_scale_rotation_translation().1).collect() }
    }

    fn bake(&self, skeleton: &Skeleton, file: &KeyFile, name: &str, clip: &KeyClip) -> AnimationClip {
        let joints = skeleton.joints.len();
        // Each key says where each joint it moves is, in the model's space.
        let mut used = vec![false; joints];
        let mut keys: Vec<(f32, Vec<Quat>)> = Vec::new();
        for (time, poses) in &clip.keys {
            let mut rotations = self.rotations.clone();
            for pose in poses {
                let Some(pose) = file.poses.get(pose) else {
                    log::warn!("clip {name}: no pose called {pose:?}");
                    continue;
                };
                for (joint, key) in pose {
                    self.place(skeleton, joint, *key, &mut rotations, &mut used, name);
                    if let Some(side) = joint.strip_suffix(".L") {
                        let other = format!("{side}.R");
                        if !pose.contains_key(&other) {
                            self.place(skeleton, &other, key.mirrored(), &mut rotations, &mut used, name);
                        }
                    }
                }
            }
            keys.push((*time, rotations));
        }

        let duration = clip.duration.max(1.0 / KEY_FPS);
        let frames = ((duration * KEY_FPS).round() as usize).max(1);
        let times: Vec<f32> = (0..=frames).map(|f| f as f32 / frames as f32 * duration).collect();
        let mut channels = Vec::new();
        for joint in (0..joints).filter(|&joint| used[joint]) {
            let parent = skeleton.joints[joint].parent;
            let rotations: Vec<Quat> = times
                .iter()
                .map(|&time| {
                    let here = sample(&keys, joint, time, duration, clip.looping);
                    // Whatever the parent does, this joint points where the key says.
                    let above = parent.map_or_else(
                        || skeleton.root.to_scale_rotation_translation().1,
                        |parent| self.at(skeleton, &keys, &used, parent, time, duration, clip.looping),
                    );
                    (above.inverse() * here).normalize()
                })
                .collect();
            channels.push(Channel {
                joint,
                interpolation: Interpolation::Linear,
                times: times.clone(),
                keys: Keyframes::Rotation(rotations),
            });
        }
        let events = clip.events.iter().map(|(time, name)| ClipEvent { time: *time, name: name.clone() }).collect();
        AnimationClip { name: name.to_string(), duration, channels, events }
    }

    fn place(
        &self,
        skeleton: &Skeleton,
        joint: &str,
        key: JointKey,
        rotations: &mut [Quat],
        used: &mut [bool],
        clip: &str,
    ) {
        let Some(index) = skeleton.joint(joint) else {
            log::warn!("clip {clip}: the model has no joint {joint:?}");
            return;
        };
        let rest = self.rotations[index];
        rotations[index] = if let Some(aim) = key.aim {
            let aim = aim.normalize_or(Vec3::Y);
            let bone = rest * Vec3::Y;
            Quat::from_axis_angle(aim, key.twist.to_radians()) * Quat::from_rotation_arc(bone, aim) * rest
        } else if let Some(turn) = key.turn {
            let [x, y, z] = turn.to_array().map(f32::to_radians);
            Quat::from_euler(EulerRot::YXZ, y, x, z) * rest
        } else {
            rest
        };
        used[index] = true;
    }

    /// Where a joint is in the model's space, at a moment of a clip.
    #[allow(clippy::too_many_arguments)]
    fn at(
        &self,
        skeleton: &Skeleton,
        keys: &[(f32, Vec<Quat>)],
        used: &[bool],
        joint: usize,
        time: f32,
        duration: f32,
        looping: bool,
    ) -> Quat {
        if used[joint] {
            return sample(keys, joint, time, duration, looping);
        }
        // Not in any key: it follows its parent, as it rests.
        let local = skeleton.joints[joint].rest.rotation;
        match skeleton.joints[joint].parent {
            Some(parent) => self.at(skeleton, keys, used, parent, time, duration, looping) * local,
            None => skeleton.root.to_scale_rotation_translation().1 * local,
        }
    }
}

/// A joint's rotation at a moment, on a smooth curve through the keys.
fn sample(keys: &[(f32, Vec<Quat>)], joint: usize, time: f32, duration: f32, looping: bool) -> Quat {
    let count = keys.len();
    match count {
        0 => return Quat::IDENTITY,
        1 => return keys[0].1[joint],
        _ => {}
    }
    let next = keys.iter().position(|(at, _)| *at > time);
    let (from, to) = match next {
        Some(0) => (if looping { count - 1 } else { 0 }, 0),
        Some(next) => (next - 1, next),
        None if looping => (count - 1, 0),
        None => (count - 1, count - 1),
    };
    if from == to {
        return keys[from].1[joint];
    }
    let start = keys[from].0;
    let mut end = keys[to].0;
    let mut at = time;
    if end <= start {
        end += duration;
        if at < start {
            at += duration;
        }
    }
    let t = ((at - start) / (end - start).max(1e-4)).clamp(0.0, 1.0);
    let wrap = |i: isize| -> usize {
        if looping { i.rem_euclid(count as isize) as usize } else { i.clamp(0, count as isize - 1) as usize }
    };
    let before = keys[wrap(from as isize - 1)].1[joint];
    let after = keys[wrap(to as isize + 1)].1[joint];
    catmull_rom(before, keys[from].1[joint], keys[to].1[joint], after, t)
}

fn catmull_rom(a: Quat, b: Quat, c: Quat, d: Quat, t: f32) -> Quat {
    // On the same side of the sphere as `b`, or the curve takes the long way around.
    let side = |q: Quat| -> Vec4 {
        let v = Vec4::from(q);
        if v.dot(Vec4::from(b)) < 0.0 { -v } else { v }
    };
    let (p0, p1, p2, p3) = (side(a), Vec4::from(b), side(c), side(d));
    let (t2, t3) = (t * t, t * t * t);
    let v = 0.5
        * (p1 * 2.0
            + (p2 - p0) * t
            + (p0 * 2.0 - p1 * 5.0 + p2 * 4.0 - p3) * t2
            + (p1 * 3.0 - p0 - p2 * 3.0 + p3) * t3);
    Quat::from_vec4(v).normalize()
}

#[cfg(test)]
mod tests {
    use glam::Mat4;
    use grimora_core::animation::{Joint, JointTransform};

    use super::*;

    /// A body with a wing on each side, folded back, one joint each.
    fn skeleton() -> Skeleton {
        let folded = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
        let joint = |name: &str, parent, x: f32| Joint {
            name: name.into(),
            parent,
            rest: JointTransform { translation: Vec3::new(x, 0.0, 0.0), rotation: folded, scale: Vec3::ONE },
            inverse_bind: Mat4::IDENTITY,
        };
        let body =
            Joint { name: "Body".into(), parent: None, rest: JointTransform::IDENTITY, inverse_bind: Mat4::IDENTITY };
        Skeleton::new(vec![body, joint("Wing.L", Some(0), 1.0), joint("Wing.R", Some(0), -1.0)], Mat4::IDENTITY, vec![])
    }

    fn bone(skeleton: &Skeleton, clip: &AnimationClip, name: &str, time: f32) -> Vec3 {
        let mut pose = skeleton.rest_pose();
        clip.sample(time, &mut pose);
        let mut globals = Vec::new();
        skeleton.global_transforms(&pose, &mut globals);
        (globals[skeleton.joint(name).unwrap()].to_scale_rotation_translation().1 * Vec3::Y).normalize()
    }

    const FILE: &str = r#"(
        poses: {
            "up": { "Wing.L": (aim: (1.0, 1.0, 0.0)) },
            "down": { "Wing.L": (aim: (1.0, -1.0, 0.0)) },
            "bent": { "Body": (turn: (0.0, 0.0, 90.0)), "Wing.L": (aim: (1.0, 0.0, 0.0)) },
        },
        clips: {
            "flap": (duration: 1.0, looping: true, keys: [(0.0, ["up"]), (0.5, ["down"])]),
            "lean": (duration: 1.0, keys: [(0.0, ["bent"])]),
        },
    )"#;

    #[test]
    fn a_joint_points_where_the_key_says_and_the_other_side_mirrors_it() {
        let skeleton = skeleton();
        let clips = bake_keys(&skeleton, &parse_keys(FILE).unwrap());
        let flap = clips.iter().find(|c| c.name == "flap").unwrap();
        let up = Vec3::new(1.0, 1.0, 0.0).normalize();
        assert!(bone(&skeleton, flap, "Wing.L", 0.0).distance(up) < 0.01);
        assert!(bone(&skeleton, flap, "Wing.R", 0.0).distance(Vec3::new(-up.x, up.y, 0.0)) < 0.01);
        let down = Vec3::new(1.0, -1.0, 0.0).normalize();
        assert!(bone(&skeleton, flap, "Wing.L", 0.5).distance(down) < 0.01);
    }

    #[test]
    fn a_looping_clip_ends_where_it_starts() {
        let skeleton = skeleton();
        let clips = bake_keys(&skeleton, &parse_keys(FILE).unwrap());
        let flap = clips.iter().find(|c| c.name == "flap").unwrap();
        let (start, end) = (bone(&skeleton, flap, "Wing.L", 0.0), bone(&skeleton, flap, "Wing.L", 1.0));
        assert!(start.distance(end) < 0.01, "{start} and {end}");
    }

    #[test]
    fn the_keys_are_in_the_models_space_whatever_the_parent_does() {
        let skeleton = skeleton();
        let clips = bake_keys(&skeleton, &parse_keys(FILE).unwrap());
        let lean = clips.iter().find(|c| c.name == "lean").unwrap();
        assert!(bone(&skeleton, lean, "Wing.L", 0.0).distance(Vec3::X) < 0.01);
    }

    #[test]
    fn every_pose_is_a_clip_too() {
        let skeleton = skeleton();
        let clips = bake_keys(&skeleton, &parse_keys(FILE).unwrap());
        assert!(clips.iter().any(|c| c.name == "pose:up"));
    }
}
