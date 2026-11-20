//! Death without a clip: the body becomes a ragdoll whose joints are muscles. Each joint has
//! anatomical limits and a motor pulling it towards a pose, with a tone that changes over time
//! and by region: the knees give first, the arms reach out to break the fall, the head is held
//! for a moment, and some bodies writhe before they go still. Nothing here is a clip: what the
//! body does comes from how it is falling.

use std::f32::consts::PI;

use glam::{Mat4, Quat, Vec3};
use grimora_core::{Animator, JointTransform, Rng, Skeleton, Transform};
use grimora_physics::{BodyHandle, JointHandle, LimbDesc, LimbShape, Physics, SocketDesc};
use hecs::Entity;

use grimora_scene::{BodyDesc, ColliderShape};

use crate::combat::{Fighter, Health};
use crate::npc::Agent;
use crate::world::{Attached, Body};
use crate::{Renderable, World};

/// A sudden push where a blow or a shot landed, in N·s.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Push {
    pub at: Vec3,
    pub impulse: Vec3,
}

/// Which muscles a limb's tone follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Region {
    Root,
    Torso,
    Neck,
    Arm,
    Leg,
}

/// What a limb is in the body, for the behaviors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Hips,
    Spine,
    Chest,
    Head,
    Shoulder,
    Elbow,
    Wrist,
    Hip,
    Knee,
    Ankle,
}

/// One limb of the rig: a bone, its share of the weight and the joint to its parent.
/// Limits are in degrees around the joint's X (bend: forward for a limb hanging down,
/// back for the spine, toes up for a foot), Y (twist) and Z (out from the body, for the left
/// side; the right side is mirrored).
struct Spec {
    bone: &'static str,
    parent: Option<&'static str>,
    part: Part,
    region: Region,
    share: f32,
    limits: [[f32; 2]; 3],
    strength: f32,
}

/// Shares of the weight are Winter's (Biomechanics and Motor Control of Human Movement).
const SPECS: [Spec; 16] = [
    spec("hips", None, Part::Hips, Region::Root, 0.142, [[0.0; 2]; 3], 0.0),
    spec(
        "spine",
        Some("hips"),
        Part::Spine,
        Region::Torso,
        0.139,
        [[-35.0, 20.0], [-25.0, 25.0], [-20.0, 20.0]],
        320.0,
    ),
    spec(
        "chest",
        Some("spine"),
        Part::Chest,
        Region::Torso,
        0.216,
        [[-30.0, 15.0], [-25.0, 25.0], [-20.0, 20.0]],
        260.0,
    ),
    spec("neck", Some("chest"), Part::Head, Region::Neck, 0.081, [[-55.0, 40.0], [-70.0, 70.0], [-35.0, 35.0]], 45.0),
    spec(
        "upper_arm.L",
        Some("chest"),
        Part::Shoulder,
        Region::Arm,
        0.028,
        [[-60.0, 160.0], [-70.0, 70.0], [-50.0, 100.0]],
        90.0,
    ),
    spec(
        "forearm.L",
        Some("upper_arm.L"),
        Part::Elbow,
        Region::Arm,
        0.016,
        [[-3.0, 145.0], [-45.0, 45.0], [-4.0, 4.0]],
        60.0,
    ),
    spec(
        "hand.L",
        Some("forearm.L"),
        Part::Wrist,
        Region::Arm,
        0.006,
        [[-70.0, 70.0], [-15.0, 15.0], [-25.0, 25.0]],
        14.0,
    ),
    spec(
        "upper_arm.R",
        Some("chest"),
        Part::Shoulder,
        Region::Arm,
        0.028,
        [[-60.0, 160.0], [-70.0, 70.0], [-50.0, 100.0]],
        90.0,
    ),
    spec(
        "forearm.R",
        Some("upper_arm.R"),
        Part::Elbow,
        Region::Arm,
        0.016,
        [[-3.0, 145.0], [-45.0, 45.0], [-4.0, 4.0]],
        60.0,
    ),
    spec(
        "hand.R",
        Some("forearm.R"),
        Part::Wrist,
        Region::Arm,
        0.006,
        [[-70.0, 70.0], [-15.0, 15.0], [-25.0, 25.0]],
        14.0,
    ),
    spec("thigh.L", Some("hips"), Part::Hip, Region::Leg, 0.100, [[-25.0, 120.0], [-40.0, 40.0], [-25.0, 55.0]], 260.0),
    spec("shin.L", Some("thigh.L"), Part::Knee, Region::Leg, 0.0465, [[-150.0, 2.0], [-5.0, 5.0], [-3.0, 3.0]], 200.0),
    spec(
        "foot.L",
        Some("shin.L"),
        Part::Ankle,
        Region::Leg,
        0.0145,
        [[-45.0, 30.0], [-15.0, 15.0], [-25.0, 25.0]],
        80.0,
    ),
    spec("thigh.R", Some("hips"), Part::Hip, Region::Leg, 0.100, [[-25.0, 120.0], [-40.0, 40.0], [-25.0, 55.0]], 260.0),
    spec("shin.R", Some("thigh.R"), Part::Knee, Region::Leg, 0.0465, [[-150.0, 2.0], [-5.0, 5.0], [-3.0, 3.0]], 200.0),
    spec(
        "foot.R",
        Some("shin.R"),
        Part::Ankle,
        Region::Leg,
        0.0145,
        [[-45.0, 30.0], [-15.0, 15.0], [-25.0, 25.0]],
        80.0,
    ),
];

const fn spec(
    bone: &'static str,
    parent: Option<&'static str>,
    part: Part,
    region: Region,
    share: f32,
    limits: [[f32; 2]; 3],
    strength: f32,
) -> Spec {
    Spec { bone, parent, part, region, share, limits, strength }
}

/// Kilograms of a body at scale 1.
const BODY_MASS: f32 = 72.0;

/// The fastest a muscle at full tone pulls, as an angular frequency (rad/s): its stiffness is
/// the square, and it is near critically damped.
const MUSCLE_FREQUENCY: f32 = 30.0;
const MUSCLE_DAMPING_RATIO: f32 = 0.9;
/// What's left of a joint's stiffness once nothing holds it: damping per second, and the
/// share of the joint's strength it may use, as friction in the joint.
const DEAD_DAMPING: f32 = 2.5;
const DEAD_FRICTION: f32 = 0.04;

