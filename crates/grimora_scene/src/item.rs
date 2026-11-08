//! What there is to carry, as plain data: the catalog of items and what scene files
//! say about them. Carrying them around is in `grimora::items`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use anyhow::{Context as _, Result};
use grimora_core::{MeshData, Quat, Transform, Vec3};
use serde::{Deserialize, Serialize};

use crate::combat::{GunDesc, Weapon};
use crate::model::{DEFAULT_MAX_TEXTURE_SIZE, load_model};
use crate::shapes::MeshDesc;

/// One kind of thing a character can carry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ItemDesc {
    /// How scene files and saved games name it.
    pub id: String,
    /// What the player reads.
    pub name: String,
    pub description: String,
    /// Picture in the hotbar and inventory, from the catalog file. `icons/<id>.png` if not given.
    pub icon: Option<String>,
    /// From the catalog file. Without one, it shows as a small box on the ground.
    pub model: Option<String>,
    /// From the model's units to meters.
    pub scale: f32,
    /// How many fit in one piece.
    pub stack: u32,
    /// The cells it takes in an inventory, across and down.
    pub size: (u32, u32),
    pub kind: ItemKind,
    /// How it lies on the ground, as Euler angles in degrees. If not given, weapons lie flat.
    pub rest: Option<Vec3>,
    /// In coins, in a shop. 0 means nobody deals in it.
    pub price: u32,
}

impl Default for ItemDesc {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            description: String::new(),
            icon: None,
            model: None,
            scale: 1.0,
            stack: 1,
            size: (1, 1),
            kind: ItemKind::Misc,
            rest: None,
            price: 0,
        }
    }
}

/// Where armor goes on the body.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ArmorSlot {
    #[default]
    Head,
    Body,
    Legs,
    Feet,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub enum ItemKind {
    #[default]
    Misc,
    /// Gone when used; gives back what it says.
    Consumable {
        #[serde(default)]
        health: f32,
        #[serde(default)]
        stamina: f32,
    },
    /// Worn. Takes `defense` off each blow. A model made on the character's skeleton, in the same
    /// pose, shows on the body.
    Armor { slot: ArmorSlot, defense: f32 },
    /// Goes in the hand. Its model's blade runs along Y.
    Weapon {
        weapon: Weapon,
        /// Where the hand closes, along the model's Y and in its own units.
        #[serde(default)]
        grip: f32,
        /// Degrees around the blade from how the file has it to how the `weapon.R` joint wants it.
        #[serde(default = "default_turn")]
        turn: f32,
    },
    /// Fires. Its model's barrel runs along X, with Y up.
    Gun {
        gun: GunDesc,
        /// From the `weapon.R` joint to the model, in meters.
        #[serde(default)]
        at: Vec3,
        /// And how it turns there, as Euler angles in degrees.
        #[serde(default)]
        turn: Vec3,
    },
    /// Held open in the free hand, with the spells it can cast.
    Grimoire {
        spells: Vec<SpellDesc>,
        /// From the `hand.L` joint to the model, in meters.
        #[serde(default)]
        at: Vec3,
        /// And how it turns there, as Euler angles in degrees.
        #[serde(default)]
        turn: Vec3,
    },
}

/// A spell in a grimoire. What it does is up to the game; its picture is `icons/<id>.png`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SpellDesc {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// The edge along the model's +X faces back towards the wrist. Any weapon can do with this
/// one that cuts on both sides; one with a single edge there wants the opposite, 90.
fn default_turn() -> f32 {
    -90.0
}

impl ItemDesc {
    /// From the joint that holds it ([`ItemDesc::hold_joint`]) to the model, if it's held at all.
    pub fn hold(&self) -> Option<Transform> {
        match self.kind {
            ItemKind::Weapon { grip, turn, .. } => Some(Transform {
                position: Vec3::NEG_Y * grip * self.scale,
                rotation: Quat::from_rotation_y(turn.to_radians()),
                scale: Vec3::splat(self.scale),
            }),
            ItemKind::Grimoire { at, turn, .. } | ItemKind::Gun { at, turn, .. } => Some(Transform {
                position: at,
                rotation: Quat::from_euler(
                    glam::EulerRot::YXZ,
                    turn.y.to_radians(),
                    turn.x.to_radians(),
                    turn.z.to_radians(),
                ),
                scale: Vec3::splat(self.scale),
            }),
            _ => None,
        }
    }

