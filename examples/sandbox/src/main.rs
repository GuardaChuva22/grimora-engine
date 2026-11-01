//! Grimora sandbox: walk a hooded wanderer around `assets/scenes/sandbox.ron`,
//! or press Tab for a free-flying debug camera.
//!
//!     cargo run -p sandbox
//!     cargo run -p sandbox -- --screenshot shots/sandbox.png
//!     cargo run -p sandbox -- --help
//!
//! Passing `--camera` starts in fly mode, so screenshots can frame any view.

use grimora::prelude::*;

const DEFAULT_SCENE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/scenes/sandbox.ron");

/// The animated wanderer (made by tools/gen_wanderer.py).
const WANDERER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/models/wanderer/wanderer.gltf");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Play,
    Fly,
}

struct Sandbox {
    mode: Mode,
    fly: FlyCamera,
    player: Option<ThirdPersonController>,
    /// Indices into the world, re-created whenever the scene (re)loads.
    avatar: usize,
    lantern_light: usize,
    /// The "heart" crystal and its resting position.
    heart: Option<(usize, Vec3)>,
}

impl Sandbox {
    fn new() -> Self {
        Self { mode: Mode::Play, fly: FlyCamera::default(), player: None, avatar: 0, lantern_light: 0, heart: None }
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
}

impl Game for Sandbox {
    fn init(&mut self, ctx: &mut Context) {
        if ctx.config().camera_position.is_some() || ctx.config().look_at.is_some() {
            self.mode = Mode::Fly;
        }
    }

    fn scene_loaded(&mut self, ctx: &mut Context) {
        self.heart = ctx
            .world
            .objects
            .iter()
            .position(|o| o.name.as_deref() == Some("heart"))
            .map(|i| (i, ctx.world.objects[i].transform.position));

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
        self.avatar = ctx.world.spawn(wanderer, Transform::IDENTITY, Vec3::ONE);
        ctx.world.lights.push(PointLight {
            color: Vec3::new(1.0, 0.68, 0.38),
            intensity: 7.0,
            radius: 9.0,
            flicker: 0.6,
            ..Default::default()
        });
        self.lantern_light = ctx.world.lights.len() - 1;
    }

    fn update(&mut self, ctx: &mut Context) {
        if ctx.input.just_pressed(KeyCode::Tab) {
            self.mode = if self.mode == Mode::Play { Mode::Fly } else { Mode::Play };
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
        let avatar = &mut ctx.world.objects[self.avatar];
        avatar.transform = player.transform();
        if let Some(animator) = avatar.animator.as_mut() {
            animate(animator, player);
        }

        // The light hangs from the lantern joint, so it swings with the walk.
        if let Some(lantern) = ctx.world.joint_transform(self.avatar, "lantern") {
            ctx.world.lights[self.lantern_light].position =
                lantern.position + lantern.rotation * Vec3::new(0.0, -0.1, 0.0);
        }

        let t = ctx.time.elapsed();

        if let Some((index, rest)) = self.heart {
            let heart = &mut ctx.world.objects[index];
            heart.transform.position = rest + Vec3::Y * (0.25 + 0.15 * (t * 1.3).sin());
            heart.transform.rotation = Quat::from_rotation_y(t * 0.4);
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

fn main() -> anyhow::Result<()> {
    let config =
        EngineConfig { title: "Grimora Sandbox".into(), scene: Some(DEFAULT_SCENE.into()), ..Default::default() }
            .with_cli()?;
    grimora::run(config, Sandbox::new())
}