/// Seconds before anything reacts: the blow's own moment.
const SHOCK: f32 = 0.08;
/// How long the arms take to reach out once the body starts falling.
const BRACE_IN: f32 = 0.22;
/// How long, after the body landed, until the last muscle lets go.
const LET_GO: f32 = 1.4;
/// The share of a blow's push that carries the whole body, rather than the limb it hit.
const CARRIED: f32 = 0.75;
/// Kilograms of what falls from a dead hand.
const DROPPED_MASS: f32 = 1.4;
/// Chances of clutching the wound (hit in the trunk) and of writhing on the ground.
const CLUTCH_CHANCE: f32 = 0.45;
const WRITHE_CHANCE: f32 = 0.4;
/// The fastest a limb keeps going from the clip it was playing, in m/s and rad/s.
const KEPT_SPEED: f32 = 8.0;
const KEPT_SPIN: f32 = 20.0;
/// Seconds ahead the clip is looked at to see how each limb was moving.
const KEPT_STEP: f32 = 1.0 / 30.0;
/// A limb is heard when it lands falling faster than this, in m/s, and then not again for
/// this many seconds.
const THUD_FROM: f32 = 1.2;
const THUD_QUIET: f32 = 0.35;
/// Knocked down without dying: seconds it lies dazed, how fast it rolls over to lie face
/// down (rad/s) and for how long, and when it gives up and gets up as it lies.
const DAZED: [f32; 2] = [0.8, 2.0];
const ROLL_SPIN: f32 = 5.0;
const ROLL_SECONDS: f32 = 0.9;
/// What the side it rolls up from is lifted by while rolling, as a share of the trunk's weight.
const ROLL_LIFT: f32 = 1.4;
const KNOCKED_LONGEST: f32 = 7.0;
/// The clip that gets a knocked body up, starting face down; how long the ragdoll's pose takes
/// to give way to it; and how far behind where it stands up its hips start, in meters.
const GET_UP: &str = "get_up";
const GET_UP_BLEND: f32 = 0.45;
const GET_UP_HIPS_BEHIND: f32 = 0.18;

/// A body or a limb hitting the ground hard enough to be heard, for the game to play
/// (see [`World::take_thuds`]). `heavy` is the trunk, and `strength` goes from 0 to 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thud {
    pub at: Vec3,
    pub heavy: bool,
    pub strength: f32,
}

/// A body knocked down alive: it lies dazed, rolls onto its face and gets up.
#[derive(Debug, Clone, Copy)]
struct Knocked {
    dazed: f32,
    /// Seconds left of the roll it's in, which way, and how many it tried.
    rolling: f32,
    roll_way: f32,
    rolls: u32,
    /// The side it lifts to roll over the other.
    lift_right: bool,
}

/// A limb of a built rig.
struct Limb {
    /// Skeleton joint.
    joint: usize,
    /// Limb it hangs from.
    parent: Option<usize>,
    part: Part,
    region: Region,
    right: bool,
    share: f32,
    strength: f32,
    /// Shape in the bone's frame, at scale 1.
    shape: LimbShape,
    /// The joint's anatomical frame in this bone's frame and in its parent's.
    frame: Quat,
    parent_frame: Quat,
    /// Radians, mirrored for the right side.
    limits: [[f32; 2]; 3],
    /// The bone's turn in the model at rest, inverted: model directions into the bone.
    rest_inverse: Quat,
}

/// A ragdoll: the component of a body that died without a clip.
pub struct Ragdoll {
    limbs: Vec<Limb>,
    bodies: Vec<BodyHandle>,
    joints: Vec<Option<JointHandle>>,
    /// Limb of each skeleton joint, if it has one.
    limb_of: Vec<Option<usize>>,
    /// The local pose it died in: the joints without limbs keep it.
    held: Vec<JointTransform>,
    /// Each joint's turn when it died, and when it landed.
    death: Vec<Quat>,
    landed_pose: Option<Vec<Quat>>,
    age: f32,
    landed_at: Option<f32>,
    /// Held in ice: it falls as one piece.
    stiff: bool,
    /// The arm that holds the wound, and when it lets go, after landing.
    clutch: Option<(bool, f32)>,
    /// When it writhes (after landing), how long, and the phases of each joint.
    writhe: Option<(f32, f32)>,
    phases: [f32; 8],
    /// The leg it steps with to catch itself, and when it switches.
    step_right: bool,
    limp: bool,
    resting: bool,
    scale: f32,
    hips_rest: Vec3,
    /// What fell from its hands, gone with it.
    dropped: Vec<Entity>,
    /// Alive, and getting back up; `rise` once it's ready to.
    knocked: Option<Knocked>,
    rise: bool,
    /// Each limb's velocity at the last step, how long until it may be heard again, the same
    /// for the trunk as one, and what was heard.
    last: Vec<Vec3>,
    quiet: Vec<f32>,
    trunk_quiet: f32,
    thuds: Vec<Thud>,
}

/// Builds the rig of a skeleton, if it has every bone the ragdoll needs.
fn rig(skeleton: &Skeleton) -> Option<Vec<Limb>> {
    let mut rest = Vec::new();
    skeleton.global_transforms(&skeleton.rest_pose(), &mut rest);
    let at = |name: &str| skeleton.joint(name).map(|j| rest[j]);
    let turn = |m: Mat4| m.to_scale_rotation_translation().1.normalize();
    let mut limbs: Vec<Limb> = Vec::with_capacity(SPECS.len());
    for spec in &SPECS {
        let joint = skeleton.joint(spec.bone)?;
        let parent = match spec.parent {
            Some(name) => Some(SPECS.iter().position(|s| s.bone == name)?),
            None => None,
        };
        let right = spec.bone.ends_with(".R");
        let bone = rest[joint];
        let (origin, rotation) = (bone.w_axis.truncate(), turn(bone));
        let along = rotation * Vec3::Y;
        let to_bone = |p: Vec3| rotation.inverse() * (p - origin);
        let child = |name: &str| Some(at(name)?.w_axis.truncate());
        let capsule = |to: Vec3, radius: f32| LimbShape::Capsule { from: Vec3::ZERO, to: to_bone(to), radius };
        let shape = match spec.part {
            Part::Hips => LimbShape::Cuboid {
                center: to_bone(origin + Vec3::new(0.0, 0.03, 0.0)),
                half_extents: Vec3::new(0.14, 0.09, 0.1),
            },
            Part::Spine => capsule(child("chest")?, 0.11),
            Part::Chest => LimbShape::Cuboid {
                center: to_bone(origin + Vec3::new(0.0, 0.12, 0.01)),
                half_extents: Vec3::new(0.14, 0.12, 0.1),
            },
            Part::Head => {
                LimbShape::Ball { center: to_bone(child("head")? + Vec3::new(0.0, 0.1, -0.01)), radius: 0.11 }
            }
            Part::Shoulder => capsule(child(if right { "forearm.R" } else { "forearm.L" })?, 0.055),
            Part::Elbow => capsule(child(if right { "hand.R" } else { "hand.L" })?, 0.045),
            Part::Wrist => capsule(origin + along * 0.09, 0.035),
            Part::Hip => capsule(child(if right { "shin.R" } else { "shin.L" })?, 0.075),
            Part::Knee => capsule(child(if right { "foot.R" } else { "foot.L" })?, 0.055),
            Part::Ankle => {
                let toe = child(if right { "toe.R" } else { "toe.L" })?;
                let heel = Vec3::new(origin.x, 0.045, origin.z + 0.03);
                let tip = Vec3::new(toe.x, 0.04, toe.z - 0.06);
                LimbShape::Capsule { from: to_bone(heel), to: to_bone(tip), radius: 0.04 }
            }
        };
        // The joint's frame: Y along the limb, X the model's right made square to it.
        let x = (Vec3::X - along * along.dot(Vec3::X)).normalize_or(Vec3::Z);
        let frame = Quat::from_mat3(&glam::Mat3::from_cols(x, along, x.cross(along)));
        let parent_frame = match parent {
            Some(p) => turn(rest[limbs[p].joint]).inverse() * frame,
            None => Quat::IDENTITY,
        };
        let mirror = |[lo, hi]: [f32; 2]| if right { [-hi, -lo] } else { [lo, hi] };
        let [bend, twist, out] = spec.limits;
        let limits = [bend, mirror(twist), mirror(out)].map(|[lo, hi]| [lo.to_radians(), hi.to_radians()]);
        limbs.push(Limb {
            joint,
            parent,
            part: spec.part,
            region: spec.region,
            right,
            share: spec.share,
            strength: spec.strength,
            shape,
            frame: rotation.inverse() * frame,
            parent_frame,
            limits,
            rest_inverse: rotation.inverse(),
        });
    }
    Some(limbs)
}