    /// Which joint [`ItemDesc::hold`] is measured from.
    pub fn hold_joint(&self) -> &'static str {
        match self.kind {
            ItemKind::Grimoire { .. } => "hand.L",
            _ => "weapon.R",
        }
    }

    /// What it fires like, if it's a gun.
    pub fn gun(&self) -> Option<&GunDesc> {
        match &self.kind {
            ItemKind::Gun { gun, .. } => Some(gun),
            _ => None,
        }
    }

    /// The spells it holds, if it's a grimoire.
    pub fn spells(&self) -> &[SpellDesc] {
        match &self.kind {
            ItemKind::Grimoire { spells, .. } => spells,
            _ => &[],
        }
    }

    /// How it lies at `at` on the ground, with its lowest point there, turned by `yaw` degrees.
    pub fn lying(&self, mesh: &MeshData, at: Vec3, yaw: f32) -> Transform {
        let rest = match (self.rest, &self.kind) {
            (Some(rest), _) => rest,
            (None, ItemKind::Weapon { .. } | ItemKind::Gun { .. }) => Vec3::new(90.0, 0.0, 0.0),
            (None, _) => Vec3::ZERO,
        };
        let mut transform = Transform::from_euler_degrees(at, rest + Vec3::Y * yaw, Vec3::splat(self.scale));
        let lowest = mesh
            .vertices
            .iter()
            .map(|vertex| (transform.rotation * (Vec3::from(vertex.position) * self.scale)).y)
            .fold(f32::INFINITY, f32::min);
        if lowest.is_finite() {
            transform.position.y -= lowest;
        }
        transform
    }
}

/// Every item there is, read from one RON file: a list of [`ItemDesc`].
#[derive(Debug, Clone, Default)]
pub struct ItemCatalog {
    items: Vec<ItemDesc>,
    index: HashMap<String, usize>,
    /// Paths in the file start from here.
    dir: PathBuf,
}

impl ItemCatalog {
    /// Reads a catalog. Reading the same unchanged file again costs nothing.
    pub fn load(path: impl AsRef<Path>) -> Result<Arc<Self>> {
        type Cached = (Option<SystemTime>, Arc<ItemCatalog>);
        static CACHE: OnceLock<Mutex<HashMap<PathBuf, Cached>>> = OnceLock::new();
        let path = path.as_ref();
        let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        let mut cache = CACHE.get_or_init(Default::default).lock().unwrap();
        if let Some((cached, catalog)) = cache.get(path)
            && *cached == mtime
        {
            return Ok(catalog.clone());
        }
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let dir = path.parent().unwrap_or(Path::new(""));
        let catalog = Arc::new(Self::parse(&text, dir).with_context(|| format!("parsing {}", path.display()))?);
        cache.insert(path.to_owned(), (mtime, catalog.clone()));
        Ok(catalog)
    }

    /// Reads the text of a catalog whose paths start from `dir`.
    pub fn parse(text: &str, dir: &Path) -> Result<Self> {
        let options = ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
        let items: Vec<ItemDesc> = options.from_str(text)?;
        let mut index = HashMap::new();
        for (at, item) in items.iter().enumerate() {
            if index.insert(item.id.clone(), at).is_some() {
                log::warn!("the item {:?} is in the catalog twice; the last one counts", item.id);
            }
        }
        Ok(Self { items, index, dir: dir.to_owned() })
    }

    pub fn get(&self, id: &str) -> Option<&ItemDesc> {
        self.index.get(id).map(|&at| &self.items[at])
    }

    pub fn iter(&self) -> impl Iterator<Item = &ItemDesc> {
        self.items.iter()
    }

    /// How many of `id` fit in one slot.
    pub fn stack_size(&self, id: &str) -> u32 {
        self.get(id).map_or(1, |item| item.stack.max(1))
    }

    /// The cells id takes in an inventory, across and down.
    pub fn cells(&self, id: &str) -> (u32, u32) {
        self.get(id).map_or((1, 1), |item| (item.size.0.max(1), item.size.1.max(1)))
    }

    /// The picture of the item as a piece of an inventory, one [ItemDesc::size] of cells.
    pub fn piece_path(&self, item: &ItemDesc) -> PathBuf {
        self.dir.join("pieces").join(format!("{}.png", item.id))
    }

    pub fn icon_path(&self, item: &ItemDesc) -> PathBuf {
        match &item.icon {
            Some(icon) => self.dir.join(icon),
            None => self.dir.join("icons").join(format!("{}.png", item.id)),
        }
    }

