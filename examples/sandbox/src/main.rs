//! Grimora sandbox: walk a hooded wanderer around `assets/scenes/sandbox.ron`,
//! or press Tab for a free-flying debug camera.
//!
//!     cargo run -p sandbox
//!     cargo run -p sandbox -- --screenshot shots/sandbox.png
//!     cargo run -p sandbox -- --help
//!
//! Passing `--camera` starts in fly mode, so screenshots can frame any view.
//! F6 saves the game to the "quick" slot and F9 loads it; `--load <slot>`
//! loads a slot at startup. F3 shows frame time and world counters. E uses the
//! object in reach (the altar's crystal goes dark and wakes up again). Four
//! characters from the scene file watch, patrol, follow and run away; a fifth
//! outside the circle attacks. The left mouse button or F swings the sword, in a
//! chain of three if pressed in time; Ctrl rolls, or steps back if no direction is
//! held; Q locks the camera on an enemy, and the mouse then moves it to the next.
//! F4 runs the world in slow motion, for looking at the animations. F5 changes the weather.
//! E also takes what lies on the ground and opens the chest; I opens the inventory, and the
//! number keys use what's on the hotbar (`bag.rs`).
//! It opens on a title screen and Esc pauses (`menu.rs`); automated runs skip the title.
//! The hours pass in the scene file's `day_cycle`; `--time <hour>` stops them there.

mod bag;
mod bag_screen;
mod craft;
mod debug;
mod dialogue;
mod gear;
mod guns;
mod hud;
mod journal;
mod leap;
mod menu;
mod prompts;
mod rest;
mod sheet;
mod shop;
mod spells;
mod tale;

use bag::Bag;
use grimora::core::Weather;
use grimora::prelude::*;
use hud::Hud;
use menu::Menu;
use serde::{Deserialize, Serialize};

/// How far animals notice the character standing still, and how much further for each
/// meter per second it moves.
const FAUNA_NOTICE: f32 = 3.0;
const FAUNA_NOTICE_PER_SPEED: f32 = 1.2;
/// How tall the player is, to what's on the ground or in the water with them.
const FAUNA_BODY_HEIGHT: f32 = 1.8;
/// How far from where a great sword lands anything small on the ground is dead.
const GROUND_HIT_KILLS: f32 = 1.6;
const DEFAULT_SCENE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/scenes/sandbox.ron");

/// The animated standard body (made by tools/blender/build_wanderer.py).
const WANDERER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/models/chartest/chartest.gltf");
/// Played when the altar's crystal wakes up.
const HEART_BURST: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/effects/heart_burst.ron");
/// Played where a blow lands.
const HIT_SPARKS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/effects/hit.ron");
const BLOCK_SPARKS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/effects/block.ron");
const SWING_SOUND: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/swing.wav");
const HIT_SOUND: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/hit.wav");
/// A wooden door swinging open, and swinging shut.
const DOOR_OPEN_SOUND: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/doors/wooden_door_open.mp3");
const DOOR_CLOSE_SOUND: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/doors/wooden_door_close.mp3");
/// A roll in no armor or in plain clothes: cloth and body on the ground.
const ROLL_SOUND: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/roll/roll_light.mp3");
/// Armor knocking on itself, played on top of `ROLL_SOUND` when something is worn on the body.
const ROLL_ARMOR_SOUND: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/roll/roll_armor.mp3");
/// Two recordings of a blade meeting a blade, taken in turn so a run of blocks doesn't repeat.
const CLANG_SOUNDS: [&str; 2] = [
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/clang_1.wav"),
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/clang_2.wav"),
];
const CLANG_SOUND: &str = CLANG_SOUNDS[0];
/// A body falling to the ground, and a limb knocking on it.
const BODY_FALL_SOUNDS: [&str; 3] = [
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/body_fall_1.wav"),
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/body_fall_2.wav"),
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/body_fall_3.wav"),
];
const LIMB_KNOCK_SOUNDS: [&str; 2] = [
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/limb_knock_1.wav"),
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/limb_knock_2.wav"),
];
const GROUND_HIT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/effects/ground_hit.ron");
const SWIM_SOUND: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/swim.wav");
const BUBBLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/effects/bubbles.ron");
const STEP_SOUND: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/step_1.wav");
/// The items of a scene that has none of its own.
const ITEMS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/items/items.ron");
const ITEM_GLINT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/effects/item_glint.ron");
/// What a new game starts with in the hand, and what the enemy holds.
const FIRST_WEAPON: &str = "espada_reta";
/// The pale smear behind the blade while it hurts.
const BLADE_TRAIL: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/effects/blade_trail.ron");

/// Native chartest walks (tools/author_chartest_walk.py); other gaits use sword_moves.py.
/// The ordinary walk is `sword_brisk`; `sword_walk` is the slow one, with left Alt held.
const WALK_SPEED: f32 = 2.0;
const SLOW_SPEED: f32 = 1.2;
const RUN_SPEED: f32 = 5.5;
const STRAFE_SPEED: f32 = 0.8;
const BACK_SPEED: f32 = 0.95;
/// Seconds a cycle of each gait takes, as they were written, at the speeds above.
const WALK_CYCLE: f32 = 0.72;
const SLOW_CYCLE: f32 = 1.0;
const STRAFE_CYCLE: f32 = 0.66;
const BACK_CYCLE: f32 = 0.9;
/// Clips with a walking rhythm: going from one to another keeps the step.
const GAITS: [&str; 6] = ["sword_walk", "sword_brisk", "sword_run", "sword_back", "sword_strafe_l", "sword_strafe_r"];
const LOCOMOTION_FADE: f32 = 0.2;
/// A blow leaves the body further from where walking has it, and it needs longer to get there.
const OUT_OF_BLOW_FADE: f32 = 0.3;
const INTO_ROLL_FADE: f32 = 0.1;
/// What the body does with the kind of sword in the hand, outside the blows the weapon names.
struct Moveset {
    idle: &'static str,
    /// Plays over the upper body whenever the character isn't standing still in `idle`.
    carry: Option<&'static str>,
    /// The guard's stances, held, and the blows that end in each one. Every blow moves it to the next.
    guards: &'static [(&'static str, &'static str)],
    /// Lower for more iron.
    clang_pitch: f32,
}

const LIGHT: Moveset = Moveset {
    idle: "sword_idle",
    carry: None,
    guards: &[("sword_guard_1", "sword_block_1"), ("sword_guard_2", "sword_block_2")],
    clang_pitch: 1.0,
};
const HEAVY: Moveset = Moveset {
    idle: "heavy_idle",
    carry: Some("heavy_carry"),
    guards: &[("heavy_guard", "heavy_block")],
    clang_pitch: 0.78,
};
/// Chudan: both hands on the handle, the point at the eyes of whoever is in front.
const KATANA: Moveset = Moveset {
    idle: "katana_idle",
    carry: Some("katana_carry"),
    guards: &[("katana_guard", "katana_block")],
    clang_pitch: 0.92,
};
/// Legs that stay put under a gun: a gun plays its own clips over the upper body.
const GUN: Moveset =
    Moveset { idle: "gun_stand", carry: None, guards: &[("gun_stand", "gun_stand")], clang_pitch: 1.0 };

impl Moveset {
    fn of(weapon: &Weapon) -> &'static Moveset {
        if weapon.clip.starts_with("heavy") {
            &HEAVY
        } else if weapon.clip.starts_with("katana") {
            &KATANA
        } else {
            &LIGHT
        }
    }
}

/// Where the guard takes the body over from the legs.
const UPPER_BODY: &str = "spine";
/// Stamina each blow on the guard takes.
const BLOCK_COST: f32 = 14.0;
const GUARD_UP_FADE: f32 = 0.16;
const GUARD_DOWN_FADE: f32 = 0.22;
const BLOCK_FADE: f32 = 0.05;
/// How hard a blow on the guard shoves the character back.
const BLOCK_PUSH: f32 = 2.2;
/// So that no two blows on the guard ring quite alike.
const CLANG_PITCHES: [f32; 3] = [1.0, 0.93, 1.08];
const PLAYER_STAMINA: f32 = 100.0;
/// How long a breath lasts under the water, and how long it takes to get it all back.
const AIR_SECONDS: f32 = 50.0;
const AIR_BACK_SECONDS: f32 = 4.0;
/// Health lost per second with no air left, in blows of this size.
const DROWNING: f32 = 12.0;
/// Seconds between strokes and between breaths let out, as it swims.
const STROKE_EVERY: f32 = 0.85;
/// Where the character's ears are, above its feet.
const HEAD_HEIGHT: f32 = 1.6;
const BUBBLES_EVERY: f32 = 2.6;
const ROLL_COST: f32 = 20.0;
const BACKSTEP_COST: f32 = 10.0;
const RUN_COST: f32 = 9.0;
/// Seconds a roll can't be hurt, from its start; the rest of it can.
const ROLL_SAFE: f32 = 0.42;
const BACKSTEP_SAFE: f32 = 0.22;
/// Seconds a roll asked for too soon is remembered.
const ROLL_BUFFER: f32 = 0.3;
/// Stamina to get back before running again, once it ran out.
const WINDED_UNTIL: f32 = 25.0;
/// Mouse travel in a frame, in pixels, that moves the lock to the next enemy.
const LOCK_FLICK: f32 = 60.0;
const LOCK_FLICK_SECONDS: f32 = 0.35;
/// The weather F5 cycles through, and how long each change takes.
const WEATHERS: [(Weather, &str); 5] = [
    (Weather::Clear, "sol"),
    (Weather::Overcast, "nublado"),
    (Weather::Rain, "chuva"),
    (Weather::Storm, "tempestade"),
    (Weather::Snow, "neve"),
];
const WEATHER_CHANGE_SECONDS: f32 = 12.0;