fn scaled(shape: LimbShape, s: f32) -> LimbShape {
    match shape {
        LimbShape::Capsule { from, to, radius } => {
            LimbShape::Capsule { from: from * s, to: to * s, radius: radius * s }
        }
        LimbShape::Ball { center, radius } => LimbShape::Ball { center: center * s, radius: radius * s },
        LimbShape::Cuboid { center, half_extents } => {
            LimbShape::Cuboid { center: center * s, half_extents: half_extents * s }
        }
    }
}

fn center_of(shape: LimbShape) -> Vec3 {
    match shape {
        LimbShape::Capsule { from, to, .. } => (from + to) * 0.5,
        LimbShape::Ball { center, .. } | LimbShape::Cuboid { center, .. } => center,
    }
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A turn in a joint's frame from its angles, in radians: bend (X), twist (Y), out (Z).
fn angles(bend: f32, twist: f32, out: f32) -> Quat {
    Quat::from_rotation_z(out) * Quat::from_rotation_x(bend) * Quat::from_rotation_y(twist)
}

impl Ragdoll {
    /// The world turn of each limb's joint frame, as it is now.
    fn joint_turns(&self, physics: &Physics) -> Vec<Quat> {
        let turns: Vec<Quat> =
            self.bodies.iter().map(|&b| physics.body_pose(b).map_or(Quat::IDENTITY, |(_, r)| r)).collect();
        self.limbs
            .iter()
            .enumerate()
            .map(|(i, limb)| match limb.parent {
                Some(p) => (turns[p] * limb.parent_frame).inverse() * (turns[i] * limb.frame),
                None => Quat::IDENTITY,
            })
            .collect()
    }

    fn find(&self, part: Part, right: bool) -> usize {
        self.limbs
            .iter()
            .position(|l| l.part == part && (l.right == right || l.region != Region::Arm && l.region != Region::Leg))
            .unwrap_or(0)
    }

    /// Sets every muscle for this moment of the fall.
    fn drive(&mut self, physics: &mut Physics, dt: f32) {
        if self.resting {
            return;
        }
        self.age += dt;
        let age = self.age;
        self.listen(physics, dt);
        // Where the body is and where it's going.
        let (mut center, mut velocity, mut mass) = (Vec3::ZERO, Vec3::ZERO, 0.0);
        for &body in &self.bodies {
            let m = physics.body_mass(body);
            center += physics.body_center(body).unwrap_or_default() * m;
            velocity += physics.body_velocity(body).map_or(Vec3::ZERO, |(v, _)| v) * m;
            mass += m;
        }
        center /= mass.max(1e-3);
        velocity /= mass.max(1e-3);
        let feet = [self.find(Part::Ankle, false), self.find(Part::Ankle, true)];
        let feet_at = feet.iter().map(|&f| physics.body_center(self.bodies[f]).unwrap_or(center)).sum::<Vec3>() * 0.5;
        let chest = self.find(Part::Chest, false);
        let (chest_at, chest_turn) = physics.body_pose(self.bodies[chest]).unwrap_or((center, Quat::IDENTITY));
        let height = physics.raycast_scenery(chest_at, Vec3::NEG_Y, 4.0).map_or(4.0, |hit| hit.distance);
        if self.landed_at.is_none()
            && (age > 3.0 || age > 0.25 && height < 0.38 * self.scale || age > 0.9 && velocity.length() < 0.3)
        {
            self.landed_at = Some(age);
            self.landed_pose = Some(self.joint_turns(physics));
        }
        let after = self.landed_at.map(|t| age - t);
        // Which way it falls, in the chest's own terms: forward is positive, and to its right.
        let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z);
        let lean = flat(velocity) + flat(center - feet_at) * 2.0;
        let toward = chest_turn * self.limbs[chest].rest_inverse;
        let forward = flat(toward * Vec3::NEG_Z).normalize_or_zero();
        let side = flat(toward * Vec3::X).normalize_or_zero();
        let speed = lean.length() + 0.3;
        let (ahead, rightward) = (lean.dot(forward) / speed, lean.dot(side) / speed);

        let knocked = self.knocked.is_some();
        let base = self.landed_pose.clone().unwrap_or_else(|| self.death.clone());
        let deg = |d: f32| d.to_radians();
        let buckle = smooth((age - SHOCK) / 0.5);
        let brace = smooth((age - SHOCK) / BRACE_IN);
        let let_go = after.map_or(0.0, |a| smooth(a / LET_GO));
        // The writhing comes and goes in a swell, after the body lands.
        let writhing = match (self.writhe, after) {
            // Dazed: it stirs.
            _ if knocked => after.map_or(0.0, |a| 0.4 * smooth(a / 0.4)),
            (Some((from, length)), Some(a)) if a > from => {
                let t = a - from;
                smooth(t / 0.5) * (1.0 - smooth((t - length) / 1.2))
            }
            _ => 0.0,
        };
        let wave = |k: usize, rate: f32| (age * rate * 2.0 * PI + self.phases[k]).sin();
        for i in 1..self.limbs.len() {
            let Some(joint) = self.joints[i] else { continue };
            let limb = &self.limbs[i];
            let mirror = if limb.right { -1.0 } else { 1.0 };
            let death = self.death[i];
            let (mut target, mut tone) = match limb.region {
                Region::Leg if self.landed_at.is_none() => {
                    let tone = 0.55 * (1.0 - smooth((age - SHOCK) / 0.6));
                    // A step to catch the fall, while the legs still hold.
                    let stepping = age < 0.7 && lean.length() > 0.5 && limb.right == self.step_right;
                    let target = match limb.part {
                        Part::Hip if stepping => {
                            angles(deg(45.0) * ahead, 0.0, deg(30.0) * mirror * -rightward) * death
                        }
                        Part::Hip => angles(deg(30.0) * buckle, 0.0, 0.0) * death,
                        Part::Knee if stepping => angles(-deg(35.0), 0.0, 0.0) * death,
                        Part::Knee => angles(-deg(65.0) * buckle, 0.0, 0.0) * death,
                        _ => death,
                    };
                    (target, tone)
                }
                Region::Leg if knocked => (base[i], 0.15),
                Region::Leg => (base[i], 0.12 * (1.0 - let_go)),
                Region::Torso => {
                    let tone = if knocked {
                        if self.landed_at.is_none() { 0.6 } else { 0.25 }
                    } else {
                        0.6 * (1.0 - smooth((age - 0.3) / 1.2))
                    };
                    let curl = if self.landed_at.is_none() { deg(-10.0) * ahead.max(0.0) * buckle } else { 0.0 };
                    (angles(curl, 0.0, 0.0) * base[i], tone)
                }
                Region::Neck => {
                    let tone = match (self.landed_at, knocked) {
                        (None, true) => 0.6,
                        (_, true) => 0.35,
                        (None, false) => 0.45,
                        _ => 0.45 * (1.0 - smooth(after.unwrap_or(0.0) / 0.6)),
                    };
                    (angles(deg(-15.0) * brace, 0.0, 0.0) * base[i], tone)
                }
                Region::Arm => {
                    let clutching = self
                        .clutch
                        .is_some_and(|(right, until)| right == limb.right && after.is_none_or(|a| a < until));
                    let (shoulder, elbow, wrist) = if clutching {
                        // The hand goes to the wound.
                        (angles(deg(35.0), 0.0, deg(-25.0) * mirror), angles(deg(115.0), 0.0, 0.0), death)
                    } else {
                        // Arms out where the body is going: forward, back and out, or out to a side.
                        let outward = (-rightward * mirror).max(0.0);
                        let bend = deg(85.0) * ahead.max(0.0) - deg(40.0) * (-ahead).max(0.0);
                        let out = deg(20.0) + deg(45.0) * (-ahead).max(0.0) + deg(55.0) * outward;
                        (
                            angles(bend, 0.0, out * mirror),
                            angles(deg(25.0) + deg(20.0) * ahead.max(0.0), 0.0, 0.0),
                            death,
                        )
                    };
                    let reach = match limb.part {
                        Part::Shoulder => shoulder,
                        Part::Elbow => elbow,
                        _ => wrist,
                    };
                    let falling = self.landed_at.is_none();
                    let target = if falling || clutching { death.slerp(reach, brace) } else { base[i] };
                    let tone = if clutching {
                        0.5
                    } else if falling {
                        if knocked { 0.8 } else { 0.6 }
                    } else if knocked {
                        0.25
                    } else {
                        0.6 * (1.0 - smooth(after.unwrap_or(0.0) / 0.9))
                    };
                    (target, tone)
                }
                Region::Root => (death, 0.0),
            };
            if age < SHOCK {
                tone = 0.5;
                target = death;
            }
            if writhing > 0.0 {
                let k = i % 8;
                let twitch = match limb.part {
                    Part::Hip => angles(deg(25.0) * wave(k, 0.7), 0.0, deg(10.0) * wave((k + 1) & 7, 0.5)),
                    Part::Knee => angles(-deg(20.0) - deg(25.0) * wave(k, 0.7), 0.0, 0.0),
                    Part::Spine | Part::Chest => {
                        angles(deg(8.0) * wave(k, 0.45), deg(6.0) * wave((k + 2) & 7, 0.3), 0.0)
                    }
                    Part::Head => angles(deg(15.0) * wave(k, 0.35), deg(25.0) * wave((k + 3) & 7, 0.25), 0.0),
                    Part::Shoulder => {
                        angles(deg(20.0) * wave(k, 0.6), 0.0, deg(15.0) * wave((k + 1) & 7, 0.4) * mirror)
                    }
                    Part::Elbow => angles(deg(30.0) + deg(25.0) * wave(k, 0.8), 0.0, 0.0),
                    _ => Quat::IDENTITY,
                };
                target = target.slerp(twitch * base[i], writhing);
                tone = tone.max(0.3 * writhing);
            }
            // Rolling over: arms in along the body, the leg on the rising side across the other.
            if let Some(knocked) = self.knocked
                && knocked.rolling > 0.0
            {
                let lifted = limb.right == knocked.lift_right;
                (target, tone) = match limb.part {
                    Part::Shoulder | Part::Elbow | Part::Wrist => (Quat::IDENTITY, 0.5),
                    Part::Hip if lifted => (angles(deg(50.0), 0.0, -deg(20.0) * mirror), 0.5),
                    Part::Knee if lifted => (angles(-deg(70.0), 0.0, 0.0), 0.5),
                    _ => (target, tone),
                };
            }
            if self.stiff {
                (target, tone) = (death, 1.0);
            }
            let stiffness = tone * MUSCLE_FREQUENCY * MUSCLE_FREQUENCY;
            let damping = 2.0 * MUSCLE_DAMPING_RATIO * stiffness.sqrt() + DEAD_DAMPING;
            let strength = limb.strength * self.scale.powi(3) * tone.max(DEAD_FRICTION);
            physics.set_muscle(joint, target, stiffness, damping, strength);
        }
        if let (Some(_), Some(after)) = (self.knocked, after) {
            self.come_round(physics, dt, after, velocity, chest_turn);
            return;
        }
        let done = !self.stiff
            && after.is_some_and(|a| a > LET_GO.max(self.writhe.map_or(0.0, |(from, length)| from + length + 1.2)));
        if done && !self.limp {
            self.limp = true;
        }
        if self.limp && self.bodies.iter().all(|&b| physics.body_sleeping(b)) {
            self.resting = true;
        }
    }

    /// Shows the limbs where they are: the entity follows the hips, and each joint with a
    /// limb takes its turn; the others keep the pose it died in.
    fn pose(&self, physics: &Physics, transform: &mut Transform, animator: &mut Animator) {
        let Some((hips_at, _)) = physics.body_pose(self.bodies[0]) else { return };
        transform.position = hips_at - transform.rotation * (self.hips_rest * self.scale);
        self.pose_in(physics, transform, animator);
    }

    /// The limbs' pose as seen from `transform`, wherever that is.
    fn pose_in(&self, physics: &Physics, transform: &Transform, animator: &mut Animator) {
        let to_model = transform.matrix().inverse();
        let skeleton = animator.skeleton().clone();
        let mut pose = self.held.clone();
        let mut globals = vec![Mat4::IDENTITY; skeleton.joints.len()];
        for &j in skeleton.order() {
            let above = skeleton.joints[j].parent.map_or(skeleton.root, |p| globals[p]);
            if let Some(i) = self.limb_of[j]
                && let Some((at, turn)) = physics.body_pose(self.bodies[i])
            {
                let (_, above_turn, _) = above.to_scale_rotation_translation();
                pose[j].rotation = (above_turn.inverse() * transform.rotation.inverse() * turn).normalize();
                if self.limbs[i].parent.is_none() {
                    pose[j].translation = above.inverse().transform_point3(to_model.transform_point3(at));
                }
            }
            globals[j] = above * pose[j].matrix();
        }
        animator.set_pose(&pose);
    }

    /// Hears each limb that lands: falling fast, and stopped by something just under it.
    fn listen(&mut self, physics: &Physics, dt: f32) {
        self.trunk_quiet = (self.trunk_quiet - dt).max(0.0);
        for i in 0..self.bodies.len() {
            let body = self.bodies[i];
            let now = physics.body_velocity(body).map_or(Vec3::ZERO, |(v, _)| v);
            let before = std::mem::replace(&mut self.last[i], now);
            self.quiet[i] = (self.quiet[i] - dt).max(0.0);
            let falling = -before.y;
            let limb = &self.limbs[i];
            let heavy = matches!(limb.region, Region::Root | Region::Torso);
            if falling < THUD_FROM
                || now.y - before.y < falling * 0.5
                || self.quiet[i] > 0.0
                || limb.part == Part::Wrist
                || heavy && self.trunk_quiet > 0.0
            {
                continue;
            }
            let at = physics.body_center(body).unwrap_or_default();
            if physics.raycast_scenery(at, Vec3::NEG_Y, 0.35 * self.scale).is_none() {
                continue;
            }
            self.quiet[i] = THUD_QUIET;
            if heavy {
                self.trunk_quiet = THUD_QUIET;
            }
            let strength = ((falling - THUD_FROM) / 3.0 + 0.25).min(1.0);
            self.thuds.push(Thud { at, heavy, strength });
        }
    }

    /// A knocked body on the ground: dazed for a moment, then onto its face, then ready to get up.
    fn come_round(&mut self, physics: &mut Physics, dt: f32, after: f32, velocity: Vec3, chest_turn: Quat) {
        let Some(mut knocked) = self.knocked else { return };
        let chest = self.find(Part::Chest, false);
        let facing = chest_turn * self.limbs[chest].rest_inverse * Vec3::NEG_Z;
        let prone = facing.y < -0.55;
        let hips_at = physics.body_center(self.bodies[0]).unwrap_or_default();
        let chest_at = physics.body_center(self.bodies[chest]).unwrap_or(hips_at);
        let axis = (chest_at - hips_at).normalize_or_zero();
        if after > KNOCKED_LONGEST || after > knocked.dazed && prone && velocity.length() < 0.6 {
            self.rise = true;
        } else if knocked.rolling > 0.0 {
            knocked.rolling -= dt;
            for &body in &self.bodies {
                physics.wake_body(body);
            }
            let spine = self.find(Part::Spine, false);
            let right = chest_turn * self.limbs[chest].rest_inverse * Vec3::X;
            let side = if knocked.lift_right { right } else { -right };
            for limb in [0, spine, chest] {
                let body = self.bodies[limb];
                let along = physics.body_velocity(body).map_or(0.0, |(_, spin)| spin.dot(axis));
                physics.add_spin(body, axis * (ROLL_SPIN * knocked.roll_way - along) * 0.3);
                // The rising side is lifted, so it tips over the edge of the other.
                let at = physics.body_center(body).unwrap_or(hips_at) + side * 0.15 * self.scale;
                let lift = Vec3::Y * physics.body_mass(body) * 9.81 * ROLL_LIFT * dt;
                physics.add_impulse_at(body, lift, at);
            }
            if prone {
                knocked.rolling = 0.0;
            }
        } else if after > knocked.dazed && !prone && knocked.rolls < 3 && velocity.length() < 0.8 {
            // Over the side it already leans to, or the side of the leg it stepped with.
            let way = axis.cross(facing).dot(Vec3::NEG_Y);
            knocked.roll_way = if way.abs() < 0.1 { if self.step_right { 1.0 } else { -1.0 } } else { way.signum() };
            let right = chest_turn * self.limbs[chest].rest_inverse * Vec3::X;
            knocked.lift_right = axis.cross(right).dot(Vec3::Y) * knocked.roll_way > 0.0;
            knocked.rolling = ROLL_SECONDS;
            knocked.rolls += 1;
        }
        self.knocked = Some(knocked);
    }
}

