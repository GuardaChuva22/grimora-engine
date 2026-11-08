//! The inventory screen, also the inside of a chest: pieces lying on a grid with no lines, one
//! lifted at a time and put down somewhere else, and what's known about the one chosen. Nothing
//! here uses the interface's widgets, since what's chosen is a cell and not a widget.

use std::collections::HashMap;

use grimora::portrait::{PortraitShot, Sitter, camera_facing, yaw_of};
use grimora::prelude::*;
use grimora::render::{ART_PIXEL, ImageId, UiInput};
use grimora::scene::ArmorSlot;

use crate::Sandbox;
use crate::bag::{ARMOR_SLOTS, cell_of, hotbar_slot, slot_of};

/// The player's grid.
pub const COLUMNS: u32 = 10;
pub const ROWS: u32 = 6;
/// A cell on screen, which is [`CELL_ART`] texels of a piece's picture at [`ART_PIXEL`] each.
const CELL: f32 = 48.0;
const TITLE_GAP: f32 = 12.0;
/// From the bottom of one grid to the top of the next one's title.
const FIELD_GAP: f32 = 44.0;
const DETAILS_WIDTH: f32 = 300.0;
const DETAILS_GAP: f32 = 48.0;
/// The square picture in the details, three texels of it for each of its 48.
const ART: f32 = 144.0;
const STAT_ICON: f32 = 24.0;
const STAT_WIDTH: f32 = 96.0;
/// How far above the bottom of the grid the numbers start.
const STATS_ABOVE: f32 = 62.0;
const KEYS_GAP: f32 = 22.0;

/// The body, left of the grid: a column of places, the character, and the hand on the other side.
const PLACE: f32 = 64.0;
const FIGURE_ART: (u32, u32) = (90, 144);
const FIGURE_SCALE: f32 = 2.0;
const FIGURE_FOV: f32 = 26.0;
const FIGURE_DISTANCE: f32 = 5.0;
/// Which height of the character is in the middle of the picture.
const FIGURE_MIDDLE: f32 = 0.93;
/// How much of a turn the character starts at, which shows the armor's shape better than the front.
const FIGURE_START: f32 = 0.35;
/// Radians for each pixel the mouse drags it.
const FIGURE_DRAG: f32 = 0.012;
const BODY_GAP: f32 = 44.0;
/// [`Cursor::x`] on [`Side::Body`] for the hand: the ones before it are in [`ARMOR_SLOTS`].
const BODY_PLACES: u32 = ARMOR_SLOTS.len() as u32;
const PLACE_WORDS: [&str; 4] = ["CABEÇA", "PEITO", "PERNAS", "PÉS"];

const DARK: Vec4 = Vec4::new(0.006, 0.008, 0.03, 0.93);
const FIELD: Vec4 = Vec4::new(0.0, 0.0, 0.015, 0.75);
const DOT: Vec4 = Vec4::new(0.55, 0.5, 0.62, 0.45);
const PIECE_BACK: Vec4 = Vec4::new(0.11, 0.12, 0.24, 0.42);
const CHOSEN_BACK: Vec4 = Vec4::new(0.26, 0.25, 0.42, 0.7);
const LEFT_BEHIND: Vec4 = Vec4::new(1.0, 1.0, 1.0, 0.3);
const DIM: Vec4 = Vec4::new(0.72, 0.7, 0.72, 0.9);

const RED: Vec4 = Vec4::new(0.95, 0.32, 0.28, 1.0);
const GREEN: Vec4 = Vec4::new(0.42, 0.88, 0.5, 1.0);
const BLUE: Vec4 = Vec4::new(0.45, 0.68, 1.0, 1.0);
const FROST: Vec4 = Vec4::new(0.6, 0.9, 1.0, 1.0);

const USE_PIECE: &str = "use_slot";
const DROP_PIECE: &str = "drop_slot";
const TURN_PIECE: &str = "turn_piece";
const ARRANGE: &str = "arrange_bag";

const KEYS: &str = "ENTER  PEGAR    R  USAR    X  LARGAR    O  ORGANIZAR    I  VOLTAR";
const KEYS_PAD: &str = "A  PEGAR    X  USAR    Y  LARGAR    LB  ORGANIZAR    B  VOLTAR";
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
}

