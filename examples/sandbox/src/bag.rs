//! What the wanderer carries: the hotbar always on screen, the inventory screen (also
//! the inside of a chest), and the weapon in the hand. The world goes on while the screen is
//! open, as in the games it copies: the character just stands there, and can be hit.

use std::collections::HashMap;

use grimora::hecs;
use grimora::prelude::*;
use grimora::render::{ImageId, Slot};
use grimora::scene::ArmorSlot;

use crate::bag_screen::{COLUMNS, Cursor, ROWS, Screen, Side, Tab};
use crate::{Mode, STEP_SOUND, Sandbox};

/// The hotbar on the HUD, with its numbers under it.
const HOTBAR_SIZE: f32 = 48.0;
const HOTBAR_GAP: f32 = 6.0;
const HOTBAR_BOTTOM: f32 = 30.0;
/// A weapon says nothing the player needs to keep reading: its box shows for this many
/// seconds after it's taken out, the last of them fading away.
const WEAPON_NOTE_SECONDS: f32 = 3.0;
const WEAPON_NOTE_FADE: f32 = 0.6;
/// Where a weapon put down lands, in front of the character.
pub(crate) const DROP_AHEAD: f32 = 0.9;

const DIM: Vec4 = Vec4::new(0.78, 0.75, 0.7, 0.85);

pub(crate) const NO_ROOM: &str = "Não há lugar na mochila";
/// In order, on the body's grid: each has a column of [`PLACE_CELLS`] cells, with room for
/// any piece, which is how putting one on takes off the one there and nothing else.
pub const ARMOR_SLOTS: [ArmorSlot; 4] = [ArmorSlot::Head, ArmorSlot::Body, ArmorSlot::Legs, ArmorSlot::Feet];
const PLACE_CELLS: u32 = 4;

/// What's on the body: an inventory, so that a piece goes in and out the way it goes to a chest.
pub fn body_grid() -> Inventory {
    Inventory::new(ARMOR_SLOTS.len() as u32 * PLACE_CELLS, PLACE_CELLS)
}

/// The cell where a piece on the body goes.
pub fn cell_of(slot: ArmorSlot) -> (u32, u32) {
    let column = ARMOR_SLOTS.iter().position(|&place| place == slot).unwrap_or(0);
    (column as u32 * PLACE_CELLS, 0)
}

/// Where an item goes on the body, if it does.
pub fn slot_of(catalog: &ItemCatalog, item: &str) -> Option<ArmorSlot> {
    match catalog.get(item)?.kind {
        ItemKind::Armor { slot, .. } => Some(slot),
        _ => None,
    }
}

pub const OPEN_BAG: &str = "inventory";
const OPEN_SOUND: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/ui/open_inv.mp3");
const CLOSE_SOUND: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/ui/close_inv.mp3");
pub(crate) const USE_SLOT: &str = "use_slot";
pub(crate) const DROP_SLOT: &str = "drop_slot";
const WEAPON_NEXT: &str = "weapon_next";
const WEAPON_PREVIOUS: &str = "weapon_previous";
const EAT: &str = "eat";
pub(crate) const HOTBAR_KEYS: [(&str, KeyCode); HOTBAR] = [
    ("hotbar_1", KeyCode::Digit1),
    ("hotbar_2", KeyCode::Digit2),
    ("hotbar_3", KeyCode::Digit3),
    ("hotbar_4", KeyCode::Digit4),
    ("hotbar_5", KeyCode::Digit5),
];

/// What the character can do with what's in the hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeldKind {
    Weapon,
    Gun,
    Book,
    /// Anything else, which is only held, offered and maybe eaten.
    Thing,
}

/// What's in the hand.
pub(crate) struct Equipped {
    pub piece: u32,
    pub item: String,
    /// The model, following the hand.
    pub held: Entity,
    pub kind: HeldKind,
}

#[derive(Default)]
pub struct Bag {
    pub inventory: Inventory,
    /// What's on the body, which takes no room in the inventory.
    pub body: Inventory,
    /// What's showing on the body: the item and the model following it.
    dressed: Vec<(String, Entity)>,
    /// The starting things are in the inventory.
    stocked: bool,
    pub open: bool,
    /// Whether the screen was open the last time its sound played.
    sounded_open: bool,
    /// The container being looked into.
    pub chest: Option<String>,
    /// The screen's cursor and what it's moving.
    pub screen: Screen,
    /// Each item's picture, once it's been asked for. `None` if the file is missing.
    icons: HashMap<String, Option<ImageId>>,
    pub(crate) equipped: Option<Equipped>,
    /// The slot of the hotbar last picked, which may be an empty one.
    selected: usize,
    /// What's carried on the body and the change of what's in the hand.
    pub(crate) gear: crate::gear::Gear,
    /// A `--menu inventory` or `--menu chest` run's screen and the frame to open it on.
    pub scheduled: Option<(bool, u64)>,
    /// What rises from anything on the ground.
    glint: Option<ParticleEffect>,
    /// The weapon whose box is showing, and how long ago it was taken out.
    weapon_note: Option<(String, f32)>,
}

impl Bag {
    pub fn close(&mut self) {
        self.open = false;
        self.chest = None;
        self.screen.lifted = None;
    }

    pub fn armed(&self) -> bool {
        self.equipped.as_ref().is_some_and(|equipped| equipped.kind == HeldKind::Weapon)
    }

    /// The gun in the hand, if that's what's there.
    pub fn gun(&self) -> Option<Entity> {
        self.equipped.as_ref().filter(|equipped| equipped.kind == HeldKind::Gun).map(|equipped| equipped.held)
    }

    pub fn gun_item(&self) -> Option<&str> {
        self.equipped.as_ref().filter(|equipped| equipped.kind == HeldKind::Gun).map(|equipped| equipped.item.as_str())
    }

    /// The book in the hand, if that's what's there.
    pub fn book(&self) -> Option<Entity> {
        self.equipped.as_ref().filter(|equipped| equipped.kind == HeldKind::Book).map(|equipped| equipped.held)
    }

    pub fn book_item(&self) -> Option<&str> {
        self.equipped.as_ref().filter(|equipped| equipped.kind == HeldKind::Book).map(|equipped| equipped.item.as_str())
    }