impl World {
    /// Lets `entity` fall as a ragdoll, pushed by `push` and at once as one rigid piece if
    /// `stiff` (held in ice). Returns false, doing nothing, if it has no skeleton with the
    /// bones a ragdoll needs.
    pub fn start_ragdoll(&mut self, entity: Entity, physics: &mut Physics, push: Option<Push>, stiff: bool) -> bool {
        self.begin_ragdoll(entity, physics, push, stiff, false)
    }

    fn begin_ragdoll(
        &mut self,
        entity: Entity,
        physics: &mut Physics,
        push: Option<Push>,
        stiff: bool,
        knocked: bool,
    ) -> bool {
        let lying = match self.ecs.get::<&mut Ragdoll>(entity) {
            Ok(mut ragdoll) => {
                // Knocked down and now dead where it lies: it lets go, and drops what it held.
                let dies = ragdoll.knocked.is_some() && !knocked;
                if dies {
                    ragdoll.knocked = None;
                    ragdoll.rise = false;
                    ragdoll.landed_at = None;
                    ragdoll.landed_pose = None;
                    ragdoll.age = SHOCK;
                }
                Some(dies)
            }
            Err(_) => None,
        };
        if let Some(dies) = lying {
            if let Some(push) = push {
                self.push_ragdoll(entity, physics, push);
            }
            if dies {
                self.drop_held(entity, physics, Vec3::ZERO);
            }
            return true;
        }
        let mut query = self.ecs.query_one::<(&Transform, &Animator, Option<&Agent>)>(entity);
        let Ok((transform, animator, agent)) = query.get() else { return false };
        let skeleton = animator.skeleton().clone();
        let Some(limbs) = rig(&skeleton) else { return false };
        let scale = transform.scale.element_sum() / 3.0;
        let model = transform.matrix();
        let moving = agent.map_or(Vec3::ZERO, Agent::velocity);
        let has_agent = agent.is_some();
        let held = animator.pose().to_vec();
        // Where the clip would have taken each limb a moment later: it keeps going that way.
        let mut ahead = animator.clone();
        ahead.update(KEPT_STEP);
        let next_pose = ahead.pose().to_vec();
        drop(query);
        let mut now = Vec::new();
        skeleton.global_transforms(&held, &mut now);
        let mut next = Vec::new();
        skeleton.global_transforms(&next_pose, &mut next);
        let mut rest = Vec::new();
        skeleton.global_transforms(&skeleton.rest_pose(), &mut rest);

        let places: Vec<(Vec3, Quat)> = limbs
            .iter()
            .map(|limb| {
                let (_, turn, at) = (model * now[limb.joint]).to_scale_rotation_translation();
                (at, turn.normalize())
            })
            .collect();
        // Most of a blow carries the whole body along; the rest jolts the limb it landed on.
        let carried = push.map_or(Vec3::ZERO, |p| p.impulse * CARRIED / (BODY_MASS * scale.powi(3)));
        let bodies: Vec<BodyHandle> = limbs
            .iter()
            .zip(&places)
            .map(|(limb, &(position, rotation))| {
                let (_, turn, at) = (model * next[limb.joint]).to_scale_rotation_translation();
                let linear = ((at - position) / KEPT_STEP).clamp_length_max(KEPT_SPEED);
                let spin = turn.normalize() * rotation.inverse();
                let spin = if spin.w < 0.0 { -spin } else { spin };
                let angular = (spin.to_scaled_axis() / KEPT_STEP).clamp_length_max(KEPT_SPIN);
                physics.add_limb(&LimbDesc {
                    shape: scaled(limb.shape, scale),
                    mass: BODY_MASS * scale.powi(3) * limb.share,
                    position,
                    rotation,
                    linear: moving + carried + linear,
                    angular,
                })
            })
            .collect();
        let mut death = vec![Quat::IDENTITY; limbs.len()];
        let mut joints = vec![None; limbs.len()];
        for (i, limb) in limbs.iter().enumerate() {
            let Some(p) = limb.parent else { continue };
            let (parent_at, parent_turn) = places[p];
            let (child_at, child_turn) = places[i];
            let turn = (parent_turn * limb.parent_frame).inverse() * (child_turn * limb.frame);
            death[i] = if turn.w < 0.0 { -turn } else { turn };
            // A pose past the limits would snap: they open up to it.
            let mut limits = limb.limits;
            for (k, part) in [death[i].x, death[i].y, death[i].z].into_iter().enumerate() {
                let angle = 2.0 * part.clamp(-1.0, 1.0).asin();
                limits[k] = [limits[k][0].min(angle - 0.05), limits[k][1].max(angle + 0.05)];
            }
            joints[i] = Some(physics.add_socket(&SocketDesc {
                parent: bodies[p],
                child: bodies[i],
                parent_frame: (parent_turn.inverse() * (child_at - parent_at), limb.parent_frame),
                child_frame: (Vec3::ZERO, limb.frame),
                limits,
            }));
        }
        let mut hit_limb = None;
        if let Some(push) = push {
            let nearest = places
                .iter()
                .zip(&limbs)
                .map(|(&(at, turn), limb)| at + turn * center_of(limb.shape) * scale)
                .enumerate()
                .min_by(|a, b| a.1.distance_squared(push.at).total_cmp(&b.1.distance_squared(push.at)))
                .map(|(i, _)| i);
            if let Some(i) = nearest {
                physics.add_impulse_at(bodies[i], push.impulse * (1.0 - CARRIED), push.at);
                hit_limb = Some(i);
            }
        }
        let seed = entity.to_bits().get() ^ u64::from(transform_bits(places[0].0));
        let mut rng = Rng::new(seed);
        let in_trunk = hit_limb.is_some_and(|i| matches!(limbs[i].region, Region::Torso | Region::Root));
        let clutch = (!knocked && in_trunk && rng.range(0.0, 1.0) < CLUTCH_CHANCE)
            .then(|| (rng.range(0.0, 1.0) < 0.5, rng.range(0.6, 1.6)));
        let writhe = (rng.range(0.0, 1.0) < WRITHE_CHANCE).then(|| (rng.range(0.2, 0.8), rng.range(1.5, 4.0)));
        let phases = std::array::from_fn(|_| rng.range(0.0, 2.0 * PI));
        let step_right = push.map_or(rng.range(0.0, 1.0) < 0.5, |p| {
            let (_, hips_turn) = places[0];
            (hips_turn * limbs[0].rest_inverse * Vec3::X).dot(p.impulse) > 0.0
        });
        let mut limb_of = vec![None; skeleton.joints.len()];
        for (i, limb) in limbs.iter().enumerate() {
            limb_of[limb.joint] = Some(i);
        }
        let hips_rest = rest[limbs[0].joint].w_axis.truncate();
        let count = limbs.len();
        let ragdoll = Ragdoll {
            limbs,
            bodies,
            joints,
            limb_of,
            held,
            death,
            landed_pose: None,
            age: 0.0,
            landed_at: None,
            stiff,
            clutch,
            writhe,
            phases,
            step_right,
            limp: false,
            resting: false,
            dropped: Vec::new(),
            knocked: knocked.then(|| Knocked {
                dazed: rng.range(DAZED[0], DAZED[1]),
                rolling: 0.0,
                roll_way: 1.0,
                rolls: 0,
                lift_right: false,
            }),
            rise: false,
            last: vec![Vec3::ZERO; count],
            quiet: vec![0.0; count],
            trunk_quiet: 0.0,
            thuds: Vec::new(),
            scale,
            hips_rest,
        };
        let _ = self.ecs.insert_one(entity, ragdoll);
        if has_agent && !knocked {
            self.drop_held(entity, physics, moving + carried);
        }
        true
    }

