//! The sandbox's HUD: health and stamina top left, the compass top centre with the health of
//! the enemy in the fight under it, what's in the hand bottom left, the prompt of the object in
//! reach, notices at the bottom right and an F3 panel with frame time and world counters.
//! The hotbar is in `bag.rs`.

use grimora::prelude::*;
use grimora::render::{ART_PIXEL, ImageId};

const TEXT: Vec4 = Vec4::new(0.9, 0.87, 0.76, 1.0);
const DIM: Vec4 = Vec4::new(0.7, 0.68, 0.6, 0.9);
const PALE: Vec4 = Vec4::new(0.98, 0.95, 0.84, 1.0);
const WARN: Vec4 = Vec4::new(0.95, 0.5, 0.42, 1.0);
const GREEN: Vec4 = Vec4::new(0.55, 0.85, 0.5, 1.0);
const GOLD: Vec4 = Vec4::new(0.95, 0.8, 0.5, 1.0);
/// The dark of every panel.
const INK: Vec4 = Vec4::new(0.015, 0.018, 0.05, 0.86);
const LINE: Vec4 = Vec4::new(0.62, 0.46, 0.58, 1.0);
const TRACK: Vec4 = Vec4::new(0.01, 0.01, 0.03, 0.8);
/// The lines of the character's panel and of the bars.
const FRAME: Vec4 = Vec4::new(0.62, 0.46, 0.58, 1.0);
const VITALS_BACK: Vec4 = Vec4::new(0.02, 0.03, 0.12, 0.72);
const PORTRAIT_BACK: Vec4 = Vec4::new(0.45, 0.42, 0.68, 0.25);
const HEALTH: BarLook = BarLook {
    dark: Vec4::new(0.38, 0.08, 0.1, 1.0),
    bright: Vec4::new(0.76, 0.16, 0.19, 1.0),
    track: Vec4::new(0.12, 0.03, 0.03, 1.0),
};
const STAMINA: BarLook = BarLook {
    dark: Vec4::new(0.07, 0.33, 0.08, 1.0),
    bright: Vec4::new(0.25, 0.56, 0.3, 1.0),
    track: Vec4::new(0.05, 0.1, 0.04, 1.0),
};
const THIRD_BAR: BarLook = BarLook {
    dark: Vec4::new(0.07, 0.38, 0.38, 1.0),
    bright: Vec4::new(0.16, 0.66, 0.76, 1.0),
    track: Vec4::new(0.04, 0.05, 0.09, 1.0),
};
pub(crate) const WINDED_COLOR: Vec4 = Vec4::new(0.55, 0.85, 0.5, 1.0);
pub(crate) const FROZEN_COLOR: Vec4 = Vec4::new(0.6, 0.85, 1.0, 1.0);
pub(crate) const BURN_COLOR: Vec4 = Vec4::new(0.98, 0.55, 0.35, 1.0);
/// The part of a bar just lost, before it drains away.
const GHOST_ALPHA: f32 = 0.4;

const MARGIN: f32 = 12.0;
const SHEET_PAD: f32 = 6.0;
/// The character's panel, in the corner of the screen, with the portrait's frame and the
/// bars. Under the bars there's room left for icons.
const VITALS_SIZE: Vec2 = Vec2::new(330.0, 115.0);
const PORTRAIT: Rect = Rect { min: Vec2::new(11.0, 12.0), size: Vec2::new(72.0, 90.0) };
const BAR_LEFT: f32 = 92.0;
const BAR_TOP: f32 = 12.0;
const BAR_SIZE: Vec2 = Vec2::new(228.0, 12.0);
const BAR_ROW: f32 = 19.0;
/// How much brighter the health gets at the top of a pulse.
const PULSE_LIFT: f32 = 0.35;
/// Under this much health, the bar pulses.
const LOW_HEALTH: f32 = 0.25;
const PULSE_SPEED: f32 = 5.0;

/// Seconds that what was lost stays on the bar, then how much of the bar drains per second.
const GHOST_WAIT: f32 = 0.6;
const GHOST_DRAIN: f32 = 0.45;

const COMPASS_SIZE: Vec2 = Vec2::new(340.0, 20.0);
/// Degrees from the middle of the compass to its ends.
const COMPASS_SPAN: f32 = 100.0;
const COMPASS_TICK: i32 = 15;
const COMPASS_TOP: f32 = 10.0;
const FOE_TOP: f32 = 54.0;
const FOE_SIZE: Vec2 = Vec2::new(440.0, 8.0);

