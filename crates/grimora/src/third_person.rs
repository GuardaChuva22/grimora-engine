use grimora_core::actions::{JUMP, MOVE_BACK, MOVE_FORWARD, MOVE_LEFT, MOVE_RIGHT, RUN};
use grimora_core::{Camera, Input, Quat, Transform, Vec3};
use grimora_physics::{CharacterBody, Physics};
use serde::{Deserialize, Serialize};

/// Third-person character: walks relative to the camera, runs, jumps, and
/// keeps an orbit camera behind it that never clips into walls or terrain.
///
/// Controls: the mouse or right stick orbits the camera and the wheel zooms; moving,
/// running and jumping are the actions in `grimora_core::actions`.
#[derive(Debug, Clone)]
pub struct ThirdPersonController {
    pub body: CharacterBody,
    /// Direction the character faces, in radians (0 looks towards -Z).
    pub facing: f32,
    pub walk_speed: f32,
    pub run_speed: f32,
    /// How fast it goes sideways and backwards while it keeps facing one way.
    pub strafe_speed: f32,
    pub back_speed: f32,
    pub jump_speed: f32,
    pub gravity: f32,
    /// How quickly the character reaches its target speed (higher = snappier).
    pub acceleration: f32,
    /// Radians per mouse unit.
    pub sensitivity: f32,
    pub camera_yaw: f32,
    pub camera_pitch: f32,
    pub camera_distance: f32,
    /// Point the camera orbits, measured up from the feet.
    pub camera_height: f32,
    /// Ignores the movement and jump keys, as during a blow; the camera still turns.
    pub locked: bool,
    /// Faces where the camera looks, or the locked target, and walks without turning or
    /// running: holding a guard up, say.
    pub aiming: bool,
    /// Can't run: out of breath.
    pub exhausted: bool,
    /// A point the camera stays on instead of following the mouse. The character
    /// faces it too, unless running.
    pub lock_target: Option<Vec3>,
    /// World-space movement to add in the next `update`: what the clip's `root`
    /// joint did (see `Animator::root_delta`).
    pub root_motion: Vec3,
    velocity: Vec3,
    /// Where [`ThirdPersonController::turn_to`] is turning it.
    turn_goal: Option<f32>,
    current_distance: f32,
    shake: f32,
}

/// How quickly `turn_to` closes the gap, per second: 90 degrees take about a tenth of one.
const QUICK_TURN: f32 = 20.0;

/// What of a [`ThirdPersonController`] belongs in a saved game.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CharacterState {
    /// Feet.
    pub position: Vec3,
    pub facing: f32,
    pub velocity: Vec3,
    pub camera_yaw: f32,
    pub camera_pitch: f32,
    pub camera_distance: f32,
}

impl ThirdPersonController {
    pub fn state(&self) -> CharacterState {
        CharacterState {
            position: self.body.position,
            facing: self.facing,
            velocity: self.velocity,
            camera_yaw: self.camera_yaw,
            camera_pitch: self.camera_pitch,
            camera_distance: self.camera_distance,
        }
    }

    /// Puts the character back as saved; the camera follows on the next `update`.
    pub fn restore(&mut self, state: &CharacterState) {
        self.body.position = state.position;
        self.facing = state.facing;
        self.velocity = state.velocity;
        self.camera_yaw = state.camera_yaw;
        self.camera_pitch = state.camera_pitch;
        self.camera_distance = state.camera_distance;
        self.current_distance = state.camera_distance;
    }

    pub fn new(position: Vec3, facing: f32) -> Self {
        Self {
            body: CharacterBody::new(position, 0.35, 1.8),
            facing,
            walk_speed: 4.5,
            run_speed: 8.5,
            strafe_speed: 4.5,
            back_speed: 4.5,
            jump_speed: 7.0,
            gravity: 22.0,
            acceleration: 12.0,
            sensitivity: 0.0025,
            camera_yaw: facing,
            camera_pitch: -0.3,
            camera_distance: 5.5,
            camera_height: 1.6,
            locked: false,
            aiming: false,
            exhausted: false,
            lock_target: None,
            root_motion: Vec3::ZERO,
            velocity: Vec3::ZERO,
            turn_goal: None,
            current_distance: 5.5,
            shake: 0.0,
        }
    }

