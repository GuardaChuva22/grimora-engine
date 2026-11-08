//! Animation written as poses instead of bones: where the hands, feet and blade go, solved
//! into joint rotations by two-bone IK. A [`PoseClip`] is what lives in a `.ron` file and what
//! the animator edits; [`Rig::bake`] turns it into an ordinary [`AnimationClip`].
//!
//! All positions are in meters in the character's space: x right, y up, looking towards -z.
//! Angles are degrees, as (pitch, yaw, roll): pitch leans forward (for a foot, toes down),
//! yaw turns left, roll leans to the character's right.
//!
//! A control between its keys follows a monotone cubic, so it eases into stops, and on top of
//! that a spring ([`Control::spring`]) makes the body follow the keys instead of being on them:
//! it trails a little, overshoots where a move stops and settles. Feet and the root have no
//! spring, or they would slide.

use std::collections::BTreeMap;

use glam::{Mat3, Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::animation::{AnimationClip, Channel, ClipEvent, Interpolation, JointTransform, Keyframes, Skeleton};

/// Frames per second clips are baked at, and what keys snap to.
pub const POSE_FPS: f32 = 60.0;
/// Steps per second the springs run at.
const SPRING_RATE: f32 = 240.0;
/// Degrees from rest that (fingers, fingertips, thumb, thumb across the palm) bend.
const FIST: [f32; 4] = [26.0, 64.0, 46.0, 34.0];
const OPEN: [f32; 4] = [8.0, 10.0, 6.0, 0.0];
/// Where feet go in a roll, from the hips; `x` is mirrored for the left foot.
const TUCK_FEET: Vec3 = Vec3::new(0.1, -0.28, -0.32);
const TUCK_HAND_R: Vec3 = Vec3::new(0.2, -0.16, -0.26);
const TUCK_HAND_L: Vec3 = Vec3::new(-0.2, -0.16, -0.26);
/// Meters along the handle from the sword hand to the other one, when it holds on too.
pub const TWO_HANDS_APART: f32 = 0.105;
/// The hidden clip with the other hand's fit, from the model's grip file.
pub const SECOND_GRIP_CLIP: &str = "__grip_left";

/// One thing a clip can key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Control {
    /// Where the character is: the engine moves it by this, see `Animator::root_delta`. Only x and z.
    Root,
    /// Offset from the standing hips.
    Hips,
    HipsRot,
    /// Rotation over the hips, shared by spine and chest.
    Torso,
    /// Rotation over the chest, shared with the neck.
    Head,
    /// The ankle. Unlike the rest it ignores [`Control::Root`], so a foot with no new key
    /// stays planted on the ground.
    FootL,
    FootR,
    FootLRot,
    FootRRot,
    /// Degrees the toes bend, in x.
    ToeL,
    ToeR,
    /// Which way the knee points.
    KneeL,
    KneeR,
    /// The middle of the grip.
    HandR,
    /// Where the blade points.
    Blade,
    /// Where its back edge points: the one towards the wrist. With a straight wrist it runs
    /// back up the forearm, and the blade is at right angles to it.
    Edge,
    ElbowR,
    /// The wrist of the free hand.
    HandL,
    /// How that hand turns from its rest pose, in the body's space, as Euler angles: palm up
    /// to hold a book, whatever the elbow does. All zeros leaves it to the forearm.
    HandLRot,
    ElbowL,
    /// 0..1 in x: from those feet and hands to balled up, for rolling.
    Tuck,
    /// 0..1 in x: from where [`Control::HandL`] says to on the handle, closed, under the sword hand.
    GripL,
    /// 0..1 in x: closes the free hand's fingers where it is, as around a handle.
    CurlL,
}

impl Control {
    pub const ALL: [Control; 23] = [
        Control::Root,
        Control::Hips,
        Control::HipsRot,
        Control::Torso,
        Control::Head,
        Control::FootL,
        Control::FootR,
        Control::FootLRot,
        Control::FootRRot,
        Control::ToeL,
        Control::ToeR,
        Control::KneeL,
        Control::KneeR,
        Control::HandR,
        Control::Blade,
        Control::Edge,
        Control::ElbowR,
        Control::HandL,
        Control::HandLRot,
        Control::ElbowL,
        Control::Tuck,
        Control::GripL,
        Control::CurlL,
    ];

    /// The value it has in a clip that doesn't key it.
    pub fn default_value(self) -> Vec3 {
        match self {
            Control::FootL => Vec3::new(-0.11, 0.09, -0.1),
            Control::FootR => Vec3::new(0.13, 0.09, 0.14),
            Control::KneeL => Vec3::new(-0.12, 0.0, -1.0),
            Control::KneeR => Vec3::new(0.12, 0.0, -1.0),
            Control::HandR => Vec3::new(0.34, 0.86, -0.12),
            Control::Blade => Vec3::new(0.26, -0.46, -0.85),
            Control::Edge => Vec3::new(0.08, 0.88, -0.46),
            Control::ElbowR => Vec3::new(0.35, -0.2, 0.9),
            Control::HandL => Vec3::new(-0.33, 0.97, -0.05),
            Control::ElbowL => Vec3::new(-0.4, -0.2, 0.8),
            _ => Vec3::ZERO,
        }
    }

    /// How it follows its keys, as (hertz, damping): the lower the hertz the later it arrives,
    /// and under 1 of damping it overshoots. The hips lead and the free arm floats.
    pub fn spring(self) -> Option<(f32, f32)> {
        match self {
            Control::Hips => Some((8.0, 0.6)),
            Control::HipsRot => Some((10.0, 0.62)),
            Control::Torso => Some((7.0, 0.52)),
            Control::Head => Some((5.5, 0.62)),
            Control::HandR => Some((17.0, 0.6)),
            Control::Blade | Control::Edge => Some((15.0, 0.55)),
            Control::ElbowR => Some((9.0, 0.75)),
            Control::HandL => Some((4.2, 0.45)),
            Control::ElbowL => Some((5.0, 0.65)),
            _ => None,
        }
    }
}

/// One key: seconds and a value.
pub type PoseKey = (f32, Vec3);

/// A clip as poses. The file's name is the clip's name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PoseClip {
    /// Seconds.
    pub duration: f32,
    #[serde(default)]
    pub looping: bool,
    /// Whether the body follows the keys on springs.
    #[serde(default = "yes")]
    pub springs: bool,
    /// Keys of each control, by time.
    pub tracks: BTreeMap<Control, Vec<PoseKey>>,
    /// (seconds, name), as in [`AnimationClip::events`].
    #[serde(default)]
    pub events: Vec<(f32, String)>,
}

fn yes() -> bool {
    true
}

/// A value for every control.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Controls([Vec3; Control::ALL.len()]);

impl Default for Controls {
    fn default() -> Self {
        Self(Control::ALL.map(Control::default_value))
    }
}

impl std::ops::Index<Control> for Controls {
    type Output = Vec3;
    fn index(&self, control: Control) -> &Vec3 {
        &self.0[control as usize]
    }
}

impl std::ops::IndexMut<Control> for Controls {
    fn index_mut(&mut self, control: Control) -> &mut Vec3 {
        &mut self.0[control as usize]
    }
}

/// A monotone cubic through one control's keys, flat at both ends unless it loops.
#[derive(Debug, Clone)]
struct Curve {
    times: Vec<f32>,
    values: Vec<Vec3>,
    tangents: Vec<Vec3>,
}

