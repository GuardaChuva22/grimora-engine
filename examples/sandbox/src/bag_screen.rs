//! The character's screens, one tab each along the top: the inventory, making things, skills,
//! the diary and the map. The inventory is also the inside of a chest: pieces lying on a grid, one
//! lifted at a time and put down somewhere else, and a card of what's known about the one chosen.
//! The inventory uses none of the interface's widgets, since what's chosen is a cell and not a
//! widget; the other tabs do.

use std::collections::HashMap;
use std::sync::Arc;

use grimora::portrait::{PortraitShot, Sitter, camera_facing, yaw_of};
use grimora::prelude::*;
use grimora::render::{ART_PIXEL, ImageId, UiInput};
use grimora::scene::ArmorSlot;

use crate::Sandbox;
use crate::bag::{ARMOR_SLOTS, cell_of, slot_of};
use crate::craft::Craft;
use crate::sheet::SheetDesc;

/// The player's grid.
pub const COLUMNS: u32 = 10;
pub const ROWS: u32 = 6;
/// What a cell can measure: it grows with the room, and a piece's picture is 24 texels a cell.
const SMALLEST_CELL: f32 = 32.0;
const LARGEST_CELL: f32 = 64.0;
const TITLE_GAP: f32 = 10.0;
/// Room above a grid for its title.
const GRID_TITLE: f32 = 28.0;
/// From a grid down to the hotbar under it, and the number under each slot.
const BAR_GAP: f32 = 16.0;
const BAR_NUMBER: f32 = 24.0;
const DETAILS_WIDTH: f32 = 300.0;
/// The square picture in the details, three texels of it for each of its 48.
const ART: f32 = 144.0;
const STAT_ICON: f32 = 24.0;
const STAT_WIDTH: f32 = 96.0;
/// How far above the bottom of the grid the numbers start.
const STATS_ABOVE: f32 = 62.0;

/// The frame of every tab: the tabs along the top, panels under them, keys under those.
const MARGIN: f32 = 24.0;
const TABS_TOP: f32 = 16.0;
const TAB_HEIGHT: f32 = 30.0;
const TAB_GAP: f32 = 8.0;
const PANEL_GAP: f32 = 12.0;
pub(crate) const PANEL_PAD: f32 = 20.0;
const LEFT_WIDTH: f32 = 312.0;
const RIGHT_WIDTH: f32 = 280.0;
const BOTTOM: f32 = 34.0;

/// The card of what's chosen, beside it.
const CARD_WIDTH: f32 = 292.0;
const CARD_PAD: f32 = 14.0;
const CARD_ICON: f32 = 64.0;
const CARD_STAT: f32 = 88.0;

/// The body, in the left panel: a column of places and the character beside it.
const PLACE: f32 = 56.0;
const FIGURE_ART: (u32, u32) = (100, 170);
const FIGURE_SCALE: f32 = 2.0;
const FIGURE_GAP: f32 = 12.0;
/// Above the body, for the character's name.
const NAME_HEIGHT: f32 = 30.0;
const FIGURE_FOV: f32 = 26.0;
const FIGURE_DISTANCE: f32 = 5.0;
/// Which height of the character is in the middle of the picture.
const FIGURE_MIDDLE: f32 = 0.93;
/// How much of a turn the character starts at, which shows the armor's shape better than the front.
const FIGURE_START: f32 = 0.35;
/// Radians for each pixel the mouse drags it.
const FIGURE_DRAG: f32 = 0.012;
/// [`Cursor::x`] on [`Side::Body`] for the hand: the ones before it are in [`ARMOR_SLOTS`].
const BODY_PLACES: u32 = ARMOR_SLOTS.len() as u32;
const PLACE_WORDS: [&str; 4] = ["CABEÇA", "PEITO", "PERNAS", "PÉS"];

const DARK: Vec4 = Vec4::new(0.006, 0.008, 0.03, 0.93);
pub(crate) const PANEL: Vec4 = Vec4::new(0.055, 0.05, 0.1, 0.94);
/// Under a row of a list, a cell of a grid, anything that sits in a panel.
pub(crate) const ROW: Vec4 = Vec4::new(0.0, 0.0, 0.02, 0.6);
pub(crate) const MAUVE: Vec4 = Vec4::new(0.62, 0.46, 0.58, 1.0);
pub(crate) const PALE: Vec4 = Vec4::new(0.98, 0.95, 0.84, 1.0);
pub(crate) const TEXT: Vec4 = Vec4::new(0.9, 0.87, 0.76, 1.0);
const TAB_CHOSEN: Vec4 = Vec4::new(0.5, 0.36, 0.48, 0.95);
const TAB_HOVER: Vec4 = Vec4::new(0.24, 0.18, 0.28, 0.92);
const CARD_BACK: Vec4 = Vec4::new(0.02, 0.02, 0.05, 0.97);
const FIGURE_BACK: Vec4 = Vec4::new(0.3, 0.12, 0.2, 0.55);
const PIECE_BACK: Vec4 = Vec4::new(0.11, 0.12, 0.24, 0.42);
const CHOSEN_BACK: Vec4 = Vec4::new(0.26, 0.25, 0.42, 0.7);
const LEFT_BEHIND: Vec4 = Vec4::new(1.0, 1.0, 1.0, 0.3);
pub(crate) const DIM: Vec4 = Vec4::new(0.72, 0.7, 0.72, 0.9);

pub(crate) const RED: Vec4 = Vec4::new(0.95, 0.32, 0.28, 1.0);
const GREEN: Vec4 = Vec4::new(0.42, 0.88, 0.5, 1.0);
const BLUE: Vec4 = Vec4::new(0.45, 0.68, 1.0, 1.0);
const FROST: Vec4 = Vec4::new(0.6, 0.9, 1.0, 1.0);
pub(crate) const GOLD: Vec4 = Vec4::new(0.95, 0.8, 0.5, 1.0);

const USE_PIECE: &str = "use_slot";
const DROP_PIECE: &str = "drop_slot";
const TURN_PIECE: &str = "turn_piece";
const ARRANGE: &str = "arrange_bag";

const KEYS: &str = "ENTER  PEGAR    R  USAR    X  LARGAR    O  ORGANIZAR    Q E  ABAS    I  FECHAR";
const KEYS_PAD: &str = "A  PEGAR    X  USAR    Y  LARGAR    LB  ORGANIZAR    LT RT  ABAS    B  FECHAR";
const TAB_KEYS: &str = "ENTER  ESCOLHER    Q E  ABAS    I  FECHAR";
const TAB_KEYS_PAD: &str = "A  ESCOLHER    LT RT  ABAS    B  FECHAR";

/// Which tab is up.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Inventory,
    Crafting,
    Skills,
    Diary,
    Map,
}

const TABS: [(Tab, &str); 5] = [
    (Tab::Inventory, "INVENTÁRIO"),
    (Tab::Crafting, "FABRICAÇÃO"),
    (Tab::Skills, "HABILIDADES"),
    (Tab::Diary, "DIÁRIO"),
    (Tab::Map, "MAPA"),
];

impl Tab {
    /// For `--menu journal:<name>`: the tab, and the page of the diary.
    pub fn named(name: &str) -> (Self, usize) {
        match name {
            "ficha" | "inventario" => (Self::Inventory, 0),
            "criar" | "fabricacao" => (Self::Crafting, 0),
            "habilidades" => (Self::Skills, 0),
            "bestiario" => (Self::Diary, 1),
            "mapa" => (Self::Map, 0),
            _ => (Self::Diary, 0),
        }
    }
}
const CHEST_KEYS: &str = "ENTER  PEGAR    R  PASSAR    O  ORGANIZAR    I  VOLTAR";
const CHEST_KEYS_PAD: &str = "A  PEGAR    X  PASSAR    LB  ORGANIZAR    B  VOLTAR";
const LIFTED_KEYS: &str = "ENTER  COLOCAR    T  GIRAR    I  VOLTAR";
const LIFTED_KEYS_PAD: &str = "A  COLOCAR    RB  GIRAR    B  SOLTAR";

