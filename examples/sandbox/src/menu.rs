//! The sandbox's menus: the title screen over the live scene, the pause menu on
//! Esc and the options, which persist in a small file next to the saves.
//!
//! Runs that automated tools make go straight into play: `--screenshot`,
//! `--frames`, `--load`, `--camera`, `--look-at` and `--no-ui` skip the title
//! screen, and `--screenshot` and `--frames` also ignore the saved options and
//! write none. `--menu title|pause|options` opens a menu anyway, to capture it.

use grimora::prelude::*;

use crate::{Mode, QUICK_LOAD, QUICK_SAVE, QUICK_SLOT, Sandbox, TOGGLE_FLY, animate};

const TITLE: &str = "GRIMORA";
const TITLE_SIZE: f32 = 64.0;
const TITLE_GAP: f32 = 28.0;
const HEADING_SIZE: f32 = 32.0;
const HEADING_GAP: f32 = 18.0;
/// The big lists, in the title and the pause.
const BUTTON: Vec2 = Vec2::new(320.0, 38.0);
const BUTTON_TEXT: f32 = 24.0;
const OPTION_ROW: Vec2 = Vec2::new(600.0, 32.0);
const OPTION_TEXT: f32 = 16.0;
const GAP: f32 = 4.0;
/// How far the dark behind a list goes on after it, fading.
const SHADE_FADE: f32 = 220.0;
const SHADE: Vec4 = Vec4::new(0.015, 0.018, 0.05, 0.86);
/// The menus' look: lines of text, no boxes.
const MENU_THEME: Theme = Theme {
    text_size: BUTTON_TEXT,
    text: Vec4::new(0.9, 0.87, 0.76, 1.0),
    accent: Vec4::new(0.98, 0.95, 0.84, 1.0),
    disabled: Vec4::new(0.6, 0.58, 0.62, 0.55),
    fill: Vec4::new(0.015, 0.018, 0.05, 0.86),
    hover: Vec4::new(0.36, 0.27, 0.46, 0.85),
    pressed: Vec4::new(0.5, 0.38, 0.6, 0.9),
    border: Vec4::new(0.62, 0.46, 0.58, 1.0),
    track: Vec4::new(0.0, 0.0, 0.03, 0.75),
    frames: false,
    upper: true,
};
const PALE: Vec4 = Vec4::new(0.98, 0.95, 0.84, 1.0);
const DIM: Vec4 = Vec4::new(0.7, 0.68, 0.6, 0.95);
const LINE: Vec4 = Vec4::new(0.62, 0.46, 0.58, 1.0);

const VOLUME_STEP: f32 = 0.05;
const SENSITIVITY_RANGE: std::ops::RangeInclusive<f32> = 0.25..=3.0;
const SENSITIVITY_STEP: f32 = 0.05;
const UI_SCALE_NAMES: [&str; 3] = ["100%", "125%", "150%"];
const RESOLUTION_NAMES: [&str; 4] = ["1280 x 720", "1600 x 900", "1920 x 1080", "2560 x 1440"];
const SHADOW_NAMES: [&str; 3] = ["Baixa", "Média", "Alta"];
/// The controls screen is a grid of smaller rows: there is a lot to fit.
const CONTROL_ROW: Vec2 = Vec2::new(390.0, 28.0);
const CONTROL_TEXT: f32 = 16.0;
const CONTROL_GAP: f32 = 12.0;
/// What the player can rebind, with the names shown.
const CONTROLS: [(&str, &str); 21] = [
    (actions::MOVE_FORWARD, "Andar para frente"),
    (actions::MOVE_BACK, "Andar para trás"),
    (actions::MOVE_LEFT, "Andar à esquerda"),
    (actions::MOVE_RIGHT, "Andar à direita"),
    (actions::RUN, "Correr"),
    (actions::SLOW_WALK, "Andar devagar"),
    (actions::JUMP, "Pular"),
    (actions::INTERACT, "Usar"),
    (actions::ATTACK, "Golpear"),
    (actions::ROLL, "Rolar"),
    (actions::LOCK_ON, "Travar alvo"),
    (actions::GUARD, "Defender"),
    crate::leap::CONTROL_NAME,
    crate::spells::CONTROL_NAMES[0],
    crate::spells::CONTROL_NAMES[1],
    crate::guns::CONTROL_NAMES[0],
    (crate::bag::OPEN_BAG, "Mochila"),
    (TOGGLE_FLY, "Câmera livre"),
    (QUICK_SAVE, "Salvar"),
    (QUICK_LOAD, "Carregar"),
    crate::tale::CONTROL_NAME,
];
/// A `--menu pause` or `--menu options` run plays this many frames first, so
/// the capture shows a game in progress.
const CAPTURE_WARMUP_FRAMES: u64 = 30;

/// The keys of a screen, on a keyboard and on a gamepad.
type Keys = (&'static str, &'static str);
const TITLE_HINT: Keys = ("ENTER  ESCOLHER", "A  ESCOLHER");
const PAUSE_HINT: Keys = ("ENTER  ESCOLHER    ESC  VOLTAR AO JOGO    F3  MÉTRICAS", "A  ESCOLHER    B  VOLTAR AO JOGO");
const OPTIONS_HINT: Keys = ("A/D  AJUSTAR    ESC  VOLTAR", "ESQUERDA/DIREITA  AJUSTAR    B  VOLTAR");
const CONTROLS_HINT: Keys = ("ENTER  TROCAR    ESC  VOLTAR", "A  TROCAR    B  VOLTAR");
const CAPTURE_HINT: Keys = ("PRESSIONE A NOVA TECLA OU BOTÃO    ESC  CANCELAR", "PRESSIONE O NOVO BOTÃO OU TECLA");
const SAVE_HINT: Keys = ("ENTER  SALVAR    DEL  APAGAR    ESC  VOLTAR", "A  SALVAR    X  APAGAR    B  VOLTAR");
const LOAD_HINT: Keys = ("ENTER  CARREGAR    DEL  APAGAR    ESC  VOLTAR", "A  CARREGAR    X  APAGAR    B  VOLTAR");
const SURE_HINT: Keys = ("ENTER  CONFIRMAR    ESC  CANCELAR", "A  CONFIRMAR    B  CANCELAR");
/// What's shown in the pause, on the right. The way to walk is its own row, first.
const PLAY_KEYS: [(&str, &str); 14] = [
    (actions::RUN, "Correr"),
    (actions::SLOW_WALK, "Andar devagar"),
    (actions::JUMP, "Pular"),
    (actions::ROLL, "Rolar"),
    (actions::INTERACT, "Usar"),
    (actions::ATTACK, "Golpear"),
    (actions::GUARD, "Defender"),
    (actions::LOCK_ON, "Travar alvo"),
    (crate::bag::OPEN_BAG, "Mochila"),
    crate::spells::CONTROL_NAMES[0],
    crate::spells::CONTROL_NAMES[1],
    crate::guns::CONTROL_NAMES[0],
    crate::tale::CONTROL_NAME,
    (QUICK_SAVE, "Salvar"),
];
const PLAY_KEY_ROW: f32 = 24.0;
/// Under this width there's no room for them next to the list.
const PLAY_KEYS_FROM: f32 = 1000.0;
/// Where a game can be saved.
const SLOTS: [(&str, &str); 5] = [
    (QUICK_SLOT, "Salvamento rápido"),
    ("slot_1", "Jogo 1"),
    ("slot_2", "Jogo 2"),
    ("slot_3", "Jogo 3"),
    ("slot_4", "Jogo 4"),
];
const MENU_SAVE: &str = "Salvar";
const MENU_LOAD: &str = "Carregar";
const MENU_VIDEO: &str = "Vídeo";
const MENU_CONTROLS: &str = "Controles";
const RESTORE: &str = "Restaurar padrão";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Title,
    Pause,
    Options,
    Video,
    Controls,
    /// The slots, to save in one or else to load from one.
    Saves {
        saving: bool,
    },
}

/// A second press is needed to do this to a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sure {
    Overwrite(usize),
    Delete(usize),
}

impl Sure {
    fn slot(self) -> usize {
        match self {
            Self::Overwrite(slot) | Self::Delete(slot) => slot,
        }
    }
}

/// What the player asked for this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    NewGame,
    Load,
    LoadSlot(usize),
    Resume,
    OpenSaves(bool),
    OpenOptions,
    OpenVideo,
    OpenControls,
    Back,
    MainMenu,
    Quit,
}

#[derive(Default)]
pub struct Menu {
    /// Open screens, the top one showing. Empty while playing.
    stack: Vec<Screen>,
    /// A `--menu` run's screens and the frame to open them on.
    scheduled: Option<(Vec<Screen>, u64)>,
    options: Options,
    /// Where options persist; `None` in automated runs.
    store: Option<SaveStore>,
    /// The options as last written, to skip writing when nothing changed.
    stored: Options,
    /// The newest saved game, read when a menu opens and after saving.
    latest_save: Option<String>,
    /// How long ago each of [`SLOTS`] was saved, in seconds, if it was.
    slot_ages: [Option<u64>; SLOTS.len()],
    /// The slot waiting for its second press.
    sure: Option<Sure>,
    /// Where the title screen looks from: the scene's own camera.
    title_camera: Camera,
    /// The controls as the game defines them, for "Restaurar padrão".
    default_controls: ActionMap,
    /// The action waiting for the player to press its new control.
    capturing: Option<&'static str>,
}

