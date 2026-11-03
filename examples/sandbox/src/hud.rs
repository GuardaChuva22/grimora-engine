//! The sandbox's HUD: the controls, notices, the prompt of the object in reach,
//! health bars and an F3 panel with frame time and world counters.

use grimora::prelude::*;

const TEXT: Vec4 = Vec4::new(0.95, 0.92, 0.86, 1.0);
const DIM: Vec4 = Vec4::new(0.78, 0.75, 0.7, 0.85);
const GOLD: Vec4 = Vec4::new(1.0, 0.78, 0.42, 1.0);
const RED: Vec4 = Vec4::new(1.0, 0.45, 0.38, 1.0);
const GREEN: Vec4 = Vec4::new(0.55, 0.85, 0.5, 1.0);
const PANEL: Vec4 = Vec4::new(0.05, 0.04, 0.06, 0.75);
const BORDER: Vec4 = Vec4::new(0.55, 0.43, 0.28, 0.9);
const HEALTH: Vec4 = Vec4::new(0.72, 0.13, 0.11, 1.0);
const STAMINA: Vec4 = Vec4::new(0.36, 0.56, 0.26, 1.0);
const NOTICE_SECONDS: f32 = 2.5;
/// Notices fade out over their last this many seconds.
const FADE_SECONDS: f32 = 0.6;

struct Notice {
    text: String,
    color: Vec4,
    shown_at: f32,
}

/// The control that triggers `action` on whichever device the player is using.
fn first_control(input: &Input, action: &str) -> String {
    let gamepad = input.gamepad_last();
    let binding = input.actions.bindings(action).iter().find(|binding| binding.is_gamepad() == gamepad);
    binding.map_or_else(|| "?".to_string(), |binding| binding.label())
}

/// What the keys do while playing, with the player's own controls.
fn play_hint(input: &Input) -> String {
    let control = |action| first_control(input, action);
    let (run, jump, attack) = (control(actions::RUN), control(actions::JUMP), control(actions::ATTACK));
    let (roll, lock, guard) = (control(actions::ROLL), control(actions::LOCK_ON), control(actions::GUARD));
    if input.gamepad_last() {
        return format!(
            "L andar · R olhar · {run} correr · {jump} pular · {attack} golpear · {guard} defender · {roll} rolar · {lock} travar alvo · Start pausar"
        );
    }
    let (forward, left, back, right) = (
        control(actions::MOVE_FORWARD),
        control(actions::MOVE_LEFT),
        control(actions::MOVE_BACK),
        control(actions::MOVE_RIGHT),
    );
    let (save, load) = (control(crate::QUICK_SAVE), control(crate::QUICK_LOAD));
    format!(
        "{forward}{left}{back}{right} andar · {run} correr · {jump} pular · {attack} golpear · {guard} defender · {roll} rolar · {lock} travar alvo · {save} salvar · {load} carregar · F4 câmera lenta · Esc pausar"
    )
}

#[derive(Default)]
pub struct Hud {
    stats: bool,
    notice: Option<Notice>,
    /// Smoothed frame time in milliseconds.
    frame_ms: f32,
}

impl Hud {
    /// Shows `text` at the top of the screen for a few seconds.
    pub fn notify(&mut self, ctx: &Context, text: impl Into<String>, ok: bool) {
        let color = if ok { GOLD } else { RED };
        self.notice = Some(Notice { text: text.into(), color, shown_at: ctx.time.elapsed() });
    }

    pub fn draw(&mut self, ctx: &mut Context, flying: bool) {
        let controls = if flying {
            let walk = first_control(&ctx.input, crate::TOGGLE_FLY);
            format!("WASD voar · botão direito olhar · Q/E descer/subir · Shift rápido · {walk} andarilho · Esc pausar")
        } else {
            play_hint(&ctx.input)
        };
        self.draw_with_hint(ctx, &controls);
        if flying {
            let ui = &mut ctx.ui;
            ui.text(
                ui.at(Anchor::Top, Vec2::new(0.0, 14.0)),
                "MODO VOO",
                &TextStyle::new(16.0).color(GOLD).anchor(Anchor::Top),
            );
        }
    }