/// 12 by 12, what's lit: the marks next to an item's numbers, tinted by them.
type Mark = [&'static str; 12];
#[rustfmt::skip]
const BLADE: Mark = [
    "..........XX",
    ".........XXX",
    "........XXX.",
    ".......XXX..",
    "......XXX...",
    "X....XXX....",
    "XX..XXX.....",
    ".XXXXX......",
    "..XXX.......",
    ".XXXXX......",
    "XX...XX.....",
    "X...........",
];
#[rustfmt::skip]
const BOLT: Mark = [
    "......XXXX..",
    ".....XXXX...",
    "....XXXX....",
    "...XXXX.....",
    "..XXXXXXXX..",
    "..XXXXXXXX..",
    ".....XXXX...",
    "....XXXX....",
    "...XXXX.....",
    "..XXX.......",
    "..XX........",
    "..X.........",
];
#[rustfmt::skip]
const SPAN: Mark = [
    "............",
    "............",
    "...X....X...",
    "..XX....XX..",
    ".XXX....XXX.",
    "XXXXXXXXXXXX",
    "XXXXXXXXXXXX",
    ".XXX....XXX.",
    "..XX....XX..",
    "...X....X...",
    "............",
    "............",
];
#[rustfmt::skip]
const HEART: Mark = [
    "............",
    ".XXX....XXX.",
    "XXXXX..XXXXX",
    "XXXXXXXXXXXX",
    "XXXXXXXXXXXX",
    "XXXXXXXXXXXX",
    ".XXXXXXXXXX.",
    "..XXXXXXXX..",
    "...XXXXXX...",
    "....XXXX....",
    ".....XX.....",
    "............",
];
#[rustfmt::skip]
const SNOW: Mark = [
    ".....XX.....",
    "..X..XX..X..",
    "...X.XX.X...",
    "....XXXX....",
    ".X..XXXX..X.",
    "XXXXXXXXXXXX",
    "XXXXXXXXXXXX",
    ".X..XXXX..X.",
    "....XXXX....",
    "...X.XX.X...",
    "..X..XX..X..",
    ".....XX.....",
];
#[rustfmt::skip]
const FLAME: Mark = [
    ".....X......",
    ".....XX.....",
    "....XXX.....",
    "....XXXX..X.",
    "..X.XXXX..X.",
    "..XXXXXXX.XX",
    ".XXXXX.XXXXX",
    ".XXXX..XXXXX",
    ".XXX....XXXX",
    ".XXX....XXX.",
    "..XXX..XXX..",
    "....XXXX....",
];
#[rustfmt::skip]
const SHIELD: Mark = [
    "............",
    ".XXXXXXXXXX.",
    ".XXXXXXXXXX.",
    ".XXX....XXX.",
    ".XXX....XXX.",
    ".XXX....XXX.",
    ".XXXX..XXXX.",
    "..XXXXXXXX..",
    "..XXXXXXXX..",
    "...XXXXXX...",
    "....XXXX....",
    ".....XX.....",
];
#[rustfmt::skip]
const COIN_MARK: Mark = [
    "............",
    "....XXXX....",
    "..XXXXXXXX..",
    "..XXX..XXX..",
    ".XXX.XX.XXX.",
    ".XXX.XX.XXX.",
    ".XXX.XX.XXX.",
    ".XXX.XX.XXX.",
    "..XXX..XXX..",
    "..XXXXXXXX..",
    "....XXXX....",
    "............",
];
const MARKS: [&Mark; 8] = [&BLADE, &BOLT, &SPAN, &HEART, &SNOW, &FLAME, &SHIELD, &COIN_MARK];
/// Where the marks that also say what's wrong with the character are, in [`draw_marks`]'s list.
pub(crate) const MARK_BOLT: usize = 1;
pub(crate) const MARK_SNOW: usize = 4;
pub(crate) const MARK_FLAME: usize = 5;
pub(crate) const MARK_SHIELD: usize = 6;
pub(crate) const MARK_COIN: usize = 7;

/// Which grid.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Side {
    #[default]
    Own,
    Chest,
    /// [`Cursor::x`] is the place, one of [`ARMOR_SLOTS`].
    Body,
    /// [`Cursor::x`] is the slot of the hotbar.
    Bar,
}

/// The cell chosen. While something is lifted, it's where its top left cell would go.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cursor {
    pub side: Side,
    pub x: u32,
    pub y: u32,
}

/// The piece being moved, which stays where it was until it lands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lifted {
    pub side: Side,
    pub id: u32,
    pub turned: bool,
    /// The cell of the piece that the mouse holds it by.
    grab: (u32, u32),
    /// Where the mouse picked it up, if it did: letting go elsewhere puts it down.
    dragged_from: Option<Vec2>,
}

/// A picture and its texels.
#[derive(Debug, Clone, Copy)]
struct Picture {
    image: ImageId,
    size: (u32, u32),
}

#[derive(Default)]
pub struct Screen {
    pub cursor: Cursor,
    pub lifted: Option<Lifted>,
    last_mouse: Option<Vec2>,
    /// Where the mouse has what's lifted, which goes with it anywhere on the screen.
    float_at: Option<Vec2>,
    /// The mouse is over nothing to land on.
    over_nothing: bool,
    /// Each item's picture as a piece, both ways round, once it's been asked for.
    pieces: HashMap<(String, bool), Option<Picture>>,
    pub(crate) marks: Vec<ImageId>,
    /// Radians the player turned the character in the picture.
    figure_turn: f32,
    turning_figure: bool,
    pub tab: Tab,
    /// Which page of the diary: the quests or the animals.
    pub diary_page: usize,
    pub(crate) craft: Craft,
    /// The character's attributes and skills, read once.
    pub(crate) sheet: Option<Arc<SheetDesc>>,
}

impl Screen {
    /// Where the mouse has what's lifted.
    #[cfg(test)]
    pub fn dragged_to(&self) -> Option<Vec2> {
        self.float_at
    }
}

/// A grid on the screen.
#[derive(Debug, Clone, Copy)]
pub struct Field {
    side: Side,
    origin: Vec2,
    columns: u32,
    rows: u32,
    cell: f32,
}

impl Field {
    fn size(&self) -> Vec2 {
        Vec2::new(self.columns as f32, self.rows as f32) * self.cell
    }

    /// The screen area of some cells.
    fn cells(&self, x: u32, y: u32, size: (u32, u32)) -> Rect {
        let min = self.origin + Vec2::new(x as f32, y as f32) * self.cell;
        Rect::new(min, Vec2::new(size.0 as f32, size.1 as f32) * self.cell)
    }

    /// The middle of a cell on the screen.
    #[cfg(test)]
    pub fn center_of(&self, x: u32, y: u32) -> Vec2 {
        self.cells(x, y, (1, 1)).point(Anchor::Center)
    }

    /// The cell under a point on the screen.
    fn cell_at(&self, point: Vec2) -> Option<(u32, u32)> {
        let from = (point - self.origin) / self.cell;
        let inside = from.x >= 0.0 && from.y >= 0.0 && from.x < self.columns as f32 && from.y < self.rows as f32;
        inside.then_some((from.x as u32, from.y as u32))
    }
}

/// The places on the body and the character between them, on the screen.
#[derive(Debug, Clone, Copy)]
pub struct BodyPanel {
    origin: Vec2,
    height: f32,
}

impl BodyPanel {
    fn width() -> f32 {
        PLACE + FIGURE_GAP + FIGURE_ART.0 as f32 * FIGURE_SCALE
    }

    fn figure(&self) -> Rect {
        let size = Vec2::new(FIGURE_ART.0 as f32, FIGURE_ART.1 as f32) * FIGURE_SCALE;
        Rect::new(self.origin + Vec2::new(PLACE + FIGURE_GAP, (self.height - size.y) * 0.5), size)
    }

    /// The square of a place, down the left.
    pub fn place(&self, at: u32) -> Rect {
        let size = Vec2::splat(PLACE);
        let gap = (self.height - PLACE * BODY_PLACES as f32) / (BODY_PLACES - 1) as f32;
        Rect::new(self.origin + Vec2::Y * ((PLACE + gap) * at as f32).floor(), size)
    }

    fn place_at(&self, point: Vec2) -> Option<u32> {
        (0..BODY_PLACES).find(|&at| self.place(at).contains(point))
    }
}

/// The place of a slot on the body.
fn place_of(slot: ArmorSlot) -> u32 {
    ARMOR_SLOTS.iter().position(|&place| place == slot).unwrap_or(0) as u32
}

/// Where everything of a tab is on the screen.
#[derive(Debug, Clone)]
pub struct Layout {
    /// What the panels sit in: everything under the tabs.
    pub content: Rect,
    pub panels: Vec<Rect>,
    /// The grids, the chest's first.
    pub fields: Vec<Field>,
    /// The hotbar under the player's grid, if it's shown.
    pub bar: Vec<Rect>,
    pub body: Option<BodyPanel>,
    /// Under the body: the level and such.
    pub info: Option<Rect>,
    /// The panel on the right: the attributes, or what can be made.
    pub side: Option<Rect>,
}

/// A cell's size that fits, in steps of 8.
fn snap(cell: f32) -> f32 {
    ((cell / 8.0).floor() * 8.0).clamp(SMALLEST_CELL, LARGEST_CELL)
}