impl Curve {
    fn new(keys: &[PoseKey], looping: bool) -> Self {
        let mut sorted: Vec<PoseKey> = keys.to_vec();
        sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
        // Two keys at one moment: the last one counts.
        let mut unique: Vec<PoseKey> = Vec::with_capacity(sorted.len());
        for key in sorted {
            match unique.last_mut() {
                Some(last) if last.0 == key.0 => *last = key,
                _ => unique.push(key),
            }
        }
        let times: Vec<f32> = unique.iter().map(|k| k.0).collect();
        let values: Vec<Vec3> = unique.iter().map(|k| k.1).collect();
        let n = times.len();
        let slope_blend = |h0: f32, h1: f32, s0: f32, s1: f32| {
            if s0 * s1 <= 0.0 {
                return 0.0;
            }
            let (w0, w1) = (2.0 * h1 + h0, h1 + 2.0 * h0);
            (w0 + w1) / (w0 / s0 + w1 / s1)
        };
        let mut tangents = vec![Vec3::ZERO; n];
        for k in 0..n {
            let (before, after) = if k > 0 && k + 1 < n {
                (k - 1, k + 1)
            } else if looping && n > 2 {
                // The two ends are one key: the slopes on either side of the seam.
                (n - 2, 1)
            } else {
                continue;
            };
            let (h0, h1) = if before < after {
                (times[k] - times[before], times[after] - times[k])
            } else {
                (times[n - 1] - times[before], times[after] - times[0])
            };
            let (here_a, here_b) = if before < after { (values[k], values[k]) } else { (values[n - 1], values[0]) };
            let s0 = (here_a - values[before]) / h0.max(1e-6);
            let s1 = (values[after] - here_b) / h1.max(1e-6);
            tangents[k] = Vec3::new(
                slope_blend(h0, h1, s0.x, s1.x),
                slope_blend(h0, h1, s0.y, s1.y),
                slope_blend(h0, h1, s0.z, s1.z),
            );
        }
        Self { times, values, tangents }
    }

    fn at(&self, t: f32) -> Vec3 {
        let n = self.times.len();
        if n == 0 {
            return Vec3::ZERO;
        }
        if t <= self.times[0] {
            return self.values[0];
        }
        if t >= self.times[n - 1] {
            return self.values[n - 1];
        }
        let k = self.times.partition_point(|&at| at <= t) - 1;
        let h = self.times[k + 1] - self.times[k];
        let u = (t - self.times[k]) / h;
        let h00 = (1.0 + 2.0 * u) * (1.0 - u) * (1.0 - u);
        let h10 = u * (1.0 - u) * (1.0 - u);
        let h01 = u * u * (3.0 - 2.0 * u);
        let h11 = u * u * (u - 1.0);
        self.values[k] * h00
            + self.tangents[k] * (h10 * h)
            + self.values[k + 1] * h01
            + self.tangents[k + 1] * (h11 * h)
    }
}

/// What a damped spring does chasing `values`, given at `SPRING_RATE`.
fn follow(values: &[Vec3], hertz: f32, damping: f32) -> Vec<Vec3> {
    let k1 = damping / (std::f32::consts::PI * hertz);
    let k2 = 1.0 / (std::f32::consts::TAU * hertz).powi(2);
    let dt = 1.0 / SPRING_RATE;
    let Some(&first) = values.first() else { return Vec::new() };
    let (mut y, mut v) = (first, Vec3::ZERO);
    values
        .iter()
        .map(|&x| {
            y += v * dt;
            v += (x - y - v * k1) * (dt / k2);
            y
        })
        .collect()
}

/// Gives a [`PoseClip`]'s controls at any moment. Build a new one after changing the clip.
#[derive(Debug, Clone)]
pub struct PoseSampler {
    curves: Vec<(Control, Curve, Option<Vec<Vec3>>)>,
    duration: f32,
    looping: bool,
}

impl PoseSampler {
    pub fn new(clip: &PoseClip) -> Self {
        Self::with_springs(clip, clip.springs)
    }

    pub fn with_springs(clip: &PoseClip, springs: bool) -> Self {
        let curves = clip
            .tracks
            .iter()
            .filter(|(_, keys)| !keys.is_empty())
            .map(|(&control, keys)| {
                let curve = Curve::new(keys, clip.looping);
                let table = control.spring().filter(|_| springs).map(|(hertz, damping)| {
                    if clip.looping {
                        // Round enough times to repeat, and the last lap is the loop.
                        let steps = ((clip.duration * SPRING_RATE).round() as usize).max(1);
                        let lap: Vec<Vec3> = (0..steps).map(|k| curve.at(k as f32 / SPRING_RATE)).collect();
                        let laps: Vec<Vec3> = lap.iter().cycle().take(steps * 4).copied().collect();
                        follow(&laps, hertz, damping)[steps * 3..].to_vec()
                    } else {
                        let steps = (clip.duration * SPRING_RATE).round() as usize + 2;
                        let raw: Vec<Vec3> = (0..steps).map(|k| curve.at(k as f32 / SPRING_RATE)).collect();
                        follow(&raw, hertz, damping)
                    }
                });
                (control, curve, table)
            })
            .collect();
        Self { curves, duration: clip.duration, looping: clip.looping }
    }

    /// Every control at `t` seconds.
    pub fn sample(&self, t: f32) -> Controls {
        let mut out = Controls::default();
        for (control, curve, table) in &self.curves {
            out[*control] = match table {
                Some(table) if self.looping => {
                    let lap = t.rem_euclid(self.duration.max(1e-6));
                    table[(lap * SPRING_RATE).round() as usize % table.len()]
                }
                Some(table) => {
                    let at = (t * SPRING_RATE).clamp(0.0, table.len() as f32 - 1.0);
                    let k = (at as usize).min(table.len().saturating_sub(2));
                    table[k].lerp(table[(k + 1).min(table.len() - 1)], at - k as f32)
                }
                None => curve.at(t),
            };
        }
        out
    }
}

/// Rotation of (pitch, yaw, roll) in degrees.
pub fn rotation_of(angles: Vec3) -> Quat {
    Quat::from_rotation_y(angles.y.to_radians())
        * Quat::from_rotation_x(-angles.x.to_radians())
        * Quat::from_rotation_z(-angles.z.to_radians())
}

/// A rotation whose first column is `direction`, the second as near `pole` as can be.
fn frame(direction: Vec3, pole: Vec3) -> Mat3 {
    let d = direction.normalize_or_zero();
    let mut p = pole - d * pole.dot(d);
    if p.length() < 1e-5 {
        p = Vec3::Z - d * d.z;
        if p.length() < 1e-5 {
            p = Vec3::X - d * d.x;
        }
    }
    let p = p.normalize_or_zero();
    Mat3::from_cols(d, p, d.cross(p))
}

/// The rotation that takes a limb at rest to `direction`, with its hinge towards `pole`.
fn aim(rest_dir: Vec3, rest_pole: Vec3, direction: Vec3, pole: Vec3) -> Quat {
    Quat::from_mat3(&(frame(direction, pole) * frame(rest_dir, rest_pole).transpose()))
}

/// Poles for the two bones of a bent limb, from the axis it really bends around: the wanted
/// `pole` can point right along a bone, and the bone would spin there.
fn hinge_poles(upper: Vec3, lower: Vec3, pole: Vec3) -> (Vec3, Vec3) {
    let axis = lower.cross(upper);
    if axis.length() < 0.008 {
        return (pole, pole);
    }
    let axis = axis.normalize();
    (axis.cross(upper.normalize_or_zero()), axis.cross(lower.normalize_or_zero()))
}