/// The speeds F4 cycles through.
const SLOW_MOTION: [f32; 4] = [1.0, 0.5, 0.25, 0.1];

const PLAYER_HEALTH: f32 = 60.0;
const PLAYER_TEAM: u32 = 0;
/// Seconds on the ground before getting up at the start.
const RESPAWN_SECONDS: f32 = 4.0;
/// Seconds the enemy's bar stays up after last seeing it in action.
const FOE_BAR_SECONDS: f32 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum Mode {
    Play,
    Fly,
}

const QUICK_SLOT: &str = "quick";
/// Seconds that a burn stays on the status column.
const BURN_SHOWN: f32 = 6.0;

/// The sandbox's own actions, on top of the engine's (see `menu.rs`).
const TOGGLE_FLY: &str = "toggle_fly";
const QUICK_SAVE: &str = "quick_save";
const QUICK_LOAD: &str = "quick_load";

/// The sandbox's part of a saved game; the engine adds the camera and the
/// crates and stones the wanderer pushed around.
#[derive(Debug, Serialize, Deserialize)]
struct SandboxSave {
    player: CharacterState,
    mode: Mode,
    #[serde(default)]
    heart_asleep: bool,
    #[serde(default)]
    health: Option<f32>,
    #[serde(default)]
    inventory: Option<Inventory>,
    /// Which guns are out of a shot.
    #[serde(default)]
    guns_empty: std::collections::HashMap<String, bool>,
    /// Flags, quests and the animals met.
    #[serde(default)]
    story: grimora::story::Story,
    /// What's on the body, out of the inventory.
    #[serde(default)]
    body: Option<Inventory>,
    /// In a save older than [`SandboxSave::body`], the pieces of the inventory being worn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    worn: Vec<u32>,
    /// Where the character last rested, in the world.
    #[serde(default)]
    respawn: Option<(WorldPos, f32)>,
}

/// The "heart" crystal on the altar, as the scene file left it.
struct Heart {
    entity: Entity,
    /// Holds what the crystal does around it, which stays put while the crystal floats.
    altar: Option<Entity>,
    rest: Vec3,
    emissive: f32,
    light: f32,
}

/// How much of its glow the heart keeps while asleep.
const ASLEEP_GLOW: f32 = 0.06;

struct Sandbox {
    mode: Mode,
    fly: FlyCamera,
    player: Option<ThirdPersonController>,
    /// Entities re-created whenever the scene (re)loads.
    avatar: Entity,
    heart: Option<Heart>,
    heart_asleep: bool,
    /// Current brightness of the heart, easing towards awake (1) or asleep.
    heart_glow: f32,
    steps: Footsteps,
    hud: Hud,
    prompts: prompts::Prompts,
    hit_sparks: Option<ParticleEffect>,
    block_sparks: Option<ParticleEffect>,
    ground_hit: Option<ParticleEffect>,
    /// Which of `GUARD_STANCES` the guard is in.
    guard_stance: usize,
    clangs: usize,
    /// Seconds since the player died, while on the ground.
    dead_for: Option<f32>,
    /// The enemy shown at the top of the screen, and for how much longer.
    foe: Option<(Entity, f32)>,
    /// The enemy the camera stays on.
    locked_on: Option<Entity>,
    /// Seconds until the mouse can move the lock again.
    flick_wait: f32,
    /// The roll or backstep under way, and whether it may be cut short yet.
    dodge: Option<(&'static str, bool)>,
    /// Seconds left of a roll asked for too soon.
    roll_wanted: f32,
    /// Out of stamina from running, and getting it back.
    winded: bool,
    /// How much breath is left, `0..1`.
    air: f32,
    /// Seconds until the next stroke is heard, breath let out, and bit of drowning.
    stroke_in: f32,
    bubbles_in: f32,
    choke_in: f32,
    bubbles: Option<ParticleEffect>,
    /// Seconds left of the mark of a burn.
    burned_for: f32,
    /// Two points along the player's blade that leave a trail.
    trails: Option<[Entity; 2]>,
    menu: Menu,
    bag: Bag,
    debug: debug::Debug,
    spells: spells::Spells,
    guns: guns::Guns,
    leap: leap::Leap,
    tale: tale::Tale,
}

impl Sandbox {
    fn new() -> Self {
        Self {
            mode: Mode::Play,
            fly: FlyCamera::default(),
            player: None,
            avatar: Entity::DANGLING,
            heart: None,
            heart_asleep: false,
            heart_glow: 1.0,
            steps: Footsteps::default(),
            hud: Hud::default(),
            prompts: prompts::Prompts::default(),
            hit_sparks: None,
            block_sparks: None,
            ground_hit: None,
            guard_stance: 0,
            clangs: 0,
            dead_for: None,
            foe: None,
            locked_on: None,
            flick_wait: 0.0,
            dodge: None,
            roll_wanted: 0.0,
            winded: false,
            air: 1.0,
            stroke_in: 0.0,
            bubbles_in: 0.0,
            choke_in: 0.0,
            bubbles: None,
            burned_for: 0.0,
            trails: None,
            menu: Menu::default(),
            bag: Bag::default(),
            debug: debug::Debug::default(),
            spells: spells::Spells::default(),
            guns: guns::Guns::default(),
            leap: leap::Leap::default(),
            tale: tale::Tale::default(),
        }
    }

    fn spawn_player(&mut self, ctx: &Context) {
        let (position, yaw) = match (self.tale.respawn, ctx.world.player_spawn) {
            (Some((at, yaw)), _) => (ctx.world.origin().to_local(at), yaw),
            (None, Some(spawn)) => (spawn.position, spawn.yaw),
            (None, None) => {
                let p = ctx.camera.position;
                (Vec3::new(p.x, ctx.world.ground_height(p.x, p.z), p.z), ctx.camera.yaw)
            }
        };
        println!("[sandbox] player spawned at {position}");
        let mut player = ThirdPersonController::new(position + Vec3::Y * 0.1, yaw);
        player.walk_speed = WALK_SPEED;
        player.slow_speed = SLOW_SPEED;
        player.run_speed = RUN_SPEED;
        player.strafe_speed = STRAFE_SPEED;
        player.back_speed = BACK_SPEED;
        // Low and looking almost level, with the sky taking up half of the view.
        player.camera_pitch = -0.1;
        self.player = Some(player);
    }

    fn save(&mut self, ctx: &Context, slot: &str) {
        if self.player.is_none() {
            return;
        }
        if self.dead_for.is_some() {
            self.hud.notify(ctx, "Não é possível salvar agora", false);
            return;
        }
        let save = self.save_state(ctx);
        match ctx.save_game(slot, &save) {
            Ok(path) => {
                println!("[sandbox] saved to {}", path.display());
                self.hud.notify(ctx, "Jogo salvo", true);
            }
            Err(err) => {
                eprintln!("[sandbox] save failed: {err:#}");
                self.hud.notify(ctx, "Não foi possível salvar", false);
            }
        }
    }

    fn save_state(&self, ctx: &Context) -> SandboxSave {
        let player = self.player.as_ref().expect("there is someone to save");
        let health = ctx.world.get::<Health>(self.avatar).map(|health| health.current);
        let inventory = Some(self.bag.inventory.clone());
        SandboxSave {
            player: player.state(ctx.origin()),
            mode: self.mode,
            heart_asleep: self.heart_asleep,
            health,
            inventory,
            guns_empty: self.guns.empty.clone(),
            story: self.tale.story.clone(),
            body: Some(self.bag.body.clone()),
            worn: Vec::new(),
            respawn: self.tale.respawn,
        }
    }

    /// Returns whether the slot loaded; if not, a notice says so.
    fn load(&mut self, ctx: &mut Context, slot: &str) -> bool {
        match ctx.load_game::<SandboxSave>(slot) {
            Ok(save) => {
                if let Some(player) = &mut self.player {
                    player.restore(&save.player, ctx.origin());
                }
                self.mode = save.mode;
                self.heart_asleep = save.heart_asleep;
                self.bag.close();
                self.guns.empty = save.guns_empty;
                self.tale.story = save.story;
                self.tale.respawn = save.respawn;
                self.tale.panel = None;
                self.guns.reset = true;
                if let Some(mut inventory) = save.inventory {
                    inventory.settle(&ctx.world.items.clone(), bag_screen::COLUMNS, bag_screen::ROWS);
                    self.bag.inventory = inventory;
                }
                self.bag.body = save.body.unwrap_or_else(bag::body_grid);
                for id in save.worn {
                    self.bag.wear(&ctx.world.items.clone(), id);
                }
                self.stand_up(ctx);
                if let (Some(saved), Some(health)) = (save.health, ctx.world.get_mut::<Health>(self.avatar)) {
                    health.current = saved;
                }
                println!("[sandbox] loaded {slot:?}");
                self.hud.notify(ctx, "Jogo carregado", true);
                true
            }
            Err(err) => {
                eprintln!("[sandbox] load failed: {err:#}");
                self.hud.notify(ctx, "Nenhum jogo salvo para carregar", false);
                false
            }
        }
    }