/// The largest cell that fits a grid, and maybe a hotbar under it, in a panel.
fn cell_for(panel: Rect, columns: u32, rows: u32, bar: bool) -> f32 {
    let across = (panel.size.x - PANEL_PAD * 2.0) / columns as f32;
    let under = if bar { BAR_GAP + BAR_NUMBER } else { 0.0 };
    let down = (panel.size.y - PANEL_PAD * 2.0 - GRID_TITLE - under) / (rows + u32::from(bar)) as f32;
    snap(across.min(down))
}

/// A grid in the middle of a panel, and the hotbar under it if it has one.
fn grid_in(panel: Rect, side: Side, inventory: &Inventory, cell: f32, bar: bool) -> (Field, Vec<Rect>) {
    let (columns, rows) = (inventory.width(), inventory.height());
    let size = Vec2::new(columns as f32, rows as f32) * cell;
    let under = if bar { BAR_GAP + cell + BAR_NUMBER } else { 0.0 };
    let height = GRID_TITLE + size.y + under;
    let x = panel.min.x + (panel.size.x - size.x) * 0.5;
    let y = panel.min.y + ((panel.size.y - height) * 0.5).max(PANEL_PAD) + GRID_TITLE;
    let origin = Vec2::new(x, y).floor();
    let field = Field { side, origin, columns, rows, cell };
    let mut slots = Vec::new();
    if bar {
        let gap = (cell * 0.25).floor();
        let width = HOTBAR as f32 * (cell + gap) - gap;
        let left = (origin.x + (size.x - width) * 0.5).floor();
        for at in 0..HOTBAR {
            let min = Vec2::new(left + at as f32 * (cell + gap), origin.y + size.y + BAR_GAP);
            slots.push(Rect::new(min, Vec2::splat(cell)));
        }
    }
    (field, slots)
}

/// The tabs along the top, as wide as the screen lets them be.
fn tab_rects(screen: Vec2) -> Vec<Rect> {
    let count = TABS.len() as f32;
    let width = ((screen.x - MARGIN * 2.0 - TAB_GAP * (count - 1.0)) / count).floor();
    let at = |i: usize| Vec2::new(MARGIN + i as f32 * (width + TAB_GAP), TABS_TOP).floor();
    (0..TABS.len()).map(|i| Rect::new(at(i), Vec2::new(width, TAB_HEIGHT))).collect()
}

/// A panel's width taken from the left of what's left, and the gap after it.
fn take_left(rest: &mut Rect, width: f32) -> Rect {
    let width = width.min(rest.size.x).floor();
    let taken = Rect::new(rest.min, Vec2::new(width, rest.size.y));
    *rest =
        Rect::new(rest.min + Vec2::X * (width + PANEL_GAP), Vec2::new(rest.size.x - width - PANEL_GAP, rest.size.y));
    taken
}

/// What goes where on a tab. With a chest open there are no tabs: the chest on the left, the bag
/// on the right.
fn layout(screen: Vec2, own: &Inventory, chest: Option<&Inventory>, tab: Tab) -> Layout {
    let top = if chest.is_some() { TABS_TOP } else { TABS_TOP + TAB_HEIGHT + PANEL_GAP };
    let content = Rect::new(Vec2::new(MARGIN, top), Vec2::new(screen.x - MARGIN * 2.0, screen.y - top - BOTTOM));
    let mut out =
        Layout { content, panels: Vec::new(), fields: Vec::new(), bar: Vec::new(), body: None, info: None, side: None };
    let mut rest = content;
    if let Some(chest) = chest {
        let left = take_left(&mut rest, (content.size.x - PANEL_GAP) * 0.5);
        let cell =
            cell_for(left, chest.width(), chest.height(), false).min(cell_for(rest, own.width(), own.height(), true));
        let (chest_field, _) = grid_in(left, Side::Chest, chest, cell, false);
        let (own_field, bar) = grid_in(rest, Side::Own, own, cell, true);
        out.panels = vec![left, rest];
        out.fields = vec![chest_field, own_field];
        out.bar = bar;
        return out;
    }
    match tab {
        Tab::Inventory => {
            let left = take_left(&mut rest, LEFT_WIDTH);
            let center_width = rest.size.x - RIGHT_WIDTH - PANEL_GAP;
            let center = take_left(&mut rest, center_width);
            let cell = cell_for(center, own.width(), own.height(), true);
            let (field, bar) = grid_in(center, Side::Own, own, cell, true);
            let height = FIGURE_ART.1 as f32 * FIGURE_SCALE;
            let x = left.min.x + (left.size.x - BodyPanel::width()) * 0.5;
            let origin = Vec2::new(x, left.min.y + PANEL_PAD + NAME_HEIGHT).floor();
            let top = origin.y + height + PANEL_PAD;
            let info = Rect::new(
                Vec2::new(left.min.x + PANEL_PAD, top),
                Vec2::new(left.size.x - PANEL_PAD * 2.0, left.max().y - PANEL_PAD - top),
            );
            out.panels = vec![left, center, rest];
            out.fields = vec![field];
            out.bar = bar;
            out.body = Some(BodyPanel { origin, height });
            out.info = (info.size.y > 40.0).then_some(info);
            out.side = Some(rest);
        }
        Tab::Crafting => {
            let left = take_left(&mut rest, (content.size.x - PANEL_GAP) * 0.5);
            let cell = cell_for(left, own.width(), own.height(), true);
            let (field, bar) = grid_in(left, Side::Own, own, cell, true);
            out.panels = vec![left, rest];
            out.fields = vec![field];
            out.bar = bar;
            out.side = Some(rest);
        }
        Tab::Skills | Tab::Diary | Tab::Map => out.panels = vec![content],
    }
    out
}

/// The body with no chest open.
#[cfg(test)]
pub fn own_body(ctx: &Context, own: &Inventory) -> BodyPanel {
    layout(ctx.ui.size(), own, None, Tab::Inventory).body.unwrap()
}

/// The player's grid with no chest open.
#[cfg(test)]
pub fn own_field(ctx: &Context, own: &Inventory) -> Field {
    layout(ctx.ui.size(), own, None, Tab::Inventory).fields[0]
}

/// A slot of the hotbar on the inventory tab.
#[cfg(test)]
pub fn bar_slot(ctx: &Context, own: &Inventory, at: usize) -> Rect {
    layout(ctx.ui.size(), own, None, Tab::Inventory).bar[at]
}

/// What an item's numbers are: which mark, its color, the number, and a word for it.
fn stats(item: &ItemDesc) -> Vec<(usize, Vec4, String, &'static str)> {
    match &item.kind {
        ItemKind::Weapon { weapon, .. } => {
            let long = weapon.blade.as_ref().map_or(weapon.reach, |blade| blade.tip);
            vec![
                (0, RED, format!("{:.0}", weapon.damage), "DANO"),
                (1, GREEN, format!("{:.0}", weapon.stamina), "FÔLEGO"),
                (2, BLUE, format!("{long:.1}").replace('.', ","), "ALCANCE"),
            ]
        }
        ItemKind::Gun { gun, .. } => {
            vec![(0, RED, format!("{:.0}", gun.damage), "DANO"), (2, BLUE, format!("{:.0}", gun.range), "ALCANCE")]
        }
        ItemKind::Consumable { health, stamina } => {
            let mut out = Vec::new();
            if *health != 0.0 {
                out.push((3, RED, format!("+{health:.0}"), "VIDA"));
            }
            if *stamina != 0.0 {
                out.push((1, GREEN, format!("+{stamina:.0}"), "FÔLEGO"));
            }
            out
        }
        ItemKind::Grimoire { spells, .. } => vec![(4, FROST, spells.len().to_string(), "FEITIÇOS")],
        ItemKind::Armor { defense, .. } => vec![(MARK_SHIELD, BLUE, format!("{defense:.0}"), "DEFESA")],
        ItemKind::Misc => Vec::new(),
    }
}

/// A quarter turn clockwise: what points up then points to the right.
fn turned_quarter(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let mut out = vec![0; rgba.len()];
    for y in 0..h {
        for x in 0..w {
            let (from, to) = ((y * w + x) * 4, (x * h + (h - 1 - y)) * 4);
            out[to..to + 4].copy_from_slice(&rgba[from..from + 4]);
        }
    }
    out
}

pub(crate) fn draw_marks(ui: &mut Ui) -> Vec<ImageId> {
    let image = |ui: &mut Ui, rows: &Mark| {
        let lit = rows.iter().flat_map(|row| row.bytes());
        let rgba: Vec<u8> = lit.flat_map(|c| [255, 255, 255, if c == b'X' { 255 } else { 0 }]).collect();
        ui.add_image(12, 12, &rgba)
    };
    MARKS.iter().filter_map(|rows| image(ui, rows)).collect()
}

