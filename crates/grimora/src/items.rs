//! Carrying things: [`Inventory`] is a grid that pieces fit into, each as many cells as its item
//! says, with [`HOTBAR`] shortcuts to them for the keys, and it goes in the game's own part of
//! a saved game. In the world, [`Pickup`]s lie around to be taken and [`Container`]s hold
//! items, and [`ItemState`] remembers both by key, so they stay as the player left them through
//! streaming and saved games. The game decides what taking and using things looks like.

use std::collections::BTreeMap;

use grimora_core::{Transform, Vec3, WorldTransform};
use grimora_physics::Physics;
use grimora_scene::{ContainerDesc, Interactable, ItemCatalog, ItemKind};
use hecs::Entity;
use serde::{Deserialize, Serialize};

use crate::World;
use crate::combat::{Health, Stamina};
use crate::world::{MeshId, Renderable};

/// The shortcuts the keys reach.
pub const HOTBAR: usize = 5;
/// How close the player must be to take something that was dropped.
const PICKUP_RADIUS: f32 = 2.0;
/// The grid of a container from a save older than grids.
const OLD_CONTAINER: (u32, u32) = (8, 5);

/// Some of one item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemStack {
    pub item: String,
    pub count: u32,
}

/// A stack lying on an inventory's grid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placed {
    /// Stays the same wherever it's moved in this inventory, for the shortcuts and the hand.
    pub id: u32,
    pub stack: ItemStack,
    /// Its top left cell.
    pub x: u32,
    pub y: u32,
    /// The cells it takes across and down, after turning.
    pub size: (u32, u32),
    /// Lying a quarter turn from how the catalog has it.
    pub turned: bool,
}

impl Placed {
    /// Whether it takes this cell.
    pub fn covers(&self, x: u32, y: u32) -> bool {
        (self.x..self.x + self.size.0).contains(&x) && (self.y..self.y + self.size.1).contains(&y)
    }

    fn overlaps(&self, x: u32, y: u32, (w, h): (u32, u32)) -> bool {
        self.x < x + w && x < self.x + self.size.0 && self.y < y + h && y < self.y + self.size.1
    }
}

/// What would happen if a piece were put down somewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Landing {
    /// There's room.
    Free,
    /// On top of this piece, of the same item.
    Merge(u32),
    /// On this one piece, which gets out of the way if it can.
    Over(u32),
    /// Off the grid, or on more than one piece.
    Blocked,
}

/// A grid of pieces. See the module docs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inventory {
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
    #[serde(default)]
    pieces: Vec<Placed>,
    #[serde(default)]
    shortcuts: [Option<u32>; HOTBAR],
    /// The piece in the hand. In a save older than grids, the hotbar slot it was in.
    #[serde(default)]
    hand: Option<u32>,
    #[serde(default)]
    next_id: u32,
    /// What a save older than grids holds, until [`Inventory::settle`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    slots: Vec<Option<ItemStack>>,
}

