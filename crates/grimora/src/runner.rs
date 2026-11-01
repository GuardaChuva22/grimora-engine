use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context as _, Result};
use grimora_core::{Camera, Input, KeyCode, Time, Vec2};
use grimora_physics::Physics;
use grimora_render::{DrawItem, Frame, FrameStats, MeshHandle, Renderer, RendererSettings};
use grimora_scene::Scene;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::{CursorGrabMode, Window, WindowId};

use crate::{EngineConfig, Game, World};

/// Frames ignored by the timing report while shaders and caches warm up.
const WARMUP_FRAMES: u64 = 10;
const HOT_RELOAD_INTERVAL: Duration = Duration::from_millis(400);

/// The game's view of the engine during `init` and `update`.
pub struct Context {
    pub time: Time,
    pub input: Input,
    pub camera: Camera,
    pub world: World,
    /// Collision world built from the scene's colliders; stepped before `update`,
    /// after which dynamic bodies' poses are copied into `world`.
    pub physics: Physics,
    config: EngineConfig,
    exit: bool,
    cursor_grab: bool,
    screenshot: Option<PathBuf>,
    stats: FrameStats,
    aspect: f32,
}

impl Context {
    /// Closes the engine at the end of this frame.
    pub fn exit(&mut self) {
        self.exit = true;
    }

    /// Hides and locks the cursor (for mouse look) or releases it.
    pub fn set_cursor_grab(&mut self, grab: bool) {
        self.cursor_grab = grab;
    }

    /// Saves this frame as a PNG once it is rendered.
    pub fn screenshot(&mut self, path: impl Into<PathBuf>) {
        self.screenshot = Some(path.into());
    }

    /// Draw counters from the previous frame.
    pub fn render_stats(&self) -> FrameStats {
        self.stats
    }

    /// Width / height of the window.
    pub fn aspect(&self) -> f32 {
        self.aspect
    }

    /// How the engine was started (command line flags included).
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }
}

/// Opens a window and runs `game` until it exits.
pub fn run(config: EngineConfig, game: impl Game) -> Result<()> {
    init_logging();
    let event_loop = EventLoop::new().context("creating event loop")?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        config: config.clone(),
        game,
        ctx: Context {
            time: Time::new(),
            input: Input::default(),
            camera: Camera::default(),
            world: World::default(),
            physics: Physics::new(),
            config,
            exit: false,
            cursor_grab: false,
            screenshot: None,
            stats: FrameStats::default(),
            aspect: 16.0 / 9.0,
        },
        window: None,
        renderer: None,
        handles: Vec::new(),
        grabbed: false,
        scene_mtime: None,
        last_reload_check: Instant::now(),
        frame_times: Vec::new(),
        title_timer: (Instant::now(), 0),
        error: None,
    };
    event_loop.run_app(&mut app).context("running event loop")?;
    if let Some(error) = app.error {
        return Err(error);
    }
    app.report();
    Ok(())
}

fn init_logging() {
    let env = env_logger::Env::default().default_filter_or("info,wgpu_core=warn,wgpu_hal=warn,naga=warn");
    let _ = env_logger::Builder::from_env(env).format_timestamp(None).try_init();
}

struct App<G: Game> {
    config: EngineConfig,
    game: G,
    ctx: Context,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    /// GPU handle for each world mesh, by `MeshId` index.
    handles: Vec<MeshHandle>,
    grabbed: bool,
    scene_mtime: Option<SystemTime>,
    last_reload_check: Instant,
    frame_times: Vec<f32>,
    title_timer: (Instant, u32),
    error: Option<anyhow::Error>,
}

