//! The character's sheet: name, level, attributes and skills, read from `assets/story/sheet.ron`.
//! For now they're only shown, and nothing in the game changes them; life, defense and breath
//! are the game's own.

use std::sync::Arc;

use grimora::prelude::*;
use serde::Deserialize;

use crate::Sandbox;
use crate::bag_screen::{DIM, GOLD, Layout, MARK_COIN, MAUVE, PALE, PANEL_PAD, RED, ROW, TEXT};

const SHEET: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/story/sheet.ron");

const TITLE: f32 = 28.0;
const TRAIT_ROW: f32 = 26.0;
const TRAIT_GAP: f32 = 4.0;
const BAR: f32 = 26.0;
const BAR_GAP: f32 = 6.0;
const SECTION_GAP: f32 = 14.0;
const SIDE_PAD: f32 = 14.0;
const MARK: f32 = 24.0;
const INFO_ROW: f32 = 32.0;
const SKILL_ROW: f32 = 32.0;
const SKILL_GAP: f32 = 10.0;
const ABOUT_HEIGHT: f32 = 120.0;

const HOVER: Vec4 = Vec4::new(0.36, 0.27, 0.46, 0.85);
const TRACK: Vec4 = Vec4::new(0.0, 0.0, 0.0, 0.6);
const ARCANE: Vec4 = Vec4::new(0.4, 0.44, 0.86, 1.0);
const LIFE: Vec4 = Vec4::new(0.76, 0.25, 0.25, 1.0);
const DEFENSE: Vec4 = Vec4::new(0.56, 0.56, 0.64, 1.0);
const ENDURANCE: Vec4 = Vec4::new(0.8, 0.6, 0.26, 1.0);