    /// Shows a title when the player walks into a named area.
    fn check_triggers(&mut self, ctx: &mut Context, center: Vec3) {
        for event in ctx.world.update_triggers(center) {
            println!("[sandbox] {event:?}");
            let TriggerEvent::Enter(name) = event else { continue };
            let title = match name.as_str() {
                "circle" => "O Círculo de Pedras",
                "shrine" => "O Santuário Esquecido",
                _ => continue,
            };
            // Only the first time: after that, it's a place the character knows.
            let flag = format!("area:{name}");
            if !self.tale.story.flags.contains(&flag) {
                self.tale.story.set_flag(flag);
                self.hud.show_area(ctx, title);
            }
        }
    }

    /// Marks what can be used around the player and uses the chosen one when E is pressed.
    fn interact(&mut self, ctx: &mut Context, center: Vec3) {
        let Some(target) = self.prompts.update(ctx, center) else { return };
        if !ctx.input.action_just_pressed(actions::INTERACT) {
            return;
        }
        if ctx.world.toggle_door(target.entity).is_some() {
            return;
        }
        if let Some(key) = ctx.world.get::<Pickup>(target.entity).map(|p| p.key.clone()) {
            let player = self.player.as_ref().map_or(Vec3::ZERO, |p| p.body.position);
            if ctx.world.villagers_see_theft(&key, player, &ctx.physics) {
                self.hud.notify(ctx, "Viram você pegando o que não é seu.".to_string(), true);
            }
            self.take_pickup(ctx, target.entity);
            return;
        }
        if let Some(key) = ctx.world.container_key(target.entity) {
            let player = self.player.as_ref().map_or(Vec3::ZERO, |p| p.body.position);
            if ctx.world.villagers_see_theft(&key, player, &ctx.physics) {
                self.hud.notify(ctx, "Viram você mexendo no baú. A vila não gostou.".to_string(), true);
            }
            self.open_chest(key);
            return;
        }
        if self.talk_to_villager(ctx, target.entity) {
            return;
        }
        if let Some(name) = target.name.as_deref()
            && self.start_talk(ctx, name)
        {
            return;
        }
        match target.name.as_deref() {
            Some("campfire") => {
                self.tale.panel = Some(tale::Panel::Rest(rest::Resting::default()));
                ctx.ui.reset_focus();
            }
            Some("heart") => {
                self.heart_asleep = !self.heart_asleep;
                if !self.heart_asleep {
                    self.burst_heart(ctx);
                }
                let text = if self.heart_asleep { "O cristal adormece" } else { "O cristal desperta" };
                self.hud.notify(ctx, text, true);
            }
            Some("shrine_crystal") => self.hud.notify(ctx, "A pedra está fria ao toque", true),
            _ => {}
        }
    }

    /// Moves the characters and says something when one takes an interest.
    /// Animals notice someone moving fast from further away.
    fn update_fauna(&mut self, ctx: &mut Context, center: Vec3) {
        let speed = self.player.as_ref().map_or(0.0, |player| player.velocity().length());
        ctx.world.scare_fauna(center, FAUNA_NOTICE + speed * FAUNA_NOTICE_PER_SPEED);
        if self.dead_for.is_none()
            && let Some(player) = &self.player
        {
            ctx.world.touch_fauna(player.body.position, FAUNA_BODY_HEIGHT);
        }
        for event in ctx.world.fauna.take_events() {
            match event {
                FaunaEvent::Hurt { damage, staggers, at, message, .. } => {
                    self.on_animal_harm(ctx, damage, staggers, at, message.as_deref());
                }
                FaunaEvent::Killed { species, at } => {
                    self.tale.story.killed(&species);
                    if species != "bovino"
                        && let Some(effect) = &self.hit_sparks
                    {
                        ctx.world.play_effect(effect, at);
                    }
                }
                // A blow on a big animal lands like one on anybody: the thud, the sparks, a jolt.
                FaunaEvent::Struck { at, killed, .. } => {
                    ctx.audio.play_at(HIT_SOUND, 0.9, Emitter::at(at));
                    if let Some(effect) = &self.hit_sparks {
                        ctx.world.play_effect(effect, at);
                    }
                    if let Some(player) = &mut self.player {
                        player.shake(if killed { 0.06 } else { 0.035 });
                    }
                }
            }
        }
    }

    /// A blow makes someone flinch, and a burn only hurts, unless it's the last thing they feel.
    fn on_animal_harm(&mut self, ctx: &mut Context, damage: f32, staggers: bool, at: Vec3, message: Option<&str>) {
        if self.dead_for.is_some() {
            return;
        }
        let worn = !staggers && ctx.world.get_mut::<Health>(self.avatar).is_some_and(|health| health.wear(damage));
        let landed = worn || ctx.world.damage(self.avatar, damage, &mut ctx.physics).is_some();
        if !landed {
            return;
        }
        if worn {
            self.burned_for = BURN_SHOWN;
        }
        if let Some(player) = &mut self.player {
            player.shake(if staggers { 0.12 } else { 0.03 });
        }
        if staggers && let Some(effect) = &self.hit_sparks {
            ctx.world.play_effect(effect, at);
        }
        if ctx.world.get::<Health>(self.avatar).is_some_and(|health| health.is_dead()) {
            self.dead_for = Some(0.0);
            ctx.world.npcs_ignore_player = true;
        } else if let Some(message) = message {
            self.hud.notify(ctx, message, false);
        }
    }

    fn update_npcs(&mut self, ctx: &mut Context, center: Vec3) {
        let dt = ctx.time.delta();
        for event in ctx.world.update_npcs(&mut ctx.physics, center, dt) {
            println!("[sandbox] {event:?}");
            let NpcEvent::Noticed(entity) = event else { continue };
            let name = ctx.world.get::<Name>(entity).map(|name| name.0.clone());
            match name.as_deref() {
                Some("follower") => self.hud.notify(ctx, "Alguém começa a seguir você", true),
                Some("hostile") => {
                    self.foe = Some((entity, FOE_BAR_SECONDS));
                    self.hud.notify(ctx, "Ele não veio conversar", false);
                }
                _ => {}
            }
        }
    }

    fn moveset(&self, ctx: &Context) -> &'static Moveset {
        if self.bag.gun().is_some() {
            return &GUN;
        }
        moveset_of(&ctx.world, self.avatar)
    }

    /// Back on its feet, with full health and nobody after it for now.
    fn stand_up(&mut self, ctx: &mut Context) {
        self.dead_for = None;
        self.foe = None;
        self.locked_on = None;
        self.dodge = None;
        ctx.world.npcs_ignore_player = false;
        ctx.world.revive(self.avatar, &mut ctx.physics);
        let idle = self.moveset(ctx).idle;
        if let Some(animator) = ctx.world.get_mut::<Animator>(self.avatar) {
            animator.play(idle, 0.0);
        }
        if let Some(stamina) = ctx.world.get_mut::<Stamina>(self.avatar) {
            stamina.current = stamina.max;
        }
    }

    /// Q locks the camera on the enemy nearest to where it looks, or lets go; the
    /// lock lets go by itself if the enemy dies or gets far, and a flick of the
    /// mouse moves it to the next enemy on that side.
    fn update_lock(&mut self, ctx: &mut Context, dt: f32) {
        let Some(player) = self.player.as_mut() else { return };
        let from = player.body.position + Vec3::Y * player.camera_height;
        if self.mode != Mode::Play || self.dead_for.is_some() {
            self.locked_on = None;
        } else if ctx.input.action_just_pressed(actions::LOCK_ON) {
            self.locked_on = match self.locked_on {
                Some(_) => None,
                None => ctx.world.find_lock_target(&ctx.camera, from, PLAYER_TEAM, &ctx.physics),
            };
        }
        self.flick_wait = (self.flick_wait - dt).max(0.0);
        if let Some(current) = self.locked_on {
            let across = ctx.input.look_delta(dt).x;
            if !ctx.world.lock_holds(current, from) {
                self.locked_on = None;
            } else if self.flick_wait <= 0.0 && across.abs() >= LOCK_FLICK {
                self.flick_wait = LOCK_FLICK_SECONDS;
                let next = ctx.world.next_lock_target(current, &ctx.camera, from, PLAYER_TEAM, across, &ctx.physics);
                self.locked_on = next.or(self.locked_on);
            }
        }
        player.lock_target = self.locked_on.and_then(|target| ctx.world.lock_point(target));
    }

