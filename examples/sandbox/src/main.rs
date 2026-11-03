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
//! F4 runs the world in slow motion, for looking at the animations.
//! It opens on a title screen and Esc pauses (`menu.rs`); automated runs skip the title.
//! The hours pass in the scene file's `day_cycle`; `--time <hour>` stops them there.

mod hud;
mod menu;

use grimora::prelude::*;
use hud::Hud;
use menu::Menu;
use serde::{Deserialize, Serialize};

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
const CLANG_SOUND: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/clang.wav");
const STEP_SOUND: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/step_1.wav");
const SWORD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/models/weapons/espada_reta.glb");
/// The pale smear behind the blade while it hurts.
const BLADE_TRAIL: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/effects/blade_trail.ron");
/// From the `weapon.R` joint to the sword: the file's grip is at its origin and its
/// blade along Y, 3.6 units long; its edges go up and down at rest.
const SWORD_SCALE: f32 = 0.263;

/// Native chartest walk (tools/author_chartest_walk.py); other gaits use sword_moves.py.
const WALK_SPEED: f32 = 1.2;
const RUN_SPEED: f32 = 5.5;
const STRAFE_SPEED: f32 = 0.8;
const BACK_SPEED: f32 = 0.95;
/// Seconds a cycle of each gait takes, as they were written, at the speeds above.
const WALK_CYCLE: f32 = 1.0;
const STRAFE_CYCLE: f32 = 0.66;
const BACK_CYCLE: f32 = 0.9;
/// Clips with a walking rhythm: going from one to another keeps the step.
const GAITS: [&str; 5] = ["sword_walk", "sword_run", "sword_back", "sword_strafe_l", "sword_strafe_r"];
const LOCOMOTION_FADE: f32 = 0.2;
/// A blow leaves the body further from where walking has it, and it needs longer to get there.
const OUT_OF_BLOW_FADE: f32 = 0.3;
const INTO_ROLL_FADE: f32 = 0.1;
/// The guard's stances, held, and the blows that end in each one. Every blow moves it to the next.
const GUARD_STANCES: [(&str, &str); 3] =
    [("sword_guard_1", "sword_block_1"), ("sword_guard_2", "sword_block_2"), ("sword_guard_3", "sword_block_3")];
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
}

/// The "heart" crystal on the altar, as the scene file left it.
struct Heart {
    entity: Entity,
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
    /// Carries the lantern's light; follows the `lantern` joint.
    lantern: Entity,
    heart: Option<Heart>,
    heart_asleep: bool,
    /// Current brightness of the heart, easing towards awake (1) or asleep.
    heart_glow: f32,
    steps: Footsteps,
    hud: Hud,
    hit_sparks: Option<ParticleEffect>,
    block_sparks: Option<ParticleEffect>,
    /// Which of `GUARD_STANCES` the guard is in.
    guard_stance: usize,
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
    /// Two points along the player's blade that leave a trail.
    trails: Option<[Entity; 2]>,
    menu: Menu,
}

impl Sandbox {
    fn new() -> Self {
        Self {
            mode: Mode::Play,
            fly: FlyCamera::default(),
            player: None,
            avatar: Entity::DANGLING,
            lantern: Entity::DANGLING,
            heart: None,
            heart_asleep: false,
            heart_glow: 1.0,
            steps: Footsteps::default(),
            hud: Hud::default(),
            hit_sparks: None,
            block_sparks: None,
            guard_stance: 0,
            dead_for: None,
            foe: None,
            locked_on: None,
            flick_wait: 0.0,
            dodge: None,
            roll_wanted: 0.0,
            winded: false,
            trails: None,
            menu: Menu::default(),
        }
    }

    fn spawn_player(&mut self, ctx: &Context) {
        let (position, yaw) = match ctx.world.player_spawn {
            Some(spawn) => (spawn.position, spawn.yaw),
            None => {
                let p = ctx.camera.position;
                (Vec3::new(p.x, ctx.world.ground_height(p.x, p.z), p.z), ctx.camera.yaw)
            }
        };
        println!("[sandbox] player spawned at {position}");
        let mut player = ThirdPersonController::new(position + Vec3::Y * 0.1, yaw);
        player.walk_speed = WALK_SPEED;
        player.run_speed = RUN_SPEED;
        player.strafe_speed = STRAFE_SPEED;
        player.back_speed = BACK_SPEED;
        self.player = Some(player);
    }