    /// For a run that captures a spell.
    pub fn put_book_in_hand(&mut self, catalog: &ItemCatalog) {
        self.put_in_hand(catalog, crate::spells::GRIMOIRE);
    }

    /// Takes one of these out, getting it from nowhere if there's none.
    pub fn put_in_hand(&mut self, catalog: &ItemCatalog, item: &str) {
        if self.inventory.first_of(item).is_none() {
            self.inventory.add(catalog, item, 1);
        }
        let piece = self.inventory.first_of(item).map(|piece| piece.id);
        self.inventory.set_hand(piece);
        self.gear.skip_change();
    }

    /// Starts over with what a new game has.
    pub fn reset(&mut self) {
        self.close();
        self.inventory = Inventory::default();
        self.body = body_grid();
        self.stocked = false;
    }

    /// The piece worn in this place.
    pub fn worn_in(&self, slot: ArmorSlot) -> Option<&Placed> {
        let (x, y) = cell_of(slot);
        self.body.at(x, y)
    }

    /// Puts on a piece of the inventory, and what was in its place goes where this one was.
    /// Returns false if that doesn't fit or it's not something to wear.
    pub fn wear(&mut self, catalog: &ItemCatalog, id: u32) -> bool {
        let Some(piece) = self.inventory.piece(id) else { return false };
        let Some(slot) = slot_of(catalog, &piece.stack.item) else { return false };
        self.inventory.move_piece_to(catalog, id, &mut self.body, cell_of(slot), false)
    }

    /// Back in the inventory. Returns false if there's no room for it.
    pub fn take_off(&mut self, catalog: &ItemCatalog, slot: ArmorSlot) -> bool {
        let Some(piece) = self.worn_in(slot).cloned() else { return true };
        if self.inventory.add(catalog, &piece.stack.item, piece.stack.count) > 0 {
            return false;
        }
        self.body.take(piece.id, piece.stack.count);
        true
    }

    /// All that's taken off a blow.
    pub fn defense(&self, catalog: &ItemCatalog) -> f32 {
        let worn = self.body.pieces().iter().filter_map(|piece| catalog.get(&piece.stack.item));
        worn.map(|item| if let ItemKind::Armor { defense, .. } = item.kind { defense } else { 0.0 })
            .fold(0.0, |all, defense| all + defense)
    }

    /// The model in the hand is gone with the character it was in.
    pub fn forget_hand(&mut self) {
        self.equipped = None;
        self.gear.forget();
    }

    pub(crate) fn icon(&mut self, catalog: &ItemCatalog, ui: &mut Ui, id: &str) -> Option<ImageId> {
        if let Some(&icon) = self.icons.get(id) {
            return icon;
        }
        let picture = catalog.get(id).and_then(|item| {
            let texture = grimora::scene::load_texture(catalog.icon_path(item));
            texture.map_err(|err| eprintln!("[sandbox] {err:#}")).ok()
        });
        let icon = picture.and_then(|t| ui.add_image(t.width, t.height, &t.rgba));
        self.icons.insert(id.to_owned(), icon);
        icon
    }

    /// How `stack` shows in a slot. `short` borrows from it, for when there's no picture.
    fn slot<'a>(&mut self, catalog: &ItemCatalog, ui: &mut Ui, stack: &'a Option<ItemStack>, marked: bool) -> Slot<'a> {
        let Some(stack) = stack else { return Slot { marked, ..Default::default() } };
        let end = stack.item.char_indices().nth(3).map_or(stack.item.len(), |(at, _)| at);
        let image = self.icon(catalog, ui, &stack.item);
        Slot { image, short: &stack.item[..end], count: stack.count, marked }
    }
}

/// What a capture of the screen has, on top of what a new game starts with.
const CAPTURE_ITEMS: [(&str, u32); 14] = [
    ("capacete_ferro", 1),
    ("peitoral_ferro", 1),
    ("calca_ferro", 1),
    ("botas_ferro", 1),
    ("zweihander", 1),
    ("rapieira", 1),
    ("cimitarra", 1),
    ("machado_de_abordagem", 1),
    ("maca", 3),
    ("bala_de_chumbo", 30),
    ("pistola", 1),
    ("lanterna", 1),
    ("adaga", 1),
    ("mosquete", 1),
];

/// Where a slot of the hotbar is on the screen.
pub(crate) fn hotbar_slot(screen: Vec2, at: usize) -> Rect {
    let width = HOTBAR as f32 * (HOTBAR_SIZE + HOTBAR_GAP) - HOTBAR_GAP;
    let x = (screen.x - width) * 0.5 + at as f32 * (HOTBAR_SIZE + HOTBAR_GAP);
    Rect::new(Vec2::new(x, screen.y - HOTBAR_BOTTOM - HOTBAR_SIZE), Vec2::splat(HOTBAR_SIZE))
}

/// The controls of what's in this file, added to the ones of `menu.rs`.
pub fn add_controls(controls: &mut ActionMap) {
    controls.add(OPEN_BAG, [Binding::Key(KeyCode::KeyI), Binding::Pad(PadButton::Select)]);
    controls.add(
        USE_SLOT,
        [Binding::Key(KeyCode::KeyR), Binding::Mouse(MouseButton::Right), Binding::Pad(PadButton::West)],
    );
    controls.add(DROP_SLOT, [Binding::Key(KeyCode::KeyX), Binding::Pad(PadButton::North)]);
    controls.add(WEAPON_NEXT, [Binding::Pad(PadButton::DPadRight)]);
    controls.add(WEAPON_PREVIOUS, [Binding::Pad(PadButton::DPadLeft)]);
    controls.add(EAT, [Binding::Pad(PadButton::DPadUp)]);
    for (action, key) in HOTBAR_KEYS {
        controls.add(action, [Binding::Key(key)]);
    }
    crate::bag_screen::add_controls(controls);
}