    /// Starts a roll if the player asks for one and can: in the direction held, or
    /// a step back if none is. Returns whether one is under way.
    fn update_dodge(&mut self, ctx: &mut Context, dt: f32) -> bool {
        let Some(player) = self.player.as_mut() else { return false };
        if ctx.input.action_just_pressed(actions::ROLL) {
            self.roll_wanted = ROLL_BUFFER;
        } else {
            self.roll_wanted = (self.roll_wanted - dt).max(0.0);
        }
        let holding = player.move_direction(&ctx.input);
        let Ok((fighter, health, animator, stamina)) =
            ctx.world.ecs.query_one_mut::<(&mut Fighter, &mut Health, &mut Animator, &mut Stamina)>(self.avatar)
        else {
            return false;
        };
        if let Some((clip, free)) = &mut self.dodge {
            *free |= animator.fired("cancel");
            let cut_short = *free && holding != Vec3::ZERO;
            if cut_short || animator.current() != Some(*clip) || animator.finished() {
                self.dodge = None;
            }
        }
        let free = match self.dodge {
            Some((_, free)) => free,
            None => !fighter.busy() || fighter.can_cancel(),
        };
        let can = self.mode == Mode::Play
            && self.dead_for.is_none()
            && player.body.grounded
            && !player.swimming()
            && !health.staggered();
        if self.roll_wanted > 0.0 && free && can {
            let rolling = holding != Vec3::ZERO;
            let (clip, cost, safe) = if rolling {
                ("sword_roll", ROLL_COST, ROLL_SAFE)
            } else {
                ("sword_backstep", BACKSTEP_COST, BACKSTEP_SAFE)
            };
            if stamina.spend(cost) {
                self.roll_wanted = 0.0;
                fighter.interrupt();
                health.make_untouchable(safe);
                if rolling {
                    player.turn_to(holding);
                    let pitch = 0.96 + 0.08 * (ctx.time.elapsed().fract() as f32);
                    ctx.audio.play_pitched(ROLL_SOUND, 0.8, pitch);
                    if self.bag.worn_in(grimora::scene::ArmorSlot::Body).is_some() {
                        ctx.audio.play_pitched(ROLL_ARMOR_SOUND, 0.8, pitch);
                    }
                } else if let Some(target) = player.lock_target {
                    player.turn_to(target - player.body.position);
                }
                animator.replay_once(clip, INTO_ROLL_FADE);
                animator.speed = 1.0;
                self.dodge = Some((clip, false));
            }
        }
        self.dodge.is_some()
    }

    /// Starts a blow if the player asks for one and can. Returns whether the
    /// character is busy fighting or on the ground, and can't walk.
    fn fight(&mut self, ctx: &mut Context, dodging: bool) -> bool {
        let Some(player) = self.player.as_mut() else { return false };
        let asks = ctx.input.action_just_pressed(actions::ATTACK);
        let can = self.mode == Mode::Play && self.dead_for.is_none() && player.body.grounded && !player.swimming();
        let holding = player.move_direction(&ctx.input);
        let Ok((fighter, health)) = ctx.world.ecs.query_one_mut::<(&mut Fighter, &Health)>(self.avatar) else {
            return false;
        };
        if asks && can && self.bag.armed() && !health.staggered() && !dodging {
            fighter.attack();
        }
        // Walking away cuts the recovery short; the next blow still follows on, for a moment.
        if fighter.can_cancel() && holding != Vec3::ZERO && !asks {
            fighter.walk_off();
        }
        fighter.busy() || health.staggered() || health.is_dead() || dodging
    }

    /// Holds the sword up for as long as the player asks and nothing else is going on, facing
    /// where the camera looks. The legs stay free: it plays over the upper body.
    fn update_guard(&mut self, ctx: &mut Context, fighting: bool) {
        ctx.world.player_holds_weapon(self.bag.armed() || self.bag.gun().is_some());
        // The book has its own poses, in `spells.rs`, and a gun in `guns.rs`.
        if self.bag.book().is_some() || self.bag.gun().is_some() || self.bag.gear.busy() {
            if let Some(guard) = ctx.world.get_mut::<Guard>(self.avatar) {
                guard.raised = false;
            }
            return;
        }
        let moveset = self.moveset(ctx);
        let Some(player) = self.player.as_mut() else { return };
        let asks = ctx.input.action(actions::GUARD)
            && self.bag.armed()
            && self.mode == Mode::Play
            && self.dead_for.is_none()
            && player.body.grounded
            && !player.swimming()
            && !fighting;
        let Ok((guard, animator)) = ctx.world.ecs.query_one_mut::<(&mut Guard, &mut Animator)>(self.avatar) else {
            return;
        };
        guard.raised = asks;
        player.aiming = asks;
        if !asks {
            self.guard_stance = 0;
            match moveset.carry {
                Some(carry) if !fighting && animator.current() != Some(moveset.idle) => {
                    animator.overlay_loop(carry, UPPER_BODY, GUARD_UP_FADE);
                }
                _ => animator.overlay_stop(GUARD_DOWN_FADE),
            }
            return;
        }
        let (stance, block) = moveset.guards[self.guard_stance % moveset.guards.len()];
        if animator.overlay_current() != Some(block) || animator.overlay_finished() {
            animator.overlay_loop(stance, UPPER_BODY, GUARD_UP_FADE);
        }
    }

    /// The third blow of a great sword reaches the ground: it shakes, and the earth flies.
    fn update_ground_hit(&mut self, ctx: &mut Context) {
        let struck = ctx.world.get::<Animator>(self.avatar).is_some_and(|animator| animator.fired("impact"));
        let Some(weapon) = ctx.world.joint_transform(self.avatar, "weapon.R").filter(|_| struck) else { return };
        let Some(player) = self.player.as_mut() else { return };
        player.shake(0.11);
        ctx.audio.play_pitched(Footsteps::SOUNDS[0], 1.0, 0.42);
        ctx.audio.play_pitched(CLANG_SOUND, 0.28, 0.5);
        let reach = ctx.world.get::<Fighter>(self.avatar).and_then(|f| f.weapon.blade.as_ref().map(|b| b.tip));
        let reach = reach.unwrap_or(1.0);
        let tip = weapon.position + weapon.rotation * Vec3::Y * reach;
        let ground = player.body.position.y + 0.02;
        if let Some(effect) = &self.ground_hit {
            ctx.world.play_effect(effect, Vec3::new(tip.x, tip.y.max(ground), tip.z));
        }
        ctx.world.strike_fauna(Vec3::new(tip.x, tip.y.max(ground), tip.z), GROUND_HIT_KILLS);
    }

    /// A blow rang on a guard. On the player's, the sword moves to meet the next one.
    fn on_blocked(&mut self, ctx: &mut Context, target: Entity, position: Vec3, direction: Vec3) {
        if let Some(effect) = &self.block_sparks {
            ctx.world.play_effect(effect, position);
        }
        if target != self.avatar {
            self.clangs = (self.clangs + 1) % CLANG_PITCHES.len();
            ctx.audio.play_at(CLANG_SOUNDS[self.clangs % CLANG_SOUNDS.len()], 0.9, Emitter::at(position));
            return;
        }
        let moveset = self.moveset(ctx);
        self.guard_stance = (self.guard_stance + 1) % moveset.guards.len();
        self.clangs = (self.clangs + 1) % CLANG_PITCHES.len();
        let clang = CLANG_SOUNDS[self.clangs % CLANG_SOUNDS.len()];
        ctx.audio.play_pitched(clang, 0.9, CLANG_PITCHES[self.clangs] * moveset.clang_pitch);
        if let Some(animator) = ctx.world.get_mut::<Animator>(self.avatar) {
            animator.overlay_once(moveset.guards[self.guard_stance].1, UPPER_BODY, BLOCK_FADE);
        }
        if let Some(player) = &mut self.player {
            player.push(direction * BLOCK_PUSH);
            player.shake(0.05);
        }
    }

