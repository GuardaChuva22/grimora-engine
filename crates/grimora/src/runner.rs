use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context as _, Result, bail};
use grimora_audio::Audio;
use grimora_core::{Animator, Camera, Input, KeyCode, Time, Transform, Vec2};
use grimora_physics::Physics;
use grimora_render::{DrawItem, Frame, FrameStats, MeshHandle, Renderer, RendererSettings, Ui};
use grimora_scene::Scene;
use serde::Serialize;
use serde::de::DeserializeOwned;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::{CursorGrabMode, Window, WindowId};

use crate::save::{SAVE_FORMAT, SaveFile, WorldState};
use crate::{EngineConfig, Game, Renderable, SaveStore, World};

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
    /// Sound. Scene loops are replaced on every (re)load; loops the game starts
    /// stay until it stops them. The listener follows `camera` after `update`.
    pub audio: Audio,
    /// Where [`Context::save_game`] and [`Context::load_game`] keep their slots.
    pub saves: SaveStore,
    /// HUD and menus: text, panels and bars drawn over the frame. Cleared
    /// before every `update`, so draw it each frame.
    pub ui: Ui,
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

    /// Saves `game` (the game's own state) to `slot`, together with the camera
    /// and the scene objects that moved or were hidden. Returns the file written.
    pub fn save_game<T: Serialize>(&self, slot: &str, game: &T) -> Result<PathBuf> {
        let world = WorldState::capture(&self.world, &self.physics, &self.camera);
        let path = self.saves.write(slot, &SaveFile::new(self.config.scene.as_deref(), world, game))?;
        log::info!("Saved {}", path.display());
        Ok(path)
    }

    /// Loads `slot`: puts the camera and scene objects back as they were saved
    /// and returns the game's own state for it to apply. Fails, changing
    /// nothing, if the file is missing or broken or was saved in another scene.
    pub fn load_game<T: DeserializeOwned>(&mut self, slot: &str) -> Result<T> {
        let file: SaveFile<T> = self.saves.read(slot)?;
        if file.format > SAVE_FORMAT {
            bail!("save slot {slot:?} is from a newer engine (format {})", file.format);
        }
        let current = self.config.scene.as_deref().and_then(Path::file_name).map(|n| n.to_string_lossy());
        if let (Some(saved), Some(current)) = (&file.scene, &current)
            && saved != current
        {
            bail!("save slot {slot:?} was made in {saved}, but {current} is loaded");
        }
        let unmatched = file.world.restore(&mut self.world, &mut self.physics, &mut self.camera);
        if unmatched > 0 {
            log::warn!("{unmatched} saved objects no longer match the scene");
        }
        self.audio.set_scene_sounds(&self.world.active_sounds());
        log::info!("Loaded save slot {slot:?}");
        Ok(file.game)
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
            audio: start_audio(&config),
            saves: config.save_dir.clone().map_or_else(|| SaveStore::for_game(&config.title), SaveStore::new),
            ui: Ui::default(),
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

/// Screenshot runs mix without a device, so automated captures stay silent.
fn start_audio(config: &EngineConfig) -> Audio {
    if !config.audio {
        Audio::silent()
    } else if config.screenshot.is_some() {
        log::info!("Audio: headless (screenshot run)");
        Audio::headless()
    } else {
        Audio::new()
    }
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
        self.ctx.physics = Physics::new();
        self.ctx.world = World::from_scene(scene, &mut self.ctx.physics);
        log::info!(
            "Built {} entities and {} colliders in {:.0} ms",
            self.ctx.world.ecs.len(),
            self.ctx.physics.collider_count(),
            started.elapsed().as_secs_f64() * 1000.0
        );
        if self.ctx.world.streaming_stats().is_some() {
            let started = Instant::now();
            self.ctx.world.load_around(self.ctx.camera.position, &mut self.ctx.physics);
            if let Some(s) = self.ctx.world.streaming_stats() {
                log::info!(
                    "Streaming: {}/{} cells and {}/{} regions loaded ({} objects) in {:.0} ms",
                    s.loaded_cells,
                    s.cells,
                    s.loaded_regions,
                    s.regions,
                    s.objects,
                    started.elapsed().as_secs_f64() * 1000.0
                );
            }
        }
        self.ctx.audio.set_scene_sounds(&self.ctx.world.active_sounds());
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
        let (size, scale) = match &self.window {
            Some(window) => {
                let scale = window.scale_factor() as f32;
                let size = window.inner_size();
                (Vec2::new(size.width as f32, size.height as f32) / scale, scale)
            }
            None => (self.ctx.ui.size(), self.ctx.ui.scale()),
        };
        self.ctx.ui.begin(size, scale);
        self.game.update(&mut self.ctx);
        if !self.config.ui {
            self.ctx.ui.begin(size, scale);
        }
        self.ctx.world.update_animations(self.ctx.time.delta());
        if self.ctx.world.update_streaming(self.ctx.camera.position, &mut self.ctx.physics) {
            self.ctx.audio.set_scene_sounds(&self.ctx.world.active_sounds());
        }
        self.ctx.audio.follow_camera(&self.ctx.camera);
        self.ctx.audio.update();
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
        let mut joint_matrices = Vec::new();
        let mut query = world.ecs.query::<(&Transform, &Renderable, Option<&Animator>)>();
        let draws: Vec<DrawItem> = query
            .iter()
            .filter(|(_, r, _)| r.visible)
            .filter_map(|(transform, r, animator)| {
                let mesh = *self.handles.get(r.mesh.0)?;
                let joints = animator.map(|a| {
                    joint_matrices.extend_from_slice(a.skinning_matrices());
                    (joint_matrices.len() - a.skinning_matrices().len()) as u32
                });
                Some(DrawItem { mesh, transform: *transform, color: r.color, emissive: r.emissive, joints })
            })
            .collect();
        drop(query);
        let frame = Frame {
            camera: &self.ctx.camera,
            environment: &world.environment,
            lights: &world.active_lights(),
            draws: &draws,
            joint_matrices: &joint_matrices,
            time: self.ctx.time.elapsed(),
            ui: &self.ctx.ui,
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
            "[grimora] frames={} avg_ms={avg:.2} p95_ms={p95:.2} max_ms={:.2} draw_calls={} objects={} culled={} shadow_casters={} triangles={} gpu=\"{gpu}\"",
            sorted.len(),
            sorted[sorted.len() - 1],
            s.draw_calls,
            s.instances,
            s.culled,
            s.shadow_casters,
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
        // Raw motion arrives even when the window is in the background; ignore it
        // then, and in screenshot runs so they don't depend on the real mouse.
        let focused = self.window.as_ref().is_some_and(|w| w.has_focus());
        if let DeviceEvent::MouseMotion { delta } = event
            && focused
            && self.config.screenshot.is_none()
        {
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
