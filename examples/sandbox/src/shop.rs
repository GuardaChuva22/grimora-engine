//! Buying and selling: a list on the left and, on the right, what's known about the one chosen.

use grimora::prelude::*;
use grimora::story::sale_price;

use crate::Sandbox;
use crate::menu::left_margin;
use crate::tale::{Panel, scrolled};

const COIN: &str = "moeda";
const DARK: Vec4 = Vec4::new(0.006, 0.008, 0.03, 0.95);
const LINE: Vec4 = Vec4::new(0.62, 0.46, 0.58, 1.0);
const PALE: Vec4 = Vec4::new(0.98, 0.95, 0.84, 1.0);
const DIM: Vec4 = Vec4::new(0.7, 0.68, 0.6, 0.9);
const GOLD: Vec4 = Vec4::new(0.95, 0.8, 0.5, 1.0);
const TOP: f32 = 40.0;
const SIDE: Vec2 = Vec2::new(150.0, 30.0);
const LIST_TOP: f32 = 138.0;
const LIST_WIDTH: f32 = 470.0;
const ROW: f32 = 30.0;
const GAP: f32 = 64.0;
/// How far down what's known about an item ends.
const DETAILS_BOTTOM: f32 = 420.0;
const KEYS: &str = "ENTER  NEGOCIAR    Q/E  COMPRAR OU VENDER    ESC  SAIR";
const KEYS_PAD: &str = "A  NEGOCIAR    LB/RB  COMPRAR OU VENDER    B  SAIR";

/// A shop that's open.
pub struct Stall {
    id: String,
    selling: bool,
    first: usize,
}

impl Stall {
    pub fn new(id: &str) -> Self {
        Self { id: id.to_owned(), selling: false, first: 0 }
    }
}

