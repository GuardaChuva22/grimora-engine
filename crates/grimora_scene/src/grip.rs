//! How a hand holds a weapon, as one small file next to the model: `<model>_grip.ron`.
//!
//! A clip doesn't say where the fingers go or how the weapon sits in the palm, the file does,
//! so every clip of the model holds it the same way: [`apply_fit`] takes those joints out of
//! all of them, and the hand alone carries the weapon. The fit is worked out on load from the
//! bind pose: fingers wrap around a cylinder, phalanx by phalanx.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use glam::{Mat3, Quat, Vec3};
use grimora_core::animation::{AnimationClip, Channel, Interpolation, Keyframes};
use grimora_core::{FREE_HAND_CLIP, SECOND_GRIP_CLIP, Skeleton};
use serde::Deserialize;

/// How an empty hand hangs, degrees from the bind pose: the wrist bends a little towards the
/// palm and towards the little finger, and each phalanx of the fingers curls a bit more than
/// the last until the middle one.
const FREE_WRIST: (f32, f32) = (10.0, 7.0);
const FREE_FINGERS: [f32; 3] = [4.0, 9.0, 6.0];
const FREE_THUMB: [f32; 3] = [-6.0, 0.0, 4.0];
/// The same, lying on a page.
const FLAT_FINGERS: [f32; 3] = [-4.0, 0.0, 0.0];
const FLAT_THUMB: [f32; 3] = [-45.0, 0.0, 0.0];

/// A hand closing on a round handle. Lengths are meters, angles degrees.
#[derive(Debug, Clone, Deserialize)]
pub struct GripDesc {
    pub hand: String,
    pub weapon: String,
    /// The knuckle of the middle finger, and one further along the row towards the index:
    /// the hand's axes come from these two.
    pub knuckle: String,
    pub row_towards: String,
    /// The middle of the handle from `knuckle`, as (along the row, along the fingers, into the palm).
    pub center: Vec3,
    pub radius: f32,
    /// How far the blade leans out from square to the fingers, so that the forearm and blade
    /// meet at more than a right angle with a straight wrist, as a handle does.
    #[serde(default)]
    pub tilt: f32,
    pub fingers: Vec<FingerDesc>,
    pub thumb: ThumbDesc,
    #[serde(default)]
    pub carry: Option<CarryDesc>,
    /// The other hand, for the clips that hold the weapon with both ([`grimora_core::Control::GripL`]).
    /// Its fingers only close in those: no clip but those has anything to say about it.
    #[serde(default)]
    pub second: Option<Box<GripDesc>>,
    /// It's the mirror image of a right hand, whose palm is on the other side of the row.
    #[serde(default)]
    pub left: bool,
}

