use grimora_core::{Camera, Input, KeyCode, Quat, Transform, Vec3};
use grimora_physics::{CharacterBody, Physics};
use serde::{Deserialize, Serialize};

/// Third-person character: walks relative to the camera, runs, jumps, and
/// keeps an orbit camera behind it that never clips into walls or terrain.
///
/// Controls: mouse orbits the camera, WASD moves, Shift runs, Space jumps,
/// mouse wheel zooms.
#[derive(Debug, Clone)]
pub struct ThirdPersonController {
    pub body: CharacterBody,
    /// Direction the character faces, in radians (0 looks towards -Z).
    pub facing: f32,
    pub walk_speed: f32,
    pub run_speed: f32,
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
    velocity: Vec3,
    current_distance: f32,
}

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
            jump_speed: 7.0,
            gravity: 22.0,
            acceleration: 12.0,
            sensitivity: 0.0025,
            camera_yaw: facing,
            camera_pitch: -0.3,
            camera_distance: 5.5,
            camera_height: 1.6,
            velocity: Vec3::ZERO,
            current_distance: 5.5,
        }
    }

    pub fn velocity(&self) -> Vec3 {
        self.velocity
    }

    /// Transform for the character's mesh (feet at the origin, facing -Z).
    pub fn transform(&self) -> Transform {
        Transform { position: self.body.position, rotation: Quat::from_rotation_y(self.facing), scale: Vec3::ONE }
    }

    /// Reads input, moves the body through `physics` (pushing dynamic bodies
    /// in the way) and places `camera`.
    pub fn update(&mut self, input: &Input, physics: &mut Physics, camera: &mut Camera, dt: f32) {
        let look = input.mouse_delta() * self.sensitivity;
        self.camera_yaw -= look.x;
        self.camera_pitch = (self.camera_pitch - look.y).clamp(-1.2, 0.6);
        self.camera_distance = (self.camera_distance * 0.9f32.powf(input.scroll())).clamp(2.0, 14.0);

        // Movement is relative to where the camera looks, flattened onto the ground.
        let (sin, cos) = self.camera_yaw.sin_cos();
        let forward = Vec3::new(-sin, 0.0, -cos);
        let right = Vec3::new(cos, 0.0, -sin);
        let wish = (forward * input.axis(KeyCode::KeyS, KeyCode::KeyW)
            + right * input.axis(KeyCode::KeyA, KeyCode::KeyD))
        .normalize_or_zero();
        let running = input.pressed(KeyCode::ShiftLeft) || input.pressed(KeyCode::ShiftRight);
        let target = wish * if running { self.run_speed } else { self.walk_speed };

        // Exponential smoothing: frame-rate independent acceleration. Less control in the air.
        let control = if self.body.grounded { self.acceleration } else { self.acceleration * 0.3 };
        let blend = 1.0 - (-control * dt).exp();
        let horizontal = Vec3::new(self.velocity.x, 0.0, self.velocity.z).lerp(target, blend);

        let mut vertical = self.velocity.y - self.gravity * dt;
        if self.body.grounded {
            vertical = vertical.max(-1.0);
            if input.just_pressed(KeyCode::Space) {
                vertical = self.jump_speed;
            }
        }
        self.velocity = horizontal + Vec3::Y * vertical;

        let before = self.body.position;
        self.body.move_and_slide(physics, self.velocity * dt, dt);
        // Hitting a ceiling or landing stops vertical motion.
        let moved_y = (self.body.position.y - before.y) / dt.max(1e-5);
        if (self.body.grounded && self.velocity.y < 0.0) || (self.velocity.y > 0.0 && moved_y < self.velocity.y * 0.5) {
            self.velocity.y = 0.0;
        }

        if wish != Vec3::ZERO {
            let desired = (-wish.x).atan2(-wish.z);
            self.facing = lerp_angle(self.facing, desired, 1.0 - (-14.0 * dt).exp());
        }

        self.place_camera(physics, camera, dt);
    }

    fn place_camera(&mut self, physics: &Physics, camera: &mut Camera, dt: f32) {
        let pivot = self.body.position + Vec3::Y * self.camera_height;
        let (sy, cy) = self.camera_yaw.sin_cos();
        let (sp, cp) = self.camera_pitch.sin_cos();
        let look = Vec3::new(-sy * cp, sp, -cy * cp);

        // Pull the camera in front of anything between it and the character;
        // ease back out so it doesn't pop.
        let wanted = physics
            .raycast(pivot, -look, self.camera_distance + 0.3)
            .map_or(self.camera_distance, |hit| (hit.distance - 0.3).max(0.6));
        self.current_distance = if wanted < self.current_distance {
            wanted
        } else {
            self.current_distance + (wanted - self.current_distance) * (1.0 - (-4.0 * dt).exp())
        };

        camera.position = pivot - look * self.current_distance;
        camera.yaw = self.camera_yaw;
        camera.pitch = self.camera_pitch;
    }
}

fn lerp_angle(from: f32, to: f32, t: f32) -> f32 {
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
