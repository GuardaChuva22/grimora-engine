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
    /// The joint called `root`, whose travel moves the character instead of its
    /// pose: see [`Animator::root_delta`].
    motion: Option<usize>,
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
        let motion = joints.iter().position(|j| j.name == "root");
        Self { joints, root, clips, order, motion }
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

    /// Joint indices with parents before children.
    pub fn order(&self) -> &[usize] {
        &self.order
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

/// A named moment in a clip: a foot landing, a blow connecting. See [`Animator::events`].
#[derive(Debug, Clone, PartialEq)]
pub struct ClipEvent {
    /// Seconds from the start of the clip.
    pub time: f32,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct AnimationClip {
    pub name: String,
    /// Seconds.
    pub duration: f32,
    pub channels: Vec<Channel>,
    pub events: Vec<ClipEvent>,
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

    /// The translation it gives `joint` at `time`, if it animates that.
    fn translation_at(&self, joint: usize, time: f32) -> Option<Vec3> {
        self.channels.iter().filter(|c| c.joint == joint).find_map(|channel| {
            let Keyframes::Translation(v) = &channel.keys else { return None };
            let (a, b, t) = channel.keys_at(time)?;
            Some(v[a].lerp(v[b], t))
        })
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
    /// Another clip mixed in, kept at the same point of its cycle, and how much of it shows.
    mix: Option<(usize, f32)>,
}

impl Track {
    fn new(clip: usize, time: f32, looping: bool) -> Self {
        Self { clip, time, looping, mix: None }
    }
}

/// A clip on its way out, under the one that replaced it.
#[derive(Debug, Clone, Copy)]
struct Fading {
    track: Track,
    /// Its own playback rate: what comes next doesn't change how it ends.
    speed: f32,
    remaining: f32,
    total: f32,
}

impl Fading {
    /// How much of the clip above it shows. Smoothstep makes the fade ease in and out.
    fn weight(&self) -> f32 {
        let t = 1.0 - (self.remaining / self.total).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }
}

/// Clips that can be fading out at once. Reaching it takes a change every couple of
/// frames, and by then the oldest hardly shows.
const MAX_FADING: usize = 8;

/// Clips played over part of the body, the rest following the clips underneath: the arms
/// holding a guard while the legs walk.
#[derive(Debug, Clone)]
struct Overlay {
    current: Track,
    previous: Vec<Fading>,
    /// The joints it moves.
    mask: Vec<bool>,
    /// How far in it has faded, from 0 to 1, and how fast that changes per second.
    amount: f32,
    rate: f32,
}

/// The hidden clip with an empty hand: the wrist first, then the fingers, keyed twice, as it
/// hangs and as it lies flat on something.
pub const FREE_HAND_CLIP: &str = "__hand_free";
/// Seconds for a hand to open or to close on something.
const FREE_HAND_FADE: f32 = 0.22;

/// A hand that may be empty. Clips are written for a hand holding a weapon, with the forearm
/// and wrist turned to aim it, so all of that goes away while it's empty.
#[derive(Debug, Clone)]
struct FreeHand {
    hand: usize,
    forearm: usize,
    /// Along the forearm, in its own frame.
    axis: Vec3,
    /// Each joint hanging, and lying flat.
    turns: Vec<(usize, Quat, Quat)>,
    empty: bool,
    amount: f32,
    /// The clip places this hand itself, on something: it keeps its arm and only the fingers open.
    placed: bool,
    placed_amount: f32,
    /// How far the fingers are open, from closed on a grip to an empty hand's.
    fingers: f32,
    /// Closing on something it doesn't have yet.
    grasping: bool,
    /// Holds something small: the arm lets go as if empty, and the fingers stay this far open.
    loose: Option<f32>,
}

impl FreeHand {
    fn of(skeleton: &Skeleton) -> Option<Self> {
        let clip = &skeleton.clips[skeleton.clip(FREE_HAND_CLIP)?];
        let hand = clip.channels.first()?.joint;
        let forearm = skeleton.joints[hand].parent?;
        let turns = clip.channels.iter().filter_map(|channel| match &channel.keys {
            Keyframes::Rotation(keys) => Some((channel.joint, *keys.first()?, *keys.last()?)),
            _ => None,
        });
        Some(Self {
            hand,
            forearm,
            axis: skeleton.joints[hand].rest.translation.normalize_or_zero(),
            turns: turns.collect(),
            empty: false,
            amount: 0.0,
            placed: false,
            placed_amount: 0.0,
            fingers: 0.0,
            grasping: false,
            loose: None,
        })
    }

    fn apply(&mut self, skeleton: &Skeleton, pose: &mut [JointTransform], dt: f32) {
        let step = dt / FREE_HAND_FADE;
        let empty = self.empty && !self.grasping;
        let fingers = if empty { 1.0 } else { self.loose.unwrap_or(0.0) };
        let hangs = empty || self.loose.is_some();
        self.amount = (self.amount + if hangs { step } else { -step }).clamp(0.0, 1.0);
        self.fingers += (fingers - self.fingers).clamp(-step, step);
        self.placed_amount = (self.placed_amount + if self.placed { step } else { -step }).clamp(0.0, 1.0);
        if self.amount <= 0.0 && self.fingers <= 0.0 {
            return;
        }
        let smooth = |amount: f32| amount * amount * (3.0 - 2.0 * amount);
        let (weight, placed, open) = (smooth(self.amount), smooth(self.placed_amount), smooth(self.fingers));
        let hanging = weight * (1.0 - placed);
        // A forearm hangs untwisted: the elbow's bend stays and the roll around itself goes.
        let forearm = &mut pose[self.forearm].rotation;
        let bent = skeleton.joints[self.forearm].rest.rotation.inverse() * *forearm;
        let along = self.axis * Vec3::new(bent.x, bent.y, bent.z).dot(self.axis);
        let twist = Quat::from_xyzw(along.x, along.y, along.z, bent.w).normalize();
        *forearm = (*forearm * Quat::IDENTITY.slerp(twist.inverse(), hanging)).normalize();
        for &(joint, hangs, flat) in &self.turns {
            let (turn, amount) = if joint == self.hand { (hangs, hanging) } else { (hangs.slerp(flat, placed), open) };
            pose[joint].rotation = pose[joint].rotation.slerp(turn, amount);
        }
    }
}

/// Plays a skeleton's clips: looping or once, with crossfades between them. A change in
/// the middle of a fade fades from what's on screen, however many clips that takes.
///
/// Call [`Animator::update`] once per frame (the engine does it for animators
/// on world objects), then read [`Animator::skinning_matrices`].
#[derive(Debug, Clone)]
pub struct Animator {
    skeleton: Arc<Skeleton>,
    free_hand: Option<FreeHand>,
    current: Option<Track>,
    /// Clips being faded out, oldest first.
    previous: Vec<Fading>,
    overlay: Option<Overlay>,
    /// Playback rate of the clips underneath; 1 is normal speed. Overlays keep their own time.
    pub speed: f32,
    pose: Vec<JointTransform>,
    scratch: Vec<JointTransform>,
    mixing: Vec<JointTransform>,
    overlay_pose: Vec<JointTransform>,
    globals: Vec<Mat4>,
    matrices: Vec<Mat4>,
    /// Events the last update passed, as clip and event indices.
    fired: Vec<(usize, usize)>,
    root_delta: Vec3,
    /// Joints bent on top of every clip, and how far in all: see [`Animator::set_bend`].
    bend: Vec<usize>,
    bend_angle: f32,
    /// How much the overlay keeps its own orientation: see [`Animator::set_overlay_steady`].
    overlay_steady: f32,
    /// Joints the overlay leaves to the clips underneath, and how far it has let them go (0 to
    /// 1) and is going: see [`Animator::overlay_leave`].
    left_out: Vec<bool>,
    left_out_amount: f32,
    left_out_target: f32,
}

/// Seconds for the overlay to let go of a limb or take it back.
const LEAVE_FADE: f32 = 0.3;

impl Animator {
    /// Starts in the bind pose with nothing playing.
    pub fn new(skeleton: Arc<Skeleton>) -> Self {
        let mut animator = Self {
            pose: skeleton.rest_pose(),
            scratch: Vec::new(),
            mixing: Vec::new(),
            overlay_pose: Vec::new(),
            free_hand: FreeHand::of(&skeleton),
            skeleton,
            current: None,
            previous: Vec::new(),
            overlay: None,
            speed: 1.0,
            globals: Vec::new(),
            matrices: Vec::new(),
            fired: Vec::new(),
            root_delta: Vec3::ZERO,
            bend: Vec::new(),
            bend_angle: 0.0,
            overlay_steady: 0.0,
            left_out: Vec::new(),
            left_out_amount: 0.0,
            left_out_target: 0.0,
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

    /// Plays the clip `name` once from its start, even if it is already playing.
    pub fn replay_once(&mut self, name: &str, fade: f32) -> bool {
        let Some(clip) = self.skeleton.clip(name) else { return false };
        self.switch_to(Track::new(clip, 0.0, false), fade);
        true
    }

    /// Like [`Animator::play`], picking up at the same point of the cycle as the clip
    /// that was looping, so that the feet keep their step from one gait to another.
    pub fn play_in_step(&mut self, name: &str, fade: f32) -> bool {
        let Some(clip) = self.skeleton.clip(name) else { return false };
        if self.current.is_some_and(|track| track.clip == clip && track.mix.is_none()) {
            return true;
        }
        self.switch_to(Track::new(clip, self.time_in_step(clip), true), fade);
        true
    }

    /// Loops `name` mixed with `other` by `weight`, from 0 (all `name`) to 1 (all `other`), both
    /// at the same point of their cycles: walking on a diagonal, say. It can be called every
    /// frame with a new weight, and changing either clip fades and keeps the step.
    pub fn play_mixed(&mut self, name: &str, other: &str, weight: f32, fade: f32) -> bool {
        let (Some(clip), Some(other)) = (self.skeleton.clip(name), self.skeleton.clip(other)) else { return false };
        let mix = Some((other, weight.clamp(0.0, 1.0)));
        if let Some(track) = &mut self.current
            && track.clip == clip
            && track.mix.is_some_and(|(already, _)| already == other)
        {
            track.mix = mix;
            track.looping = true;
            return true;
        }
        self.switch_to(Track { mix, ..Track::new(clip, self.time_in_step(clip), true) }, fade);
        true
    }

    /// Where `clip` would be at the point of the cycle that the current clip is at, if it loops.
    fn time_in_step(&self, clip: usize) -> f32 {
        match self.current {
            Some(track) if track.looping => {
                let clips = &self.skeleton.clips;
                track.time / clips[track.clip].duration.max(1e-3) * clips[clip].duration
            }
            _ => 0.0,
        }
    }

    fn start(&mut self, name: &str, fade: f32, looping: bool) -> bool {
        let Some(clip) = self.skeleton.clip(name) else { return false };
        if let Some(current) = &mut self.current
            && current.clip == clip
            && current.mix.is_none()
        {
            current.looping = looping;
            return true;
        }
        self.switch_to(Track::new(clip, 0.0, looping), fade);
        true
    }

    fn switch_to(&mut self, track: Track, fade: f32) {
        match self.current.take() {
            Some(old) if fade > 0.0 => push_fading(&mut self.previous, old, self.speed, fade),
            _ => self.previous.clear(),
        }
        self.current = Some(track);
    }

    /// Loops `name` over `from` and the joints under it, fading in over `fade` seconds. Does
    /// nothing if it's already looping there.
    pub fn overlay_loop(&mut self, name: &str, from: &str, fade: f32) -> bool {
        let Some(clip) = self.skeleton.clip(name) else { return false };
        let already = self.overlay.as_ref().is_some_and(|over| over.current.clip == clip && over.rate >= 0.0);
        already || self.start_overlay(Track::new(clip, 0.0, true), from, fade)
    }

    /// Plays `name` once from its start over `from` and the joints under it, then holds its
    /// last frame (see [`Animator::overlay_finished`]).
    pub fn overlay_once(&mut self, name: &str, from: &str, fade: f32) -> bool {
        let Some(clip) = self.skeleton.clip(name) else { return false };
        self.start_overlay(Track::new(clip, 0.0, false), from, fade)
    }

    fn start_overlay(&mut self, track: Track, from: &str, fade: f32) -> bool {
        let Some(joint) = self.skeleton.joint(from) else { return false };
        let rate = 1.0 / fade.max(1e-3);
        match &mut self.overlay {
            Some(over) => {
                push_fading(&mut over.previous, over.current, 1.0, fade.max(1e-3));
                over.current = track;
                over.rate = rate;
            }
            None => {
                let mut mask = vec![false; self.skeleton.joints.len()];
                for &j in self.skeleton.order() {
                    mask[j] = j == joint || self.skeleton.joints[j].parent.is_some_and(|parent| mask[parent]);
                }
                self.overlay = Some(Overlay { current: track, previous: Vec::new(), mask, amount: 0.0, rate });
            }
        }
        true
    }

    /// Fades the overlay away over `fade` seconds, leaving the clips underneath.
    pub fn overlay_stop(&mut self, fade: f32) {
        if let Some(over) = &mut self.overlay {
            over.rate = over.rate.min(-1.0 / fade.max(1e-3));
        }
    }

    /// Name of the clip playing over the body, unless it's fading away.
    pub fn overlay_current(&self) -> Option<&str> {
        let over = self.overlay.as_ref().filter(|over| over.rate >= 0.0)?;
        Some(self.skeleton.clips[over.current.clip].name.as_str())
    }

    /// Seconds into the clip playing over the body.
    pub fn overlay_time(&self) -> f32 {
        self.overlay.as_ref().map_or(0.0, |over| over.current.time)
    }

    /// The clip over the body, started with [`Animator::overlay_once`], has reached its end.
    pub fn overlay_finished(&self) -> bool {
        self.overlay.as_ref().is_some_and(|over| {
            !over.current.looping && over.current.time >= self.skeleton.clips[over.current.clip].duration
        })
    }

    /// Jumps to the end of a clip played once, firing nothing.
    pub fn skip_to_end(&mut self) {
        if let Some(track) = &mut self.current {
            track.time = self.skeleton.clips[track.clip].duration;
        }
    }

    /// Back to the bind pose.
    pub fn stop(&mut self) {
        self.current = None;
        self.previous.clear();
        self.overlay = None;
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

    /// Names of the events the last [`Animator::update`] passed. Only the current
    /// clip fires them, not one fading out, and not while it plays backwards.
    pub fn events(&self) -> impl Iterator<Item = &str> {
        self.fired.iter().map(|&(clip, event)| self.skeleton.clips[clip].events[event].name.as_str())
    }

    /// The last [`Animator::update`] passed an event called `name`.
    pub fn fired(&self, name: &str) -> bool {
        self.events().any(|event| event == name)
    }

    /// How far the clip moved its `root` joint in the last [`Animator::update`], in
    /// model space. The pose itself stays put: the game moves the character by
    /// this, so a step or a roll is stopped by walls like any other movement.
    pub fn root_delta(&self) -> Vec3 {
        self.root_delta
    }

    /// Tells the hand of a [`FREE_HAND_CLIP`] whether it has anything in it: an empty one lets
    /// go, whatever the clip says, and the other closes the way the clip has it. The world
    /// does this for whatever is attached to a joint.
    pub fn set_hand_empty(&mut self, empty: bool) {
        if let Some(free) = &mut self.free_hand {
            free.empty = empty;
        }
    }

    /// The clip playing puts the empty hand on something, with its own arm and wrist: only
    /// the fingers open, lying flat.
    pub fn set_hand_placed(&mut self, placed: bool) {
        if let Some(free) = &mut self.free_hand {
            free.placed = placed;
        }
    }

    /// The hand closes as if it had something, ahead of getting it.
    pub fn set_hand_grasping(&mut self, grasping: bool) {
        if let Some(free) = &mut self.free_hand {
            free.grasping = grasping;
        }
    }

    /// The hand has something small: its arm moves as if it were empty, whatever the clip's grip
    /// says, and the fingers stay this far open (0 to 1). `None` is a hand closed on a grip.
    pub fn set_hand_loose(&mut self, open: Option<f32>) {
        if let Some(free) = &mut self.free_hand {
            free.loose = open;
        }
    }

    /// Whether `joint` is the free hand or something it carries.
    pub fn in_free_hand(&self, joint: &str) -> bool {
        let (Some(free), Some(mut at)) = (&self.free_hand, self.skeleton.joint(joint)) else { return false };
        loop {
            if at == free.hand {
                return true;
            }
            match self.skeleton.joints[at].parent {
                Some(parent) => at = parent,
                None => return false,
            }
        }
    }

    /// Advances playback by `dt` seconds and recomputes the pose.
    pub fn update(&mut self, dt: f32) {
        let clips = &self.skeleton.clips;
        self.fired.clear();
        self.root_delta = Vec3::ZERO;
        if let Some(track) = &mut self.current {
            let played = dt * self.speed;
            passed_events(track, &clips[track.clip], played, &mut self.fired);
            let before = track.time;
            advance(track, played, clips[track.clip].duration);
            if let Some(joint) = self.skeleton.motion
                && track.time > before
                && let (Some(from), Some(to)) = (
                    clips[track.clip].translation_at(joint, before),
                    clips[track.clip].translation_at(joint, track.time),
                )
            {
                self.root_delta = self.skeleton.root.transform_vector3(to - from);
            }
        }
        advance_fades(&mut self.previous, dt, clips);

        let Some(current) = self.current else { return };
        blend_stack(&self.skeleton, &self.previous, current, &mut self.pose, &mut self.scratch, &mut self.mixing);
        if let Some(over) = &mut self.overlay {
            let duration = clips[over.current.clip].duration;
            advance(&mut over.current, dt, duration);
            advance_fades(&mut over.previous, dt, clips);
            over.amount = (over.amount + over.rate * dt).clamp(0.0, 1.0);
            blend_stack(
                &self.skeleton,
                &over.previous,
                over.current,
                &mut self.overlay_pose,
                &mut self.scratch,
                &mut self.mixing,
            );
            let weight = over.amount * over.amount * (3.0 - 2.0 * over.amount);
            let step = dt / LEAVE_FADE;
            self.left_out_amount += (self.left_out_target - self.left_out_amount).clamp(-step, step);
            let free = self.left_out_amount * self.left_out_amount * (3.0 - 2.0 * self.left_out_amount);
            let joints = &self.skeleton.joints;
            let mut held = Vec::new();
            if self.overlay_steady > 0.0 {
                // The overlay's top joints turn by what the clip underneath has done above them.
                let chain = |pose: &[JointTransform], mut at: Option<usize>| {
                    let mut turn = Quat::IDENTITY;
                    while let Some(index) = at {
                        turn = pose[index].rotation * turn;
                        at = joints[index].parent;
                    }
                    turn
                };
                for (j, joint) in joints.iter().enumerate() {
                    if over.mask[j] && joint.parent.is_some_and(|parent| !over.mask[parent]) {
                        let wanted = chain(&self.pose, joint.parent).inverse()
                            * chain(&self.overlay_pose, joint.parent)
                            * self.overlay_pose[j].rotation;
                        held.push((j, wanted));
                    }
                }
            }
            for (j, pose) in self.pose.iter_mut().enumerate() {
                if over.mask[j] {
                    let kept = if self.left_out.get(j).copied().unwrap_or(false) { 1.0 - free } else { 1.0 };
                    *pose = pose.lerp(&self.overlay_pose[j], weight * kept);
                }
            }
            for (j, wanted) in held {
                self.pose[j].rotation = self.pose[j].rotation.slerp(wanted, weight * self.overlay_steady);
            }
            if over.amount <= 0.0 && over.rate < 0.0 {
                self.overlay = None;
            }
        }
        if let Some(free) = &mut self.free_hand {
            free.apply(&self.skeleton, &mut self.pose, dt);
        }
        self.apply_bend();
        if let Some(joint) = self.skeleton.motion {
            self.pose[joint].translation = self.skeleton.joints[joint].rest.translation;
        }
        self.evaluate();
    }

    /// Bends `joints`, parents first, by `angle` radians in all around the character's own
    /// right, on top of whatever is playing: up when positive. For aiming up or down with a
    /// pose written looking straight ahead. An angle of 0 turns it off.
    pub fn set_bend(&mut self, joints: &[&str], angle: f32) {
        self.bend_angle = angle;
        if self.bend.len() != joints.len() {
            self.bend = joints.iter().filter_map(|name| self.skeleton.joint(name)).collect();
        }
    }

    /// With 1, an overlay faces the way it was written whatever the clip underneath does to the
    /// hips: for aiming, where a sway below would throw the barrel off. 0 lets it ride on them.
    pub fn set_overlay_steady(&mut self, steady: f32) {
        self.overlay_steady = steady.clamp(0.0, 1.0);
    }

    /// Leaves a joint and everything under it to the clips underneath, whatever overlay plays:
    /// a free arm swinging with the walk while the other holds a gun. `None` gives it back to the
    /// overlay. Either way it changes over a moment, not at once.
    pub fn overlay_leave(&mut self, from: Option<&str>) {
        match from.and_then(|name| self.skeleton.joint(name)) {
            Some(joint) => {
                let mut mask = vec![false; self.skeleton.joints.len()];
                for &j in self.skeleton.order() {
                    mask[j] = j == joint || self.skeleton.joints[j].parent.is_some_and(|parent| mask[parent]);
                }
                if mask != self.left_out {
                    // Another limb: it starts from the overlay, as nothing else was let go.
                    self.left_out = mask;
                    self.left_out_amount = 0.0;
                }
                self.left_out_target = 1.0;
            }
            None => self.left_out_target = 0.0,
        }
    }

    fn apply_bend(&mut self) {
        if self.bend_angle == 0.0 || self.bend.is_empty() {
            return;
        }
        let share = self.bend_angle / self.bend.len() as f32;
        let (_, root, _) = self.skeleton.root.to_scale_rotation_translation();
        for at in 0..self.bend.len() {
            let joint = self.bend[at];
            let mut above = Quat::IDENTITY;
            let mut parent = self.skeleton.joints[joint].parent;
            while let Some(index) = parent {
                above = self.pose[index].rotation * above;
                parent = self.skeleton.joints[index].parent;
            }
            let axis = (root * above).inverse() * Vec3::X;
            self.pose[joint].rotation =
                Quat::from_axis_angle(axis.normalize_or_zero(), share) * self.pose[joint].rotation;
        }
    }

    fn evaluate(&mut self) {
        self.skeleton.global_transforms(&self.pose, &mut self.globals);
        self.matrices.clear();
        self.matrices.extend(self.globals.iter().zip(&self.skeleton.joints).map(|(g, j)| *g * j.inverse_bind));
    }

    /// Shows `pose`, one local transform per joint, and stops whatever was playing: for
    /// tools that work out their own poses.
    pub fn set_pose(&mut self, pose: &[JointTransform]) {
        self.current = None;
        self.previous.clear();
        self.overlay = None;
        self.pose.clear();
        self.pose.extend_from_slice(pose);
        self.pose.resize(self.skeleton.joints.len(), JointTransform::IDENTITY);
        self.evaluate();
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

/// One clip's pose at the track's time, on top of the rest pose, with what's mixed in.
fn sample_track(skeleton: &Skeleton, track: Track, out: &mut Vec<JointTransform>, mixing: &mut Vec<JointTransform>) {
    out.clear();
    out.extend(skeleton.joints.iter().map(|j| j.rest));
    let clip = &skeleton.clips[track.clip];
    clip.sample(track.time, out);
    if let Some((other, weight)) = track.mix {
        let other = &skeleton.clips[other];
        mixing.clear();
        mixing.extend(skeleton.joints.iter().map(|j| j.rest));
        other.sample(track.time / clip.duration.max(1e-3) * other.duration, mixing);
        for (pose, mixed) in out.iter_mut().zip(mixing.iter()) {
            *pose = pose.lerp(mixed, weight);
        }
    }
}

/// What's on screen of a clip and those fading out under it: from the oldest up, each one
/// over what's below it.
fn blend_stack(
    skeleton: &Skeleton,
    previous: &[Fading],
    current: Track,
    out: &mut Vec<JointTransform>,
    scratch: &mut Vec<JointTransform>,
    mixing: &mut Vec<JointTransform>,
) {
    sample_track(skeleton, previous.first().map_or(current, |fading| fading.track), out, mixing);
    for (k, under) in previous.iter().enumerate() {
        let over = previous.get(k + 1).map_or(current, |fading| fading.track);
        sample_track(skeleton, over, scratch, mixing);
        let weight = under.weight();
        for (pose, over) in out.iter_mut().zip(scratch.iter()) {
            *pose = pose.lerp(over, weight);
        }
    }
}

fn push_fading(previous: &mut Vec<Fading>, track: Track, speed: f32, fade: f32) {
    if previous.len() == MAX_FADING {
        previous.remove(0);
    }
    previous.push(Fading { track, speed, remaining: fade, total: fade });
}

/// Moves the clips fading out along. A fade that's over covers everything under it.
fn advance_fades(previous: &mut Vec<Fading>, dt: f32, clips: &[AnimationClip]) {
    for fading in previous.iter_mut() {
        let duration = clips[fading.track.clip].duration;
        advance(&mut fading.track, dt * fading.speed, duration);
        fading.remaining -= dt;
    }
    if let Some(done) = previous.iter().rposition(|fading| fading.remaining <= 0.0) {
        previous.drain(..=done);
    }
}

/// Collects the events between `track.time` and `dt` seconds later, the start
/// included and the end not, so a loop fires each one once per lap.
fn passed_events(track: &Track, clip: &AnimationClip, dt: f32, fired: &mut Vec<(usize, usize)>) {
    if dt <= 0.0 || clip.events.is_empty() {
        return;
    }
    let end = track.time + dt;
    if track.looping && clip.duration > 0.0 {
        let mut lap = 0.0;
        while lap < end {
            for (i, event) in clip.events.iter().enumerate() {
                let at = lap + event.time.rem_euclid(clip.duration);
                if at >= track.time && at < end {
                    fired.push((track.clip, i));
                }
            }
            lap += clip.duration;
        }
    } else if track.time < clip.duration {
        // An event on the last frame still counts: the clip stops there.
        let last = end >= clip.duration;
        for (i, event) in clip.events.iter().enumerate() {
            if event.time >= track.time && (event.time < end || last) {
                fired.push((track.clip, i));
            }
        }
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
            events: vec![
                ClipEvent { time: 0.0, name: "start".into() },
                ClipEvent { time: 0.5, name: "mid".into() },
                ClipEvent { time: 1.0, name: "end".into() },
            ],
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
            events: Vec::new(),
        };
        Arc::new(Skeleton::new(joints, Mat4::IDENTITY, vec![bend, wave]))
    }

    /// Where a model-space point bound to `joint` ends up.
    fn skinned(animator: &Animator, joint: usize, point: Vec3) -> Vec3 {
        animator.skinning_matrices()[joint].transform_point3(point)
    }

    /// Runs `steps` updates of `dt` and lists the events they fire, in order.
    fn collect(animator: &mut Animator, steps: usize, dt: f32) -> Vec<String> {
        let mut seen = Vec::new();
        for _ in 0..steps {
            animator.update(dt);
            seen.extend(animator.events().map(str::to_owned));
        }
        seen
    }

    #[test]
    fn a_loop_fires_each_event_once_per_lap() {
        let mut animator = Animator::new(arm());
        animator.play("Bend", 0.0);
        // "end" sits on the seam, so it counts as the loop's start.
        assert_eq!(collect(&mut animator, 11, 0.09), ["start", "end", "mid"]);
        assert_eq!(collect(&mut animator, 11, 0.09), ["start", "end", "mid"]);
    }

    #[test]
    fn a_long_step_fires_everything_it_skipped() {
        let mut animator = Animator::new(arm());
        animator.play("Bend", 0.0);
        animator.update(0.1);
        animator.update(1.0); // from 0.1 past the seam to 0.1 again
        let seen: Vec<_> = animator.events().collect();
        assert_eq!(seen, ["mid", "start", "end"]);
    }

    #[test]
    fn a_clip_played_once_fires_each_event_once() {
        let mut animator = Animator::new(arm());
        animator.play_once("Bend", 0.0);
        assert_eq!(collect(&mut animator, 30, 0.1), ["start", "mid", "end"]);
        assert!(animator.finished());
        // Events last until the next update, and replaying fires them again.
        animator.replay_once("Bend", 0.0);
        assert_eq!(animator.time(), 0.0);
        assert_eq!(collect(&mut animator, 30, 0.1), ["start", "mid", "end"]);
    }

    #[test]
    fn the_playback_rate_moves_the_events_too() {
        let mut animator = Animator::new(arm());
        animator.play_once("Bend", 0.0);
        animator.speed = 0.5;
        assert_eq!(collect(&mut animator, 9, 0.1), ["start"]);
        assert_eq!(collect(&mut animator, 2, 0.1), ["mid"]);
        animator.speed = -1.0;
        assert!(collect(&mut animator, 3, 0.1).is_empty());
    }

    #[test]
    fn a_clip_fading_out_fires_nothing() {
        let mut animator = Animator::new(arm());
        animator.play("Bend", 0.0);
        animator.update(0.4);
        assert!(animator.play("wave", 0.5));
        animator.update(0.2); // Bend, fading out, passes its 0.5
        assert!(!animator.fired("mid"));
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

    #[test]
    fn a_fade_cut_short_keeps_what_was_showing() {
        let mut animator = Animator::new(arm());
        animator.play("wave", 0.0);
        animator.update(1.2); // shoulder stepped to +X
        animator.play("Bend", 1.0);
        animator.update(0.01); // still almost all wave
        animator.replay_once("wave", 1.0); // back at its start, where the shoulder is at 0
        animator.update(0.01);
        let x = animator.pose()[1].translation.x;
        assert!(x > 0.9, "the shoulder jumped to x = {x}");
        animator.update(1.1);
        assert_eq!(animator.current(), Some("wave"));
        assert_eq!(animator.pose()[1].translation, Vec3::X, "wave alone again, past its step");
    }

    #[test]
    fn an_overlay_moves_only_its_joints_and_fades() {
        let mut animator = Animator::new(arm());
        animator.play("wave", 0.0);
        animator.update(1.2); // shoulder at +X
        assert!(animator.overlay_loop("Bend", "elbow", 0.5));
        animator.update(0.5);
        assert_eq!(animator.pose()[1].translation, Vec3::X, "the shoulder keeps the clip underneath");
        let elbow = animator.pose()[1].rotation;
        assert_eq!(animator.overlay_current(), Some("Bend"));
        animator.overlay_stop(0.5);
        assert_eq!(animator.overlay_current(), None);
        animator.update(0.25);
        assert!(animator.pose()[1].rotation != elbow || animator.overlay.is_some());
        animator.update(0.5);
        assert!(animator.overlay.is_none());
    }

    #[test]
    fn a_mix_shows_both_clips_and_can_change_every_frame() {
        let mut animator = Animator::new(arm());
        animator.play("wave", 0.0);
        animator.update(1.2);
        animator.play_mixed("wave", "Bend", 0.5, 0.0);
        animator.update(0.1);
        let x = animator.pose()[1].translation.x;
        assert!((x - 0.5).abs() < 1e-5, "half of each, x = {x}");
        animator.play_mixed("wave", "Bend", 0.0, 0.5);
        animator.update(0.01);
        assert_eq!(animator.pose()[1].translation, Vec3::X, "a new weight is no new clip");
    }

    #[test]
    fn a_fade_takes_its_time_at_any_speed() {
        let mut animator = Animator::new(arm());
        animator.play("wave", 0.0);
        animator.update(1.2);
        animator.play("Bend", 1.0);
        animator.speed = 0.25;
        animator.update(1.1);
        assert_eq!(animator.pose()[1].translation, Vec3::ZERO, "fade finished");
    }
}