impl Sandbox {
    pub(crate) fn run_shop(&mut self, ctx: &mut Context, mut stall: Stall) -> Option<Panel> {
        if ctx.input.pad_just_pressed(PadButton::East) {
            return None;
        }
        let book = self.tale.book.clone();
        let shop = book.shop(&stall.id)?;
        let catalog = ctx.world.items.clone();
        if ctx.input.just_pressed(KeyCode::KeyQ)
            || ctx.input.just_pressed(KeyCode::KeyE)
            || ctx.input.pad_just_pressed(PadButton::LeftBumper)
            || ctx.input.pad_just_pressed(PadButton::RightBumper)
        {
            stall.selling = !stall.selling;
            stall.first = 0;
            ctx.ui.reset_focus();
        }

        let screen = ctx.ui.screen();
        ctx.ui.rect(screen, DARK);
        ctx.ui.theme.text_size = 16.0;
        let left = left_margin(&ctx.ui);
        ctx.ui.text(Vec2::new(left, TOP), &shop.name.to_uppercase(), &TextStyle::new(24.0).color(PALE));
        let coins = self.bag.inventory.count_of(COIN);
        let purse = TextStyle::new(16.0).color(GOLD).anchor(Anchor::TopRight);
        ctx.ui.text(Vec2::new(left + LIST_WIDTH, TOP + 6.0), &format!("{coins} MOEDAS"), &purse);

        // What's on offer, as (item, price, how many there are).
        let rows: Vec<(String, u32, Option<u32>)> = if stall.selling {
            let mut rows: Vec<(String, u32, Option<u32>)> = Vec::new();
            for piece in self.bag.inventory.pieces() {
                let Some(item) = catalog.get(&piece.stack.item) else { continue };
                if item.id == COIN || item.price == 0 || rows.iter().any(|(id, ..)| *id == item.id) {
                    continue;
                }
                let count = Some(self.bag.inventory.count_of(&item.id));
                rows.push((item.id.clone(), sale_price(item, shop.buys_at), count));
            }
            rows
        } else {
            let price = |ware: &grimora::scene::story::WaresDesc| {
                ware.price.or(catalog.get(&ware.item).map(|item| item.price)).unwrap_or(0)
            };
            shop.wares.iter().map(|ware| (ware.item.clone(), price(ware), None)).collect()
        };
        let label = |item: &str| catalog.get(item).map_or(item, |item| item.name.as_str()).to_owned();
        let focused = rows.iter().position(|(item, ..)| ctx.ui.is_focused(&label(item)));
        let visible = ((ctx.ui.size().y - LIST_TOP - 40.0) / ROW) as usize;
        stall.first = scrolled(stall.first, rows.len(), visible, focused);

        let mut dealt = None;
        let mut y = LIST_TOP;
        for (item, price, count) in rows.iter().skip(stall.first).take(visible) {
            let row = Rect::new(Vec2::new(left, y), Vec2::new(LIST_WIDTH, ROW - 2.0));
            let name = label(item);
            let value = match count {
                Some(count) if *count > 1 => format!("{price}  (x{count})"),
                _ => price.to_string(),
            };
            if ctx.ui.button_row(row, &name, &value) {
                dealt = Some((item.clone(), *price));
            }
            y += ROW;
        }
        if rows.is_empty() {
            ctx.ui.text(Vec2::new(left, LIST_TOP), "Nada para negociar.", &TextStyle::new(16.0).color(DIM));
        }

        if let Some((item, price)) = dealt {
            if stall.selling {
                self.sell(ctx, &item, price);
            } else {
                self.buy(ctx, &item, price);
            }
        }

        for (at, name) in ["Comprar", "Vender"].iter().enumerate() {
            let rect = Rect::new(Vec2::new(left + at as f32 * (SIDE.x + 8.0), TOP + 44.0), SIDE);
            let this = (at == 1) == stall.selling;
            if ctx.ui.button(rect, name) && !this {
                stall.selling = at == 1;
                stall.first = 0;
            }
            if this {
                let under = Rect::new(Vec2::new(rect.min.x, rect.max().y + 2.0), Vec2::new(SIDE.x, ctx.ui.line()));
                ctx.ui.dither(under, LINE, 0.0, false);
            }
        }

        if let Some((item, ..)) = rows.iter().find(|(item, ..)| ctx.ui.is_focused(&label(item))).or(rows.first()) {
            self.draw_item_details(ctx, left + LIST_WIDTH + GAP, TOP, DETAILS_BOTTOM, item);
        }

        let keys = if ctx.input.gamepad_last() { KEYS_PAD } else { KEYS };
        let style = TextStyle::new(8.0).color(DIM).anchor(Anchor::Bottom);
        ctx.ui.text(ctx.ui.at(Anchor::Bottom, Vec2::new(0.0, -14.0)), keys, &style);
        Some(Panel::Shop(stall))
    }

    pub(crate) fn buy(&mut self, ctx: &mut Context, item: &str, price: u32) {
        let catalog = ctx.world.items.clone();
        let bag = &mut self.bag.inventory;
        if bag.count_of(COIN) < price {
            self.hud.notify(ctx, "Moedas de menos", false);
            return;
        }
        let before = bag.clone();
        bag.take_item(COIN, price);
        if bag.add(&catalog, item, 1) > 0 {
            *bag = before;
            self.hud.notify(ctx, "Não cabe mais nada", false);
            return;
        }
        ctx.audio.play_pitched(crate::STEP_SOUND, 0.6, 1.5);
    }

    pub(crate) fn sell(&mut self, ctx: &mut Context, item: &str, price: u32) {
        let catalog = ctx.world.items.clone();
        let bag = &self.bag.inventory;
        // Nothing in use goes, unless there's nothing else of it.
        let in_use = |id: u32| bag.hand() == Some(id);
        let piece =
            bag.pieces().iter().find(|piece| piece.stack.item == item && !in_use(piece.id)).map(|piece| piece.id);
        let Some(piece) = piece else {
            self.hud.notify(ctx, "Isso está em uso", false);
            return;
        };
        let before = self.bag.inventory.clone();
        self.bag.inventory.take(piece, 1);
        if self.bag.inventory.add(&catalog, COIN, price) > 0 {
            self.bag.inventory = before;
            self.hud.notify(ctx, "Não cabe mais nada", false);
            return;
        }
        if self.bag.inventory.count_of(item) == 0 {
            ctx.ui.reset_focus();
        }
        ctx.audio.play_pitched(crate::STEP_SOUND, 0.6, 1.5);
    }
}
