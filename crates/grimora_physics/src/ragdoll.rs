//! Bodies and joints for ragdolls: limbs that tumble under their own weight, held together by
//! anatomical joints, each with a muscle (a motor) pulling it towards a pose with whatever
//! tone the game gives it.

use grimora_core::{Quat, Vec3};
use rapier3d::math::Pose;
use rapier3d::prelude::{
    ColliderBuilder, GenericJointBuilder, Group, ImpulseJointHandle, InteractionGroups, InteractionTestMode,
    JointAxesMask, JointAxis, MotorModel, RigidBodyBuilder,
};

use crate::{BodyHandle, Physics, from_rapier, rotation, to_rapier};

/// The collision group of ragdoll limbs: characters, cameras and path searches pass
/// through them, and they don't catch on the capsules of the living.
pub(crate) const RAGDOLLS: Group = Group::GROUP_30;

/// The shape of a limb, in its own frame (its joint's origin).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LimbShape {
    Capsule { from: Vec3, to: Vec3, radius: f32 },
    Ball { center: Vec3, radius: f32 },
    Cuboid { center: Vec3, half_extents: Vec3 },
}

/// A limb to add: where it is and how it's moving when it goes limp.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LimbDesc {
    pub shape: LimbShape,
    pub mass: f32,
    pub position: Vec3,
    pub rotation: Quat,
    pub linear: Vec3,
    pub angular: Vec3,
}

/// A joint between two limbs. Each frame is where the joint sits in that limb's own frame,
/// the same place in the world when the joint is at rest: X is the axis the joint bends
/// around (forward, for a knee or an elbow), Y runs along the child limb (its twist), and
/// `limits` are the angles, in radians, it may turn around each.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SocketDesc {
    pub parent: BodyHandle,
    pub child: BodyHandle,
    pub parent_frame: (Vec3, Quat),
    pub child_frame: (Vec3, Quat),
    pub limits: [[f32; 2]; 3],
}

/// A joint from [`Physics::add_socket`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct JointHandle(ImpulseJointHandle);

const ANGLES: [JointAxis; 3] = [JointAxis::AngX, JointAxis::AngY, JointAxis::AngZ];

impl Physics {
    /// Adds a limb of a ragdoll: a dynamic body that collides with the scenery and other
    /// limbs, but not with the capsules of characters, and that characters walk through.
    pub fn add_limb(&mut self, desc: &LimbDesc) -> BodyHandle {
        let builder = match desc.shape {
            LimbShape::Capsule { from, to, radius } => {
                ColliderBuilder::capsule_from_endpoints(to_rapier(from), to_rapier(to), radius)
            }
            LimbShape::Ball { center, radius } => ColliderBuilder::ball(radius).translation(to_rapier(center)),
            LimbShape::Cuboid { center, half_extents: h } => {
                ColliderBuilder::cuboid(h.x, h.y, h.z).translation(to_rapier(center))
            }
        };
        let groups = InteractionGroups::new(RAGDOLLS, Group::ALL.difference(crate::AGENTS), InteractionTestMode::And);
        let collider = builder.mass(desc.mass).friction(0.9).restitution(0.0).collision_groups(groups);
        let body = RigidBodyBuilder::dynamic()
            .pose(Pose::from_parts(to_rapier(desc.position), rotation(desc.rotation)))
            .linvel(to_rapier(desc.linear))
            .angvel(to_rapier(desc.angular))
            .linear_damping(0.05)
            .angular_damping(0.6)
            .ccd_enabled(true);
        let (handle, _) = self.world.insert(body, collider);
        BodyHandle(handle)
    }

    /// Joins two limbs at a ball joint with limits on each of its three angles. The two limbs
    /// don't collide with each other.
    pub fn add_socket(&mut self, desc: &SocketDesc) -> JointHandle {
        let frame = |(at, turn): (Vec3, Quat)| Pose::from_parts(to_rapier(at), rotation(turn));
        let mut joint = GenericJointBuilder::new(JointAxesMask::LIN_AXES)
            .local_frame1(frame(desc.parent_frame))
            .local_frame2(frame(desc.child_frame))
            .contacts_enabled(false);
        for (axis, limits) in ANGLES.into_iter().zip(desc.limits) {
            joint = joint.limits(axis, limits).motor_model(axis, MotorModel::AccelerationBased);
        }
        JointHandle(self.world.impulse_joints.insert(desc.parent.0, desc.child.0, joint.build(), true))
    }

    /// Pulls a joint towards `target`, the child's turn in the joint's frame (identity is the
    /// rest pose), as a muscle would: `stiffness` and `damping` are per second squared and per
    /// second, whatever the limbs weigh, and `strength` is the most torque it gives, in N·m.
    /// With a stiffness of 0, only the damping is left: a dead joint's stiffness.
    pub fn set_muscle(&mut self, joint: JointHandle, target: Quat, stiffness: f32, damping: f32, strength: f32) {
        let Some(joint) = self.world.impulse_joints.get_mut(joint.0, false) else { return };
        // The motor follows each imaginary part of the turn, as the joint's own error does.
        let target = if target.w < 0.0 { -target } else { target };
        let parts = [target.x, target.y, target.z];
        for (axis, part) in ANGLES.into_iter().zip(parts) {
            let angle = 2.0 * part.clamp(-1.0, 1.0).asin();
            joint.data.set_motor_position(axis, angle, stiffness, damping);
            joint.data.set_motor_max_force(axis, strength);
        }
    }

    /// Removes a joint from [`Physics::add_socket`].
    pub fn remove_joint(&mut self, joint: JointHandle) {
        self.world.impulse_joints.remove(joint.0, true);
    }

    /// Whether a dynamic body has come to rest and stopped being simulated.
    pub fn body_sleeping(&self, body: BodyHandle) -> bool {
        self.world.bodies.get(body.0).is_none_or(|b| b.is_sleeping())
    }

    /// Wakes a sleeping body, as when something new pushes it.
    pub fn wake_body(&mut self, body: BodyHandle) {
        if let Some(body) = self.world.bodies.get_mut(body.0) {
            body.wake_up(true);
        }
    }

    /// Adds to a dynamic body's spin, in radians per second about each world axis.
    pub fn add_spin(&mut self, body: BodyHandle, spin: Vec3) {
        if let Some(body) = self.world.bodies.get_mut(body.0) {
            let now = body.angvel();
            body.set_angvel(now + to_rapier(spin), true);
        }
    }

    /// Where a dynamic body's center of mass is, in the world.
    pub fn body_center(&self, body: BodyHandle) -> Option<Vec3> {
        Some(from_rapier(self.world.bodies.get(body.0)?.center_of_mass()))
    }

    /// A dynamic body's mass, in kilograms.
    pub fn body_mass(&self, body: BodyHandle) -> f32 {
        self.world.bodies.get(body.0).map_or(0.0, |b| b.mass())
    }
}