    /// Lands blows, and shows and sounds what happened.
    fn update_combat(&mut self, ctx: &mut Context, dt: f32) {
        for event in ctx.world.update_combat(&mut ctx.physics, dt) {
            let (attacker, target, damage, position, direction, killed) = match event {
                CombatEvent::Swing { attacker } => {
                    // Each blow turns to the target, or where the player is pushing.
                    if attacker == self.avatar
                        && let Some(player) = self.player.as_mut()
                    {
                        let holding = player.move_direction(&ctx.input);
                        match player.lock_target {
                            Some(target) => player.turn_to(target - player.body.position),
                            None => player.turn_to(holding),
                        }
                        // A blow cuts down the generated plant right in front (stays cut in saves).
                        let at = player.body.position;
                        ctx.world.cut_generated_near(at, 2.2, &mut ctx.physics);
                    }
                    continue;
                }
                CombatEvent::Blocked { target, position, direction, .. } => {
                    self.on_blocked(ctx, target, position, direction);
                    continue;
                }
                CombatEvent::Hit { attacker, target, damage, position, direction, killed } => {
                    (attacker, target, damage, position, direction, killed)
                }
            };
            println!("[sandbox] hit for {damage}: {attacker:?} -> {target:?}, killed: {killed}");
            if attacker == self.avatar && ctx.world.get::<grimora::Villager>(target).is_some() {
                let player = self.player.as_ref().map_or(position, |p| p.body.position);
                ctx.world.villager_attacked(target, player, &mut ctx.physics);
            }
            if killed
                && attacker == self.avatar
                && let Some(name) = ctx.world.get::<Name>(target)
            {
                self.tale.story.set_flag(format!("defeated:{}", name.0));
            }
            ctx.audio.play_at(HIT_SOUND, 0.9, Emitter::at(position));
            if let Some(effect) = &self.hit_sparks {
                ctx.world.play_effect(effect, position);
            }
            if attacker == self.avatar
                && let Some(player) = &mut self.player
            {
                player.shake(0.035);
            }
            if target == self.avatar {
                if let Some(player) = &mut self.player {
                    player.push(direction * 3.0);
                    player.shake(0.07);
                }
                if killed {
                    self.dead_for = Some(0.0);
                    ctx.world.npcs_ignore_player = true;
                }
            } else if attacker == self.avatar && !killed && ctx.world.get::<Fighter>(target).is_some() {
                self.foe = Some((target, FOE_BAR_SECONDS));
            }
        }

        // The rest of the sounds come from the clips themselves.
        let mut sounds = Vec::new();
        let mut steps = Vec::new();
        for (entity, transform, animator) in ctx.world.ecs.query::<(Entity, &Transform, &Animator)>().iter() {
            let at = transform.position;
            for event in animator.events() {
                match event {
                    "swing" => sounds.push((SWING_SOUND, 0.6, Emitter::at(at))),
                    // The player's roll has its own recording, played when it starts.
                    "fall" if entity == self.avatar && animator.current() == Some("sword_roll") => {}
                    "fall" => sounds.push((STEP_SOUND, 0.9, Emitter::at(at))),
                    "step" if entity != self.avatar => steps.push(at),
                    _ => {}
                }
            }
        }
        for (sound, volume, at) in sounds {
            ctx.audio.play_at(sound, volume, at);
        }
        for (i, at) in steps.into_iter().enumerate() {
            let ground = ctx.world.ground_under(&ctx.physics, at);
            let take = self.steps.count + i * 7;
            let depth = Footsteps::wading(ctx, at);
            ctx.audio.play_at(Footsteps::of_npc(ground, take), 0.3 * Footsteps::under_water(depth), Emitter::quiet(at));
            if let Some((splash, loud, tone)) = Footsteps::splash(depth, take) {
                ctx.audio.play_pitched_at(splash, 0.3 * loud, tone, Emitter::quiet(at));
            }
        }
        // Doors starting to open or to shut, whoever moved them.
        for (at, opening) in ctx.world.take_door_sounds() {
            let sound = if opening { DOOR_OPEN_SOUND } else { DOOR_CLOSE_SOUND };
            ctx.audio.play_at(sound, 0.8, Emitter::at(at));
        }
        // Bodies hitting the ground, the trunk heavier than a limb.
        for thud in ctx.world.take_thuds() {
            let take = (thud.at.x.to_bits() ^ thud.at.z.to_bits().rotate_left(7)) as usize;
            let (sound, volume) = if thud.heavy {
                (BODY_FALL_SOUNDS[take % BODY_FALL_SOUNDS.len()], 1.0)
            } else {
                (LIMB_KNOCK_SOUNDS[take % LIMB_KNOCK_SOUNDS.len()], 0.6)
            };
            ctx.audio.play_at(sound, volume * thud.strength, Emitter::at(thud.at));
        }
    }

    /// The player's health, and that of the enemy in the fight.
    fn draw_health(&mut self, ctx: &mut Context) {
        self.ask_for_face(ctx);
        if self.hud.demo {
            self.hud.draw_demo(ctx);
            return;
        }
        let dt = ctx.time.delta();
        if let Some(health) = ctx.world.get::<Health>(self.avatar).map(|health| health.fraction()) {
            let stamina = ctx.world.get::<Stamina>(self.avatar).map(|stamina| stamina.fraction());
            let air = (self.air < 1.0).then_some(self.air);
            self.hud.draw_vitals(ctx, health, stamina, air);
        }
        self.burned_for = (self.burned_for - dt).max(0.0);
        let statuses = self.statuses(ctx);
        self.hud.draw_statuses(ctx, &statuses);
        if let Some(point) = self.locked_on.and_then(|target| ctx.world.lock_point(target)) {
            self.hud.draw_lock(ctx, point);
        }
        for (point, text) in ctx.world.villager_lines() {
            self.hud.draw_saying(ctx, point, &text);
        }
        let Some((foe, left)) = &mut self.foe else { return };
        *left -= dt;
        let (foe, left) = (*foe, *left);
        match ctx.world.get::<Health>(foe).map(|health| health.fraction()) {
            Some(fraction) if left > 0.0 && fraction > 0.0 => self.hud.draw_foe(ctx, "Errante corrompido", fraction),
            _ => self.foe = None,
        }
    }

    /// What's wrong with the character right now.
    fn statuses(&self, ctx: &Context) -> Vec<hud::Status> {
        let mut statuses = Vec::new();
        if self.winded {
            let name = "Sem fôlego";
            statuses.push(hud::Status { name, mark: bag_screen::MARK_BOLT, color: hud::WINDED_COLOR, left: None });
        }
        if let Some(frozen) = ctx.world.get::<Frozen>(self.avatar) {
            let left = Some(frozen.left / spells::FREEZE_SECONDS);
            statuses.push(hud::Status {
                name: "Congelado",
                mark: bag_screen::MARK_SNOW,
                color: hud::FROZEN_COLOR,
                left,
            });
        }
        if self.burned_for > 0.0 {
            let left = Some(self.burned_for / BURN_SHOWN);
            statuses.push(hud::Status {
                name: "Queimadura",
                mark: bag_screen::MARK_FLAME,
                color: hud::BURN_COLOR,
                left,
            });
        }
        statuses
    }

    /// Running spends stamina, and an empty bar leaves the character winded for a while.
    fn update_breath(&mut self, ctx: &mut Context, dt: f32) {
        let Some(player) = self.player.as_mut() else { return };
        let Some(stamina) = ctx.world.get_mut::<Stamina>(self.avatar) else { return };
        let speed = Vec2::new(player.velocity().x, player.velocity().z).length();
        if speed > (player.walk_speed + player.run_speed) * 0.5 && player.body.grounded && !self.winded {
            stamina.spend(RUN_COST * dt);
        }
        if stamina.current <= 0.0 {
            self.winded = true;
        } else if stamina.current >= WINDED_UNTIL {
            self.winded = false;
        }
        player.exhausted = self.winded;
    }

    /// Strokes, breath let out under the water, and what happens when it runs out.
    fn update_swim(&mut self, ctx: &mut Context, dt: f32) {
        let Some(player) = self.player.as_ref() else { return };
        self.stroke_in -= dt;
        if player.swimming() && player.velocity().length() > 0.8 && self.stroke_in <= 0.0 {
            self.stroke_in = STROKE_EVERY;
            ctx.audio.play(SWIM_SOUND, 0.4);
        }
        let under = player.head_under() && self.dead_for.is_none();
        let change = if under { -1.0 / AIR_SECONDS } else { 1.0 / AIR_BACK_SECONDS };
        self.air = (self.air + change * dt).clamp(0.0, 1.0);
        if !under {
            self.choke_in = 0.0;
            return;
        }
        self.bubbles_in -= dt;
        if self.bubbles_in <= 0.0 {
            self.bubbles_in = BUBBLES_EVERY * (0.25 + 0.75 * self.air);
            let mouth = player.body.position + Vec3::Y * 1.6;
            if let Some(effect) = &self.bubbles {
                ctx.world.play_effect(effect, mouth);
            }
        }
        if self.air > 0.0 {
            return;
        }
        self.choke_in -= dt;
        if self.choke_in <= 0.0 {
            self.choke_in = 1.0;
            if ctx.world.damage(self.avatar, DROWNING, &mut ctx.physics) == Some(true) {
                self.dead_for = Some(0.0);
                ctx.world.npcs_ignore_player = true;
                self.hud.notify(ctx, "Você se afogou", false);
            }
        }
    }

    /// The blade leaves a trail while it hurts.
    fn update_trails(&mut self, ctx: &mut Context) {
        let Some(trails) = self.trails else { return };
        let striking = ctx.world.get::<Fighter>(self.avatar).is_some_and(|fighter| fighter.striking());
        let Some(joint) = ctx.world.joint_transform(self.avatar, "weapon.R") else { return };
        let tip =
            ctx.world.get::<Fighter>(self.avatar).and_then(|f| f.weapon.blade.as_ref().map(|b| b.tip)).unwrap_or(0.8);
        for (trail, along) in trails.into_iter().zip([tip, tip * 0.64]) {
            let at = joint.position + joint.rotation * (Vec3::Y * along);
            if let Some(transform) = ctx.world.get_mut::<Transform>(trail) {
                transform.position = at;
            }
            if let Some(system) = ctx.world.get_mut::<ParticleSystem>(trail) {
                system.emitting = striking;
            }
        }
    }