impl<G: Game> App<G> {
    fn start(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let attributes = Window::default_attributes()
            .with_title(&self.config.title)
            .with_inner_size(PhysicalSize::new(self.config.width, self.config.height));
        let window = Arc::new(event_loop.create_window(attributes).context("creating window")?);
        let size = window.inner_size();
        let settings = RendererSettings { vsync: self.config.vsync, msaa: self.config.msaa, ..Default::default() };
        let renderer = pollster::block_on(Renderer::new(window.clone(), size.width, size.height, &settings))?;
        self.ctx.aspect = renderer.aspect();
        self.window = Some(window);
        self.renderer = Some(renderer);

        if let Some(path) = self.config.scene.clone() {
            self.load_scene(&path, true)?;
        }
        self.game.init(&mut self.ctx);
        self.ctx.time = Time::new();
        Ok(())
    }

    fn load_scene(&mut self, path: &Path, initial: bool) -> Result<()> {
        let started = Instant::now();
        let scene = Scene::load(path)?;
        log::info!(
            "Loaded {} ({} objects, {} meshes, {} triangles) in {:.0} ms",
            path.display(),
            scene.objects.len(),
            scene.meshes.len(),
            scene.triangle_count(),
            started.elapsed().as_secs_f64() * 1000.0
        );
        if initial {
            self.ctx.camera = scene.camera;
            if let Some(position) = self.config.camera_position {
                self.ctx.camera.position = position;
            }
            if let Some(target) = self.config.look_at {
                self.ctx.camera.look_at(target);
            }
        }
        let started = Instant::now();
        self.ctx.physics = Physics::from_scene(&scene);
        log::info!(
            "Built {} colliders in {:.0} ms",
            self.ctx.physics.collider_count(),
            started.elapsed().as_secs_f64() * 1000.0
        );
        self.ctx.world = World::from_scene(scene);
        if let Some(renderer) = &mut self.renderer {
            renderer.clear_meshes();
        }
        self.handles.clear();
        self.scene_mtime = modified(path);
        self.game.scene_loaded(&mut self.ctx);
        Ok(())
    }

    fn check_hot_reload(&mut self) {
        if !self.config.hot_reload || self.last_reload_check.elapsed() < HOT_RELOAD_INTERVAL {
            return;
        }
        self.last_reload_check = Instant::now();
        let Some(path) = self.config.scene.clone() else {
            return;
        };
        if modified(&path) != self.scene_mtime {
            self.reload(&path);
        }
    }