    /// What a dead hand held falls: a copy of it, tumbling as a body, while the one held hides
    /// (whoever owns it may still want it). Only what's in or under a hand.
    fn drop_held(&mut self, entity: Entity, physics: &mut Physics, moving: Vec3) {
        let held: Vec<(Entity, Transform, Renderable)> = self
            .ecs
            .query::<(Entity, &Attached, &Transform, &Renderable)>()
            .iter()
            .filter(|(_, a, _, r)| a.parent == entity && !a.hidden && r.visible && in_hand(&a.joint))
            .map(|(e, _, t, r)| (e, *t, *r))
            .collect();
        let mut dropped = Vec::new();
        for (original, transform, renderable) in held {
            let Some(mesh) = self.mesh(renderable.mesh) else { continue };
            let points: Vec<Vec3> = mesh.vertices.iter().map(|v| Vec3::from(v.position)).collect();
            if points.len() < 4 {
                continue;
            }
            let shape = ColliderShape::ConvexHull { points };
            let desc = BodyDesc::Dynamic {
                density: 500.0,
                mass: Some(DROPPED_MASS),
                friction: 0.7,
                restitution: 0.15,
                angular_damping: 0.4,
            };
            let Some(body) = physics.add_body(&shape, &transform, &desc) else { continue };
            physics.set_body_state(body, transform.position, transform.rotation, moving, Vec3::new(2.0, 0.0, 1.0));
            if let Ok(mut attached) = self.ecs.get::<&mut Attached>(original) {
                attached.hidden = true;
            }
            dropped.push(self.ecs.spawn((transform, renderable, Body(body))));
        }
        if let Ok(mut ragdoll) = self.ecs.get::<&mut Ragdoll>(entity) {
            ragdoll.dropped.extend(dropped);
        }
    }