    /// Puts a sword in the hands of the enemy with the clips for it, and readies the
    /// player's blade to leave a trail. What the player holds is up to `bag.rs`.
    fn give_swords(&mut self, ctx: &mut Context) {
        let catalog = ctx.world.items.clone();
        let hold = catalog.get(FIRST_WEAPON).and_then(|item| item.hold());
        if let (Some(hostile), Some(sword), Some(hold)) =
            (ctx.world.find("hostile"), ctx.world.item_mesh(FIRST_WEAPON), hold)
        {
            ctx.world.attach(hostile, "weapon.R", sword, hold);
        }
        // The sheet each blade sweeps while it cuts.
        let fighters: Vec<_> = [Some(self.avatar), ctx.world.find("hostile")].into_iter().flatten().collect();
        for (seed, fighter) in fighters.into_iter().enumerate() {
            let trail = grimora::core::BladeTrail::new(Default::default(), seed as u32 + 1);
            let _ = ctx.world.ecs.insert_one(fighter, trail);
        }
        self.trails = load_effect(BLADE_TRAIL).map_err(|err| eprintln!("[sandbox] {err:#}")).ok().map(|effect| {
            [0, 1].map(|seed| {
                let system = ParticleSystem::new(effect.clone(), Vec3::ZERO, 1.0, seed);
                ctx.world.ecs.spawn((Transform::IDENTITY, system))
            })
        });
    }

    fn burst_heart(&self, ctx: &mut Context) {
        let Some(heart) = &self.heart else { return };
        match load_effect(HEART_BURST) {
            Ok(effect) => {
                // From where it rests and not where it is now: the waves know where the floor is.
                ctx.world.play_effect(&effect, heart.rest + Vec3::Y * 0.6);
            }
            Err(err) => eprintln!("[sandbox] {err:#}"),
        }
    }

    /// The heart floats and turns, and fades to its glow; its light is a
    /// component of the same entity, so it floats along.
    fn animate_heart(&mut self, ctx: &mut Context) {
        let Some(heart) = &self.heart else { return };
        let t = ctx.time.world_elapsed();
        let target = if self.heart_asleep { ASLEEP_GLOW } else { 1.0 };
        self.heart_glow += (target - self.heart_glow) * (1.0 - (-3.0 * ctx.time.delta()).exp());
        let glow = self.heart_glow;
        let asleep = self.heart_asleep;
        if let Some(altar) = heart.altar
            && let Ok(circle) = ctx.world.ecs.query_one_mut::<&mut ParticleSystem>(altar)
        {
            circle.emitting = !asleep;
        }
        let query = ctx
            .world
            .ecs
            .query_one_mut::<(&mut Transform, &mut Renderable, &mut Light, Option<&mut ParticleSystem>)>(heart.entity);
        let Ok((transform, renderable, Light(light), motes)) = query else { return };
        if let Some(motes) = motes {
            motes.emitting = !asleep;
        }
        // It sinks and slows as it goes dark.
        transform.position = heart.rest + Vec3::Y * (0.25 * glow + 0.15 * glow * (t * 1.3).sin());
        transform.rotation = Quat::from_rotation_y(t * 0.4);
        renderable.emissive = heart.emissive * glow;
        light.intensity = heart.light * glow;
    }
}

impl Game for Sandbox {
    fn init(&mut self, ctx: &mut Context) {
        ctx.set_ui_follows_window(true);
        self.start_menu(ctx);
        if ctx.config().camera_position.is_some() || ctx.config().look_at.is_some() {
            self.mode = Mode::Fly;
        }
        if let Some(slot) = ctx.config().load.clone() {
            self.load(ctx, &slot);
        }
    }

    fn origin_shifted(&mut self, _ctx: &mut Context, offset: Vec3) {
        if let Some(player) = &mut self.player {
            player.shift(offset);
        }
        if let Some(heart) = &mut self.heart {
            heart.rest += offset;
        }
        self.spells.shift(offset);
        self.guns.shift(offset);
    }

    fn scene_loaded(&mut self, ctx: &mut Context) {
        self.load_book();
        self.tale.beasts.clear();
        self.tale.panel = None;
        self.spells.forget_world();
        self.guns.forget_world(&mut ctx.camera);
        self.heart = ctx.world.find("heart").and_then(|entity| {
            Some(Heart {
                entity,
                altar: ctx.world.find("altar"),
                rest: ctx.world.transform(entity)?.position,
                emissive: ctx.world.get::<Renderable>(entity)?.emissive,
                light: ctx.world.get::<Light>(entity)?.0.intensity,
            })
        });

        self.hit_sparks = load_effect(HIT_SPARKS).map_err(|err| eprintln!("[sandbox] {err:#}")).ok();
        self.block_sparks = load_effect(BLOCK_SPARKS).map_err(|err| eprintln!("[sandbox] {err:#}")).ok();
        self.ground_hit = load_effect(GROUND_HIT).map_err(|err| eprintln!("[sandbox] {err:#}")).ok();
        self.bubbles = load_effect(BUBBLES).map_err(|err| eprintln!("[sandbox] {err:#}")).ok();

        // A generated world's weather turns by itself, unless a capture or the command line
        // asked for one.
        let config = ctx.config();
        if ctx.world.auto_weather.is_none()
            && config.world.is_some()
            && config.screenshot.is_none()
            && config.weather.is_none()
            && config.time_of_day.is_none()
        {
            ctx.world.auto_weather = Some(grimora::AutoWeather::warm(0x5EED_C11A));
        }

        // Hot reloads keep the player where they are; only the first load spawns.
        if self.player.is_none() {
            self.spawn_player(ctx);
        }

        // The glTF model is rigged and animated; it gets an `Animator` on spawn.
        let mesh = grimora::scene::load_model(WANDERER.as_ref(), grimora::scene::DEFAULT_MAX_TEXTURE_SIZE)
            .unwrap_or_else(|err| {
                eprintln!("[sandbox] {err:#}; using the static wanderer");
                MeshDesc::Wanderer.build()
            });
        let wanderer = ctx.world.add_mesh(mesh);
        self.avatar = ctx.world.spawn(wanderer, Transform::IDENTITY);
        let components = (
            Health::new(PLAYER_HEALTH, PLAYER_TEAM),
            // Whatever is in the hand decides: see `bag.rs`.
            Fighter::new(Weapon::default()),
            Stamina::new(PLAYER_STAMINA),
            Guard { cost: BLOCK_COST, ..Default::default() },
        );
        let _ = ctx.world.ecs.insert(self.avatar, components);
        if let Some(animator) = ctx.world.get_mut::<Animator>(self.avatar) {
            animator.play("sword_idle", 0.0);
        }
        self.stock_bag(ctx);
        self.give_swords(ctx);
        self.dead_for = None;
        self.foe = None;
        self.locked_on = None;
        self.dodge = None;
        ctx.world.npcs_ignore_player = false;
    }

    fn update(&mut self, ctx: &mut Context) {
        // A generated world's ground is solid around the character, wherever the camera is.
        ctx.land_focus = self.player.as_ref().map(|p| p.body.center());
        if self.run_menu(ctx) {
            return;
        }
        // With the inventory open the world goes on, but the character takes no orders.
        self.debug_frame(ctx);
        let panel_open = self.run_panel(ctx);
        let bag_open = panel_open || self.run_bag(ctx);
        let wheel_open = self.run_wheel(ctx, bag_open);
        let input = (bag_open || wheel_open).then(|| std::mem::take(&mut ctx.input));
        // Nothing pressed, but the hints still know which key is which.
        if let Some(input) = &input {
            ctx.input.actions = input.actions.clone();
        }
        self.play(ctx, bag_open);
        if let Some(input) = input {
            ctx.input = input;
        }
        self.draw_wheel(ctx);
        self.draw_dark(ctx);
    }
}