impl Inventory {
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height, ..Default::default() }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pieces(&self) -> &[Placed] {
        &self.pieces
    }

    pub fn piece(&self, id: u32) -> Option<&Placed> {
        self.pieces.iter().find(|piece| piece.id == id)
    }

    /// Whether a piece takes cells: what's on the hotbar has left the grid.
    pub fn on_grid(&self, id: u32) -> bool {
        !self.shortcuts.contains(&Some(id))
    }

    /// The pieces that take cells.
    pub fn grid_pieces(&self) -> impl Iterator<Item = &Placed> {
        self.pieces.iter().filter(|piece| self.on_grid(piece.id))
    }

    /// The piece on this cell.
    pub fn at(&self, x: u32, y: u32) -> Option<&Placed> {
        self.grid_pieces().find(|piece| piece.covers(x, y))
    }

    /// The first piece of `item`.
    pub fn first_of(&self, item: &str) -> Option<&Placed> {
        self.pieces.iter().find(|piece| piece.stack.item == item)
    }

    /// How many of `item` it holds, in all.
    pub fn count_of(&self, item: &str) -> u32 {
        self.pieces.iter().filter(|piece| piece.stack.item == item).map(|piece| piece.stack.count).sum()
    }

    /// The cells taken, out of [`Inventory::width`] times [`Inventory::height`].
    pub fn cells_taken(&self) -> u32 {
        self.grid_pieces().map(|piece| piece.size.0 * piece.size.1).sum()
    }

    /// Where something of this size first fits, going down the columns from the left.
    fn spot(&self, size: (u32, u32), skip: Option<u32>) -> Option<(u32, u32)> {
        let (w, h) = size;
        for x in 0..(self.width + 1).saturating_sub(w) {
            for y in 0..(self.height + 1).saturating_sub(h) {
                if !self.grid_pieces().any(|piece| Some(piece.id) != skip && piece.overlaps(x, y, size)) {
                    return Some((x, y));
                }
            }
        }
        None
    }

    /// Puts a new piece wherever it fits, turning it if it has to. Returns its id.
    fn place(&mut self, catalog: &ItemCatalog, stack: ItemStack) -> Result<u32, ItemStack> {
        let size = catalog.cells(&stack.item);
        let turned = (size.1, size.0);
        let (at, size, is_turned) = match (self.spot(size, None), self.spot(turned, None)) {
            (Some(at), _) => (at, size, false),
            (None, Some(at)) => (at, turned, true),
            (None, None) => return Err(stack),
        };
        let id = self.next_id;
        self.next_id += 1;
        self.pieces.push(Placed { id, stack, x: at.0, y: at.1, size, turned: is_turned });
        Ok(id)
    }

    /// Adds as many as fit: on top of the pieces already there, then as new ones. Returns how
    /// many didn't fit.
    pub fn add(&mut self, catalog: &ItemCatalog, item: &str, count: u32) -> u32 {
        let size = catalog.stack_size(item);
        let mut left = count;
        for piece in self.pieces.iter_mut().filter(|piece| piece.stack.item == item) {
            let moved = left.min(size.saturating_sub(piece.stack.count));
            piece.stack.count += moved;
            left -= moved;
        }
        while left > 0 {
            let moved = left.min(size);
            if self.place(catalog, ItemStack { item: item.to_owned(), count: moved }).is_err() {
                break;
            }
            left -= moved;
        }
        left
    }

    /// [`Inventory::add`] that gets more rows if it needs them, for what has to fit: what a
    /// chest was given in its scene file, and what's in a save.
    fn add_all(&mut self, catalog: &ItemCatalog, item: &str, count: u32) {
        let mut left = self.add(catalog, item, count);
        // A bit of room is all it takes unless the item is wider than the grid.
        let (w, h) = catalog.cells(item);
        self.width = self.width.max(w.min(h));
        while left > 0 {
            self.height += 1;
            left = self.add(catalog, item, left);
        }
    }

    /// Takes up to `count` out of a piece and returns what it got.
    pub fn take(&mut self, id: u32, count: u32) -> Option<ItemStack> {
        let at = self.pieces.iter().position(|piece| piece.id == id)?;
        let stack = &mut self.pieces[at].stack;
        let taken = ItemStack { item: stack.item.clone(), count: count.min(stack.count) };
        stack.count -= taken.count;
        if stack.count == 0 {
            self.pieces.remove(at);
            self.forget(id, &taken.item);
        }
        (taken.count > 0).then_some(taken)
    }

    /// Takes up to `count` of `item`, from wherever it is. Returns how many.
    pub fn take_item(&mut self, item: &str, count: u32) -> u32 {
        let mut taken = 0;
        while taken < count {
            let Some(id) = self.first_of(item).map(|piece| piece.id) else { break };
            taken += self.take(id, count - taken).map_or(0, |stack| stack.count);
        }
        taken
    }

    /// A piece is gone: the shortcut and the hand go to another of the same item, if any.
    fn forget(&mut self, id: u32, item: &str) {
        let other = self.grid_pieces().find(|piece| piece.stack.item == item).map(|piece| piece.id);
        for shortcut in self.shortcuts.iter_mut().filter(|shortcut| **shortcut == Some(id)) {
            *shortcut = other;
        }
        if self.hand == Some(id) {
            self.hand = other;
        }
    }

    /// What would happen if something of `item` and `size` were put at this cell, leaving
    /// out the piece `moving`, which is the one being moved if it's from here.
    pub fn landing(
        &self,
        catalog: &ItemCatalog,
        item: &str,
        moving: Option<u32>,
        (x, y): (u32, u32),
        size: (u32, u32),
    ) -> Landing {
        if x + size.0 > self.width || y + size.1 > self.height {
            return Landing::Blocked;
        }
        let mut under = self.grid_pieces().filter(|piece| Some(piece.id) != moving && piece.overlaps(x, y, size));
        match (under.next(), under.next()) {
            (None, _) => Landing::Free,
            (Some(piece), None) => {
                let room = piece.stack.count < catalog.stack_size(item);
                if piece.stack.item == item && room { Landing::Merge(piece.id) } else { Landing::Over(piece.id) }
            }
            _ => Landing::Blocked,
        }
    }

    /// Moves a piece on this grid, facing as `turned` says. A piece it lands on goes to where this
    /// one was, or wherever it fits. Returns whether anything changed.
    pub fn move_piece(&mut self, catalog: &ItemCatalog, id: u32, at: (u32, u32), turned: bool) -> bool {
        let Some(piece) = self.piece(id).cloned() else { return false };
        let size = if turned == piece.turned { piece.size } else { (piece.size.1, piece.size.0) };
        match self.landing(catalog, &piece.stack.item, Some(id), at, size) {
            Landing::Blocked => false,
            Landing::Free => {
                self.set_place(id, at, size, turned);
                true
            }
            Landing::Merge(other) => {
                let room = catalog.stack_size(&piece.stack.item) - self.piece(other).map_or(0, |p| p.stack.count);
                let moved = room.min(piece.stack.count);
                self.take(id, moved);
                self.pieces.iter_mut().filter(|p| p.id == other).for_each(|p| p.stack.count += moved);
                true
            }
            Landing::Over(other) => {
                let before = self.clone();
                self.set_place(id, at, size, turned);
                let Some(under) = self.piece(other).cloned() else { return false };
                let moved = self.landing(catalog, &under.stack.item, Some(other), (piece.x, piece.y), under.size);
                let target =
                    if moved == Landing::Free { Some((piece.x, piece.y)) } else { self.spot(under.size, Some(other)) };
                match target {
                    Some(target) => self.set_place(other, target, under.size, under.turned),
                    None => {
                        *self = before;
                        return false;
                    }
                }
                true
            }
        }
    }

    fn set_place(&mut self, id: u32, at: (u32, u32), size: (u32, u32), turned: bool) {
        if let Some(piece) = self.pieces.iter_mut().find(|piece| piece.id == id) {
            (piece.x, piece.y, piece.size, piece.turned) = (at.0, at.1, size, turned);
        }
    }

    /// [`Inventory::move_piece`] into another inventory. A piece it lands on comes here.
    pub fn move_piece_to(
        &mut self,
        catalog: &ItemCatalog,
        id: u32,
        other: &mut Self,
        at: (u32, u32),
        turned: bool,
    ) -> bool {
        let Some(piece) = self.piece(id).cloned() else { return false };
        let size = if turned == piece.turned { piece.size } else { (piece.size.1, piece.size.0) };
        match other.landing(catalog, &piece.stack.item, None, at, size) {
            Landing::Blocked => false,
            Landing::Free => {
                self.take(id, piece.stack.count);
                other.insert(piece.stack, at, size, turned);
                true
            }
            Landing::Merge(under) => {
                let room = catalog.stack_size(&piece.stack.item) - other.piece(under).map_or(0, |p| p.stack.count);
                let moved = room.min(piece.stack.count);
                self.take(id, moved);
                other.pieces.iter_mut().filter(|p| p.id == under).for_each(|p| p.stack.count += moved);
                true
            }
            Landing::Over(under) => {
                let Some(back) = other.piece(under).cloned() else { return false };
                let mut here = self.clone();
                here.take(id, piece.stack.count);
                let free =
                    here.landing(catalog, &back.stack.item, None, (piece.x, piece.y), back.size) == Landing::Free;
                let Some(target) = (if free { Some((piece.x, piece.y)) } else { here.spot(back.size, None) }) else {
                    return false;
                };
                here.insert(back.stack.clone(), target, back.size, back.turned);
                other.take(under, back.stack.count);
                other.insert(piece.stack, at, size, turned);
                *self = here;
                true
            }
        }
    }

    fn insert(&mut self, stack: ItemStack, at: (u32, u32), size: (u32, u32), turned: bool) {
        let id = self.next_id;
        self.next_id += 1;
        self.pieces.push(Placed { id, stack, x: at.0, y: at.1, size, turned });
    }

    /// Joins what can be joined and lays everything out from the top left, the longest first.
    /// Changes nothing and returns false if that wouldn't fit, which can happen when the
    /// player has packed it better.
    pub fn arrange(&mut self, catalog: &ItemCatalog) -> bool {
        let mut tidy = self.clone();
        // What's on the hotbar isn't part of this.
        let (on_bar, mut pieces): (Vec<Placed>, Vec<Placed>) =
            std::mem::take(&mut tidy.pieces).into_iter().partition(|piece| !self.on_grid(piece.id));
        // The fullest stack stays, and the others pour into it.
        pieces.sort_by_key(|piece| (std::cmp::Reverse(piece.stack.count), piece.id));
        let mut joined: Vec<Placed> = Vec::new();
        for mut piece in pieces {
            let limit = catalog.stack_size(&piece.stack.item);
            for held in joined.iter_mut().filter(|held| held.stack.item == piece.stack.item) {
                let moved = piece.stack.count.min(limit.saturating_sub(held.stack.count));
                held.stack.count += moved;
                piece.stack.count -= moved;
            }
            if piece.stack.count > 0 {
                joined.push(piece);
            } else {
                let keeper = joined.iter().find(|held| held.stack.item == piece.stack.item).map(|held| held.id);
                tidy.shortcuts.iter_mut().filter(|s| **s == Some(piece.id)).for_each(|s| *s = keeper);
                if tidy.hand == Some(piece.id) {
                    tidy.hand = keeper;
                }
            }
        }
        joined.sort_by_key(|piece| {
            let (w, h) = catalog.cells(&piece.stack.item);
            (std::cmp::Reverse(h), std::cmp::Reverse(w), piece.stack.item.clone(), piece.id)
        });
        for mut piece in joined {
            let natural = catalog.cells(&piece.stack.item);
            let Some(at) = tidy.spot(natural, None) else { return false };
            (piece.x, piece.y, piece.size, piece.turned) = (at.0, at.1, natural, false);
            tidy.pieces.push(piece);
        }
        tidy.pieces.extend(on_bar);
        *self = tidy;
        true
    }

    /// The piece this hotbar key answers to.
    pub fn shortcut(&self, key: usize) -> Option<&Placed> {
        self.piece((*self.shortcuts.get(key)?)?)
    }

    /// The key a piece answers to.
    pub fn shortcut_of(&self, id: u32) -> Option<usize> {
        self.shortcuts.iter().position(|shortcut| *shortcut == Some(id))
    }

    /// Puts a piece on this slot of the hotbar, out of the grid, or empties the slot. What was
    /// there goes back on the grid. Changes nothing and returns false if there's no room for that.
    pub fn set_shortcut(&mut self, key: usize, id: Option<u32>) -> bool {
        if key >= HOTBAR {
            return false;
        }
        let id = id.filter(|&id| self.piece(id).is_some());
        let old = self.shortcuts[key];
        if old == id {
            return true;
        }
        let before = self.clone();
        if id.is_some() {
            self.shortcuts.iter_mut().filter(|shortcut| **shortcut == id).for_each(|shortcut| *shortcut = None);
        }
        self.shortcuts[key] = id;
        if let Some(old) = old
            && !self.back_on_grid(old)
        {
            *self = before;
            return false;
        }
        true
    }

    /// Two slots of the hotbar trade what they have.
    pub fn swap_shortcuts(&mut self, a: usize, b: usize) {
        if a < HOTBAR && b < HOTBAR {
            self.shortcuts.swap(a, b);
        }
    }

    /// A piece off the hotbar lands on this cell of the grid, as [`Inventory::move_piece`] does.
    pub fn take_off_bar(&mut self, catalog: &ItemCatalog, id: u32, at: (u32, u32), turned: bool) -> bool {
        let before = self.clone();
        self.shortcuts.iter_mut().filter(|shortcut| **shortcut == Some(id)).for_each(|shortcut| *shortcut = None);
        if !self.move_piece(catalog, id, at, turned) {
            *self = before;
            return false;
        }
        if self.hand == Some(id) {
            self.hand = None;
        }
        true
    }

    /// Finds cells for a piece that left the hotbar: the ones it had, if they're free.
    fn back_on_grid(&mut self, id: u32) -> bool {
        let Some(piece) = self.piece(id).cloned() else { return true };
        if self.hand == Some(id) {
            self.hand = None;
        }
        let inside = piece.x + piece.size.0 <= self.width && piece.y + piece.size.1 <= self.height;
        let others = self.grid_pieces().any(|p| p.id != id && p.overlaps(piece.x, piece.y, piece.size));
        if inside && !others {
            return true;
        }
        if let Some(at) = self.spot(piece.size, Some(id)) {
            self.set_place(id, at, piece.size, piece.turned);
            return true;
        }
        let turned = (piece.size.1, piece.size.0);
        if let Some(at) = self.spot(turned, Some(id)) {
            self.set_place(id, at, turned, !piece.turned);
            return true;
        }
        false
    }

    /// Puts a piece in the hand, or empties it.
    pub fn set_hand(&mut self, id: Option<u32>) {
        self.hand = id.filter(|&id| self.piece(id).is_some());
    }

    /// The piece in the hand.
    pub fn hand(&self) -> Option<u32> {
        self.hand
    }

    /// The item in the hand.
    pub fn held(&self) -> Option<&ItemStack> {
        Some(&self.piece(self.hand?)?.stack)
    }

    /// Brings what a save older than grids holds into a grid of this size, with more rows if
    /// it doesn't all fit. Does nothing to anything newer.
    pub fn settle(&mut self, catalog: &ItemCatalog, width: u32, height: u32) {
        if self.slots.is_empty() {
            return;
        }
        let slots = std::mem::take(&mut self.slots);
        let hand = self.hand.take();
        (self.width, self.height) = (width, height);
        for (slot, stack) in slots.into_iter().enumerate() {
            let Some(stack) = stack else { continue };
            let before = self.next_id;
            self.add_all(catalog, &stack.item, stack.count);
            // A stack that went on top of another has no piece of its own.
            let id = if self.next_id > before { Some(before) } else { self.first_of(&stack.item).map(|p| p.id) };
            if slot < HOTBAR {
                self.set_shortcut(slot, id);
            }
            if hand == Some(slot as u32) {
                self.hand = id;
            }
        }
    }
}

