//! What can be used, marked in the world: a small dot over each thing near the player, and,
//! over the one E would use, a mark of what it is with the key and what it does beside it.

use grimora::prelude::*;
use grimora::render::{ART_PIXEL, ImageId, Ui};
use grimora::{InteractKind, Nearby};

use crate::hud::first_control;

/// Meters from the player that things get their dot.
const REACH: f32 = 6.0;
/// Closer than this the dot is whole; it fades out from here to [`REACH`].
const WHOLE: f32 = 3.5;
/// How much worse a new pick must not be for the one already chosen to stay chosen, so the
/// choice doesn't flicker between two things side by side.
const STICK: f32 = 0.35;
/// How fast marks come and go, per second.
const EASE: f32 = 12.0;
/// The mark of the chosen thing, a whole number of art pixels per texel.
const MARK_SIDE: f32 = 12.0 * ART_PIXEL;
/// How far the chosen mark rises while it comes in.
const RISE: f32 = 6.0;
const GAP: f32 = 8.0;

const PALE: Vec4 = Vec4::new(0.98, 0.95, 0.84, 1.0);
const TEXT: Vec4 = Vec4::new(0.9, 0.87, 0.76, 1.0);
const LINE: Vec4 = Vec4::new(0.62, 0.46, 0.58, 1.0);
const INK: Vec4 = Vec4::new(0.015, 0.018, 0.05, 0.9);
/// Someone else's: taking it is theft.
const THEFT: Vec4 = Vec4::new(0.95, 0.5, 0.42, 1.0);

