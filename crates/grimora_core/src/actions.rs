//! Named actions: games ask for `"jump"` and the player decides which keys,
//! mouse buttons or gamepad controls that is.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use winit::event::MouseButton;
use winit::keyboard::KeyCode;

/// The actions the engine's own controllers read. Games add theirs with [`ActionMap::add`].
pub const MOVE_FORWARD: &str = "move_forward";
pub const MOVE_BACK: &str = "move_back";
pub const MOVE_LEFT: &str = "move_left";
pub const MOVE_RIGHT: &str = "move_right";
pub const RUN: &str = "run";
pub const JUMP: &str = "jump";
pub const INTERACT: &str = "interact";
pub const ATTACK: &str = "attack";
pub const ROLL: &str = "roll";
pub const LOCK_ON: &str = "lock_on";
pub const GUARD: &str = "guard";
/// Escape and Start. Not meant to be rebound, so a player can always reach the menu.
pub const PAUSE: &str = "pause";

/// A gamepad button, named by its position (South is A on Xbox pads, X on PlayStation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PadButton {
    South,
    East,
    West,
    North,
    LeftBumper,
    RightBumper,
    LeftTrigger,
    RightTrigger,
    Select,
    Start,
    LeftStick,
    RightStick,
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
}

/// A gamepad stick axis, from -1 to 1. Up and right are positive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PadAxis {
    LeftX,
    LeftY,
    RightX,
    RightY,
}

/// One physical control an action can be bound to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Binding {
    Key(KeyCode),
    Mouse(MouseButton),
    Pad(PadButton),
    /// One direction of a stick: `Stick(LeftY, true)` is the left stick pushed up.
    Stick(PadAxis, bool),
}

/// Keys the engine or every game keeps for itself, which actions can't take.
const RESERVED: [KeyCode; 4] = [KeyCode::Escape, KeyCode::F3, KeyCode::F5, KeyCode::F12];

impl Binding {
    pub fn is_gamepad(self) -> bool {
        matches!(self, Self::Pad(_) | Self::Stick(..))
    }

    /// Can't be given to an action: Esc and Start open the menu, F3, F5 and F12 are debug keys.
    pub fn is_reserved(self) -> bool {
        match self {
            Self::Key(key) => RESERVED.contains(&key),
            Self::Pad(button) => button == PadButton::Start,
            _ => false,
        }
    }

    /// Short name to show the player, such as `W`, `Espaço` or `Clique`.
    pub fn label(self) -> String {
        match self {
            Self::Key(key) => key_label(key),
            Self::Mouse(MouseButton::Left) => "Clique".into(),
            Self::Mouse(MouseButton::Right) => "Clique direito".into(),
            Self::Mouse(MouseButton::Middle) => "Clique do meio".into(),
            Self::Mouse(MouseButton::Back) => "Mouse 4".into(),
            Self::Mouse(MouseButton::Forward) => "Mouse 5".into(),
            Self::Mouse(MouseButton::Other(n)) => format!("Mouse {n}"),
            Self::Pad(button) => pad_label(button).into(),
            Self::Stick(axis, positive) => {
                let side = if matches!(axis, PadAxis::LeftX | PadAxis::LeftY) { "L" } else { "R" };
                let way = match (axis, positive) {
                    (PadAxis::LeftX | PadAxis::RightX, true) => "direita",
                    (PadAxis::LeftX | PadAxis::RightX, false) => "esquerda",
                    (_, true) => "cima",
                    (_, false) => "baixo",
                };
                format!("{side} {way}")
            }
        }
    }
}

fn pad_label(button: PadButton) -> &'static str {
    match button {
        PadButton::South => "A",
        PadButton::East => "B",
        PadButton::West => "X",
        PadButton::North => "Y",
        PadButton::LeftBumper => "LB",
        PadButton::RightBumper => "RB",
        PadButton::LeftTrigger => "LT",
        PadButton::RightTrigger => "RT",
        PadButton::Select => "Select",
        PadButton::Start => "Start",
        PadButton::LeftStick => "L3",
        PadButton::RightStick => "R3",
        PadButton::DPadUp => "D-pad cima",
        PadButton::DPadDown => "D-pad baixo",
        PadButton::DPadLeft => "D-pad esquerda",
        PadButton::DPadRight => "D-pad direita",
    }
}

