//! A conversation: a box at the bottom with what's being said, typed out, and the answers under
//! it, with the bust of whoever is talking standing on the box, drawn from their model as they
//! are right now.

use grimora::portrait::{PortraitShot, Sitter, camera_facing, yaw_of};
use grimora::prelude::*;
use grimora::render::UiInput;
use grimora::scene::story::{ChoiceDesc, DialogueDesc};

use crate::Sandbox;
use crate::shop::Stall;
use crate::tale::{Panel, wrapped};

const LETTERS_PER_SECOND: f32 = 42.0;
const BOX_WIDTH: f32 = 820.0;
const BOX_BOTTOM: f32 = 28.0;
const PAD: Vec2 = Vec2::new(26.0, 18.0);
const NAME_ROW: f32 = 28.0;
const TEXT_GAP: f32 = 14.0;
const ANSWER_ROW: f32 = 26.0;
const INK: Vec4 = Vec4::new(0.015, 0.018, 0.05, 0.9);
const LINE: Vec4 = Vec4::new(0.62, 0.46, 0.58, 1.0);
const NAME: Vec4 = Vec4::new(0.95, 0.8, 0.5, 1.0);
const TEXT: Vec4 = Vec4::new(0.9, 0.87, 0.76, 1.0);
/// The bust's picture, shown at twice that size so its pixels match the interface's.
const BUST_SIZE: (u32, u32) = (108, 144);
const BUST_SCALE: f32 = 2.0;
/// How far in from the box's left edge it stands.
const BUST_LEFT: f32 = 12.0;
/// Where it looks, below the base of the head, and from how far, showing the chest and the head.
const BUST_BELOW_HEAD_JOINT: f32 = 0.16;
const BUST_DISTANCE: f32 = 2.0;
const BUST_FOV: f32 = 25.0;
const BUST_TURN: f32 = -0.3;

/// A conversation under way.
pub struct Talk {
    speaker: String,
    object: String,
    dialogue: DialogueDesc,
    node: String,
    /// How many letters of it are out.
    shown: f32,
}

impl Talk {
    pub fn new(dialogue: &DialogueDesc, node: &str) -> Self {
        Self {
            speaker: dialogue.speaker.clone(),
            object: dialogue.object.clone(),
            dialogue: dialogue.clone(),
            node: node.to_owned(),
            shown: 0.0,
        }
    }

    /// All the text at once.
    pub fn finish(&mut self) {
        self.shown = f32::MAX;
    }
}

impl Sandbox {
    pub(crate) fn run_talk(&mut self, ctx: &mut Context, mut talk: Talk) -> Option<Panel> {
        if ctx.input.pad_just_pressed(PadButton::East) {
            return None;
        }
        let node = talk.dialogue.node(&talk.node)?.clone();
        let answers: Vec<ChoiceDesc> = node
            .choices
            .iter()
            .filter(|choice| self.tale.story.all_hold(&self.tale.book, &self.bag.inventory, &choice.when))
            .cloned()
            .collect();
        let goodbye = [ChoiceDesc { text: "Continuar".to_owned(), ..Default::default() }];
        let answers = if answers.is_empty() { &goodbye[..] } else { &answers[..] };

        // How tall the box is doesn't change as the letters come out.
        let screen = ctx.ui.size();
        let width = BOX_WIDTH.min(screen.x - 48.0);
        let style = TextStyle::new(16.0).color(TEXT);
        let text = wrapped(&ctx.ui, &node.text, &style, width - PAD.x * 2.0);
        let text_height = ctx.ui.measure(&text, &style).y;
        let height = PAD.y * 2.0 + NAME_ROW + text_height + TEXT_GAP + answers.len() as f32 * ANSWER_ROW;
        let top_left = Vec2::new((screen.x - width) * 0.5, screen.y - BOX_BOTTOM - height).floor();
        let panel = Rect::new(top_left, Vec2::new(width, height));

        self.draw_bust(ctx, &talk.object, panel);
        ctx.ui.dither(panel, INK, INK.w, false);
        ctx.ui.dither(Rect::new(panel.min, Vec2::new(width, ctx.ui.line())), LINE, 0.0, false);
        let mut y = panel.min.y + PAD.y;
        let name = TextStyle::new(16.0).color(NAME);
        ctx.ui.text(Vec2::new(panel.min.x + PAD.x, y), &talk.speaker.to_uppercase(), &name);
        y += NAME_ROW;

        let letters = text.chars().count();
        let input: UiInput = ctx.ui.input();
        let typing = (talk.shown as usize) < letters;
        if typing && (input.activate || input.mouse_pressed) {
            talk.finish();
        } else if typing {
            talk.shown += ctx.time.delta() * LETTERS_PER_SECOND;
        }
        let out: String = text.chars().take(talk.shown.min(letters as f32) as usize).collect();
        ctx.ui.text(Vec2::new(panel.min.x + PAD.x, y), &out, &style);
        y += text_height + TEXT_GAP;
        if typing {
            return Some(Panel::Talk(talk));
        }

        ctx.ui.theme.text_size = 16.0;
        let mut chosen = None;
        for (at, answer) in answers.iter().enumerate() {
            let row = Rect::new(Vec2::new(panel.min.x + PAD.x, y), Vec2::new(width - PAD.x * 2.0, ANSWER_ROW - 2.0));
            if ctx.ui.button(row, &answer.text) {
                chosen = Some(at);
            }
            y += ANSWER_ROW;
        }
        let Some(chosen) = chosen else { return Some(Panel::Talk(talk)) };

        let answer = &answers[chosen];
        let book = self.tale.book.clone();
        let (shop, news) =
            self.tale.story.apply(&book, &ctx.world.items.clone(), &mut self.bag.inventory, &answer.does);
        self.tell(ctx, news);
        ctx.ui.reset_focus();
        if let Some(shop) = shop {
            self.tale.panel = Some(Panel::Shop(Stall::new(&shop)));
            return None;
        }
        let next = answer.next.clone()?;
        talk.node = next;
        talk.shown = 0.0;
        Some(Panel::Talk(talk))
    }

    /// Whoever is talking, standing on the box, which cuts them off at the chest.
    fn draw_bust(&mut self, ctx: &mut Context, object: &str, panel: Rect) {
        let Some(image) = self.picture(ctx, "bust", BUST_SIZE) else { return };
        let Some(speaker) = ctx.world.find(object) else { return };
        let Some(head) = ctx.world.joint_transform(speaker, "head") else { return };
        let Some(body) = ctx.world.transform(speaker) else { return };
        let point = head.position - Vec3::Y * BUST_BELOW_HEAD_JOINT;
        let camera = camera_facing(point, yaw_of(&body) + BUST_TURN, BUST_DISTANCE, BUST_FOV);
        ctx.portraits.push(PortraitShot::new(image, Sitter::Entity(speaker), camera));
        let size = Vec2::new(BUST_SIZE.0 as f32, BUST_SIZE.1 as f32) * BUST_SCALE;
        let at = Vec2::new(panel.min.x + BUST_LEFT, panel.min.y - size.y);
        ctx.ui.image(Rect::new(at, size), image, Vec4::ONE);
    }
}