impl Menu {
    fn refresh_saves(&mut self, saves: &SaveStore) {
        let slots = saves.slots().unwrap_or_else(|err| {
            eprintln!("[sandbox] warning: can't list the saved games: {err:#}");
            Vec::new()
        });
        self.latest_save = slots.first().map(|slot| slot.name.clone());
        let now = std::time::SystemTime::now();
        for (age, (name, _)) in self.slot_ages.iter_mut().zip(SLOTS) {
            let slot = slots.iter().find(|slot| slot.name == name);
            *age = slot.map(|slot| now.duration_since(slot.modified).map_or(0, |since| since.as_secs()));
        }
    }

    fn store_options(&mut self) {
        let Some(store) = &self.store else { return };
        if self.options == self.stored {
            return;
        }
        if self.options.save(store) {
            self.stored = self.options.clone();
        }
    }
}

/// Closing the window from the options screen doesn't lose them.
impl Drop for Menu {
    fn drop(&mut self) {
        self.store_options();
    }
}

impl Sandbox {
    /// Reads the options and picks the first screen. Called once from `init`.
    pub(crate) fn start_menu(&mut self, ctx: &mut Context) {
        self.menu.title_camera = ctx.camera;
        ctx.ui.theme = MENU_THEME;
        let controls = &mut ctx.input.actions;
        controls.add(TOGGLE_FLY, [Binding::Key(KeyCode::Tab)]);
        controls.add(QUICK_SAVE, [Binding::Key(KeyCode::F6)]);
        controls.add(QUICK_LOAD, [Binding::Key(KeyCode::F9)]);
        controls.add(crate::tale::OPEN_JOURNAL, [Binding::Key(KeyCode::KeyJ), Binding::Pad(PadButton::RightStick)]);
        crate::bag::add_controls(controls);
        crate::spells::add_controls(controls);
        crate::guns::add_controls(controls);
        crate::leap::add_controls(controls);
        self.menu.default_controls = controls.clone();
        let config = ctx.config();
        let automated = config.screenshot.is_some() || config.frames.is_some();
        let straight_to_play =
            automated || config.load.is_some() || config.camera_position.is_some() || config.look_at.is_some();
        let straight_to_play = straight_to_play || !config.ui;
        let warmup = config.frames.map_or(CAPTURE_WARMUP_FRAMES, |frames| CAPTURE_WARMUP_FRAMES.min(frames));
        let requested = config.menu.clone();

        if !automated {
            let store = Options::store(&ctx.saves);
            self.menu.options = Options::load(&store);
            self.menu.stored = self.menu.options.clone();
            self.menu.store = Some(store);
            self.menu.options.apply(ctx);
        }
        match requested.as_deref() {
            None if straight_to_play => {}
            None | Some("title") => self.menu.stack = vec![Screen::Title],
            Some("pause") => self.menu.scheduled = Some((vec![Screen::Pause], warmup)),
            Some("options") => self.menu.scheduled = Some((vec![Screen::Pause, Screen::Options], warmup)),
            Some("video") => self.menu.scheduled = Some((vec![Screen::Pause, Screen::Options, Screen::Video], warmup)),
            Some("save") => {
                self.menu.scheduled = Some((vec![Screen::Pause, Screen::Saves { saving: true }], warmup));
            }
            Some("load") => {
                self.menu.scheduled = Some((vec![Screen::Pause, Screen::Saves { saving: false }], warmup));
            }
            Some("inventory") => self.bag.scheduled = Some((false, warmup)),
            Some(name @ ("dialogue" | "shop" | "rest" | "death" | "area")) => {
                self.tale.scheduled = Some((name.to_owned(), warmup));
            }
            Some(journal) if journal.starts_with("journal") => {
                self.tale.scheduled = Some((journal.to_owned(), warmup));
            }
            Some("hud") => self.hud.demo = true,
            Some("chest") => self.bag.scheduled = Some((true, warmup)),
            Some(gear) if gear.starts_with("gear:") => {
                self.bag.gear.scheduled = Some(gear["gear:".len()..].to_owned());
            }
            Some(gun) if gun.starts_with("gun:") => {
                self.guns.scheduled = Some((gun["gun:".len()..].to_owned(), warmup));
            }
            Some(aura) if aura.starts_with("aura:") => {
                self.debug.scheduled_aura = Some((aura["aura:".len()..].to_owned(), 30));
            }
            Some(spell) if spell.starts_with("spell:") => {
                self.spells.scheduled = Some((spell["spell:".len()..].to_owned(), warmup));
            }
            Some("controls") => {
                self.menu.scheduled = Some((vec![Screen::Pause, Screen::Options, Screen::Controls], warmup));
            }
            Some(other) => {
                eprintln!(
                    "[sandbox] unknown menu {other:?}: use title, pause, options, video, controls, save, load, inventory or chest"
                );
                ctx.exit();
            }
        }
        self.menu.refresh_saves(&ctx.saves);
    }

    /// Runs whatever menu is open, or opens the pause menu on Esc. Returns true
    /// if gameplay should sit out this frame. That includes the frame a menu
    /// closes, so the key that closed it can't also act in the game.
    pub(crate) fn run_menu(&mut self, ctx: &mut Context) -> bool {
        // A new game whose scene file turned out to be broken keeps the old world.
        if self.player.is_none() {
            self.spawn_player(ctx);
        }
        if let Some((_, frame)) = &self.menu.scheduled
            && ctx.time.frame() >= *frame
            && let Some((stack, _)) = self.menu.scheduled.take()
        {
            self.menu.stack = stack;
            ctx.set_paused(true);
        }
        // B goes back in menus; Start only opens and closes the pause menu.
        let mut escape = ctx.input.action_just_pressed(actions::PAUSE);
        if !self.menu.stack.is_empty() && ctx.input.pad_just_pressed(PadButton::East) {
            escape = true;
        }
        if self.menu.stack.is_empty() {
            if !escape {
                return false;
            }
            // The screen of things isn't a menu: Esc just closes it.
            if self.bag.open {
                self.bag.close();
                return false;
            }
            if self.tale.panel.take().is_some() {
                return false;
            }
            escape = false;
            self.menu.stack.push(Screen::Pause);
            self.menu.refresh_saves(&ctx.saves);
            ctx.set_paused(true);
            ctx.ui.reset_focus();
        }
        ctx.set_cursor_grab(false);
        if self.menu.capturing.is_some() {
            self.capture_control(ctx);
            escape = false;
        }
        // Esc takes back the question first.
        if escape && self.menu.sure.take().is_some() {
            escape = false;
        }

        let screen = self.menu.stack[self.menu.stack.len() - 1];
        let can_load = self.menu.latest_save.is_some();
        let (mut action, hint) = match screen {
            Screen::Title => {
                self.live_scene(ctx);
                (draw_title(&mut ctx.ui, can_load), TITLE_HINT)
            }
            Screen::Pause => {
                let keys = play_keys(&ctx.input);
                (draw_pause(&mut ctx.ui, can_load, &keys), PAUSE_HINT)
            }
            Screen::Options => {
                let (changed, action) = draw_options(&mut ctx.ui, &mut self.menu.options);
                if changed {
                    self.menu.options.apply(ctx);
                }
                (action, OPTIONS_HINT)
            }
            Screen::Video => {
                let size = ctx.window_size();
                let (changed, action) = draw_video(&mut ctx.ui, &mut self.menu.options, size);
                if changed {
                    self.menu.options.apply(ctx);
                }
                (action, OPTIONS_HINT)
            }
            Screen::Controls => {
                let action = self.draw_controls(ctx);
                (action, if self.menu.capturing.is_some() { CAPTURE_HINT } else { CONTROLS_HINT })
            }
            Screen::Saves { saving } => {
                let action = self.draw_saves(ctx, saving);
                let keys = match (self.menu.sure, saving) {
                    (Some(_), _) => SURE_HINT,
                    (None, true) => SAVE_HINT,
                    (None, false) => LOAD_HINT,
                };
                (action, keys)
            }
        };
        let hint = if ctx.input.gamepad_last() { hint.1 } else { hint.0 };
        self.hud.draw_with_hint(ctx, hint);
        if escape && screen != Screen::Title {
            action = Some(if screen == Screen::Pause { Action::Resume } else { Action::Back });
        }
        if let Some(action) = action {
            self.apply_menu_action(ctx, action);
        }
        true
    }