fn keys_of(input: &Input, lifted: bool, chest: bool) -> &'static str {
    match (lifted, chest, input.gamepad_last()) {
        (true, _, false) => LIFTED_KEYS,
        (true, _, true) => LIFTED_KEYS_PAD,
        (false, false, false) => KEYS,
        (false, false, true) => KEYS_PAD,
        (false, true, false) => CHEST_KEYS,
        (false, true, true) => CHEST_KEYS_PAD,
    }
}

/// The controls of this screen, added to the ones of `bag.rs`.
pub fn add_controls(controls: &mut ActionMap) {
    controls.add(TURN_PIECE, [Binding::Key(KeyCode::KeyT), Binding::Pad(PadButton::RightBumper)]);
    controls.add(ARRANGE, [Binding::Key(KeyCode::KeyO), Binding::Pad(PadButton::LeftBumper)]);
}

impl Sandbox {
    /// The grid on this side, as it is now.
    fn side_inventory(&self, ctx: &mut Context, side: Side) -> Option<Inventory> {
        match side {
            Side::Own | Side::Bar => Some(self.bag.inventory.clone()),
            Side::Chest => {
                let key = self.bag.chest.clone()?;
                ctx.world.container(&key).cloned()
            }
            Side::Body => Some(self.bag.body.clone()),
        }
    }

    /// What's in a place on the body.
    fn in_place(&self, place: u32) -> Option<Placed> {
        self.bag.worn_in(*ARMOR_SLOTS.get(place as usize)?).cloned()
    }

    /// Whether what's lifted could go on this place.
    fn fits_place(&self, catalog: &ItemCatalog, lifted: &Lifted, place: u32) -> bool {
        let piece = match lifted.side {
            Side::Own | Side::Bar => self.bag.inventory.piece(lifted.id),
            Side::Body => self.bag.body.piece(lifted.id),
            Side::Chest => None,
        };
        let Some(piece) = piece else { return false };
        ARMOR_SLOTS.get(place as usize).is_some_and(|&slot| slot_of(catalog, &piece.stack.item) == Some(slot))
    }

    /// Whether what's lifted could go on the hotbar: all but what's worn.
    fn fits_bar(&self, ctx: &mut Context, catalog: &ItemCatalog, lifted: &Lifted) -> bool {
        let inventory = self.side_inventory(ctx, lifted.side);
        let piece = inventory.as_ref().and_then(|inventory| inventory.piece(lifted.id));
        lifted.side != Side::Body
            && piece.and_then(|piece| catalog.get(&piece.stack.item)).is_some_and(crate::gear::carryable)
    }

    /// Whether a piece from the body can land here: another piece on the way would have to go
    /// on the body, and that only works if it's for the same place.
    fn can_leave_body(&self, catalog: &ItemCatalog, lifted: &Lifted, at: (u32, u32), size: (u32, u32)) -> bool {
        let Some(piece) = self.bag.body.piece(lifted.id) else { return false };
        match self.bag.inventory.landing(catalog, &piece.stack.item, None, at, size) {
            Landing::Blocked => false,
            Landing::Over(under) => {
                let under = self.bag.inventory.piece(under).map(|p| p.stack.item.as_str());
                under.and_then(|item| slot_of(catalog, item)) == slot_of(catalog, &piece.stack.item)
            }
            _ => true,
        }
    }

    /// Whether a screen with something lifted lets go of it, for the key that closes it.
    pub(crate) fn drop_lifted(&mut self) -> bool {
        self.bag.screen.lifted.take().is_some()
    }

    /// One step of the arrows. A piece is one place to stop, whichever cell is under it.
    fn move_cursor(
        &mut self,
        own: &Inventory,
        chest: Option<&Inventory>,
        across: i32,
        down: i32,
        footprint: (u32, u32),
    ) {
        let grid = |side| if side == Side::Own { Some(own) } else { chest };
        let lifted = self.bag.screen.lifted.is_some();
        let mut cursor = self.bag.screen.cursor;
        if cursor.side == Side::Body {
            let place = &mut self.bag.screen.cursor;
            if across == 1 {
                *place = Cursor { side: Side::Own, x: 0, y: place.x.min(own.height().saturating_sub(1)) };
            } else {
                place.x = (cursor.x as i32 + down).clamp(0, BODY_PLACES as i32 - 1) as u32;
            }
            return;
        }
        if cursor.side == Side::Bar {
            let place = &mut self.bag.screen.cursor;
            if down < 0 {
                let x = cursor.x.min(own.width().saturating_sub(footprint.0));
                *place = Cursor { side: Side::Own, x, y: own.height().saturating_sub(footprint.1) };
            } else {
                place.x = (cursor.x as i32 + across).clamp(0, HOTBAR as i32 - 1) as u32;
            }
            return;
        }
        let start = grid(cursor.side).and_then(|g| g.at(cursor.x, cursor.y)).map(|p| p.id);
        for _ in 0..COLUMNS.max(ROWS) * 2 {
            let Some(here) = grid(cursor.side) else { return };
            let last_x = here.width().saturating_sub(footprint.0) as i32;
            let last_y = here.height().saturating_sub(footprint.1) as i32;
            let (x, y) = (cursor.x as i32 + across, cursor.y as i32 + down);
            if x < 0 && cursor.side == Side::Own && chest.is_none() {
                self.bag.screen.cursor = Cursor { side: Side::Body, x: place_of(ArmorSlot::Body), y: 0 };
                return;
            }
            if !(0..=last_x).contains(&x) {
                return;
            }
            if y < 0 {
                let Some(above) = chest.filter(|_| cursor.side == Side::Own) else { return };
                cursor.side = Side::Chest;
                cursor.x = cursor.x.min(above.width().saturating_sub(footprint.0));
                cursor.y = above.height().saturating_sub(footprint.1);
            } else if y > last_y {
                if cursor.side == Side::Own {
                    let x = cursor.x.min(HOTBAR as u32 - 1);
                    self.bag.screen.cursor = Cursor { side: Side::Bar, x, y: 0 };
                    return;
                }
                cursor.side = Side::Own;
                cursor.x = cursor.x.min(own.width().saturating_sub(footprint.0));
                cursor.y = 0;
            } else {
                (cursor.x, cursor.y) = (x as u32, y as u32);
            }
            let now = grid(cursor.side).and_then(|g| g.at(cursor.x, cursor.y)).map(|p| p.id);
            let same_piece = start.is_some() && now == start && !lifted;
            if !same_piece {
                self.bag.screen.cursor = cursor;
                return;
            }
        }
    }