/// Where the middle joint goes, and how far short of the target the limb stops.
fn two_bones(start: Vec3, target: Vec3, a: f32, b: f32, pole: Vec3) -> (Vec3, f32) {
    let to = target - start;
    let reach = (a + b) * 0.999;
    let short = (to.length() - reach).max(0.0);
    let distance = to.length().min(reach).max((a - b).abs() + 1e-3).max(1e-3);
    let d = to.normalize_or_zero();
    let along = (a * a - b * b + distance * distance) / (2.0 * distance);
    let height = (a * a - along * along).max(0.0).sqrt();
    let mut side = pole - d * pole.dot(d);
    if side.length() < 1e-5 {
        side = Vec3::Z;
    }
    (start + d * along + side.normalize_or_zero() * height, short)
}

#[derive(Debug, Clone, Copy)]
struct Leg {
    thigh: usize,
    shin: usize,
    foot: usize,
    toe: usize,
    sign: f32,
    target: Control,
    rotation: Control,
    toe_bend: Control,
    knee: Control,
    rest_poles: (Vec3, Vec3),
}

#[derive(Debug, Clone, Copy)]
struct Arm {
    clavicle: usize,
    upper: usize,
    forearm: usize,
    hand: usize,
    fingers: usize,
    fingertips: usize,
    thumb: usize,
    right: bool,
    rest_poles: (Vec3, Vec3),
    hand_axes: HandAxes,
    /// Bind-space grip alignment for native thumb chains; the free hand keeps its rest pose.
    thumb_contact: Option<Quat>,
}

/// The hand's bind directions in model space, independent of its resting pose.
#[derive(Debug, Clone, Copy)]
struct HandAxes {
    fingers: Vec3,
    palm: Vec3,
    bend: Vec3,
}

impl HandAxes {
    fn from_bind(finger_offset: Vec3, thumb_offset: Vec3, right: bool) -> Option<Self> {
        let fingers = finger_offset.try_normalize()?;
        let palm = if right { thumb_offset.cross(fingers) } else { fingers.cross(thumb_offset) }.try_normalize()?;
        let bend = fingers.cross(palm).normalize();
        Some(Self { fingers, palm, bend })
    }

    fn thumb_turn(&self, hand: Quat, right: bool, curl: [f32; 4], contact: Option<Quat>) -> Quat {
        if let Some(contact) = contact {
            let amount = ((curl[2] - OPEN[2]) / (FIST[2] - OPEN[2])).clamp(0.0, 1.0);
            return hand * Quat::IDENTITY.slerp(contact, amount);
        }
        let closing = if right { -1.0 } else { 1.0 };
        let bend = Quat::from_axis_angle(self.bend, curl[2].to_radians());
        let across = Quat::from_axis_angle(self.palm, (-closing * curl[3]).to_radians());
        hand * bend * across
    }
}

/// A fitted fist carried by the hand, including native joints the generic rig does not name.
#[derive(Debug, Clone)]
struct GripReference {
    turns: Vec<Option<Quat>>,
    hand_frame: Mat3,
    /// From the wrist to the handle, at rest. Only the second hand's is used: the first has
    /// the weapon's joint for this.
    offset: Vec3,
}

impl GripReference {
    fn new(skeleton: &Skeleton, hand: usize, weapon: usize, bind: &[Mat4]) -> Option<Self> {
        let clip = skeleton.clip("__grip_right")?;
        let mut pose = skeleton.rest_pose();
        skeleton.clips[clip].sample(0.0, &mut pose);
        let mut closed = Vec::new();
        skeleton.global_transforms(&pose, &mut closed);
        let rotation = |matrix: Mat4| matrix.to_scale_rotation_translation().1;
        let hand_delta = rotation(closed[hand]) * rotation(bind[hand]).inverse();
        let relative = hand_delta.inverse();
        let turns = skeleton
            .joints
            .iter()
            .enumerate()
            .map(|(joint, _)| {
                let mut ancestor = joint;
                while ancestor != hand {
                    if ancestor == weapon {
                        return None;
                    }
                    ancestor = skeleton.joints[ancestor].parent?;
                }
                (joint != hand)
                    .then(|| (relative * rotation(closed[joint]) * rotation(bind[joint]).inverse()).normalize())
            })
            .collect();
        // The exported socket's Y follows the blade; Z identifies its edge orientation.
        let socket = relative * rotation(closed[weapon]);
        Some(Self { turns, hand_frame: frame(socket * Vec3::Y, socket * Vec3::Z), offset: Vec3::ZERO })
    }

    /// The other hand, from [`SECOND_GRIP_CLIP`], whose channels for `weapon` say where the
    /// handle is in that hand.
    fn second(skeleton: &Skeleton, hand: usize, weapon: usize, bind: &[Mat4]) -> Option<Self> {
        let clip = skeleton.clip(SECOND_GRIP_CLIP)?;
        let mut pose = skeleton.rest_pose();
        skeleton.clips[clip].sample(0.0, &mut pose);
        let held = pose[weapon];
        let mut closed = Vec::new();
        skeleton.global_transforms(&pose, &mut closed);
        let rotation = |matrix: Mat4| matrix.to_scale_rotation_translation().1;
        let turns = skeleton
            .joints
            .iter()
            .enumerate()
            .map(|(joint, _)| {
                let mut ancestor = joint;
                while ancestor != hand {
                    ancestor = skeleton.joints[ancestor].parent?;
                }
                (joint != hand).then(|| (rotation(closed[joint]) * rotation(bind[joint]).inverse()).normalize())
            })
            .collect();
        let socket = rotation(bind[hand]) * held.rotation;
        let offset = bind[hand].transform_point3(held.translation) - bind[hand].w_axis.truncate();
        Some(Self { turns, hand_frame: frame(socket * Vec3::Y, socket * Vec3::Z), offset })
    }

    /// This joint's rotation, `amount` of the way from `open` to closed on the handle.
    fn closing(&self, joint: usize, hand: Quat, amount: f32, open: Quat) -> Option<Quat> {
        Some(open.slerp(hand * self.turns[joint]?, amount))
    }

    fn apply(
        &self,
        joint: usize,
        hand: Quat,
        heads: &[Vec3],
        parents: &[Option<usize>],
        turns: &mut [Quat],
        positions: &mut [Vec3],
    ) -> bool {
        let Some(reference) = self.turns[joint] else { return false };
        let Some(parent) = parents[joint] else { return false };
        turns[joint] = hand * reference;
        positions[joint] = positions[parent] + turns[parent] * (heads[joint] - heads[parent]);
        true
    }
}

/// What [`Rig::solve`] makes of some controls.
#[derive(Debug, Clone, Default)]
pub struct SolvedPose {
    /// Each joint's rotation from rest, in the character's space.
    pub turns: Vec<Quat>,
    /// Each joint's position relative to the root.
    pub positions: Vec<Vec3>,
    /// How far a limb stops short of its target, in meters.
    pub short_hand: f32,
    pub short_foot: f32,
    /// Degrees the sword hand bends from the forearm's line; over about 55 looks forced.
    pub wrist: f32,
    /// Degrees it is turned around the forearm; over about 100 looks forced.
    pub twist: f32,
    /// The same of the other hand, while it holds on too.
    pub wrist_l: f32,
    pub twist_l: f32,
}

