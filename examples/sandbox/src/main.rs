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
//! object in reach (the altar's crystal goes dark and wakes up again).

mod hud;

use grimora::prelude::*;
use hud::Hud;
use serde::{Deserialize, Serialize};

const DEFAULT_SCENE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/scenes/sandbox.ron");

/// The animated wanderer (made by tools/gen_wanderer.py).
const WANDERER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/models/wanderer/wanderer.gltf");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum Mode {
    Play,
    Fly,
}

const QUICK_SLOT: &str = "quick";

/// The sandbox's part of a saved game; the engine adds the camera and the
/// crates and stones the wanderer pushed around.
#[derive(Debug, Serialize, Deserialize)]
struct SandboxSave {
    player: CharacterState,
    mode: Mode,
    #[serde(default)]
    heart_asleep: bool,
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
        self.player = Some(ThirdPersonController::new(position + Vec3::Y * 0.1, yaw));
    }

    fn save(&mut self, ctx: &Context, slot: &str) {
        let Some(player) = &self.player else { return };
        let save = SandboxSave { player: player.state(), mode: self.mode, heart_asleep: self.heart_asleep };
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

    fn load(&mut self, ctx: &mut Context, slot: &str) {
        match ctx.load_game::<SandboxSave>(slot) {
            Ok(save) => {
                if let Some(player) = &mut self.player {
                    player.restore(&save.player);
                }
                self.mode = save.mode;
                self.heart_asleep = save.heart_asleep;
                println!("[sandbox] loaded {slot:?}");
                self.hud.notify(ctx, "Jogo carregado", true);
            }
            Err(err) => {
                eprintln!("[sandbox] load failed: {err:#}");
                self.hud.notify(ctx, "Nenhum jogo salvo para carregar", false);
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
        if !ctx.input.just_pressed(KeyCode::KeyE) {
            return;
        }
        match target.name.as_deref() {
            Some("heart") => {
                self.heart_asleep = !self.heart_asleep;
                let text = if self.heart_asleep { "O cristal adormece" } else { "O cristal desperta" };
                self.hud.notify(ctx, text, true);
            }
            Some("shrine_crystal") => self.hud.notify(ctx, "A pedra está fria ao toque", true),
            _ => {}
        }
    }

    /// The heart floats and turns, and fades to its glow; its light is a
    /// component of the same entity, so it floats along.
    fn animate_heart(&mut self, ctx: &mut Context) {
        let Some(heart) = &self.heart else { return };
        let t = ctx.time.elapsed();
        let target = if self.heart_asleep { ASLEEP_GLOW } else { 1.0 };
        self.heart_glow += (target - self.heart_glow) * (1.0 - (-3.0 * ctx.time.delta()).exp());
        let glow = self.heart_glow;
        let query = ctx.world.ecs.query_one_mut::<(&mut Transform, &mut Renderable, &mut Light)>(heart.entity);
        let Ok((transform, renderable, Light(light))) = query else { return };
        // It sinks and slows as it goes dark.
        transform.position = heart.rest + Vec3::Y * (0.25 * glow + 0.15 * glow * (t * 1.3).sin());
        transform.rotation = Quat::from_rotation_y(t * 0.4);
        renderable.emissive = heart.emissive * glow;
        light.intensity = heart.light * glow;
    }
}

impl Game for Sandbox {
    fn init(&mut self, ctx: &mut Context) {
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
        let light = PointLight {
            position: Vec3::new(0.0, -0.1, 0.0),
            color: Vec3::new(1.0, 0.68, 0.38),
            intensity: 7.0,
            radius: 9.0,
            flicker: 0.6,
        };
        self.lantern = ctx.world.ecs.spawn((Transform::IDENTITY, Light(light)));
    }

    fn update(&mut self, ctx: &mut Context) {
        if ctx.input.just_pressed(KeyCode::Tab) {
            self.mode = if self.mode == Mode::Play { Mode::Fly } else { Mode::Play };
        }
        if ctx.input.just_pressed(KeyCode::F6) {
            self.save(ctx, QUICK_SLOT);
        }
        if ctx.input.just_pressed(KeyCode::F9) {
            self.load(ctx, QUICK_SLOT);
        }
        self.hud.draw(ctx, self.mode == Mode::Fly);
        let dt = ctx.time.delta();
        let automated = ctx.config().screenshot.is_some();

        let Some(player) = self.player.as_mut() else { return };
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
            animate(animator, player);
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
        self.check_triggers(ctx, center);
        if self.mode == Mode::Play {
            self.interact(ctx, center);
        }
    }
}

/// Picks the wanderer's clip from how the character moves.
fn animate(animator: &mut Animator, player: &ThirdPersonController) {
    let velocity = player.velocity();
    let speed = Vec2::new(velocity.x, velocity.z).length();
    let (clip, rate) = if !player.body.grounded {
        ("jump", 1.0)
    } else if speed < 0.4 {
        ("idle", 1.0)
    } else if speed < (player.walk_speed + player.run_speed) * 0.5 {
        // Clips are authored at the controller's walk and run speeds; scale the
        // playback so the feet don't slide while speeding up or slowing down.
        ("walk", (speed / player.walk_speed).max(0.5))
    } else {
        ("run", speed / player.run_speed)
    };
    animator.play(clip, 0.2);
    animator.speed = rate;
}

/// Plays a footstep whenever the walk or run cycle plants a foot, and one on landing.
#[derive(Default)]
struct Footsteps {
    /// Clip index and cycle position (0..1) last frame, while walking or running.
    cycle: Option<(usize, f32)>,
    count: usize,
    /// Seconds off the ground; only real jumps and falls thump on landing.
    air_time: f32,
}

impl Footsteps {
    const SOUNDS: [&str; 2] = [
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/step_1.wav"),
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/step_2.wav"),
    ];
    /// Where tools/gen_wanderer.py's walk and run cycles plant the left and right foot.
    const PLANTS: [f32; 2] = [0.25, 0.75];

    fn update(&mut self, audio: &mut Audio, animator: &Animator, player: &ThirdPersonController, dt: f32) {
        let grounded = player.body.grounded;
        let landed = grounded && self.air_time > 0.25;
        self.air_time = if grounded { 0.0 } else { self.air_time + dt };

        let skeleton = animator.skeleton();
        let cycle = animator.current().filter(|name| matches!(*name, "walk" | "run")).and_then(|name| {
            let clip = skeleton.clip(name)?;
            Some((clip, animator.time() / skeleton.clips[clip].duration))
        });
        // A foot planted if its mark lies between last frame's position and this one
        // (same clip only: a crossfade restarts the cycle).
        let planted = match (self.cycle, cycle) {
            (Some((a, from)), Some((b, to))) if a == b => Self::PLANTS
                .iter()
                .any(|&mark| if to >= from { from < mark && mark <= to } else { from < mark || mark <= to }),
            _ => false,
        };
        self.cycle = cycle;

        if landed || (grounded && planted) {
            self.count += 1;
            let v = player.velocity();
            let speed = Vec2::new(v.x, v.z).length();
            let pitch = [1.0, 0.94, 1.05, 0.97][self.count % 4];
            let volume = if landed { 0.7 } else { (0.3 + speed * 0.04).min(0.6) };
            audio.play_pitched(Self::SOUNDS[self.count % 2], volume, pitch);
        }
    }
}

fn main() -> anyhow::Result<()> {
    let config =
        EngineConfig { title: "Grimora Sandbox".into(), scene: Some(DEFAULT_SCENE.into()), ..Default::default() }
            .with_cli()?;
    grimora::run(config, Sandbox::new())
}