    /// Whether `entity` has a skeleton with every bone a ragdoll needs.
    pub fn can_ragdoll(&self, entity: Entity) -> bool {
        self.ecs.get::<&Animator>(entity).is_ok_and(|animator| rig(animator.skeleton()).is_some())
    }

    /// Whether `entity` lies as a ragdoll.
    pub fn is_ragdoll(&self, entity: Entity) -> bool {
        self.ecs.get::<&Ragdoll>(entity).is_ok()
    }

    /// Sets the muscles of every ragdoll for the next physics step. The engine calls this
    /// before stepping.
    pub fn drive_ragdolls(&mut self, physics: &mut Physics, dt: f32) {
        let mut rising = Vec::new();
        for (entity, ragdoll, health) in self.ecs.query_mut::<(Entity, &mut Ragdoll, Option<&mut Health>)>() {
            ragdoll.drive(physics, dt);
            if ragdoll.knocked.is_some()
                && let Some(health) = health
            {
                health.hold_staggered(1.0);
            }
            if ragdoll.rise {
                rising.push(entity);
            }
        }
        for entity in rising {
            self.get_up(entity, physics);
        }
    }

    /// Knocks `entity` down alive, pushed by `push`: it falls as a ragdoll, lies dazed, rolls
    /// onto its face and gets up with the clip `get_up`. Returns false, doing nothing, if it
    /// is dead, already down or has no rig for it.
    pub fn knock_down(&mut self, entity: Entity, physics: &mut Physics, push: Option<Push>) -> bool {
        if self.is_ragdoll(entity)
            || !self.can_ragdoll(entity)
            || self.ecs.get::<&Health>(entity).is_ok_and(|health| health.is_dead())
        {
            return false;
        }
        if let Ok(mut fighter) = self.ecs.get::<&mut Fighter>(entity) {
            fighter.interrupt();
        }
        if let Ok(mut agent) = self.ecs.get::<&mut Agent>(entity) {
            agent.stop();
            physics.set_enabled(agent.collider(), false);
        }
        self.begin_ragdoll(entity, physics, push, false, true)
    }