    fn apply_menu_action(&mut self, ctx: &mut Context, action: Action) {
        match action {
            Action::NewGame => {
                self.restart_world(ctx);
                self.close_menu(ctx);
            }
            Action::Load => {
                let Some(slot) = self.menu.latest_save.clone() else { return };
                // A failed load leaves a notice on screen and the menu open.
                if self.load(ctx, &slot) {
                    self.close_menu(ctx);
                }
            }
            Action::Resume => self.close_menu(ctx),
            Action::OpenSaves(saving) => {
                self.menu.stack.push(Screen::Saves { saving });
                self.menu.sure = None;
                self.menu.refresh_saves(&ctx.saves);
                ctx.ui.reset_focus();
            }
            Action::LoadSlot(slot) => {
                if self.load(ctx, SLOTS[slot].0) {
                    self.close_menu(ctx);
                }
            }
            Action::OpenOptions => {
                self.menu.stack.push(Screen::Options);
                ctx.ui.reset_focus();
            }
            Action::OpenVideo => {
                self.menu.stack.push(Screen::Video);
                ctx.ui.reset_focus();
            }
            Action::OpenControls => {
                self.menu.stack.push(Screen::Controls);
                ctx.ui.reset_focus();
            }
            Action::Back => {
                self.menu.store_options();
                let left = self.menu.stack.pop();
                self.menu.capturing = None;
                self.menu.sure = None;
                // Back where the player came from.
                ctx.ui.set_focus(match left {
                    Some(Screen::Video) => MENU_VIDEO,
                    Some(Screen::Controls) => MENU_CONTROLS,
                    Some(Screen::Saves { saving: true }) => MENU_SAVE,
                    Some(Screen::Saves { saving: false }) => MENU_LOAD,
                    _ => MENU_OPTIONS,
                });
            }
            Action::MainMenu => {
                self.restart_world(ctx);
                ctx.camera = self.menu.title_camera;
                ctx.set_paused(false);
                self.menu.stack = vec![Screen::Title];
                self.menu.refresh_saves(&ctx.saves);
                ctx.ui.reset_focus();
            }
            Action::Quit => ctx.exit(),
        }
    }

    /// Waits for the new control of the action being rebound; Esc cancels. The
    /// widgets see nothing meanwhile, so the key can't also press what has the focus.
    fn capture_control(&mut self, ctx: &mut Context) {
        let Some(action) = self.menu.capturing else { return };
        ctx.ui.set_input(UiInput::default());
        let label = CONTROLS.iter().find(|(name, _)| *name == action).map_or("", |(_, label)| *label);
        // Back on its row, wherever the key would have moved the focus.
        ctx.ui.set_focus(label);
        if ctx.input.just_pressed(KeyCode::Escape) {
            self.menu.capturing = None;
            return;
        }
        let Some(binding) = ctx.input.first_pressed() else { return };
        if binding.is_reserved() {
            self.hud.notify(ctx, format!("{} é reservada", binding.label()), false);
            return;
        }
        self.menu.capturing = None;
        if let Some(other) = ctx.input.actions.rebind(action, binding) {
            let other = CONTROLS.iter().find(|(name, _)| *name == other).map_or("", |(_, label)| *label);
            self.hud.notify(ctx, format!("Trocada com {other}"), true);
        }
        self.menu.options.bindings = Some(ctx.input.actions.clone());
    }

    /// The camera options and a row per action, two to a line.
    fn draw_controls(&mut self, ctx: &mut Context) -> Option<Action> {
        let gamepad = ctx.input.gamepad_last();
        let capturing = self.menu.capturing;
        let rows = 2 + CONTROLS.len().div_ceil(2) as u32;
        let ui = &mut ctx.ui;
        let wide = Vec2::new(CONTROL_ROW.x * 2.0 + CONTROL_GAP, CONTROL_ROW.y);
        ui.theme.text_size = CONTROL_TEXT;
        let mut column = menu_list(ui, MENU_CONTROLS, rows, wide);
        let (mut left, mut right) = {
            let row = column.row();
            (Rect::new(row.min, CONTROL_ROW), Rect::new(row.min + Vec2::X * (row.size.x - CONTROL_ROW.x), CONTROL_ROW))
        };
        let options = &mut self.menu.options;
        let mut changed = ui.number_slider(
            left,
            "Sensibilidade",
            &mut options.look_sensitivity,
            SENSITIVITY_RANGE,
            SENSITIVITY_STEP,
            2,
        );
        changed |= ui.toggle(right, "Inverter eixo Y", &mut options.invert_look_y);
        let mut wanted = None;
        for pair in CONTROLS.chunks(2) {
            let row = column.row();
            (left, right) = (
                Rect::new(row.min, CONTROL_ROW),
                Rect::new(row.min + Vec2::X * (row.size.x - CONTROL_ROW.x), CONTROL_ROW),
            );
            for (&(action, label), rect) in pair.iter().zip([left, right]) {
                let value = if capturing == Some(action) {
                    "Pressione...".to_string()
                } else {
                    ctx.input.actions.label(action, gamepad)
                };
                if ui.button_row(rect, label, &value) {
                    wanted = Some(action);
                }
            }
        }
        let row = column.row();
        (left, right) =
            (Rect::new(row.min, CONTROL_ROW), Rect::new(row.min + Vec2::X * (row.size.x - CONTROL_ROW.x), CONTROL_ROW));
        let restore = ui.button(left, RESTORE);
        let back = ui.button(right, "Voltar");

        if restore {
            ctx.input.actions = self.menu.default_controls.clone();
            self.menu.options.bindings = None;
            self.menu.capturing = None;
            changed = true;
        } else if wanted.is_some() {
            self.menu.capturing = wanted;
        }
        if changed {
            ctx.input.look_sensitivity = self.menu.options.look_sensitivity;
            ctx.input.invert_look_y = self.menu.options.invert_look_y;
        }
        back.then_some(Action::Back)
    }

    /// The slots. Writing over a game or deleting one takes a second press, and moving away
    /// takes the question back.
    fn draw_saves(&mut self, ctx: &mut Context, saving: bool) -> Option<Action> {
        let deleting = ctx.input.just_pressed(KeyCode::Delete) || ctx.input.pad_just_pressed(PadButton::West);
        let ages = self.menu.slot_ages;
        let sure = self.menu.sure;
        let ui = &mut ctx.ui;
        ui.theme.text_size = OPTION_TEXT;
        let heading = if saving { MENU_SAVE } else { MENU_LOAD };
        let mut column = menu_list(ui, heading, SLOTS.len() as u32 + 1, OPTION_ROW);
        let mut used = None;
        let mut focused = None;
        for (slot, (_, label)) in SLOTS.iter().enumerate() {
            let value = match sure.filter(|sure| sure.slot() == slot) {
                Some(Sure::Overwrite(_)) => "Escrever por cima?".to_owned(),
                Some(Sure::Delete(_)) => "Apagar?".to_owned(),
                None => ages[slot].map_or_else(|| "Vazio".to_owned(), ago),
            };
            if ui.button_row_if(saving || ages[slot].is_some(), column.row(), label, &value) {
                used = Some(slot);
            }
            if ui.is_focused(label) {
                focused = Some(slot);
            }
        }
        let back = ui.button(column.row(), "Voltar");

        if sure.is_some_and(|sure| Some(sure.slot()) != focused) {
            self.menu.sure = None;
        }
        if deleting && let Some(slot) = focused.filter(|slot| ages[*slot].is_some()) {
            self.menu.sure = Some(Sure::Delete(slot));
        }
        let Some(slot) = used else { return back.then_some(Action::Back) };
        let name = SLOTS[slot].0;
        match self.menu.sure.take() {
            Some(Sure::Delete(_)) => {
                if let Err(err) = ctx.saves.delete(name) {
                    eprintln!("[sandbox] can't delete a saved game: {err:#}");
                }
                self.hud.notify(ctx, "Jogo apagado", true);
                self.menu.refresh_saves(&ctx.saves);
            }
            None if !saving => return Some(Action::LoadSlot(slot)),
            None if ages[slot].is_some() => self.menu.sure = Some(Sure::Overwrite(slot)),
            _ => {
                self.save(ctx, name);
                self.menu.refresh_saves(&ctx.saves);
            }
        }
        None
    }

    fn close_menu(&mut self, ctx: &mut Context) {
        self.menu.stack.clear();
        ctx.set_paused(false);
    }

    /// Back to the scene as the file has it; `scene_loaded` spawns the player.
    fn restart_world(&mut self, ctx: &mut Context) {
        ctx.reload_scene();
        self.player = None;
        self.mode = Mode::Play;
        self.heart_asleep = false;
        self.heart_glow = 1.0;
        self.steps = Default::default();
        self.bag.reset();
        self.tale.restart();
        self.hud.clear_notice();
    }

    /// Nobody is playing on the title screen, but the scene goes on: the
    /// wanderer stands idle and the others keep their routines,
    /// with no interest in it. Starting or loading a game ends that.
    fn live_scene(&mut self, ctx: &mut Context) {
        let dt = ctx.time.delta();
        ctx.world.npcs_ignore_player = true;
        let Some(player) = self.player.as_mut() else { return };
        let mut unused = ctx.camera;
        player.update(&Input::default(), &mut ctx.physics, &mut unused, dt);
        if let Some(transform) = ctx.world.get_mut::<Transform>(self.avatar) {
            *transform = player.transform();
        }
        let moveset = crate::moveset_of(&ctx.world, self.avatar);
        if let Some(animator) = ctx.world.get_mut::<Animator>(self.avatar) {
            animate(animator, moveset, player, &Input::default());
        }
        let center = player.body.center();
        ctx.world.update_npcs(&mut ctx.physics, center, dt);
        self.animate_heart(ctx);
    }
}

const MENU_OPTIONS: &str = "Opções";

/// Hands out rows down the screen, each `size` wide and tall, `GAP` apart.
pub(crate) struct Column {
    x: f32,
    y: f32,
    size: Vec2,
}

impl Column {
    pub(crate) fn x(&self) -> f32 {
        self.x
    }

    pub(crate) fn y(&self) -> f32 {
        self.y
    }

    pub(crate) fn row(&mut self) -> Rect {
        let rect = Rect::new(Vec2::new(self.x, self.y), self.size);
        self.y += self.size.y + GAP;
        rect
    }
}