/// A skeleton as the solver needs it. Works with any model with the joints it looks for.
#[derive(Debug, Clone)]
pub struct Rig {
    heads: Vec<Vec3>,
    rest_rotations: Vec<Quat>,
    order: Vec<usize>,
    parents: Vec<Option<usize>>,
    root_rotation: Quat,
    rest: Vec<JointTransform>,
    root: usize,
    hips: usize,
    spine: usize,
    chest: usize,
    neck: usize,
    head: usize,
    weapon: usize,
    lantern: Option<usize>,
    legs: [Leg; 2],
    arms: [Arm; 2],
    hand_rest: Mat3,
    weapon_rest: Mat3,
    right_grip: Option<GripReference>,
    second_grip: Option<GripReference>,
}

impl Rig {
    /// `None` if required joints are missing or the hand's bind directions are degenerate.
    pub fn new(skeleton: &Skeleton) -> Option<Self> {
        let joint = |name: &str| skeleton.joint(name);
        let mut globals = Vec::new();
        skeleton.global_transforms(&skeleton.rest_pose(), &mut globals);
        let heads: Vec<Vec3> = globals.iter().map(|g| g.w_axis.truncate()).collect();
        let rest_rotations = globals.iter().map(|g| g.to_scale_rotation_translation().1).collect();
        let leg = |x: &str, sign, target, rotation, toe_bend, knee| {
            let thigh = joint(&format!("thigh.{x}"))?;
            let shin = joint(&format!("shin.{x}"))?;
            let foot = joint(&format!("foot.{x}"))?;
            Some(Leg {
                thigh,
                shin,
                foot,
                toe: joint(&format!("toe.{x}"))?,
                sign,
                target,
                rotation,
                toe_bend,
                knee,
                rest_poles: hinge_poles(heads[shin] - heads[thigh], heads[foot] - heads[shin], Vec3::NEG_Z),
            })
        };
        let arm = |x: &str, right| {
            let upper = joint(&format!("upper_arm.{x}"))?;
            let forearm = joint(&format!("forearm.{x}"))?;
            let hand = joint(&format!("hand.{x}"))?;
            let fingers = joint(&format!("fingers.{x}"))?;
            let thumb = joint(&format!("thumb.{x}"))?;
            let hand_axes = HandAxes::from_bind(heads[fingers] - heads[hand], heads[thumb] - heads[hand], right)?;
            let thumb_child = skeleton.joints.iter().enumerate().find_map(|(index, child)| {
                (child.parent == Some(thumb) && heads[index].distance_squared(heads[thumb]) > 1e-8).then_some(index)
            });
            let thumb_contact = thumb_child.and_then(|child| {
                if !right {
                    return Some(Quat::IDENTITY);
                }
                let direction = (heads[child] - heads[thumb]).try_normalize()?;
                let target = (heads[joint("weapon.R")?] - heads[thumb]).try_normalize()?;
                Some(aim(direction, hand_axes.palm, target, hand_axes.palm))
            });
            Some(Arm {
                clavicle: joint(&format!("clavicle.{x}"))?,
                upper,
                forearm,
                hand,
                fingers,
                fingertips: joint(&format!("fingertips.{x}"))?,
                thumb,
                right,
                rest_poles: hinge_poles(heads[forearm] - heads[upper], heads[hand] - heads[forearm], Vec3::Z),
                hand_axes,
                thumb_contact,
            })
        };
        let legs = [
            leg("L", -1.0, Control::FootL, Control::FootLRot, Control::ToeL, Control::KneeL)?,
            leg("R", 1.0, Control::FootR, Control::FootRRot, Control::ToeR, Control::KneeR)?,
        ];
        let arms = [arm("R", true)?, arm("L", false)?];
        let hand = arms[0].hand_axes;
        let right_grip = GripReference::new(skeleton, arms[0].hand, joint("weapon.R")?, &globals);
        let second_grip = GripReference::second(skeleton, arms[1].hand, joint("weapon.R")?, &globals);
        let hand_rest = right_grip
            .as_ref()
            .map_or_else(|| frame(hand.bend, -hand.fingers + hand.palm * 0.3), |grip| grip.hand_frame);
        Some(Self {
            heads,
            rest_rotations,
            order: skeleton.order().to_vec(),
            parents: skeleton.joints.iter().map(|j| j.parent).collect(),
            root_rotation: skeleton.root.to_scale_rotation_translation().1,
            rest: skeleton.rest_pose(),
            root: joint("root")?,
            hips: joint("hips")?,
            spine: joint("spine")?,
            chest: joint("chest")?,
            neck: joint("neck")?,
            head: joint("head")?,
            weapon: joint("weapon.R")?,
            lantern: joint("lantern"),
            legs,
            arms,
            // The blade across the palm, its back edge towards the wrist and a little to the palm's side.
            hand_rest,
            // How the weapon joint sits at rest: edges up and down.
            weapon_rest: frame(Vec3::NEG_Z, Vec3::Y),
            right_grip,
            second_grip,
        })
    }

    /// How the right hand holds things and where the joints of that hand are at rest, for tools
    /// that write clips: its frame, `weapon.R` and `fingers.R`.
    pub fn hand_numbers(&self) -> (Mat3, Vec3, Vec3) {
        (self.hand_rest, self.heads[self.weapon], self.heads[self.arms[0].fingers])
    }

    fn rest_dir(&self, from: usize, to: usize) -> Vec3 {
        (self.heads[to] - self.heads[from]).normalize_or_zero()
    }

    fn length(&self, from: usize, to: usize) -> f32 {
        (self.heads[to] - self.heads[from]).length()
    }

    fn finger_turns(&self, arm: &Arm, hand: Quat, curl: [f32; 4], turns: &mut [Quat]) {
        let bend = |degrees: f32| Quat::from_axis_angle(arm.hand_axes.bend, degrees.to_radians());
        let fingers = hand * bend(curl[0]);
        turns[arm.fingers] = fingers;
        turns[arm.fingertips] = fingers * bend(curl[1]);
        turns[arm.thumb] = arm.hand_axes.thumb_turn(hand, arm.right, curl, arm.thumb_contact);
    }