impl Sandbox {
    /// Gives a new game its first things.
    pub(crate) fn stock_bag(&mut self, ctx: &mut Context) {
        if ctx.world.items.iter().next().is_none() {
            // A scene with no items of its own still has a sword to hold.
            match ItemCatalog::load(crate::ITEMS) {
                Ok(catalog) => ctx.world.items = catalog,
                Err(err) => eprintln!("[sandbox] {err:#}"),
            }
        }
        self.bag.forget_hand();
        self.bag.glint = load_effect(crate::ITEM_GLINT).map_err(|err| eprintln!("[sandbox] {err:#}")).ok();
        if !self.bag.stocked {
            self.bag.stocked = true;
            self.bag.inventory = Inventory::new(COLUMNS, ROWS);
            self.bag.body = body_grid();
            self.bag.inventory.add(&ctx.world.items.clone(), crate::FIRST_WEAPON, 1);
            // In the last slot of the bar, which leaves the others for what's picked up.
            self.bag.inventory.add(&ctx.world.items.clone(), crate::spells::GRIMOIRE, 1);
            let book = self.bag.inventory.first_of(crate::spells::GRIMOIRE).map(|piece| piece.id);
            self.bag.inventory.set_shortcut(HOTBAR - 1, book);
            let first = self.bag.inventory.first_of(crate::FIRST_WEAPON).map(|piece| piece.id);
            self.bag.inventory.set_shortcut(0, first);
            self.bag.inventory.set_hand(first);
            self.bag.inventory.add(&ctx.world.items.clone(), "moeda", 15);
        }
    }

    /// Handles what's carried this frame. Returns whether the inventory screen is open,
    /// which keeps the character from taking orders.
    pub(crate) fn run_bag(&mut self, ctx: &mut Context) -> bool {
        self.capture_bag(ctx);
        let toggled = ctx.input.action_just_pressed(OPEN_BAG);
        let diary = ctx.input.action_just_pressed(crate::tale::OPEN_JOURNAL);
        if self.bag.open {
            let back = ctx.input.pad_just_pressed(PadButton::East);
            let on_diary = self.bag.chest.is_none() && self.bag.screen.tab == Tab::Diary;
            // The back button lets go of what's lifted before it closes anything.
            if toggled || (diary && on_diary) || (back && !self.drop_lifted()) {
                self.bag.close();
            } else if diary && self.bag.chest.is_none() {
                self.bag.screen.tab = Tab::Diary;
                self.bag.screen.lifted = None;
                ctx.ui.reset_focus();
            }
        } else if (toggled || diary) && self.mode == Mode::Play && self.dead_for.is_none() {
            self.open_bag(None);
            if diary {
                self.bag.screen.tab = Tab::Diary;
            }
            ctx.ui.reset_focus();
        }
        if self.bag.open {
            ctx.set_cursor_grab(false);
            self.draw_bag(ctx);
        } else {
            self.draw_hotbar(ctx);
            self.draw_weapon_note(ctx);
            if self.mode == Mode::Play && self.dead_for.is_none() {
                self.hotbar_keys(ctx);
            }
        }
        // However it opened or closed (a key, a chest, the pause menu), it sounds once.
        if self.bag.open != self.bag.sounded_open {
            self.bag.sounded_open = self.bag.open;
            ctx.audio.play(if self.bag.open { OPEN_SOUND } else { CLOSE_SOUND }, 0.7);
        }
        self.sync_body(ctx);
        self.glint_pickups(ctx);
        self.bag.open
    }

    /// A capture of the screen, with some things in it to show.
    fn capture_bag(&mut self, ctx: &mut Context) {
        let Some((chest, frame)) = self.bag.scheduled else { return };
        if ctx.time.frame() < frame {
            return;
        }
        self.bag.scheduled = None;
        let catalog = ctx.world.items.clone();
        for (item, count) in CAPTURE_ITEMS {
            self.bag.inventory.add(&catalog, item, count);
            if !chest && let Some(piece) = self.bag.inventory.first_of(item).map(|piece| piece.id) {
                self.bag.wear(&catalog, piece);
            }
        }
        let chest = chest.then(|| ctx.world.find("chest").and_then(|e| ctx.world.container_key(e))).flatten();
        self.open_bag(chest);
    }

    pub(crate) fn open_bag(&mut self, chest: Option<String>) {
        self.bag.open = true;
        self.bag.chest = chest;
        self.bag.screen.lifted = None;
        self.bag.screen.tab = Tab::Inventory;
        let side = if self.bag.chest.is_some() { Side::Chest } else { Side::Own };
        self.bag.screen.cursor = Cursor { side, x: 0, y: 0 };
    }

    /// Takes what lies in front of the character.
    pub(crate) fn take_pickup(&mut self, ctx: &mut Context, entity: Entity) {
        let Some(id) = ctx.world.get::<Pickup>(entity).map(|pickup| pickup.item.clone()) else { return };
        let name = ctx.world.items.get(&id).map_or(id.clone(), |item| item.name.clone());
        match ctx.world.pick_up(entity, &mut self.bag.inventory, &mut ctx.physics) {
            0 => self.hud.notify(ctx, "Não cabe mais nada", false),
            1 => self.hud.notify(ctx, name, true),
            taken => self.hud.notify(ctx, format!("{name} x{taken}"), true),
        }
        ctx.audio.play_pitched(STEP_SOUND, 0.6, 1.5);
    }

    pub(crate) fn open_chest(&mut self, key: String) {
        self.open_bag(Some(key));
    }

    /// A number key takes out what's in that slot, and an empty slot, or the one already in the
    /// hand, leaves the hand empty. On a gamepad, the directional goes from slot to slot, and
    /// up takes out the first thing there is to eat.
    fn hotbar_keys(&mut self, ctx: &mut Context) {
        for (key, (action, _)) in HOTBAR_KEYS.into_iter().enumerate() {
            if ctx.input.action_just_pressed(action) {
                let in_hand = self.bag.inventory.hand().and_then(|id| self.bag.inventory.shortcut_of(id));
                self.select_slot(ctx, key, in_hand == Some(key));
            }
        }
        let step = i32::from(ctx.input.action_just_pressed(WEAPON_NEXT))
            - i32::from(ctx.input.action_just_pressed(WEAPON_PREVIOUS));
        if step != 0 {
            let hand = self.bag.inventory.hand();
            let from = hand.and_then(|id| self.bag.inventory.shortcut_of(id)).unwrap_or(self.bag.selected) as i32;
            self.select_slot(ctx, (from + step).rem_euclid(HOTBAR as i32) as usize, false);
        }
        if ctx.input.action_just_pressed(EAT) {
            let catalog = ctx.world.items.clone();
            let food = (0..HOTBAR).find(|&key| {
                let item = self.bag.inventory.shortcut(key).and_then(|piece| catalog.get(&piece.stack.item));
                matches!(item.map(|item| &item.kind), Some(ItemKind::Consumable { .. }))
            });
            if let Some(key) = food {
                self.select_slot(ctx, key, false);
            }
        }
    }