/// The whole file.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SheetDesc {
    pub name: String,
    /// What the character is: human, and so on.
    pub people: String,
    pub level: u32,
    /// Experience so far, and what the next level takes.
    pub experience: (u32, u32),
    pub attributes: Vec<TraitDesc>,
    /// The arcane flow: what's left, and the most there can be.
    pub arcane: (u32, u32),
    /// The skills tab, a column each.
    pub columns: Vec<ColumnDesc>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TraitDesc {
    pub name: String,
    pub value: i32,
    pub description: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ColumnDesc {
    pub name: String,
    /// The mark beside each of its entries, sRGB.
    pub color: (f32, f32, f32),
    pub entries: Vec<SkillDesc>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SkillDesc {
    pub name: String,
    pub level: Option<u32>,
    pub description: String,
}

fn load() -> SheetDesc {
    let text = std::fs::read_to_string(SHEET).map_err(anyhow::Error::from);
    let sheet = text.and_then(|text| ron::from_str(&text).map_err(anyhow::Error::from));
    sheet.unwrap_or_else(|err| {
        eprintln!("[sandbox] {SHEET}: {err:#}");
        SheetDesc::default()
    })
}

/// The same color, darker, for where a bar starts.
fn darker(color: Vec4) -> Vec4 {
    Vec4::new(color.x * 0.45, color.y * 0.45, color.z * 0.45, color.w)
}

/// A bar filled `share` of the way, with its name and number on it.
fn bar(ui: &mut Ui, row: Rect, color: Vec4, share: f32, name: &str, value: &str) {
    ui.rect(row, ROW);
    let filled = Vec2::new((row.size.x * share.clamp(0.0, 1.0)).floor(), row.size.y);
    if filled.x > 0.0 {
        ui.gradient(Rect::new(row.min, filled), darker(color), color);
    }
    let middle = row.point(Anchor::Center).y.floor();
    ui.text(Vec2::new(row.min.x + 10.0, middle), name, &TextStyle::new(16.0).color(PALE).anchor(Anchor::Left));
    let style = TextStyle::new(16.0).color(PALE).anchor(Anchor::Right);
    ui.text(Vec2::new(row.max().x - 10.0, middle), value, &style);
}

impl Sandbox {
    pub(crate) fn sheet(&mut self) -> Arc<SheetDesc> {
        self.bag.screen.sheet.get_or_insert_with(|| Arc::new(load())).clone()
    }

    /// The inventory tab's own parts: the name over the body, the level under it, and the
    /// attributes on the right.
    pub(crate) fn draw_character(&mut self, ctx: &mut Context, layout: &Layout) {
        let sheet = self.sheet();
        if layout.body.is_some()
            && let Some(left) = layout.panels.first()
        {
            let name = if sheet.people.is_empty() {
                sheet.name.to_uppercase()
            } else {
                format!("{}  ·  {}", sheet.name, sheet.people).to_uppercase()
            };
            let at = Vec2::new(left.point(Anchor::Center).x, left.min.y + PANEL_PAD).floor();
            ctx.ui.text(at, &name, &TextStyle::new(16.0).color(PALE).anchor(Anchor::Top));
        }
        if let Some(info) = layout.info {
            self.draw_info(ctx, info, &sheet);
        }
        if let Some(side) = layout.side {
            self.draw_attributes(ctx, side, &sheet);
        }
    }

    /// Under the body: the level, how far to the next, the coins and what's in the hand.
    fn draw_info(&mut self, ctx: &mut Context, rect: Rect, sheet: &SheetDesc) {
        ctx.ui.rect(rect, ROW);
        let inner = rect.expand(-SIDE_PAD);
        let (left, right) = (inner.min.x, inner.max().x);
        let mut y = inner.min.y;
        ctx.ui.text(Vec2::new(left, y), &format!("NÍVEL {}", sheet.level), &TextStyle::new(16.0).color(PALE));
        let (have, next) = sheet.experience;
        let style = TextStyle::new(8.0).color(DIM).anchor(Anchor::TopRight);
        ctx.ui.text(Vec2::new(right, y + 4.0), &format!("{have} / {next} EXP"), &style);
        y += 24.0;
        let track = Rect::new(Vec2::new(left, y), Vec2::new(inner.size.x, 6.0));
        ctx.ui.rect(track, TRACK);
        let share = (have as f32 / next.max(1) as f32).clamp(0.0, 1.0);
        if share > 0.0 {
            ctx.ui.gradient(Rect::new(track.min, Vec2::new((track.size.x * share).floor(), track.size.y)), MAUVE, PALE);
        }
        y += 22.0;

        let catalog = ctx.world.items.clone();
        let inventory = &self.bag.inventory;
        let hand = inventory.hand().and_then(|id| inventory.piece(id)).and_then(|piece| catalog.get(&piece.stack.item));
        let hand = hand.map_or_else(|| "-".to_owned(), |item| item.name.clone());
        let coins = inventory.count_of("moeda").to_string();
        let rows = [(MARK_COIN, GOLD, "MOEDAS", coins), (0, PALE, "NA MÃO", hand)];
        for (mark, color, word, value) in rows {
            if y + MARK > inner.max().y {
                break;
            }
            if let Some(&image) = self.bag.screen.marks.get(mark) {
                ctx.ui.image(Rect::new(Vec2::new(left, y), Vec2::splat(MARK)), image, color);
            }
            let middle = y + MARK * 0.5;
            let small = TextStyle::new(8.0).color(DIM).anchor(Anchor::Left);
            ctx.ui.text(Vec2::new(left + MARK + 10.0, middle), word, &small);
            let number = TextStyle::new(16.0).color(color).anchor(Anchor::Right);
            ctx.ui.text(Vec2::new(right, middle), &value, &number);
            y += INFO_ROW;
        }
    }

    /// The right panel: the attributes, and what the character has left.
    fn draw_attributes(&mut self, ctx: &mut Context, panel: Rect, sheet: &SheetDesc) {
        let inner = panel.expand(-SIDE_PAD);
        let (left, width) = (inner.min.x, inner.size.x);
        let center = inner.point(Anchor::Center).x.floor();
        let title = TextStyle::new(16.0).color(PALE).anchor(Anchor::Top);
        let mut y = panel.min.y + PANEL_PAD;
        ctx.ui.text(Vec2::new(center, y), "ATRIBUTOS", &title);
        y += TITLE;
        let cursor = ctx.ui.input().cursor;
        let mut about = None;
        for attribute in &sheet.attributes {
            let row = Rect::new(Vec2::new(left, y), Vec2::new(width, TRAIT_ROW));
            let hovered = cursor.is_some_and(|at| row.contains(at));
            ctx.ui.rect(row, if hovered { HOVER } else { ROW });
            if hovered {
                about = Some(attribute);
            }
            let middle = row.point(Anchor::Center).y.floor();
            let name = TextStyle::new(16.0).color(TEXT).anchor(Anchor::Left);
            ctx.ui.text(Vec2::new(left + 10.0, middle), &attribute.name, &name);
            let color = match attribute.value.signum() {
                1 => PALE,
                -1 => RED,
                _ => DIM,
            };
            let value = TextStyle::new(16.0).color(color).anchor(Anchor::Right);
            ctx.ui.text(Vec2::new(row.max().x - 10.0, middle), &attribute.value.to_string(), &value);
            y += TRAIT_ROW + TRAIT_GAP;
        }

        y += SECTION_GAP;
        ctx.ui.text(Vec2::new(center, y), "RECURSOS", &title);
        y += TITLE;
        let (life, most) = ctx.world.get::<Health>(self.avatar).map_or((0.0, 0.0), |h| (h.current, h.max));
        let (breath, deepest) = ctx.world.get::<Stamina>(self.avatar).map_or((0.0, 0.0), |s| (s.current, s.max));
        let defense = self.bag.defense(&ctx.world.items.clone());
        let (flow, fullest) = sheet.arcane;
        let bars = [
            ("Fluxo Arcano", ARCANE, flow as f32 / fullest.max(1) as f32, format!("{flow}/{fullest}")),
            ("Vida", LIFE, life / most.max(1.0), format!("{life:.0}/{most:.0}")),
            ("Defesa", DEFENSE, 1.0, format!("{defense:.0}")),
            ("Resistência", ENDURANCE, breath / deepest.max(1.0), format!("{deepest:.0}")),
        ];
        for (name, color, share, value) in bars {
            bar(&mut ctx.ui, Rect::new(Vec2::new(left, y), Vec2::new(width, BAR)), color, share, name, &value);
            y += BAR + BAR_GAP;
        }

        if let Some(attribute) = about {
            let style = TextStyle::new(8.0).color(DIM).wrap(width);
            let size = ctx.ui.measure(&attribute.description, &style);
            let at = Vec2::new(left, (panel.max().y - PANEL_PAD - size.y).max(y + 8.0)).floor();
            ctx.ui.text(at, &attribute.description, &style);
        }
    }

    /// Habilidades: a column for each kind, and what the one chosen is at the bottom.
    pub(crate) fn draw_skills(&mut self, ctx: &mut Context, panel: Rect) {
        let sheet = self.sheet();
        let inner = panel.expand(-PANEL_PAD);
        let count = sheet.columns.len().max(1) as f32;
        let width = ((inner.size.x - SKILL_GAP * (count - 1.0)) / count).floor();
        let about_top = inner.max().y - ABOUT_HEIGHT;
        let mut about = None;
        // Mixed case: the names are long, and small letters are narrower.
        let upper = std::mem::replace(&mut ctx.ui.theme.upper, false);
        for (at, column) in sheet.columns.iter().enumerate() {
            let x = inner.min.x + at as f32 * (width + SKILL_GAP);
            let color = Vec4::new(column.color.0, column.color.1, column.color.2, 1.0);
            let title = TextStyle::new(16.0).color(PALE).anchor(Anchor::Top);
            ctx.ui.text(Vec2::new(x + width * 0.5, inner.min.y).floor(), &column.name.to_uppercase(), &title);
            let mut y = inner.min.y + TITLE + 6.0;
            for entry in &column.entries {
                if y + SKILL_ROW > about_top - 12.0 {
                    break;
                }
                let row = Rect::new(Vec2::new(x, y), Vec2::new(width, SKILL_ROW - 4.0));
                ctx.ui.rect(row, ROW);
                let dot = Rect::anchored(
                    Vec2::new(x + 11.0, row.point(Anchor::Center).y).floor(),
                    Vec2::splat(6.0),
                    Anchor::Center,
                );
                ctx.ui.rect(dot, color);
                let level = entry.level.map_or_else(String::new, |level| format!("Nv {level}"));
                let words = Rect::new(row.min + Vec2::X * 20.0, row.size - Vec2::X * 20.0);
                ctx.ui.button_row(words, &entry.name, &level);
                if ctx.ui.is_focused(&entry.name) {
                    about = Some((entry, color, column.name.as_str()));
                }
                y += SKILL_ROW;
            }
        }

        ctx.ui.theme.upper = upper;
        let box_rect = Rect::new(Vec2::new(inner.min.x, about_top), Vec2::new(inner.size.x, ABOUT_HEIGHT));
        ctx.ui.rect(box_rect, ROW);
        let text = box_rect.expand(-SIDE_PAD);
        match about {
            Some((entry, color, kind)) => {
                let name = TextStyle::new(16.0).color(color);
                ctx.ui.text(text.min, &entry.name.to_uppercase(), &name);
                let level = entry
                    .level
                    .map_or_else(|| kind.to_uppercase(), |level| format!("{}  ·  NÍVEL {level}", kind.to_uppercase()));
                let small = TextStyle::new(8.0).color(DIM).anchor(Anchor::TopRight);
                ctx.ui.text(Vec2::new(text.max().x, text.min.y + 4.0), &level, &small);
                let about = TextStyle::new(16.0).color(TEXT).wrap(text.size.x);
                ctx.ui.text(text.min + Vec2::Y * 28.0, &entry.description, &about);
            }
            None => {
                let hint = TextStyle::new(16.0).color(DIM);
                ctx.ui.text(text.min, "Escolha uma habilidade para ler sobre ela.", &hint);
            }
        }
    }
}