    /// Every joint's rotation and position from controls.
    pub fn solve(&self, c: &Controls) -> SolvedPose {
        let n = self.heads.len();
        let mut turns = vec![Quat::IDENTITY; n];
        let mut positions = vec![Vec3::ZERO; n];
        let mut solved_joint = vec![false; n];
        let mut out = SolvedPose::default();
        let from_parent = |joint: usize, parent: usize, turns: &[Quat], positions: &[Vec3]| {
            positions[parent] + turns[parent] * (self.heads[joint] - self.heads[parent])
        };

        turns[self.hips] = rotation_of(c[Control::HipsRot]);
        positions[self.hips] = self.heads[self.hips] + c[Control::Hips];
        let torso = rotation_of(c[Control::Torso]);
        turns[self.spine] = turns[self.hips] * Quat::IDENTITY.slerp(torso, 0.45);
        turns[self.chest] = turns[self.hips] * torso;
        positions[self.spine] = from_parent(self.spine, self.hips, &turns, &positions);
        positions[self.chest] = from_parent(self.chest, self.spine, &turns, &positions);
        let head = rotation_of(c[Control::Head]);
        turns[self.neck] = turns[self.chest] * Quat::IDENTITY.slerp(head, 0.4);
        turns[self.head] = turns[self.chest] * head;
        positions[self.neck] = from_parent(self.neck, self.chest, &turns, &positions);
        positions[self.head] = from_parent(self.head, self.neck, &turns, &positions);
        for joint in [self.root, self.hips, self.spine, self.chest, self.neck, self.head] {
            solved_joint[joint] = true;
        }
        let tuck = c[Control::Tuck].x.clamp(0.0, 1.0);
        let root_at = Vec3::new(c[Control::Root].x, 0.0, c[Control::Root].z);
        let second = self.second_grip.as_ref().filter(|_| c[Control::GripL].x > 0.0);
        let both_hands = if second.is_some() { c[Control::GripL].x.clamp(0.0, 1.0) * (1.0 - tuck) } else { 0.0 };

        // Legs, from the ankle back up.
        for leg in &self.legs {
            let mut target = c[leg.target] - root_at;
            if tuck > 0.0 {
                let offset = Vec3::new(leg.sign * TUCK_FEET.x.abs(), TUCK_FEET.y, TUCK_FEET.z);
                target = target.lerp(positions[self.hips] + turns[self.hips] * offset, tuck);
            }
            positions[leg.thigh] =
                positions[self.hips] + turns[self.hips] * (self.heads[leg.thigh] - self.heads[self.hips]);
            let pole = if tuck > 0.5 { turns[self.hips] * c[leg.knee] } else { c[leg.knee] };
            let (thigh_len, shin_len) = (self.length(leg.thigh, leg.shin), self.length(leg.shin, leg.foot));
            let (knee, short) = two_bones(positions[leg.thigh], target, thigh_len, shin_len, pole);
            out.short_foot = out.short_foot.max(short);
            let ankle = knee + (target - knee).normalize_or_zero() * shin_len;
            let (pole_a, pole_b) = hinge_poles(knee - positions[leg.thigh], ankle - knee, pole);
            turns[leg.thigh] =
                aim(self.rest_dir(leg.thigh, leg.shin), leg.rest_poles.0, knee - positions[leg.thigh], pole_a);
            positions[leg.shin] = knee;
            turns[leg.shin] = aim(self.rest_dir(leg.shin, leg.foot), leg.rest_poles.1, ankle - knee, pole_b);
            positions[leg.foot] = ankle;
            let base = if tuck > 0.5 { turns[self.hips] } else { Quat::IDENTITY };
            turns[leg.foot] = base * rotation_of(c[leg.rotation]);
            positions[leg.toe] = from_parent(leg.toe, leg.foot, &turns, &positions);
            turns[leg.toe] = turns[leg.foot] * rotation_of(Vec3::new(-c[leg.toe_bend].x, 0.0, 0.0));
            for joint in [leg.thigh, leg.shin, leg.foot, leg.toe] {
                solved_joint[joint] = true;
            }
        }

        // Arms: the hand decides where the wrist goes, and the elbow follows.
        let chest = turns[self.chest];
        let chest_at = positions[self.chest];
        for arm in &self.arms {
            let (target, elbow_pole, tuck_hand) = if arm.right {
                (c[Control::HandR], c[Control::ElbowR], TUCK_HAND_R)
            } else {
                (c[Control::HandL], c[Control::ElbowL], TUCK_HAND_L)
            };
            let mut palm = target;
            if tuck > 0.0 {
                palm = palm.lerp(chest_at + chest * tuck_hand, tuck);
            }
            let mut hand_turn = None;
            let wrist = if arm.right {
                let mut blade = c[Control::Blade].normalize_or_zero();
                let mut edge = c[Control::Edge];
                if tuck > 0.0 {
                    blade = blade.lerp(chest * Vec3::new(0.25, 0.15, 0.95), tuck).normalize_or_zero();
                    edge = edge.lerp(chest * Vec3::new(0.0, 1.0, -0.1), tuck);
                }
                let held = frame(blade, edge);
                let turn = Quat::from_mat3(&(held * self.hand_rest.transpose()));
                turns[self.weapon] = Quat::from_mat3(&(held * self.weapon_rest.transpose()));
                hand_turn = Some(turn);
                palm - turn * (self.heads[self.weapon] - self.heads[arm.hand])
            } else if let Some(grip) = second {
                let blade = c[Control::Blade].normalize_or_zero();
                let turn = Quat::from_mat3(&(frame(blade, c[Control::Edge]) * grip.hand_frame.transpose()));
                hand_turn = Some(turn);
                let on_handle = c[Control::HandR] - blade * TWO_HANDS_APART - turn * grip.offset;
                palm.lerp(on_handle, both_hands)
            } else {
                palm
            };
            // The collar bone gives a fifth of the way towards the hand.
            positions[arm.clavicle] = chest_at + chest * (self.heads[arm.clavicle] - self.heads[self.chest]);
            let shoulder = positions[arm.clavicle] + chest * (self.heads[arm.upper] - self.heads[arm.clavicle]);
            let rest = chest * self.rest_dir(arm.upper, arm.forearm);
            let pull = Quat::from_rotation_arc(rest, (wrist - shoulder).normalize_or_zero());
            turns[arm.clavicle] = Quat::IDENTITY.slerp(pull, 0.18) * chest;
            positions[arm.upper] =
                positions[arm.clavicle] + turns[arm.clavicle] * (self.heads[arm.upper] - self.heads[arm.clavicle]);
            let (upper_len, fore_len) = (self.length(arm.upper, arm.forearm), self.length(arm.forearm, arm.hand));
            let (elbow, short) = two_bones(positions[arm.upper], wrist, upper_len, fore_len, elbow_pole);
            out.short_hand = out.short_hand.max(short);
            let at_wrist = elbow + (wrist - elbow).normalize_or_zero() * fore_len;
            let (pole_a, pole_b) = hinge_poles(elbow - positions[arm.upper], at_wrist - elbow, elbow_pole);
            turns[arm.upper] =
                aim(self.rest_dir(arm.upper, arm.forearm), arm.rest_poles.0, elbow - positions[arm.upper], pole_a);
            positions[arm.forearm] = elbow;
            turns[arm.forearm] = aim(self.rest_dir(arm.forearm, arm.hand), arm.rest_poles.1, at_wrist - elbow, pole_b);
            positions[arm.hand] = at_wrist;
            // The free hand has no blade to tell it how to turn, unless it's told.
            let free_turn = c[Control::HandLRot];
            let turned_freely = !arm.right && both_hands <= 0.0 && free_turn != Vec3::ZERO;
            if turned_freely {
                hand_turn = Some(rotation_of(free_turn));
            }
            let hand = match hand_turn {
                None => turns[arm.forearm],
                Some(hand) => {
                    // The forearm takes half of the hand's roll around its own axis.
                    let axis = (at_wrist - elbow).normalize_or_zero();
                    let mut turn = turns[arm.forearm].inverse() * hand;
                    if turn.w < 0.0 {
                        turn = -turn;
                    }
                    let local = turns[arm.forearm].inverse() * axis;
                    let along = local * Vec3::new(turn.x, turn.y, turn.z).dot(local);
                    let twist = Quat::from_xyzw(along.x, along.y, along.z, turn.w).normalize();
                    let angle = 2.0 * twist.w.abs().min(1.0).acos().to_degrees();
                    // Near half a turn the direction of the twist is a coin toss: leave it to the wrist.
                    let share = 0.5 * ((150.0 - angle) / 30.0).clamp(0.0, 1.0);
                    let amount = if arm.right || turned_freely { 1.0 } else { both_hands };
                    turns[arm.forearm] *= Quat::IDENTITY.slerp(twist, share * amount);
                    let wrist = (hand * self.rest_dir(arm.hand, arm.fingers)).angle_between(axis).to_degrees();
                    if arm.right {
                        (out.twist, out.wrist) = (angle, wrist);
                    } else {
                        (out.twist_l, out.wrist_l) = (angle * amount, wrist * amount);
                    }
                    turns[arm.forearm].slerp(hand, amount)
                }
            };
            turns[arm.hand] = hand;
            self.finger_turns(arm, hand, if arm.right { FIST } else { OPEN }, &mut turns);
            positions[arm.fingers] = from_parent(arm.fingers, arm.hand, &turns, &positions);
            positions[arm.fingertips] = from_parent(arm.fingertips, arm.fingers, &turns, &positions);
            positions[arm.thumb] = from_parent(arm.thumb, arm.hand, &turns, &positions);
            for joint in [arm.clavicle, arm.upper, arm.forearm, arm.hand, arm.fingers, arm.fingertips, arm.thumb] {
                solved_joint[joint] = true;
            }
        }
        positions[self.weapon] =
            self.parents[self.weapon].map_or(Vec3::ZERO, |p| from_parent(self.weapon, p, &turns, &positions));
        solved_joint[self.weapon] = true;
        if let Some(lantern) = self.lantern {
            turns[lantern] = turns[self.hips];
        }
        // Joints waiting for cloth, with nothing on them yet.
        let right_hand = turns[self.arms[0].hand];
        let left_hand = turns[self.arms[1].hand];
        for &joint in &self.order {
            // Apply before the generic inheritance pass so independent native index and
            // third phalanges retain their fitted rotations and their actual parent links.
            if self.right_grip.as_ref().is_some_and(|grip| {
                grip.apply(joint, right_hand, &self.heads, &self.parents, &mut turns, &mut positions)
            }) {
                continue;
            }
            let open = if solved_joint[joint] { turns[joint] } else { left_hand };
            let closed = both_hands.max(c[Control::CurlL].x.clamp(0.0, 1.0));
            let closer = self.second_grip.as_ref().filter(|_| closed > 0.0);
            if let Some(turn) = closer.and_then(|grip| grip.closing(joint, left_hand, closed, open))
                && let Some(parent) = self.parents[joint]
            {
                turns[joint] = turn;
                positions[joint] = from_parent(joint, parent, &turns, &positions);
                continue;
            }
            if solved_joint[joint] {
                continue;
            }
            if let Some(parent) = self.parents[joint] {
                if Some(joint) != self.lantern {
                    turns[joint] = turns[parent];
                }
                positions[joint] = from_parent(joint, parent, &turns, &positions);
            }
        }
        out.turns = turns;
        out.positions = positions;
        out
    }