fn column_height(rows: u32, size: Vec2) -> f32 {
    rows as f32 * size.y + (rows - 1) as f32 * GAP
}

/// How long ago, for a person: `seconds` of it.
fn ago(seconds: u64) -> String {
    match seconds {
        0..60 => "Agora".to_owned(),
        60..3600 => format!("Há {} min", seconds / 60),
        3600..86_400 => format!("Há {} h", seconds / 3600),
        86_400..172_800 => "Há 1 dia".to_owned(),
        _ => format!("Há {} dias", seconds / 86_400),
    }
}

/// Where the menus' lists start.
pub(crate) fn left_margin(ui: &Ui) -> f32 {
    (ui.size().x * 0.06).max(24.0)
}

/// Darkness over the left of the screen up to `width`, fading out after it, which leaves the
/// rest of the scene to look at.
fn shade(ui: &mut Ui, width: f32) {
    let height = ui.size().y;
    // A dither all the way, since a flat dark next to one shows where they meet.
    ui.dither(Rect::new(Vec2::ZERO, Vec2::new(width, height)), SHADE, SHADE.w, false);
    ui.dither(Rect::new(Vec2::X * width, Vec2::new(SHADE_FADE, height)), SHADE, 0.0, false);
}

/// The title's name and list, in the middle of the left of the screen.
fn title_column(ui: &Ui) -> (Vec2, Column) {
    let height = TITLE_SIZE + TITLE_GAP + column_height(5, BUTTON);
    let left = left_margin(ui);
    let top = ((ui.size().y - height) * 0.5).max(8.0);
    let column = Column { x: left, y: top + TITLE_SIZE + TITLE_GAP, size: BUTTON };
    (Vec2::new(left, top), column)
}

fn draw_title(ui: &mut Ui, can_continue: bool) -> Option<Action> {
    ui.theme.text_size = BUTTON_TEXT;
    let (at, mut column) = title_column(ui);
    shade(ui, at.x + BUTTON.x);
    ui.text(at, TITLE, &TextStyle::new(TITLE_SIZE).color(PALE).outline(Vec4::ZERO));

    let mut action = None;
    if ui.button(column.row(), "Novo jogo") {
        action = Some(Action::NewGame);
    }
    if ui.button_if(can_continue, column.row(), "Continuar") {
        action = Some(Action::Load);
    }
    if ui.button_if(can_continue, column.row(), MENU_LOAD) {
        action = Some(Action::OpenSaves(false));
    }
    if ui.button(column.row(), MENU_OPTIONS) {
        action = Some(Action::OpenOptions);
    }
    if ui.button(column.row(), "Sair") {
        action = Some(Action::Quit);
    }
    action
}

/// A heading and a list on the left of the screen, on the dark.
pub(crate) fn menu_list(ui: &mut Ui, heading: &str, rows: u32, row: Vec2) -> Column {
    let left = left_margin(ui);
    let row = Vec2::new(row.x.min(ui.size().x - left * 2.0), row.y);
    let height = HEADING_SIZE + HEADING_GAP + column_height(rows, row);
    let top = ((ui.size().y - height) * 0.5).max(8.0);
    shade(ui, left + row.x);
    let style = TextStyle::new(HEADING_SIZE).color(PALE);
    ui.text(Vec2::new(left, top), &heading.to_uppercase(), &style);
    let under = Rect::new(Vec2::new(left, top + HEADING_SIZE + HEADING_GAP * 0.4), Vec2::new(row.x, ui.line()));
    ui.dither(under, LINE, 0.0, false);
    Column { x: left, y: top + HEADING_SIZE + HEADING_GAP, size: row }
}

/// What the keys do while playing, with the ones of this device.
fn play_keys(input: &Input) -> Vec<(&'static str, String)> {
    let gamepad = input.gamepad_last();
    let walking = if gamepad {
        "Analógico".to_owned()
    } else {
        let walk = [actions::MOVE_FORWARD, actions::MOVE_LEFT, actions::MOVE_BACK, actions::MOVE_RIGHT];
        walk.map(|action| input.actions.label(action, false)).join(" ")
    };
    let mut keys = vec![("Andar", walking)];
    keys.extend(PLAY_KEYS.iter().map(|(action, name)| (*name, input.actions.label(action, gamepad))));
    keys
}

fn draw_pause(ui: &mut Ui, can_load: bool, keys: &[(&str, String)]) -> Option<Action> {
    ui.theme.text_size = BUTTON_TEXT;
    draw_play_keys(ui, keys);
    let mut column = menu_list(ui, "Pausa", 6, BUTTON);
    let mut action = None;
    if ui.button(column.row(), "Continuar") {
        action = Some(Action::Resume);
    }
    if ui.button(column.row(), MENU_SAVE) {
        action = Some(Action::OpenSaves(true));
    }
    if ui.button_if(can_load, column.row(), MENU_LOAD) {
        action = Some(Action::OpenSaves(false));
    }
    if ui.button(column.row(), MENU_OPTIONS) {
        action = Some(Action::OpenOptions);
    }
    if ui.button(column.row(), "Menu principal") {
        action = Some(Action::MainMenu);
    }
    if ui.button(column.row(), "Sair") {
        action = Some(Action::Quit);
    }
    action
}

/// On the right of the pause: what each thing does while playing. No box, only a dark that
/// comes from the edge of the screen.
fn draw_play_keys(ui: &mut Ui, keys: &[(&str, String)]) {
    let size = ui.size();
    if size.x < PLAY_KEYS_FROM {
        return;
    }
    const WIDTH: f32 = 300.0;
    let right = size.x - left_margin(ui);
    let from = right - WIDTH;
    ui.dither(Rect::new(Vec2::X * (from - 16.0), Vec2::new(size.x - from + 16.0, size.y)), SHADE, SHADE.w, false);
    let fade = Rect::new(Vec2::X * (from - 16.0 - SHADE_FADE), Vec2::new(SHADE_FADE, size.y));
    ui.dither(fade, Vec4::new(SHADE.x, SHADE.y, SHADE.z, 0.0), SHADE.w, false);
    let top = (size.y - PLAY_KEY_ROW * keys.len() as f32) * 0.5;
    let name = TextStyle::new(16.0).color(DIM).anchor(Anchor::Left);
    let key = TextStyle::new(16.0).color(PALE).anchor(Anchor::Right);
    for (row, (what, control)) in keys.iter().enumerate() {
        let y = top + PLAY_KEY_ROW * (row as f32 + 0.5);
        ui.text(Vec2::new(from, y), &what.to_uppercase(), &name);
        ui.text(Vec2::new(right, y), &control.to_uppercase(), &key);
    }
}

/// Returns whether anything changed, and what was asked for.
fn draw_options(ui: &mut Ui, options: &mut Options) -> (bool, Option<Action>) {
    ui.theme.text_size = OPTION_TEXT;
    let mut column = menu_list(ui, MENU_OPTIONS, 7, OPTION_ROW);
    let mut changed = false;
    changed |= ui.slider(column.row(), "Volume geral", &mut options.master_volume, 0.0..=1.0, VOLUME_STEP);
    changed |= ui.slider(column.row(), "Ambiente", &mut options.ambient_volume, 0.0..=1.0, VOLUME_STEP);
    changed |= ui.slider(column.row(), "Efeitos", &mut options.effects_volume, 0.0..=1.0, VOLUME_STEP);
    let mut scale = options.ui_scale_index();
    if ui.choice(column.row(), "Tamanho da interface", &UI_SCALE_NAMES, &mut scale) {
        options.ui_scale = grimora::UI_SCALES[scale];
        changed = true;
    }
    let mut action = None;
    if ui.button(column.row(), MENU_VIDEO) {
        action = Some(Action::OpenVideo);
    }
    if ui.button(column.row(), MENU_CONTROLS) {
        action = Some(Action::OpenControls);
    }
    if ui.button(column.row(), "Voltar") {
        action = Some(Action::Back);
    }
    (changed, action)
}

/// Returns whether anything changed, and what was asked for. `size` is the
/// window's, to show the closest resolution until the player picks one.
fn draw_video(ui: &mut Ui, options: &mut Options, size: (u32, u32)) -> (bool, Option<Action>) {
    ui.theme.text_size = OPTION_TEXT;
    let mut column = menu_list(ui, MENU_VIDEO, 7, OPTION_ROW);
    let mut changed = ui.toggle(column.row(), "Tela cheia", &mut options.fullscreen);
    let current = options.resolution.unwrap_or(size);
    let mut resolution = grimora::RESOLUTIONS
        .iter()
        .position(|option| option.0 >= current.0 && option.1 >= current.1)
        .unwrap_or(grimora::RESOLUTIONS.len() - 1);
    if ui.choice(column.row(), "Resolução", &RESOLUTION_NAMES, &mut resolution) {
        options.resolution = Some(grimora::RESOLUTIONS[resolution]);
        changed = true;
    }
    changed |= ui.toggle(column.row(), "V-Sync", &mut options.vsync);
    let mut shadows = ShadowQuality::ALL.iter().position(|quality| *quality == options.shadows).unwrap_or(1);
    if ui.choice(column.row(), "Sombras", &SHADOW_NAMES, &mut shadows) {
        options.shadows = ShadowQuality::ALL[shadows];
        changed = true;
    }
    changed |= ui.toggle(column.row(), "Texturas suaves", &mut options.smooth_textures);
    changed |= ui.toggle(column.row(), "Antisserrilhado (ao reiniciar)", &mut options.antialiasing);
    let back = ui.button(column.row(), "Voltar");
    (changed, back.then_some(Action::Back))
}