/// On an entity: it lies there to be taken with [`World::pick_up`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pickup {
    pub key: String,
    pub item: String,
    pub count: u32,
    /// The game put it there, rather than the scene file.
    dropped: bool,
}

/// On an entity: it holds items, which [`World::container`] gives by this key.
#[derive(Debug, Clone, PartialEq)]
pub struct Container {
    pub key: String,
    /// What it holds until someone opens it.
    pub desc: ContainerDesc,
}

impl Pickup {
    pub(crate) fn new(key: String, item: String, count: u32) -> Self {
        Self { key, item, count, dropped: false }
    }
}

/// Something the game dropped, which only exists here until it's taken.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DroppedItem {
    pub key: String,
    pub stack: ItemStack,
    /// In the world, not local: it lasts through saved games.
    pub transform: WorldTransform,
}

/// What became of the items in the world, by key rather than by entity: a pickup in a
/// region is a new entity each time the region loads. Saved games keep this.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ItemState {
    /// Pickups of the scene that aren't as the file has them: how many are left of each.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub picked: BTreeMap<String, u32>,
    /// Containers that have been opened.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub containers: BTreeMap<String, Inventory>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dropped: Vec<DroppedItem>,
    /// Numbers the keys of what's dropped.
    #[serde(default)]
    pub drops: u64,
}

