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

/// Where the lantern hangs relative to the character (left hand, slightly forward).
const LANTERN_OFFSET: Vec3 = Vec3::new(-0.34, 0.86, -0.22);

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
    lantern: usize,
    lantern_light: usize,
    /// The "heart" crystal and its resting position.
    heart: Option<(usize, Vec3)>,
    steps: Footsteps,
}

impl Sandbox {
    fn new() -> Self {
        Self {
            mode: Mode::Play,
            fly: FlyCamera::default(),
            player: None,
            avatar: 0,
            lantern: 0,
            lantern_light: 0,
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

        let wanderer = ctx.world.add_mesh(MeshDesc::Wanderer.build());
        self.avatar = ctx.world.spawn(wanderer, Transform::IDENTITY, Vec3::ONE);
        let cube = ctx.world.add_mesh(MeshDesc::Cube.build());
        self.lantern = ctx.world.spawn(cube, Transform::IDENTITY, Vec3::new(1.0, 0.7, 0.35));
        ctx.world.objects[self.lantern].emissive = 6.0;
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
            }
        }
        // Fell out of the world: back to the start.
        if player.body.position.y < -50.0 {
            self.spawn_player(ctx);
        }

        let Some(player) = self.player.as_ref() else { return };
        if self.mode == Mode::Play {
            self.steps.update(&mut ctx.audio, player, dt);
        }
        let body = player.transform();
        ctx.world.objects[self.avatar].transform = body;

        // The lantern sways a little with speed and time.
        let t = ctx.time.elapsed();
        let speed = player.velocity().length();
        let sway = Quat::from_rotation_x((t * 5.0).sin() * 0.05 * (1.0 + speed * 0.3));
        let lantern = Transform {
            position: body.position + body.rotation * LANTERN_OFFSET,
            rotation: body.rotation * sway,
            scale: Vec3::new(0.14, 0.2, 0.14),
        };
        ctx.world.objects[self.lantern].transform = lantern;
        ctx.world.lights[self.lantern_light].position = lantern.position + Vec3::Y * 0.1;

        if let Some((index, rest)) = self.heart {
            let heart = &mut ctx.world.objects[index];
            heart.transform.position = rest + Vec3::Y * (0.25 + 0.15 * (t * 1.3).sin());
            heart.transform.rotation = Quat::from_rotation_y(t * 0.4);
        }
    }
}

/// Plays a footstep every stride while the wanderer is on the ground, and one on landing.
#[derive(Default)]
struct Footsteps {
    walked: f32,
    count: usize,
    /// Seconds off the ground; only real jumps and falls thump on landing.
    air_time: f32,
}

impl Footsteps {
    const SOUNDS: [&str; 2] = [
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/step_1.wav"),
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/step_2.wav"),
    ];

    fn update(&mut self, audio: &mut Audio, player: &ThirdPersonController, dt: f32) {
        let v = player.velocity();
        let speed = Vec2::new(v.x, v.z).length();
        let grounded = player.body.grounded;
        let landed = grounded && self.air_time > 0.25;
        self.air_time = if grounded { 0.0 } else { self.air_time + dt };
        // Longer strides when running; the first step comes soon after starting.
        let stride = 0.7 + speed * 0.06;
        if !grounded || speed < 0.4 {
            self.walked = stride * 0.6;
        } else {
            self.walked += speed * dt;
        }
        if landed || (grounded && self.walked >= stride) {
            self.walked = (self.walked - stride).max(0.0);
            self.count += 1;
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