const NOTICE_SECONDS: f32 = 3.5;
/// Notices fade out over their last this many seconds.
const FADE_SECONDS: f32 = 0.6;
const MOST_NOTICES: usize = 4;
const NOTICE_ROW: f32 = 28.0;
/// How far the ends of a strip take to fade away.
pub(crate) const STRIP_FADE: f32 = 36.0;

/// The column of what's wrong with the character, on the left edge, in the middle.
const STATUS_SIDE: f32 = 32.0;
const STATUS_ROW: f32 = 40.0;
const STATUS_MARK: f32 = 24.0;
/// Seconds that a new one says its name, the last of them fading.
const STATUS_NAME_SECONDS: f32 = 2.5;

/// From the bottom of the screen: the prompt, over the hotbar.
const PROMPT_BOTTOM: f32 = 112.0;
/// How long the name of a new place stays, how long it takes to come and go, and how far down it is.
const AREA_SECONDS: f32 = 4.5;
const AREA_FADE: f32 = 0.9;
const AREA_HEIGHT: f32 = 0.24;

const EQUIPPED_SIDE: f32 = 56.0;

struct Notice {
    text: String,
    color: Vec4,
    shown_at: f32,
}

/// A bar's value as it shows, late when going down, so a hit can be seen and measured.
#[derive(Default, Clone, Copy)]
struct Ghost {
    shown: f32,
    wait: f32,
}

impl Ghost {
    fn follow(&mut self, value: f32, dt: f32) -> f32 {
        if value >= self.shown {
            self.shown = value;
            self.wait = GHOST_WAIT;
        } else if self.wait > 0.0 {
            self.wait -= dt;
        } else {
            self.shown = (self.shown - GHOST_DRAIN * dt).max(value);
        }
        self.shown
    }
}

/// The control that triggers `action` on whichever device the player is using.
pub fn first_control(input: &Input, action: &str) -> String {
    let gamepad = input.gamepad_last();
    let binding = input.actions.bindings(action).iter().find(|binding| binding.is_gamepad() == gamepad);
    binding.map_or_else(|| "?".to_string(), |binding| binding.label())
}

fn faded(color: Vec4, alpha: f32) -> Vec4 {
    Vec4::new(color.x, color.y, color.z, color.w * alpha)
}

/// The colors of a bar. The fill goes from `dark` to `bright` at its current end.
#[derive(Clone, Copy)]
struct BarLook {
    dark: Vec4,
    bright: Vec4,
    track: Vec4,
}

/// A bar: what's lost as `ghost`, what's left in its colors.
fn meter(ui: &mut Ui, rect: Rect, value: f32, ghost: f32, look: BarLook) {
    ui.panel(rect, look.track, FRAME);
    let inner = rect.expand(-ui.line());
    let part = |fraction: f32| Rect::new(inner.min, Vec2::new(inner.size.x * fraction.clamp(0.0, 1.0), inner.size.y));
    if ghost > value {
        ui.rect(part(ghost), faded(look.bright, GHOST_ALPHA));
    }
    if value > 0.0 {
        ui.gradient(part(value), look.dark, look.bright);
    }
}

/// A band of ink with ends that dissolve away, for what floats over the world without a frame.
pub(crate) fn strip(ui: &mut Ui, rect: Rect, alpha: f32) {
    let ink = faded(INK, alpha);
    let fade = STRIP_FADE.min(rect.size.x * 0.5);
    let middle = Rect::new(rect.min + Vec2::X * fade, Vec2::new(rect.size.x - fade * 2.0, rect.size.y));
    ui.dither(Rect::new(rect.min, Vec2::new(fade, rect.size.y)), faded(ink, 0.0), ink.w, false);
    ui.dither(middle, ink, ink.w, false);
    ui.dither(Rect::new(Vec2::new(middle.max().x, rect.min.y), Vec2::new(fade, rect.size.y)), ink, 0.0, false);
}

/// Something wrong with the character, for [`Hud::draw_statuses`].
pub struct Status {
    pub name: &'static str,
    /// Which of the marks of `bag_screen.rs`.
    pub mark: usize,
    pub color: Vec4,
    /// How much of it is left, `0..1`, if it ends on its own.
    pub left: Option<f32>,
}

