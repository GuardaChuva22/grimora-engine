//! Skeletal animation: a [`Skeleton`] with its [`AnimationClip`]s, and an
//! [`Animator`] that plays clips, crossfades between them and produces the
//! joint matrices the renderer skins vertices with.
//!
//! Everything is in the mesh's model space; the object's transform is applied
//! on top by the renderer.

use std::sync::Arc;

use glam::{Mat4, Quat, Vec3};

/// Vertices store joint indices as bytes, so a skeleton has at most this many joints.
pub const MAX_JOINTS: usize = 256;

/// A joint's local translation, rotation and scale relative to its parent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JointTransform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl JointTransform {
    pub const IDENTITY: Self = Self { translation: Vec3::ZERO, rotation: Quat::IDENTITY, scale: Vec3::ONE };

    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }

    /// Blends towards `other` (`t` = 0 gives `self`, 1 gives `other`).
    pub fn lerp(&self, other: &Self, t: f32) -> Self {
        Self {
            translation: self.translation.lerp(other.translation, t),
            rotation: self.rotation.slerp(other.rotation, t),
            scale: self.scale.lerp(other.scale, t),
        }
    }
}

impl Default for JointTransform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

#[derive(Debug, Clone)]
pub struct Joint {
    pub name: String,
    pub parent: Option<usize>,
    /// Local transform in the bind pose (what's drawn without animation).
    pub rest: JointTransform,
    /// Takes model-space vertices into the joint's space at bind time.
    pub inverse_bind: Mat4,
}

/// Joint hierarchy plus the clips that animate it.
#[derive(Debug, Clone, Default)]
pub struct Skeleton {
    pub joints: Vec<Joint>,
    /// Transform above the root joints (an armature node's rotation or scale, say).
    pub root: Mat4,
    pub clips: Vec<AnimationClip>,
    /// Joint indices with parents before children.
    order: Vec<usize>,
}

impl Skeleton {
    /// Builds a skeleton. Parent links that would form a cycle are cut.
    pub fn new(mut joints: Vec<Joint>, root: Mat4, clips: Vec<AnimationClip>) -> Self {
        let n = joints.len();
        for joint in &mut joints {
            joint.parent = joint.parent.filter(|&p| p < n);
        }
        // Depth-first order from the roots; joints left over sit in a cycle.
        let mut children = vec![Vec::new(); n];
        for (i, joint) in joints.iter().enumerate() {
            if let Some(p) = joint.parent {
                children[p].push(i);
            }
        }
        let mut order = Vec::with_capacity(n);
        let mut stack: Vec<usize> = (0..n).filter(|&i| joints[i].parent.is_none()).rev().collect();
        while let Some(i) = stack.pop() {
            order.push(i);
            stack.extend(children[i].iter().rev());
        }
        if order.len() < n {
            let mut placed = vec![false; n];
            order.iter().for_each(|&i| placed[i] = true);
            for i in (0..n).filter(|&i| !placed[i]) {
                joints[i].parent = None;
                order.push(i);
            }
        }
        Self { joints, root, clips, order }
    }

    pub fn joint(&self, name: &str) -> Option<usize> {
        self.joints.iter().position(|j| j.name == name)
    }

    /// Finds a clip by name, ignoring case if there's no exact match.
    pub fn clip(&self, name: &str) -> Option<usize> {
        self.clips
            .iter()
            .position(|c| c.name == name)
            .or_else(|| self.clips.iter().position(|c| c.name.eq_ignore_ascii_case(name)))
    }

    pub fn rest_pose(&self) -> Vec<JointTransform> {
        self.joints.iter().map(|j| j.rest).collect()
    }