    pub fn velocity(&self) -> Vec3 {
        self.velocity
    }

    /// Adds to its speed, as a blow would; it slows back down by itself.
    pub fn push(&mut self, velocity: Vec3) {
        self.velocity += velocity;
    }

    /// Transform for the character's mesh (feet at the origin, facing -Z).
    pub fn transform(&self) -> Transform {
        Transform { position: self.body.position, rotation: Quat::from_rotation_y(self.facing), scale: Vec3::ONE }
    }

    /// Shakes the camera for an instant, by about this many meters.
    pub fn shake(&mut self, amount: f32) {
        self.shake = self.shake.max(amount);
    }

    /// Where the movement keys point, flattened onto the ground; longest at 1.
    pub fn move_direction(&self, input: &Input) -> Vec3 {
        let (sin, cos) = self.camera_yaw.sin_cos();
        let forward = Vec3::new(-sin, 0.0, -cos);
        let right = Vec3::new(cos, 0.0, -sin);
        // A stick pushed partway walks slower; diagonals don't go faster.
        (forward * input.action_axis(MOVE_BACK, MOVE_FORWARD) + right * input.action_axis(MOVE_LEFT, MOVE_RIGHT))
            .clamp_length_max(1.0)
    }

    /// Turns the character to look in a direction on the ground, right away.
    pub fn face(&mut self, direction: Vec3) {
        if direction.x != 0.0 || direction.z != 0.0 {
            self.facing = (-direction.x).atan2(-direction.z);
        }
    }

    /// Swings the character round to a direction on the ground while it is `locked`, as a
    /// blow or a roll starts: fast, but in sight of the eye.
    pub fn turn_to(&mut self, direction: Vec3) {
        if direction.x != 0.0 || direction.z != 0.0 {
            self.turn_goal = Some((-direction.x).atan2(-direction.z));
        }
    }

    /// Whether it faces the locked target now, rather than where it walks.
    pub fn strafing(&self, input: &Input) -> bool {
        self.aiming || (self.lock_target.is_some() && !(input.action(RUN) && !self.exhausted))
    }

    /// How fast it can go in a direction on the ground while it keeps facing the same way.
    fn strafing_speed(&self, direction: Vec3) -> f32 {
        let local = Quat::from_rotation_y(-self.facing) * direction;
        let (side, along) = (local.x.abs(), local.z.abs());
        let ahead = if local.z <= 0.0 { self.walk_speed } else { self.back_speed };
        (self.strafe_speed * side + ahead * along) / (side + along).max(1e-5)
    }

