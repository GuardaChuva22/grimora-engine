use grimora_core::{Camera, Input, KeyCode, MouseButton, Vec3};

/// Free-flying debug camera: hold the right mouse button to look around,
/// WASD to move, E/Q (or Space/Ctrl) for up/down, Shift to go fast. Its keys
/// are fixed, since it is a tool for whoever is building the game, but the
/// player's look sensitivity applies.
#[derive(Debug, Clone)]
pub struct FlyCamera {
    pub speed: f32,
    pub fast_multiplier: f32,
    /// Radians per mouse unit.
    pub sensitivity: f32,
}

impl Default for FlyCamera {
    fn default() -> Self {
        Self { speed: 8.0, fast_multiplier: 4.0, sensitivity: 0.0025 }
    }
}

impl FlyCamera {
    /// Moves `camera`; returns whether the cursor should be captured (looking).
    pub fn update(&mut self, camera: &mut Camera, input: &Input, dt: f32) -> bool {
        let looking = input.mouse_pressed(MouseButton::Right);
        if looking {
            let d = input.look_delta(dt) * self.sensitivity;
            camera.yaw -= d.x;
            camera.pitch = (camera.pitch - d.y).clamp(-Camera::MAX_PITCH, Camera::MAX_PITCH);
        }

        // Scroll adjusts speed, like in most editors.
        if input.scroll() != 0.0 {
            self.speed = (self.speed * 1.15f32.powf(input.scroll())).clamp(0.5, 200.0);
        }

        let forward = camera.forward();
        let right = camera.right();
        let up = input.axis(KeyCode::KeyQ, KeyCode::KeyE) + input.axis(KeyCode::ControlLeft, KeyCode::Space);
        let wish = forward * input.axis(KeyCode::KeyS, KeyCode::KeyW)
            + right * input.axis(KeyCode::KeyA, KeyCode::KeyD)
            + Vec3::Y * up.clamp(-1.0, 1.0);
        let fast = input.pressed(KeyCode::ShiftLeft) || input.pressed(KeyCode::ShiftRight);
        let speed = self.speed * if fast { self.fast_multiplier } else { 1.0 };
        camera.position += wish.normalize_or_zero() * speed * dt;
        looking
    }
}
