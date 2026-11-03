use std::collections::HashSet;

use glam::Vec2;
pub use winit::event::MouseButton;
pub use winit::keyboard::KeyCode;

use crate::actions::{ActionMap, Binding, PadAxis, PadButton};

/// Stick travel below this reads as zero: sticks rarely rest exactly at the center.
const DEAD_ZONE: f32 = 0.2;
/// A stick direction counts as pressed past this.
const STICK_PRESSED: f32 = 0.5;
/// Mouse units per second a stick pushed all the way turns the view.
const STICK_LOOK_SPEED: f32 = 1100.0;

/// Keyboard and mouse state for the current frame.
///
/// "Pressed" means held down; "just pressed" is true only on the frame the
/// key went down. The runner feeds events in and calls [`Input::end_frame`].
#[derive(Debug)]
pub struct Input {
    keys: HashSet<KeyCode>,
    keys_just_pressed: HashSet<KeyCode>,
    keys_just_released: HashSet<KeyCode>,
    keys_repeated: HashSet<KeyCode>,
    buttons: HashSet<MouseButton>,
    buttons_just_pressed: HashSet<MouseButton>,
    buttons_just_released: HashSet<MouseButton>,
    mouse_delta: Vec2,
    scroll: f32,
    cursor: Vec2,
    pad_buttons: HashSet<PadButton>,
    pad_just_pressed: HashSet<PadButton>,
    pad_just_released: HashSet<PadButton>,
    /// Menu directions the gamepad moved this frame (see [`Input::pad_nav`]).
    pad_nav: HashSet<PadButton>,
    /// Indexed by [`axis_index`]; the dead zone is already out.
    pad_axes: [f32; 4],
    pad_axes_before: [f32; 4],
    gamepad_last: bool,
    /// Which controls trigger each action; survives [`Input::clear`].
    pub actions: ActionMap,
    /// Multiplies mouse and stick look: 1 is the engine's default.
    pub look_sensitivity: f32,
    /// Pushing the mouse or stick forward looks down.
    pub invert_look_y: bool,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            keys: HashSet::new(),
            keys_just_pressed: HashSet::new(),
            keys_just_released: HashSet::new(),
            keys_repeated: HashSet::new(),
            buttons: HashSet::new(),
            buttons_just_pressed: HashSet::new(),
            buttons_just_released: HashSet::new(),
            mouse_delta: Vec2::ZERO,
            scroll: 0.0,
            cursor: Vec2::ZERO,
            pad_buttons: HashSet::new(),
            pad_just_pressed: HashSet::new(),
            pad_just_released: HashSet::new(),
            pad_nav: HashSet::new(),
            pad_axes: [0.0; 4],
            pad_axes_before: [0.0; 4],
            gamepad_last: false,
            actions: ActionMap::default(),
            look_sensitivity: 1.0,
            invert_look_y: false,
        }
    }
}

