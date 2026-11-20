//! Fabricação: the bag on the left, to see what's there, and on the right everything that can be
//! made, what the bag has enough for first. One is chosen, how many, and made.

use grimora::prelude::*;
use grimora::scene::story::RecipeDesc;
use grimora::story::{can_make, make};

use crate::Sandbox;
use crate::bag_screen::{DIM, Layout, MAUVE, PALE, PANEL_PAD, RED, ROW, TEXT};

const LIST_ROW: f32 = 30.0;
const ROW_ICON: f32 = 24.0;
/// Rows of the list before it scrolls.
const LIST_ROWS: usize = 8;
const ART: f32 = 96.0;
const NEED_ROW: f32 = 28.0;
const BUTTON: Vec2 = Vec2::new(180.0, 32.0);
const BUTTON_BACK: Vec4 = Vec4::new(0.5, 0.36, 0.48, 0.6);
const FADED: Vec4 = Vec4::new(1.0, 1.0, 1.0, 0.35);

/// What the tab remembers between frames.
#[derive(Debug, Clone, Default)]
pub struct Craft {
    /// The recipe chosen, by its place in the book.
    chosen: Option<usize>,
    count: f32,
    /// The first row of the list shown.
    first: usize,
}

/// How many times over the bag has what a recipe needs.
fn times(bag: &Inventory, recipe: &RecipeDesc) -> u32 {
    recipe.needs.iter().map(|(item, count)| bag.count_of(item) / (*count).max(1)).min().unwrap_or(0)
}

