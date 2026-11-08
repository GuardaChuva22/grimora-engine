//! Resting at a fire, which heals, passes the hours and is where the character will wake up;
//! and the dark that comes with falling and lifts on waking.

use grimora::prelude::*;

use crate::Sandbox;
use crate::menu::menu_list;
use crate::tale::Panel;

/// What the fire offers, and the hour it goes to.
const HOURS: [(&str, f32); 4] =
    [("Até o amanhecer", 6.0), ("Até o meio-dia", 12.0), ("Até o anoitecer", 19.0), ("Até a meia-noite", 0.0)];
const ROW: Vec2 = Vec2::new(360.0, 32.0);
const FADE_SECONDS: f32 = 0.9;
const WAKE_SECONDS: f32 = 1.4;
const NIGHT: Vec4 = Vec4::new(0.004, 0.005, 0.02, 1.0);
const FALL_DARK: f32 = 0.92;
const FALL_DARK_FROM: f32 = 0.5;
const FALL_DARK_TO: f32 = 2.6;
const WORDS_FROM: f32 = 1.0;
const WORDS_TO: f32 = 2.2;
const WORDS: Vec4 = Vec4::new(0.86, 0.3, 0.28, 1.0);
const WORDS_SIZE: f32 = 48.0;
const HINT: &str = "ENTER  ESCOLHER    ESC  SAIR";
const HINT_PAD: &str = "A  ESCOLHER    B  SAIR";
const DIM: Vec4 = Vec4::new(0.7, 0.68, 0.6, 0.9);

/// The fire's screen: the choice, or the dark falling for the hour chosen.
#[derive(Debug, Default)]
pub struct Resting {
    falling: Option<(f32, f32)>,
}

fn smooth(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn dark(ui: &mut Ui, alpha: f32) {
    if alpha > 0.0 {
        let screen = ui.screen();
        ui.dither(screen, Vec4::new(NIGHT.x, NIGHT.y, NIGHT.z, alpha), alpha, false);
    }
}

impl Sandbox {
    pub(crate) fn run_rest(&mut self, ctx: &mut Context, mut resting: Resting) -> Option<Panel> {
        if let Some((seconds, hour)) = &mut resting.falling {
            *seconds += ctx.time.delta();
            dark(&mut ctx.ui, (*seconds / FADE_SECONDS).min(1.0));
            if *seconds < FADE_SECONDS {
                return Some(Panel::Rest(resting));
            }
            let hour = *hour;
            self.rest_until(ctx, hour);
            return None;
        }
        if ctx.input.pad_just_pressed(PadButton::East) {
            return None;
        }
        ctx.ui.theme.text_size = 16.0;
        let rows = HOURS.len() as u32 + 1;
        let mut column = menu_list(&mut ctx.ui, "Descansar", rows, ROW);
        for (name, hour) in HOURS {
            if ctx.ui.button(column.row(), name) {
                resting.falling = Some((0.0, hour));
            }
        }
        if ctx.ui.button(column.row(), "Levantar") {
            return None;
        }
        let keys = if ctx.input.gamepad_last() { HINT_PAD } else { HINT };
        let style = TextStyle::new(8.0).color(DIM);
        ctx.ui.text(Vec2::new(column.x(), column.y() + 8.0), keys, &style);
        Some(Panel::Rest(resting))
    }

    /// Everything that a night's sleep does.
    fn rest_until(&mut self, ctx: &mut Context, hour: f32) {
        let now = ctx.world.day_cycle.as_ref().map(|day| day.hour());
        if let Some(now) = now {
            let days = (hour - now).rem_euclid(24.0) / 24.0;
            ctx.world.set_time_of_day(hour);
            ctx.world.pass_fauna_days(days, ctx.camera.position);
        }
        if let Some(health) = ctx.world.get_mut::<Health>(self.avatar) {
            health.current = health.max;
        }
        if let Some(stamina) = ctx.world.get_mut::<Stamina>(self.avatar) {
            stamina.current = stamina.max;
        }
        self.burned_for = 0.0;
        self.winded = false;
        if let Some(player) = &self.player {
            self.tale.respawn = Some((ctx.origin().to_world(player.body.position), player.facing));
        }
        self.tale.waking = 1.0;
        self.hud.notify(ctx, "Você descansa", true);
    }

    /// The dark over the world when the character falls, and as it lifts when they wake.
    pub(crate) fn draw_dark(&mut self, ctx: &mut Context) {
        if let Some(dead_for) = self.dead_for {
            dark(&mut ctx.ui, smooth(FALL_DARK_FROM, FALL_DARK_TO, dead_for) * FALL_DARK);
            let words = smooth(WORDS_FROM, WORDS_TO, dead_for);
            if words > 0.0 {
                let color = Vec4::new(WORDS.x, WORDS.y, WORDS.z, words);
                let style = TextStyle::new(WORDS_SIZE).color(color).outline(Vec4::ZERO).anchor(Anchor::Center);
                ctx.ui.text(ctx.ui.at(Anchor::Center, Vec2::ZERO), "VOCÊ CAIU", &style);
            }
        } else if self.tale.waking > 0.0 {
            self.tale.waking = (self.tale.waking - ctx.time.delta() / WAKE_SECONDS).max(0.0);
            dark(&mut ctx.ui, smooth(0.0, 1.0, self.tale.waking) * FALL_DARK);
        }
    }
}