    /// The part of the HUD every state has: `hint` at the bottom of the screen,
    /// notices and the F3 panel.
    pub fn draw_with_hint(&mut self, ctx: &mut Context, hint: &str) {
        if ctx.input.just_pressed(KeyCode::F3) {
            self.stats = !self.stats;
        }
        let ms = ctx.time.delta() * 1000.0;
        self.frame_ms = if self.frame_ms == 0.0 { ms } else { self.frame_ms + (ms - self.frame_ms) * 0.05 };

        let ui = &mut ctx.ui;
        let style = TextStyle::new(16.0).color(DIM).anchor(Anchor::Bottom).wrap(ui.size().x - 32.0);
        ui.text(ui.at(Anchor::Bottom, Vec2::new(0.0, -14.0)), hint, &style);

        self.draw_notice(ctx);
        if self.stats {
            self.draw_stats(ctx);
        }
    }

    /// The player's health, in the bottom left corner.
    pub fn draw_health(&self, ctx: &mut Context, fraction: f32) {
        let ui = &mut ctx.ui;
        let size = Vec2::new(240.0, 14.0);
        let at = ui.at(Anchor::BottomLeft, Vec2::new(18.0, -82.0));
        let bar = Rect::anchored(at, size, Anchor::BottomLeft);
        ui.panel(bar.expand(4.0), PANEL, BORDER);
        ui.bar(bar, fraction, HEALTH, Vec4::new(0.0, 0.0, 0.0, 0.6));
    }

    /// The player's stamina, under the health.
    pub fn draw_stamina(&self, ctx: &mut Context, fraction: f32) {
        let ui = &mut ctx.ui;
        let size = Vec2::new(200.0, 8.0);
        let at = ui.at(Anchor::BottomLeft, Vec2::new(18.0, -64.0));
        let bar = Rect::anchored(at, size, Anchor::BottomLeft);
        ui.panel(bar.expand(3.0), PANEL, BORDER);
        ui.bar(bar, fraction, STAMINA, Vec4::new(0.0, 0.0, 0.0, 0.6));
    }

    /// A mark on the locked enemy.
    pub fn draw_lock(&self, ctx: &mut Context, point: Vec3) {
        let ui = &mut ctx.ui;
        let size = ui.size();
        let clip = ctx.camera.view_projection(size.x / size.y.max(1.0)) * point.extend(1.0);
        if clip.w <= 0.0 {
            return;
        }
        let (x, y) = (clip.x / clip.w, clip.y / clip.w);
        let at = Vec2::new((x + 1.0) * 0.5 * size.x, (1.0 - y) * 0.5 * size.y);
        ui.panel(Rect::anchored(at, Vec2::splat(7.0), Anchor::Center), GOLD, PANEL);
    }

    /// The health of the enemy in the fight, at the top of the screen.
    pub fn draw_foe(&self, ctx: &mut Context, name: &str, fraction: f32) {
        let ui = &mut ctx.ui;
        let at = ui.at(Anchor::Top, Vec2::new(0.0, 14.0));
        ui.text(at, name, &TextStyle::new(16.0).color(TEXT).anchor(Anchor::Top));
        let bar = Rect::anchored(at + Vec2::Y * 24.0, Vec2::new(320.0, 10.0), Anchor::Top);
        ui.panel(bar.expand(3.0), PANEL, BORDER);
        ui.bar(bar, fraction, HEALTH, Vec4::new(0.0, 0.0, 0.0, 0.6));
    }

    /// Forgets the notice on screen.
    pub fn clear_notice(&mut self) {
        self.notice = None;
    }

    /// The key and what it does, for the object the player could use right now.
    pub fn draw_prompt(&self, ctx: &mut Context, prompt: &str) {
        let control = first_control(&ctx.input, actions::INTERACT);
        let ui = &mut ctx.ui;
        let key = TextStyle::new(24.0).color(GOLD).anchor(Anchor::Left);
        let label = TextStyle::new(24.0).color(TEXT).anchor(Anchor::Left);
        let (key_size, label_size) = (ui.measure(&control, &key), ui.measure(prompt, &label));
        const GAP: f32 = 12.0;
        let size = Vec2::new(key_size.x + GAP + label_size.x, key_size.y.max(label_size.y));
        let at = ui.at(Anchor::Bottom, Vec2::new(0.0, -72.0));
        let box_ = Rect::anchored(at, size, Anchor::Center);
        ui.panel(box_.expand(10.0), PANEL, BORDER);
        let left = box_.point(Anchor::Left);
        ui.text(left, &control, &key);
        ui.text(left + Vec2::X * (key_size.x + GAP), prompt, &label);
    }

