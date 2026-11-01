//! Grimora sandbox: walk a hooded wanderer around `assets/scenes/sandbox.ron`,
//! or press Tab for a free-flying debug camera.
//!
//!     cargo run -p sandbox
//!     cargo run -p sandbox -- --screenshot shots/sandbox.png
//!     cargo run -p sandbox -- --help
//!
//! Passing `--camera` starts in fly mode, so screenshots can frame any view.
//! F6 saves the game to the "quick" slot and F9 loads it; `--load <slot>`
//! loads a slot at startup.

use grimora::prelude::*;
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
}

struct Sandbox {
    mode: Mode,
    fly: FlyCamera,
    player: Option<ThirdPersonController>,
    /// Entities re-created whenever the scene (re)loads.
    avatar: Entity,
    /// Carries the lantern's light; follows the `lantern` joint.
    lantern: Entity,
    /// The "heart" crystal and its resting position.
    heart: Option<(Entity, Vec3)>,
    steps: Footsteps,
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
            steps: Footsteps::default(),
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

    fn save(&self, ctx: &Context, slot: &str) {
        let Some(player) = &self.player else { return };
        match ctx.save_game(slot, &SandboxSave { player: player.state(), mode: self.mode }) {
            Ok(path) => println!("[sandbox] saved to {}", path.display()),
            Err(err) => eprintln!("[sandbox] save failed: {err:#}"),
        }
    }

    fn load(&mut self, ctx: &mut Context, slot: &str) {
        match ctx.load_game::<SandboxSave>(slot) {
            Ok(save) => {
                if let Some(player) = &mut self.player {
                    player.restore(&save.player);
                }
                self.mode = save.mode;
                println!("[sandbox] loaded {slot:?}");
            }
            Err(err) => eprintln!("[sandbox] load failed: {err:#}"),
        }
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
        self.heart = ctx.world.find("heart").and_then(|e| Some((e, ctx.world.transform(e)?.position)));

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

        let t = ctx.time.elapsed();

        // Its light is a component of the same entity, so it floats along.
        if let Some((heart, rest)) = self.heart
            && let Some(transform) = ctx.world.get_mut::<Transform>(heart)
        {
            transform.position = rest + Vec3::Y * (0.25 + 0.15 * (t * 1.3).sin());
            transform.rotation = Quat::from_rotation_y(t * 0.4);
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