/// Where the weapon points while it isn't being used: the grip stays, and the arm turns.
#[derive(Debug, Clone, Deserialize)]
pub struct CarryDesc {
    /// Which way the blade points in the model, facing -Z: reached in the first clip by rolling
    /// the forearm, and then bending the wrist for what's left.
    pub aim: Vec3,
    /// Clips that carry it, which all get the first one's turn so that they stay alike.
    pub clips: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FingerDesc {
    /// From the knuckle out.
    pub joints: Vec<String>,
    /// Length of the last phalanx, which no joint marks the end of.
    pub tip: f32,
    /// From the skin touching the handle to the middle of the bone.
    pub pad: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ThumbDesc {
    pub joints: Vec<String>,
    pub tip: f32,
    pub pad: f32,
    /// Where the tip lands around the handle: 0 faces along the fingers, 90 into the palm.
    pub around: f32,
    /// And along it, from the handle's middle towards the blade.
    pub along: f32,
}

/// What a [`GripDesc`] comes to on one skeleton.
#[derive(Debug, Clone)]
pub struct GripFit {
    /// Local rotation of each finger joint.
    pub joints: Vec<(usize, Quat)>,
    /// The hand with nothing in it: local rotation of the wrist first, then of the fingers.
    pub free: Vec<(usize, Quat)>,
    /// The same joints, with the fingers lying flat on something.
    pub flat: Vec<(usize, Quat)>,
    pub weapon: usize,
    pub weapon_rotation: Quat,
    pub weapon_translation: Vec3,
    /// In the bind pose: the middle of the handle, its axis and the bones it should be
    /// touching, for checking the fit.
    pub center: Vec3,
    pub axis: Vec3,
    pub phalanges: Vec<Vec3>,
    pub carry: Option<CarryDesc>,
    pub second: Option<Box<GripFit>>,
}

/// The file with a model's grip: `chartest_grip.ron` for `chartest.gltf`.
pub fn grip_path(model: &Path) -> PathBuf {
    let stem = model.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    model.with_file_name(format!("{stem}_grip.ron"))
}

fn rotation(global: glam::Mat4) -> Quat {
    global.to_scale_rotation_translation().1.normalize()
}

impl GripDesc {
    /// Fits the hand to the handle in the bind pose.
    pub fn fit(&self, skeleton: &Skeleton) -> Result<GripFit> {
        let joint = |name: &str| skeleton.joint(name).with_context(|| format!("no joint {name}"));
        let mut globals = Vec::new();
        skeleton.global_transforms(&skeleton.rest_pose(), &mut globals);
        let head = |j: usize| globals[j].w_axis.truncate();
        let rest = |j: usize| rotation(globals[j]);
        let rest_of = rest;
        let (hand, weapon, knuckle) = (joint(&self.hand)?, joint(&self.weapon)?, joint(&self.knuckle)?);

        // The hand's axes.
        let along_fingers = (head(knuckle) - head(hand)).normalize();
        let towards = head(joint(&self.row_towards)?) - head(knuckle);
        let row = (towards - along_fingers * towards.dot(along_fingers)).normalize();
        let palm = if self.left { along_fingers.cross(row) } else { row.cross(along_fingers) }.normalize();

        // The handle, and the plane around it in which fingers wrap.
        let center = head(knuckle) + row * self.center.x + along_fingers * self.center.y + palm * self.center.z;
        let (sin, cos) = self.tilt.to_radians().sin_cos();
        let axis = (row * cos + along_fingers * sin).normalize();
        let e1 = (along_fingers - axis * along_fingers.dot(axis)).normalize();
        let e2 = palm;

        // The edge runs back along the hand, in line with the knuckles.
        let edge = -e1;
        let socket = Mat3::from_cols(axis.cross(edge), axis, edge);
        let weapon_rotation = (rest(hand).inverse() * Quat::from_mat3(&socket)).normalize();
        let weapon_translation = globals[hand].inverse().transform_point3(center);

        let mut joints = Vec::new();
        let mut phalanges = Vec::new();
        let mut pose_chain = |names: &[String], points: &[Vec3]| -> Result<()> {
            let ids = names.iter().map(|name| joint(name)).collect::<Result<Vec<_>>>()?;
            debug_assert_eq!(ids.len() + 1, points.len());
            let mut above = rest(hand);
            let mut last_rest = Vec3::ZERO;
            for (k, &id) in ids.iter().enumerate() {
                let rest_dir = match ids.get(k + 1) {
                    Some(&next) => (head(next) - head(id)).normalize(),
                    None => last_rest,
                };
                last_rest = rest_dir;
                let turn = Quat::from_rotation_arc(rest_dir, (points[k + 1] - points[k]).normalize());
                let global = turn * rest(id);
                joints.push((id, (above.inverse() * global).normalize()));
                above = global;
            }
            phalanges.extend_from_slice(&points[1..]);
            Ok(())
        };

        for finger in &self.fingers {
            let ids = finger.joints.iter().map(|name| joint(name)).collect::<Result<Vec<_>>>()?;
            let start = head(ids[0]);
            let lengths: Vec<f32> = (0..ids.len())
                .map(|k| ids.get(k + 1).map_or(finger.tip, |&next| head(next).distance(head(ids[k]))))
                .collect();
            let wrap = self.radius + finger.pad;
            let d = start - center;
            let along = d.dot(axis);
            let (u, v) = (d.dot(e1), d.dot(e2));
            let far = (u * u + v * v).sqrt();
            // The first phalanx reaches the circle from the knuckle, the rest are chords of it.
            let open =
                ((far * far + wrap * wrap - lengths[0] * lengths[0]) / (2.0 * far * wrap)).clamp(-1.0, 1.0).acos();
            let mut angle = v.atan2(u) + open;
            let on_circle = |angle: f32| center + axis * along + e1 * (wrap * angle.cos()) + e2 * (wrap * angle.sin());
            let mut points = vec![start, on_circle(angle)];
            for &length in &lengths[1..] {
                angle += 2.0 * (length / (2.0 * wrap)).min(0.95).asin();
                points.push(on_circle(angle));
            }
            pose_chain(&finger.joints, &points)?;
        }

        // The thumb comes from the other side: it reaches a spot on the handle, as an arm would.
        let thumb = &self.thumb;
        let ids = thumb.joints.iter().map(|name| joint(name)).collect::<Result<Vec<_>>>()?;
        anyhow::ensure!(ids.len() == 3, "the thumb has three joints");
        let (l0, l1) = (head(ids[0]).distance(head(ids[1])), head(ids[1]).distance(head(ids[2])));
        let wrap = self.radius + thumb.pad;
        let around = thumb.around.to_radians();
        let (cos_a, sin_a) = (around.cos(), around.sin());
        let tip_at = center + axis * thumb.along + e1 * (wrap * cos_a) + e2 * (wrap * sin_a);
        // The last phalanx lies along the handle's curve, towards the blade.
        let tangent = (e1 * sin_a - e2 * cos_a + axis * 0.8).normalize();
        let last = tip_at - tangent * thumb.tip;
        let start = head(ids[0]);
        let to = last - start;
        let reach = ((l0 + l1) * 0.999).min(to.length()).max((l0 - l1).abs() + 1e-3);
        let dir = to.normalize();
        let run = (l0 * l0 - l1 * l1 + reach * reach) / (2.0 * reach);
        let rise = (l0 * l0 - run * run).max(0.0).sqrt();
        let middle = (start + last) * 0.5;
        let outward = middle - (center + axis * (middle - center).dot(axis));
        let side = (outward - dir * outward.dot(dir)).normalize_or_zero();
        let bend = start + dir * run + side * rise;
        let reached = start + dir * reach;
        pose_chain(&thumb.joints, &[start, bend, reached, reached + tangent * thumb.tip])?;

        // The same hand with nothing in it, from the bind pose.
        let curl = along_fingers.cross(palm).normalize();
        let wrist = Quat::from_axis_angle(curl, FREE_WRIST.0.to_radians())
            * Quat::from_axis_angle(palm, FREE_WRIST.1.to_radians());
        let above = skeleton.joints[hand].parent.map_or(Quat::IDENTITY, rest_of);
        let thumb_along = (head(ids[1]) - head(ids[0])).normalize();
        let relaxed = |fingers: &[f32; 3], thumbs: &[f32; 3]| -> Result<Vec<(usize, Quat)>> {
            let mut joints = vec![(hand, (above.inverse() * wrist * rest(hand)).normalize())];
            let mut relax = |names: &[String], axis: Vec3, degrees: &[f32; 3]| -> Result<()> {
                let (mut above, mut total) = (rest(hand), 0.0);
                for (name, degrees) in names.iter().zip(degrees) {
                    let id = joint(name)?;
                    total += degrees.to_radians();
                    let global = Quat::from_axis_angle(axis, total) * rest(id);
                    joints.push((id, (above.inverse() * global).normalize()));
                    above = global;
                }
                Ok(())
            };
            for finger in &self.fingers {
                relax(&finger.joints, curl, fingers)?;
            }
            relax(&thumb.joints, thumb_along.cross(palm).normalize(), thumbs)?;
            Ok(joints)
        };
        let (free, flat) = (relaxed(&FREE_FINGERS, &FREE_THUMB)?, relaxed(&FLAT_FINGERS, &FLAT_THUMB)?);

        let second = self.second.as_ref().map(|hand| hand.fit(skeleton).map(Box::new)).transpose()?;
        Ok(GripFit {
            second,
            joints,
            free,
            flat,
            weapon,
            weapon_rotation,
            weapon_translation,
            center,
            axis,
            phalanges,
            carry: self.carry.clone(),
        })
    }
}

/// Puts the grip in the skeleton: the weapon's joint at the handle, and every clip holding it.
pub fn apply_fit(skeleton: &mut Skeleton, fit: &GripFit) {
    let weapon = &mut skeleton.joints[fit.weapon];
    weapon.rest.translation = fit.weapon_translation;
    weapon.rest.rotation = fit.weapon_rotation;
    let mut held: Vec<(usize, Quat)> = fit.joints.clone();
    held.push((fit.weapon, fit.weapon_rotation));
    skeleton.clips.retain(|clip| clip.name != FREE_HAND_CLIP);
    for clip in skeleton.clips.iter_mut().filter(|clip| clip.name != SECOND_GRIP_CLIP) {
        clip.channels.retain(|channel| !held.iter().any(|(joint, _)| *joint == channel.joint));
        for &(joint, turn) in &held {
            clip.channels.push(Channel {
                joint,
                interpolation: Interpolation::Linear,
                times: vec![0.0, clip.duration.max(1e-3)],
                keys: Keyframes::Rotation(vec![turn, turn]),
            });
        }
    }
    // Clips sample these as well, into a pose that starts from the joint's rest.
    for &(joint, turn) in &fit.joints {
        skeleton.joints[joint].rest.rotation = turn;
    }
    skeleton.clips.push(free_hand_clip(fit));
    if let Some(second) = &fit.second {
        skeleton.clips.retain(|clip| clip.name != SECOND_GRIP_CLIP);
        skeleton.clips.push(second_grip_clip(second));
    }
}

fn free_hand_clip(fit: &GripFit) -> AnimationClip {
    let channel = |(&(joint, hangs), &(_, flat)): (&(usize, Quat), &(usize, Quat))| Channel {
        joint,
        interpolation: Interpolation::Linear,
        times: vec![0.0, 1.0],
        keys: Keyframes::Rotation(vec![hangs, flat]),
    };
    let channels = fit.free.iter().zip(&fit.flat).map(channel).collect();
    AnimationClip { name: FREE_HAND_CLIP.to_string(), duration: 1.0, channels, events: Vec::new() }
}

/// What the rig reads about the other hand: the closed fingers, and the weapon's joint keyed
/// to where the handle is in that hand, though it isn't that hand's child.
fn second_grip_clip(fit: &GripFit) -> AnimationClip {
    let times = vec![0.0];
    let channel = |joint, keys| Channel { joint, interpolation: Interpolation::Linear, times: times.clone(), keys };
    let mut channels: Vec<Channel> =
        fit.joints.iter().map(|&(joint, turn)| channel(joint, Keyframes::Rotation(vec![turn]))).collect();
    channels.push(channel(fit.weapon, Keyframes::Rotation(vec![fit.weapon_rotation])));
    channels.push(channel(fit.weapon, Keyframes::Translation(vec![fit.weapon_translation])));
    AnimationClip { name: SECOND_GRIP_CLIP.to_string(), duration: 1.0 / 60.0, channels, events: Vec::new() }
}

/// Turns the arm in the clips that carry the weapon, so that it points where the file says.
/// Call it once, on clips that already hold it: see [`apply_fit`].
pub fn apply_carry(skeleton: &mut Skeleton, fit: &GripFit) -> Option<()> {
    let carry = fit.carry.as_ref()?;
    let weapon = fit.weapon;
    let hand = skeleton.joints[weapon].parent?;
    let forearm = skeleton.joints[hand].parent?;
    let first = skeleton.clip(carry.clips.first()?)?;
    let mut pose = skeleton.rest_pose();
    skeleton.clips[first].sample(0.0, &mut pose);
    let mut globals = Vec::new();
    skeleton.global_transforms(&pose, &mut globals);

    let along = (globals[hand].w_axis.truncate() - globals[forearm].w_axis.truncate()).normalize();
    let aim = carry.aim.normalize();
    let blade = rotation(globals[weapon]) * Vec3::Y;
    let flat = |v: Vec3| (v - along * v.dot(along)).normalize();
    let (from, to) = (flat(blade), flat(aim));
    let roll = along.dot(from.cross(to)).atan2(from.dot(to));
    let rolled = Quat::from_axis_angle(along, roll);
    let local_roll = Quat::from_axis_angle(rotation(globals[forearm]).inverse() * along, roll);
    let held = rolled * rotation(globals[hand]);
    let wrist = held.inverse() * Quat::from_rotation_arc((rolled * blade).normalize(), aim) * held;

    for name in &carry.clips {
        let Some(clip) = skeleton.clip(name) else { continue };
        for channel in &mut skeleton.clips[clip].channels {
            let turn = match channel.joint {
                joint if joint == forearm => local_roll,
                joint if joint == hand => wrist,
                _ => continue,
            };
            if let Keyframes::Rotation(keys) = &mut channel.keys {
                for key in keys {
                    *key = (*key * turn).normalize();
                }
            }
        }
    }
    Some(())
}

/// Reads and fits a model's grip, if it has one. A broken file only logs its error.
pub fn load_grip(model: &Path, skeleton: &Skeleton) -> Option<GripFit> {
    let path = grip_path(model);
    let text = std::fs::read_to_string(&path).ok()?;
    let fit = ron::from_str::<GripDesc>(&text).map_err(anyhow::Error::from).and_then(|desc| desc.fit(skeleton));
    match fit {
        Ok(fit) => Some(fit),
        Err(err) => {
            log::error!("grip {}: {err:#}", path.display());
            None
        }
    }
}