    /// Puts what's in this slot in the hand, or nothing. Armor goes on the body instead.
    fn select_slot(&mut self, ctx: &mut Context, key: usize, empty: bool) {
        self.bag.selected = key;
        let catalog = ctx.world.items.clone();
        let piece = self.bag.inventory.shortcut(key).map(|piece| (piece.id, piece.stack.item.clone()));
        match piece {
            Some((id, item)) if !empty && slot_of(&catalog, &item).is_some() => self.use_piece(ctx, id),
            Some((id, _)) if !empty => self.bag.inventory.set_hand(Some(id)),
            _ => self.bag.inventory.set_hand(None),
        }
    }

    /// A weapon goes in the hand, getting a shortcut if it has none; food gets eaten.
    pub(crate) fn use_piece(&mut self, ctx: &mut Context, id: u32) {
        let catalog = ctx.world.items.clone();
        let Some(stack) = self.bag.inventory.piece(id).map(|piece| piece.stack.clone()) else { return };
        let Some(item) = catalog.get(&stack.item) else { return };
        match &item.kind {
            ItemKind::Weapon { .. } | ItemKind::Grimoire { .. } | ItemKind::Gun { .. } => {
                if self.bag.inventory.shortcut_of(id).is_none() {
                    let inventory = &self.bag.inventory;
                    let free = (0..HOTBAR).find(|&key| inventory.shortcut(key).is_none());
                    let in_hand = inventory.hand().and_then(|hand| inventory.shortcut_of(hand));
                    self.bag.inventory.set_shortcut(free.or(in_hand).unwrap_or(0), Some(id));
                }
                self.bag.inventory.set_hand(Some(id));
            }
            ItemKind::Consumable { .. } => {
                let busy = ctx.world.get::<Fighter>(self.avatar).is_some_and(|fighter| fighter.busy());
                if busy || self.dodge.is_some() || self.dead_for.is_some() {
                    return;
                }
                if ctx.world.consume(self.avatar, &stack.item) {
                    self.bag.inventory.take(id, 1);
                    ctx.audio.play_pitched(STEP_SOUND, 0.6, 0.7);
                    self.hud.notify(ctx, format!("Você come: {}", item.name), true);
                }
            }
            ItemKind::Armor { .. } => {
                if self.bag.wear(&catalog, id) {
                    ctx.audio.play_pitched(STEP_SOUND, 0.6, 0.85);
                } else {
                    self.hud.notify(ctx, NO_ROOM, false);
                }
            }
            ItemKind::Misc => self.hud.notify(ctx, item.description.clone(), true),
        }
    }

    /// Anything lying on the ground glints, or nobody would find it in the grass.
    fn glint_pickups(&mut self, ctx: &mut Context) {
        let Some(glint) = &self.bag.glint else { return };
        let bare: Vec<Entity> =
            ctx.world.ecs.query::<hecs::Without<(Entity, &Pickup), &ParticleSystem>>().iter().map(|(e, _)| e).collect();
        for (seed, entity) in bare.into_iter().enumerate() {
            let _ =
                ctx.world.ecs.insert_one(entity, ParticleSystem::new(glint.clone(), Vec3::Y * 0.05, 1.0, seed as u64));
        }
    }

    /// Shows on the body what's on the body, and only that. Something with no model of its own
    /// still protects, but doesn't show.
    fn sync_body(&mut self, ctx: &mut Context) {
        let catalog = ctx.world.items.clone();
        let wanted: Vec<&ItemDesc> = self
            .bag
            .body
            .pieces()
            .iter()
            .filter_map(|piece| catalog.get(&piece.stack.item))
            .filter(|item| item.model.is_some())
            .collect();
        let avatar = self.avatar;
        let on_this_body = |world: &World, entity| world.get::<Worn>(entity).is_some_and(|worn| worn.parent == avatar);
        let mut dressed = std::mem::take(&mut self.bag.dressed);
        dressed.retain(|(item, entity)| {
            let keep = wanted.iter().any(|want| want.id == *item) && on_this_body(&ctx.world, *entity);
            if !keep {
                ctx.world.despawn(*entity, &mut ctx.physics);
            }
            keep
        });
        for item in wanted {
            if !dressed.iter().any(|(on, _)| *on == item.id)
                && let Some(mesh) = ctx.world.item_mesh(&item.id)
            {
                dressed.push((item.id.clone(), ctx.world.dress(avatar, mesh)));
            }
        }
        self.bag.dressed = dressed;
    }

    pub(crate) fn draw_hotbar(&mut self, ctx: &mut Context) {
        let size = ctx.ui.size();
        let slots: Vec<Rect> = (0..HOTBAR).map(|at| hotbar_slot(size, at)).collect();
        self.draw_hotbar_in(ctx, &slots);
    }