    fn draw_notice(&mut self, ctx: &mut Context) {
        let Some(notice) = &self.notice else { return };
        let age = ctx.time.elapsed() - notice.shown_at;
        if age > NOTICE_SECONDS {
            self.notice = None;
            return;
        }
        let fade = ((NOTICE_SECONDS - age) / FADE_SECONDS).min(1.0);
        let ui = &mut ctx.ui;
        let style = TextStyle::new(24.0).color(notice.color * Vec4::new(1.0, 1.0, 1.0, fade)).anchor(Anchor::Center);
        let at = ui.at(Anchor::Top, Vec2::new(0.0, 72.0));
        let size = ui.measure(&notice.text, &style);
        let back = Rect::anchored(at, size, Anchor::Center).expand(12.0);
        ui.panel(back, PANEL * Vec4::new(1.0, 1.0, 1.0, fade), BORDER * Vec4::new(1.0, 1.0, 1.0, fade));
        ui.text(at, &notice.text, &style);
    }

    fn draw_stats(&self, ctx: &mut Context) {
        let render = ctx.render_stats();
        let mut rows = vec![
            ("desenhos", render.draw_calls.to_string()),
            ("objetos", render.instances.to_string()),
            ("fora de vista", render.culled.to_string()),
            ("sombras", render.shadow_casters.to_string()),
            ("triângulos", format!("{}k", render.triangles / 1000)),
            ("partículas", render.particles.to_string()),
            ("entidades", ctx.world.ecs.len().to_string()),
        ];
        if let Some(s) = ctx.world.streaming_stats() {
            rows.push(("células", format!("{}/{}", s.loaded_cells, s.cells)));
        }
        let areas: Vec<&str> = ctx.world.trigger_state.inside.iter().map(String::as_str).collect();
        rows.push(("área", if areas.is_empty() { "-".to_owned() } else { areas.join(", ") }));

        const WIDTH: f32 = 260.0;
        const ROW: f32 = 20.0;
        const PAD: f32 = 12.0;
        let ui = &mut ctx.ui;
        let height = PAD * 2.0 + ROW * 2.0 + 10.0 + ROW * rows.len() as f32;
        let panel = Rect::new(Vec2::splat(14.0), Vec2::new(WIDTH, height));
        ui.panel(panel, PANEL, BORDER);

        let left = panel.min.x + PAD;
        let right = panel.max().x - PAD;
        let mut y = panel.min.y + PAD;
        let fps = 1000.0 / self.frame_ms.max(0.001);
        let budget = if self.frame_ms <= 1000.0 / 60.0 + 0.5 {
            GREEN
        } else if self.frame_ms <= 1000.0 / 30.0 {
            GOLD
        } else {
            RED
        };
        ui.text(Vec2::new(left, y), &format!("{fps:.0} fps"), &TextStyle::new(16.0).color(budget));
        ui.text(
            Vec2::new(right, y),
            &format!("{:.2} ms", self.frame_ms),
            &TextStyle::new(16.0).anchor(Anchor::TopRight),
        );
        y += ROW;
        // The bar fills at 33 ms (30 fps); the notch marks 16.7 ms (60 fps).
        let bar = Rect::new(Vec2::new(left, y), Vec2::new(right - left, 8.0));
        ui.bar(bar, self.frame_ms / (2000.0 / 60.0), budget, Vec4::new(0.0, 0.0, 0.0, 0.6));
        ui.rect(Rect::new(Vec2::new(bar.point(Anchor::Center).x, y - 2.0), Vec2::new(1.0, 12.0)), DIM);
        y += ROW;

        let label = TextStyle::new(16.0).color(DIM);
        let value = TextStyle::new(16.0).color(TEXT).anchor(Anchor::TopRight);
        y += 10.0;
        for (name, text) in rows {
            ui.text(Vec2::new(left, y), name, &label);
            ui.text(Vec2::new(right, y), &text, &value);
            y += ROW;
        }
    }
}