impl World {
    /// The mesh of `item`, loaded the first time it's needed.
    pub fn item_mesh(&mut self, item: &str) -> Option<MeshId> {
        if let Some(&mesh) = self.item_meshes.get(item) {
            return Some(mesh);
        }
        let catalog = self.items.clone();
        let mesh = self.add_mesh(catalog.mesh(catalog.get(item)?));
        self.item_meshes.insert(item.to_owned(), mesh);
        Some(mesh)
    }

    /// Moves what `entity` holds into `inventory`, as much as fits, and removes the
    /// entity if nothing is left. Returns how many it took: none if it's not a [`Pickup`]
    /// or the inventory is full.
    pub fn pick_up(&mut self, entity: Entity, inventory: &mut Inventory, physics: &mut Physics) -> u32 {
        let Some((key, item, count, dropped)) =
            self.get::<Pickup>(entity).map(|p| (p.key.clone(), p.item.clone(), p.count, p.dropped))
        else {
            return 0;
        };
        let left = inventory.add(&self.items.clone(), &item, count);
        if left == count {
            return 0;
        }
        if dropped {
            let state = &mut self.item_state.dropped;
            match left {
                0 => state.retain(|drop| drop.key != key),
                _ => state.iter_mut().filter(|drop| drop.key == key).for_each(|drop| drop.stack.count = left),
            }
        } else {
            self.item_state.picked.insert(key, left);
        }
        match left {
            0 => {
                self.despawn(entity, physics);
            }
            _ => {
                if let Some(pickup) = self.get_mut::<Pickup>(entity) {
                    pickup.count = left;
                }
            }
        }
        count - left
    }