fn key_label(key: KeyCode) -> String {
    let name = match key {
        KeyCode::Space => "Espaço",
        KeyCode::ShiftLeft | KeyCode::ShiftRight => "Shift",
        KeyCode::ControlLeft | KeyCode::ControlRight => "Ctrl",
        KeyCode::AltLeft | KeyCode::AltRight => "Alt",
        KeyCode::ArrowUp => "Seta cima",
        KeyCode::ArrowDown => "Seta baixo",
        KeyCode::ArrowLeft => "Seta esquerda",
        KeyCode::ArrowRight => "Seta direita",
        KeyCode::CapsLock => "Caps Lock",
        _ => "",
    };
    if !name.is_empty() {
        return name.into();
    }
    // `KeyW` and `Digit1` read better as `W` and `1`.
    let debug = format!("{key:?}");
    debug.strip_prefix("Key").or_else(|| debug.strip_prefix("Digit")).unwrap_or(&debug).to_string()
}

/// Which controls trigger which action. The default holds the engine's actions,
/// on keyboard, mouse and gamepad.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ActionMap {
    actions: BTreeMap<String, Vec<Binding>>,
}

impl Default for ActionMap {
    fn default() -> Self {
        use Binding::{Key, Mouse, Pad, Stick};
        let mut map = Self { actions: BTreeMap::new() };
        map.add(MOVE_FORWARD, [Key(KeyCode::KeyW), Stick(PadAxis::LeftY, true)]);
        map.add(MOVE_BACK, [Key(KeyCode::KeyS), Stick(PadAxis::LeftY, false)]);
        map.add(MOVE_LEFT, [Key(KeyCode::KeyA), Stick(PadAxis::LeftX, false)]);
        map.add(MOVE_RIGHT, [Key(KeyCode::KeyD), Stick(PadAxis::LeftX, true)]);
        map.add(RUN, [Key(KeyCode::ShiftLeft), Key(KeyCode::ShiftRight), Pad(PadButton::LeftBumper)]);
        map.add(JUMP, [Key(KeyCode::Space), Pad(PadButton::South)]);
        map.add(INTERACT, [Key(KeyCode::KeyE), Pad(PadButton::West)]);
        map.add(ATTACK, [Mouse(MouseButton::Left), Key(KeyCode::KeyF), Pad(PadButton::RightTrigger)]);
        map.add(ROLL, [Key(KeyCode::ControlLeft), Pad(PadButton::East)]);
        map.add(LOCK_ON, [Key(KeyCode::KeyQ), Pad(PadButton::RightBumper)]);
        map.add(GUARD, [Mouse(MouseButton::Right), Pad(PadButton::LeftTrigger)]);
        map.add(PAUSE, [Key(KeyCode::Escape), Pad(PadButton::Start)]);
        map
    }
}

impl ActionMap {
    /// Adds an action with its default controls. One already there keeps its
    /// own, so games can call this after loading what the player chose.
    pub fn add(&mut self, action: &str, bindings: impl IntoIterator<Item = Binding>) {
        self.actions.entry(action.to_string()).or_insert_with(|| bindings.into_iter().collect());
    }

    /// Replaces the controls of an action.
    pub fn set(&mut self, action: &str, bindings: impl IntoIterator<Item = Binding>) {
        self.actions.insert(action.to_string(), bindings.into_iter().collect());
    }

    pub fn bindings(&self, action: &str) -> &[Binding] {
        self.actions.get(action).map_or(&[], Vec::as_slice)
    }

    /// What triggers `action`, for hints: `W`, or `Clique / F`. With `gamepad`,
    /// the gamepad's controls instead of the keyboard's and mouse's.
    pub fn label(&self, action: &str, gamepad: bool) -> String {
        let mut labels: Vec<String> = Vec::new();
        for binding in self.bindings(action).iter().filter(|binding| binding.is_gamepad() == gamepad) {
            let label = binding.label();
            if !labels.contains(&label) {
                labels.push(label);
            }
        }
        labels.join(" / ")
    }

