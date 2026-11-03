//! Gamepads: gilrs reads the hardware and [`Gamepads::update`] feeds [`Input`].
//! With several connected, they all act as one.

use gilrs::{Axis, Button, EventType, Gilrs};
use grimora_core::{Input, PadAxis, PadButton};

/// A menu direction held this long starts to repeat, in seconds...
const NAV_FIRST_REPEAT: f32 = 0.4;
/// ...this often.
const NAV_NEXT_REPEAT: f32 = 0.12;
/// How far a stick must go to count as a menu direction.
const NAV_STICK: f32 = 0.5;

pub(crate) struct Gamepads {
    /// `None` when the system offers no way to read gamepads: the game goes on.
    gilrs: Option<Gilrs>,
}

impl Gamepads {
    pub(crate) fn new() -> Self {
        let gilrs = match Gilrs::new() {
            Ok(gilrs) => Some(gilrs),
            Err(err) => {
                log::warn!("gamepads unavailable: {err}");
                None
            }
        };
        Self { gilrs }
    }

    pub(crate) fn update(&mut self, input: &mut Input) {
        let Some(gilrs) = &mut self.gilrs else { return };
        while let Some(event) = gilrs.next_event() {
            match event.event {
                EventType::ButtonPressed(button, _) => {
                    if let Some(button) = pad_button(button) {
                        input.pad_button_down(button);
                    }
                }
                EventType::ButtonReleased(button, _) => {
                    if let Some(button) = pad_button(button) {
                        input.pad_button_up(button);
                    }
                }
                EventType::AxisChanged(axis, value, _) => {
                    if let Some(axis) = pad_axis(axis) {
                        input.set_pad_axis(axis, value);
                    }
                }
                EventType::Connected => log::info!("gamepad connected: {}", gilrs.gamepad(event.id).name()),
                EventType::Disconnected => log::info!("gamepad disconnected"),
                _ => {}
            }
        }
    }
}

fn pad_button(button: Button) -> Option<PadButton> {
    Some(match button {
        Button::South => PadButton::South,
        Button::East => PadButton::East,
        Button::West => PadButton::West,
        Button::North => PadButton::North,
        // gilrs calls the bumpers triggers, and the triggers "triggers 2".
        Button::LeftTrigger => PadButton::LeftBumper,
        Button::RightTrigger => PadButton::RightBumper,
        Button::LeftTrigger2 => PadButton::LeftTrigger,
        Button::RightTrigger2 => PadButton::RightTrigger,
        Button::Select => PadButton::Select,
        Button::Start => PadButton::Start,
        Button::LeftThumb => PadButton::LeftStick,
        Button::RightThumb => PadButton::RightStick,
        Button::DPadUp => PadButton::DPadUp,
        Button::DPadDown => PadButton::DPadDown,
        Button::DPadLeft => PadButton::DPadLeft,
        Button::DPadRight => PadButton::DPadRight,
        _ => return None,
    })
}

fn pad_axis(axis: Axis) -> Option<PadAxis> {
    Some(match axis {
        Axis::LeftStickX => PadAxis::LeftX,
        Axis::LeftStickY => PadAxis::LeftY,
        Axis::RightStickX => PadAxis::RightX,
        Axis::RightStickY => PadAxis::RightY,
        _ => return None,
    })
}

/// Turns the D-pad and left stick into menu moves that repeat while held.
#[derive(Debug, Default)]
pub(crate) struct MenuNav {
    /// Seconds to the next repeat of each direction; `None` while not held.
    held: [Option<f32>; 4],
}

impl MenuNav {
    pub(crate) fn update(&mut self, input: &mut Input, dt: f32) {
        let (x, y) = (input.pad_axis(PadAxis::LeftX), input.pad_axis(PadAxis::LeftY));
        let directions = [
            (PadButton::DPadUp, y >= NAV_STICK),
            (PadButton::DPadDown, y <= -NAV_STICK),
            (PadButton::DPadLeft, x <= -NAV_STICK),
            (PadButton::DPadRight, x >= NAV_STICK),
        ];
        for (held, (direction, stick)) in self.held.iter_mut().zip(directions) {
            if !(stick || input.pad_pressed(direction)) {
                *held = None;
                continue;
            }
            match held {
                None => {
                    input.add_pad_nav(direction);
                    *held = Some(NAV_FIRST_REPEAT);
                }
                Some(wait) => {
                    *wait -= dt;
                    if *wait <= 0.0 {
                        input.add_pad_nav(direction);
                        *wait = NAV_NEXT_REPEAT;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moves(nav: &mut MenuNav, input: &mut Input, frames: u32, direction: PadButton) -> u32 {
        let mut count = 0;
        for _ in 0..frames {
            nav.update(input, 0.05);
            count += u32::from(input.pad_nav(direction));
            input.end_frame();
        }
        count
    }

    #[test]
    fn held_directions_move_once_and_then_repeat() {
        let (mut nav, mut input) = (MenuNav::default(), Input::default());
        input.pad_button_down(PadButton::DPadDown);
        // First the press, then nothing for 0.4 s, then every 0.12 s.
        assert_eq!(moves(&mut nav, &mut input, 8, PadButton::DPadDown), 1);
        assert_eq!(moves(&mut nav, &mut input, 6, PadButton::DPadDown), 2);
        input.pad_button_up(PadButton::DPadDown);
        assert_eq!(moves(&mut nav, &mut input, 20, PadButton::DPadDown), 0);

        input.set_pad_axis(PadAxis::LeftY, 1.0);
        assert_eq!(moves(&mut nav, &mut input, 3, PadButton::DPadUp), 1);
        input.set_pad_axis(PadAxis::LeftY, 0.0);
        assert_eq!(moves(&mut nav, &mut input, 3, PadButton::DPadUp), 0);
    }
}