/// MSAA is fixed when the window opens, so its option is read before that.
/// Automated runs ignore the saved options, as in [`Sandbox::start_menu`].
pub fn apply_startup_options(config: &mut EngineConfig) {
    if config.screenshot.is_some() || config.frames.is_some() {
        return;
    }
    let saves = config.save_dir.clone().map_or_else(|| SaveStore::for_game(&config.title), SaveStore::new);
    Options::load(&Options::store(&saves)).apply_at_startup(config);
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::DEFAULT_SCENE;

    const DT: f32 = 1.0 / 60.0;

    /// A save folder, deleted at the end of the test.
    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("grimora_menu_{name}_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            Self(dir)
        }

        fn options_file(&self) -> PathBuf {
            self.0.join("config").join("options.ron")
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn start_with(config: EngineConfig, dir: &Path) -> (Sandbox, Context) {
        let config = EngineConfig {
            scene: Some(DEFAULT_SCENE.into()),
            save_dir: Some(dir.into()),
            escape_quits: false,
            ..config
        };
        let mut game = Sandbox::new();
        let ctx = Context::headless(config, &mut game).unwrap();
        (game, ctx)
    }

    fn start(dir: &Path) -> (Sandbox, Context) {
        start_with(EngineConfig::default(), dir)
    }

    fn frames(game: &mut Sandbox, ctx: &mut Context, count: u32) {
        for _ in 0..count {
            ctx.headless_frame(game, DT).unwrap();
        }
    }

    fn press(game: &mut Sandbox, ctx: &mut Context, key: KeyCode) {
        ctx.input.key_down(key);
        frames(game, ctx, 1);
        ctx.input.key_up(key);
        frames(game, ctx, 1);
    }

    /// Clicks at a point of the window, as a player would: move, down, up.
    fn click(game: &mut Sandbox, ctx: &mut Context, at: Vec2) {
        ctx.input.set_cursor(at);
        frames(game, ctx, 1);
        ctx.input.button_down(MouseButton::Left);
        frames(game, ctx, 1);
        ctx.input.button_up(MouseButton::Left);
        frames(game, ctx, 1);
    }

    fn screen(game: &Sandbox) -> Option<Screen> {
        game.menu.stack.last().copied()
    }

    /// From the title to the controls screen, with the keyboard.
    fn open_controls(game: &mut Sandbox, ctx: &mut Context) {
        // Options is the third row of the title, with no save to continue.
        press(game, ctx, KeyCode::ArrowDown);
        press(game, ctx, KeyCode::Enter);
        assert_eq!(screen(game), Some(Screen::Options));
        for _ in 0..5 {
            press(game, ctx, KeyCode::ArrowDown);
        }
        press(game, ctx, KeyCode::Enter);
        assert_eq!(screen(game), Some(Screen::Controls));
    }

    #[test]
    fn rebinding_a_control_works_at_once_and_after_restarting() {
        let dir = Folder::new("rebind");
        let (mut game, mut ctx) = start(&dir.0);
        frames(&mut game, &mut ctx, 2);
        open_controls(&mut game, &mut ctx);
        // Sensitivity, invert Y, then the rows: forward, back, left, right, run, jump.
        for _ in 0..7 {
            press(&mut game, &mut ctx, KeyCode::ArrowDown);
        }
        assert!(ctx.ui.is_focused("Pular"));
        press(&mut game, &mut ctx, KeyCode::Enter);
        assert_eq!(game.menu.capturing, Some(actions::JUMP));

        // Reserved keys are refused and it keeps waiting; Esc gives up without leaving the screen.
        press(&mut game, &mut ctx, KeyCode::F12);
        assert_eq!(game.menu.capturing, Some(actions::JUMP));
        press(&mut game, &mut ctx, KeyCode::Escape);
        assert_eq!(game.menu.capturing, None);
        assert_eq!(screen(&game), Some(Screen::Controls));
        assert_eq!(ctx.input.actions.label(actions::JUMP, false), "Espaço");

        // The key takes the action, without pressing the row again or moving the focus.
        press(&mut game, &mut ctx, KeyCode::Enter);
        press(&mut game, &mut ctx, KeyCode::KeyS);
        assert_eq!(game.menu.capturing, None);
        assert!(ctx.ui.is_focused("Pular"));
        assert_eq!(ctx.input.actions.label(actions::JUMP, false), "S");
        assert_eq!(ctx.input.actions.label(actions::MOVE_BACK, false), "Espaço", "the keys swapped");

        // A gamepad button goes alongside the keys.
        press(&mut game, &mut ctx, KeyCode::Enter);
        ctx.input.pad_button_down(PadButton::North);
        frames(&mut game, &mut ctx, 1);
        ctx.input.pad_button_up(PadButton::North);
        frames(&mut game, &mut ctx, 1);
        assert_eq!(ctx.input.actions.label(actions::JUMP, true), "Y");
        assert_eq!(ctx.input.actions.label(actions::JUMP, false), "S");

        press(&mut game, &mut ctx, KeyCode::Escape);
        assert_eq!(screen(&game), Some(Screen::Options));
        assert!(ctx.ui.is_focused(MENU_CONTROLS));
        drop(game);

        let (mut game, mut ctx) = start(&dir.0);
        frames(&mut game, &mut ctx, 2);
        assert_eq!(ctx.input.actions.label(actions::JUMP, false), "S");
        assert_eq!(ctx.input.actions.label(TOGGLE_FLY, false), "Tab", "the sandbox's own actions stay");
        open_controls(&mut game, &mut ctx);
        ctx.ui.set_focus(RESTORE);
        frames(&mut game, &mut ctx, 1);
        press(&mut game, &mut ctx, KeyCode::Enter);
        assert_eq!(ctx.input.actions.label(actions::JUMP, false), "Espaço");
    }

    #[test]
    fn the_guard_goes_up_meets_blows_in_turn_and_comes_down() {
        let dir = Folder::new("guard");
        let (mut game, mut ctx) = start_with(EngineConfig { load: Some("none".into()), ..Default::default() }, &dir.0);
        frames(&mut game, &mut ctx, 30);
        ctx.input.button_down(MouseButton::Right);
        frames(&mut game, &mut ctx, 3);
        let over = |game: &Sandbox, ctx: &Context| {
            ctx.world.get::<Animator>(game.avatar).unwrap().overlay_current().map(str::to_string)
        };
        assert_eq!(over(&game, &ctx).as_deref(), Some("sword_guard_1"));
        assert!(ctx.world.get::<Guard>(game.avatar).unwrap().raised);

        let avatar = game.avatar;
        for (block, stance) in [("sword_block_2", "sword_guard_2"), ("sword_block_1", "sword_guard_1")] {
            game.on_blocked(&mut ctx, avatar, Vec3::Y, Vec3::Z);
            frames(&mut game, &mut ctx, 1);
            assert_eq!(over(&game, &ctx).as_deref(), Some(block));
            frames(&mut game, &mut ctx, 45);
            assert_eq!(over(&game, &ctx).as_deref(), Some(stance));
        }

        // The legs are free: it walks with the guard up, and without turning round.
        let (start, facing) = {
            let player = game.player.as_ref().unwrap();
            (player.body.position, player.facing)
        };
        ctx.input.key_down(KeyCode::KeyS);
        frames(&mut game, &mut ctx, 40);
        ctx.input.key_up(KeyCode::KeyS);
        let player = game.player.as_ref().unwrap();
        assert!(player.body.position.distance(start) > 0.3, "it should walk with the guard up");
        assert!((player.facing - facing).abs() < 0.2, "and keep facing the same way");
        assert_eq!(ctx.world.get::<Animator>(game.avatar).unwrap().current(), Some("sword_back"));

        ctx.input.button_up(MouseButton::Right);
        frames(&mut game, &mut ctx, 2);
        assert_eq!(over(&game, &ctx), None);
        assert!(!ctx.world.get::<Guard>(game.avatar).unwrap().raised);
    }

    #[test]
    fn the_rebound_key_moves_the_player_and_the_old_one_does_not() {
        let dir = Folder::new("rebound_play");
        let (mut game, mut ctx) = start_with(EngineConfig { load: Some("none".into()), ..Default::default() }, &dir.0);
        frames(&mut game, &mut ctx, 30);
        ctx.input.actions.rebind(actions::MOVE_FORWARD, Binding::Key(KeyCode::KeyO));
        let start = game.player.as_ref().unwrap().body.position;
        ctx.input.key_down(KeyCode::KeyW);
        frames(&mut game, &mut ctx, 20);
        ctx.input.key_up(KeyCode::KeyW);
        assert!(game.player.as_ref().unwrap().body.position.distance(start) < 0.05);
        ctx.input.key_down(KeyCode::KeyO);
        frames(&mut game, &mut ctx, 40);
        ctx.input.key_up(KeyCode::KeyO);
        assert!(game.player.as_ref().unwrap().body.position.distance(start) > 0.5);
    }

    #[test]
    fn the_gamepad_plays_and_works_the_menus() {
        let dir = Folder::new("gamepad");
        let (mut game, mut ctx) = start(&dir.0);
        frames(&mut game, &mut ctx, 2);
        let tap = |game: &mut Sandbox, ctx: &mut Context, button| {
            ctx.input.pad_button_down(button);
            frames(game, ctx, 1);
            ctx.input.pad_button_up(button);
            frames(game, ctx, 1);
        };
        // New game with A.
        tap(&mut game, &mut ctx, PadButton::South);
        frames(&mut game, &mut ctx, 30);
        assert_eq!(screen(&game), None);

        // Half a stick walks at about half the speed; the right stick turns the camera.
        let start_at = game.player.as_ref().unwrap().body.position;
        let yaw = game.player.as_ref().unwrap().camera_yaw;
        ctx.input.set_pad_axis(PadAxis::LeftY, 0.6);
        ctx.input.set_pad_axis(PadAxis::RightX, 1.0);
        frames(&mut game, &mut ctx, 60);
        ctx.input.set_pad_axis(PadAxis::LeftY, 0.0);
        ctx.input.set_pad_axis(PadAxis::RightX, 0.0);
        let player = game.player.as_ref().unwrap();
        assert!(player.body.position.distance(start_at) > 0.3);
        assert!(player.body.position.distance(start_at) < 2.25 * 0.5 + 0.3);
        assert!(player.camera_yaw < yaw, "turns right");

        // Start pauses, the D-pad moves, B goes back out of a submenu and Start resumes.
        tap(&mut game, &mut ctx, PadButton::Start);
        assert_eq!(screen(&game), Some(Screen::Pause));
        // Continue, save and options: with no save yet, load is skipped.
        for _ in 0..2 {
            tap(&mut game, &mut ctx, PadButton::DPadDown);
        }
        tap(&mut game, &mut ctx, PadButton::South);
        assert_eq!(screen(&game), Some(Screen::Options));
        tap(&mut game, &mut ctx, PadButton::East);
        assert_eq!(screen(&game), Some(Screen::Pause));
        tap(&mut game, &mut ctx, PadButton::Start);
        assert_eq!(screen(&game), None);
    }

    #[test]
    fn automated_runs_go_straight_into_play() {
        let dir = Folder::new("automated");
        std::fs::create_dir_all(dir.options_file().parent().unwrap()).unwrap();
        std::fs::write(dir.options_file(), "(master_volume: 0.1, ui_scale: 1.5, fullscreen: true)").unwrap();
        let runs = [
            EngineConfig { screenshot: Some("x.png".into()), ..Default::default() },
            EngineConfig { frames: Some(10), ..Default::default() },
        ];
        for config in runs {
            let (mut game, mut ctx) = start_with(config, &dir.0);
            frames(&mut game, &mut ctx, 3);
            assert_eq!(screen(&game), None);
            assert!(!ctx.is_paused());
            assert_eq!(ctx.audio.master_volume(), 1.0, "saved options don't affect automated runs");
            assert_eq!(ctx.ui_scale(), 1.0);
            assert!(!ctx.is_fullscreen());
        }
        let runs = [
            EngineConfig { load: Some("none".into()), ..Default::default() },
            EngineConfig { camera_position: Some(Vec3::new(0.0, 5.0, 20.0)), ..Default::default() },
            EngineConfig { look_at: Some(Vec3::ZERO), ..Default::default() },
            EngineConfig { ui: false, ..Default::default() },
        ];
        for config in runs {
            let (mut game, mut ctx) = start_with(config, &dir.0);
            frames(&mut game, &mut ctx, 1);
            assert_eq!(screen(&game), None);
        }
    }

    #[test]
    fn the_menu_flag_opens_a_menu_for_capture() {
        let dir = Folder::new("flag");
        let shot = |menu: &str| EngineConfig {
            screenshot: Some("x.png".into()),
            menu: Some(menu.into()),
            ..Default::default()
        };
        let (mut game, mut ctx) = start_with(shot("title"), &dir.0);
        frames(&mut game, &mut ctx, 1);
        assert_eq!(screen(&game), Some(Screen::Title));
        assert!(!ctx.is_paused(), "the scene stays alive behind the title");

        let (mut game, mut ctx) = start_with(shot("options"), &dir.0);
        frames(&mut game, &mut ctx, 5);
        assert_eq!(screen(&game), None, "plays for a moment first");
        frames(&mut game, &mut ctx, CAPTURE_WARMUP_FRAMES as u32);
        assert_eq!(screen(&game), Some(Screen::Options));
        assert!(ctx.is_paused());

        let (_, ctx) = start_with(shot("nope"), &dir.0);
        assert!(ctx.exit_requested(), "a mistyped name doesn't silently capture the game");
        assert!(!dir.options_file().exists(), "capturing writes nothing");
    }

    #[test]
    fn title_screen_starts_a_new_game_and_skips_continue_without_a_save() {
        let dir = Folder::new("title");
        let (mut game, mut ctx) = start(&dir.0);
        frames(&mut game, &mut ctx, 2);
        assert_eq!(screen(&game), Some(Screen::Title));
        assert!(!ctx.cursor_grabbed());
        assert!(ctx.ui.is_focused("Novo jogo"));

        press(&mut game, &mut ctx, KeyCode::ArrowDown);
        assert!(ctx.ui.is_focused(MENU_OPTIONS), "Continuar is disabled with no save");
        press(&mut game, &mut ctx, KeyCode::KeyW);
        assert!(ctx.ui.is_focused("Novo jogo"));

        // Esc has nowhere to go from here, and doesn't quit either.
        press(&mut game, &mut ctx, KeyCode::Escape);
        assert_eq!(screen(&game), Some(Screen::Title));
        assert!(!ctx.exit_requested());

        let camera = ctx.camera.position;
        press(&mut game, &mut ctx, KeyCode::Enter);
        assert_eq!(screen(&game), None);
        frames(&mut game, &mut ctx, 2);
        assert!(ctx.cursor_grabbed(), "play takes the cursor back");
        assert!(ctx.camera.position != camera, "the camera moved behind the wanderer");
    }

    #[test]
    fn title_screen_keeps_the_scene_alive() {
        let dir = Folder::new("alive");
        let (mut game, mut ctx) = start(&dir.0);
        let start_at = ctx.camera.position;
        frames(&mut game, &mut ctx, 30);
        assert_eq!(ctx.camera.position, start_at, "the title holds the scene's camera");
        let avatar = ctx.world.transform(game.avatar).unwrap().position;
        assert!(avatar.length() > 1.0, "the wanderer stands at the spawn, not at the origin: {avatar}");
        let animator = ctx.world.get::<Animator>(game.avatar).unwrap();
        assert_eq!(animator.current(), Some("sword_idle"));
    }

    #[test]
    fn esc_pauses_and_resumes_and_frees_the_cursor() {
        let dir = Folder::new("pause");
        let (mut game, mut ctx) = start_with(EngineConfig { load: Some("none".into()), ..Default::default() }, &dir.0);
        frames(&mut game, &mut ctx, 5);
        assert!(ctx.cursor_grabbed());

        press(&mut game, &mut ctx, KeyCode::Escape);
        assert_eq!(screen(&game), Some(Screen::Pause));
        assert!(ctx.is_paused() && !ctx.cursor_grabbed());
        assert!(ctx.ui.is_focused("Continuar"));

        // Nothing the game does reaches the character.
        let before = game.player.as_ref().unwrap().body.position;
        ctx.input.key_down(KeyCode::KeyW);
        frames(&mut game, &mut ctx, 20);
        ctx.input.key_up(KeyCode::KeyW);
        frames(&mut game, &mut ctx, 1);
        assert_eq!(game.player.as_ref().unwrap().body.position, before);

        press(&mut game, &mut ctx, KeyCode::Escape);
        assert_eq!(screen(&game), None);
        assert!(!ctx.is_paused() && ctx.cursor_grabbed());
    }

    /// Puts the player right next to the enemy, facing it, and returns the enemy.
    fn next_to_the_enemy(game: &mut Sandbox, ctx: &mut Context) -> Entity {
        let hostile = ctx.world.find("hostile").unwrap();
        let at = ctx.world.transform(hostile).unwrap().position;
        let player = game.player.as_mut().unwrap();
        player.body.position = at + Vec3::new(1.2, 0.1, 0.0);
        player.facing = std::f32::consts::FRAC_PI_2;
        player.camera_yaw = std::f32::consts::FRAC_PI_2;
        hostile
    }

    #[test]
    fn a_fight_waits_for_the_pause_menu_and_goes_on_after_it() {
        let dir = Folder::new("fight");
        let (mut game, mut ctx) = start_with(EngineConfig { load: Some("none".into()), ..Default::default() }, &dir.0);
        frames(&mut game, &mut ctx, 5);
        let hostile = next_to_the_enemy(&mut game, &mut ctx);
        // Just the player's blow here.
        ctx.world.npcs_ignore_player = true;
        press(&mut game, &mut ctx, KeyCode::KeyF);
        frames(&mut game, &mut ctx, 3);
        let avatar = game.avatar;
        assert!(ctx.world.get::<Fighter>(avatar).unwrap().busy());
        assert_eq!(ctx.world.get::<Animator>(avatar).unwrap().current(), Some("medium_1"));

        press(&mut game, &mut ctx, KeyCode::Escape);
        assert!(ctx.is_paused());
        let time = ctx.world.get::<Animator>(avatar).unwrap().time();
        // The attack keys do nothing in the menu, and the blow doesn't land.
        ctx.input.key_down(KeyCode::KeyF);
        frames(&mut game, &mut ctx, 90);
        ctx.input.key_up(KeyCode::KeyF);
        assert_eq!(ctx.world.get::<Animator>(avatar).unwrap().time(), time);
        assert_eq!(ctx.world.get::<Health>(hostile).unwrap().current, 30.0);

        press(&mut game, &mut ctx, KeyCode::Escape);
        assert!(!ctx.is_paused());
        frames(&mut game, &mut ctx, 70);
        assert_eq!(ctx.world.get::<Health>(hostile).unwrap().current, 20.0);
        assert!(!ctx.world.get::<Fighter>(avatar).unwrap().busy());
        assert_eq!(game.foe.map(|(foe, _)| foe), Some(hostile));
    }

    #[test]
    fn q_locks_on_the_enemy_in_view_and_lets_go() {
        let dir = Folder::new("lock");
        let (mut game, mut ctx) = start_with(EngineConfig { load: Some("none".into()), ..Default::default() }, &dir.0);
        frames(&mut game, &mut ctx, 5);
        let hostile = next_to_the_enemy(&mut game, &mut ctx);
        game.player.as_mut().unwrap().body.position += Vec3::new(3.0, 0.0, 0.0);
        ctx.world.npcs_ignore_player = true;
        frames(&mut game, &mut ctx, 2);

        press(&mut game, &mut ctx, KeyCode::KeyQ);
        assert_eq!(game.locked_on, Some(hostile));
        let player = game.player.as_ref().unwrap();
        let target = player.lock_target.unwrap();
        // The camera ends up looking at it, and the player faces it, walking or not.
        frames(&mut game, &mut ctx, 60);
        let player = game.player.as_ref().unwrap();
        let to = target - player.body.position;
        let wanted = (-to.x).atan2(-to.z);
        assert!(
            (player.facing - wanted)
                .rem_euclid(std::f32::consts::TAU)
                .min((wanted - player.facing).rem_euclid(std::f32::consts::TAU))
                < 0.2
        );

        press(&mut game, &mut ctx, KeyCode::KeyQ);
        assert_eq!(game.locked_on, None);
        assert!(game.player.as_ref().unwrap().lock_target.is_none());
    }

    #[test]
    fn a_roll_travels_cant_be_hurt_at_first_and_costs_stamina() {
        let dir = Folder::new("roll");
        let (mut game, mut ctx) = start_with(EngineConfig { load: Some("none".into()), ..Default::default() }, &dir.0);
        frames(&mut game, &mut ctx, 5);
        let avatar = game.avatar;
        ctx.world.npcs_ignore_player = true;
        let before = game.player.as_ref().unwrap().body.position;
        let yaw = game.player.as_ref().unwrap().camera_yaw;

        ctx.input.key_down(KeyCode::KeyW);
        press(&mut game, &mut ctx, KeyCode::ControlLeft);
        ctx.input.key_up(KeyCode::KeyW);
        assert_eq!(ctx.world.get::<Animator>(avatar).unwrap().current(), Some("sword_roll"));
        assert!(ctx.world.get::<Health>(avatar).unwrap().untouchable());
        assert!(ctx.world.get::<Stamina>(avatar).unwrap().current < crate::PLAYER_STAMINA);
        let mut physics_free = ctx.world.damage(avatar, 5.0, &mut ctx.physics);
        assert_eq!(physics_free.take(), None, "a roll's start can't be hurt");

        frames(&mut game, &mut ctx, 60);
        let moved = game.player.as_ref().unwrap().body.position - before;
        let along = moved.dot(Vec3::new(-yaw.sin(), 0.0, -yaw.cos()));
        assert!(along > 2.0, "it should roll forward, went {along} ({moved})");
        assert!(game.dodge.is_none());
        assert!(!ctx.world.get::<Health>(avatar).unwrap().untouchable());
    }

    #[test]
    fn the_enemy_kills_the_player_who_gets_up_at_the_start() {
        let dir = Folder::new("death");
        let (mut game, mut ctx) = start_with(EngineConfig { load: Some("none".into()), ..Default::default() }, &dir.0);
        frames(&mut game, &mut ctx, 5);
        let hostile = next_to_the_enemy(&mut game, &mut ctx);
        let avatar = game.avatar;
        ctx.world.get_mut::<Health>(avatar).unwrap().current = 1.0;

        let mut waited = 0;
        while game.dead_for.is_none() && waited < 600 {
            frames(&mut game, &mut ctx, 1);
            waited += 1;
        }
        assert!(game.dead_for.is_some(), "still alive after {waited} frames");
        frames(&mut game, &mut ctx, 1);
        assert!(ctx.world.is_ragdoll(avatar), "the body falls as a ragdoll");
        assert!(game.player.as_ref().unwrap().locked);

        // Dead, it can't swing or save, and the enemy loses interest.
        ctx.input.key_down(KeyCode::KeyF);
        frames(&mut game, &mut ctx, 2);
        ctx.input.key_up(KeyCode::KeyF);
        assert!(!ctx.world.get::<Fighter>(avatar).unwrap().busy());
        press(&mut game, &mut ctx, KeyCode::F6);
        assert!(ctx.saves.slots().unwrap().is_empty());
        // It finishes the swing it is in first.
        frames(&mut game, &mut ctx, 80);
        assert!(!ctx.world.get::<Brain>(hostile).unwrap().noticed());

        frames(&mut game, &mut ctx, (crate::RESPAWN_SECONDS * 60.0) as u32);
        assert!(game.dead_for.is_none());
        let health = ctx.world.get::<Health>(avatar).unwrap();
        assert_eq!(health.current, health.max);
        drop(health);
        assert!(!ctx.world.is_ragdoll(avatar));
        assert!(!game.player.as_ref().unwrap().locked);
        let spawn = ctx.world.player_spawn.unwrap().position;
        assert!(game.player.as_ref().unwrap().body.position.distance(spawn) < 0.5);
        assert!(!ctx.world.npcs_ignore_player);
    }

    #[test]
    fn nobody_hunts_on_the_title_screen_until_the_game_starts() {
        let dir = Folder::new("title_hunt");
        let (mut game, mut ctx) = start_with(EngineConfig::default(), &dir.0);
        frames(&mut game, &mut ctx, 2);
        assert_eq!(screen(&game), Some(Screen::Title));
        assert!(ctx.world.npcs_ignore_player);

        // A new game after a death starts over, on its feet.
        game.dead_for = Some(1.0);
        game.apply_menu_action(&mut ctx, Action::NewGame);
        frames(&mut game, &mut ctx, 3);
        assert_eq!(screen(&game), None);
        assert!(game.dead_for.is_none());
        assert!(!ctx.world.npcs_ignore_player);
        let health = ctx.world.get::<Health>(game.avatar).unwrap();
        assert_eq!(health.current, health.max);
    }

    #[test]
    fn saving_from_the_pause_menu_enables_continue_on_the_title() {
        let dir = Folder::new("save");
        let (mut game, mut ctx) = start_with(EngineConfig { load: Some("none".into()), ..Default::default() }, &dir.0);
        frames(&mut game, &mut ctx, 5);
        press(&mut game, &mut ctx, KeyCode::Escape);
        // Carregar is disabled with no save, so the list is 5 long for now.
        press(&mut game, &mut ctx, KeyCode::ArrowDown);
        assert!(ctx.ui.is_focused("Salvar"));
        press(&mut game, &mut ctx, KeyCode::Enter);
        assert_eq!(screen(&game), Some(Screen::Saves { saving: true }));
        frames(&mut game, &mut ctx, 1);
        press(&mut game, &mut ctx, KeyCode::Enter);
        assert!(ctx.saves.exists(QUICK_SLOT));
        assert_eq!(screen(&game), Some(Screen::Saves { saving: true }), "saving stays in the menu");
        press(&mut game, &mut ctx, KeyCode::Escape);
        assert_eq!(screen(&game), Some(Screen::Pause));
        frames(&mut game, &mut ctx, 1);
        assert!(ctx.ui.is_focused("Salvar"), "back where the player came from");

        press(&mut game, &mut ctx, KeyCode::ArrowDown);
        assert!(ctx.ui.is_focused("Carregar"), "it can be used now");
        press(&mut game, &mut ctx, KeyCode::ArrowDown);
        press(&mut game, &mut ctx, KeyCode::ArrowDown);
        assert!(ctx.ui.is_focused("Menu principal"));
        press(&mut game, &mut ctx, KeyCode::Space);
        assert_eq!(screen(&game), Some(Screen::Title));
        assert!(!ctx.is_paused(), "the title screen's scene runs");
        assert_eq!(ctx.camera.position, game.menu.title_camera.position);
        frames(&mut game, &mut ctx, 2);

        press(&mut game, &mut ctx, KeyCode::ArrowDown);
        assert!(ctx.ui.is_focused("Continuar"));
        press(&mut game, &mut ctx, KeyCode::Enter);
        assert_eq!(screen(&game), None, "the saved game loaded");
    }

    #[test]
    fn a_game_saved_in_a_slot_is_only_replaced_or_deleted_on_a_second_press() {
        let dir = Folder::new("slots");
        let (mut game, mut ctx) = start_with(EngineConfig { load: Some("none".into()), ..Default::default() }, &dir.0);
        frames(&mut game, &mut ctx, 5);
        press(&mut game, &mut ctx, KeyCode::Escape);
        press(&mut game, &mut ctx, KeyCode::ArrowDown);
        press(&mut game, &mut ctx, KeyCode::Enter);
        frames(&mut game, &mut ctx, 1);
        press(&mut game, &mut ctx, KeyCode::ArrowDown);
        assert!(ctx.ui.is_focused("Jogo 1"));
        press(&mut game, &mut ctx, KeyCode::Enter);
        assert!(ctx.saves.exists("slot_1"), "an empty slot asks nothing");
        let saved = std::fs::metadata(ctx.saves.path("slot_1").unwrap()).unwrap().modified().unwrap();

        press(&mut game, &mut ctx, KeyCode::Enter);
        assert_eq!(game.menu.sure, Some(Sure::Overwrite(1)));
        press(&mut game, &mut ctx, KeyCode::ArrowDown);
        frames(&mut game, &mut ctx, 1);
        assert_eq!(game.menu.sure, None, "moving away takes the question back");
        assert_eq!(std::fs::metadata(ctx.saves.path("slot_1").unwrap()).unwrap().modified().unwrap(), saved);

        press(&mut game, &mut ctx, KeyCode::ArrowUp);
        press(&mut game, &mut ctx, KeyCode::Delete);
        assert_eq!(game.menu.sure, Some(Sure::Delete(1)));
        press(&mut game, &mut ctx, KeyCode::Escape);
        assert_eq!(game.menu.sure, None);
        assert_eq!(screen(&game), Some(Screen::Saves { saving: true }), "Esc took back the question only");
        assert!(ctx.saves.exists("slot_1"));

        press(&mut game, &mut ctx, KeyCode::Delete);
        press(&mut game, &mut ctx, KeyCode::Enter);
        assert!(!ctx.saves.exists("slot_1"));
    }

    #[test]
    fn a_slot_is_loaded_from_its_row() {
        let dir = Folder::new("load_slot");
        let (mut game, mut ctx) = start_with(EngineConfig { load: Some("none".into()), ..Default::default() }, &dir.0);
        frames(&mut game, &mut ctx, 5);
        game.save(&ctx, "slot_2");
        game.apply_menu_action(&mut ctx, Action::OpenSaves(false));
        frames(&mut game, &mut ctx, 2);
        assert!(ctx.ui.is_focused("Jogo 2"), "the only one that can be loaded");
        press(&mut game, &mut ctx, KeyCode::Enter);
        assert_eq!(screen(&game), None);
    }

    #[test]
    fn how_long_ago_reads_like_a_person() {
        assert_eq!(ago(5), "Agora");
        assert_eq!(ago(300), "Há 5 min");
        assert_eq!(ago(7200), "Há 2 h");
        assert_eq!(ago(90_000), "Há 1 dia");
        assert_eq!(ago(400_000), "Há 4 dias");
    }

    #[test]
    fn a_failed_load_stays_in_the_menu() {
        let dir = Folder::new("broken_save");
        std::fs::create_dir_all(&dir.0).unwrap();
        std::fs::write(dir.0.join("quick.ron"), "not a save").unwrap();
        let (mut game, mut ctx) = start(&dir.0);
        frames(&mut game, &mut ctx, 2);
        press(&mut game, &mut ctx, KeyCode::ArrowDown);
        assert!(ctx.ui.is_focused("Continuar"));
        press(&mut game, &mut ctx, KeyCode::Enter);
        assert_eq!(screen(&game), Some(Screen::Title));
    }

    #[test]
    fn options_change_the_audio_and_persist() {
        let dir = Folder::new("options");
        let (mut game, mut ctx) = start(&dir.0);
        frames(&mut game, &mut ctx, 2);
        press(&mut game, &mut ctx, KeyCode::ArrowDown);
        press(&mut game, &mut ctx, KeyCode::Enter);
        assert_eq!(screen(&game), Some(Screen::Options));
        frames(&mut game, &mut ctx, 1);
        assert!(ctx.ui.is_focused("Volume geral"));

        press(&mut game, &mut ctx, KeyCode::ArrowLeft);
        press(&mut game, &mut ctx, KeyCode::ArrowLeft);
        assert!((ctx.audio.master_volume() - 0.9).abs() < 1e-3, "{}", ctx.audio.master_volume());
        press(&mut game, &mut ctx, KeyCode::ArrowDown);
        press(&mut game, &mut ctx, KeyCode::KeyA);
        assert!((ctx.audio.volume(Category::Ambient) - 0.95).abs() < 1e-3);
        press(&mut game, &mut ctx, KeyCode::ArrowDown);
        press(&mut game, &mut ctx, KeyCode::KeyA);
        assert!((ctx.audio.volume(Category::Effects) - 0.95).abs() < 1e-3);
        press(&mut game, &mut ctx, KeyCode::ArrowDown);
        press(&mut game, &mut ctx, KeyCode::ArrowRight);
        assert_eq!(ctx.ui_scale(), 1.25);
        press(&mut game, &mut ctx, KeyCode::ArrowDown);
        press(&mut game, &mut ctx, KeyCode::Enter);
        assert_eq!(screen(&game), Some(Screen::Video));
        press(&mut game, &mut ctx, KeyCode::Enter);
        assert!(ctx.is_fullscreen());
        press(&mut game, &mut ctx, KeyCode::ArrowDown);
        press(&mut game, &mut ctx, KeyCode::ArrowDown);
        press(&mut game, &mut ctx, KeyCode::Enter);
        assert!(!ctx.vsync());
        press(&mut game, &mut ctx, KeyCode::ArrowDown);
        press(&mut game, &mut ctx, KeyCode::ArrowRight);
        assert_eq!(ctx.shadow_quality(), ShadowQuality::High);
        assert!(ctx.smooth_textures(), "filtered unless the player says otherwise");
        press(&mut game, &mut ctx, KeyCode::ArrowDown);
        press(&mut game, &mut ctx, KeyCode::Enter);
        assert!(!ctx.smooth_textures());
        assert!(!dir.options_file().exists(), "written on the way out, not on every change");

        press(&mut game, &mut ctx, KeyCode::Escape);
        assert_eq!(screen(&game), Some(Screen::Options));
        frames(&mut game, &mut ctx, 1);
        assert!(ctx.ui.is_focused(MENU_VIDEO), "back where the player came from");
        press(&mut game, &mut ctx, KeyCode::Escape);
        assert_eq!(screen(&game), Some(Screen::Title));
        frames(&mut game, &mut ctx, 1);
        assert!(ctx.ui.is_focused(MENU_OPTIONS), "back where the player came from");
        assert!(dir.options_file().is_file());
        assert!(!dir.0.join("config").join("options.ron.tmp").exists());
        assert!(ctx.saves.slots().unwrap().is_empty(), "options aren't a saved game");

        // Next time the game opens, they are back.
        let (_, ctx) = start(&dir.0);
        assert!((ctx.audio.master_volume() - 0.9).abs() < 1e-3);
        assert_eq!(ctx.ui_scale(), 1.25);
        assert!(ctx.is_fullscreen());
        assert!(!ctx.vsync());
        assert_eq!(ctx.shadow_quality(), ShadowQuality::High);
        assert!(!ctx.smooth_textures());
    }

    #[test]
    fn closing_the_window_keeps_the_options() {
        let dir = Folder::new("close");
        let (mut game, mut ctx) = start(&dir.0);
        frames(&mut game, &mut ctx, 2);
        press(&mut game, &mut ctx, KeyCode::ArrowDown);
        press(&mut game, &mut ctx, KeyCode::Enter);
        frames(&mut game, &mut ctx, 1);
        press(&mut game, &mut ctx, KeyCode::ArrowLeft);
        assert!(!dir.options_file().exists());
        drop(game);
        assert!(dir.options_file().is_file());
    }

    #[test]
    fn a_broken_options_file_is_ignored() {
        let dir = Folder::new("broken_options");
        std::fs::create_dir_all(dir.options_file().parent().unwrap()).unwrap();
        std::fs::write(dir.options_file(), "(master_volume: oops").unwrap();
        let (_, ctx) = start(&dir.0);
        assert_eq!(ctx.audio.master_volume(), 1.0);

        // Fields it doesn't name keep their defaults, and odd values are made safe.
        std::fs::write(dir.options_file(), "(master_volume: 7.0, ui_scale: 3.0)").unwrap();
        let (_, ctx) = start(&dir.0);
        assert_eq!(ctx.audio.master_volume(), 1.0);
        assert_eq!(ctx.ui_scale(), 1.0);
        assert_eq!(ctx.audio.volume(Category::Effects), 1.0);
    }

    #[test]
    fn the_mouse_works_the_menus_too() {
        let dir = Folder::new("mouse");
        let (mut game, mut ctx) = start(&dir.0);
        frames(&mut game, &mut ctx, 2);

        let (_, mut column) = title_column(&ctx.ui);
        let _new_game = column.row();
        let continue_game = column.row().point(Anchor::Center);
        let _load = column.row();
        let options = column.row().point(Anchor::Center);
        click(&mut game, &mut ctx, continue_game);
        assert_eq!(screen(&game), Some(Screen::Title), "Continuar is disabled with no save");
        click(&mut game, &mut ctx, options);
        assert_eq!(screen(&game), Some(Screen::Options));
    }
}