    /// Whether `entity` is down and will get up.
    pub fn is_knocked_down(&self, entity: Entity) -> bool {
        self.ecs.get::<&Ragdoll>(entity).is_ok_and(|ragdoll| ragdoll.knocked.is_some())
    }

    /// Pushes the limbs of a body already lying as a ragdoll.
    fn push_ragdoll(&mut self, entity: Entity, physics: &mut Physics, push: Push) {
        let Ok(ragdoll) = self.ecs.get::<&Ragdoll>(entity) else { return };
        let nearest = ragdoll
            .bodies
            .iter()
            .enumerate()
            .filter_map(|(i, &b)| Some((i, physics.body_center(b)?.distance_squared(push.at))))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i);
        for &body in &ragdoll.bodies {
            physics.wake_body(body);
        }
        if let Some(i) = nearest {
            physics.add_impulse_at(ragdoll.bodies[i], push.impulse, push.at);
        }
    }

    /// A knocked body ready to get up: the entity stands where its hips lie, facing where its
    /// head is, and the clip takes over from the pose it lies in.
    fn get_up(&mut self, entity: Entity, physics: &mut Physics) {
        let mut query = self.ecs.query_one::<(&Ragdoll, &mut Transform, &mut Animator)>(entity);
        let Ok((ragdoll, transform, animator)) = query.get() else { return };
        let Some((hips_at, _)) = physics.body_pose(ragdoll.bodies[0]) else { return };
        let chest = ragdoll.find(Part::Chest, false);
        let chest_at = physics.body_pose(ragdoll.bodies[chest]).map_or(hips_at, |(at, _)| at);
        let head = Vec3::new(chest_at.x - hips_at.x, 0.0, chest_at.z - hips_at.z)
            .normalize_or(transform.rotation * Vec3::NEG_Z);
        let facing = (-head.x).atan2(-head.z);
        let ground = physics
            .raycast_scenery(hips_at + Vec3::Y * 0.5, Vec3::NEG_Y, 3.0)
            .map_or(hips_at.y - 0.15, |hit| hit.point.y);
        transform.rotation = Quat::from_rotation_y(facing);
        transform.position = Vec3::new(hips_at.x, ground, hips_at.z) + head * GET_UP_HIPS_BEHIND * ragdoll.scale;
        ragdoll.pose_in(physics, transform, animator);
        animator.fade_from_still(GET_UP_BLEND);
        animator.speed = 1.0;
        let lasts = animator.replay_once(GET_UP, 0.0).then(|| animator.skeleton().clip(GET_UP)).flatten();
        let lasts = lasts.map_or(0.3, |clip| animator.skeleton().clips[clip].duration);
        let position = transform.position;
        drop(query);
        remove_ragdoll(&mut self.ecs, entity, physics);
        if let Ok(mut agent) = self.ecs.get::<&mut Agent>(entity) {
            agent.teleport(physics, position, facing);
            physics.set_enabled(agent.collider(), true);
        }
        if let Ok(mut health) = self.ecs.get::<&mut Health>(entity) {
            health.set_staggered(lasts);
        }
    }

    /// The bodies heard hitting the ground since the last call, for the game to play.
    pub fn take_thuds(&mut self) -> Vec<Thud> {
        let mut thuds = Vec::new();
        for ragdoll in self.ecs.query_mut::<&mut Ragdoll>() {
            thuds.append(&mut ragdoll.thuds);
        }
        thuds
    }

    /// Shows every ragdoll where its limbs are. The engine calls this after `Game::update`.
    pub fn pose_ragdolls(&mut self, physics: &Physics) {
        for (ragdoll, transform, animator) in self.ecs.query_mut::<(&Ragdoll, &mut Transform, &mut Animator)>() {
            if !ragdoll.resting {
                ragdoll.pose(physics, transform, animator);
            }
        }
    }
}