    fn save(&mut self, ctx: &Context, slot: &str) {
        let Some(player) = &self.player else { return };
        if self.dead_for.is_some() {
            self.hud.notify(ctx, "Não é possível salvar agora", false);
            return;
        }
        let health = ctx.world.get::<Health>(self.avatar).map(|health| health.current);
        let save = SandboxSave { player: player.state(), mode: self.mode, heart_asleep: self.heart_asleep, health };
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

    /// Returns whether the slot loaded; if not, a notice says so.
    fn load(&mut self, ctx: &mut Context, slot: &str) -> bool {
        match ctx.load_game::<SandboxSave>(slot) {
            Ok(save) => {
                if let Some(player) = &mut self.player {
                    player.restore(&save.player);
                }
                self.mode = save.mode;
                self.heart_asleep = save.heart_asleep;
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
            match name.as_str() {
                "circle" => self.hud.notify(ctx, "O círculo de pedras", true),
                "shrine" => self.hud.notify(ctx, "Um santuário esquecido", true),
                _ => {}
            }
        }
    }

    /// Shows the prompt of the object in reach and uses it when E is pressed.
    fn interact(&mut self, ctx: &mut Context, center: Vec3) {
        let Some(target) = ctx.world.interaction_target(center, ctx.camera.forward(), &ctx.physics) else { return };
        self.hud.draw_prompt(ctx, &target.prompt);
        if !ctx.input.action_just_pressed(actions::INTERACT) {
            return;
        }
        match target.name.as_deref() {
            Some("heart") => {
                self.heart_asleep = !self.heart_asleep;
                if !self.heart_asleep {
                    self.burst_heart(ctx);
                }
                let text = if self.heart_asleep { "O cristal adormece" } else { "O cristal desperta" };
                self.hud.notify(ctx, text, true);
            }
            Some("shrine_crystal") => self.hud.notify(ctx, "A pedra está fria ao toque", true),
            Some("watcher") => self.hud.notify(ctx, "O cristal só acorda para quem o toca", true),
            _ => {}
        }
    }

    /// Moves the characters and says something when one takes an interest.
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

    /// Back on its feet, with full health and nobody after it for now.
    fn stand_up(&mut self, ctx: &mut Context) {
        self.dead_for = None;
        self.foe = None;
        self.locked_on = None;
        self.dodge = None;
        ctx.world.npcs_ignore_player = false;
        ctx.world.revive(self.avatar, &mut ctx.physics);
        if let Some(animator) = ctx.world.get_mut::<Animator>(self.avatar) {
            animator.play("sword_idle", 0.0);
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
        let can = self.mode == Mode::Play && self.dead_for.is_none() && player.body.grounded && !health.staggered();
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
        let can = self.mode == Mode::Play && self.dead_for.is_none() && player.body.grounded;
        let holding = player.move_direction(&ctx.input);
        let Ok((fighter, health)) = ctx.world.ecs.query_one_mut::<(&mut Fighter, &Health)>(self.avatar) else {
            return false;
        };
        if asks && can && !health.staggered() && !dodging {
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
        let Some(player) = self.player.as_mut() else { return };
        let asks = ctx.input.action(actions::GUARD)
            && self.mode == Mode::Play
            && self.dead_for.is_none()
            && player.body.grounded
            && !fighting;
        let Ok((guard, animator)) = ctx.world.ecs.query_one_mut::<(&mut Guard, &mut Animator)>(self.avatar) else {
            return;
        };
        guard.raised = asks;
        player.aiming = asks;
        if !asks {
            self.guard_stance = 0;
            animator.overlay_stop(GUARD_DOWN_FADE);
            return;
        }
        let (stance, block) = GUARD_STANCES[self.guard_stance];
        if animator.overlay_current() != Some(block) || animator.overlay_finished() {
            animator.overlay_loop(stance, UPPER_BODY, GUARD_UP_FADE);
        }
    }

    /// A blow rang on a guard. On the player's, the sword moves to meet the next one.
    fn on_blocked(&mut self, ctx: &mut Context, target: Entity, position: Vec3, direction: Vec3) {
        if let Some(effect) = &self.block_sparks {
            ctx.world.play_effect(effect, position);
        }
        if target != self.avatar {
            ctx.audio.play_at(CLANG_SOUND, 0.9, Emitter::at(position));
            return;
        }
        self.guard_stance = (self.guard_stance + 1) % GUARD_STANCES.len();
        ctx.audio.play_pitched(CLANG_SOUND, 0.9, CLANG_PITCHES[self.guard_stance]);
        if let Some(animator) = ctx.world.get_mut::<Animator>(self.avatar) {
            animator.overlay_once(GUARD_STANCES[self.guard_stance].1, UPPER_BODY, BLOCK_FADE);
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
                    self.hud.notify(ctx, "Você caiu", false);
                }
            } else if attacker == self.avatar && !killed && ctx.world.get::<Fighter>(target).is_some() {
                self.foe = Some((target, FOE_BAR_SECONDS));
            }
        }

        // The rest of the sounds come from the clips themselves.
        let mut sounds = Vec::new();
        for (entity, transform, animator) in ctx.world.ecs.query::<(Entity, &Transform, &Animator)>().iter() {
            let at = Emitter::at(transform.position);
            for event in animator.events() {
                match event {
                    "swing" => sounds.push((SWING_SOUND, 0.6, at)),
                    "fall" => sounds.push((STEP_SOUND, 0.9, at)),
                    "step" if entity != self.avatar => sounds.push((STEP_SOUND, 0.25, at)),
                    _ => {}
                }
            }
        }
        for (sound, volume, at) in sounds {
            ctx.audio.play_at(sound, volume, at);
        }
    }

    /// The player's health, and that of the enemy in the fight.
    fn draw_health(&mut self, ctx: &mut Context) {
        let dt = ctx.time.delta();
        if let Some(fraction) = ctx.world.get::<Health>(self.avatar).map(|health| health.fraction()) {
            self.hud.draw_health(ctx, fraction);
        }
        if let Some(fraction) = ctx.world.get::<Stamina>(self.avatar).map(|stamina| stamina.fraction()) {
            self.hud.draw_stamina(ctx, fraction);
        }
        if let Some(point) = self.locked_on.and_then(|target| ctx.world.lock_point(target)) {
            self.hud.draw_lock(ctx, point);
        }
        let Some((foe, left)) = &mut self.foe else { return };
        *left -= dt;
        let (foe, left) = (*foe, *left);
        match ctx.world.get::<Health>(foe).map(|health| health.fraction()) {
            Some(fraction) if left > 0.0 && fraction > 0.0 => self.hud.draw_foe(ctx, "Errante corrompido", fraction),
            _ => self.foe = None,
        }
    }

    /// Running spends stamina, and an empty bar leaves the character winded for a while.
    fn update_breath(&mut self, ctx: &mut Context, dt: f32) {
        let Some(player) = self.player.as_mut() else { return };
        let Some(stamina) = ctx.world.get_mut::<Stamina>(self.avatar) else { return };
        let speed = Vec2::new(player.velocity().x, player.velocity().z).length();
        if speed > (player.walk_speed + player.run_speed) * 0.5 && !self.winded {
            stamina.spend(RUN_COST * dt);
        }
        if stamina.current <= 0.0 {
            self.winded = true;
        } else if stamina.current >= WINDED_UNTIL {
            self.winded = false;
        }
        player.exhausted = self.winded;
    }

    /// The blade leaves a trail while it hurts.
    fn update_trails(&mut self, ctx: &mut Context) {
        let Some(trails) = self.trails else { return };
        let striking = ctx.world.get::<Fighter>(self.avatar).is_some_and(|fighter| fighter.striking());
        let Some(joint) = ctx.world.joint_transform(self.avatar, "weapon.R") else { return };
        for (trail, along) in trails.into_iter().zip([0.78, 0.5]) {
            let at = joint.position + joint.rotation * (Vec3::Y * along);
            if let Some(transform) = ctx.world.get_mut::<Transform>(trail) {
                transform.position = at;
            }
            if let Some(system) = ctx.world.get_mut::<ParticleSystem>(trail) {
                system.emitting = striking;
            }
        }
    }

    /// Puts the sword in the hands of the player and of the enemy with the clips for it.
    fn give_swords(&mut self, ctx: &mut Context) {
        let sword = match grimora::scene::load_model(SWORD.as_ref(), grimora::scene::DEFAULT_MAX_TEXTURE_SIZE) {
            Ok(mesh) => ctx.world.add_mesh(mesh),
            Err(err) => {
                eprintln!("[sandbox] {err:#}");
                return;
            }
        };
        let hold = Transform {
            position: Vec3::ZERO,
            rotation: Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2),
            scale: Vec3::splat(SWORD_SCALE),
        };
        ctx.world.attach(self.avatar, "weapon.R", sword, hold);
        if let Some(hostile) = ctx.world.find("hostile") {
            ctx.world.attach(hostile, "weapon.R", sword, hold);
        }
        self.trails = load_effect(BLADE_TRAIL).map_err(|err| eprintln!("[sandbox] {err:#}")).ok().map(|effect| {
            [0, 1].map(|seed| {
                let system = ParticleSystem::new(effect.clone(), Vec3::ZERO, 1.0, seed);
                ctx.world.ecs.spawn((Transform::IDENTITY, system))
            })
        });
    }

    fn burst_heart(&self, ctx: &mut Context) {
        let Some(at) = self.heart.as_ref().and_then(|h| ctx.world.transform(h.entity)) else { return };
        match load_effect(HEART_BURST) {
            Ok(effect) => {
                ctx.world.play_effect(&effect, at.position + Vec3::Y * 0.6);
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
        self.start_menu(ctx);
        if ctx.config().camera_position.is_some() || ctx.config().look_at.is_some() {
            self.mode = Mode::Fly;
        }
        if let Some(slot) = ctx.config().load.clone() {
            self.load(ctx, &slot);
        }
    }

    fn scene_loaded(&mut self, ctx: &mut Context) {
        self.heart = ctx.world.find("heart").and_then(|entity| {
            Some(Heart {
                entity,
                rest: ctx.world.transform(entity)?.position,
                emissive: ctx.world.get::<Renderable>(entity)?.emissive,
                light: ctx.world.get::<Light>(entity)?.0.intensity,
            })
        });

        self.hit_sparks = load_effect(HIT_SPARKS).map_err(|err| eprintln!("[sandbox] {err:#}")).ok();
        self.block_sparks = load_effect(BLOCK_SPARKS).map_err(|err| eprintln!("[sandbox] {err:#}")).ok();

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
        let weapon = Weapon {
            damage: 10.0,
            reach: 1.9,
            cooldown: 0.0,
            clip: "sword_1".into(),
            combo: vec!["sword_2".into(), "sword_3".into()],
            blade: Some(Blade::default()),
            stamina: 16.0,
            hit_stop: 0.075,
            ..Default::default()
        };
        let components = (
            Health::new(PLAYER_HEALTH, PLAYER_TEAM),
            Fighter::new(weapon),
            Stamina::new(PLAYER_STAMINA),
            Guard { cost: BLOCK_COST, ..Default::default() },
        );
        let _ = ctx.world.ecs.insert(self.avatar, components);
        if let Some(animator) = ctx.world.get_mut::<Animator>(self.avatar) {
            animator.play("sword_idle", 0.0);
        }
        self.give_swords(ctx);
        self.dead_for = None;
        self.foe = None;
        self.locked_on = None;
        self.dodge = None;
        ctx.world.npcs_ignore_player = false;
        let light = PointLight {
            position: Vec3::new(0.0, -0.1, 0.0),
            color: Vec3::new(1.0, 0.68, 0.38),
            intensity: 7.0,
            radius: 9.0,
            flicker: 0.6,
            night_only: false,
        };
        self.lantern = ctx.world.ecs.spawn((Transform::IDENTITY, Light(light)));
    }

    fn update(&mut self, ctx: &mut Context) {
        if self.run_menu(ctx) {
            return;
        }
        if ctx.input.action_just_pressed(TOGGLE_FLY) {
            self.mode = if self.mode == Mode::Play { Mode::Fly } else { Mode::Play };
        }
        if ctx.input.action_just_pressed(QUICK_SAVE) {
            self.save(ctx, QUICK_SLOT);
        }
        if ctx.input.action_just_pressed(QUICK_LOAD) {
            self.load(ctx, QUICK_SLOT);
        }
        self.hud.draw(ctx, self.mode == Mode::Fly);
        self.draw_health(ctx);
        if ctx.input.just_pressed(KeyCode::F4) {
            let at = SLOW_MOTION.iter().position(|&s| s == ctx.time.scale()).unwrap_or(0);
            let scale = SLOW_MOTION[(at + 1) % SLOW_MOTION.len()];
            ctx.time.set_scale(scale);
            self.hud.notify(ctx, format!("Velocidade do mundo: {:.0}%", scale * 100.0), true);
        }
        let dt = ctx.time.world_delta();
        let automated = ctx.config().screenshot.is_some();

        if let Some(dead_for) = &mut self.dead_for {
            *dead_for += dt;
            if *dead_for > RESPAWN_SECONDS {
                self.spawn_player(ctx);
                self.stand_up(ctx);
                self.hud.notify(ctx, "Você desperta de novo", true);
            }
        }

        self.update_lock(ctx, dt);
        let dodging = self.update_dodge(ctx, dt);
        let fighting = self.fight(ctx, dodging);
        self.update_guard(ctx, fighting);
        self.update_breath(ctx, dt);
        let root_motion = ctx.world.get::<Animator>(self.avatar).map_or(Vec3::ZERO, |animator| animator.root_delta());
        let Some(player) = self.player.as_mut() else { return };
        player.locked = fighting;
        player.root_motion = Quat::from_rotation_y(player.facing) * root_motion;
        match self.mode {
            Mode::Play => {
                player.update(&ctx.input, &mut ctx.physics, &mut ctx.camera, dt);
                ctx.set_cursor_grab(!automated);
            }
            Mode::Fly => {
                let looking = self.fly.update(&mut ctx.camera, &ctx.input, dt);
                ctx.set_cursor_grab(looking);
                // The character stays under physics (and lands) while the camera flies.
                let mut unused = ctx.camera;
                player.update(&Input::default(), &mut ctx.physics, &mut unused, dt);
            }
        }
        // Fell out of the world: back to the start.
        if player.body.position.y < -50.0 {
            self.spawn_player(ctx);
        }

        let Some(player) = self.player.as_ref() else { return };
        if let Some(transform) = ctx.world.get_mut::<Transform>(self.avatar) {
            *transform = player.transform();
        }
        if let Some(animator) = ctx.world.get_mut::<Animator>(self.avatar) {
            if !fighting {
                // While the camera flies, the keys move the camera and not the character.
                let still = Input::default();
                animate(animator, player, if self.mode == Mode::Play { &ctx.input } else { &still });
            }
            if self.mode == Mode::Play {
                self.steps.update(&mut ctx.audio, animator, player, dt);
            }
        }

        // The light hangs from the lantern joint, so it swings with the walk.
        if let Some(joint) = ctx.world.joint_transform(self.avatar, "lantern")
            && let Some(lantern) = ctx.world.get_mut::<Transform>(self.lantern)
        {
            *lantern = joint;
        }

        let center = player.body.center();
        self.animate_heart(ctx);
        self.update_npcs(ctx, center);
        self.update_combat(ctx, dt);
        self.update_trails(ctx);
        self.check_triggers(ctx, center);
        if self.mode == Mode::Play {
            self.interact(ctx, center);
        }
    }
}

/// Picks the wanderer's clip from how the character moves.
fn animate(animator: &mut Animator, player: &ThirdPersonController, input: &Input) {
    let velocity = player.velocity();
    let speed = Vec2::new(velocity.x, velocity.z).length();
    if !player.body.grounded {
        // Hold the airborne pose on long falls; never loop back into take-off.
        animator.play_once(if velocity.y > 0.5 { "jump" } else { "jump_fall" }, 0.06);
        animator.speed = 1.0;
        return;
    }
    let (landing, landing_rate) = if speed < 0.4 {
        ("jump_land", 1.0)
    } else if speed < (player.walk_speed + player.run_speed) * 0.5 {
        ("jump_land_walk", (speed / WALK_SPEED).max(0.2))
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
            Some("jump_land_walk") => (speed / WALK_SPEED).max(0.2),
            Some("jump_land_run") => (speed / RUN_SPEED).max(0.2),
            _ => 1.0,
        };
        return;
    }
    // Speeding up, the clip follows what the player asks for: the speed lags behind by a few
    // frames, long enough to flash through idle and the walk on the way out of a blow. Slowing
    // down, it follows the speed, so that the feet keep up with the body.
    let wish = player.move_direction(input);
    let top = if input.action(actions::RUN) && !player.exhausted { player.run_speed } else { player.walk_speed };
    let wanted = speed.max(wish.length() * top);
    // Which way it moves, as the character sees it.
    let local = Quat::from_rotation_y(-player.facing) * if wish == Vec3::ZERO { velocity } else { wish };
    let from = animator.current().unwrap_or_default();
    let walking = GAITS.contains(&from);
    let settled = walking || from == "sword_idle" || from == GUARD_STANCES[0].0 || from.starts_with("jump");
    let fade = if settled { LOCOMOTION_FADE } else { OUT_OF_BLOW_FADE };
    if wanted < 0.4 {
        // The guard stands in its own stance, which spreads the feet.
        animator.play(if player.aiming { GUARD_STANCES[0].0 } else { "sword_idle" }, fade);
        animator.speed = 1.0;
    } else if player.strafing(input) {
        // Facing one way, it walks in any direction: forwards or backwards, mixed with sideways.
        let (along, along_speed, along_cycle) = if local.z <= 0.0 {
            ("sword_walk", WALK_SPEED, WALK_CYCLE)
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
        animator.play_in_step("sword_walk", fade);
        animator.speed = (speed / WALK_SPEED).clamp(0.5, 1.5);
    } else {
        animator.play_in_step("sword_run", fade);
        animator.speed = (speed / RUN_SPEED).clamp(0.5, 1.5);
    }
}

/// Plays a footstep whenever the walk or run clip fires `step`, and one on landing.
#[derive(Default)]
struct Footsteps {
    count: usize,
    /// Seconds off the ground; only real jumps and falls thump on landing.
    air_time: f32,
}

impl Footsteps {
    const SOUNDS: [&str; 2] = [
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/step_1.wav"),
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/step_2.wav"),
    ];

    fn update(&mut self, audio: &mut Audio, animator: &Animator, player: &ThirdPersonController, dt: f32) {
        let grounded = player.body.grounded;
        let landed = grounded && self.air_time > 0.25;
        self.air_time = if grounded { 0.0 } else { self.air_time + dt };

        if landed || (grounded && animator.fired("step")) {
            self.count += 1;
            let v = player.velocity();
            let speed = Vec2::new(v.x, v.z).length();
            let pitch = [1.0, 0.94, 1.05, 0.97][self.count % 4];
            let volume = if landed { 0.7 } else { (0.3 + speed * 0.04).min(0.6) };
            audio.play_pitched(Self::SOUNDS[self.count % 2], volume, pitch);
        }
    }
}

#[cfg(test)]
mod locomotion_tests {
    use super::*;

    #[test]
    fn jump_lands_on_contact_and_recovers_at_each_movement_speed() {
        let mesh = grimora::scene::load_model(WANDERER.as_ref(), grimora::scene::DEFAULT_MAX_TEXTURE_SIZE).unwrap();
        let skeleton = mesh.skeleton.unwrap();
        for (speed, landing, recovered) in [
            (0.0, "jump_land", "sword_idle"),
            (WALK_SPEED, "jump_land_walk", "sword_walk"),
            (RUN_SPEED, "jump_land_run", "sword_run"),
        ] {
            let mut player = ThirdPersonController::new(Vec3::ZERO, 0.0);
            player.walk_speed = WALK_SPEED;
            player.run_speed = RUN_SPEED;
            player.body.grounded = false;
            player.push(Vec3::new(0.0, 7.0, -speed));
            let mut animator = Animator::new(skeleton.clone());
            let input = Input::default();
            animate(&mut animator, &player, &input);
            assert_eq!(animator.current(), Some("jump"));
            animator.update(0.2);
            player.push(Vec3::new(0.0, -12.0, 0.0));
            animate(&mut animator, &player, &input);
            assert_eq!(animator.current(), Some("jump_fall"));
            animator.update(1.0);
            animate(&mut animator, &player, &input);
            assert!(animator.finished(), "a long fall must hold, not replay take-off");
            player.body.grounded = true;
            animate(&mut animator, &player, &input);
            assert_eq!(animator.current(), Some(landing));
            animator.update(0.07);
            animate(&mut animator, &player, &input);
            assert_eq!(animator.current(), Some(landing), "keep the impact absorption");
            animator.update(2.0);
            animate(&mut animator, &player, &input);
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
    menu::apply_startup_options(&mut config);
    grimora::run(config, Sandbox::new())
}