impl Sandbox {
    /// A frame of the game itself.
    fn play(&mut self, ctx: &mut Context, bag_open: bool) {
        if ctx.input.action_just_pressed(TOGGLE_FLY) {
            self.mode = if self.mode == Mode::Play { Mode::Fly } else { Mode::Play };
        }
        // Sounds are near or far from the character, and to the left or right of the camera.
        let player = self.player.as_ref().filter(|_| self.mode == Mode::Play);
        ctx.audio.set_hearing_point(player.map(|player| player.body.position + Vec3::Y * HEAD_HEIGHT));
        if ctx.input.action_just_pressed(QUICK_SAVE) {
            self.save(ctx, QUICK_SLOT);
        }
        if ctx.input.action_just_pressed(QUICK_LOAD) {
            self.load(ctx, QUICK_SLOT);
        }
        if !bag_open {
            self.hud.draw(ctx, self.mode == Mode::Fly);
            self.draw_health(ctx);
        }
        self.aura_keys(ctx);
        if ctx.input.just_pressed(KeyCode::F4) {
            let at = SLOW_MOTION.iter().position(|&s| s == ctx.time.scale()).unwrap_or(0);
            let scale = SLOW_MOTION[(at + 1) % SLOW_MOTION.len()];
            ctx.time.set_scale(scale);
            self.hud.notify(ctx, format!("Velocidade do mundo: {:.0}%", scale * 100.0), true);
        }
        if ctx.input.just_pressed(KeyCode::F5)
            && let Some(weather) = ctx.world.weather()
        {
            let at = WEATHERS.iter().position(|(w, _)| *w == weather).unwrap_or(0);
            let (next, name) = WEATHERS[(at + 1) % WEATHERS.len()];
            ctx.world.change_weather(next, WEATHER_CHANGE_SECONDS);
            self.hud.notify(ctx, format!("Clima: {name}"), true);
        }
        let dt = ctx.time.world_delta();
        let automated = ctx.config().screenshot.is_some();

        if let Some(dead_for) = &mut self.dead_for {
            *dead_for += dt;
            if *dead_for > RESPAWN_SECONDS {
                self.spawn_player(ctx);
                self.stand_up(ctx);
                self.tale.waking = 1.0;
                self.hud.notify(ctx, "Você desperta de novo", true);
            }
        }

        self.update_lock(ctx, dt);
        let leaping = self.update_leap(ctx, dt);
        let dodging = !leaping && self.update_dodge(ctx, dt);
        let casting = self.update_spells(ctx, dodging || leaping, bag_open, dt);
        self.update_guns(ctx, dodging || leaping, bag_open, dt);
        self.update_gear(ctx, dodging || leaping);
        let fighting = (!leaping && self.fight(ctx, dodging)) || casting;
        self.update_guard(ctx, fighting);
        self.update_breath(ctx, dt);
        let root_motion = ctx.world.get::<Animator>(self.avatar).map_or(Vec3::ZERO, |animator| animator.root_delta());
        let Some(player) = self.player.as_mut() else { return };
        player.locked = fighting || self.leap.holds();
        // The leap moves the body itself: a roll out of it would push twice.
        let root_motion = if leaping { Vec3::ZERO } else { root_motion };
        player.root_motion = Quat::from_rotation_y(player.facing) * root_motion;
        player.water = ctx.world.water_around(player.body.position);
        let was_swimming = player.swimming();
        match self.mode {
            Mode::Play => {
                player.update(&ctx.input, &mut ctx.physics, &mut ctx.camera, dt);
                ctx.set_cursor_grab(!automated && !bag_open);
            }
            Mode::Fly => {
                let looking = self.fly.update(&mut ctx.camera, &ctx.input, dt);
                ctx.set_cursor_grab(looking);
                // The character stays under physics (and lands) while the camera flies.
                let mut unused = ctx.camera;
                player.update(&Input::default(), &mut ctx.physics, &mut unused, dt);
            }
        }
        if player.swimming() && !was_swimming {
            ctx.world.splash(player.body.position, -player.velocity().y / 10.0);
        }
        // Fell out of the world: back to the start.
        if player.body.position.y < -50.0 {
            self.spawn_player(ctx);
        }

        let Some(player) = self.player.as_ref() else { return };
        if let Some(transform) = ctx.world.get_mut::<Transform>(self.avatar) {
            *transform = player.transform();
        }
        let moveset = self.moveset(ctx);
        let mut step = None;
        if let Some(animator) = ctx.world.get_mut::<Animator>(self.avatar) {
            if !fighting && !leaping {
                // While the camera flies, the keys move the camera and not the character.
                let still = Input::default();
                animate(animator, moveset, player, if self.mode == Mode::Play { &ctx.input } else { &still });
            }
            if self.mode == Mode::Play {
                step = self.steps.update(animator, player, dt);
            }
        }
        if let Some(volume) = step {
            let foot = player.body.position;
            let ground = ctx.world.ground_under(&ctx.physics, foot);
            let shod = self.bag.worn_in(grimora::scene::ArmorSlot::Feet).is_some();
            let (sound, pitch) = self.steps.take(shod, ground);
            let depth = Footsteps::wading(ctx, foot);
            // In the water the bed is felt more than heard, under the splash.
            ctx.audio.play_pitched(sound, volume * Footsteps::under_water(depth), pitch);
            if let Some((splash, loud, tone)) = Footsteps::splash(depth, self.steps.count) {
                ctx.audio.play_pitched(splash, volume * loud, tone);
            }
        }

        let center = player.body.center();
        self.update_ground_hit(ctx);
        self.animate_heart(ctx);
        self.update_npcs(ctx, center);
        self.update_fauna(ctx, center);
        self.update_combat(ctx, dt);
        self.update_swim(ctx, dt);
        self.update_trails(ctx);
        self.check_triggers(ctx, center);
        if self.mode == Mode::Play && !bag_open {
            self.interact(ctx, center);
        }
    }
}

fn moveset_of(world: &World, avatar: Entity) -> &'static Moveset {
    world.get::<Fighter>(avatar).map_or(&LIGHT, |fighter| Moveset::of(&fighter.weapon))
}

/// Picks the wanderer's clip from how the character moves.
fn animate(animator: &mut Animator, moveset: &Moveset, player: &ThirdPersonController, input: &Input) {
    let velocity = player.velocity();
    let speed = Vec2::new(velocity.x, velocity.z).length();
    if player.swimming() {
        // Its own clips, if the character has them; otherwise the walk makes do as a kick.
        let moving = velocity.length() > 0.6;
        let (clip, fallback) = if moving { ("swim", "sword_walk") } else { ("swim_idle", moveset.idle) };
        if !animator.play(clip, LOCOMOTION_FADE) {
            animator.play(fallback, LOCOMOTION_FADE);
        }
        animator.speed = if moving { (velocity.length() / player.swim_speed).clamp(0.6, 1.8) } else { 1.0 };
        return;
    }
    if !player.body.grounded {
        // Hold the airborne pose on long falls; never loop back into take-off.
        animator.play_once(if velocity.y > 0.5 { "jump" } else { "jump_fall" }, 0.06);
        animator.speed = 1.0;
        return;
    }
    let (landing, landing_rate) = if speed < 0.4 {
        ("jump_land", 1.0)
    } else if speed < (player.walk_speed + player.run_speed) * 0.5 {
        ("jump_land_walk", (speed / SLOW_SPEED).max(0.2))
    } else {
        ("jump_land_run", speed / RUN_SPEED)
    };
    if matches!(animator.current(), Some("jump" | "jump_fall")) && animator.time() > 0.06 {
        // Ground contact, not the airborne clip's duration, triggers absorption.
        animator.replay_once(landing, 0.045);
        animator.speed = landing_rate;
        return;
    }
    if matches!(animator.current(), Some("jump_land" | "jump_land_walk" | "jump_land_run"))
        && !animator.finished()
        && (animator.current() == Some(landing) || animator.time() < 0.10)
    {
        animator.speed = match animator.current() {
            Some("jump_land_walk") => (speed / SLOW_SPEED).max(0.2),
            Some("jump_land_run") => (speed / RUN_SPEED).max(0.2),
            _ => 1.0,
        };
        return;
    }
    // Speeding up, the clip follows what the player asks for: the speed lags behind by a few
    // frames, long enough to flash through idle and the walk on the way out of a blow. Slowing
    // down, it follows the speed, so that the feet keep up with the body.
    let wish = player.move_direction(input);
    let top = if input.action(actions::RUN) && !player.exhausted {
        player.run_speed
    } else if player.slow_walking {
        player.slow_speed
    } else {
        player.walk_speed
    };
    let wanted = speed.max(wish.length() * top);
    // Which way it moves, as the character sees it.
    let local = Quat::from_rotation_y(-player.facing) * if wish == Vec3::ZERO { velocity } else { wish };
    let from = animator.current().unwrap_or_default();
    let walking = GAITS.contains(&from);
    let settled = walking || from == moveset.idle || from == moveset.guards[0].0 || from.starts_with("jump");
    let fade = if settled { LOCOMOTION_FADE } else { OUT_OF_BLOW_FADE };
    if wanted < 0.4 {
        // The guard stands in its own stance, which spreads the feet.
        animator.play(if player.aiming { moveset.guards[0].0 } else { moveset.idle }, fade);
        animator.speed = 1.0;
    } else if player.strafing(input) {
        // Facing one way, it walks in any direction: forwards or backwards, mixed with sideways.
        let (along, along_speed, along_cycle) = if local.z <= 0.0 && player.slow_walking {
            ("sword_walk", SLOW_SPEED, SLOW_CYCLE)
        } else if local.z <= 0.0 {
            ("sword_brisk", WALK_SPEED, WALK_CYCLE)
        } else {
            ("sword_back", BACK_SPEED, BACK_CYCLE)
        };
        let side = if local.x < 0.0 { "sword_strafe_l" } else { "sword_strafe_r" };
        let sideways = local.x.abs().atan2(local.z.abs()) / std::f32::consts::FRAC_PI_2;
        animator.play_mixed(along, side, sideways, fade);
        // Lengths of a cycle on the ground, which is what keeps the feet from sliding.
        let stride = (along_speed * along_cycle) * (1.0 - sideways) + (STRAFE_SPEED * STRAFE_CYCLE) * sideways;
        animator.speed = (speed.max(0.3) / stride * along_cycle).clamp(0.5, 2.2);
    } else if wanted < (player.walk_speed + player.run_speed) * 0.5 {
        // Clips are authored at the controller's walk and run speeds; scale the
        // playback so the feet don't slide while speeding up or slowing down.
        // The slow walk below halfway to the ordinary one, so letting go of Alt speeds the step up.
        if wanted < (player.slow_speed + player.walk_speed) * 0.5 {
            animator.play_in_step("sword_walk", fade);
            animator.speed = (speed / SLOW_SPEED).clamp(0.5, 1.5);
        } else {
            animator.play_in_step("sword_brisk", fade);
            animator.speed = (speed / WALK_SPEED).clamp(0.5, 1.5);
        }
    } else {
        animator.play_in_step("sword_run", fade);
        animator.speed = (speed / RUN_SPEED).clamp(0.5, 1.5);
    }
}

