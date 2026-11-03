//! Pausing the world and the rest of what the runner does around `Game::update`,
//! through the same code the window loop uses. Headless, like `character.rs`.

use std::path::PathBuf;

use grimora::core::ParticleInstance;
use grimora::prelude::*;
use grimora::scene::{DEFAULT_MAX_TEXTURE_SIZE, load_model};

const DT: f32 = 1.0 / 60.0;
const WANDERER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/models/wanderer/wanderer.gltf");
const SCENE: &str = r#"(
    day_cycle: (time: 9.0, day_length: 60.0),
    objects: [
        (name: "floor", mesh: Cube, position: (0.0, -0.5, 0.0), scale: (100.0, 1.0, 100.0)),
        (name: "box", mesh: Cube, position: (0.0, 40.0, 0.0), body: Dynamic(density: 500.0)),
        (name: "torch", mesh: Cube, position: (5.0, 0.0, 0.0), effect: (path: "fire.ron")),
    ],
)"#;

/// A folder with a scene and its effect, deleted at the end of the test.
struct Assets(PathBuf);

impl Assets {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("grimora_pause_{name}_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("scene.ron"), SCENE).unwrap();
        std::fs::write(dir.join("fire.ron"), "[(rate: 60.0, lifetime: (1.0, 1.0), max_distance: 500.0)]").unwrap();
        Self(dir)
    }

    fn config(&self) -> EngineConfig {
        EngineConfig {
            scene: Some(self.0.join("scene.ron")),
            save_dir: Some(self.0.join("saves")),
            ..Default::default()
        }
    }
}

impl Drop for Assets {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Spawns a walking wanderer on every scene load and counts what the engine calls.
#[derive(Default)]
struct TestGame {
    walker: Option<Entity>,
    updates: u32,
    loads: u32,
    /// Draws a button each frame and counts its uses.
    button: bool,
    used: u32,
}

impl Game for TestGame {
    fn scene_loaded(&mut self, ctx: &mut Context) {
        self.loads += 1;
        let mesh = ctx.world.add_mesh(load_model(WANDERER.as_ref(), DEFAULT_MAX_TEXTURE_SIZE).unwrap());
        let walker = ctx.world.spawn(mesh, Transform::IDENTITY);
        assert!(ctx.world.get_mut::<Animator>(walker).unwrap().play("walk", 0.0));
        self.walker = Some(walker);
    }