impl Sandbox {
    pub(crate) fn draw_crafting(&mut self, ctx: &mut Context, layout: &Layout) {
        let book = self.tale.book.clone();
        let catalog = ctx.world.items.clone();
        let mut order: Vec<usize> = (0..book.recipes.len()).collect();
        order.sort_by_key(|&at| (times(&self.bag.inventory, &book.recipes[at]) == 0, at));
        let chosen = self.bag.screen.craft.chosen.filter(|&at| at < book.recipes.len()).or(order.first().copied());

        // The bag, with what the chosen one needs marked.
        let needs: Vec<String> =
            chosen.map(|at| book.recipes[at].needs.iter().map(|(item, _)| item.clone()).collect()).unwrap_or_default();
        if let Some(&field) = layout.fields.first() {
            let own = self.bag.inventory.clone();
            self.draw_field(ctx, field, &own, "MOCHILA", false, &needs);
        }
        self.draw_hotbar_in(ctx, &layout.bar);

        let Some(panel) = layout.side else { return };
        let inner = panel.expand(-PANEL_PAD);
        let (left, width) = (inner.min.x, inner.size.x);
        let mut y = inner.min.y;
        ctx.ui.text(Vec2::new(left, y), "FABRICAR", &TextStyle::new(16.0).color(PALE));
        let ready = order.iter().filter(|&&at| times(&self.bag.inventory, &book.recipes[at]) > 0).count();
        let count = format!("{ready} DE {} PRONTAS", order.len());
        ctx.ui.text(
            Vec2::new(inner.max().x, y + 4.0),
            &count,
            &TextStyle::new(8.0).color(DIM).anchor(Anchor::TopRight),
        );
        y += 32.0;

        // Everything there is to make.
        let focused_row = chosen.and_then(|chosen| order.iter().position(|&at| at == chosen));
        let first = crate::tale::scrolled(self.bag.screen.craft.first, order.len(), LIST_ROWS, focused_row);
        self.bag.screen.craft.first = first;
        let mut picked = None;
        for &at in order.iter().skip(first).take(LIST_ROWS) {
            let recipe = &book.recipes[at];
            let name = catalog.get(&recipe.makes.0).map_or(recipe.makes.0.as_str(), |item| item.name.as_str());
            let label = if recipe.makes.1 > 1 { format!("{name} x{}", recipe.makes.1) } else { name.to_owned() };
            let can = times(&self.bag.inventory, recipe);
            let row = Rect::new(Vec2::new(left, y), Vec2::new(width, LIST_ROW - 2.0));
            ctx.ui.rect(row, ROW);
            if let Some(icon) = self.bag.icon(&catalog, &mut ctx.ui, &recipe.makes.0) {
                let spot = Rect::new(Vec2::new(left + 4.0, y + 2.0), Vec2::splat(ROW_ICON));
                ctx.ui.image(spot, icon, if can > 0 { Vec4::ONE } else { FADED });
            }
            let value = if can > 0 { format!("PODE {can}") } else { "FALTA".to_owned() };
            let words = Rect::new(row.min + Vec2::X * (ROW_ICON + 10.0), row.size - Vec2::X * (ROW_ICON + 10.0));
            if ctx.ui.button_row(words, &label, &value) || ctx.ui.is_focused(&label) {
                picked = Some(at);
            }
            y += LIST_ROW;
        }
        if let Some(at) = picked
            && Some(at) != self.bag.screen.craft.chosen
        {
            self.bag.screen.craft.chosen = Some(at);
            self.bag.screen.craft.count = 1.0;
        }
        let Some(at) = self.bag.screen.craft.chosen.filter(|&at| at < book.recipes.len()).or(chosen) else {
            let style = TextStyle::new(16.0).color(DIM);
            ctx.ui.text(Vec2::new(left, y), "Nada a fabricar ainda.", &style);
            return;
        };
        let recipe = &book.recipes[at];

        // The one chosen: what it is and what it takes.
        y = inner.min.y + 32.0 + LIST_ROWS as f32 * LIST_ROW + 10.0;
        ctx.ui.rect(Rect::new(Vec2::new(left, y), Vec2::new(width, 2.0)), MAUVE * Vec4::new(1.0, 1.0, 1.0, 0.5));
        y += 16.0;
        if let Some(icon) = self.bag.icon(&catalog, &mut ctx.ui, &recipe.makes.0) {
            ctx.ui.image(Rect::new(Vec2::new(left, y), Vec2::splat(ART)), icon, Vec4::ONE);
        }
        let beside = left + ART + 16.0;
        let words = inner.max().x - beside;
        if let Some(item) = catalog.get(&recipe.makes.0) {
            let name = TextStyle::new(16.0).color(PALE).wrap(words);
            let bottom = ctx.ui.text(Vec2::new(beside, y), &item.name.to_uppercase(), &name).max().y;
            let about = TextStyle::new(8.0).color(DIM).wrap(words);
            ctx.ui.text(Vec2::new(beside, bottom + 8.0), &item.description, &about);
        }
        y += ART + 16.0;
        ctx.ui.text(Vec2::new(left, y), "PRECISA DE", &TextStyle::new(8.0).color(DIM));
        y += 16.0;
        let bottom = inner.max().y - BUTTON.y - 12.0;
        for (item, count) in &recipe.needs {
            if y + NEED_ROW > bottom {
                break;
            }
            let has = self.bag.inventory.count_of(item);
            let color = if has >= *count { TEXT } else { RED };
            if let Some(icon) = self.bag.icon(&catalog, &mut ctx.ui, item) {
                ctx.ui.image(Rect::new(Vec2::new(left, y), Vec2::splat(ROW_ICON)), icon, Vec4::ONE);
            }
            let name = catalog.get(item).map_or(item.as_str(), |item| item.name.as_str());
            let middle = y + ROW_ICON * 0.5;
            let style = TextStyle::new(16.0).color(color).anchor(Anchor::Left);
            ctx.ui.text(Vec2::new(left + ROW_ICON + 10.0, middle), name, &style);
            let numbers = TextStyle::new(16.0).color(color).anchor(Anchor::Right);
            ctx.ui.text(Vec2::new(inner.max().x, middle), &format!("{has} / {count}"), &numbers);
            y += NEED_ROW;
        }

        // How many, and the button.
        let can = times(&self.bag.inventory, recipe);
        let most = can.max(1);
        let amount = &mut self.bag.screen.craft.count;
        *amount = amount.round().clamp(1.0, most as f32);
        let line = inner.max().y - BUTTON.y;
        let slider = Rect::new(Vec2::new(left, line), Vec2::new(width - BUTTON.x - 16.0, BUTTON.y));
        if most > 1 {
            ctx.ui.number_slider(slider, "Quantidade", amount, 1.0..=most as f32, 1.0, 0);
        } else {
            ctx.ui.button_row_if(false, slider, "Quantidade", "1");
        }
        let times_asked = amount.round().max(1.0) as u32;
        let button = Rect::new(Vec2::new(inner.max().x - BUTTON.x, line), BUTTON);
        if can > 0 {
            ctx.ui.rect(button, BUTTON_BACK);
        }
        if ctx.ui.button_if(can > 0, button, "Fabricar") {
            let mut made = 0;
            for _ in 0..times_asked {
                if !make(&catalog, &mut self.bag.inventory, recipe) {
                    break;
                }
                made += 1;
            }
            let name = catalog.get(&recipe.makes.0).map_or(String::new(), |item| item.name.clone());
            let total = made * recipe.makes.1;
            if made > 0 {
                let text = if total > 1 { format!("Feito: {name} x{total}") } else { format!("Feito: {name}") };
                self.hud.notify(ctx, text, true);
            } else if can_make(&self.bag.inventory, recipe) {
                self.hud.notify(ctx, "Não cabe mais nada", false);
            } else {
                self.hud.notify(ctx, "Falta material", false);
            }
            self.bag.screen.craft.count = 1.0;
        }
    }
}