    /// Joint transforms relative to their parents, as an [`crate::Animator`] wants them. The
    /// `root` joint goes to `root_at`, which is `Vec3::ZERO` to leave travel to the caller.
    pub fn local_pose(&self, solved: &SolvedPose, controls: &Controls, root_at: Vec3, pose: &mut Vec<JointTransform>) {
        pose.clear();
        pose.extend_from_slice(&self.rest);
        let global = |joint: usize| solved.turns[joint] * self.rest_rotations[joint];
        for (joint, local) in pose.iter_mut().enumerate() {
            let above = self.parents[joint].map_or(self.root_rotation, global);
            local.rotation = (above.inverse() * global(joint)).normalize();
        }
        pose[self.root].translation = self.rest[self.root].translation + self.root_rotation.inverse() * root_at;
        let above = self.parents[self.hips].map_or(self.root_rotation, |p| self.rest_rotations[p]);
        pose[self.hips].translation = self.rest[self.hips].translation + above.inverse() * controls[Control::Hips];
    }

    /// The clip as an ordinary one, sampled at [`POSE_FPS`].
    pub fn bake(&self, name: &str, clip: &PoseClip) -> AnimationClip {
        let frames = ((clip.duration * POSE_FPS).round() as usize).max(1);
        let sampler = PoseSampler::new(clip);
        let n = self.heads.len();
        let times: Vec<f32> = (0..=frames).map(|k| k as f32 / POSE_FPS).collect();
        let mut rotations: Vec<Vec<Quat>> = vec![Vec::with_capacity(frames + 1); n];
        let (mut root, mut hips) = (Vec::with_capacity(frames + 1), Vec::with_capacity(frames + 1));
        let mut pose = Vec::new();
        for &t in &times {
            let controls = sampler.sample(t);
            let solved = self.solve(&controls);
            let root_at = Vec3::new(controls[Control::Root].x, 0.0, controls[Control::Root].z);
            self.local_pose(&solved, &controls, root_at, &mut pose);
            for (joint, list) in rotations.iter_mut().enumerate() {
                let q = pose[joint].rotation;
                // Keep to one side, so blending between frames takes the short way.
                let q = if list.last().is_some_and(|before: &Quat| before.dot(q) < 0.0) { -q } else { q };
                list.push(q);
            }
            root.push(pose[self.root].translation);
            hips.push(pose[self.hips].translation);
        }
        let mut channels: Vec<Channel> = rotations
            .into_iter()
            .enumerate()
            .map(|(joint, keys)| Channel {
                joint,
                interpolation: Interpolation::Linear,
                times: times.clone(),
                keys: Keyframes::Rotation(keys),
            })
            .collect();
        for (joint, keys) in [(self.root, root), (self.hips, hips)] {
            channels.push(Channel {
                joint,
                interpolation: Interpolation::Linear,
                times: times.clone(),
                keys: Keyframes::Translation(keys),
            });
        }
        let mut events: Vec<ClipEvent> =
            clip.events.iter().map(|(time, name)| ClipEvent { time: *time, name: name.clone() }).collect();
        events.sort_by(|a, b| a.time.total_cmp(&b.time));
        AnimationClip { name: name.to_string(), duration: frames as f32 / POSE_FPS, channels, events }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grip_reference_fixture() -> (Skeleton, Quat, Quat, Quat, Quat) {
        use crate::animation::Joint;
        let root_turn = rotation_of(Vec3::new(17.0, -31.0, 12.0));
        let hand_turn = rotation_of(Vec3::new(-22.0, 19.0, 8.0));
        let first = Quat::from_rotation_z(-0.65);
        let second = Quat::from_rotation_z(-0.9);
        let third = Quat::from_rotation_z(-0.4);
        let socket = rotation_of(Vec3::new(8.0, 46.0, -12.0));
        let joints = [
            ("root", None, Vec3::ZERO),
            ("hand.R", Some(0), Vec3::new(0.4, 1.1, -0.2)),
            ("fingers.R", Some(1), Vec3::new(0.07, -0.05, -0.02)),
            ("fingertips.R", Some(2), Vec3::new(0.03, -0.02, 0.0)),
            ("index_01", Some(1), Vec3::new(0.04, -0.06, -0.04)),
            ("index_02", Some(4), Vec3::new(0.03, -0.02, 0.0)),
            ("index_03", Some(5), Vec3::new(0.02, -0.02, 0.0)),
            ("thumb.R", Some(1), Vec3::new(-0.01, -0.02, -0.04)),
            ("weapon.R", Some(1), Vec3::new(0.06, -0.04, 0.0)),
            ("hand.L", Some(0), Vec3::new(-0.4, 1.1, -0.2)),
        ]
        .into_iter()
        .map(|(name, parent, translation)| Joint {
            name: name.into(),
            parent,
            rest: JointTransform { translation, ..Default::default() },
            inverse_bind: Mat4::IDENTITY,
        })
        .collect();
        let channels = [(1, hand_turn), (2, first), (3, second), (4, first), (5, second), (6, third), (8, socket)]
            .into_iter()
            .map(|(joint, turn)| Channel {
                joint,
                interpolation: Interpolation::Linear,
                times: vec![0.0],
                keys: Keyframes::Rotation(vec![turn]),
            })
            .collect();
        let clip = AnimationClip { name: "__grip_right".into(), duration: 1.0 / 60.0, channels, events: Vec::new() };
        (Skeleton::new(joints, Mat4::from_quat(root_turn), vec![clip]), root_turn, first, second, third)
    }

    #[test]
    fn fitted_grip_removes_reference_hand_motion_and_keeps_independent_finger_chains() {
        let (skeleton, root, first, second, third) = grip_reference_fixture();
        let mut bind = Vec::new();
        skeleton.global_transforms(&skeleton.rest_pose(), &mut bind);
        let grip = GripReference::new(&skeleton, 1, 8, &bind).unwrap();
        assert!(grip.turns[0].is_none() && grip.turns[1].is_none());
        assert!(grip.turns[8].is_none() && grip.turns[9].is_none());
        assert!(grip.turns[4].unwrap().dot(root * first * root.inverse()).abs() > 1.0 - 1e-6);
        assert!(grip.turns[6].unwrap().dot(root * first * second * third * root.inverse()).abs() > 1.0 - 1e-6);
        let socket = root * rotation_of(Vec3::new(8.0, 46.0, -12.0));
        let expected_frame = frame(socket * Vec3::Y, socket * Vec3::Z);
        assert!((grip.hand_frame - expected_frame).to_cols_array().iter().all(|d| d.abs() < 1e-5));

        let heads: Vec<_> = bind.iter().map(|matrix| matrix.w_axis.truncate()).collect();
        let parents: Vec<_> = skeleton.joints.iter().map(|joint| joint.parent).collect();
        for hand in [Quat::IDENTITY, rotation_of(Vec3::new(41.0, -73.0, 22.0))] {
            let mut turns = vec![Quat::IDENTITY; heads.len()];
            let mut positions = heads.clone();
            turns[1] = hand;
            positions[1] = Vec3::new(-0.3, 1.5, 0.7);
            for &joint in skeleton.order() {
                grip.apply(joint, hand, &heads, &parents, &mut turns, &mut positions);
            }
            // Native third joints must follow their own index chain, not the grouped finger.
            assert!(turns[6].dot(hand * root * first * second * third * root.inverse()).abs() > 1.0 - 1e-6);
            for joint in 2..8 {
                let parent = parents[joint].unwrap();
                assert!(
                    (positions[joint].distance(positions[parent]) - heads[joint].distance(heads[parent])).abs() < 1e-6
                );
            }
        }
    }

    #[test]
    fn skeletons_without_a_fitted_grip_keep_the_existing_solver_fallback() {
        let (mut skeleton, _, _, _, _) = grip_reference_fixture();
        skeleton.clips.clear();
        let mut bind = Vec::new();
        skeleton.global_transforms(&skeleton.rest_pose(), &mut bind);
        assert!(GripReference::new(&skeleton, 1, 8, &bind).is_none());
    }

    #[test]
    fn native_thumb_closes_toward_the_grip_without_stretching_and_opens_at_rest() {
        let axes =
            HandAxes::from_bind(Vec3::new(0.06444, -0.06102, -0.03815), Vec3::new(-0.02402, -0.03987, -0.04809), true)
                .unwrap();
        let phalanges = [
            Vec3::new(0.00898, -0.02352, -0.01999),
            Vec3::new(0.00866, -0.02622, -0.02030),
            Vec3::new(0.01466, -0.02843, -0.01231),
        ];
        let grip = Vec3::new(0.07054, -0.04275, 0.00915);
        let contact = aim(phalanges[0].normalize(), axes.palm, grip.normalize(), axes.palm);
        let hand = rotation_of(Vec3::new(-17.0, 26.0, 11.0));
        let closed = axes.thumb_turn(hand, true, FIST, Some(contact));
        let aimed = (closed * phalanges[0]).normalize();
        assert!(aimed.distance(hand * grip.normalize()) < 1e-6);
        for segment in phalanges {
            assert!(((closed * segment).length() - segment.length()).abs() < 1e-6);
        }
        let open = axes.thumb_turn(hand, true, OPEN, Some(contact));
        assert!(open.dot(hand).abs() > 1.0 - 1e-6);
        let free_hand = axes.thumb_turn(hand, false, OPEN, Some(Quat::IDENTITY));
        assert!(free_hand.dot(hand).abs() > 1.0 - 1e-6);
    }

    #[test]
    fn thumbs_without_native_phalanges_keep_the_existing_curl() {
        let (sin, cos) = 35f32.to_radians().sin_cos();
        let hand = rotation_of(Vec3::new(13.0, -24.0, 7.0));
        for right in [true, false] {
            let sign = if right { 1.0 } else { -1.0 };
            let direction = Vec3::new(sign * sin, -cos, 0.0);
            let axes = HandAxes::from_bind(direction * 0.09, direction * 0.03 + Vec3::NEG_Z * 0.036, right).unwrap();
            let palm = Vec3::new(-sign * cos, -sin, 0.0);
            for curl in [FIST, OPEN] {
                let old = hand
                    * Quat::from_rotation_z((-sign * curl[2]).to_radians())
                    * Quat::from_axis_angle(palm, (sign * curl[3]).to_radians());
                let current = axes.thumb_turn(hand, right, curl, None);
                assert!(current.dot(old).abs() > 1.0 - 1e-6);
            }
        }
    }

    #[test]
    fn planar_bind_hands_preserve_the_original_mirrored_curl_axes() {
        let (sin, cos) = 35f32.to_radians().sin_cos();
        for right in [true, false] {
            let sign = if right { 1.0 } else { -1.0 };
            let direction = Vec3::new(sign * sin, -cos, 0.0);
            let expected_palm = Vec3::new(-sign * cos, -sin, 0.0);
            let axes = HandAxes::from_bind(direction * 0.09, direction * 0.03 + Vec3::NEG_Z * 0.036, right).unwrap();
            assert!(axes.fingers.distance(direction) < 1e-6);
            assert!(axes.palm.distance(expected_palm) < 1e-6);
            assert!(axes.bend.distance(Vec3::NEG_Z * sign) < 1e-6);
            for degrees in FIST.into_iter().chain(OPEN) {
                let old = Quat::from_rotation_z((-sign * degrees).to_radians());
                let derived = Quat::from_axis_angle(axes.bend, degrees.to_radians());
                assert!(old.dot(derived).abs() > 1.0 - 1e-6);
            }
            if right {
                let old = frame(Vec3::NEG_Z, -direction + expected_palm * 0.3);
                let derived = frame(axes.bend, -axes.fingers + axes.palm * 0.3);
                assert!((old - derived).to_cols_array().iter().all(|difference| difference.abs() < 1e-6));
            }
        }
    }

    #[test]
    fn hand_axes_and_grip_follow_a_spatially_rotated_bind_pose() {
        let direction = Vec3::new(0.57, -0.82, 0.0).normalize();
        let finger_offset = direction * 0.09;
        let thumb_offset = direction * 0.03 + Vec3::NEG_Z * 0.036;
        let turn = rotation_of(Vec3::new(27.0, -39.0, 16.0));
        for right in [true, false] {
            let base = HandAxes::from_bind(finger_offset, thumb_offset, right).unwrap();
            let rotated = HandAxes::from_bind(turn * finger_offset, turn * thumb_offset, right).unwrap();
            assert!(rotated.fingers.distance(turn * base.fingers) < 1e-6);
            assert!(rotated.palm.distance(turn * base.palm) < 1e-6);
            assert!(rotated.bend.distance(turn * base.bend) < 1e-6);
            let curl = Quat::from_axis_angle(base.bend, FIST[0].to_radians());
            let rotated_curl = Quat::from_axis_angle(rotated.bend, FIST[0].to_radians());
            assert!((rotated_curl * rotated.fingers).distance(turn * (curl * base.fingers)) < 1e-6);
            if right {
                let held = frame(Vec3::new(0.75, -0.28, -0.60), Vec3::Y);
                let base_grip = held * frame(base.bend, -base.fingers + base.palm * 0.3).transpose();
                let rotated_grip = held * frame(rotated.bend, -rotated.fingers + rotated.palm * 0.3).transpose();
                assert!((rotated_grip * rotated.fingers).distance(base_grip * base.fingers) < 1e-6);
                assert!((rotated_grip * rotated.palm).distance(base_grip * base.palm) < 1e-6);
            }
        }
    }

    #[test]
    fn bent_bind_limbs_keep_their_orientation_when_aimed_at_rest() {
        // Bind limbs need not lie in the XY plane or start straight.
        for (upper, lower, fallback) in [
            (Vec3::new(0.171, -0.194, 0.014), Vec3::new(0.170, -0.141, -0.106), Vec3::Z),
            (Vec3::new(-0.056, -0.405, 0.034), Vec3::new(-0.033, -0.418, -0.046), Vec3::NEG_Z),
        ] {
            let rest = hinge_poles(upper, lower, fallback);
            for (direction, pole) in [(upper, rest.0), (lower, rest.1)] {
                let at_rest = aim(direction.normalize(), pole, direction, pole);
                assert!(at_rest.dot(Quat::IDENTITY).abs() > 1.0 - 1e-6, "{at_rest:?}");
            }
            // Moving the entire bind limb rigidly must not add axial twist to either segment.
            let turn = rotation_of(Vec3::new(23.0, -41.0, 17.0));
            let target = hinge_poles(turn * upper, turn * lower, turn * fallback);
            for (direction, rest_pole, target_pole) in [(upper, rest.0, target.0), (lower, rest.1, target.1)] {
                let aimed = aim(direction.normalize(), rest_pole, turn * direction, target_pole);
                assert!(aimed.dot(turn).abs() > 1.0 - 1e-6, "{aimed:?} != {turn:?}");
            }
            let old_aim = aim(upper.normalize(), fallback, upper, rest.0);
            assert!(old_aim.dot(Quat::IDENTITY).abs() < 0.99, "the fixture must expose the fixed-pole twist");
        }
    }

    #[test]
    fn straight_bind_limbs_keep_the_original_pole_fallback() {
        let direction = Vec3::new(0.57, -0.82, 0.0).normalize();
        for fallback in [Vec3::Z, Vec3::NEG_Z] {
            let upper = direction * 0.30;
            let lower = direction * 0.27;
            let rest = hinge_poles(upper, lower, fallback);
            assert_eq!(rest, (fallback, fallback));
            let turn = rotation_of(Vec3::new(-19.0, 34.0, 12.0));
            let target = hinge_poles(turn * upper, turn * lower, turn * fallback);
            for (segment, rest_pole, target_pole) in [(upper, rest.0, target.0), (lower, rest.1, target.1)] {
                let at_rest = aim(segment.normalize(), rest_pole, segment, rest_pole);
                assert!(at_rest.dot(Quat::IDENTITY).abs() > 1.0 - 1e-6, "{at_rest:?}");
                let aimed = aim(segment.normalize(), rest_pole, turn * segment, target_pole);
                assert!(aimed.dot(turn).abs() > 1.0 - 1e-6, "{aimed:?} != {turn:?}");
            }
        }
    }

    #[test]
    fn curves_go_through_their_keys_without_overshooting() {
        let keys = [(0.0, Vec3::ZERO), (1.0, Vec3::X), (2.0, Vec3::X), (3.0, Vec3::ZERO)];
        let curve = Curve::new(&keys, false);
        assert_eq!(curve.at(1.0), Vec3::X);
        assert_eq!(curve.at(-1.0), Vec3::ZERO);
        assert_eq!(curve.at(9.0), Vec3::ZERO);
        for k in 0..=300 {
            let x = curve.at(k as f32 / 100.0).x;
            assert!((-1e-5..=1.00001).contains(&x), "{x}");
        }
    }

    #[test]
    fn two_keys_at_one_moment_leave_the_last() {
        let curve = Curve::new(&[(0.5, Vec3::X), (0.5, Vec3::Y)], false);
        assert_eq!(curve.at(0.5), Vec3::Y);
    }

    #[test]
    fn a_spring_overshoots_where_a_move_stops_and_settles() {
        let clip = PoseClip {
            duration: 1.0,
            looping: false,
            springs: true,
            tracks: BTreeMap::from([(Control::Torso, vec![(0.0, Vec3::ZERO), (0.1, Vec3::Y * 10.0)])]),
            events: Vec::new(),
        };
        let sampler = PoseSampler::new(&clip);
        let peak = (0..60).map(|k| sampler.sample(k as f32 / 60.0)[Control::Torso].y).fold(0.0, f32::max);
        assert!(peak > 10.5, "{peak}");
        assert!((sampler.sample(1.0)[Control::Torso].y - 10.0).abs() < 0.1);
        let raw = PoseSampler::with_springs(&clip, false);
        assert_eq!(raw.sample(1.0)[Control::Torso].y, 10.0);
    }

    #[test]
    fn a_control_nobody_keyed_has_its_default_and_feet_have_no_spring() {
        let clip =
            PoseClip { duration: 1.0, looping: false, springs: true, tracks: BTreeMap::new(), events: Vec::new() };
        let controls = PoseSampler::new(&clip).sample(0.5);
        assert_eq!(controls[Control::FootL], Control::FootL.default_value());
        assert_eq!(Control::FootL.spring(), None);
        assert_eq!(Control::Root.spring(), None);
    }

    #[test]
    fn a_loop_ends_where_it_starts() {
        let clip = PoseClip {
            duration: 1.0,
            looping: true,
            springs: true,
            tracks: BTreeMap::from([(
                Control::Head,
                vec![(0.0, Vec3::ZERO), (0.5, Vec3::Y * 10.0), (1.0, Vec3::ZERO)],
            )]),
            events: Vec::new(),
        };
        let sampler = PoseSampler::new(&clip);
        let (start, end) = (sampler.sample(0.0)[Control::Head], sampler.sample(1.0)[Control::Head]);
        assert!(start.distance(end) < 1e-3, "{start} {end}");
    }

    #[test]
    fn clips_read_back_what_was_written() {
        let clip = PoseClip {
            duration: 0.5,
            looping: false,
            springs: true,
            tracks: BTreeMap::from([(Control::HandR, vec![(0.0, Vec3::new(0.1, 0.2, 0.3))])]),
            events: vec![(0.25, "hit_on".to_string())],
        };
        let text = ron::to_string(&clip).unwrap();
        assert_eq!(ron::from_str::<PoseClip>(&text).unwrap(), clip);
        let short: PoseClip = ron::from_str("(duration: 1.0, tracks: {foot_l: [(0.0, (0.0, 0.1, 0.0))]})").unwrap();
        assert!(short.springs && !short.looping);
    }
}