impl Field {
    fn size(&self) -> Vec2 {
        Vec2::new(self.columns as f32, self.rows as f32) * CELL
    }

    fn rect(&self) -> Rect {
        Rect::new(self.origin, self.size())
    }

    /// The screen area of some cells.
    fn cells(&self, x: u32, y: u32, size: (u32, u32)) -> Rect {
        let min = self.origin + Vec2::new(x as f32, y as f32) * CELL;
        Rect::new(min, Vec2::new(size.0 as f32, size.1 as f32) * CELL)
    }

    /// The middle of a cell on the screen.
    #[cfg(test)]
    pub fn center_of(&self, x: u32, y: u32) -> Vec2 {
        self.cells(x, y, (1, 1)).point(Anchor::Center)
    }

    /// The cell under a point on the screen.
    fn cell_at(&self, point: Vec2) -> Option<(u32, u32)> {
        let from = (point - self.origin) / CELL;
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
        PLACE * 2.0 + FIGURE_ART.0 as f32 * FIGURE_SCALE
    }

    fn figure(&self) -> Rect {
        let size = Vec2::new(FIGURE_ART.0 as f32, FIGURE_ART.1 as f32) * FIGURE_SCALE;
        Rect::new(self.origin + Vec2::new(PLACE, (self.height - size.y) * 0.5), size)
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

/// Where the body goes: with a chest open, the screen is for the chest.
fn body_panel(screen: Vec2, own: &Inventory, chest: Option<&Inventory>) -> Option<BodyPanel> {
    if chest.is_some() {
        return None;
    }
    let (fields, _) = layout(screen, own, None);
    let field = fields.first()?;
    let origin = Vec2::new(field.origin.x - BODY_GAP - BodyPanel::width(), field.origin.y);
    Some(BodyPanel { origin, height: field.size().y })
}

/// The grids on the screen, the chest over the player's, and where the details start.
fn layout(screen: Vec2, own: &Inventory, chest: Option<&Inventory>) -> (Vec<Field>, f32) {
    let field = |side, inventory: &Inventory| Field {
        side,
        origin: Vec2::ZERO,
        columns: inventory.width(),
        rows: inventory.height(),
    };
    let own_field = field(Side::Own, own);
    let chest_field = chest.map(|chest| field(Side::Chest, chest));
    let widest = chest_field.map_or(0.0, |chest| chest.size().x).max(own_field.size().x);
    let above = chest_field.map_or(0.0, |chest| chest.size().y + FIELD_GAP);
    let body = if chest.is_none() { BodyPanel::width() + BODY_GAP } else { 0.0 };
    let total = Vec2::new(body + widest + DETAILS_GAP + DETAILS_WIDTH, above + own_field.size().y);
    let origin = ((screen - total) * 0.5).floor().max(Vec2::new(8.0, 36.0)) + Vec2::X * body;
    let mut fields = Vec::new();
    if let Some(chest) = chest_field {
        fields.push(Field { origin, ..chest });
    }
    fields.push(Field { origin: origin + Vec2::Y * above, ..own_field });
    (fields, origin.x + widest + DETAILS_GAP)
}

/// The body with no chest open.
#[cfg(test)]
pub fn own_body(ctx: &Context, own: &Inventory) -> BodyPanel {
    body_panel(ctx.ui.size(), own, None).unwrap()
}

/// The player's grid with no chest open, and where the details start.
#[cfg(test)]
pub fn own_field(ctx: &Context, own: &Inventory) -> (Field, f32) {
    let (fields, left) = layout(ctx.ui.size(), own, None);
    (fields[0], left)
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
    fn bag_input(&mut self, ctx: &mut Context, fields: &[Field], panel: Option<BodyPanel>, input: UiInput) {
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
        let screen_size = ctx.ui.size();
        let on_bar = mouse.and_then(|at| (0..HOTBAR).find(|&slot| hotbar_slot(screen_size, slot).contains(at)));
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
            && from.distance(to) >= CELL * 0.5
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

    fn draw_field(&mut self, ctx: &mut Context, field: Field, inventory: &Inventory, title: &str) {
        let theme = ctx.ui.theme;
        let title_at = field.origin - Vec2::Y * TITLE_GAP;
        ctx.ui.text(title_at, title, &TextStyle::new(16.0).color(theme.text).anchor(Anchor::BottomLeft));
        let room = format!("{} DE {}", inventory.cells_taken(), inventory.width() * inventory.height());
        let room_at = title_at + Vec2::X * field.size().x;
        ctx.ui.text(room_at, &room, &TextStyle::new(8.0).color(DIM).anchor(Anchor::BottomRight));
        ctx.ui.rect(field.rect(), FIELD);
        for row in 0..=field.rows {
            for column in 0..=field.columns {
                let at = field.origin + Vec2::new(column as f32, row as f32) * CELL - Vec2::splat(ART_PIXEL * 0.5);
                ctx.ui.rect(Rect::new(at, Vec2::splat(ART_PIXEL)), DOT);
            }
        }

        let screen = &self.bag.screen;
        let (lifted, cursor) = (screen.lifted, screen.cursor);
        let chosen =
            inventory.at(cursor.x, cursor.y).filter(|_| lifted.is_none() && cursor.side == field.side).map(|p| p.id);

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
            }
            if piece.stack.count > 1 {
                let style = TextStyle::new(8.0).color(theme.accent).anchor(Anchor::BottomRight);
                ctx.ui.text(inset.max() - Vec2::splat(3.0), &piece.stack.count.to_string(), &style);
            }
        }
    }

    /// The character as it stands, with a place around it for each thing it can have on.
    fn draw_body(&mut self, ctx: &mut Context, panel: BodyPanel) {
        let catalog = ctx.world.items.clone();
        let theme = ctx.ui.theme;
        let title_at = panel.origin - Vec2::Y * TITLE_GAP;
        ctx.ui.text(title_at, "CORPO", &TextStyle::new(16.0).color(theme.text).anchor(Anchor::BottomLeft));
        let defense = format!("{:.0} DE DEFESA", self.bag.defense(&catalog));
        let defense_at = title_at + Vec2::X * BodyPanel::width();
        ctx.ui.text(defense_at, &defense, &TextStyle::new(8.0).color(DIM).anchor(Anchor::BottomRight));

        if let (Some(image), Some(body)) =
            (self.picture(ctx, "bag_figure", FIGURE_ART), ctx.world.transform(self.avatar))
        {
            let yaw = yaw_of(&body) + FIGURE_START + self.bag.screen.figure_turn;
            let middle = body.position + Vec3::Y * FIGURE_MIDDLE;
            let camera = camera_facing(middle, yaw, FIGURE_DISTANCE, FIGURE_FOV);
            ctx.portraits.push(PortraitShot::new(image, Sitter::Entity(self.avatar), camera));
            ctx.ui.image(panel.figure(), image, Vec4::ONE);
        }

        let screen = &self.bag.screen;
        let (lifted, cursor) = (screen.lifted, screen.cursor);
        for place in 0..BODY_PLACES {
            let rect = panel.place(place);
            let inset = rect.expand(-ART_PIXEL);
            let focused = cursor.side == Side::Body && cursor.x == place;
            let there = self.in_place(place);
            ctx.ui.rect(rect, FIELD);
            for corner in [rect.min, rect.min + Vec2::X * PLACE, rect.min + Vec2::Y * PLACE, rect.max()] {
                ctx.ui.rect(Rect::new(corner - Vec2::splat(ART_PIXEL * 0.5), Vec2::splat(ART_PIXEL)), DOT);
            }
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

    /// The hotbar over the screen, taking what's dragged to it.
    fn draw_bar(&mut self, ctx: &mut Context) {
        self.draw_hotbar(ctx);
        let catalog = ctx.world.items.clone();
        let theme = ctx.ui.theme;
        let size = ctx.ui.size();
        let (lifted, cursor) = (self.bag.screen.lifted, self.bag.screen.cursor);
        let fits = lifted.is_some_and(|lifted| self.fits_bar(ctx, &catalog, &lifted));
        for slot in 0..HOTBAR {
            let inset = hotbar_slot(size, slot).expand(-ART_PIXEL);
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
        let screen_size = ctx.ui.size();
        let input = ctx.ui.input();
        let Some(own) = self.side_inventory(ctx, Side::Own) else { return };
        let chest = self.bag.chest.is_some().then(|| self.side_inventory(ctx, Side::Chest)).flatten();
        let (fields, _) = layout(screen_size, &own, chest.as_ref());
        let panel = body_panel(screen_size, &own, chest.as_ref());
        self.bag_input(ctx, &fields, panel, input);

        // The keys may have changed anything.
        let Some(own) = self.side_inventory(ctx, Side::Own) else { return };
        let chest = self.bag.chest.is_some().then(|| self.side_inventory(ctx, Side::Chest)).flatten();
        let (fields, details_left) = layout(screen_size, &own, chest.as_ref());
        ctx.ui.rect(ctx.ui.screen(), DARK);
        if let Some(panel) = panel {
            self.draw_body(ctx, panel);
        }
        for field in &fields {
            match field.side {
                Side::Own => self.draw_field(ctx, *field, &own, "MOCHILA"),
                Side::Chest => {
                    if let Some(chest) = &chest {
                        self.draw_field(ctx, *field, chest, "BAÚ");
                    }
                }
                Side::Body | Side::Bar => {}
            }
        }
        self.draw_bar(ctx);

        let body = self.bag.body.clone();
        let grid = |side| match side {
            Side::Own | Side::Bar => Some(&own),
            Side::Chest => chest.as_ref(),
            Side::Body => Some(&body),
        };
        let screen = &self.bag.screen;
        let (lifted, cursor) = (screen.lifted, screen.cursor);
        let theme = ctx.ui.theme;
        let field_of = |side| fields.iter().copied().find(|field: &Field| field.side == side);

        let mut shown = None;
        let over_nothing = screen.over_nothing;
        let float_at = screen.float_at;
        if matches!(cursor.side, Side::Body | Side::Bar) || (lifted.is_some() && over_nothing) {
            // The places draw themselves, and what's lifted says what it is.
            shown = match lifted {
                Some(lifted) => grid(lifted.side).and_then(|g| g.piece(lifted.id)).map(|p| p.stack.item.clone()),
                None if cursor.side == Side::Body => self.in_place(cursor.x).map(|piece| piece.stack.item),
                None => own.shortcut(cursor.x as usize).map(|piece| piece.stack.item.clone()),
            };
        } else if let Some(lifted) = lifted {
            // The piece over where it would land, red if it can't.
            let piece = grid(lifted.side).and_then(|g| g.piece(lifted.id)).cloned();
            if let (Some(piece), Some(field), Some(target)) = (piece, field_of(cursor.side), grid(cursor.side)) {
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
                shown = Some(piece.stack.item);
            }
        } else if let Some(field) = field_of(cursor.side) {
            let piece = grid(cursor.side).and_then(|g| g.at(cursor.x, cursor.y));
            match piece {
                Some(piece) => shown = Some(piece.stack.item.clone()),
                None => {
                    let rect = field.cells(cursor.x, cursor.y, (1, 1)).expand(-ART_PIXEL);
                    ctx.ui.panel(rect, Vec4::ZERO, theme.border);
                }
            }
        }

        let top = fields.first().map_or(0.0, |field| field.origin.y);
        let bottom = fields.last().map_or(0.0, |field| field.rect().max().y);
        if let Some(id) = shown {
            self.draw_item_details(ctx, details_left, top, bottom, &id);
        }

        // Above everything else, on the mouse, wherever that goes.
        if let (Some(lifted), Some(at)) = (lifted, float_at)
            && let Some(piece) = grid(lifted.side).and_then(|g| g.piece(lifted.id)).cloned()
        {
            let size = if lifted.turned == piece.turned { piece.size } else { (piece.size.1, piece.size.0) };
            let shape = Vec2::new(size.0 as f32, size.1 as f32) * CELL;
            let rect = Rect::anchored(at.floor(), shape, Anchor::Center);
            self.draw_piece_picture(ctx, rect, &piece.stack.item, lifted.turned, Vec4::ONE);
        }

        let keys = keys_of(&ctx.input, lifted.is_some(), chest.is_some());
        let style = TextStyle::new(8.0).color(DIM).anchor(Anchor::Top);
        ctx.ui.text(Vec2::new(screen_size.x * 0.5, bottom + KEYS_GAP).floor(), keys, &style);
        self.hud.draw_with_hint(ctx, "");
    }
}