    /// The hotbar in these slots, which the bag screen puts under its grid.
    pub(crate) fn draw_hotbar_in(&mut self, ctx: &mut Context, slots: &[Rect]) {
        let catalog = ctx.world.items.clone();
        let hand = self.bag.inventory.hand();
        let ink = ctx.ui.theme.fill;
        let (line, accent, text) = (ctx.ui.theme.border, ctx.ui.theme.accent, ctx.ui.theme.text);
        let stroke = ctx.ui.line();
        let stacks: Vec<_> =
            (0..HOTBAR).map(|key| self.bag.inventory.shortcut(key).map(|piece| piece.stack.clone())).collect();
        for (at, (stack, &rect)) in stacks.iter().zip(slots).enumerate() {
            let in_hand = hand.is_some() && hand.and_then(|id| self.bag.inventory.shortcut_of(id)) == Some(at);
            ctx.ui.dither(rect, ink, ink.w, false);
            if in_hand {
                // The one in the hand has a double line, in the brightest color there is.
                ctx.ui.panel(rect.expand(stroke), Vec4::ZERO, accent);
                ctx.ui.panel(rect, Vec4::ZERO, accent);
            } else {
                ctx.ui.panel(rect, Vec4::ZERO, line);
            }
            let slot = self.bag.slot(&catalog, &mut ctx.ui, stack, false);
            ctx.ui.slot_contents(rect, slot, text, Vec2::ZERO);
            let color = if in_hand { accent } else { DIM };
            let number = TextStyle::new(16.0).color(color).anchor(Anchor::Top);
            ctx.ui.text(rect.point(Anchor::Bottom) + Vec2::Y * 4.0, &(at + 1).to_string(), &number);
        }
    }