/// What's in the hand, for the corner of the screen.
pub struct Equipped<'a> {
    pub icon: Option<ImageId>,
    pub name: &'a str,
    /// The lines under the name, each with its color.
    pub lines: &'a [(String, Vec4)],
    /// How much of the square, from the top, is covered: a cooldown or a reload.
    pub covered: f32,
    /// For what shows for a moment and goes away.
    pub alpha: f32,
}

/// The box of what's in the hand, in the bottom left corner.
pub fn draw_equipped(ui: &mut Ui, equipped: &Equipped<'_>) {
    let alpha = equipped.alpha;
    if alpha <= 0.0 {
        return;
    }
    let name = TextStyle::new(16.0).color(faded(PALE, alpha)).outline(faded(TextStyle::default().outline, alpha));
    let name_text = equipped.name.to_uppercase();
    let mut width = ui.measure(&name_text, &name).x;
    let body = TextStyle::new(16.0).outline(faded(TextStyle::default().outline, alpha));
    for (line, _) in equipped.lines {
        width = width.max(ui.measure(line, &body).x);
    }
    const ROW: f32 = 21.0;
    const GAP: f32 = 10.0;
    let height = EQUIPPED_SIDE.max(ROW * (equipped.lines.len() + 1) as f32 + 4.0) + SHEET_PAD * 2.0;
    let size = Vec2::new(EQUIPPED_SIDE + GAP + width + SHEET_PAD * 2.0 + 4.0, height);
    let at = ui.at(Anchor::BottomLeft, Vec2::new(MARGIN, -MARGIN));
    let panel = Rect::anchored(at, size, Anchor::BottomLeft);
    let ink = faded(INK, alpha);
    ui.dither(panel, ink, ink.w, false);
    ui.panel(panel, Vec4::ZERO, faded(LINE, alpha));

    let middle = panel.min.y + (height - EQUIPPED_SIDE) * 0.5;
    let square = Rect::new(Vec2::new(panel.min.x + SHEET_PAD, middle), Vec2::splat(EQUIPPED_SIDE));
    ui.panel(square, faded(TRACK, alpha), faded(LINE, alpha));
    if let Some(icon) = equipped.icon {
        ui.image(square.expand(-ART_PIXEL * 2.0), icon, Vec4::new(1.0, 1.0, 1.0, alpha));
    }
    if equipped.covered > 0.0 {
        let inner = square.expand(-ui.line());
        let cover = Vec2::new(inner.size.x, inner.size.y * equipped.covered.clamp(0.0, 1.0));
        ui.dither(Rect::new(inner.min, cover), Vec4::new(0.01, 0.01, 0.03, 0.8 * alpha), 0.8 * alpha, false);
    }

    let x = square.max().x + GAP;
    let total = ROW * (equipped.lines.len() + 1) as f32;
    let mut y = panel.min.y + (height - total) * 0.5 + 1.0;
    ui.text(Vec2::new(x, y), &name_text, &name);
    for (line, color) in equipped.lines {
        y += ROW;
        ui.text(Vec2::new(x, y), line, &body.color(faded(*color, alpha)));
    }
}

/// The colors that lines under a name usually have.
pub fn equipped_dim() -> Vec4 {
    DIM
}

pub fn equipped_warn() -> Vec4 {
    WARN
}

#[derive(Default)]
pub struct Hud {
    /// The character's face, drawn each frame into the frame in the corner.
    pub face: Option<grimora::render::ImageId>,
    /// The name of the place just found, and when.
    area: Option<(String, f32)>,
    stats: bool,
    notices: Vec<Notice>,
    /// Smoothed frame time in milliseconds.
    frame_ms: f32,
    health: Ghost,
    stamina: Ghost,
    foe: Ghost,
    /// When the enemy's bar last showed, as a bar for someone new shouldn't drain from the last one's.
    foe_seen: f32,
    /// What's being shown in the status column, and since when.
    status_since: Vec<(&'static str, f32)>,
    status_marks: Vec<ImageId>,
    /// A `--menu hud` run: everything at once, for a capture. Use `--frames 120`.
    pub demo: bool,
}

impl Hud {
    /// Shows `text` at the bottom right of the screen for a few seconds.
    pub fn notify(&mut self, ctx: &Context, text: impl Into<String>, ok: bool) {
        let text = text.into();
        let color = if ok { TEXT } else { WARN };
        self.notices.retain(|notice| notice.text != text);
        self.notices.push(Notice { text, color, shown_at: ctx.time.elapsed() });
        if self.notices.len() > MOST_NOTICES {
            self.notices.remove(0);
        }
    }