    /// Gives `binding` to `action`, in place of what it had on the same kind of
    /// device. An action that was using it gets `action`'s old control, so
    /// nothing ends up with no way to trigger it. Returns that other action,
    /// or `None` if there was none. Reserved keys change nothing.
    pub fn rebind(&mut self, action: &str, binding: Binding) -> Option<String> {
        if binding.is_reserved() {
            return None;
        }
        let gamepad = binding.is_gamepad();
        let current = self.actions.get_mut(action)?;
        let old: Vec<Binding> = current.iter().copied().filter(|b| b.is_gamepad() == gamepad).collect();
        current.retain(|b| b.is_gamepad() != gamepad);
        current.insert(0, binding);

        let other = self
            .actions
            .iter()
            .find(|(name, bindings)| name.as_str() != action && bindings.contains(&binding))
            .map(|(name, _)| name.clone())?;
        let bindings = self.actions.get_mut(&other)?;
        let at = bindings.iter().position(|b| *b == binding)?;
        match old.into_iter().find(|b| *b != binding) {
            Some(swapped) => bindings[at] = swapped,
            None => {
                bindings.remove(at);
            }
        }
        Some(other)
    }

    /// `self` as saved by an older version: actions added since then get their defaults.
    pub fn with_defaults_from(mut self, defaults: &ActionMap) -> Self {
        for (action, bindings) in &defaults.actions {
            self.actions.entry(action.clone()).or_insert_with(|| bindings.clone());
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Binding::{Key, Pad};

    #[test]
    fn rebinding_swaps_with_the_action_using_the_key() {
        let mut map = ActionMap::default();
        assert_eq!(map.rebind(JUMP, Key(KeyCode::KeyE)).as_deref(), Some(INTERACT));
        assert_eq!(map.label(JUMP, false), "E");
        assert_eq!(map.label(INTERACT, false), "Espaço");
        // Gamepad controls stay when the keyboard's change.
        assert_eq!(map.label(JUMP, true), "A");
        assert_eq!(map.rebind(JUMP, Key(KeyCode::KeyG)), None);
        assert_eq!(map.label(JUMP, false), "G");
    }

    #[test]
    fn rebinding_replaces_the_whole_device() {
        let mut map = ActionMap::default();
        assert_eq!(map.label(ATTACK, false), "Clique / F");
        map.rebind(ATTACK, Key(KeyCode::KeyG));
        assert_eq!(map.label(ATTACK, false), "G");
        map.rebind(ATTACK, Pad(PadButton::North));
        assert_eq!(map.label(ATTACK, true), "Y");
        assert_eq!(map.label(ATTACK, false), "G");
    }

    #[test]
    fn rebinding_to_its_own_key_changes_nothing_else() {
        let mut map = ActionMap::default();
        let run_keys = map.bindings(RUN).to_vec();
        map.rebind(JUMP, Key(KeyCode::Space));
        assert_eq!(map.label(JUMP, false), "Espaço");
        assert_eq!(map.bindings(RUN), run_keys);
    }

    #[test]
    fn reserved_keys_are_refused() {
        let mut map = ActionMap::default();
        let before = map.clone();
        map.rebind(JUMP, Key(KeyCode::Escape));
        map.rebind(JUMP, Key(KeyCode::F12));
        assert_eq!(map, before);
    }

    #[test]
    fn saved_maps_gain_new_actions_and_keep_choices() {
        let mut defaults = ActionMap::default();
        defaults.add("voar", [Key(KeyCode::Tab)]);
        let mut saved = ActionMap { actions: BTreeMap::new() };
        saved.set(JUMP, [Key(KeyCode::KeyJ)]);
        let merged = saved.with_defaults_from(&defaults);
        assert_eq!(merged.label(JUMP, false), "J");
        assert_eq!(merged.label("voar", false), "Tab");
        assert_eq!(merged.label(RUN, false), "Shift");
    }

    #[test]
    fn bindings_survive_ron() {
        let map = ActionMap::default();
        let text = ron::to_string(&map).unwrap();
        assert_eq!(ron::from_str::<ActionMap>(&text).unwrap(), map);
    }
}