    /// The first use of a piece lifts it, and the next puts it down at the cursor.
    fn lift_or_land(&mut self, ctx: &mut Context, dragged_from: Option<Vec2>) {
        let Some(lifted) = self.bag.screen.lifted else {
            let cursor = self.bag.screen.cursor;
            if matches!(cursor.side, Side::Body | Side::Bar) {
                let piece = match cursor.side {
                    Side::Body => self.in_place(cursor.x),
                    _ => self.bag.inventory.shortcut(cursor.x as usize).cloned(),
                };
                let Some(piece) = piece else { return };
                self.bag.screen.lifted =
                    Some(Lifted { side: cursor.side, id: piece.id, turned: piece.turned, grab: (0, 0), dragged_from });
                return;
            }
            let Some(inventory) = self.side_inventory(ctx, cursor.side) else { return };
            let Some(piece) = inventory.at(cursor.x, cursor.y) else { return };
            self.bag.screen.lifted = Some(Lifted {
                side: cursor.side,
                id: piece.id,
                turned: piece.turned,
                grab: (cursor.x - piece.x, cursor.y - piece.y),
                dragged_from,
            });
            self.bag.screen.cursor = Cursor { side: cursor.side, x: piece.x, y: piece.y };
            return;
        };
        let cursor = self.bag.screen.cursor;
        let catalog = ctx.world.items.clone();
        let at = (cursor.x, cursor.y);
        let landed = match (lifted.side, cursor.side) {
            (_, Side::Body) if !self.fits_place(&catalog, &lifted, cursor.x) => false,
            (Side::Own | Side::Bar, Side::Body) => {
                let (own, body) = (&mut self.bag.inventory, &mut self.bag.body);
                let slot = ARMOR_SLOTS[cursor.x as usize];
                own.move_piece_to(&catalog, lifted.id, body, cell_of(slot), false)
            }
            (Side::Body, Side::Bar) => false,
            (_, Side::Bar) if !self.fits_bar(ctx, &catalog, &lifted) => false,
            (Side::Own, Side::Bar) => self.bag.inventory.set_shortcut(cursor.x as usize, Some(lifted.id)),
            (Side::Bar, Side::Bar) => {
                if let Some(from) = self.bag.inventory.shortcut_of(lifted.id) {
                    self.bag.inventory.swap_shortcuts(from, cursor.x as usize);
                }
                true
            }
            (Side::Bar, Side::Own) => self.bag.inventory.take_off_bar(&catalog, lifted.id, at, lifted.turned),
            (Side::Chest, Side::Bar) => {
                let Some(key) = self.bag.chest.clone() else { return };
                let own = &mut self.bag.inventory;
                let Some(chest) = ctx.world.container(&key) else { return };
                let Some(stack) = chest.piece(lifted.id).map(|piece| piece.stack.clone()) else { return };
                let left = own.add(&catalog, &stack.item, stack.count);
                chest.take(lifted.id, stack.count - left);
                let new = own.grid_pieces().find(|piece| piece.stack.item == stack.item).map(|piece| piece.id);
                let on_bar =
                    own.pieces().iter().rev().find(|piece| piece.stack.item == stack.item).map(|piece| piece.id);
                left < stack.count && own.set_shortcut(cursor.x as usize, new.or(on_bar))
            }
            // Back where it came from.
            (Side::Body, Side::Body) => true,
            (Side::Body, Side::Own) => {
                let piece = self.bag.body.piece(lifted.id);
                let turned = piece.is_some_and(|piece| piece.turned) != lifted.turned;
                let size = piece.map_or((1, 1), |piece| if turned { (piece.size.1, piece.size.0) } else { piece.size });
                self.can_leave_body(&catalog, &lifted, at, size) && {
                    let (own, body) = (&mut self.bag.inventory, &mut self.bag.body);
                    body.move_piece_to(&catalog, lifted.id, own, at, lifted.turned)
                }
            }
            (Side::Own, Side::Own) => self.bag.inventory.move_piece(&catalog, lifted.id, at, lifted.turned),
            (Side::Body, _) | (_, Side::Body) => false,
            (from, to) => {
                let Some(key) = self.bag.chest.clone() else { return };
                let own = &mut self.bag.inventory;
                let Some(chest) = ctx.world.container(&key) else { return };
                match (from, to) {
                    (Side::Own | Side::Bar, _) => own.move_piece_to(&catalog, lifted.id, chest, at, lifted.turned),
                    (_, Side::Own) => chest.move_piece_to(&catalog, lifted.id, own, at, lifted.turned),
                    _ => chest.move_piece(&catalog, lifted.id, at, lifted.turned),
                }
            }
        };
        if landed {
            self.bag.screen.lifted = None;
        }
    }

    /// With a chest open, it passes the piece to the other side; otherwise, it uses it.
    fn use_chosen(&mut self, ctx: &mut Context, chosen: u32) {
        let cursor = self.bag.screen.cursor;
        let side = cursor.side;
        if side == Side::Body {
            let catalog = ctx.world.items.clone();
            let slot = ARMOR_SLOTS.get(cursor.x as usize).copied();
            if slot.is_some_and(|slot| !self.bag.take_off(&catalog, slot)) {
                self.hud.notify(ctx, crate::bag::NO_ROOM, false);
            }
            return;
        }
        let Some(key) = self.bag.chest.clone() else {
            // What's on the hotbar goes back in the bag.
            if side == Side::Bar {
                if !self.bag.inventory.set_shortcut(cursor.x as usize, None) {
                    self.hud.notify(ctx, crate::bag::NO_ROOM, false);
                }
            } else {
                self.use_piece(ctx, chosen);
            }
            return;
        };
        let catalog = ctx.world.items.clone();
        let own = &mut self.bag.inventory;
        let Some(chest) = ctx.world.container(&key) else { return };
        let (from, to) = if side != Side::Chest { (own, chest) } else { (chest, own) };
        let Some(stack) = from.piece(chosen).map(|piece| piece.stack.clone()) else { return };
        let left = to.add(&catalog, &stack.item, stack.count);
        from.take(chosen, stack.count - left);
    }

    /// Leaves a whole piece on the ground in front of the character.
    fn drop_piece(&mut self, ctx: &mut Context, id: u32) {
        let Some(player) = &self.player else { return };
        let center = player.body.center();
        let yaw = player.facing;
        let ahead = Quat::from_rotation_y(yaw) * Vec3::NEG_Z * crate::bag::DROP_AHEAD;
        let on_body = self.bag.screen.cursor.side == Side::Body && self.bag.body.piece(id).is_some();
        let from = if on_body { &mut self.bag.body } else { &mut self.bag.inventory };
        let Some(stack) = from.take(id, u32::MAX) else { return };
        ctx.world.drop_item(stack, center + ahead, yaw.to_degrees(), &ctx.physics);
    }

    /// What the keys and the mouse do this frame.
    fn bag_input(&mut self, ctx: &mut Context, layout: &Layout, input: UiInput) {
        let (fields, panel) = (&layout.fields[..], layout.body);
        let catalog = ctx.world.items.clone();
        let Some(own) = self.side_inventory(ctx, Side::Own) else { return };
        let chest = self.bag.chest.is_some().then(|| self.side_inventory(ctx, Side::Chest)).flatten();
        let body = self.bag.body.clone();
        let grid = |side| match side {
            Side::Own | Side::Bar => Some(&own),
            Side::Chest => chest.as_ref(),
            Side::Body => Some(&body),
        };

        // Turning first, so that the arrows and the mouse see the new shape.
        if ctx.input.action_just_pressed(TURN_PIECE)
            && let Some(lifted) = self.bag.screen.lifted.as_mut()
        {
            lifted.turned = !lifted.turned;
            lifted.grab = (0, 0);
        }
        let footprint = self.bag.screen.lifted.and_then(|lifted| {
            let piece = grid(lifted.side)?.piece(lifted.id)?;
            Some(if lifted.turned == piece.turned { piece.size } else { (piece.size.1, piece.size.0) })
        });

        let across = i32::from(input.right) - i32::from(input.left);
        let down = i32::from(input.down) - i32::from(input.up);
        let size = footprint.unwrap_or((1, 1));
        if across != 0 || down != 0 {
            self.bag.screen.float_at = None;
            self.bag.screen.over_nothing = false;
        }
        if across != 0 {
            self.move_cursor(&own, chest.as_ref(), across, 0, size);
        }
        if down != 0 {
            self.move_cursor(&own, chest.as_ref(), 0, down, size);
        }
        // A piece turned at the edge comes back in.
        if !matches!(self.bag.screen.cursor.side, Side::Body | Side::Bar)
            && let Some(here) = grid(self.bag.screen.cursor.side)
        {
            let cursor = &mut self.bag.screen.cursor;
            cursor.x = cursor.x.min(here.width().saturating_sub(size.0));
            cursor.y = cursor.y.min(here.height().saturating_sub(size.1));
        }

        // The mouse takes the cursor only when it moves.
        let mouse = input.cursor;
        let hit = mouse.and_then(|at| fields.iter().find_map(|f| Some((f.side, f.cell_at(at)?, *f))));
        if mouse != self.bag.screen.last_mouse
            && let Some((side, (x, y), field)) = hit
        {
            let grab = self.bag.screen.lifted.map_or((0, 0), |lifted| lifted.grab);
            let (x, y) = (x.saturating_sub(grab.0), y.saturating_sub(grab.1));
            self.bag.screen.cursor = Cursor {
                side,
                x: x.min(field.columns.saturating_sub(size.0)),
                y: y.min(field.rows.saturating_sub(size.1)),
            };
        }
        // Over the body, what's lifted goes to its own place, which is where the cursor goes.
        let on_place = panel.zip(mouse).and_then(|(panel, at)| {
            let lifted = self.bag.screen.lifted.and_then(|lifted| grid(lifted.side)?.piece(lifted.id).cloned());
            let own_place = || lifted.as_ref().and_then(|piece| slot_of(&catalog, &piece.stack.item)).map(place_of);
            panel.place_at(at).or_else(|| panel.figure().contains(at).then(own_place).flatten())
        });
        if mouse != self.bag.screen.last_mouse
            && let Some(place) = on_place
        {
            self.bag.screen.cursor = Cursor { side: Side::Body, x: place, y: 0 };
        }
        let on_bar = mouse.and_then(|at| layout.bar.iter().position(|slot| slot.contains(at)));
        let moved = mouse != self.bag.screen.last_mouse;
        if moved && let Some(slot) = on_bar {
            self.bag.screen.cursor = Cursor { side: Side::Bar, x: slot as u32, y: 0 };
        }
        let over_target = hit.is_some() || on_place.is_some() || on_bar.is_some();
        if moved {
            self.bag.screen.over_nothing = !over_target;
        }
        // What the mouse has goes with it, wherever that is.
        match self.bag.screen.lifted {
            Some(lifted) if moved || lifted.dragged_from.is_some() || self.bag.screen.float_at.is_some() => {
                self.bag.screen.float_at = mouse.or(self.bag.screen.float_at);
            }
            Some(_) => {}
            None => self.bag.screen.float_at = None,
        }
        self.bag.screen.last_mouse = mouse;

        // Dragging over the empty picture turns the character.
        let over_figure = panel.zip(mouse).is_some_and(|(panel, at)| panel.figure().contains(at));
        if input.mouse_pressed && over_figure && self.bag.screen.lifted.is_none() {
            self.bag.screen.turning_figure = true;
        }
        if !ctx.input.mouse_pressed(MouseButton::Left) {
            self.bag.screen.turning_figure = false;
        }
        if self.bag.screen.turning_figure {
            self.bag.screen.figure_turn += ctx.input.mouse_delta().x * FIGURE_DRAG;
        }

        if input.activate {
            self.lift_or_land(ctx, None);
        } else if input.mouse_pressed && over_target {
            self.lift_or_land(ctx, mouse);
        } else if input.mouse_released
            && let Some(lifted) = self.bag.screen.lifted
            && let (Some(from), Some(to)) = (lifted.dragged_from, mouse)
            && from.distance(to) >= fields.first().map_or(24.0, |field| field.cell * 0.5)
        {
            if over_target {
                self.lift_or_land(ctx, None);
            }
            // Let go over nothing, or where it can't go: it was never moved.
            self.bag.screen.lifted = None;
        }
        if self.bag.screen.lifted.is_none() {
            self.bag.screen.float_at = None;
        }

        if self.bag.screen.lifted.is_some() {
            return;
        }
        let cursor = self.bag.screen.cursor;
        let chosen = if cursor.side == Side::Body {
            self.in_place(cursor.x).map(|piece| piece.id)
        } else if cursor.side == Side::Bar {
            own.shortcut(cursor.x as usize).map(|piece| piece.id)
        } else {
            grid(cursor.side).and_then(|g| g.at(cursor.x, cursor.y)).map(|piece| piece.id)
        };
        let right_click = ctx.input.mouse_just_pressed(MouseButton::Right);
        if ctx.input.action_just_pressed(ARRANGE) {
            let arranged = match cursor.side {
                Side::Own | Side::Body | Side::Bar => self.bag.inventory.arrange(&catalog),
                Side::Chest => {
                    let key = self.bag.chest.clone();
                    key.and_then(|key| ctx.world.container(&key).map(|c| c.arrange(&catalog))).unwrap_or(true)
                }
            };
            if !arranged {
                self.hud.notify(ctx, "Não dá para arrumar melhor que isso", false);
            }
            return;
        }
        let Some(chosen) = chosen else { return };
        if ctx.input.action_just_pressed(USE_PIECE) && (over_target || !right_click) {
            self.use_chosen(ctx, chosen);
        } else if ctx.input.action_just_pressed(DROP_PIECE) && cursor.side != Side::Chest {
            self.drop_piece(ctx, chosen);
        }
    }