    /// Leaves `stack` on the ground under `at`, turned by `yaw` degrees. Stays through saved
    /// games, and needs no room in a cell or region. `None` if the catalog doesn't have it.
    pub fn drop_item(&mut self, stack: ItemStack, at: Vec3, yaw: f32, physics: &Physics) -> Option<Entity> {
        let floor = physics.raycast_scenery(at, Vec3::NEG_Y, 4.0).map(|hit| hit.point.y);
        let ground = floor.unwrap_or_else(|| self.ground_height(at.x, at.z));
        let mesh = self.item_mesh(&stack.item)?;
        let catalog = self.items.clone();
        let item = catalog.get(&stack.item)?;
        let transform = item.lying(self.mesh(mesh)?, at.with_y(ground), yaw);
        self.item_state.drops += 1;
        let key = format!("dropped#{}", self.item_state.drops);
        let world = self.origin.transform_to_world(&transform);
        self.item_state.dropped.push(DroppedItem { key: key.clone(), stack: stack.clone(), transform: world });
        Some(self.spawn_dropped(key, stack, transform))
    }

    fn spawn_dropped(&mut self, key: String, stack: ItemStack, transform: Transform) -> Entity {
        let name = self.items.get(&stack.item).map_or(stack.item.clone(), |item| item.name.clone());
        let interact = Interactable { prompt: format!("pegar {name}"), radius: PICKUP_RADIUS };
        let mesh = self.item_mesh(&stack.item).unwrap_or(MeshId(0));
        let pickup = Pickup { key, item: stack.item, count: stack.count, dropped: true };
        self.ecs.spawn((transform, Renderable::new(mesh), interact, pickup))
    }