    /// Model-space transform of every joint for a local `pose`.
    pub fn global_transforms(&self, pose: &[JointTransform], out: &mut Vec<Mat4>) {
        out.clear();
        out.resize(self.joints.len(), Mat4::IDENTITY);
        for &i in &self.order {
            let local = pose.get(i).unwrap_or(&self.joints[i].rest).matrix();
            out[i] = match self.joints[i].parent {
                Some(p) => out[p] * local,
                None => self.root * local,
            };
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interpolation {
    /// Holds each key until the next one.
    Step,
    Linear,
}

/// Key values for one joint property.
#[derive(Debug, Clone)]
pub enum Keyframes {
    Translation(Vec<Vec3>),
    Rotation(Vec<Quat>),
    Scale(Vec<Vec3>),
}

/// Animation of one property of one joint.
#[derive(Debug, Clone)]
pub struct Channel {
    pub joint: usize,
    pub interpolation: Interpolation,
    /// Seconds, increasing; one per key.
    pub times: Vec<f32>,
    pub keys: Keyframes,
}

#[derive(Debug, Clone)]
pub struct AnimationClip {
    pub name: String,
    /// Seconds.
    pub duration: f32,
    pub channels: Vec<Channel>,
}

impl AnimationClip {
    /// Writes the clip's values at `time` (seconds, clamped to the clip) into `pose`.
    /// Joints the clip doesn't animate keep their current value.
    pub fn sample(&self, time: f32, pose: &mut [JointTransform]) {
        for channel in &self.channels {
            let Some(target) = pose.get_mut(channel.joint) else { continue };
            let Some((a, b, t)) = channel.keys_at(time) else { continue };
            match &channel.keys {
                Keyframes::Translation(v) => target.translation = v[a].lerp(v[b], t),
                Keyframes::Rotation(v) => target.rotation = v[a].slerp(v[b], t).normalize(),
                Keyframes::Scale(v) => target.scale = v[a].lerp(v[b], t),
            }
        }
    }
}

impl Channel {
    fn key_count(&self) -> usize {
        let values = match &self.keys {
            Keyframes::Translation(v) | Keyframes::Scale(v) => v.len(),
            Keyframes::Rotation(v) => v.len(),
        };
        values.min(self.times.len())
    }

    /// The two keys around `time` and the blend factor between them.
    fn keys_at(&self, time: f32) -> Option<(usize, usize, f32)> {
        let n = self.key_count();
        if n == 0 {
            return None;
        }
        let next = self.times[..n].partition_point(|&k| k <= time);
        if next == 0 {
            return Some((0, 0, 0.0));
        }
        if next == n {
            return Some((n - 1, n - 1, 0.0));
        }
        let (t0, t1) = (self.times[next - 1], self.times[next]);
        let t = match self.interpolation {
            Interpolation::Step => 0.0,
            Interpolation::Linear => ((time - t0) / (t1 - t0).max(1e-6)).clamp(0.0, 1.0),
        };
        Some((next - 1, next, t))
    }
}

#[derive(Debug, Clone, Copy)]
struct Track {
    clip: usize,
    time: f32,
    looping: bool,
}

/// Plays a skeleton's clips: looping or once, with crossfades between them.
///
/// Call [`Animator::update`] once per frame (the engine does it for animators
/// on world objects), then read [`Animator::skinning_matrices`].
#[derive(Debug, Clone)]
pub struct Animator {
    skeleton: Arc<Skeleton>,
    current: Option<Track>,
    /// Clip being faded out, with the fade's remaining and total seconds.
    previous: Option<(Track, f32, f32)>,
    /// Playback rate; 1 is normal speed.
    pub speed: f32,
    pose: Vec<JointTransform>,
    scratch: Vec<JointTransform>,
    globals: Vec<Mat4>,
    matrices: Vec<Mat4>,
}

impl Animator {
    /// Starts in the bind pose with nothing playing.
    pub fn new(skeleton: Arc<Skeleton>) -> Self {
        let mut animator = Self {
            pose: skeleton.rest_pose(),
            scratch: Vec::new(),
            skeleton,
            current: None,
            previous: None,
            speed: 1.0,
            globals: Vec::new(),
            matrices: Vec::new(),
        };
        animator.evaluate();
        animator
    }

    pub fn skeleton(&self) -> &Arc<Skeleton> {
        &self.skeleton
    }

    /// Loops the clip `name`, fading from the current one over `fade` seconds.
    /// Does nothing if it is already playing. Returns false if there's no such clip.
    pub fn play(&mut self, name: &str, fade: f32) -> bool {
        self.start(name, fade, true)
    }

    /// Plays the clip `name` once and holds its last frame (see [`Animator::finished`]).
    pub fn play_once(&mut self, name: &str, fade: f32) -> bool {
        self.start(name, fade, false)
    }

    fn start(&mut self, name: &str, fade: f32, looping: bool) -> bool {
        let Some(clip) = self.skeleton.clip(name) else { return false };
        if let Some(current) = &mut self.current
            && current.clip == clip
        {
            current.looping = looping;
            return true;
        }
        self.previous = match self.current.take() {
            Some(old) if fade > 0.0 => Some((old, fade, fade)),
            _ => None,
        };
        self.current = Some(Track { clip, time: 0.0, looping });
        true
    }

    /// Back to the bind pose.
    pub fn stop(&mut self) {
        self.current = None;
        self.previous = None;
        self.pose = self.skeleton.rest_pose();
        self.evaluate();
    }

    /// Name of the clip playing (or holding its last frame).
    pub fn current(&self) -> Option<&str> {
        self.current.map(|t| self.skeleton.clips[t.clip].name.as_str())
    }

    /// Seconds into the current clip.
    pub fn time(&self) -> f32 {
        self.current.map_or(0.0, |t| t.time)
    }

    /// A clip started with [`Animator::play_once`] has reached its end.
    pub fn finished(&self) -> bool {
        self.current.is_some_and(|t| !t.looping && t.time >= self.skeleton.clips[t.clip].duration)
    }

    /// Advances playback by `dt` seconds and recomputes the pose.
    pub fn update(&mut self, dt: f32) {
        let dt = dt * self.speed;
        let clips = &self.skeleton.clips;
        if let Some(track) = &mut self.current {
            advance(track, dt, clips[track.clip].duration);
        }
        if let Some((track, remaining, _)) = &mut self.previous {
            advance(track, dt, clips[track.clip].duration);
            *remaining -= dt.abs();
            if *remaining <= 0.0 {
                self.previous = None;
            }
        }

        let Some(current) = self.current else { return };
        self.pose.clear();
        self.pose.extend(self.skeleton.joints.iter().map(|j| j.rest));
        clips[current.clip].sample(current.time, &mut self.pose);
        if let Some((old, remaining, total)) = self.previous {
            self.scratch.clear();
            self.scratch.extend(self.skeleton.joints.iter().map(|j| j.rest));
            clips[old.clip].sample(old.time, &mut self.scratch);
            // Smoothstep makes the fade ease in and out.
            let t = 1.0 - (remaining / total).clamp(0.0, 1.0);
            let w = t * t * (3.0 - 2.0 * t);
            for (pose, old) in self.pose.iter_mut().zip(&self.scratch) {
                *pose = old.lerp(pose, w);
            }
        }
        self.evaluate();
    }

    fn evaluate(&mut self) {
        self.skeleton.global_transforms(&self.pose, &mut self.globals);
        self.matrices.clear();
        self.matrices.extend(self.globals.iter().zip(&self.skeleton.joints).map(|(g, j)| *g * j.inverse_bind));
    }

    /// The current local pose, one transform per joint.
    pub fn pose(&self) -> &[JointTransform] {
        &self.pose
    }

    /// Joint matrices for skinning: model space, bind pose already removed.
    pub fn skinning_matrices(&self) -> &[Mat4] {
        &self.matrices
    }

    /// Model-space transform of a joint, for attaching things to it (a lantern
    /// in a hand, a hat on a head). Multiply by the object's transform for world space.
    pub fn joint_transform(&self, name: &str) -> Option<Mat4> {
        self.globals.get(self.skeleton.joint(name)?).copied()
    }
}

fn advance(track: &mut Track, dt: f32, duration: f32) {
    track.time += dt;
    if track.looping && duration > 0.0 {
        track.time = track.time.rem_euclid(duration);
    } else {
        track.time = track.time.clamp(0.0, duration.max(0.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two-joint arm along +Y with a clip that bends the elbow 90° around Z.
    fn arm() -> Arc<Skeleton> {
        let joint = |name: &str, parent, y: f32| Joint {
            name: name.into(),
            parent,
            rest: JointTransform { translation: Vec3::Y * y, ..JointTransform::IDENTITY },
            inverse_bind: Mat4::IDENTITY,
        };
        // Children listed before parents to exercise the evaluation order.
        let mut joints = vec![joint("elbow", Some(1), 1.0), joint("shoulder", None, 0.0)];
        let globals = [Mat4::from_translation(Vec3::Y), Mat4::IDENTITY];
        for (j, g) in joints.iter_mut().zip(globals) {
            j.inverse_bind = g.inverse();
        }
        let bend = AnimationClip {
            name: "Bend".into(),
            duration: 1.0,
            channels: vec![Channel {
                joint: 0,
                interpolation: Interpolation::Linear,
                times: vec![0.0, 1.0],
                keys: Keyframes::Rotation(vec![Quat::IDENTITY, Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)]),
            }],
        };
        let wave = AnimationClip {
            name: "wave".into(),
            duration: 2.0,
            channels: vec![Channel {
                joint: 1,
                interpolation: Interpolation::Step,
                times: vec![0.0, 1.0],
                keys: Keyframes::Translation(vec![Vec3::ZERO, Vec3::X]),
            }],
        };
        Arc::new(Skeleton::new(joints, Mat4::IDENTITY, vec![bend, wave]))
    }

    /// Where a model-space point bound to `joint` ends up.
    fn skinned(animator: &Animator, joint: usize, point: Vec3) -> Vec3 {
        animator.skinning_matrices()[joint].transform_point3(point)
    }

    #[test]
    fn bind_pose_skins_to_identity() {
        let animator = Animator::new(arm());
        for m in animator.skinning_matrices() {
            assert!(m.abs_diff_eq(Mat4::IDENTITY, 1e-6));
        }
    }

    #[test]
    fn samples_and_interpolates() {
        let mut animator = Animator::new(arm());
        assert!(animator.play("bend", 0.0), "case-insensitive lookup");
        animator.update(0.5);
        // The forearm tip at (0, 2, 0) rotates 45° around the elbow at (0, 1, 0).
        let tip = skinned(&animator, 0, Vec3::new(0.0, 2.0, 0.0));
        let s = std::f32::consts::FRAC_1_SQRT_2;
        assert!(tip.abs_diff_eq(Vec3::new(-s, 1.0 + s, 0.0), 1e-5), "{tip}");
        // The elbow follows the shoulder in model space.
        let elbow = animator.joint_transform("elbow").unwrap().w_axis.truncate();
        assert!(elbow.abs_diff_eq(Vec3::Y, 1e-6));
    }

    #[test]
    fn loops_and_steps() {
        let mut animator = Animator::new(arm());
        animator.play("wave", 0.0);
        animator.update(1.5);
        assert_eq!(animator.pose()[1].translation, Vec3::X, "step holds the key");
        animator.update(1.0); // 2.5 s wraps to 0.5 s
        assert!((animator.time() - 0.5).abs() < 1e-5);
        assert_eq!(animator.pose()[1].translation, Vec3::ZERO);
    }

    #[test]
    fn play_once_holds_the_last_frame() {
        let mut animator = Animator::new(arm());
        animator.play_once("Bend", 0.0);
        animator.update(3.0);
        assert!(animator.finished());
        let angle = animator.pose()[0].rotation.angle_between(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2));
        assert!(angle < 1e-4);
    }

    #[test]
    fn crossfades_between_clips() {
        let mut animator = Animator::new(arm());
        animator.play("wave", 0.0);
        animator.update(1.2); // shoulder stepped to +X
        animator.play("Bend", 1.0);
        animator.update(0.5); // halfway through the fade
        let x = animator.pose()[1].translation.x;
        assert!(x > 0.1 && x < 0.9, "blended shoulder x = {x}");
        animator.update(0.6);
        assert_eq!(animator.pose()[1].translation, Vec3::ZERO, "fade finished");
        assert_eq!(animator.current(), Some("Bend"));
        assert!(!animator.play("missing", 0.2));
    }
}