    /// An item's picture as a piece, or its icon if that's missing.
    fn piece_picture(&mut self, ctx: &mut Context, id: &str, turned: bool) -> Option<Picture> {
        let key = (id.to_owned(), turned);
        if let Some(picture) = self.bag.screen.pieces.get(&key) {
            return *picture;
        }
        let catalog = ctx.world.items.clone();
        let picture = catalog.get(id).and_then(|item| {
            let piece = catalog.piece_path(item);
            let path = if piece.exists() { piece } else { catalog.icon_path(item) };
            let texture = grimora::scene::load_texture(path).map_err(|err| eprintln!("[sandbox] {err:#}")).ok()?;
            let (width, height) = (texture.width, texture.height);
            if turned {
                let rgba = turned_quarter(width, height, &texture.rgba);
                let image = ctx.ui.add_image(height, width, &rgba)?;
                Some(Picture { image, size: (height, width) })
            } else {
                let image = ctx.ui.add_image(width, height, &texture.rgba)?;
                Some(Picture { image, size: (width, height) })
            }
        });
        self.bag.screen.pieces.insert(key, picture);
        picture
    }

    /// A piece's picture over its cells, at a whole number of texels if it's small.
    fn draw_piece_picture(&mut self, ctx: &mut Context, rect: Rect, id: &str, turned: bool, tint: Vec4) {
        let Some(picture) = self.piece_picture(ctx, id, turned) else { return };
        let art = Vec2::new(picture.size.0 as f32, picture.size.1 as f32);
        let fit = (rect.size / art).min_element();
        let shown = Rect::anchored(rect.point(Anchor::Center), art * fit, Anchor::Center);
        ctx.ui.image(shown, picture.image, tint);
    }

    /// A grid: a square for each cell, and the pieces on them. With `focus`, the one chosen
    /// stands out; the pieces of an item in `marked` have a line around them.
    pub(crate) fn draw_field(
        &mut self,
        ctx: &mut Context,
        field: Field,
        inventory: &Inventory,
        title: &str,
        focus: bool,
        marked: &[String],
    ) {
        let theme = ctx.ui.theme;
        let title_at = field.origin - Vec2::Y * TITLE_GAP;
        ctx.ui.text(title_at, title, &TextStyle::new(16.0).color(PALE).anchor(Anchor::BottomLeft));
        let room = format!("{} DE {}", inventory.cells_taken(), inventory.width() * inventory.height());
        let room_at = title_at + Vec2::X * field.size().x;
        ctx.ui.text(room_at, &room, &TextStyle::new(8.0).color(DIM).anchor(Anchor::BottomRight));
        for row in 0..field.rows {
            for column in 0..field.columns {
                ctx.ui.rect(field.cells(column, row, (1, 1)).expand(-ART_PIXEL * 0.5), ROW);
            }
        }

        let screen = &self.bag.screen;
        let (lifted, cursor) = (screen.lifted, screen.cursor);
        let chosen = inventory
            .at(cursor.x, cursor.y)
            .filter(|_| focus && lifted.is_none() && cursor.side == field.side)
            .map(|p| p.id);

        for piece in inventory.grid_pieces() {
            let rect = field.cells(piece.x, piece.y, piece.size);
            let inset = rect.expand(-ART_PIXEL);
            let is_chosen = chosen == Some(piece.id);
            let is_lifted = lifted.is_some_and(|l| l.side == field.side && l.id == piece.id);
            ctx.ui.rect(inset, if is_chosen { CHOSEN_BACK } else { PIECE_BACK });
            let tint = if is_lifted { LEFT_BEHIND } else { Vec4::ONE };
            self.draw_piece_picture(ctx, rect, &piece.stack.item, piece.turned, tint);
            if is_chosen {
                ctx.ui.panel(inset, Vec4::ZERO, theme.accent);
            } else if marked.contains(&piece.stack.item) {
                ctx.ui.panel(inset, Vec4::ZERO, GOLD);
            }
            if piece.stack.count > 1 {
                let style = TextStyle::new(8.0).color(theme.accent).anchor(Anchor::BottomRight);
                ctx.ui.text(inset.max() - Vec2::splat(3.0), &piece.stack.count.to_string(), &style);
            }
        }
    }

    /// The character as it stands, with a place beside it for each thing it can have on.
    fn draw_body(&mut self, ctx: &mut Context, panel: BodyPanel) {
        let catalog = ctx.world.items.clone();
        let theme = ctx.ui.theme;
        let figure = panel.figure();
        ctx.ui.rect(figure, ROW);
        ctx.ui.dither(figure, FIGURE_BACK, 0.0, false);
        if let (Some(image), Some(body)) =
            (self.picture(ctx, "bag_figure", FIGURE_ART), ctx.world.transform(self.avatar))
        {
            let yaw = yaw_of(&body) + FIGURE_START + self.bag.screen.figure_turn;
            let middle = body.position + Vec3::Y * FIGURE_MIDDLE;
            let camera = camera_facing(middle, yaw, FIGURE_DISTANCE, FIGURE_FOV);
            ctx.portraits.push(PortraitShot::new(image, Sitter::Entity(self.avatar), camera));
            ctx.ui.image(figure, image, Vec4::ONE);
        }

        let screen = &self.bag.screen;
        let (lifted, cursor) = (screen.lifted, screen.cursor);
        for place in 0..BODY_PLACES {
            let rect = panel.place(place);
            let inset = rect.expand(-ART_PIXEL);
            let focused = cursor.side == Side::Body && cursor.x == place;
            let there = self.in_place(place);
            ctx.ui.rect(rect, ROW);
            match &there {
                Some(piece) => {
                    let leaving = lifted.is_some_and(|l| l.id == piece.id && l.side == Side::Body);
                    ctx.ui.rect(inset, if focused && lifted.is_none() { CHOSEN_BACK } else { PIECE_BACK });
                    if let Some(icon) = self.bag.icon(&catalog, &mut ctx.ui, &piece.stack.item) {
                        ctx.ui.image(inset, icon, if leaving { LEFT_BEHIND } else { Vec4::ONE });
                    }
                }
                None => {
                    let style = TextStyle::new(8.0).color(DIM).anchor(Anchor::Center);
                    ctx.ui.text(rect.point(Anchor::Center).floor(), PLACE_WORDS[place as usize], &style);
                }
            }
            let fits = lifted.as_ref().is_some_and(|l| self.fits_place(&catalog, l, place));
            if focused || fits {
                let wrong = focused && lifted.is_some() && !fits;
                let line = if wrong { RED } else { theme.accent };
                ctx.ui.panel(inset, Vec4::ZERO, line);
            }
        }
    }