    fn update(&mut self, ctx: &mut Context) {
        self.updates += 1;
        if self.button && ctx.ui.button(Rect::new(Vec2::new(100.0, 100.0), Vec2::new(200.0, 40.0)), "usar") {
            self.used += 1;
        }
    }
}

fn start(assets: &Assets) -> (TestGame, Context) {
    let mut game = TestGame::default();
    let ctx = Context::headless(assets.config(), &mut game).unwrap();
    (game, ctx)
}

fn frames(game: &mut TestGame, ctx: &mut Context, count: u32) {
    for _ in 0..count {
        ctx.headless_frame(game, DT).unwrap();
    }
}

/// Everything in the world that moves on its own.
#[derive(PartialEq)]
struct Snapshot {
    falling: Vec3,
    walk_time: f32,
    pose: Vec<Mat4>,
    particles: Vec<ParticleInstance>,
    world_time: f32,
    hour: Option<f32>,
}

fn snapshot(game: &TestGame, ctx: &Context) -> Snapshot {
    let animator = ctx.world.get::<Animator>(game.walker.unwrap()).unwrap();
    Snapshot {
        falling: ctx.world.transform(ctx.world.find("box").unwrap()).unwrap().position,
        walk_time: animator.time(),
        pose: animator.skinning_matrices().to_vec(),
        particles: ctx.world.active_particles(),
        world_time: ctx.time.world_elapsed(),
        hour: ctx.world.time_of_day(),
    }
}

#[test]
fn nothing_moves_during_a_pause_and_everything_goes_on_after_it() {
    let assets = Assets::new("freeze");
    let (mut game, mut ctx) = start(&assets);
    assert_eq!(game.loads, 1);

    // Running: the box falls, the walk cycle turns and the fire burns.
    frames(&mut game, &mut ctx, 1);
    let first = snapshot(&game, &ctx);
    frames(&mut game, &mut ctx, 30);
    let running = snapshot(&game, &ctx);
    assert!(running.falling.y < first.falling.y - 0.1, "{} then {}", first.falling, running.falling);
    assert!(running.walk_time != first.walk_time);
    assert!(running.pose != first.pose);
    assert!(!running.particles.is_empty());
    assert!(running.particles != first.particles);
    assert!(running.hour.unwrap() > first.hour.unwrap(), "the hours pass");

    ctx.set_paused(true);
    assert!(ctx.is_paused());
    let (updates, real_time) = (game.updates, ctx.time.elapsed());
    let paused = snapshot(&game, &ctx);
    frames(&mut game, &mut ctx, 120);
    assert!(snapshot(&game, &ctx) == paused, "not a thing moved");
    assert_eq!(game.updates, updates + 120, "the game still runs, for its menus");
    assert_eq!(ctx.time.world_delta(), 0.0);
    assert_eq!(ctx.time.delta(), DT, "HUD fades run on real time");
    assert!((ctx.time.elapsed() - real_time - 2.0).abs() < 1e-3);

    ctx.set_paused(false);
    frames(&mut game, &mut ctx, 30);
    let resumed = snapshot(&game, &ctx);
    assert!(resumed.falling.y < paused.falling.y - 0.1, "{} then {}", paused.falling, resumed.falling);
    assert!(resumed.walk_time != paused.walk_time);
    assert!(resumed.pose != paused.pose);
    assert!(resumed.particles != paused.particles);
    assert!(resumed.hour.unwrap() > paused.hour.unwrap());
    assert!(
        (resumed.world_time - paused.world_time - 0.5).abs() < 1e-3,
        "the world clock picks up where it stopped: {} then {}",
        paused.world_time,
        resumed.world_time
    );
    // It fell from where it stopped, not as if two seconds had passed: 30 frames
    // at the speed it had, instead of 150.
    assert!(paused.falling.y - resumed.falling.y < 6.0, "{} then {}", paused.falling, resumed.falling);
}

/// `--time` holds the hour however long the game runs.
#[test]
fn a_time_from_the_command_line_stops_the_clock() {
    let assets = Assets::new("time");
    let mut game = TestGame::default();
    let config = EngineConfig { time_of_day: Some(3.0), ..assets.config() };
    let mut ctx = Context::headless(config, &mut game).unwrap();
    frames(&mut game, &mut ctx, 60);
    assert_eq!(ctx.world.time_of_day(), Some(3.0));
}

/// Pausing from inside `Game::update` freezes that very frame's animations and particles.
#[test]
fn a_pause_takes_effect_within_the_frame() {
    struct PauseAt(TestGame, u32);
    impl Game for PauseAt {
        fn scene_loaded(&mut self, ctx: &mut Context) {
            self.0.scene_loaded(ctx);
        }

        fn update(&mut self, ctx: &mut Context) {
            self.0.update(ctx);
            if self.0.updates == self.1 {
                ctx.set_paused(true);
            }
        }
    }

    let assets = Assets::new("within");
    let mut game = PauseAt(TestGame::default(), 11);
    let mut ctx = Context::headless(assets.config(), &mut game).unwrap();
    for _ in 0..10 {
        ctx.headless_frame(&mut game, DT).unwrap();
    }
    let walk = ctx.world.get::<Animator>(game.0.walker.unwrap()).unwrap().time();
    ctx.headless_frame(&mut game, DT).unwrap();
    assert!(ctx.is_paused());
    assert_eq!(ctx.world.get::<Animator>(game.0.walker.unwrap()).unwrap().time(), walk);
}

#[test]
fn escape_quits_unless_the_game_takes_it_over() {
    let assets = Assets::new("escape");
    let (mut game, mut ctx) = start(&assets);
    frames(&mut game, &mut ctx, 1);
    assert!(!ctx.exit_requested());
    ctx.input.key_down(KeyCode::Escape);
    frames(&mut game, &mut ctx, 1);
    assert!(ctx.exit_requested(), "today's behavior stays the default");

    let mut game = TestGame::default();
    let config = EngineConfig { escape_quits: false, ..assets.config() };
    let mut ctx = Context::headless(config, &mut game).unwrap();
    ctx.input.key_down(KeyCode::Escape);
    assert!(ctx.input.just_pressed(KeyCode::Escape));
    frames(&mut game, &mut ctx, 1);
    assert!(!ctx.exit_requested());
}

#[test]
fn widgets_work_during_a_pause_with_the_cursor_free() {
    let assets = Assets::new("widgets");
    let (mut game, mut ctx) = start(&assets);
    game.button = true;
    ctx.set_paused(true);
    let inside = Vec2::new(150.0, 120.0);

    // While grabbed, the mouse can't reach them.
    ctx.set_cursor_grab(true);
    frames(&mut game, &mut ctx, 1);
    assert!(ctx.cursor_grabbed());
    ctx.input.set_cursor(inside);
    ctx.input.button_down(MouseButton::Left);
    frames(&mut game, &mut ctx, 1);
    ctx.input.button_up(MouseButton::Left);
    frames(&mut game, &mut ctx, 1);
    assert_eq!(game.used, 0);

    ctx.set_cursor_grab(false);
    frames(&mut game, &mut ctx, 1);
    ctx.input.button_down(MouseButton::Left);
    frames(&mut game, &mut ctx, 1);
    ctx.input.button_up(MouseButton::Left);
    frames(&mut game, &mut ctx, 1);
    assert_eq!(game.used, 1);

    ctx.input.key_down(KeyCode::Enter);
    frames(&mut game, &mut ctx, 2);
    assert_eq!(game.used, 2, "the keyboard too, once per press");
}

#[test]
fn the_ui_scale_shrinks_the_layout_and_follows_the_cursor() {
    let assets = Assets::new("scale");
    let (mut game, mut ctx) = start(&assets);
    game.button = true;
    frames(&mut game, &mut ctx, 1);
    let size = ctx.ui.size();

    ctx.set_ui_scale(2.0);
    frames(&mut game, &mut ctx, 1);
    assert_eq!(ctx.ui.size(), size / 2.0);
    assert_eq!(ctx.ui.scale(), 2.0);

    // The button is at (100..300, 100..140) in UI pixels: double that on the window.
    ctx.input.set_cursor(Vec2::new(150.0, 120.0) * 2.0);
    frames(&mut game, &mut ctx, 1);
    ctx.input.button_down(MouseButton::Left);
    frames(&mut game, &mut ctx, 1);
    ctx.input.button_up(MouseButton::Left);
    frames(&mut game, &mut ctx, 1);
    assert_eq!(game.used, 1);
}

#[test]
fn no_ui_runs_draw_nothing_but_the_game_still_runs() {
    let assets = Assets::new("no_ui");
    let mut game = TestGame { button: true, ..Default::default() };
    let config = EngineConfig { ui: false, ..assets.config() };
    let mut ctx = Context::headless(config, &mut game).unwrap();
    frames(&mut game, &mut ctx, 2);
    assert!(ctx.ui.is_empty());
    assert!(ctx.ui.is_focused("usar"), "widget state survives");
}

#[test]
fn reload_scene_rebuilds_the_world_on_the_next_frame() {
    let assets = Assets::new("reload");
    let (mut game, mut ctx) = start(&assets);
    frames(&mut game, &mut ctx, 30);
    let fallen = ctx.world.transform(ctx.world.find("box").unwrap()).unwrap().position;
    assert!(fallen.y < 39.0);
    let camera = Vec3::new(1.0, 2.0, 3.0);
    ctx.camera.position = camera;

    ctx.reload_scene();
    assert_eq!(game.loads, 1, "not in the middle of a frame");
    frames(&mut game, &mut ctx, 1);
    assert_eq!(game.loads, 2);
    let back = ctx.world.transform(ctx.world.find("box").unwrap()).unwrap().position;
    assert!(back.y > fallen.y, "{fallen} then {back}");
    assert_eq!(ctx.camera.position, camera, "the camera is the game's");
    frames(&mut game, &mut ctx, 1);
    assert_eq!(game.loads, 2, "once");
}