    pub fn spell_icon_path(&self, spell: &SpellDesc) -> PathBuf {
        self.dir.join("icons").join(format!("{}.png", spell.id))
    }

    pub fn model_path(&self, item: &ItemDesc) -> Option<PathBuf> {
        item.model.as_ref().map(|model| self.dir.join(model))
    }

    /// The item's model, or a small box if it has none or it won't load.
    pub fn mesh(&self, item: &ItemDesc) -> MeshData {
        let model = self.model_path(item).map(|path| load_model(&path, DEFAULT_MAX_TEXTURE_SIZE));
        match model {
            Some(Ok(mesh)) => return mesh,
            Some(Err(err)) => log::error!("item {:?}: {err:#}", item.id),
            None => {}
        }
        MeshDesc::Cube.build()
    }
}

/// `pickup:` on an object: it lies there to be taken. Its mesh, scale and how it lies come
/// from the catalog, and it gets an `interact` if it has none.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PickupDesc {
    pub item: String,
    pub count: u32,
}

impl Default for PickupDesc {
    fn default() -> Self {
        Self { item: String::new(), count: 1 }
    }
}

/// `container:` on an object: a chest. What's inside lives with the saved game once it's touched.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ContainerDesc {
    /// How much it holds, if it doesn't say its size: this many cells, in rows of 8.
    pub slots: u32,
    /// Its grid, across and down.
    pub size: (u32, u32),
    /// What it starts with, as (item, how many).
    pub items: Vec<(String, u32)>,
}

impl Default for ContainerDesc {
    fn default() -> Self {
        Self { slots: 16, size: (0, 0), items: Vec::new() }
    }
}

impl ContainerDesc {
    /// Its grid, across and down.
    pub fn grid(&self) -> (u32, u32) {
        match self.size {
            (0, _) | (_, 0) => (8, self.slots.div_ceil(8).max(1)),
            size => size,
        }
    }
}

/// What an object in a built scene is, to the item system. `key` says which one it is across
/// saved games and streaming: the object's name, or its item and place if it has none.
#[derive(Debug, Clone, PartialEq)]
pub enum ObjectItems {
    Pickup { key: String, item: String, count: u32 },
    Container { key: String, desc: ContainerDesc },
}

/// The key of an object: see [`ObjectItems`].
pub(crate) fn object_key(name: Option<&String>, what: &str, position: Vec3) -> String {
    match name {
        Some(name) => name.clone(),
        None => format!("{what}@{:.1},{:.1},{:.1}", position.x, position.y, position.z),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATALOG: &str = r#"[
        (id: "maca", name: "Maçã", stack: 5, kind: Consumable(health: 15.0)),
        (id: "pedra", name: "Pedra", stack: 0),
        (id: "espada", name: "Espada", scale: 0.25, kind: Weapon(weapon: (damage: 12.0), grip: 0.2, turn: 90.0)),
    ]"#;

    #[test]
    fn the_catalog_says_what_each_item_is() {
        let catalog = ItemCatalog::parse(CATALOG, Path::new("items")).unwrap();
        assert_eq!(catalog.iter().count(), 3);
        assert_eq!(catalog.stack_size("maca"), 5);
        assert_eq!(catalog.stack_size("pedra"), 1, "a stack of none would never hold anything");
        assert_eq!(catalog.stack_size("nada"), 1);
        assert!(matches!(catalog.get("maca").unwrap().kind, ItemKind::Consumable { health: 15.0, stamina: 0.0 }));
        assert!(catalog.get("maca").unwrap().hold().is_none());
        let sword = catalog.get("espada").unwrap();
        assert!(matches!(&sword.kind, ItemKind::Weapon { weapon, .. } if weapon.damage == 12.0));
        assert_eq!(catalog.icon_path(sword), Path::new("items").join("icons").join("espada.png"));

        let hold = sword.hold().unwrap();
        assert!((hold.position.y + 0.05).abs() < 1e-6, "the grip's place goes to the joint: {}", hold.position);
        let along = hold.rotation * Vec3::X;
        assert!((along - Vec3::NEG_Z).length() < 1e-5, "its edge faces away from the wrist: {along}");
    }

    #[test]
    fn things_lie_on_the_ground() {
        let catalog = ItemCatalog::parse(CATALOG, Path::new("")).unwrap();
        let cube = MeshDesc::Cube.build();
        let lowest = |item: &ItemDesc, t: Transform| {
            let low = |v: &grimora_core::Vertex| (t.rotation * (Vec3::from(v.position) * item.scale)).y;
            t.position.y + cube.vertices.iter().map(low).fold(f32::INFINITY, f32::min)
        };
        for id in ["maca", "espada"] {
            let item = catalog.get(id).unwrap();
            let lying = item.lying(&cube, Vec3::new(1.0, 3.0, 2.0), 40.0);
            assert!((lowest(item, lying) - 3.0).abs() < 1e-5, "{id}");
        }
        let sword = catalog.get("espada").unwrap().lying(&cube, Vec3::ZERO, 0.0);
        assert!((sword.rotation * Vec3::Y).y.abs() < 1e-5, "a blade lies flat");
    }
}