    /// The hotbar under the grid, taking what's dragged to it.
    fn draw_bar(&mut self, ctx: &mut Context, slots: &[Rect]) {
        self.draw_hotbar_in(ctx, slots);
        let catalog = ctx.world.items.clone();
        let theme = ctx.ui.theme;
        let (lifted, cursor) = (self.bag.screen.lifted, self.bag.screen.cursor);
        let fits = lifted.is_some_and(|lifted| self.fits_bar(ctx, &catalog, &lifted));
        for (slot, rect) in slots.iter().enumerate() {
            let inset = rect.expand(-ART_PIXEL);
            let there = self.bag.inventory.shortcut(slot).map(|piece| piece.id);
            if lifted.is_some_and(|l| l.side == Side::Bar && Some(l.id) == there) {
                ctx.ui.rect(inset, DARK);
            }
            let focused = cursor.side == Side::Bar && cursor.x == slot as u32 && !self.bag.screen.over_nothing;
            if focused || (lifted.is_some() && fits) {
                let line = if focused && lifted.is_some() && !fits { RED } else { theme.accent };
                ctx.ui.panel(inset, Vec4::ZERO, line);
            }
        }
    }

    /// What's known about an item, on a card beside `near`: to its right, or its left if the
    /// screen ends first.
    fn draw_item_card(&mut self, ctx: &mut Context, near: Rect, id: &str) {
        let catalog = ctx.world.items.clone();
        let Some(item) = catalog.get(id) else { return };
        let theme = ctx.ui.theme;
        let inner = CARD_WIDTH - CARD_PAD * 2.0;
        let description = TextStyle::new(8.0).color(DIM).wrap(inner);
        let text_height =
            if item.description.is_empty() { 0.0 } else { ctx.ui.measure(&item.description, &description).y + 12.0 };
        let numbers = stats(item);
        let stats_height = if numbers.is_empty() { 0.0 } else { STAT_ICON + 30.0 };
        let height = (CARD_PAD * 2.0 + CARD_ICON + 12.0 + text_height + stats_height).ceil();
        let screen = ctx.ui.size();
        let mut x = near.max().x + 8.0;
        if x + CARD_WIDTH > screen.x - 8.0 {
            x = near.min.x - 8.0 - CARD_WIDTH;
        }
        let y = near.min.y.min(screen.y - 8.0 - height).max(8.0);
        let card = Rect::new(Vec2::new(x, y).floor(), Vec2::new(CARD_WIDTH, height));
        ctx.ui.panel(card, CARD_BACK, MAUVE);

        let corner = card.min + Vec2::splat(CARD_PAD);
        if let Some(icon) = self.bag.icon(&catalog, &mut ctx.ui, id) {
            ctx.ui.image(Rect::new(corner, Vec2::splat(CARD_ICON)), icon, Vec4::ONE);
        }
        let beside = corner.x + CARD_ICON + 12.0;
        let name = TextStyle::new(16.0).color(theme.accent).wrap(card.max().x - CARD_PAD - beside);
        ctx.ui.text(Vec2::new(beside, corner.y + 4.0), &item.name.to_uppercase(), &name);
        let kind = match &item.kind {
            ItemKind::Weapon { .. } => "ARMA",
            ItemKind::Gun { .. } => "ARMA DE FOGO",
            ItemKind::Consumable { .. } => "CONSUMÍVEL",
            ItemKind::Grimoire { .. } => "GRIMÓRIO",
            ItemKind::Armor { .. } => "ARMADURA",
            ItemKind::Misc => "OBJETO",
        };
        let small = TextStyle::new(8.0).color(DIM).anchor(Anchor::BottomLeft);
        ctx.ui.text(Vec2::new(beside, corner.y + CARD_ICON - 4.0), kind, &small);
        let mut y = corner.y + CARD_ICON + 12.0;
        if !item.description.is_empty() {
            ctx.ui.text(Vec2::new(corner.x, y), &item.description, &description);
            y += text_height;
        }

        let start = card.min.x + (CARD_WIDTH - numbers.len() as f32 * CARD_STAT) * 0.5;
        for (at, (which, color, value, word)) in numbers.iter().enumerate() {
            let center = (start + (at as f32 + 0.5) * CARD_STAT).floor();
            if let Some(&image) = self.bag.screen.marks.get(*which) {
                let spot = Rect::new(Vec2::new(center - STAT_ICON - 2.0, y), Vec2::splat(STAT_ICON));
                ctx.ui.image(spot, image, *color);
            }
            let number = TextStyle::new(16.0).color(*color).anchor(Anchor::Left);
            ctx.ui.text(Vec2::new(center + 2.0, y + STAT_ICON * 0.5), value, &number);
            let small = TextStyle::new(8.0).color(DIM).anchor(Anchor::Top);
            ctx.ui.text(Vec2::new(center, y + STAT_ICON + 6.0), word, &small);
        }
    }

    /// Q and E, or clicking one, change the tab.
    fn switch_tab(&mut self, ctx: &mut Context) {
        let now = TABS.iter().position(|(tab, _)| *tab == self.bag.screen.tab).unwrap_or(0);
        let next_key = ctx.input.just_pressed(KeyCode::KeyE) || ctx.input.pad_just_pressed(PadButton::RightTrigger);
        let back_key = ctx.input.just_pressed(KeyCode::KeyQ) || ctx.input.pad_just_pressed(PadButton::LeftTrigger);
        let step = i32::from(next_key) - i32::from(back_key);
        let mut next = (now as i32 + step).rem_euclid(TABS.len() as i32) as usize;
        let input = ctx.ui.input();
        if input.mouse_pressed
            && let Some(at) = input.cursor
            && let Some(clicked) = tab_rects(ctx.ui.size()).iter().position(|rect| rect.contains(at))
        {
            next = clicked;
        }
        if next != now {
            self.bag.screen.tab = TABS[next].0;
            self.bag.screen.lifted = None;
            self.bag.screen.float_at = None;
            ctx.ui.reset_focus();
        }
    }

    fn draw_tabs(&mut self, ctx: &mut Context) {
        let cursor = ctx.ui.input().cursor;
        for (rect, (tab, name)) in tab_rects(ctx.ui.size()).into_iter().zip(TABS) {
            let chosen = self.bag.screen.tab == tab;
            let hovered = cursor.is_some_and(|at| rect.contains(at));
            let fill = if chosen {
                TAB_CHOSEN
            } else if hovered {
                TAB_HOVER
            } else {
                PANEL
            };
            ctx.ui.rect(rect, fill);
            let color = if chosen || hovered { PALE } else { DIM };
            let style = TextStyle::new(16.0).color(color).anchor(Anchor::Center);
            ctx.ui.text(rect.point(Anchor::Center).floor(), name, &style);
        }
    }

    fn draw_map(&mut self, ctx: &mut Context, panel: Rect) {
        let style = TextStyle::new(16.0).color(DIM).anchor(Anchor::Center);
        ctx.ui.text(panel.point(Anchor::Center).floor(), "O MAPA AINDA NÃO FOI DESENHADO", &style);
    }