fn axis_index(axis: PadAxis) -> usize {
    match axis {
        PadAxis::LeftX => 0,
        PadAxis::LeftY => 1,
        PadAxis::RightX => 2,
        PadAxis::RightY => 3,
    }
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

    /// True on the frames the system repeats a held key (never on the first
    /// press): menus move on this, gameplay usually doesn't.
    pub fn just_repeated(&self, key: KeyCode) -> bool {
        self.keys_repeated.contains(&key)
    }

    pub fn mouse_pressed(&self, button: MouseButton) -> bool {
        self.buttons.contains(&button)
    }

    pub fn mouse_just_pressed(&self, button: MouseButton) -> bool {
        self.buttons_just_pressed.contains(&button)
    }

    pub fn mouse_just_released(&self, button: MouseButton) -> bool {
        self.buttons_just_released.contains(&button)
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

    /// Whether something bound to `action` is held.
    pub fn action(&self, action: &str) -> bool {
        self.actions.bindings(action).iter().any(|binding| self.binding_value(*binding) >= STICK_PRESSED)
    }

    /// True on the frame something bound to `action` went down.
    pub fn action_just_pressed(&self, action: &str) -> bool {
        self.actions.bindings(action).iter().any(|binding| self.binding_just_pressed(*binding))
    }

    /// How far `action` is pushed, from 0 to 1: sticks give the values in between.
    pub fn action_value(&self, action: &str) -> f32 {
        self.actions.bindings(action).iter().map(|binding| self.binding_value(*binding)).fold(0.0, f32::max)
    }

    /// `positive` minus `negative`, from -1 to 1.
    pub fn action_axis(&self, negative: &str, positive: &str) -> f32 {
        self.action_value(positive) - self.action_value(negative)
    }

    fn binding_value(&self, binding: Binding) -> f32 {
        match binding {
            Binding::Key(key) => f32::from(u8::from(self.pressed(key))),
            Binding::Mouse(button) => f32::from(u8::from(self.mouse_pressed(button))),
            Binding::Pad(button) => f32::from(u8::from(self.pad_pressed(button))),
            Binding::Stick(axis, positive) => {
                let value = self.pad_axis(axis);
                (if positive { value } else { -value }).max(0.0)
            }
        }
    }

    fn binding_just_pressed(&self, binding: Binding) -> bool {
        match binding {
            Binding::Key(key) => self.just_pressed(key),
            Binding::Mouse(button) => self.mouse_just_pressed(button),
            Binding::Pad(button) => self.pad_just_pressed(button),
            Binding::Stick(axis, positive) => {
                let sign = if positive { 1.0 } else { -1.0 };
                let index = axis_index(axis);
                self.pad_axes[index] * sign >= STICK_PRESSED && self.pad_axes_before[index] * sign < STICK_PRESSED
            }
        }
    }

    /// What was pressed this frame, for a screen that asks for a new control: a
    /// key, a mouse button, a gamepad button or a stick pushed in some direction.
    pub fn first_pressed(&self) -> Option<Binding> {
        let keys = self.keys_just_pressed.iter().map(|key| Binding::Key(*key));
        let mouse = self.buttons_just_pressed.iter().map(|button| Binding::Mouse(*button));
        let pad = self.pad_just_pressed.iter().map(|button| Binding::Pad(*button));
        let sticks = [PadAxis::LeftX, PadAxis::LeftY, PadAxis::RightX, PadAxis::RightY]
            .into_iter()
            .flat_map(|axis| [Binding::Stick(axis, true), Binding::Stick(axis, false)])
            .filter(|binding| self.binding_just_pressed(*binding));
        keys.chain(mouse).chain(pad).chain(sticks).next()
    }

    /// How far to turn the view this frame, in mouse units (x right, y down):
    /// the mouse plus the right stick, with the player's sensitivity and inversion.
    pub fn look_delta(&self, dt: f32) -> Vec2 {
        let stick = Vec2::new(self.pad_axis(PadAxis::RightX), -self.pad_axis(PadAxis::RightY));
        let mut look = (self.mouse_delta + stick * STICK_LOOK_SPEED * dt) * self.look_sensitivity;
        if self.invert_look_y {
            look.y = -look.y;
        }
        look
    }

    pub fn pad_pressed(&self, button: PadButton) -> bool {
        self.pad_buttons.contains(&button)
    }

    pub fn pad_just_pressed(&self, button: PadButton) -> bool {
        self.pad_just_pressed.contains(&button)
    }

    pub fn pad_just_released(&self, button: PadButton) -> bool {
        self.pad_just_released.contains(&button)
    }

    /// From -1 to 1, with 0 inside the dead zone.
    pub fn pad_axis(&self, axis: PadAxis) -> f32 {
        self.pad_axes[axis_index(axis)]
    }

    /// True on the frames the gamepad moves through a menu in this direction
    /// (`DPadUp`...): the D-pad or the left stick, repeating while held.
    pub fn pad_nav(&self, direction: PadButton) -> bool {
        self.pad_nav.contains(&direction)
    }

    /// Whether the player last used the gamepad, so hints can show its buttons.
    pub fn gamepad_last(&self) -> bool {
        self.gamepad_last
    }

    pub fn pad_button_down(&mut self, button: PadButton) {
        self.gamepad_last = true;
        if self.pad_buttons.insert(button) {
            self.pad_just_pressed.insert(button);
            if matches!(button, PadButton::DPadUp | PadButton::DPadDown | PadButton::DPadLeft | PadButton::DPadRight) {
                self.pad_nav.insert(button);
            }
        }
    }

    pub fn pad_button_up(&mut self, button: PadButton) {
        if self.pad_buttons.remove(&button) {
            self.pad_just_released.insert(button);
        }
    }

    /// Sets a stick axis, taking out the dead zone and rescaling what is left to 0..1.
    pub fn set_pad_axis(&mut self, axis: PadAxis, value: f32) {
        let size = value.abs();
        let value =
            if size < DEAD_ZONE { 0.0 } else { value.signum() * ((size - DEAD_ZONE) / (1.0 - DEAD_ZONE)).min(1.0) };
        if value != 0.0 {
            self.gamepad_last = true;
        }
        self.pad_axes[axis_index(axis)] = value;
    }

    /// The gamepad moved through a menu in this direction (`DPadUp`...).
    pub fn add_pad_nav(&mut self, direction: PadButton) {
        self.gamepad_last = true;
        self.pad_nav.insert(direction);
    }

    pub fn key_down(&mut self, key: KeyCode) {
        self.gamepad_last = false;
        if self.keys.insert(key) {
            self.keys_just_pressed.insert(key);
        }
    }

    /// The system repeated a held key. A key the engine thought was up (focus
    /// came back with it held) counts as a fresh press.
    pub fn key_repeat(&mut self, key: KeyCode) {
        if self.keys.contains(&key) {
            self.keys_repeated.insert(key);
        } else {
            self.key_down(key);
        }
    }

    pub fn key_up(&mut self, key: KeyCode) {
        if self.keys.remove(&key) {
            self.keys_just_released.insert(key);
        }
    }

    pub fn button_down(&mut self, button: MouseButton) {
        self.gamepad_last = false;
        if self.buttons.insert(button) {
            self.buttons_just_pressed.insert(button);
        }
    }

    pub fn button_up(&mut self, button: MouseButton) {
        if self.buttons.remove(&button) {
            self.buttons_just_released.insert(button);
        }
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

    /// Releases everything, e.g. when the window loses focus. The player's
    /// choices stay.
    pub fn clear(&mut self) {
        *self = Self {
            cursor: self.cursor,
            gamepad_last: self.gamepad_last,
            actions: std::mem::take(&mut self.actions),
            look_sensitivity: self.look_sensitivity,
            invert_look_y: self.invert_look_y,
            ..Self::default()
        };
    }

    /// Clears per-frame state. Called by the runner after the game update.
    pub fn end_frame(&mut self) {
        self.keys_just_pressed.clear();
        self.keys_just_released.clear();
        self.keys_repeated.clear();
        self.buttons_just_pressed.clear();
        self.buttons_just_released.clear();
        self.mouse_delta = Vec2::ZERO;
        self.scroll = 0.0;
        self.pad_just_pressed.clear();
        self.pad_just_released.clear();
        self.pad_nav.clear();
        self.pad_axes_before = self.pad_axes;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::{JUMP, MOVE_BACK, MOVE_FORWARD};

    #[test]
    fn mouse_release_and_key_repeat_last_one_frame() {
        let mut input = Input::default();
        input.button_down(MouseButton::Left);
        input.end_frame();
        input.button_up(MouseButton::Left);
        assert!(input.mouse_just_released(MouseButton::Left));
        input.end_frame();
        assert!(!input.mouse_just_released(MouseButton::Left));

        input.key_repeat(KeyCode::ArrowDown);
        assert!(input.just_pressed(KeyCode::ArrowDown), "repeat of a key we thought was up is a press");
        assert!(!input.just_repeated(KeyCode::ArrowDown));
        input.end_frame();
        input.key_repeat(KeyCode::ArrowDown);
        assert!(input.just_repeated(KeyCode::ArrowDown) && !input.just_pressed(KeyCode::ArrowDown));
        input.end_frame();
        assert!(!input.just_repeated(KeyCode::ArrowDown));
    }

    #[test]
    fn actions_follow_their_bindings_on_any_device() {
        let mut input = Input::default();
        input.key_down(KeyCode::Space);
        assert!(input.action(JUMP) && input.action_just_pressed(JUMP));
        input.end_frame();
        assert!(input.action(JUMP) && !input.action_just_pressed(JUMP));
        input.key_up(KeyCode::Space);
        assert!(!input.action(JUMP));

        input.pad_button_down(PadButton::South);
        assert!(input.action_just_pressed(JUMP) && input.gamepad_last());
        input.pad_button_up(PadButton::South);
        input.end_frame();

        input.actions.rebind(JUMP, Binding::Key(KeyCode::KeyJ));
        input.key_down(KeyCode::Space);
        assert!(!input.action(JUMP));
        input.key_down(KeyCode::KeyJ);
        assert!(input.action(JUMP) && !input.gamepad_last());
    }

    #[test]
    fn sticks_have_a_dead_zone_and_analog_values() {
        let mut input = Input::default();
        input.set_pad_axis(PadAxis::LeftY, 0.15);
        assert_eq!(input.action_value(MOVE_FORWARD), 0.0);
        input.set_pad_axis(PadAxis::LeftY, 0.6);
        assert!((input.action_value(MOVE_FORWARD) - 0.5).abs() < 1e-5);
        assert_eq!(input.action_value(MOVE_BACK), 0.0);
        input.set_pad_axis(PadAxis::LeftY, -1.0);
        assert_eq!(input.action_axis(MOVE_BACK, MOVE_FORWARD), -1.0);

        // As a button, a stick presses once when it crosses the middle of its travel.
        input.end_frame();
        input.set_pad_axis(PadAxis::LeftX, 1.0);
        assert_eq!(input.first_pressed(), Some(Binding::Stick(PadAxis::LeftX, true)));
        input.end_frame();
        assert_eq!(input.first_pressed(), None);
    }

    #[test]
    fn looking_uses_the_players_settings() {
        let mut input = Input::default();
        input.add_mouse_delta(Vec2::new(10.0, 20.0));
        assert_eq!(input.look_delta(0.1), Vec2::new(10.0, 20.0));
        input.look_sensitivity = 2.0;
        input.invert_look_y = true;
        assert_eq!(input.look_delta(0.1), Vec2::new(20.0, -40.0));
        input.end_frame();
        input.set_pad_axis(PadAxis::RightX, 1.0);
        assert!(input.look_delta(0.1).x > 100.0);
    }

    #[test]
    fn clearing_keeps_the_players_choices() {
        let mut input = Input::default();
        input.actions.rebind(JUMP, Binding::Key(KeyCode::KeyJ));
        input.look_sensitivity = 3.0;
        input.key_down(KeyCode::KeyJ);
        input.clear();
        assert!(!input.action(JUMP));
        assert_eq!(input.actions.label(JUMP, false), "J");
        assert_eq!(input.look_sensitivity, 3.0);
    }
}
