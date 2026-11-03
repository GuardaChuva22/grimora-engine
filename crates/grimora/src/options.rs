//! What the player can change: sound, video and controls, kept in a small file
//! next to the saved games.

use grimora_audio::Category;
use grimora_core::{ActionMap, Input};
use serde::{Deserialize, Serialize};

use crate::{Context, SaveStore};

const OPTIONS_SLOT: &str = "options";
/// The sizes a menu can offer for [`Options::ui_scale`].
pub const UI_SCALES: [f32; 3] = [1.0, 1.25, 1.5];
/// Window sizes a menu can offer for [`Options::resolution`].
pub const RESOLUTIONS: [(u32, u32); 4] = [(1280, 720), (1600, 900), (1920, 1080), (2560, 1440)];

/// How much the shadows cost.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShadowQuality {
    /// Maps of 1024 pixels and at most two cascades.
    Low,
    /// Maps of 2048 pixels.
    #[default]
    Medium,
    /// Maps of 4096 pixels.
    High,
}

impl ShadowQuality {
    pub const ALL: [Self; 3] = [Self::Low, Self::Medium, Self::High];

    /// `(shadow map size, most cascades)`.
    pub fn settings(self) -> (u32, u32) {
        match self {
            Self::Low => (1024, 2),
            Self::Medium => (2048, 4),
            Self::High => (4096, 4),
        }
    }
}

/// The player's choices. Fields missing from the file take their defaults, so
/// new options don't invalidate old files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub master_volume: f32,
    pub ambient_volume: f32,
    pub effects_volume: f32,
    pub ui_scale: f32,
    pub fullscreen: bool,
    /// Window size in physical pixels. `None` leaves it as the game started.
    pub resolution: Option<(u32, u32)>,
    pub vsync: bool,
    pub shadows: ShadowQuality,
    /// MSAA. Takes effect the next time the game starts (see [`Options::apply_at_startup`]).
    pub antialiasing: bool,
    /// Multiplies mouse and stick look: 1 is the engine's default.
    pub look_sensitivity: f32,
    pub invert_look_y: bool,
    /// The controls the player chose. `None` keeps the game's defaults.
    pub bindings: Option<ActionMap>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            master_volume: 1.0,
            ambient_volume: 1.0,
            effects_volume: 1.0,
            ui_scale: 1.0,
            fullscreen: false,
            resolution: None,
            vsync: true,
            shadows: ShadowQuality::Medium,
            antialiasing: true,
            look_sensitivity: 1.0,
            invert_look_y: false,
            bindings: None,
        }
    }
}

impl Options {
    /// Where options persist: `config/` inside the folder of saved games. Being
    /// apart from them, they don't show up as a slot.
    pub fn store(saves: &SaveStore) -> SaveStore {
        SaveStore::new(saves.dir().join("config"))
    }

    /// The saved options. A missing file gives the defaults; so does a broken
    /// one, with a warning: a bad options file must never stop the game.
    pub fn load(store: &SaveStore) -> Self {
        if !store.exists(OPTIONS_SLOT) {
            return Self::default();
        }
        let options: Self = store.read(OPTIONS_SLOT).unwrap_or_else(|err| {
            log::warn!("ignoring the saved options: {err:#}");
            Self::default()
        });
        options.sanitized()
    }

    /// Writes them. Failing is logged and not fatal, for the same reason.
    pub fn save(&self, store: &SaveStore) -> bool {
        match store.write(OPTIONS_SLOT, self) {
            Ok(_) => true,
            Err(err) => {
                log::warn!("can't save the options: {err:#}");
                false
            }
        }
    }

    /// Hand-edited files can hold anything the format allows.
    fn sanitized(self) -> Self {
        let volume = |v: f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { 1.0 };
        let sensitivity = if self.look_sensitivity.is_finite() { self.look_sensitivity.clamp(0.1, 5.0) } else { 1.0 };
        Self {
            master_volume: volume(self.master_volume),
            ambient_volume: volume(self.ambient_volume),
            effects_volume: volume(self.effects_volume),
            ui_scale: UI_SCALES[self.ui_scale_index()],
            look_sensitivity: sensitivity,
            resolution: self.resolution.filter(|&(w, h)| (320..=16384).contains(&w) && (200..=16384).contains(&h)),
            ..self
        }
    }

    /// Position of `ui_scale` in [`UI_SCALES`], the first if it isn't there.
    pub fn ui_scale_index(&self) -> usize {
        UI_SCALES.iter().position(|scale| (scale - self.ui_scale).abs() < 0.01).unwrap_or(0)
    }

    /// Puts them into effect. Controls replace `ctx.input.actions`, with any
    /// action the player's file doesn't know keeping its default: add the
    /// game's own actions before this.
    pub fn apply(&self, ctx: &mut Context) {
        ctx.audio.set_master_volume(self.master_volume);
        ctx.audio.set_volume(Category::Ambient, self.ambient_volume);
        ctx.audio.set_volume(Category::Effects, self.effects_volume);
        ctx.set_ui_scale(self.ui_scale);
        ctx.set_fullscreen(self.fullscreen);
        ctx.set_vsync(self.vsync);
        ctx.set_shadow_quality(self.shadows);
        if let Some((width, height)) = self.resolution {
            ctx.set_window_size(width, height);
        }
        self.apply_controls(&mut ctx.input);
    }

    fn apply_controls(&self, input: &mut Input) {
        input.look_sensitivity = self.look_sensitivity;
        input.invert_look_y = self.invert_look_y;
        if let Some(bindings) = &self.bindings {
            input.actions = bindings.clone().with_defaults_from(&input.actions);
        }
    }

    /// What can only change before the window opens. Call it on the config
    /// given to [`crate::run`], unless the run is automated.
    pub fn apply_at_startup(&self, config: &mut crate::EngineConfig) {
        if !self.antialiasing {
            config.msaa = 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grimora_core::{Binding, KeyCode, actions};

    #[test]
    fn options_with_controls_survive_the_file() {
        let dir = std::env::temp_dir().join(format!("grimora_options_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = SaveStore::new(&dir);
        let mut bindings = ActionMap::default();
        bindings.rebind(actions::JUMP, Binding::Key(KeyCode::KeyJ));
        let options = Options {
            shadows: ShadowQuality::High,
            vsync: false,
            resolution: Some((1920, 1080)),
            look_sensitivity: 1.5,
            invert_look_y: true,
            bindings: Some(bindings),
            ..Default::default()
        };
        assert!(options.save(&store));
        assert_eq!(Options::load(&store), options);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_hand_edited_file_is_tamed() {
        let options = Options {
            master_volume: 7.0,
            ui_scale: 3.0,
            look_sensitivity: f32::NAN,
            resolution: Some((1, 1)),
            ..Default::default()
        }
        .sanitized();
        assert_eq!((options.master_volume, options.ui_scale, options.look_sensitivity), (1.0, 1.0, 1.0));
        assert_eq!(options.resolution, None);
    }

    #[test]
    fn controls_keep_new_actions_and_choices() {
        let mut input = Input::default();
        input.actions.add("voar", [Binding::Key(KeyCode::Tab)]);
        let mut chosen = ActionMap::default();
        chosen.rebind(actions::JUMP, Binding::Key(KeyCode::KeyJ));
        let options = Options { bindings: Some(chosen), invert_look_y: true, ..Default::default() };
        options.apply_controls(&mut input);
        assert_eq!(input.actions.label(actions::JUMP, false), "J");
        assert_eq!(input.actions.label("voar", false), "Tab");
        assert!(input.invert_look_y);
    }
}