    /// Says the name of a place across the top, for a moment.
    pub fn show_area(&mut self, ctx: &Context, name: &str) {
        self.area = Some((name.to_uppercase(), ctx.time.elapsed()));
    }

    fn draw_area(&mut self, ctx: &mut Context) {
        let Some((name, since)) = &self.area else { return };
        let age = ctx.time.elapsed() - since;
        if age > AREA_SECONDS {
            self.area = None;
            return;
        }
        let alpha = (age / AREA_FADE).min((AREA_SECONDS - age) / AREA_FADE).clamp(0.0, 1.0);
        let ui = &mut ctx.ui;
        let style = TextStyle::new(32.0).color(faded(PALE, alpha)).outline(Vec4::ZERO).anchor(Anchor::Center);
        let at = Vec2::new(ui.size().x * 0.5, (ui.size().y * AREA_HEIGHT).floor());
        let size = ui.measure(name, &style) + Vec2::new(STRIP_FADE * 2.0 + 60.0, 22.0);
        strip(ui, Rect::anchored(at, size, Anchor::Center), alpha);
        ui.text(at, name, &style);
    }

    pub fn draw(&mut self, ctx: &mut Context, flying: bool) {
        self.draw_with_hint(ctx, "");
        self.draw_area(ctx);
        self.draw_compass(ctx);
        if flying {
            let walk = first_control(&ctx.input, crate::TOGGLE_FLY);
            let ui = &mut ctx.ui;
            let hint = format!("MODO VOO · WASD voar · botão direito olhar · Q/E descer/subir · {walk} andarilho");
            let style = TextStyle::new(16.0).color(GOLD).anchor(Anchor::Center).wrap(ui.size().x - 32.0);
            ui.text(ui.at(Anchor::Bottom, Vec2::new(0.0, -PROMPT_BOTTOM)), &hint, &style);
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

        if !hint.is_empty() {
            let ui = &mut ctx.ui;
            let style = TextStyle::new(16.0).color(DIM).anchor(Anchor::BottomLeft).wrap(ui.size().x - 32.0);
            let left = crate::menu::left_margin(ui);
            ui.text(ui.at(Anchor::BottomLeft, Vec2::new(left, -14.0)), hint, &style);
        }

        self.draw_notices(ctx);
        if self.stats {
            self.draw_stats(ctx);
        }
    }

    /// The player's bars, in the top left corner: no words, the color and size say which.
    /// The air shows only while it's being used or coming back.
    pub fn draw_vitals(&mut self, ctx: &mut Context, health: f32, stamina: Option<f32>, air: Option<f32>) {
        let dt = ctx.time.delta();
        let now = ctx.time.elapsed();
        let health_ghost = self.health.follow(health, dt);
        let stamina_ghost = stamina.map(|stamina| self.stamina.follow(stamina, dt));

        let ui = &mut ctx.ui;
        ui.rect(Rect::new(Vec2::ZERO, VITALS_SIZE), VITALS_BACK);
        ui.rect(PORTRAIT, PORTRAIT_BACK);
        if let Some(face) = self.face {
            ui.image(PORTRAIT, face, Vec4::ONE);
        }
        ui.panel(PORTRAIT, Vec4::ZERO, FRAME);

        let look = if health > 0.0 && health < LOW_HEALTH {
            let beat = ((now * PULSE_SPEED).sin() * 0.5 + 0.5) * PULSE_LIFT;
            let lift = |color: Vec4| color.lerp(Vec4::ONE, beat);
            BarLook { dark: lift(HEALTH.dark), bright: lift(HEALTH.bright), ..HEALTH }
        } else {
            HEALTH
        };
        let row = |n: f32| Rect::new(Vec2::new(BAR_LEFT, BAR_TOP + BAR_ROW * n), BAR_SIZE);
        meter(ui, row(0.0), health, health_ghost, look);
        if let (Some(stamina), Some(ghost)) = (stamina, stamina_ghost) {
            meter(ui, row(1.0), stamina, ghost, STAMINA);
        }
        if let Some(air) = air {
            meter(ui, row(2.0), air, air, THIRD_BAR);
        }
    }

    /// The column on the left edge: a mark for each thing that's wrong, with a line for how
    /// much of it is left. A new one says its name for a moment, then it's only the mark.
    pub fn draw_statuses(&mut self, ctx: &mut Context, statuses: &[Status]) {
        let now = ctx.time.elapsed();
        self.status_since.retain(|(name, _)| statuses.iter().any(|status| status.name == *name));
        for status in statuses {
            if !self.status_since.iter().any(|(name, _)| *name == status.name) {
                self.status_since.push((status.name, now));
            }
        }
        if statuses.is_empty() {
            return;
        }
        let ui = &mut ctx.ui;
        if self.status_marks.is_empty() {
            self.status_marks = crate::bag_screen::draw_marks(ui);
        }
        let total = STATUS_ROW * statuses.len() as f32 - (STATUS_ROW - STATUS_SIDE);
        let top = (ui.size().y - total) * 0.5;
        for (row, status) in statuses.iter().enumerate() {
            let square = Rect::new(Vec2::new(MARGIN, top + STATUS_ROW * row as f32), Vec2::splat(STATUS_SIDE));
            ui.dither(square, INK, INK.w, false);
            if let Some(mark) = self.status_marks.get(status.mark) {
                ui.image(
                    Rect::anchored(square.point(Anchor::Center), Vec2::splat(STATUS_MARK), Anchor::Center),
                    *mark,
                    status.color,
                );
            }
            if let Some(left) = status.left {
                let line = Vec2::new(STATUS_SIDE * left.clamp(0.0, 1.0), ART_PIXEL);
                ui.rect(Rect::new(Vec2::new(square.min.x, square.max().y), line), status.color);
            }
            let since = self.status_since.iter().find(|(name, _)| *name == status.name).map_or(now, |(_, at)| *at);
            let alpha = (STATUS_NAME_SECONDS - (now - since)).clamp(0.0, 1.0);
            if alpha > 0.0 {
                let style = TextStyle::new(16.0)
                    .color(faded(status.color, alpha))
                    .outline(faded(TextStyle::default().outline, alpha))
                    .anchor(Anchor::Left);
                let at = Vec2::new(square.max().x + 8.0, square.point(Anchor::Center).y);
                ui.text(at, &status.name.to_uppercase(), &style);
            }
        }
    }

    /// The compass, top centre: a strip of the horizon that turns with the camera. North is -Z.
    fn draw_compass(&mut self, ctx: &mut Context) {
        let forward = ctx.camera.forward();
        let heading = forward.x.atan2(-forward.z).to_degrees();
        let ui = &mut ctx.ui;
        let at = ui.at(Anchor::Top, Vec2::new(0.0, COMPASS_TOP));
        let strip_rect = Rect::anchored(at, COMPASS_SIZE, Anchor::Top);
        strip(ui, strip_rect, 0.9);
        let line = ui.line();
        let middle = strip_rect.point(Anchor::Center);
        let half = COMPASS_SIZE.x * 0.5;

        for step in (0..360).step_by(COMPASS_TICK as usize) {
            let off = (step as f32 - heading + 540.0).rem_euclid(360.0) - 180.0;
            if off.abs() > COMPASS_SPAN {
                continue;
            }
            let x = middle.x + off / COMPASS_SPAN * half;
            // Dissolves towards the ends, where the strip does.
            let edge = (1.0 - off.abs() / COMPASS_SPAN) * 4.0;
            let alpha = edge.clamp(0.0, 1.0);
            let letter = match step {
                0 => Some("N"),
                90 => Some("L"),
                180 => Some("S"),
                270 => Some("O"),
                _ => None,
            };
            if let Some(letter) = letter {
                let color = if step == 0 { WARN } else { PALE };
                let style = TextStyle::new(16.0)
                    .color(faded(color, alpha))
                    .anchor(Anchor::Center)
                    .outline(faded(TextStyle::default().outline, alpha));
                ui.text(Vec2::new(x, middle.y), letter, &style);
            } else {
                let tall = if step % 45 == 0 { 8.0 } else { 4.0 };
                let tick = Rect::anchored(Vec2::new(x, middle.y), Vec2::new(line, tall), Anchor::Center);
                ui.rect(tick, faded(LINE, alpha));
            }
        }
        // Where the camera is looking, a small mark pointing up from the bottom edge.
        let base = strip_rect.max().y + ART_PIXEL * 4.0;
        for row in 0..3 {
            let width = ART_PIXEL * (1 + row * 2) as f32;
            let top = base - ART_PIXEL * (3 - row) as f32;
            ui.rect(Rect::new(Vec2::new(middle.x - width * 0.5, top), Vec2::new(width, ART_PIXEL)), PALE);
        }
    }

    /// A mark on the locked enemy: the four corners of a square.
    pub fn draw_lock(&self, ctx: &mut Context, point: Vec3) {
        let ui = &mut ctx.ui;
        let size = ui.size();
        let clip = ctx.camera.view_projection(size.x / size.y.max(1.0)) * point.extend(1.0);
        if clip.w <= 0.0 {
            return;
        }
        let (x, y) = (clip.x / clip.w, clip.y / clip.w);
        let at = Vec2::new((x + 1.0) * 0.5 * size.x, (1.0 - y) * 0.5 * size.y);
        const SIDE: f32 = 18.0;
        const ARM: f32 = 6.0;
        let width = ART_PIXEL;
        let mark = Rect::anchored(at, Vec2::splat(SIDE), Anchor::Center);
        for (corner, sign) in [
            (mark.min, Vec2::new(1.0, 1.0)),
            (Vec2::new(mark.max().x - width, mark.min.y), Vec2::new(-1.0, 1.0)),
            (Vec2::new(mark.min.x, mark.max().y - width), Vec2::new(1.0, -1.0)),
            (mark.max() - width, Vec2::new(-1.0, -1.0)),
        ] {
            let across = if sign.x > 0.0 { corner.x } else { corner.x + width - ARM };
            let down = if sign.y > 0.0 { corner.y } else { corner.y + width - ARM };
            ui.rect(Rect::new(Vec2::new(across, corner.y), Vec2::new(ARM, width)), PALE);
            ui.rect(Rect::new(Vec2::new(corner.x, down), Vec2::new(width, ARM)), PALE);
        }
    }

    /// The enemy in the fight, under the compass.
    pub fn draw_foe(&mut self, ctx: &mut Context, name: &str, fraction: f32) {
        let now = ctx.time.elapsed();
        if now - self.foe_seen > 0.5 {
            self.foe = Ghost { shown: fraction, wait: GHOST_WAIT };
        }
        self.foe_seen = now;
        let ghost = self.foe.follow(fraction, ctx.time.delta());
        let ui = &mut ctx.ui;
        let at = ui.at(Anchor::Top, Vec2::new(0.0, FOE_TOP));
        ui.text(at, &name.to_uppercase(), &TextStyle::new(16.0).color(TEXT).anchor(Anchor::Top));
        let bar = Rect::anchored(at + Vec2::Y * 22.0, FOE_SIZE, Anchor::Top);
        meter(ui, bar, fraction, ghost, HEALTH);
    }

    /// A fight's worst moment, in a capture: a hit taken just now, with low health.
    pub fn draw_demo(&mut self, ctx: &mut Context) {
        let hit = ctx.time.frame() >= 100;
        if ctx.time.frame() == 100 {
            self.notify(ctx, "Maçã", true);
            self.notify(ctx, "Não cabe mais nada", false);
        }
        self.draw_vitals(ctx, if hit { 0.2 } else { 0.55 }, Some(if hit { 0.12 } else { 0.6 }), Some(0.5));
        self.draw_foe(ctx, "O Guardião da Lua", if hit { 0.62 } else { 0.8 });
        self.draw_statuses(
            ctx,
            &[
                Status { name: "Sem fôlego", mark: crate::bag_screen::MARK_BOLT, color: WINDED_COLOR, left: None },
                Status { name: "Congelado", mark: crate::bag_screen::MARK_SNOW, color: FROZEN_COLOR, left: Some(0.6) },
                Status { name: "Queimadura", mark: crate::bag_screen::MARK_FLAME, color: BURN_COLOR, left: Some(0.3) },
            ],
        );
        self.draw_prompt(ctx, "Abrir baú");
        let point = ctx.camera.position + ctx.camera.forward() * 8.0;
        self.draw_lock(ctx, point);
    }

    /// Forgets the notices on screen.
    pub fn clear_notice(&mut self) {
        self.notices.clear();
    }

    /// The key and what it does, for the object the player could use right now.
    pub fn draw_prompt(&self, ctx: &mut Context, prompt: &str) {
        let control = first_control(&ctx.input, actions::INTERACT).to_uppercase();
        let prompt = prompt.to_uppercase();
        let ui = &mut ctx.ui;
        let key = TextStyle::new(16.0).color(PALE).anchor(Anchor::Center);
        let label = TextStyle::new(16.0).color(TEXT).anchor(Anchor::Left);
        let (key_size, label_size) = (ui.measure(&control, &key), ui.measure(&prompt, &label));
        const GAP: f32 = 10.0;
        const KEY_PAD: f32 = 5.0;
        let key_width = (key_size.x + KEY_PAD * 2.0).max(22.0);
        let content = key_width + GAP + label_size.x;
        let at = ui.at(Anchor::Bottom, Vec2::new(0.0, -PROMPT_BOTTOM));
        strip(ui, Rect::anchored(at, Vec2::new(content + STRIP_FADE * 2.0 + 20.0, 28.0), Anchor::Center), 1.0);
        let left = at.x - content * 0.5;
        let box_ = Rect::anchored(Vec2::new(left, at.y), Vec2::new(key_width, 22.0), Anchor::Left);
        ui.panel(box_, Vec4::ZERO, LINE);
        ui.text(box_.point(Anchor::Center), &control, &key);
        ui.text(Vec2::new(left + key_width + GAP, at.y), &prompt, &label);
    }

    /// The latest things to say, newest at the bottom, on the free side of the screen.
    fn draw_notices(&mut self, ctx: &mut Context) {
        let now = ctx.time.elapsed();
        self.notices.retain(|notice| now - notice.shown_at <= NOTICE_SECONDS);
        let ui = &mut ctx.ui;
        let right = ui.at(Anchor::BottomRight, Vec2::new(-MARGIN - 8.0, -MARGIN - NOTICE_ROW * 0.5));
        for (row, notice) in self.notices.iter().rev().enumerate() {
            let fade = ((NOTICE_SECONDS - (now - notice.shown_at)) / FADE_SECONDS).min(1.0);
            let style = TextStyle::new(16.0)
                .color(faded(notice.color, fade))
                .outline(faded(TextStyle::default().outline, fade))
                .anchor(Anchor::Right);
            let at = right - Vec2::Y * NOTICE_ROW * row as f32;
            let width = ui.measure(&notice.text, &style).x + STRIP_FADE + 16.0;
            let band = Rect::anchored(at + Vec2::X * 8.0, Vec2::new(width, NOTICE_ROW - 4.0), Anchor::Right);
            let ink = faded(INK, fade);
            let flat = Rect::new(band.min + Vec2::X * STRIP_FADE, band.size - Vec2::X * STRIP_FADE);
            ui.dither(Rect::new(band.min, Vec2::new(STRIP_FADE, band.size.y)), faded(ink, 0.0), ink.w, false);
            ui.dither(flat, ink, ink.w, false);
            ui.text(at, &notice.text, &style);
        }
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
        let panel = Rect::new(Vec2::new(MARGIN, VITALS_SIZE.y + MARGIN), Vec2::new(WIDTH, height));
        ui.sheet(panel);

        let left = panel.min.x + PAD;
        let right = panel.max().x - PAD;
        let mut y = panel.min.y + PAD;
        let fps = 1000.0 / self.frame_ms.max(0.001);
        let budget = if self.frame_ms <= 1000.0 / 60.0 + 0.5 {
            GREEN
        } else if self.frame_ms <= 1000.0 / 30.0 {
            GOLD
        } else {
            WARN
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bar_waits_then_drains_to_what_is_left() {
        let mut ghost = Ghost::default();
        assert_eq!(ghost.follow(1.0, 0.1), 1.0);
        assert_eq!(ghost.follow(0.5, GHOST_WAIT * 0.5), 1.0, "what was lost still shows");
        ghost.follow(0.5, GHOST_WAIT);
        let draining = ghost.follow(0.5, 0.1);
        assert!(draining < 1.0 && draining > 0.5, "{draining}");
        for _ in 0..100 {
            ghost.follow(0.5, 0.1);
        }
        assert_eq!(ghost.follow(0.5, 0.1), 0.5, "stops at what is left");
        assert_eq!(ghost.follow(0.8, 0.1), 0.8, "going up shows at once");
    }
}
