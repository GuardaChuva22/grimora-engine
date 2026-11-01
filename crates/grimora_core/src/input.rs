use std::collections::HashSet;

use glam::Vec2;
pub use winit::event::MouseButton;
pub use winit::keyboard::KeyCode;

/// Keyboard and mouse state for the current frame.
///
/// "Pressed" means held down; "just pressed" is true only on the frame the
/// key went down. The runner feeds events in and calls [`Input::end_frame`].
#[derive(Debug, Default)]
pub struct Input {
    keys: HashSet<KeyCode>,
    keys_just_pressed: HashSet<KeyCode>,
    keys_just_released: HashSet<KeyCode>,
    buttons: HashSet<MouseButton>,
    buttons_just_pressed: HashSet<MouseButton>,
    mouse_delta: Vec2,
    scroll: f32,
    cursor: Vec2,
}

impl Input {
    pub fn pressed(&self, key: KeyCode) -> bool {
        self.keys.contains(&key)
    }

    pub fn just_pressed(&self, key: KeyCode) -> bool {
        self.keys_just_pressed.contains(&key)
    }

    pub fn just_released(&self, key: KeyCode) -> bool {
        self.keys_just_released.contains(&key)
    }

    pub fn mouse_pressed(&self, button: MouseButton) -> bool {
        self.buttons.contains(&button)
    }

    pub fn mouse_just_pressed(&self, button: MouseButton) -> bool {
        self.buttons_just_pressed.contains(&button)
    }

    /// Raw mouse movement this frame, in device units.
    pub fn mouse_delta(&self) -> Vec2 {
        self.mouse_delta
    }

    /// Wheel movement this frame, in lines (positive = away from the user).
    pub fn scroll(&self) -> f32 {
        self.scroll
    }

    /// Cursor position in physical window pixels.
    pub fn cursor(&self) -> Vec2 {
        self.cursor
    }

    /// -1, 0 or 1 depending on which of the two keys is held.
    pub fn axis(&self, negative: KeyCode, positive: KeyCode) -> f32 {
        f32::from(u8::from(self.pressed(positive))) - f32::from(u8::from(self.pressed(negative)))
    }

    pub fn key_down(&mut self, key: KeyCode) {
        if self.keys.insert(key) {
            self.keys_just_pressed.insert(key);
        }
    }

    pub fn key_up(&mut self, key: KeyCode) {
        if self.keys.remove(&key) {
            self.keys_just_released.insert(key);
        }
    }

    pub fn button_down(&mut self, button: MouseButton) {
        if self.buttons.insert(button) {
            self.buttons_just_pressed.insert(button);
        }
    }

    pub fn button_up(&mut self, button: MouseButton) {
        self.buttons.remove(&button);
    }

    pub fn add_mouse_delta(&mut self, delta: Vec2) {
        self.mouse_delta += delta;
    }

    pub fn add_scroll(&mut self, lines: f32) {
        self.scroll += lines;
    }

    pub fn set_cursor(&mut self, position: Vec2) {
        self.cursor = position;
    }

    /// Releases everything, e.g. when the window loses focus.
    pub fn clear(&mut self) {
        *self = Self { cursor: self.cursor, ..Self::default() };
    }

    /// Clears per-frame state. Called by the runner after the game update.
    pub fn end_frame(&mut self) {
        self.keys_just_pressed.clear();
        self.keys_just_released.clear();
        self.buttons_just_pressed.clear();
        self.mouse_delta = Vec2::ZERO;
        self.scroll = 0.0;
    }
}