fn transform_bits(at: Vec3) -> u32 {
    at.x.to_bits() ^ at.z.to_bits().rotate_left(16)
}

/// Takes a ragdoll's limbs out of the world, as its entity goes or gets back up.
pub(crate) fn remove_ragdoll(ecs: &mut hecs::World, entity: Entity, physics: &mut Physics) {
    if let Ok(ragdoll) = ecs.remove_one::<Ragdoll>(entity) {
        for thing in ragdoll.dropped {
            crate::world::despawn(ecs, thing, physics);
        }
        for joint in ragdoll.joints.into_iter().flatten() {
            physics.remove_joint(joint);
        }
        for body in ragdoll.bodies {
            physics.remove_body(body);
        }
    }
}

/// Whether something held at this joint is in a hand.
fn in_hand(joint: &str) -> bool {
    ["hand", "weapon", "fingers", "thumb", "index"].iter().any(|start| joint.starts_with(start))
}

/// A living body jolted by a blow: a spring at each joint between where it landed and the
/// spine, set moving by the blow's turn about that joint (lever × push ÷ the weight it swings),
/// on top of whatever plays. The trunk twists away, the head whips and the limb flies, then
/// all of it swings back.
pub struct Recoil {
    /// Joint, its turn (a rotation vector in its parent's frame) and how fast that changes.
    springs: Vec<(usize, Vec3, Vec3)>,
    age: f32,
}

/// How quickly a jolted joint swings back (rad/s) and how much it overshoots.
const RECOIL_FREQUENCY: f32 = 11.0;
const RECOIL_DAMPING_RATIO: f32 = 0.42;
/// The most a joint turns, in radians, and the fastest it starts.
const RECOIL_MOST: f32 = 0.75;
const RECOIL_FASTEST: f32 = 9.0;
/// The share of a blow's push a living body takes in its joints: the rest is in its feet.
const RECOIL_SHARE: f32 = 0.2;
/// Seconds after which a jolt is over.
const RECOIL_SECONDS: f32 = 1.6;
/// The head lags behind the chest by this much of its turn.
const WHIPLASH: f32 = 0.55;

/// What a joint swings, in kg·m², for a recoil; the joints not here don't move.
fn swing_inertia(joint: &str) -> Option<f32> {
    Some(match joint.trim_end_matches(".L").trim_end_matches(".R") {
        "spine" => 3.2,
        "chest" => 2.4,
        "neck" => 0.35,
        "upper_arm" => 0.22,
        "forearm" => 0.07,
        "thigh" => 1.8,
        "shin" => 0.6,
        _ => return None,
    })
}

impl World {
    /// Jolts a living body with a blow's push: see [`Recoil`].
    pub fn recoil(&mut self, entity: Entity, push: Push) {
        let mut query = self.ecs.query_one::<(&Transform, &Animator)>(entity);
        let Ok((transform, animator)) = query.get() else { return };
        let skeleton = animator.skeleton().clone();
        let model = transform.matrix();
        let world_of = |j: usize| -> Option<Mat4> { Some(model * animator.joint_transform(&skeleton.joints[j].name)?) };
        // The joint nearest the blow that can swing, then up the chain from it.
        let nearest = (0..skeleton.joints.len())
            .filter(|&j| swing_inertia(&skeleton.joints[j].name).is_some())
            .filter_map(|j| Some((j, world_of(j)?.w_axis.truncate().distance_squared(push.at))))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(j, _)| j);
        let Some(nearest) = nearest else { return };
        let mut chain = Vec::new();
        let mut at = Some(nearest);
        while let Some(j) = at {
            if swing_inertia(&skeleton.joints[j].name).is_some() {
                chain.push(j);
            }
            at = skeleton.joints[j].parent;
        }
        let mut kicks = Vec::new();
        for &j in &chain {
            let Some(inertia) = swing_inertia(&skeleton.joints[j].name) else { continue };
            let Some(joint) = world_of(j) else { continue };
            let lever = push.at - joint.w_axis.truncate();
            let spin = (lever.cross(push.impulse * RECOIL_SHARE) / inertia).clamp_length_max(RECOIL_FASTEST);
            kicks.push((j, spin));
        }
        // The head stays where it was while the chest turns under it, and catches up.
        if let (Some(chest), Some(neck)) = (skeleton.joint("chest"), skeleton.joint("neck"))
            && let Some(&(_, spin)) = kicks.iter().find(|(j, _)| *j == chest)
        {
            kicks.push((neck, -spin * WHIPLASH));
        }
        let kicks: Vec<(usize, Vec3)> = kicks
            .into_iter()
            .filter_map(|(j, spin)| {
                let parent = skeleton.joints[j].parent.and_then(world_of).unwrap_or(model);
                let (_, turn, _) = parent.to_scale_rotation_translation();
                Some((j, turn.inverse() * spin))
            })
            .collect();
        drop(query);
        let recoil = match self.ecs.get::<&mut Recoil>(entity) {
            Ok(mut recoil) => {
                recoil.age = 0.0;
                for (j, spin) in kicks {
                    match recoil.springs.iter_mut().find(|s| s.0 == j) {
                        Some(spring) => spring.2 += spin,
                        None => recoil.springs.push((j, Vec3::ZERO, spin)),
                    }
                }
                None
            }
            Err(_) => Some(Recoil { springs: kicks.into_iter().map(|(j, s)| (j, Vec3::ZERO, s)).collect(), age: 0.0 }),
        };
        if let Some(recoil) = recoil {
            let _ = self.ecs.insert_one(entity, recoil);
        }
    }

    /// Swings every jolted joint back towards its pose. The engine calls this before
    /// [`World::update_animations`].
    pub fn update_recoils(&mut self, dt: f32) {
        let stiffness = RECOIL_FREQUENCY * RECOIL_FREQUENCY;
        let damping = 2.0 * RECOIL_DAMPING_RATIO * RECOIL_FREQUENCY;
        let mut over = Vec::new();
        for (entity, recoil, animator) in self.ecs.query_mut::<(Entity, &mut Recoil, &mut Animator)>() {
            recoil.age += dt;
            let mut offsets = Vec::with_capacity(recoil.springs.len());
            for (joint, angle, speed) in &mut recoil.springs {
                *speed += (-stiffness * *angle - damping * *speed) * dt;
                *angle = (*angle + *speed * dt).clamp_length_max(RECOIL_MOST);
                offsets.push((*joint, Quat::from_scaled_axis(*angle)));
            }
            if recoil.age > RECOIL_SECONDS {
                animator.set_offsets(&[]);
                over.push(entity);
            } else {
                animator.set_offsets(&offsets);
            }
        }
        for entity in over {
            let _ = self.ecs.remove_one::<Recoil>(entity);
        }
    }
}
