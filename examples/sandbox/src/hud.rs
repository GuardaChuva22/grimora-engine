//! The sandbox's HUD: the controls, save/load notices and an F3 panel with
//! frame time and world counters.

use grimora::prelude::*;

const TEXT: Vec4 = Vec4::new(0.95, 0.92, 0.86, 1.0);
const DIM: Vec4 = Vec4::new(0.78, 0.75, 0.7, 0.85);
const GOLD: Vec4 = Vec4::new(1.0, 0.78, 0.42, 1.0);
const RED: Vec4 = Vec4::new(1.0, 0.45, 0.38, 1.0);
const GREEN: Vec4 = Vec4::new(0.55, 0.85, 0.5, 1.0);
const PANEL: Vec4 = Vec4::new(0.05, 0.04, 0.06, 0.75);
const BORDER: Vec4 = Vec4::new(0.55, 0.43, 0.28, 0.9);
const NOTICE_SECONDS: f32 = 2.5;
/// Notices fade out over their last this many seconds.
const FADE_SECONDS: f32 = 0.6;

struct Notice {
    text: String,
    color: Vec4,
    shown_at: f32,
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
        if ctx.input.just_pressed(KeyCode::F3) {
            self.stats = !self.stats;
        }
        let ms = ctx.time.delta() * 1000.0;
        self.frame_ms = if self.frame_ms == 0.0 { ms } else { self.frame_ms + (ms - self.frame_ms) * 0.05 };

        let controls = if flying {
            "WASD voar · botão direito olhar · Q/E descer/subir · Shift rápido · Tab andarilho"
        } else {
            "WASD andar · Shift correr · Espaço pular · Tab voar · F6 salvar · F9 carregar · F3 métricas"
        };
        let ui = &mut ctx.ui;
        let style = TextStyle::new(16.0).color(DIM).anchor(Anchor::Bottom).wrap(ui.size().x - 32.0);
        ui.text(ui.at(Anchor::Bottom, Vec2::new(0.0, -14.0)), controls, &style);
        if flying {
            ui.text(
                ui.at(Anchor::Top, Vec2::new(0.0, 14.0)),
                "MODO VOO",
                &TextStyle::new(16.0).color(GOLD).anchor(Anchor::Top),
            );
        }

        self.draw_notice(ctx);
        if self.stats {
            self.draw_stats(ctx);
        }
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
            ("entidades", ctx.world.ecs.len().to_string()),
        ];
        if let Some(s) = ctx.world.streaming_stats() {
            rows.push(("células", format!("{}/{}", s.loaded_cells, s.cells)));
        }

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