    /// What's known about an item, on nothing at all.
    pub(crate) fn draw_item_details(&mut self, ctx: &mut Context, left: f32, top: f32, bottom: f32, id: &str) {
        let catalog = ctx.world.items.clone();
        let Some(item) = catalog.get(id) else { return };
        let theme = ctx.ui.theme;
        let middle = left + DETAILS_WIDTH * 0.5;
        let mut y = top;
        if let Some(icon) = self.bag.icon(&catalog, &mut ctx.ui, id) {
            let at = Vec2::new(middle - ART * 0.5, y).floor();
            ctx.ui.image(Rect::new(at, Vec2::splat(ART)), icon, Vec4::ONE);
        }
        y += ART + 10.0;
        let name = TextStyle::new(16.0).color(theme.accent).anchor(Anchor::Top);
        ctx.ui.text(Vec2::new(middle, y).floor(), &item.name.to_uppercase(), &name);
        y += 30.0;
        let description = TextStyle::new(8.0).color(DIM).wrap(DETAILS_WIDTH);
        let size = ctx.ui.measure(&item.description, &description);
        let text = ctx.ui.text(Vec2::new(middle - size.x * 0.5, y).floor(), &item.description, &description);
        y = (text.max().y + 20.0).max(bottom - STATS_ABOVE);

        let numbers = stats(item);
        let start = middle - numbers.len() as f32 * STAT_WIDTH * 0.5;
        for (at, (which, color, value, word)) in numbers.iter().enumerate() {
            let center = (start + (at as f32 + 0.5) * STAT_WIDTH).floor();
            if let Some(&image) = self.bag.screen.marks.get(*which) {
                let spot = Rect::new(Vec2::new(center - STAT_ICON - 2.0, y), Vec2::splat(STAT_ICON));
                ctx.ui.image(spot, image, *color);
            }
            let number = TextStyle::new(16.0).color(*color).anchor(Anchor::Left);
            ctx.ui.text(Vec2::new(center + 2.0, y + STAT_ICON * 0.5), value, &number);
            let small = TextStyle::new(8.0).color(DIM).anchor(Anchor::Top);
            ctx.ui.text(Vec2::new(center, y + STAT_ICON + 8.0), word, &small);
        }
    }

    pub(crate) fn draw_bag(&mut self, ctx: &mut Context) {
        if self.bag.screen.marks.is_empty() {
            self.bag.screen.marks = draw_marks(&mut ctx.ui);
        }
        let text_size = ctx.ui.theme.text_size;
        ctx.ui.theme.text_size = 16.0;
        let chest = self.bag.chest.is_some();
        if !chest {
            self.switch_tab(ctx);
        }
        ctx.ui.rect(ctx.ui.screen(), DARK);
        if !chest {
            self.draw_tabs(ctx);
        }
        let tab = if chest { Tab::Inventory } else { self.bag.screen.tab };
        let keys = if tab == Tab::Inventory {
            self.draw_inventory(ctx)
        } else {
            let own = self.bag.inventory.clone();
            let layout = layout(ctx.ui.size(), &own, None, tab);
            for panel in &layout.panels {
                ctx.ui.rect(*panel, PANEL);
            }
            match tab {
                Tab::Crafting => self.draw_crafting(ctx, &layout),
                Tab::Skills => self.draw_skills(ctx, layout.content),
                Tab::Diary => self.draw_diary(ctx, layout.content),
                _ => self.draw_map(ctx, layout.content),
            }
            if ctx.input.gamepad_last() { TAB_KEYS_PAD } else { TAB_KEYS }
        };
        let size = ctx.ui.size();
        let style = TextStyle::new(8.0).color(DIM).anchor(Anchor::Bottom);
        ctx.ui.text(Vec2::new(size.x * 0.5, size.y - 12.0).floor(), keys, &style);
        ctx.ui.theme.text_size = text_size;
        self.hud.draw_with_hint(ctx, "");
    }

    /// The inventory tab, or a chest: returns the keys to show.
    fn draw_inventory(&mut self, ctx: &mut Context) -> &'static str {
        let screen_size = ctx.ui.size();
        let input = ctx.ui.input();
        let Some(own) = self.side_inventory(ctx, Side::Own) else { return "" };
        let chest = self.bag.chest.is_some().then(|| self.side_inventory(ctx, Side::Chest)).flatten();
        let before = layout(screen_size, &own, chest.as_ref(), Tab::Inventory);
        self.bag_input(ctx, &before, input);

        // The keys may have changed anything.
        let Some(own) = self.side_inventory(ctx, Side::Own) else { return "" };
        let chest = self.bag.chest.is_some().then(|| self.side_inventory(ctx, Side::Chest)).flatten();
        let layout = layout(screen_size, &own, chest.as_ref(), Tab::Inventory);
        for panel in &layout.panels {
            ctx.ui.rect(*panel, PANEL);
        }
        if let Some(panel) = layout.body {
            self.draw_body(ctx, panel);
        }
        if chest.is_none() {
            self.draw_character(ctx, &layout);
        }
        for field in &layout.fields {
            match field.side {
                Side::Own => self.draw_field(ctx, *field, &own, "MOCHILA", true, &[]),
                Side::Chest => {
                    if let Some(chest) = &chest {
                        self.draw_field(ctx, *field, chest, "BAÚ", true, &[]);
                    }
                }
                Side::Body | Side::Bar => {}
            }
        }
        self.draw_bar(ctx, &layout.bar);

        let body = self.bag.body.clone();
        let grid = |side| match side {
            Side::Own | Side::Bar => Some(&own),
            Side::Chest => chest.as_ref(),
            Side::Body => Some(&body),
        };
        let screen = &self.bag.screen;
        let (lifted, cursor) = (screen.lifted, screen.cursor);
        let theme = ctx.ui.theme;
        let field_of = |side| layout.fields.iter().copied().find(|field: &Field| field.side == side);
        let over_nothing = screen.over_nothing;
        let float_at = screen.float_at;

        let mut card = None;
        if let Some(lifted) = lifted {
            // The piece over where it would land, red if it can't.
            let on_grid = !matches!(cursor.side, Side::Body | Side::Bar) && !over_nothing;
            let piece = grid(lifted.side).and_then(|g| g.piece(lifted.id)).cloned();
            if on_grid
                && let (Some(piece), Some(field), Some(target)) = (piece, field_of(cursor.side), grid(cursor.side))
            {
                let size = if lifted.turned == piece.turned { piece.size } else { (piece.size.1, piece.size.0) };
                let moving = (lifted.side == cursor.side).then_some(lifted.id);
                let catalog = ctx.world.items.clone();
                let landing = target.landing(&catalog, &piece.stack.item, moving, (cursor.x, cursor.y), size);
                let rect = field.cells(cursor.x, cursor.y, size);
                let blocked = if lifted.side == Side::Body {
                    !self.can_leave_body(&catalog, &lifted, (cursor.x, cursor.y), size)
                } else {
                    landing == Landing::Blocked
                };
                let line = if blocked { RED } else { theme.accent };
                ctx.ui.rect(rect.expand(-ART_PIXEL), CHOSEN_BACK);
                self.draw_piece_picture(ctx, rect, &piece.stack.item, lifted.turned, Vec4::ONE);
                ctx.ui.panel(rect.expand(-ART_PIXEL), Vec4::ZERO, line);
            }
        } else if !over_nothing {
            match cursor.side {
                Side::Body => {
                    let place = layout.body.map(|panel| panel.place(cursor.x));
                    card = place.zip(self.in_place(cursor.x)).map(|(rect, piece)| (rect, piece.stack.item));
                }
                Side::Bar => {
                    let piece = own.shortcut(cursor.x as usize).map(|piece| piece.stack.item.clone());
                    card = layout.bar.get(cursor.x as usize).copied().zip(piece);
                }
                side => {
                    if let Some(field) = field_of(side) {
                        match grid(side).and_then(|g| g.at(cursor.x, cursor.y)) {
                            Some(piece) => {
                                card = Some((field.cells(piece.x, piece.y, piece.size), piece.stack.item.clone()));
                            }
                            None => {
                                let rect = field.cells(cursor.x, cursor.y, (1, 1)).expand(-ART_PIXEL);
                                ctx.ui.panel(rect, Vec4::ZERO, theme.border);
                            }
                        }
                    }
                }
            }
        }
        if let Some((near, id)) = card {
            self.draw_item_card(ctx, near, &id);
        }

        // Above everything else, on the mouse, wherever that goes.
        let cell = layout.fields.last().map_or(48.0, |field| field.cell);
        if let (Some(lifted), Some(at)) = (lifted, float_at)
            && let Some(piece) = grid(lifted.side).and_then(|g| g.piece(lifted.id)).cloned()
        {
            let size = if lifted.turned == piece.turned { piece.size } else { (piece.size.1, piece.size.0) };
            let shape = Vec2::new(size.0 as f32, size.1 as f32) * cell;
            let rect = Rect::anchored(at.floor(), shape, Anchor::Center);
            self.draw_piece_picture(ctx, rect, &piece.stack.item, lifted.turned, Vec4::ONE);
        }
        keys_of(&ctx.input, lifted.is_some(), chest.is_some())
    }
}