/// What sort of thing goes in a place on the body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StowFamily {
    /// Hangs from the belt, blade back.
    Sword,
    /// Across the back: anything too long for the belt.
    Back,
    /// A long gun over the shoulder.
    Slung,
    /// A pistol, muzzle down.
    Pistol,
    /// Hangs from the belt, whatever it is.
    Small,
    /// On the hip, for the free hand.
    Book,
    /// Out of a pocket: it doesn't show until it's in the hand, and any number of things fit.
    Pocket,
}

/// How a thing lies on the body, in the model's space, touching it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StowLie {
    /// Where the handle goes, or the middle of it, by family; a book touches the body here.
    pub at: (f32, f32, f32),
    /// Where the tip or the muzzle points.
    pub long: (f32, f32, f32),
    /// Where the edge or the top of a gun points.
    pub across: (f32, f32, f32),
}

/// A place on the body. It's only the moment the clips have the hand there: see
/// [`StowPlace::frame`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StowPlace {
    pub id: String,
    pub family: StowFamily,
    /// The joint it rides on.
    pub bone: String,
    /// The joint of the hand that takes it.
    pub hand: String,
    /// Seconds into [`StowPlace::draw`] when the hand has it.
    pub grab: f32,
    /// Seconds into [`StowPlace::stow`] when the hand lets go.
    pub release: f32,
    /// How it lies there. Without it, the way the hand takes it.
    #[serde(default)]
    pub lies: Option<StowLie>,
    /// Another hand takes it from the body, and gives it to [`StowPlace::hand`] at this moment.
    #[serde(default)]
    pub pass: Option<StowPass>,
}

/// A thing taken by one hand and handed to the other.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StowPass {
    /// The hand that takes it from the body.
    pub hand: String,
    /// Seconds into the draw when it goes to the other hand.
    pub draw: f32,
    /// Seconds into the stow when it comes back to this one.
    pub stow: f32,
}

impl StowPlace {
    pub fn draw(&self) -> String {
        format!("draw_{}", self.id)
    }

    pub fn stow(&self) -> String {
        format!("stow_{}", self.id)
    }

    /// The hand at the moment of the grab, from the bone. Whatever is held there by that hand
    /// stays exactly where it is when it changes from one joint to the other.
    pub fn frame(&self, skeleton: &grimora_core::Skeleton, time: f32) -> Option<grimora_core::Mat4> {
        self.frame_of(skeleton, &self.draw(), time)
    }

    /// The same, as the other clip has it.
    pub fn frame_stowing(&self, skeleton: &grimora_core::Skeleton) -> Option<grimora_core::Mat4> {
        self.frame_of(skeleton, &self.stow(), self.release)
    }

    fn frame_of(&self, skeleton: &grimora_core::Skeleton, clip: &str, time: f32) -> Option<grimora_core::Mat4> {
        let clip = &skeleton.clips[skeleton.clip(clip)?];
        let mut pose = skeleton.rest_pose();
        clip.sample(time, &mut pose);
        let mut globals = Vec::new();
        skeleton.global_transforms(&pose, &mut globals);
        let (bone, hand) = (skeleton.joint(&self.bone)?, skeleton.joint(&self.hand)?);
        Some(globals[bone].inverse() * globals[hand])
    }
}

/// `chartest_stow.ron`, from `tools/gen_stow_clips.py`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StowBook {
    /// Seconds into `item_eat` when the bite happens.
    pub eat_bite: f32,
    pub places: Vec<StowPlace>,
}

impl StowBook {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        ron::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }
}