    fn reload(&mut self, path: &Path) {
        // A failed parse keeps the current world, so a typo never kills the session.
        if let Err(e) = self.load_scene(path, false) {
            log::error!("Scene reload failed: {e:#}");
            self.scene_mtime = modified(path);
        }
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        self.ctx.time.tick();
        self.check_hot_reload();

        if self.ctx.input.just_pressed(KeyCode::Escape) {
            self.ctx.exit();
        }
        if self.ctx.input.just_pressed(KeyCode::F5)
            && let Some(path) = self.config.scene.clone()
        {
            self.reload(&path);
        }
        if self.ctx.input.just_pressed(KeyCode::F12) {
            let stamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs();
            self.ctx.screenshot(format!("screenshots/grimora_{stamp}.png"));
        }

        self.ctx.physics.step(self.ctx.time.delta());
        self.ctx.world.sync_bodies(&self.ctx.physics);
        self.game.update(&mut self.ctx);
        self.apply_cursor_grab();

        let frame_number = self.ctx.time.frame();
        let limit = self.config.frame_limit();
        let at_limit = limit.is_some_and(|n| frame_number >= n);
        if at_limit && let Some(path) = self.config.screenshot.clone() {
            self.ctx.screenshot = Some(path);
        }

        let Some(renderer) = self.renderer.as_mut() else {
            return Ok(());
        };
        let world = &self.ctx.world;
        for mesh in &world.meshes()[self.handles.len()..] {
            self.handles.push(renderer.upload_mesh(mesh));
        }
        let draws: Vec<DrawItem> = world
            .objects
            .iter()
            .filter(|o| o.visible)
            .filter_map(|o| {
                Some(DrawItem {
                    mesh: *self.handles.get(o.mesh.0)?,
                    transform: o.transform,
                    color: o.color,
                    emissive: o.emissive,
                })
            })
            .collect();
        let frame = Frame {
            camera: &self.ctx.camera,
            environment: &world.environment,
            lights: &world.lights,
            draws: &draws,
            time: self.ctx.time.elapsed(),
        };
        let capture = self.ctx.screenshot.take();
        if let Some(image) = renderer.render(&frame, capture.is_some())? {
            let path = capture.expect("capture was requested");
            if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir)?;
            }
            image.save(&path).with_context(|| format!("saving {}", path.display()))?;
            println!("[grimora] screenshot saved: {}", path.display());
        }
        self.ctx.stats = renderer.stats();
        self.ctx.input.end_frame();

        if frame_number > WARMUP_FRAMES {
            self.frame_times.push(self.ctx.time.delta() * 1000.0);
        }
        self.update_title();

        if self.ctx.exit || at_limit {
            event_loop.exit();
        }
        Ok(())
    }

    fn apply_cursor_grab(&mut self) {
        let Some(window) = &self.window else { return };
        if self.ctx.cursor_grab == self.grabbed {
            return;
        }
        self.grabbed = self.ctx.cursor_grab;
        if self.grabbed {
            let locked = window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));
            if let Err(e) = locked {
                log::warn!("cursor grab failed: {e}");
            }
        } else {
            let _ = window.set_cursor_grab(CursorGrabMode::None);
        }
        window.set_cursor_visible(!self.grabbed);
    }

    fn update_title(&mut self) {
        let (since, frames) = &mut self.title_timer;
        *frames += 1;
        let elapsed = since.elapsed().as_secs_f32();
        if elapsed < 0.5 {
            return;
        }
        if let Some(window) = &self.window {
            let fps = *frames as f32 / elapsed;
            let s = self.ctx.stats;
            window.set_title(&format!(
                "{} | {:.0} fps ({:.2} ms) | {} draws, {} objects, {}k tris",
                self.config.title,
                fps,
                1000.0 / fps,
                s.draw_calls,
                s.instances,
                s.triangles / 1000
            ));
        }
        *since = Instant::now();
        *frames = 0;
    }

    /// One machine-readable line summarizing the run, for scripts and AI tools.
    fn report(&self) {
        if self.frame_times.is_empty() {
            return;
        }
        let mut sorted = self.frame_times.clone();
        sorted.sort_by(f32::total_cmp);
        let avg = sorted.iter().sum::<f32>() / sorted.len() as f32;
        let p95 = sorted[((sorted.len() as f32 * 0.95) as usize).min(sorted.len() - 1)];
        let s = self.ctx.stats;
        let gpu = self.renderer.as_ref().map(|r| r.adapter_info().name.clone()).unwrap_or_default();
        println!(
            "[grimora] frames={} avg_ms={avg:.2} p95_ms={p95:.2} max_ms={:.2} draw_calls={} objects={} triangles={} gpu=\"{gpu}\"",
            sorted.len(),
            sorted[sorted.len() - 1],
            s.draw_calls,
            s.instances,
            s.triangles
        );
    }
}

impl<G: Game> ApplicationHandler for App<G> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        if let Err(e) = self.start(event_loop) {
            self.error = Some(e);
            event_loop.exit();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let input = &mut self.ctx.input;
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                    self.ctx.aspect = renderer.aspect();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    match event.state {
                        ElementState::Pressed => input.key_down(code),
                        ElementState::Released => input.key_up(code),
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => match state {
                ElementState::Pressed => input.button_down(button),
                ElementState::Released => input.button_up(button),
            },
            WindowEvent::MouseWheel { delta, .. } => input.add_scroll(match delta {
                MouseScrollDelta::LineDelta(_, y) => y,
                MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
            }),
            WindowEvent::CursorMoved { position, .. } => {
                input.set_cursor(Vec2::new(position.x as f32, position.y as f32));
            }
            WindowEvent::Focused(false) => input.clear(),
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.frame(event_loop) {
                    self.error = Some(e);
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta } = event {
            self.ctx.input.add_mouse_delta(Vec2::new(delta.0 as f32, delta.1 as f32));
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}