    /// The key of the container that `entity` is, for [`World::container`].
    pub fn container_key(&self, entity: Entity) -> Option<String> {
        self.get::<Container>(entity).map(|container| container.key.clone())
    }

    /// What the container with this key holds. It needs to be loaded the first time
    /// it's asked for, when it's filled from the scene file.
    pub fn container(&mut self, key: &str) -> Option<&mut Inventory> {
        if !self.item_state.containers.contains_key(key) {
            let desc = self.ecs.query::<&Container>().iter().find(|c| c.key == key).map(|c| c.desc.clone())?;
            let (width, height) = desc.grid();
            let mut inventory = Inventory::new(width, height);
            for (item, count) in &desc.items {
                inventory.add_all(&self.items, item, *count);
            }
            self.item_state.containers.insert(key.to_owned(), inventory);
        }
        self.item_state.containers.get_mut(key)
    }

    /// Gives `entity` what a consumable item gives. Returns whether it's something that can be
    /// used; full bars don't stop it, since the player asked.
    pub fn consume(&mut self, entity: Entity, item: &str) -> bool {
        let Some(ItemKind::Consumable { health, stamina }) = self.items.get(item).map(|i| i.kind.clone()) else {
            return false;
        };
        if let Some(bar) = self.get_mut::<Health>(entity) {
            bar.current = (bar.current + health).min(bar.max);
        }
        if let Some(bar) = self.get_mut::<Stamina>(entity) {
            bar.current = (bar.current + stamina).min(bar.max);
        }
        true
    }

    /// Makes what just loaded agree with [`ItemState::picked`].
    pub(crate) fn settle_pickups(&mut self, physics: &mut Physics) {
        let mut gone = Vec::new();
        for (entity, pickup) in self.ecs.query_mut::<(Entity, &mut Pickup)>() {
            match self.item_state.picked.get(&pickup.key) {
                Some(0) => gone.push(entity),
                Some(&left) => pickup.count = left,
                None => {}
            }
        }
        for entity in gone {
            self.despawn(entity, physics);
        }
    }

    /// Puts the items in the world as `state` has them.
    pub(crate) fn restore_items(&mut self, state: &ItemState, physics: &mut Physics) {
        let stale: Vec<Entity> =
            self.ecs.query::<(Entity, &Pickup)>().iter().filter(|(_, p)| p.dropped).map(|(e, _)| e).collect();
        for entity in stale {
            self.despawn(entity, physics);
        }
        self.item_state = state.clone();
        let catalog = self.items.clone();
        for inventory in self.item_state.containers.values_mut() {
            inventory.settle(&catalog, OLD_CONTAINER.0, OLD_CONTAINER.1);
        }
        for drop in state.dropped.clone() {
            let transform = self.origin.transform_to_local(&drop.transform);
            self.spawn_dropped(drop.key, drop.stack, transform);
        }
        self.settle_pickups(physics);
    }
}