    /// The box of a weapon, for a moment after it's taken out.
    fn draw_weapon_note(&mut self, ctx: &mut Context) {
        let held = self.bag.equipped.as_ref().filter(|e| e.kind == HeldKind::Weapon).map(|e| e.item.clone());
        let Some(id) = held else {
            self.bag.weapon_note = None;
            return;
        };
        let dt = ctx.time.delta();
        let note = self.bag.weapon_note.get_or_insert_with(|| (id.clone(), 0.0));
        if note.0 != id {
            *note = (id.clone(), 0.0);
        }
        note.1 += dt;
        let alpha = ((WEAPON_NOTE_SECONDS - note.1) / WEAPON_NOTE_FADE).clamp(0.0, 1.0);
        if alpha <= 0.0 {
            return;
        }
        let catalog = ctx.world.items.clone();
        let icon = self.bag.icon(&catalog, &mut ctx.ui, &id);
        let name = catalog.get(&id).map_or(id.as_str(), |item| item.name.as_str());
        let equipped = crate::hud::Equipped { icon, name, lines: &[], covered: 0.0, alpha };
        crate::hud::draw_equipped(&mut ctx.ui, &equipped);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use grimora::{Fighter, Health};

    use super::*;
    use crate::DEFAULT_SCENE;

    const DT: f32 = 1.0 / 60.0;

    /// A save folder, deleted at the end of the test.
    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("grimora_bag_{name}_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            Self(dir)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn start(dir: &Folder) -> (Sandbox, Context) {
        let config = EngineConfig {
            scene: Some(DEFAULT_SCENE.into()),
            save_dir: Some(dir.0.clone()),
            load: Some("none".into()),
            escape_quits: false,
            ..Default::default()
        };
        let mut game = Sandbox::new();
        let mut ctx = Context::headless(config, &mut game).unwrap();
        frames(&mut game, &mut ctx, 30);
        ctx.world.npcs_ignore_player = true;
        (game, ctx)
    }

    fn frames(game: &mut Sandbox, ctx: &mut Context, count: u32) {
        for _ in 0..count {
            ctx.headless_frame(game, DT).unwrap();
        }
    }

    fn press(game: &mut Sandbox, ctx: &mut Context, key: KeyCode) {
        ctx.input.key_down(key);
        frames(game, ctx, 1);
        ctx.input.key_up(key);
        frames(game, ctx, 1);
    }

    fn on_the_ground(ctx: &Context, id: &str) -> Option<Entity> {
        let mut query = ctx.world.ecs.query::<(Entity, &Pickup)>();
        query.iter().find(|(_, pickup)| pickup.item == id).map(|(entity, _)| entity)
    }

    /// Walks up to the thing, coming from the spawn point so as not to land in a tree, and uses it.
    fn take(game: &mut Sandbox, ctx: &mut Context, entity: Entity) {
        let at = ctx.world.transform(entity).unwrap().position;
        let toward_spawn = (Vec3::new(0.0, at.y, 15.0) - at).normalize_or(Vec3::Z);
        let player = game.player.as_mut().unwrap();
        player.body.position = at + toward_spawn * 1.0 + Vec3::Y * 0.1;
        player.facing = 0.0;
        player.camera_yaw = 0.0;
        player.camera_pitch = -0.3;
        frames(game, ctx, 5);
        press(game, ctx, KeyCode::KeyE);
    }

    fn held(game: &Sandbox) -> Option<&str> {
        game.bag.inventory.held().map(|stack| stack.item.as_str())
    }

    #[test]
    fn from_the_ground_to_the_hand_to_a_blow_through_a_save() {
        let dir = Folder::new("flow");
        let (mut game, mut ctx) = start(&dir);
        let avatar = game.avatar;
        assert_eq!(held(&game), Some(crate::FIRST_WEAPON));
        assert!(game.bag.armed());

        let sword = on_the_ground(&ctx, "espada_larga").unwrap();
        take(&mut game, &mut ctx, sword);
        assert_eq!(game.bag.inventory.count_of("espada_larga"), 1);
        assert_eq!(on_the_ground(&ctx, "espada_larga"), None, "it's not on the ground anymore");
        let piece = game.bag.inventory.first_of("espada_larga").unwrap().id;
        assert!(game.bag.inventory.set_shortcut(1, Some(piece)));
        assert_eq!(held(&game), Some(crate::FIRST_WEAPON), "taking something doesn't change the hand");

        press(&mut game, &mut ctx, KeyCode::Digit2);
        assert_eq!(held(&game), Some("espada_larga"));
        frames(&mut game, &mut ctx, 150);
        assert_eq!(game.bag.equipped.as_ref().unwrap().item, "espada_larga");
        assert_eq!(ctx.world.get::<Fighter>(avatar).unwrap().weapon.damage, 14.0);

        let hostile = ctx.world.find("hostile").unwrap();
        let at = ctx.world.transform(hostile).unwrap().position;
        let player = game.player.as_mut().unwrap();
        player.body.position = at + Vec3::new(1.2, 0.1, 0.0);
        player.facing = std::f32::consts::FRAC_PI_2;
        player.camera_yaw = std::f32::consts::FRAC_PI_2;
        let before = ctx.world.get::<Health>(hostile).unwrap().current;
        // It walks its route otherwise, and a great sword takes its time to come down.
        ctx.world.freeze(hostile, 2.0);
        press(&mut game, &mut ctx, KeyCode::KeyF);
        frames(&mut game, &mut ctx, 110);
        assert_eq!(ctx.world.get::<Health>(hostile).unwrap().current, before - 14.0, "it hits as that sword does");

        let apples = on_the_ground(&ctx, "maca").unwrap();
        take(&mut game, &mut ctx, apples);
        assert_eq!(game.bag.inventory.count_of("maca"), 2);
        let piece = game.bag.inventory.first_of("maca").unwrap().id;
        assert!(game.bag.inventory.set_shortcut(2, Some(piece)));
        ctx.world.get_mut::<Health>(avatar).unwrap().current = 40.0;
        press(&mut game, &mut ctx, KeyCode::Digit3);
        frames(&mut game, &mut ctx, 220);
        assert_eq!(held(&game), Some("maca"));
        ctx.input.button_down(MouseButton::Left);
        frames(&mut game, &mut ctx, 1);
        ctx.input.button_up(MouseButton::Left);
        frames(&mut game, &mut ctx, 150);
        assert_eq!(game.bag.inventory.count_of("maca"), 1, "one was eaten");
        assert!(ctx.world.get::<Health>(avatar).unwrap().current > 40.0);
        assert_eq!(held(&game), Some("maca"), "there is still one in the hand");
        press(&mut game, &mut ctx, KeyCode::Digit2);
        frames(&mut game, &mut ctx, 220);

        press(&mut game, &mut ctx, KeyCode::F6);
        let dagger = on_the_ground(&ctx, "adaga").unwrap();
        take(&mut game, &mut ctx, dagger);
        press(&mut game, &mut ctx, KeyCode::Digit1);
        assert_eq!(game.bag.inventory.count_of("adaga"), 1);
        assert_eq!(held(&game), Some(crate::FIRST_WEAPON));
        let hit = ctx.world.get::<Health>(ctx.world.find("hostile").unwrap()).unwrap().current;

        press(&mut game, &mut ctx, KeyCode::F9);
        frames(&mut game, &mut ctx, 3);
        assert_eq!(game.bag.inventory.count_of("adaga"), 0, "the dagger is back where it was");
        assert!(on_the_ground(&ctx, "adaga").is_some());
        assert_eq!(on_the_ground(&ctx, "espada_larga"), None, "and what had been taken stays taken");
        assert_eq!(game.bag.inventory.count_of("maca"), 1);
        assert_eq!(held(&game), Some("espada_larga"));
        assert_eq!(game.bag.equipped.as_ref().unwrap().item, "espada_larga");
        assert_eq!(ctx.world.get::<Fighter>(game.avatar).unwrap().weapon.damage, 14.0);
        assert_eq!(ctx.world.get::<Health>(ctx.world.find("hostile").unwrap()).unwrap().current, hit);
    }

    #[test]
    fn a_great_sword_stands_and_swings_its_own_way() {
        let dir = Folder::new("great");
        let (mut game, mut ctx) = start(&dir);
        let sword = on_the_ground(&ctx, "espada_larga").unwrap();
        take(&mut game, &mut ctx, sword);
        let piece = game.bag.inventory.first_of("espada_larga").unwrap().id;
        assert!(game.bag.inventory.set_shortcut(1, Some(piece)));
        press(&mut game, &mut ctx, KeyCode::Digit2);
        frames(&mut game, &mut ctx, 150);
        let avatar = game.avatar;
        assert_eq!(ctx.world.get::<Animator>(avatar).unwrap().current(), Some("sword_idle"));
        // Asked for again and again, as a player would: a great blow chains late.
        for blow in ["great_1", "great_2", "great_3", "great_4"] {
            let mut seen = false;
            for frame in 0..150 {
                if frame % 12 == 0 {
                    press(&mut game, &mut ctx, KeyCode::KeyF);
                }
                frames(&mut game, &mut ctx, 1);
                if ctx.world.get::<Animator>(avatar).unwrap().current() == Some(blow) {
                    seen = true;
                    break;
                }
            }
            assert!(seen, "{blow} never played");
        }
    }

    #[test]
    fn the_screen_moves_uses_and_drops_with_the_keys() {
        let dir = Folder::new("screen");
        let (mut game, mut ctx) = start(&dir);
        let sword = on_the_ground(&ctx, "espada_larga").unwrap();
        take(&mut game, &mut ctx, sword);

        press(&mut game, &mut ctx, KeyCode::KeyI);
        assert!(game.bag.open);
        // The sword of a new game stands in the first column, and a piece is one stop for
        // the arrows.
        let long = game.bag.inventory.first_of("espada_larga").unwrap().clone();
        while game.bag.screen.cursor.x < long.x {
            press(&mut game, &mut ctx, KeyCode::ArrowRight);
        }
        while game.bag.screen.cursor.y < long.y {
            press(&mut game, &mut ctx, KeyCode::ArrowDown);
        }
        assert_eq!((game.bag.screen.cursor.x, game.bag.screen.cursor.y), (long.x, long.y));
        press(&mut game, &mut ctx, KeyCode::ArrowDown);
        assert_eq!(game.bag.screen.cursor.y, long.y + long.size.1, "it jumped over the rest of the sword");
        press(&mut game, &mut ctx, KeyCode::ArrowUp);
        press(&mut game, &mut ctx, KeyCode::Enter);
        assert_eq!(game.bag.screen.lifted.map(|lifted| lifted.id), Some(long.id));
        assert_eq!((game.bag.screen.cursor.x, game.bag.screen.cursor.y), (long.x, long.y), "from its top left cell");
        press(&mut game, &mut ctx, KeyCode::ArrowRight);
        press(&mut game, &mut ctx, KeyCode::Enter);
        assert_eq!(game.bag.screen.lifted, None);
        let moved = game.bag.inventory.first_of("espada_larga").unwrap().clone();
        assert_eq!((moved.id, moved.x, moved.y), (long.id, long.x + 1, long.y));

        // Turned, it's four across.
        press(&mut game, &mut ctx, KeyCode::Enter);
        press(&mut game, &mut ctx, KeyCode::KeyT);
        press(&mut game, &mut ctx, KeyCode::Enter);
        let lying = game.bag.inventory.first_of("espada_larga").unwrap().clone();
        assert!(lying.turned);
        assert_eq!(lying.size, (4, 1));

        game.bag.inventory.set_hand(Some(long.id));
        assert_eq!(held(&game), Some("espada_larga"));

        press(&mut game, &mut ctx, KeyCode::KeyX);
        assert_eq!(game.bag.inventory.count_of("espada_larga"), 0);
        assert_eq!(held(&game), None);
        assert!(!game.bag.armed());
        assert!(on_the_ground(&ctx, "espada_larga").is_some(), "it's on the ground again");

        // No orders for the character while it's open: the key that would attack does nothing.
        press(&mut game, &mut ctx, KeyCode::KeyF);
        assert!(!ctx.world.get::<Fighter>(game.avatar).unwrap().busy());
        press(&mut game, &mut ctx, KeyCode::KeyI);
        assert!(!game.bag.open);
    }

    #[test]
    fn the_mouse_drags_a_piece_where_it_goes() {
        let dir = Folder::new("mouse");
        let (mut game, mut ctx) = start(&dir);
        press(&mut game, &mut ctx, KeyCode::KeyI);
        assert!(game.bag.inventory.set_shortcut(0, None), "back on the grid");
        let sword = game.bag.inventory.first_of(crate::FIRST_WEAPON).unwrap().clone();
        let field = crate::bag_screen::own_field(&ctx, &game.bag.inventory);
        let scale = ctx.ui.scale();
        let cell = |x: u32, y: u32| field.center_of(x, y) * scale;
        let free = (COLUMNS - 1, 0);
        assert!(game.bag.inventory.at(free.0, free.1).is_none());

        ctx.input.set_cursor(cell(sword.x, sword.y + 1));
        frames(&mut game, &mut ctx, 1);
        ctx.input.button_down(MouseButton::Left);
        frames(&mut game, &mut ctx, 1);
        ctx.input.button_up(MouseButton::Left);
        // A click lifts it, and it follows the mouse by the cell it was taken from.
        frames(&mut game, &mut ctx, 1);
        assert_eq!(game.bag.screen.lifted.map(|lifted| lifted.id), Some(sword.id));
        ctx.input.set_cursor(cell(free.0, free.1 + 1));
        frames(&mut game, &mut ctx, 1);
        ctx.input.button_down(MouseButton::Left);
        frames(&mut game, &mut ctx, 1);
        ctx.input.button_up(MouseButton::Left);
        frames(&mut game, &mut ctx, 1);
        assert_eq!(game.bag.screen.lifted, None);
        let moved = game.bag.inventory.piece(sword.id).unwrap();
        assert_eq!((moved.x, moved.y), free);

        // Held down, it goes where the button is let go.
        ctx.input.set_cursor(cell(free.0, free.1));
        frames(&mut game, &mut ctx, 1);
        ctx.input.button_down(MouseButton::Left);
        frames(&mut game, &mut ctx, 1);
        ctx.input.set_cursor(cell(free.0 - 1, free.1));
        frames(&mut game, &mut ctx, 1);
        ctx.input.button_up(MouseButton::Left);
        frames(&mut game, &mut ctx, 1);
        assert_eq!(game.bag.screen.lifted, None);
        assert_eq!(game.bag.inventory.piece(sword.id).unwrap().x, free.0 - 1);
    }

    #[test]
    fn armor_is_dragged_onto_the_body_and_back() {
        use grimora::scene::ArmorSlot;
        let dir = Folder::new("worn");
        let (mut game, mut ctx) = start(&dir);
        let catalog = ctx.world.items.clone();
        for item in ["peitoral_ferro", "gibao"] {
            game.bag.inventory.add(&catalog, item, 1);
        }
        press(&mut game, &mut ctx, KeyCode::KeyI);
        let scale = ctx.ui.scale();
        let field = crate::bag_screen::own_field(&ctx, &game.bag.inventory);
        let panel = crate::bag_screen::own_body(&ctx, &game.bag.inventory);
        let spot = |rect: Rect| rect.point(Anchor::Center) * scale;
        let drag = |game: &mut Sandbox, ctx: &mut Context, from: Vec2, to: Vec2| {
            ctx.input.set_cursor(from);
            frames(game, ctx, 1);
            ctx.input.button_down(MouseButton::Left);
            frames(game, ctx, 1);
            ctx.input.set_cursor(to);
            frames(game, ctx, 1);
            ctx.input.button_up(MouseButton::Left);
            frames(game, ctx, 1);
        };
        let shown =
            |ctx: &Context, avatar| ctx.world.ecs.query::<&Worn>().iter().filter(|worn| worn.parent == avatar).count();
        let piece = game.bag.inventory.first_of("peitoral_ferro").unwrap().clone();
        let from = field.center_of(piece.x, piece.y) * scale;
        let head = spot(panel.place(0));

        // Not on the head: it goes back. Over nothing, too.
        drag(&mut game, &mut ctx, from, head);
        assert_eq!(game.bag.screen.lifted, None);
        assert!(game.bag.worn_in(ArmorSlot::Head).is_none());
        drag(&mut game, &mut ctx, from, Vec2::new(4.0, 4.0));
        assert_eq!(game.bag.screen.lifted, None);
        assert!(game.bag.inventory.first_of("peitoral_ferro").is_some());
        // While it's dragged, the picture goes with the mouse, past the window of the bag.
        ctx.input.set_cursor(from);
        frames(&mut game, &mut ctx, 1);
        ctx.input.button_down(MouseButton::Left);
        frames(&mut game, &mut ctx, 1);
        ctx.input.set_cursor(spot(panel.place(1)));
        frames(&mut game, &mut ctx, 1);
        assert!(game.bag.screen.dragged_to().is_some());
        ctx.input.button_up(MouseButton::Left);
        frames(&mut game, &mut ctx, 1);
        assert_eq!(game.bag.screen.lifted, None);
        assert_eq!(game.bag.worn_in(ArmorSlot::Body).unwrap().stack.item, "peitoral_ferro");
        assert!(game.bag.inventory.first_of("peitoral_ferro").is_none(), "it leaves the grid");
        assert_eq!(shown(&ctx, game.avatar), 1, "it shows on the body");
        frames(&mut game, &mut ctx, 1);
        assert_eq!(ctx.world.get::<Health>(game.avatar).unwrap().armor, 9.0);

        // Something else for that place takes it off, and it comes back to the grid.
        let other = game.bag.inventory.first_of("gibao").unwrap().clone();
        drag(&mut game, &mut ctx, field.center_of(other.x, other.y) * scale, spot(panel.place(1)));
        assert_eq!(game.bag.worn_in(ArmorSlot::Body).unwrap().stack.item, "gibao");
        assert!(game.bag.inventory.first_of("peitoral_ferro").is_some());
        assert_eq!(shown(&ctx, game.avatar), 0, "the jerkin has no model");

        // From the body to a free cell.
        let free = (COLUMNS - 2, ROWS - 3);
        assert!(game.bag.inventory.at(free.0, free.1).is_none());
        drag(&mut game, &mut ctx, spot(panel.place(1)), field.center_of(free.0, free.1) * scale);
        assert!(game.bag.worn_in(ArmorSlot::Body).is_none());
        assert_eq!(game.bag.inventory.first_of("gibao").map(|p| (p.x, p.y)), Some(free));
        frames(&mut game, &mut ctx, 1);
        assert_eq!(ctx.world.get::<Health>(game.avatar).unwrap().armor, 0.0);

        // The hotbar takes what can be held, off the grid, and gives it back.
        let sword = game.bag.inventory.hand().unwrap();
        let slots: Vec<Vec2> = (0..HOTBAR)
            .map(|at| crate::bag_screen::bar_slot(&ctx, &game.bag.inventory, at).point(Anchor::Center) * scale)
            .collect();
        let slot = |at: usize| slots[at];
        let free = (COLUMNS - 1, 0);
        assert!(game.bag.inventory.at(free.0, free.1).is_none());
        drag(&mut game, &mut ctx, slot(0), field.center_of(free.0, free.1) * scale);
        assert_eq!(game.bag.inventory.hand(), None, "it left the hand with the hotbar");
        assert_eq!(game.bag.inventory.shortcut(0).map(|piece| piece.id), None);
        let moved = game.bag.inventory.piece(sword).unwrap().clone();
        assert_eq!((moved.x, moved.y), free);
        drag(&mut game, &mut ctx, field.center_of(moved.x, moved.y) * scale, slot(2));
        assert_eq!(game.bag.inventory.shortcut_of(sword), Some(2));
        assert!(game.bag.inventory.at(free.0, free.1).is_none(), "it left the grid");
        drag(&mut game, &mut ctx, slot(2), slot(3));
        assert_eq!(game.bag.inventory.shortcut_of(sword), Some(3));
        // What's worn doesn't go there.
        let armor = game.bag.inventory.first_of("peitoral_ferro").unwrap().clone();
        drag(&mut game, &mut ctx, field.center_of(armor.x, armor.y) * scale, slot(1));
        assert!(game.bag.inventory.on_grid(armor.id));
    }

    #[test]
    fn what_is_on_the_body_comes_back_with_a_save() {
        use grimora::scene::ArmorSlot;
        let dir = Folder::new("worn_save");
        let (mut game, mut ctx) = start(&dir);
        let catalog = ctx.world.items.clone();
        game.bag.inventory.add(&catalog, "capacete_ferro", 1);
        let helmet = game.bag.inventory.first_of("capacete_ferro").unwrap().id;
        assert!(game.bag.wear(&catalog, helmet));
        frames(&mut game, &mut ctx, 1);
        let dressed = |ctx: &Context| ctx.world.ecs.query::<&Worn>().iter().count();
        assert_eq!(dressed(&ctx), 1);
        let save = game.save_state(&ctx);
        ctx.save_game("corpo", &save).unwrap();

        assert!(game.bag.take_off(&catalog, ArmorSlot::Head));
        frames(&mut game, &mut ctx, 1);
        assert_eq!(dressed(&ctx), 0);
        assert!(game.load(&mut ctx, "corpo"));
        frames(&mut game, &mut ctx, 1);
        assert_eq!(game.bag.worn_in(ArmorSlot::Head).unwrap().stack.item, "capacete_ferro");
        assert_eq!(dressed(&ctx), 1);
    }

    #[test]
    fn the_chest_keeps_what_was_left_in_it_through_a_save() {
        let dir = Folder::new("chest");
        let (mut game, mut ctx) = start(&dir);
        let chest = ctx.world.find("chest").unwrap();
        take(&mut game, &mut ctx, chest);
        assert!(game.bag.open && game.bag.chest.is_some(), "the chest should open");
        // The focus starts on the chest's first slot, whose apples go to the backpack.
        press(&mut game, &mut ctx, KeyCode::KeyR);
        assert_eq!(game.bag.inventory.count_of("maca"), 3);
        let key = game.bag.chest.clone().unwrap();
        assert_eq!(ctx.world.container(&key).unwrap().count_of("maca"), 0);
        press(&mut game, &mut ctx, KeyCode::KeyI);
        assert!(!game.bag.open);

        press(&mut game, &mut ctx, KeyCode::F6);
        game.bag.inventory = Inventory::new(COLUMNS, ROWS);
        let catalog = ctx.world.items.clone();
        ctx.world.container(&key).unwrap().add(&catalog, "maca", 1);
        press(&mut game, &mut ctx, KeyCode::F9);
        frames(&mut game, &mut ctx, 3);
        assert_eq!(game.bag.inventory.count_of("maca"), 3);
        assert_eq!(ctx.world.container(&key).unwrap().count_of("maca"), 0);
        assert_eq!(ctx.world.container(&key).unwrap().count_of("machadinha"), 1);
    }

    #[test]
    fn what_lies_on_the_ground_glints() {
        let dir = Folder::new("glint");
        let (game, ctx) = start(&dir);
        drop(game);
        let bare = ctx.world.ecs.query::<hecs::Without<&Pickup, &ParticleSystem>>().iter().count();
        assert_eq!(bare, 0);
    }
}