    /// Reads input, moves the body through `physics` (pushing dynamic bodies
    /// in the way) and places `camera`.
    pub fn update(&mut self, input: &Input, physics: &mut Physics, camera: &mut Camera, dt: f32) {
        if let Some(target) = self.lock_target {
            self.aim_camera(target, dt);
        } else {
            let look = input.look_delta(dt) * self.sensitivity;
            self.camera_yaw -= look.x;
            self.camera_pitch = (self.camera_pitch - look.y).clamp(-1.2, 0.6);
        }
        self.camera_distance = (self.camera_distance * 0.9f32.powf(input.scroll())).clamp(2.0, 14.0);

        // Movement is relative to where the camera looks.
        let wish = if self.locked { Vec3::ZERO } else { self.move_direction(input) };
        let running = input.action(RUN) && !self.exhausted;
        let speed = if self.locked {
            0.0
        } else if self.strafing(input) {
            self.strafing_speed(wish)
        } else if running {
            self.run_speed
        } else {
            self.walk_speed
        };
        let target = wish * speed;

        // Exponential smoothing: frame-rate independent acceleration. Less control in the air.
        let control = if self.body.grounded { self.acceleration } else { self.acceleration * 0.3 };
        let blend = 1.0 - (-control * dt).exp();
        let horizontal = Vec3::new(self.velocity.x, 0.0, self.velocity.z).lerp(target, blend);

        let mut vertical = self.velocity.y - self.gravity * dt;
        if self.body.grounded {
            vertical = vertical.max(-1.0);
            if input.action_just_pressed(JUMP) && !self.locked {
                vertical = self.jump_speed;
            }
        }
        self.velocity = horizontal + Vec3::Y * vertical;

        let before = self.body.position;
        let motion = self.velocity * dt + std::mem::take(&mut self.root_motion);
        self.body.move_and_slide(physics, motion, dt);
        // Hitting a ceiling or landing stops vertical motion.
        let moved_y = (self.body.position.y - before.y) / dt.max(1e-5);
        if (self.body.grounded && self.velocity.y < 0.0) || (self.velocity.y > 0.0 && moved_y < self.velocity.y * 0.5) {
            self.velocity.y = 0.0;
        }

        if !self.locked {
            self.turn_goal = None;
        }
        if let Some(goal) = self.turn_goal {
            self.facing = lerp_angle(self.facing, goal, 1.0 - (-QUICK_TURN * dt).exp());
        } else if let (Some(target), true, false) = (self.lock_target, self.strafing(input), self.locked) {
            let to = target - self.body.position;
            self.facing = lerp_angle(self.facing, (-to.x).atan2(-to.z), 1.0 - (-14.0 * dt).exp());
        } else if self.aiming && !self.locked {
            self.facing = lerp_angle(self.facing, self.camera_yaw, 1.0 - (-14.0 * dt).exp());
        } else if wish != Vec3::ZERO {
            let desired = (-wish.x).atan2(-wish.z);
            self.facing = lerp_angle(self.facing, desired, 1.0 - (-14.0 * dt).exp());
        }

        self.place_camera(physics, camera, dt);
    }

    /// Turns the camera towards `target`, looking past the character and a little down.
    fn aim_camera(&mut self, target: Vec3, dt: f32) {
        let to = target - (self.body.position + Vec3::Y * self.camera_height);
        let flat = Vec3::new(to.x, 0.0, to.z).length();
        let blend = 1.0 - (-9.0 * dt).exp();
        self.camera_yaw = lerp_angle(self.camera_yaw, (-to.x).atan2(-to.z), blend);
        let pitch = (to.y.atan2(flat.max(1.0)) * 0.6 - 0.2).clamp(-0.9, 0.3);
        self.camera_pitch += (pitch - self.camera_pitch) * blend;
    }

    fn place_camera(&mut self, physics: &Physics, camera: &mut Camera, dt: f32) {
        let pivot = self.body.position + Vec3::Y * self.camera_height;
        let (sy, cy) = self.camera_yaw.sin_cos();
        let (sp, cp) = self.camera_pitch.sin_cos();
        let look = Vec3::new(-sy * cp, sp, -cy * cp);

        // Pull the camera in front of anything between it and the character;
        // ease back out so it doesn't pop.
        let wanted = physics
            .raycast_scenery(pivot, -look, self.camera_distance + 0.3)
            .map_or(self.camera_distance, |hit| (hit.distance - 0.3).max(0.6));
        self.current_distance = if wanted < self.current_distance {
            wanted
        } else {
            self.current_distance + (wanted - self.current_distance) * (1.0 - (-4.0 * dt).exp())
        };

        camera.position = pivot - look * self.current_distance;
        if self.shake > 0.001 {
            // Fast and not quite repeating; fades out in a fraction of a second.
            let t = self.shake * 170.0;
            camera.position += Vec3::new(t.sin(), (t * 1.31).cos(), (t * 0.77).sin()) * self.shake;
            self.shake *= (-14.0 * dt).exp();
        }
        camera.yaw = self.camera_yaw;
        camera.pitch = self.camera_pitch;
    }
}

pub(crate) fn lerp_angle(from: f32, to: f32, t: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let delta = (to - from + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI;
    from + delta * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerp_angle_takes_the_short_way() {
        let a = lerp_angle(3.0, -3.0, 0.5);
        // 3.0 → -3.0 is 0.28 rad across ±π, not 6 rad the long way round.
        assert!((a - (3.0 + 0.1416)).abs() < 1e-3, "{a}");
    }
}