/// Plays a footstep whenever the walk or run clip fires `step`, and one on landing: a
/// recording of the ground underfoot, in boots or barefoot (`assets/sounds/steps`).
#[derive(Default)]
struct Footsteps {
    count: usize,
    /// The take played last, which the next step skips.
    last: usize,
    /// Seconds off the ground; only real jumps and falls thump on landing.
    air_time: f32,
}

impl Footsteps {
    const SOUNDS: [&str; 2] = [
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/step_1.wav"),
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/step_2.wav"),
    ];
    const FOLDER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/steps");
    /// Recordings of each ground, `passo_01.wav` and on.
    const TAKES: usize = 10;

    /// How loud the step that just landed is, if one did.
    fn update(&mut self, animator: &Animator, player: &ThirdPersonController, dt: f32) -> Option<f32> {
        let grounded = player.body.grounded;
        let landed = grounded && self.air_time > 0.25;
        self.air_time = if grounded { 0.0 } else { self.air_time + dt };
        if !(landed || (grounded && animator.fired("step"))) {
            return None;
        }
        let v = player.velocity();
        let speed = Vec2::new(v.x, v.z).length();
        Some(if landed { 0.8 } else { (0.35 + speed * 0.05).min(0.7) })
    }

    /// The next take on this ground, never the one just heard, with a touch of pitch.
    fn take(&mut self, shod: bool, ground: Ground) -> (String, f32) {
        self.count += 1;
        // Steps through the takes by a stride that is coprime with their count.
        let take = (self.last + 1 + (self.count * 3) % (Self::TAKES - 1)) % Self::TAKES;
        self.last = take;
        let feet = if shod { "bota" } else { "descalco" };
        let path = format!("{}/{feet}/{}/passo_{:02}.wav", Self::FOLDER, ground.folder(), take + 1);
        (path, [1.0, 0.97, 1.03][self.count % 3])
    }

    /// Recordings of a foot going into water.
    const SPLASHES: [&str; 2] = [
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/splash_1.wav"),
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/splash_2.wav"),
    ];
    /// Water shallower than this at the foot is only wet ground.
    const WADE_FROM: f32 = 0.03;

    /// How deep the calm water is over a foot at `foot`, if any is.
    fn wading(ctx: &Context, foot: Vec3) -> f32 {
        ctx.world.water_around(foot).map_or(0.0, |water| (water.level - foot.y).max(0.0))
    }

    /// How much of a step on the bed comes through the water over it.
    fn under_water(depth: f32) -> f32 {
        if depth < Self::WADE_FROM { 1.0 } else { (1.0 - depth * 1.6).max(0.3) }
    }

    /// The splash of a foot `depth` meters into water: louder and lower the deeper it goes.
    fn splash(depth: f32, take: usize) -> Option<(&'static str, f32, f32)> {
        if depth < Self::WADE_FROM {
            return None;
        }
        let loud = (0.35 + depth * 1.4).min(1.1);
        let tone = (1.25 - depth * 0.6).max(0.8) * [1.0, 0.94, 1.06][take % 3];
        Some((Self::SPLASHES[take % 2], loud, tone))
    }

    /// Where a step by someone else on this ground is. They all wear boots for now.
    fn of_npc(ground: Ground, take: usize) -> String {
        format!("{}/bota/{}/passo_{:02}.wav", Self::FOLDER, ground.folder(), take % Self::TAKES + 1)
    }
}

#[cfg(test)]
mod fauna_tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn start(name: &str) -> (Sandbox, Context, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("grimora_fauna_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let config = EngineConfig {
            scene: Some(DEFAULT_SCENE.replace("sandbox.ron", "sea.ron").into()),
            save_dir: Some(dir.clone()),
            load: Some("none".into()),
            escape_quits: false,
            ..Default::default()
        };
        let mut game = Sandbox::new();
        let mut ctx = Context::headless(config, &mut game).unwrap();
        for _ in 0..30 {
            ctx.headless_frame(&mut game, DT).unwrap();
        }
        ctx.world.npcs_ignore_player = true;
        (game, ctx, dir)
    }

    fn health(game: &Sandbox, ctx: &Context) -> f32 {
        ctx.world.get::<Health>(game.avatar).unwrap().current
    }

    #[test]
    fn swimming_into_a_jellyfish_burns_without_stopping_anyone() {
        let (mut game, mut ctx, dir) = start("sting");
        let before = health(&game, &ctx);
        for _ in 0..30 {
            let jelly = ctx.world.fauna.jelly_positions().next().unwrap();
            game.player.as_mut().unwrap().body.position = jelly - Vec3::Y * 0.9;
            ctx.headless_frame(&mut game, DT).unwrap();
        }
        let after = health(&game, &ctx);
        assert!(after < before, "it went through a jellyfish with {after} of {before}");
        assert!(!ctx.world.get::<Health>(game.avatar).unwrap().staggered());
        assert!(game.dead_for.is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn what_a_ray_does_stops_anyone_and_a_last_burn_kills() {
        let (mut game, mut ctx, dir) = start("ray");
        let before = health(&game, &ctx);
        let at = Vec3::new(0.0, 0.0, 0.0);
        game.on_animal_harm(&mut ctx, 5.0, true, at, Some("ferrão"));
        assert_eq!(health(&game, &ctx), before - 5.0);
        assert!(game.dead_for.is_none());

        assert!(game.statuses(&ctx).is_empty(), "a blow leaves no mark");
        game.on_animal_harm(&mut ctx, 3.0, false, at, None);
        assert!(game.statuses(&ctx).iter().any(|status| status.name == "Queimadura"), "a burn shows on the column");

        ctx.world.get_mut::<Health>(game.avatar).unwrap().current = 1.0;
        game.on_animal_harm(&mut ctx, 3.0, false, at, None);
        assert_eq!(health(&game, &ctx), 0.0);
        assert!(game.dead_for.is_some(), "that burn was the end of it");
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod locomotion_tests {
    use super::*;

    #[test]
    fn movesets_name_clips_the_model_has() {
        let mesh = grimora::scene::load_model(WANDERER.as_ref(), grimora::scene::DEFAULT_MAX_TEXTURE_SIZE).unwrap();
        let skeleton = mesh.skeleton.unwrap();
        for moveset in [&LIGHT, &HEAVY, &KATANA, &GUN] {
            let guards = moveset.guards.iter().flat_map(|(stance, block)| [*stance, *block]);
            for clip in guards.chain([moveset.idle]).chain(moveset.carry) {
                assert!(skeleton.clip(clip).is_some(), "no clip {clip}");
            }
        }
        let weapon = |clip: &str| Weapon { clip: clip.into(), ..Default::default() };
        assert_eq!(Moveset::of(&weapon("heavy_1")).idle, "heavy_idle");
        assert_eq!(Moveset::of(&weapon("sword_1")).idle, "sword_idle");
        assert_eq!(Moveset::of(&weapon("katana_1")).idle, "katana_idle");
    }

    #[test]
    fn jump_lands_on_contact_and_recovers_at_each_movement_speed() {
        let mesh = grimora::scene::load_model(WANDERER.as_ref(), grimora::scene::DEFAULT_MAX_TEXTURE_SIZE).unwrap();
        let skeleton = mesh.skeleton.unwrap();
        for (speed, landing, recovered) in [
            (0.0, "jump_land", "sword_idle"),
            (WALK_SPEED, "jump_land_walk", "sword_brisk"),
            (RUN_SPEED, "jump_land_run", "sword_run"),
        ] {
            let mut player = ThirdPersonController::new(Vec3::ZERO, 0.0);
            player.walk_speed = WALK_SPEED;
            player.run_speed = RUN_SPEED;
            player.body.grounded = false;
            player.push(Vec3::new(0.0, 7.0, -speed));
            let mut animator = Animator::new(skeleton.clone());
            let input = Input::default();
            animate(&mut animator, &LIGHT, &player, &input);
            assert_eq!(animator.current(), Some("jump"));
            animator.update(0.2);
            player.push(Vec3::new(0.0, -12.0, 0.0));
            animate(&mut animator, &LIGHT, &player, &input);
            assert_eq!(animator.current(), Some("jump_fall"));
            animator.update(1.0);
            animate(&mut animator, &LIGHT, &player, &input);
            assert!(animator.finished(), "a long fall must hold, not replay take-off");
            player.body.grounded = true;
            animate(&mut animator, &LIGHT, &player, &input);
            assert_eq!(animator.current(), Some(landing));
            animator.update(0.07);
            animate(&mut animator, &LIGHT, &player, &input);
            assert_eq!(animator.current(), Some(landing), "keep the impact absorption");
            animator.update(2.0);
            animate(&mut animator, &LIGHT, &player, &input);
            assert_eq!(animator.current(), Some(recovered));
        }
    }
}

fn main() -> anyhow::Result<()> {
    let mut config = EngineConfig {
        title: "Grimora Sandbox".into(),
        scene: Some(DEFAULT_SCENE.into()),
        escape_quits: false,
        ..Default::default()
    }
    .with_cli()?;
    // A generated world brings its own ground: the default scene is only its sky and sea.
    if config.world.is_some() && config.scene.as_deref() == Some(std::path::Path::new(DEFAULT_SCENE)) {
        config.scene = Some(DEFAULT_SCENE.replace("sandbox.ron", "world.ron").into());
    }
    menu::apply_startup_options(&mut config);
    grimora::run(config, Sandbox::new())
}