type Mark = [&'static str; 12];

#[rustfmt::skip]
const TAKE: Mark = [
    "....X.X.....",
    "..X.X.X.X...",
    "..X.X.X.X...",
    "..X.X.X.X...",
    "..XXXXXXX.X.",
    "..XXXXXXX.X.",
    "..XXXXXXXXX.",
    "..XXXXXXXX..",
    "..XXXXXXXX..",
    "...XXXXXX...",
    "....XXXXX...",
    "....XXXXX...",
];
#[rustfmt::skip]
const TALK: Mark = [
    "............",
    "..XXXXXXXX..",
    ".XXXXXXXXXX.",
    "XXXXXXXXXXXX",
    "XX..X..X..XX",
    "XX..X..X..XX",
    "XXXXXXXXXXXX",
    ".XXXXXXXXXX.",
    "..XXXXXXXX..",
    "..XXX.......",
    "..XX........",
    "..X.........",
];
#[rustfmt::skip]
const DOOR: Mark = [
    "...XXXXXX...",
    "..XXXXXXXX..",
    ".XXX....XXX.",
    ".XX......XX.",
    ".XX......XX.",
    ".XX......XX.",
    ".XX....X.XX.",
    ".XX......XX.",
    ".XX......XX.",
    ".XX......XX.",
    ".XX......XX.",
    "XXXXXXXXXXXX",
];
#[rustfmt::skip]
const CHEST: Mark = [
    "............",
    "..XXXXXXXX..",
    ".XXXXXXXXXX.",
    ".XXXXXXXXXX.",
    ".XXXXXXXXXX.",
    "............",
    "XXXXX..XXXXX",
    "XXXXX..XXXXX",
    "XXXXXXXXXXXX",
    "XXXXXXXXXXXX",
    "XXXXXXXXXXXX",
    "............",
];
#[rustfmt::skip]
const USE: Mark = [
    ".....XX.....",
    ".....XX.....",
    "....XXXX....",
    "....XXXX....",
    "..XXXXXXXX..",
    "XXXXXXXXXXXX",
    "XXXXXXXXXXXX",
    "..XXXXXXXX..",
    "....XXXX....",
    "....XXXX....",
    ".....XX.....",
    ".....XX.....",
];

fn mark_of(kind: InteractKind) -> usize {
    match kind {
        InteractKind::Take => 0,
        InteractKind::Talk => 1,
        InteractKind::Door => 2,
        InteractKind::Chest => 3,
        InteractKind::Use => 4,
    }
}

/// One thing with a mark on screen, also while it fades away.
struct Shown {
    entity: Entity,
    at: Vec3,
    kind: InteractKind,
    owned: bool,
    prompt: String,
    /// How much it shows, 0..1.
    alpha: f32,
    /// How much it is the chosen one, 0..1.
    chosen: f32,
    /// Among the things near this frame.
    here: bool,
    /// How near, 0..1 (1 within [`WHOLE`]).
    near: f32,
}

#[derive(Default)]
pub struct Prompts {
    marks: Vec<ImageId>,
    shown: Vec<Shown>,
    chosen: Option<Entity>,
}

impl Prompts {
    /// Finds what is near `center` (the middle of the player's body), draws the marks and
    /// returns what E would use now.
    pub fn update(&mut self, ctx: &mut Context, center: Vec3) -> Option<Nearby> {
        let near = ctx.world.interactables_near(center, ctx.camera.position, ctx.camera.forward(), REACH, &ctx.physics);

        // The best in reach, unless the one already chosen is about as good.
        let best = near.iter().filter(|n| n.in_reach).min_by(|a, b| a.score.total_cmp(&b.score));
        let kept = self.chosen.and_then(|entity| near.iter().find(|n| n.entity == entity && n.in_reach));
        let chosen = match (kept, best) {
            (Some(kept), Some(best)) if kept.score <= best.score + STICK => Some(kept),
            (_, best) => best,
        };
        self.chosen = chosen.map(|n| n.entity);

        for shown in &mut self.shown {
            shown.here = false;
        }
        for thing in &near {
            let nearness = 1.0 - ((thing.mark.distance(center) - WHOLE) / (REACH - WHOLE)).clamp(0.0, 1.0);
            let index = match self.shown.iter().position(|s| s.entity == thing.entity) {
                Some(index) => index,
                None => {
                    self.shown.push(Shown {
                        entity: thing.entity,
                        at: thing.mark,
                        kind: thing.kind,
                        owned: thing.owned,
                        prompt: String::new(),
                        alpha: 0.0,
                        chosen: 0.0,
                        here: true,
                        near: 0.0,
                    });
                    self.shown.len() - 1
                }
            };
            let shown = &mut self.shown[index];
            shown.at = thing.mark;
            shown.kind = thing.kind;
            shown.owned = thing.owned;
            shown.prompt.clone_from(&thing.prompt);
            shown.here = true;
            shown.near = nearness;
        }

        let step = (ctx.time.delta() * EASE).min(1.0);
        for shown in &mut self.shown {
            let alpha = if shown.here { shown.near } else { 0.0 };
            let chosen = if shown.here && Some(shown.entity) == self.chosen { 1.0 } else { 0.0 };
            shown.alpha += (alpha.max(chosen) - shown.alpha) * step;
            shown.chosen += (chosen - shown.chosen) * step;
        }
        self.shown.retain(|shown| shown.here || shown.alpha > 0.01);

        self.draw(ctx);
        chosen.cloned()
    }

    fn draw(&mut self, ctx: &mut Context) {
        let key = first_control(&ctx.input, actions::INTERACT).to_uppercase();
        let ui = &mut ctx.ui;
        if self.marks.is_empty() {
            self.marks = [&TAKE, &TALK, &DOOR, &CHEST, &USE].iter().filter_map(|rows| image(ui, rows)).collect();
        }
        let size = ui.size();
        let view_projection = ctx.camera.view_projection(size.x / size.y.max(1.0));
        // The chosen one last, over the dots.
        let mut order: Vec<&Shown> = self.shown.iter().collect();
        order.sort_by(|a, b| a.chosen.total_cmp(&b.chosen));
        for shown in order {
            let clip = view_projection * shown.at.extend(1.0);
            if clip.w <= 0.0 || shown.alpha <= 0.01 {
                continue;
            }
            let at = Vec2::new((clip.x / clip.w + 1.0) * 0.5 * size.x, (1.0 - clip.y / clip.w) * 0.5 * size.y);
            // On whole art pixels, so the marks stay crisp.
            let at = (at / ART_PIXEL).round() * ART_PIXEL;
            let color = if shown.owned { THEFT } else { PALE };

            let dot = shown.alpha * (1.0 - shown.chosen);
            if dot > 0.01 {
                draw_dot(ui, at, color, dot * 0.85);
            }
            let chosen = shown.alpha * shown.chosen;
            if chosen <= 0.01 {
                continue;
            }
            let Some(&mark) = self.marks.get(mark_of(shown.kind)) else { continue };
            let middle = at - Vec2::Y * (RISE * shown.chosen + MARK_SIDE * 0.5 - RISE);
            let square = Rect::anchored(middle, Vec2::splat(MARK_SIDE), Anchor::Center);
            // A dark copy around it is its outline.
            for offset in [Vec2::X, -Vec2::X, Vec2::Y, -Vec2::Y] {
                let around = Rect::new(square.min + offset * ART_PIXEL, square.size);
                ui.image(around, mark, faded(INK, chosen));
            }
            ui.image(square, mark, faded(color, chosen));

            let key_style = TextStyle::new(16.0).color(faded(PALE, chosen)).anchor(Anchor::Center);
            let label = shown.prompt.to_uppercase();
            let label_style = TextStyle::new(16.0).color(faded(TEXT, chosen)).anchor(Anchor::Left);
            let key_width = (ui.measure(&key, &key_style).x + 10.0).max(22.0);
            let left = square.max().x + GAP;
            let box_ = Rect::anchored(Vec2::new(left, middle.y), Vec2::new(key_width, 22.0), Anchor::Left);
            ui.panel(box_, faded(INK, chosen * 0.6), faded(LINE, chosen));
            ui.text(box_.point(Anchor::Center), &key, &key_style);
            ui.text(Vec2::new(box_.max().x + GAP, middle.y), &label, &label_style);
        }
    }
}

/// A small diamond, three art pixels across, with a dark edge.
fn draw_dot(ui: &mut Ui, at: Vec2, color: Vec4, alpha: f32) {
    let p = ART_PIXEL;
    let cross = |ui: &mut Ui, half: f32, color: Vec4| {
        ui.rect(Rect::new(at - Vec2::new(half * p + p * 0.5, p * 0.5), Vec2::new((half * 2.0 + 1.0) * p, p)), color);
        for row in 1..=half as i32 {
            let wide = half - row as f32;
            for sign in [-1.0, 1.0] {
                let y = at.y - p * 0.5 + sign * row as f32 * p;
                ui.rect(
                    Rect::new(Vec2::new(at.x - wide * p - p * 0.5, y), Vec2::new((wide * 2.0 + 1.0) * p, p)),
                    color,
                );
            }
        }
    };
    cross(ui, 2.0, faded(INK, alpha));
    cross(ui, 1.0, faded(color, alpha));
}

fn image(ui: &mut Ui, rows: &Mark) -> Option<ImageId> {
    let lit = rows.iter().flat_map(|row| row.bytes());
    let rgba: Vec<u8> = lit.flat_map(|c| [255, 255, 255, if c == b'X' { 255 } else { 0 }]).collect();
    ui.add_image(12, 12, &rgba)
}

fn faded(color: Vec4, alpha: f32) -> Vec4 {
    Vec4::new(color.x, color.y, color.z, color.w * alpha)
}
