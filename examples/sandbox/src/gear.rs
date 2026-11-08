//! What's carried on the body and how it gets to the hand. Every piece of the hotbar shows
//! somewhere on the character, each in a place of its own (`chartest_stow.ron`), and changing what's
//! in the hand plays the clips that put one away and take the other out. The model is one entity all
//! along, going from a joint of the body to the hand at the moment the clip has the hand on it.

use grimora::Attached;
use grimora::core::{Mat3, Mat4, Skeleton};
use grimora::prelude::*;
use grimora::scene::{StowBook, StowFamily, StowLie, StowPlace};

use crate::bag::{Equipped, HeldKind};
use crate::{Sandbox, actions};

const STOW_BOOK: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/models/chartest/chartest_stow.ron");

const INTO_CHANGE_FADE: f32 = 0.14;
const OUT_OF_CHANGE_FADE: f32 = 0.22;
/// The hand starts closing this long before it gets there.
const CLOSE_AHEAD: f32 = 0.2;
const ACTION_FADE: f32 = 0.2;
const OFFER_CLIP: &str = "item_offer";
const EAT_CLIP: &str = "item_eat";
/// A thing in the hand is never bigger than this for each cell it takes in the inventory, however
/// big its model is, in meters, and never bigger than the most a hand can take.
const LARGEST_PER_CELL: f32 = 0.125;
const LARGEST_THING: f32 = 0.28;
/// What has no model yet is a small box of this size.
const NO_MODEL_SIZE: f32 = 0.07;
/// A blade longer than this, in meters, doesn't fit a belt.
const LONGEST_ON_THE_BELT: f32 = 1.02;
/// A blade shorter than this goes on the right of the belt, where a dagger goes.
const SHORT_BLADE: f32 = 0.4;
/// What a blade sounds like coming out of its sheath.
const DRAW_KNIFE_SOUND: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/draw_knife.wav");
const DRAW_SWORD_SOUND: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/draw_sword.wav");
/// The handle the hand closes on, in `chartest_grip.ron`: its joint's -X goes out of the palm, so
/// something round sits this much short of its own radius from there, against the palm.
const HANDLE_RADIUS: f32 = 0.0237;
/// What's smaller than the handle stays this far out, so that it shows.
const LEAST_OUT_OF_THE_PALM: f32 = 0.017;
/// What's held in the palm sits this far past the handle, towards the fingers.
const TOWARDS_THE_FINGERS: f32 = 0.022;
/// Fingers of a hand holding something out, on the same scale as [`fingers_for`].
const OFFERING_OPEN: f32 = 0.85;
/// What rides on the chest has this much more room with a breastplate on.
const ROOM_FOR_ARMOR: f32 = 0.03;
/// Seconds for something to settle in the hand, or on the body, once it gets there.
const GLIDE: f32 = 0.16;
/// Things on the body want this far between them, in meters.
const ROOM_TO_SPARE: f32 = 0.05;
/// A thing this wide, in meters, has the fingers as open as they go around something.
const WIDEST_IN_THE_FINGERS: f32 = 0.14;
const LEAST_OPEN: f32 = 0.2;
const MOST_OPEN: f32 = 0.62;

/// Whether it can go in the hand. Only what's worn can't.
pub fn carryable(item: &ItemDesc) -> bool {
    !matches!(item.kind, ItemKind::Armor { .. })
}

/// The corners of a mesh's box.
fn bounds(mesh: &MeshData) -> (Vec3, Vec3) {
    let start = (Vec3::INFINITY, Vec3::NEG_INFINITY);
    mesh.vertices.iter().fold(start, |(least, most), vertex| {
        let at = Vec3::from(vertex.position);
        (least.min(at), most.max(at))
    })
}

fn largest_of(item: &ItemDesc) -> f32 {
    (LARGEST_PER_CELL * item.size.0.max(item.size.1) as f32).min(LARGEST_THING)
}

/// What holds it, and its model from there: what the catalog says, or for something with no way of
/// being held in particular, the middle of it just past the fist.
fn hold_of(item: &ItemDesc, mesh: &MeshData) -> Transform {
    if let Some(hold) = item.hold() {
        return hold;
    }
    let (least, most) = bounds(mesh);
    let size = (most - least).max_element().max(1e-3);
    let scale = if item.model.is_some() { item.scale.min(largest_of(item) / size) } else { NO_MODEL_SIZE / size };
    Transform {
        position: -(least + most) * 0.5 * scale
            + Vec3::NEG_X * ((most.x - least.x) * scale * 0.5 - HANDLE_RADIUS).max(LEAST_OUT_OF_THE_PALM)
            + Vec3::NEG_Z * TOWARDS_THE_FINGERS,
        rotation: Quat::IDENTITY,
        scale: Vec3::splat(scale),
    }
}

/// How far the fingers stay open around it, or `None` to close on a grip.
fn fingers_for(item: &ItemDesc, mesh: &MeshData) -> Option<f32> {
    if item.hold().is_some() {
        return None;
    }
    let (least, most) = bounds(mesh);
    let wide = (most - least).max_element() * hold_of(item, mesh).scale.x;
    Some((wide / WIDEST_IN_THE_FINGERS).clamp(LEAST_OPEN, MOST_OPEN))
}

fn kind_of(item: &ItemDesc) -> HeldKind {
    match item.kind {
        ItemKind::Weapon { .. } => HeldKind::Weapon,
        ItemKind::Gun { .. } => HeldKind::Gun,
        ItemKind::Grimoire { .. } => HeldKind::Book,
        _ => HeldKind::Thing,
    }
}

/// Which kinds of place it goes to, the first one first.
fn families_of(item: &ItemDesc) -> &'static [StowFamily] {
    match &item.kind {
        ItemKind::Weapon { weapon, .. } => {
            let tip = weapon.blade.as_ref().map_or(0.8, |blade| blade.tip);
            if weapon.clip.starts_with("heavy") || tip > LONGEST_ON_THE_BELT || item.id == "machado_grande" {
                &[StowFamily::Back, StowFamily::Slung]
            } else {
                &[StowFamily::Sword]
            }
        }
        ItemKind::Gun { .. } if item.id == "mosquete" => &[StowFamily::Slung, StowFamily::Back],
        ItemKind::Gun { .. } => &[StowFamily::Pistol, StowFamily::Small],
        ItemKind::Grimoire { .. } => &[StowFamily::Book],
        _ if is_a_flask(item) => &[StowFamily::Small],
        _ => &[StowFamily::Pocket],
    }
}

/// A potion hangs from the belt. Anything else that's plain comes out of a pocket.
fn is_a_flask(item: &ItemDesc) -> bool {
    matches!(item.kind, ItemKind::Consumable { .. }) && item.size.1 > 1
}

/// A short blade goes on the right of the belt first.
fn short_blade(item: &ItemDesc) -> bool {
    matches!(&item.kind, ItemKind::Weapon { weapon, .. } if weapon.blade.as_ref().is_some_and(|b| b.tip < SHORT_BLADE))
}

/// One model the character has on: in the hand, or in its place on the body.
#[derive(Debug, Clone)]
struct Carried {
    piece: u32,
    item: String,
    entity: Entity,
    /// One of [`StowBook::places`], unless there's no place left for it.
    place: Option<String>,
    /// How it lies there.
    lies: Option<Segment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Before the moment in the clip, it's still in the hand.
    Putting { piece: u32, let_go: bool },
    /// Before the moment in the clip, it's still on the body.
    Taking { piece: u32, got: bool },
}

impl Stage {
    fn piece(self) -> u32 {
        match self {
            Stage::Putting { piece, .. } | Stage::Taking { piece, .. } => piece,
        }
    }
}

/// What the hand is doing with the thing in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Offering,
    Eating { bitten: bool },
}

#[derive(Debug, Clone, Copy)]
struct Glide {
    entity: Entity,
    from: Transform,
    to: Transform,
    time: f32,
}

#[derive(Default)]
pub struct Gear {
    book: Option<StowBook>,
    carried: Vec<Carried>,
    stage: Option<Stage>,
    action: Option<Action>,
    /// The next change happens at once: what a game loads with has no reason to be drawn.
    skip: bool,
    /// What's still settling: the entity, where it comes from and where it's going.
    glides: Vec<Glide>,
    /// Whether what's on the chest makes room for a breastplate.
    armored: bool,
    /// A `--menu gear:katana,*maca` run: what goes in the hotbar, with `*` on what's in the hand and
    /// `+` on what's in the hand and being offered.
    pub scheduled: Option<String>,
    /// The second button is held down for the capture.
    offering_for_a_capture: bool,
}

impl Gear {
    /// The hand has the upper body.
    pub fn busy(&self) -> bool {
        self.stage.is_some() || self.action.is_some()
    }

    pub fn skip_change(&mut self) {
        self.skip = true;
    }

    /// The character is new, and has nothing on it.
    pub fn forget(&mut self) {
        self.carried.clear();
        self.glides.clear();
        self.stage = None;
        self.action = None;
        self.skip = true;
    }

    fn place(&self, id: &str) -> Option<&StowPlace> {
        self.book.as_ref()?.places.iter().find(|place| place.id == id)
    }

    fn pocket(&self) -> Option<String> {
        let places = &self.book.as_ref()?.places;
        places.iter().find(|place| place.family == StowFamily::Pocket).map(|place| place.id.clone())
    }

    fn place_of(&self, piece: u32) -> Option<StowPlace> {
        self.place(self.of(piece)?.place.as_deref()?).cloned()
    }

    fn of(&self, piece: u32) -> Option<&Carried> {
        self.carried.iter().find(|carried| carried.piece == piece)
    }

    /// The first place for this with room to spare, or else the one with the most, measured with
    /// the length of this very thing.
    fn free_place(&self, item: &ItemDesc, mesh: &MeshData, skeleton: &Skeleton) -> Option<(String, Segment)> {
        let book = self.book.as_ref()?;
        let taken: Vec<&str> = self.carried.iter().filter_map(|carried| carried.place.as_deref()).collect();
        // Nothing lies in a pocket, so there's nothing to be away from.
        let others: Vec<Segment> = self.carried.iter().filter_map(|carried| carried.lies).collect();
        let mut best: Option<(f32, String, Segment)> = None;
        for family in families_of(item) {
            if *family == StowFamily::Pocket {
                continue;
            }
            let mut here: Vec<&StowPlace> = book
                .places
                .iter()
                .filter(|place| place.family == *family && place.hand == item.hold_joint())
                .filter(|place| !taken.contains(&place.id.as_str()))
                .collect();
            if short_blade(item) {
                here.sort_by_key(|place| !place.id.starts_with("belt_r"));
            }
            for place in here {
                let Some(lies) = lies_at(place, item, mesh, skeleton) else { continue };
                let room = others.iter().map(|other| apart(lies, *other)).fold(f32::MAX, f32::min);
                if room >= ROOM_TO_SPARE {
                    return Some((place.id.clone(), lies));
                }
                if best.as_ref().is_none_or(|(most, ..)| room > *most) {
                    best = Some((room, place.id.clone(), lies));
                }
            }
        }
        best.map(|(_, place, lies)| (place, lies))
    }
}

/// From one end of something to the other.
type Segment = (Vec3, Vec3);

/// The ends of a model, along its longest side.
fn ends_of(mesh: &MeshData) -> Segment {
    let (least, most) = bounds(mesh);
    let (middle, size) = ((least + most) * 0.5, most - least);
    let along = if size.x >= size.y && size.x >= size.z {
        Vec3::X
    } else if size.y >= size.z {
        Vec3::Y
    } else {
        Vec3::Z
    };
    let half = along * along.dot(size) * 0.5;
    (middle - half, middle + half)
}

/// How it lies on the body, in the model's space: the place says where its long side goes and
/// which way its edge points, and that's from the file's axes to there.
fn laid(lie: &StowLie, family: StowFamily, item: &ItemDesc, mesh: &MeshData) -> Mat4 {
    let (file_long, file_across) = match item.kind {
        ItemKind::Gun { .. } | ItemKind::Grimoire { .. } => (Vec3::X, Vec3::Y),
        _ => (Vec3::Y, Vec3::X),
    };
    let vector = |v: (f32, f32, f32)| Vec3::new(v.0, v.1, v.2);
    let long = vector(lie.long).normalize();
    let across = vector(lie.across);
    let across = (across - long * across.dot(long)).normalize();
    let wanted = Mat3::from_cols(long, across, long.cross(across));
    let file = Mat3::from_cols(file_long, file_across, file_long.cross(file_across));
    let turn = Quat::from_mat3(&(wanted * file.transpose()));
    let hold = hold_of(item, mesh);
    let (least, most) = bounds(mesh);
    let anchor = match family {
        // By the handle, where the hand goes for it.
        StowFamily::Sword | StowFamily::Back | StowFamily::Pistol | StowFamily::Slung => {
            hold.matrix().inverse().transform_point3(Vec3::ZERO) * hold.scale
        }
        _ => (least + most) * 0.5 * hold.scale,
    };
    let touching = match family {
        StowFamily::Book => across * (most - least).dot(file_across.abs()) * 0.5 * hold.scale.x,
        _ => Vec3::ZERO,
    };
    Mat4::from_scale_rotation_translation(hold.scale, turn, vector(lie.at) + touching - turn * anchor)
}

/// From the joint a place rides on to the thing in that place.
fn offset_on_body(
    place: &StowPlace,
    item: &ItemDesc,
    mesh: &MeshData,
    skeleton: &Skeleton,
    armored: bool,
) -> Option<Mat4> {
    let Some(lie) = &place.lies else {
        return Some(place.frame(skeleton, place.grab)? * hold_of(item, mesh).matrix());
    };
    let mut globals = Vec::new();
    skeleton.global_transforms(&skeleton.rest_pose(), &mut globals);
    let room = if armored && place.bone == "chest" { Vec3::Z * ROOM_FOR_ARMOR } else { Vec3::ZERO };
    let on_it = Mat4::from_translation(room) * laid(lie, place.family, item, mesh);
    Some(globals[skeleton.joint(&place.bone)?].inverse() * on_it)
}

/// Where a thing would lie on a character standing in its bind pose.
fn lies_at(place: &StowPlace, item: &ItemDesc, mesh: &MeshData, skeleton: &Skeleton) -> Option<Segment> {
    let mut globals = Vec::new();
    skeleton.global_transforms(&skeleton.rest_pose(), &mut globals);
    let bone = globals[skeleton.joint(&place.bone)?];
    let matrix = bone * offset_on_body(place, item, mesh, skeleton, false)?;
    let (from, to) = ends_of(mesh);
    Some((matrix.transform_point3(from), matrix.transform_point3(to)))
}

/// The closest two segments come.
fn apart(a: Segment, b: Segment) -> f32 {
    const STEPS: u32 = 16;
    let along = |segment: Segment, step: u32| segment.0.lerp(segment.1, step as f32 / STEPS as f32);
    let mut least = f32::MAX;
    for i in 0..=STEPS {
        for j in 0..=STEPS {
            least = least.min(along(a, i).distance(along(b, j)));
        }
    }
    least
}

impl Sandbox {
    /// Keeps what's on the body up to date with the hotbar, moves things in and out of the hand,
    /// and runs what's done with something held.
    pub(crate) fn update_gear(&mut self, ctx: &mut Context, dodging: bool) {
        if self.bag.gear.book.is_none() {
            let book = StowBook::load(STOW_BOOK);
            self.bag.gear.book = Some(book.map_err(|err| eprintln!("[sandbox] {err:#}")).unwrap_or_default());
        }
        self.stock_for_a_capture(ctx);
        self.drop_what_is_gone(ctx);
        self.put_on_the_body(ctx);
        let cut_short = dodging
            || self.dead_for.is_some()
            || ctx.world.get::<Health>(self.avatar).is_some_and(|health| health.staggered());
        self.run_change(ctx, cut_short);
        self.run_action(ctx, cut_short);
        self.hold_hand(ctx);
        self.run_glides(ctx);
    }

    fn stock_for_a_capture(&mut self, ctx: &mut Context) {
        let Some(list) = self.bag.gear.scheduled.take() else { return };
        let catalog = ctx.world.items.clone();
        self.bag.inventory = Inventory::new(40, 40);
        for (key, name) in list.split(',').enumerate() {
            let item = name.trim_start_matches(['*', '+']);
            self.bag.inventory.add(&catalog, item, 1);
            let piece = self.bag.inventory.pieces().last().map(|piece| piece.id);
            self.bag.inventory.set_shortcut(key.min(HOTBAR - 1), piece);
            if name.starts_with(['*', '+']) {
                self.bag.inventory.set_hand(piece);
            }
            self.bag.gear.offering_for_a_capture |= name.starts_with('+');
        }
        self.bag.gear.skip = true;
    }

    /// The piece that should be in the hand.
    fn wanted_in_hand(&self, ctx: &Context) -> Option<u32> {
        let id = self.bag.inventory.hand()?;
        let item = ctx.world.items.get(&self.bag.inventory.piece(id)?.stack.item)?;
        carryable(item).then_some(id)
    }

    /// Whatever isn't carried any longer, or went with a character that's gone.
    fn drop_what_is_gone(&mut self, ctx: &mut Context) {
        let avatar = self.avatar;
        let hand = self.bag.equipped.as_ref().map(|equipped| equipped.piece);
        let moving = self.bag.gear.stage.map(Stage::piece);
        let wanted = self.wanted_in_hand(ctx);
        let mut keep = Vec::new();
        for carried in std::mem::take(&mut self.bag.gear.carried) {
            let there = ctx.world.get::<Attached>(carried.entity).is_some_and(|attached| attached.parent == avatar);
            let same = self.bag.inventory.piece(carried.piece).is_some_and(|piece| piece.stack.item == carried.item);
            let shown = self.bag.inventory.shortcut_of(carried.piece).is_some();
            let involved = [hand, moving, wanted].contains(&Some(carried.piece));
            if there && same && (shown || involved) {
                keep.push(carried);
                continue;
            }
            if there {
                ctx.world.despawn(carried.entity, &mut ctx.physics);
            }
            if hand == Some(carried.piece) {
                self.bag.equipped = None;
                self.bag.gear.action = None;
                self.set_blows(ctx, None);
                if let Some(animator) = ctx.world.get_mut::<Animator>(avatar) {
                    animator.overlay_stop(OUT_OF_CHANGE_FADE);
                }
            }
            if moving == Some(carried.piece) {
                self.end_change(ctx);
            }
        }
        self.bag.gear.carried = keep;
    }

    /// What's on the chest moves out for a breastplate, and back when it comes off.
    fn make_room_for_armor(&mut self, ctx: &mut Context) {
        let armored = self.bag.worn_in(grimora::scene::ArmorSlot::Body).is_some();
        if armored == self.bag.gear.armored {
            return;
        }
        self.bag.gear.armored = armored;
        let in_hand = self.bag.equipped.as_ref().map(|equipped| equipped.piece);
        let moving = self.bag.gear.stage.map(Stage::piece);
        let pieces: Vec<u32> = self.bag.gear.carried.iter().map(|carried| carried.piece).collect();
        for piece in pieces.into_iter().filter(|piece| ![in_hand, moving].contains(&Some(*piece))) {
            self.send_to_body(ctx, piece, false);
        }
    }

    /// Every piece of the hotbar, and what's about to be in the hand, gets its model and its place.
    fn put_on_the_body(&mut self, ctx: &mut Context) {
        self.make_room_for_armor(ctx);
        let catalog = ctx.world.items.clone();
        let slots = (0..HOTBAR).filter_map(|key| self.bag.inventory.shortcut(key).map(|piece| piece.id));
        let pieces: Vec<u32> = slots.chain(self.wanted_in_hand(ctx)).collect();
        for id in pieces {
            if self.bag.gear.of(id).is_some() {
                continue;
            }
            let Some(item) = self.bag.inventory.piece(id).and_then(|piece| catalog.get(&piece.stack.item)) else {
                continue;
            };
            if !carryable(item) {
                continue;
            }
            let Some(mesh) = ctx.world.item_mesh(&item.id) else { continue };
            let skeleton = ctx.world.get::<Animator>(self.avatar).map(|animator| animator.skeleton().clone());
            let chosen = skeleton
                .zip(ctx.world.mesh(mesh))
                .and_then(|(skeleton, data)| self.bag.gear.free_place(item, data, &skeleton));
            let (mut place, lies) = chosen.map_or((None, None), |(place, lies)| (Some(place), Some(lies)));
            if families_of(item).contains(&StowFamily::Pocket) {
                place = self.bag.gear.pocket();
            }
            let entity = ctx.world.attach(self.avatar, item.hold_joint(), mesh, Transform::IDENTITY);
            let carried = Carried { piece: id, item: item.id.clone(), entity, place, lies };
            self.bag.gear.carried.push(carried);
            self.send_to_body(ctx, id, false);
        }
    }

    /// From the joint that holds it to the model.
    fn hold_for(&self, ctx: &mut Context, item: &ItemDesc) -> Option<Transform> {
        let mesh = ctx.world.item_mesh(&item.id)?;
        Some(hold_of(item, ctx.world.mesh(mesh)?))
    }

    /// On the joint that carries it, right where the clip's hand has it. With no place left, it
    /// doesn't show until it's in the hand.
    fn send_to_body(&mut self, ctx: &mut Context, piece: u32, glide: bool) {
        let catalog = ctx.world.items.clone();
        let Some(carried) = self.bag.gear.of(piece).cloned() else { return };
        let Some(item) = catalog.get(&carried.item) else { return };
        let place = self.bag.gear.place_of(piece);
        let armored = self.bag.gear.armored;
        let offset = place.as_ref().and_then(|place| {
            let mesh = ctx.world.item_mesh(&item.id)?;
            let skeleton = ctx.world.get::<Animator>(self.avatar)?.skeleton().clone();
            let (scale, rotation, position) =
                offset_on_body(place, item, ctx.world.mesh(mesh)?, &skeleton, armored)?.to_scale_rotation_translation();
            Some(Transform { position, rotation, scale })
        });
        let visible = match (place, offset) {
            (Some(place), Some(offset)) => {
                self.settle(ctx, carried.entity, &place.bone, offset, glide);
                place.family != StowFamily::Pocket
            }
            // Out of the way, so that the hand doesn't think it has it.
            _ => {
                self.settle(ctx, carried.entity, "hips", Transform::IDENTITY, false);
                false
            }
        };
        set_visible(ctx, carried.entity, visible);
        // A book closes when it's put away.
        if let Some(animator) = ctx.world.get_mut::<Animator>(carried.entity) {
            animator.stop();
        }
    }

    fn send_to_hand(&mut self, ctx: &mut Context, piece: u32, glide: bool) {
        let catalog = ctx.world.items.clone();
        let Some(carried) = self.bag.gear.of(piece).cloned() else { return };
        let Some(item) = catalog.get(&carried.item) else { return };
        let Some(hold) = self.hold_for(ctx, item) else { return };
        self.settle(ctx, carried.entity, item.hold_joint(), hold, glide);
        set_visible(ctx, carried.entity, true);
    }

    /// Hangs it from this joint just where it is, as a hand taking it from another.
    fn hold_as_it_is(&mut self, ctx: &mut Context, entity: Entity, joint: &str) {
        self.bag.gear.glides.retain(|glide| glide.entity != entity);
        let Some(now) = ctx.world.transform(entity) else { return };
        let Some(hanger) = ctx.world.joint_transform(self.avatar, joint) else { return };
        let (scale, rotation, position) = (hanger.matrix().inverse() * now.matrix()).to_scale_rotation_translation();
        if let Some(attached) = ctx.world.get_mut::<Attached>(entity) {
            attached.joint = joint.to_owned();
            attached.offset = Transform { position, rotation, scale };
        }
    }

    fn joint_of(ctx: &Context, entity: Entity) -> Option<String> {
        ctx.world.get::<Attached>(entity).map(|attached| attached.joint.clone())
    }

    /// Hangs it from this joint. Gliding, it starts from where it is and gets to `offset` shortly.
    fn settle(&mut self, ctx: &mut Context, entity: Entity, joint: &str, offset: Transform, glide: bool) {
        self.bag.gear.glides.retain(|glide| glide.entity != entity);
        let from = glide.then(|| {
            let (now, hanger) = (ctx.world.transform(entity)?, ctx.world.joint_transform(self.avatar, joint)?);
            let (scale, rotation, position) =
                (hanger.matrix().inverse() * now.matrix()).to_scale_rotation_translation();
            Some(Transform { position, rotation, scale })
        });
        let start = from.flatten();
        if let Some(attached) = ctx.world.get_mut::<Attached>(entity) {
            attached.joint = joint.to_owned();
            attached.offset = start.unwrap_or(offset);
        }
        if let Some(from) = start {
            self.bag.gear.glides.push(Glide { entity, from, to: offset, time: 0.0 });
        }
    }

    fn run_glides(&mut self, ctx: &mut Context) {
        let dt = ctx.time.world_delta();
        let mut glides = std::mem::take(&mut self.bag.gear.glides);
        glides.retain_mut(|glide| {
            glide.time += dt;
            let u = (glide.time / GLIDE).min(1.0);
            let s = u * u * (3.0 - 2.0 * u);
            if let Some(attached) = ctx.world.get_mut::<Attached>(glide.entity) {
                attached.offset = Transform {
                    position: glide.from.position.lerp(glide.to.position, s),
                    rotation: glide.from.rotation.slerp(glide.to.rotation, s),
                    scale: glide.from.scale.lerp(glide.to.scale, s),
                };
            }
            u < 1.0
        });
        self.bag.gear.glides = glides;
    }

    fn set_blows(&mut self, ctx: &mut Context, item: Option<&ItemDesc>) {
        if let Some(fighter) = ctx.world.get_mut::<Fighter>(self.avatar) {
            fighter.weapon = match item.map(|item| &item.kind) {
                Some(ItemKind::Weapon { weapon, .. }) => weapon.clone(),
                _ => Weapon::default(),
            };
        }
    }

    fn equip(&mut self, ctx: &mut Context, piece: u32) {
        let catalog = ctx.world.items.clone();
        let Some(carried) = self.bag.gear.of(piece).cloned() else { return };
        let Some(item) = catalog.get(&carried.item) else { return };
        self.set_blows(ctx, Some(item));
        self.bag.equipped = Some(Equipped { piece, item: carried.item, held: carried.entity, kind: kind_of(item) });
    }

    /// Plays this over the upper body, leaving alone the arm it doesn't use.
    /// With both hands in it, nothing is left to the clip underneath.
    fn play_change(&mut self, ctx: &mut Context, clip: &str, hand: &str, both: bool) {
        if let Some(animator) = ctx.world.get_mut::<Animator>(self.avatar) {
            animator.overlay_once(clip, crate::UPPER_BODY, INTO_CHANGE_FADE);
            let other = if hand == "hand.L" { "clavicle.R" } else { "clavicle.L" };
            animator.overlay_leave((!both).then_some(other));
        }
        if let Some(player) = self.player.as_mut() {
            player.aiming = false;
        }
    }

    fn end_change(&mut self, ctx: &mut Context) {
        self.bag.gear.stage = None;
        if let Some(animator) = ctx.world.get_mut::<Animator>(self.avatar) {
            animator.overlay_stop(OUT_OF_CHANGE_FADE);
            animator.overlay_leave(None);
            animator.set_overlay_steady(0.0);
        }
    }

    /// All at once: everything goes where the change was taking it.
    fn finish_at_once(&mut self, ctx: &mut Context) {
        let wanted = self.wanted_in_hand(ctx);
        let in_hand = self.bag.equipped.as_ref().map(|equipped| equipped.piece);
        let moving = self.bag.gear.stage.map(Stage::piece);
        for piece in [in_hand, moving].into_iter().flatten() {
            if wanted != Some(piece) {
                self.send_to_body(ctx, piece, false);
            }
        }
        if in_hand != wanted {
            self.bag.equipped = None;
            self.set_blows(ctx, None);
            if let Some(piece) = wanted {
                self.send_to_hand(ctx, piece, false);
                self.equip(ctx, piece);
            }
        } else if let Some(piece) = wanted {
            self.send_to_hand(ctx, piece, false);
        }
        self.end_change(ctx);
    }

    /// Moves on with the change of what's in the hand, or starts one.
    fn run_change(&mut self, ctx: &mut Context, cut_short: bool) {
        if std::mem::take(&mut self.bag.gear.skip) {
            return self.finish_at_once(ctx);
        }
        let wanted = self.wanted_in_hand(ctx);
        let Some(stage) = self.bag.gear.stage else {
            let in_hand = self.bag.equipped.as_ref().map(|equipped| equipped.piece);
            let hitting = ctx.world.get::<Fighter>(self.avatar).is_none_or(|fighter| fighter.busy());
            if wanted == in_hand || cut_short || hitting {
                return;
            }
            return match in_hand {
                Some(piece) => self.start_putting(ctx, piece),
                None => self.start_taking(ctx, wanted),
            };
        };
        if cut_short {
            return self.finish_at_once(ctx);
        }
        let Some(place) = self.bag.gear.place_of(stage.piece()) else {
            return self.finish_at_once(ctx);
        };
        let state = ctx.world.get::<Animator>(self.avatar).map(|animator| {
            (animator.overlay_time(), animator.overlay_finished(), animator.overlay_current().map(str::to_owned))
        });
        let Some((time, finished, playing)) = state else { return };
        // A thing drawn with both hands is measured over still hips, so the upper body leaves the
        // sway of the clip underneath while the thing is off the body, and takes it back by the end.
        if let Some(pass) = &place.pass {
            let length = place.release + place.grab;
            let steady = match stage {
                Stage::Taking { .. } => ((length - time) / (length - pass.draw).max(1e-3)).clamp(0.0, 1.0),
                Stage::Putting { .. } => (time / pass.stow.max(1e-3)).clamp(0.0, 1.0),
            };
            if let Some(animator) = ctx.world.get_mut::<Animator>(self.avatar) {
                animator.set_overlay_steady(steady * steady * (3.0 - 2.0 * steady));
            }
        }
        match stage {
            Stage::Putting { piece, let_go } => {
                // What let go of the upper body when the thing left the hand doesn't count.
                if playing.as_deref() != Some(place.stow().as_str()) {
                    return self.play_change(ctx, &place.stow(), &place.hand, place.pass.is_some());
                }
                if !let_go && wanted == Some(piece) {
                    // Changed its mind in time.
                    self.equip(ctx, piece);
                    return self.end_change(ctx);
                }
                // A thing that another hand puts back goes to that hand first.
                if let (false, Some(pass), Some(carried)) = (let_go, &place.pass, self.bag.gear.of(piece).cloned())
                    && time >= pass.stow
                    && Self::joint_of(ctx, carried.entity).as_deref() != Some(pass.hand.as_str())
                {
                    self.hold_as_it_is(ctx, carried.entity, &pass.hand);
                }
                if !let_go && time >= place.release {
                    self.send_to_body(ctx, piece, true);
                    self.bag.gear.stage = Some(Stage::Putting { piece, let_go: true });
                }
                if finished {
                    self.end_change(ctx);
                    self.start_taking(ctx, wanted);
                }
            }
            Stage::Taking { piece, got } => {
                if playing.as_deref() != Some(place.draw().as_str()) {
                    return self.play_change(ctx, &place.draw(), &place.hand, place.pass.is_some());
                }
                if !got && wanted != Some(piece) {
                    self.end_change(ctx);
                    return self.start_taking(ctx, wanted);
                }
                if let (true, Some(pass), Some(carried)) = (got, &place.pass, self.bag.gear.of(piece).cloned())
                    && time >= pass.draw
                    && Self::joint_of(ctx, carried.entity).as_deref() == Some(pass.hand.as_str())
                {
                    self.send_to_hand(ctx, piece, true);
                }
                if !got && time >= place.grab {
                    match place.pass.as_ref().map(|pass| pass.hand.clone()) {
                        Some(hand) => {
                            if let Some(carried) = self.bag.gear.of(piece).cloned() {
                                self.hold_as_it_is(ctx, carried.entity, &hand);
                            }
                        }
                        None => self.send_to_hand(ctx, piece, true),
                    }
                    self.bag.gear.stage = Some(Stage::Taking { piece, got: true });
                    match self.draw_sound(ctx, piece) {
                        Some(sound) => ctx.audio.play_pitched(sound, 0.8, 1.0),
                        None => ctx.audio.play_pitched(crate::STEP_SOUND, 0.35, 1.3),
                    }
                }
                // A blow or a guard asked for once it's in the hand doesn't wait for the clip.
                let asks = ctx.input.action_just_pressed(actions::ATTACK) || ctx.input.action(actions::GUARD);
                let is_weapon = self.bag.inventory.piece(piece).is_some_and(|piece| {
                    let item = ctx.world.items.get(&piece.stack.item);
                    matches!(item.map(|item| &item.kind), Some(ItemKind::Weapon { .. }))
                });
                if finished || (got && asks && is_weapon) {
                    self.equip(ctx, piece);
                    self.end_change(ctx);
                }
            }
        }
    }

    /// A blade rings as it leaves the sheath: a short one quick and thin, a long one longer.
    fn draw_sound(&self, ctx: &Context, piece: u32) -> Option<&'static str> {
        let piece = self.bag.inventory.piece(piece)?;
        let item = ctx.world.items.get(&piece.stack.item)?;
        let ItemKind::Weapon { weapon, .. } = &item.kind else { return None };
        weapon.blade.as_ref()?;
        Some(if short_blade(item) { DRAW_KNIFE_SOUND } else { DRAW_SWORD_SOUND })
    }

    fn start_putting(&mut self, ctx: &mut Context, piece: u32) {
        self.stop_action(ctx);
        self.bag.equipped = None;
        self.set_blows(ctx, None);
        let Some(place) = self.bag.gear.place_of(piece) else {
            // No place on the body: it goes straight to the bag.
            self.send_to_body(ctx, piece, false);
            let wanted = self.wanted_in_hand(ctx);
            return self.start_taking(ctx, wanted);
        };
        self.bag.gear.stage = Some(Stage::Putting { piece, let_go: false });
        self.play_change(ctx, &place.stow(), &place.hand, place.pass.is_some());
        ctx.audio.play_pitched(crate::STEP_SOUND, 0.3, 1.1);
    }

    fn start_taking(&mut self, ctx: &mut Context, wanted: Option<u32>) {
        let Some(piece) = wanted else { return };
        self.bag.gear.stage = Some(Stage::Taking { piece, got: false });
        match self.bag.gear.place_of(piece) {
            Some(place) => self.play_change(ctx, &place.draw(), &place.hand, place.pass.is_some()),
            None => self.finish_at_once(ctx),
        }
    }

    fn stop_action(&mut self, ctx: &mut Context) {
        if self.bag.gear.action.take().is_some()
            && let Some(animator) = ctx.world.get_mut::<Animator>(self.avatar)
        {
            animator.overlay_stop(ACTION_FADE);
            animator.overlay_leave(None);
        }
    }

    fn start_action(&mut self, ctx: &mut Context, action: Action) {
        if let Some(animator) = ctx.world.get_mut::<Animator>(self.avatar) {
            match action {
                Action::Offering => animator.overlay_loop(OFFER_CLIP, crate::UPPER_BODY, ACTION_FADE),
                Action::Eating { .. } => animator.overlay_once(EAT_CLIP, crate::UPPER_BODY, ACTION_FADE),
            };
            animator.overlay_leave(Some("clavicle.L"));
        }
        self.bag.gear.action = Some(action);
    }

    /// With something plain in the hand: the second button offers it, the first eats it, if it's food.
    fn run_action(&mut self, ctx: &mut Context, cut_short: bool) {
        let plain = self.bag.equipped.as_ref().filter(|equipped| equipped.kind == HeldKind::Thing);
        let Some((piece, id, held)) = plain.map(|e| (e.piece, e.item.clone(), e.held)) else {
            return self.stop_action(ctx);
        };
        if cut_short || self.bag.gear.stage.is_some() {
            return self.stop_action(ctx);
        }
        let catalog = ctx.world.items.clone();
        match self.bag.gear.action {
            Some(Action::Eating { bitten }) => {
                let bite = self.bag.gear.book.as_ref().map_or(0.6, |book| book.eat_bite);
                let state = ctx.world.get::<Animator>(self.avatar);
                let Some((time, finished)) = state.map(|a| (a.overlay_time(), a.overlay_finished())) else { return };
                if !bitten && time >= bite && ctx.world.consume(self.avatar, &id) {
                    self.bag.gear.action = Some(Action::Eating { bitten: true });
                    self.bag.inventory.take(piece, 1);
                    ctx.audio.play_pitched(crate::STEP_SOUND, 0.6, 0.7);
                    let name = catalog.get(&id).map_or(id.as_str(), |item| item.name.as_str());
                    self.hud.notify(ctx, format!("Você come: {name}"), true);
                    // The last one is gone from the hand, and the arm still has to come down.
                    if self.bag.inventory.piece(piece).is_none() {
                        set_visible(ctx, held, false);
                    }
                }
                if finished {
                    self.stop_action(ctx);
                }
            }
            Some(Action::Offering) => {
                if !ctx.input.action(actions::GUARD) && !self.bag.gear.offering_for_a_capture {
                    self.stop_action(ctx);
                }
            }
            None if self.mode == crate::Mode::Play || self.bag.gear.offering_for_a_capture => {
                let edible = matches!(catalog.get(&id).map(|item| &item.kind), Some(ItemKind::Consumable { .. }));
                if edible && ctx.input.action_just_pressed(actions::ATTACK) {
                    self.start_action(ctx, Action::Eating { bitten: false });
                } else if ctx.input.action(actions::GUARD) || self.bag.gear.offering_for_a_capture {
                    self.start_action(ctx, Action::Offering);
                }
            }
            None => {}
        }
    }

    /// What the fingers and the wrist do this frame.
    fn hold_hand(&mut self, ctx: &mut Context) {
        let catalog = ctx.world.items.clone();
        let stage = self.bag.gear.stage;
        // What decides the fingers is what's in the hand, or on its way there.
        let piece = match stage {
            Some(Stage::Putting { let_go: true, .. }) => None,
            Some(stage) => Some(stage.piece()),
            None => self.bag.equipped.as_ref().map(|equipped| equipped.piece),
        };
        let carried = piece.and_then(|piece| self.bag.gear.of(piece)).cloned();
        let item = carried.as_ref().and_then(|carried| catalog.get(&carried.item));
        let loose = item.and_then(|item| {
            let mesh = ctx.world.item_mesh(&item.id)?;
            fingers_for(item, ctx.world.mesh(mesh)?)
        });
        // The book is in the other hand, and this one doesn't care about it.
        let left = item.is_some_and(|item| item.hold_joint() == "hand.L");
        let close_at = match stage {
            Some(Stage::Taking { got: false, .. }) => {
                carried.and_then(|carried| self.bag.gear.place(carried.place.as_deref()?)).map(|place| place.grab)
            }
            _ => None,
        };
        let busy = self.bag.gear.busy();
        let offering = self.bag.gear.action == Some(Action::Offering);
        let loose = if offering { Some(OFFERING_OPEN) } else { loose };
        let Some(animator) = ctx.world.get_mut::<Animator>(self.avatar) else { return };
        let grasping = close_at.is_some_and(|grab| animator.overlay_time() >= grab - CLOSE_AHEAD);
        animator.set_hand_loose(if left { None } else { loose });
        animator.set_hand_grasping(grasping && !left);
        // The clips of the change have the arm and wrist they want.
        if busy {
            animator.set_hand_placed(true);
        }
    }
}

fn set_visible(ctx: &mut Context, entity: Entity, visible: bool) {
    if let Some(attached) = ctx.world.get_mut::<Attached>(entity) {
        attached.hidden = !visible;
    }
    if let Some(renderable) = ctx.world.get_mut::<Renderable>(entity) {
        renderable.visible = visible;
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::DEFAULT_SCENE;

    const DT: f32 = 1.0 / 60.0;
    /// Long enough for any clip of a change, both ways.
    const A_CHANGE: u32 = 150;

    fn start(name: &str) -> (Sandbox, Context, PathBuf) {
        let dir = std::env::temp_dir().join(format!("grimora_gear_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let config = EngineConfig {
            scene: Some(DEFAULT_SCENE.into()),
            save_dir: Some(dir.clone()),
            load: Some("none".into()),
            escape_quits: false,
            ..Default::default()
        };
        let mut game = Sandbox::new();
        let mut ctx = Context::headless(config, &mut game).unwrap();
        frames(&mut game, &mut ctx, 30);
        ctx.world.npcs_ignore_player = true;
        (game, ctx, dir)
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

    /// Puts these in the hotbar, from the first slot on, with nothing else there.
    fn stock(game: &mut Sandbox, ctx: &Context, items: &[&str]) -> Vec<u32> {
        let catalog = ctx.world.items.clone();
        game.bag.inventory = Inventory::new(40, 40);
        let mut pieces = Vec::new();
        for (key, item) in items.iter().enumerate() {
            let before: Vec<u32> = game.bag.inventory.pieces().iter().map(|piece| piece.id).collect();
            assert_eq!(game.bag.inventory.add(&catalog, item, 1), 0, "{item}");
            let added = game.bag.inventory.pieces().iter().find(|piece| !before.contains(&piece.id));
            let id = added.expect("a new piece").id;
            game.bag.inventory.set_shortcut(key, Some(id));
            pieces.push(id);
        }
        pieces
    }

    fn overlay(game: &Sandbox, ctx: &Context) -> Option<String> {
        ctx.world.get::<Animator>(game.avatar).and_then(|animator| animator.overlay_current().map(str::to_owned))
    }

    fn place_of(game: &Sandbox, piece: u32) -> Option<String> {
        game.bag.gear.of(piece).and_then(|carried| carried.place.clone())
    }

    #[test]
    fn an_empty_slot_or_the_same_key_leaves_the_hand_empty() {
        let (mut game, mut ctx, _dir) = start("empty");
        let pieces = stock(&mut game, &ctx, &["espada_reta"]);
        frames(&mut game, &mut ctx, 2);
        press(&mut game, &mut ctx, KeyCode::Digit1);
        assert_eq!(game.bag.inventory.hand(), Some(pieces[0]));
        frames(&mut game, &mut ctx, A_CHANGE);
        assert!(game.bag.armed());
        // A slot with nothing in it.
        press(&mut game, &mut ctx, KeyCode::Digit3);
        assert_eq!(game.bag.inventory.hand(), None);
        frames(&mut game, &mut ctx, A_CHANGE);
        assert!(game.bag.equipped.is_none() && !game.bag.armed());
        assert_eq!(overlay(&game, &ctx), None);
        // Back, and the same key again.
        press(&mut game, &mut ctx, KeyCode::Digit1);
        frames(&mut game, &mut ctx, A_CHANGE);
        assert!(game.bag.armed());
        press(&mut game, &mut ctx, KeyCode::Digit1);
        frames(&mut game, &mut ctx, A_CHANGE);
        assert_eq!(game.bag.inventory.hand(), None);
        assert!(!game.bag.armed());
        // The sword stays on the body, and nothing strikes with an empty hand.
        ctx.input.button_down(MouseButton::Left);
        frames(&mut game, &mut ctx, 2);
        ctx.input.button_up(MouseButton::Left);
        assert!(!ctx.world.get::<Fighter>(game.avatar).unwrap().busy());
        assert!(place_of(&game, pieces[0]).is_some());
    }

    #[test]
    fn whatever_is_in_the_hotbar_shows_on_the_body_each_in_its_own_place() {
        let (mut game, mut ctx, _dir) = start("places");
        for items in [
            ["katana", "katana", "wakizashi", "adaga", "sabre"],
            ["zweihander", "flamberge", "espada_larga", "espada_colossal", "machado_grande"],
            ["pistola", "pistola", "mosquete", "maca", "bala_de_chumbo"],
            ["tonico", "lanterna", "grimorio_de_gelo", "moeda", "maca"],
        ] {
            let pieces = stock(&mut game, &ctx, &items);
            frames(&mut game, &mut ctx, 2);
            let mut places = Vec::new();
            for (piece, item) in pieces.iter().zip(items) {
                let place = place_of(&game, *piece).unwrap_or_else(|| panic!("{item} has no place"));
                let carried = game.bag.gear.of(*piece).unwrap();
                let visible = ctx.world.get::<Renderable>(carried.entity).unwrap().visible;
                // What comes out of a pocket doesn't show, and any number of things go there.
                if place == "pocket" {
                    assert!(!visible, "{item} shows in the pocket");
                    continue;
                }
                assert!(!places.contains(&place), "{item} and another one on {place}");
                assert!(visible, "{item} does not show");
                places.push(place);
            }
        }
    }

    /// Follows a whole change and returns how far the model jumps when it changes joint.
    fn jump_of_a_change(game: &mut Sandbox, ctx: &mut Context, piece: u32) -> f32 {
        let entity = game.bag.gear.of(piece).unwrap().entity;
        let (mut last, mut most, mut moved) = (Vec3::ZERO, 0.0_f32, false);
        for frame in 0..A_CHANGE {
            let joint = ctx.world.get::<Attached>(entity).unwrap().joint.clone();
            frames(game, ctx, 1);
            let now = ctx.world.transform(entity).unwrap().position;
            let changed = ctx.world.get::<Attached>(entity).unwrap().joint != joint;
            if frame > 0 && changed {
                most = most.max((now - last).length());
                moved = true;
            }
            last = now;
        }
        assert!(moved, "it never changed joint");
        most
    }

    #[test]
    fn taking_it_out_and_putting_it_away_plays_the_clips_and_nothing_jumps() {
        let (mut game, mut ctx, _dir) = start("clips");
        for (item, clip) in [
            ("espada_reta", "belt_"),
            ("zweihander", "back_"),
            ("pistola", "holster_"),
            ("mosquete", "sling_"),
            ("maca", "pocket"),
            ("grimorio_de_gelo", "book_"),
        ] {
            let pieces = stock(&mut game, &ctx, &[item]);
            frames(&mut game, &mut ctx, A_CHANGE);
            let place = place_of(&game, pieces[0]).unwrap();
            assert!(place.starts_with(clip), "{item} on {place}");

            press(&mut game, &mut ctx, KeyCode::Digit1);
            assert_eq!(overlay(&game, &ctx), Some(format!("draw_{place}")), "{item}");
            assert!(game.bag.equipped.is_none(), "{item} is out only at the end");
            let jump = jump_of_a_change(&mut game, &mut ctx, pieces[0]);
            println!("{item}: {jump} m into the hand");
            assert!(jump < 0.2, "{item}: jumps {jump} m into the hand");
            assert_eq!(game.bag.equipped.as_ref().map(|e| e.piece), Some(pieces[0]));

            press(&mut game, &mut ctx, KeyCode::Digit1);
            assert_eq!(overlay(&game, &ctx), Some(format!("stow_{place}")), "{item}");
            let jump = jump_of_a_change(&mut game, &mut ctx, pieces[0]);
            println!("{item}: {jump} m out of the hand");
            assert!(jump < 0.2, "{item}: jumps {jump} m out of the hand");
            assert!(game.bag.equipped.is_none());
            assert_eq!(overlay(&game, &ctx), None);
        }
    }

    /// `cargo test --release -p sandbox musket_trace -- --ignored --nocapture`: where the musket and
    /// both hands are through a draw and a stow, as the game plays them.
    #[test]
    #[ignore]
    fn musket_trace() {
        let (mut game, mut ctx, _dir) = start("trace");
        let pieces = stock(&mut game, &ctx, &["mosquete"]);
        frames(&mut game, &mut ctx, A_CHANGE);
        let entity = game.bag.gear.of(pieces[0]).unwrap().entity;
        for key in 0..2 {
            press(&mut game, &mut ctx, KeyCode::Digit1);
            for frame in 0..A_CHANGE {
                frames(&mut game, &mut ctx, 1);
                if frame % 4 != 0 {
                    continue;
                }
                let joint = ctx.world.get::<Attached>(entity).unwrap().joint.clone();
                let gun = ctx.world.transform(entity).unwrap();
                let left = ctx.world.joint_transform(game.avatar, "hand.L").unwrap().position;
                let right = ctx.world.joint_transform(game.avatar, "weapon.R").unwrap().position;
                let body = ctx.world.transform(game.avatar).unwrap();
                let (root, back) = (body.position, body.rotation.inverse());
                let along = gun.rotation * Vec3::X;
                let to_left = left - gun.position;
                let off_left = (to_left - along * to_left.dot(along)).length();
                println!(
                    "{key} {frame:3} {:?} {joint:9} gun {:.2} along {:.2} | L {:.2} off the line {:.0} mm | R {:.2} from gun {:.0} mm",
                    overlay(&game, &ctx),
                    back * (gun.position - root),
                    back * along,
                    back * (left - root),
                    off_left * 1000.0,
                    back * (right - root),
                    (right - gun.position).length() * 1000.0
                );
            }
        }
    }

    #[test]
    fn one_thing_goes_away_and_then_the_other_comes_out() {
        let (mut game, mut ctx, _dir) = start("swap");
        let pieces = stock(&mut game, &ctx, &["espada_reta", "katana"]);
        press(&mut game, &mut ctx, KeyCode::Digit1);
        frames(&mut game, &mut ctx, A_CHANGE);
        press(&mut game, &mut ctx, KeyCode::Digit2);
        let (first, second) = (place_of(&game, pieces[0]).unwrap(), place_of(&game, pieces[1]).unwrap());
        assert_eq!(overlay(&game, &ctx), Some(format!("stow_{first}")));
        let mut saw_draw = false;
        for _ in 0..A_CHANGE {
            frames(&mut game, &mut ctx, 1);
            saw_draw |= overlay(&game, &ctx) == Some(format!("draw_{second}"));
        }
        assert!(saw_draw);
        assert_eq!(game.bag.equipped.as_ref().map(|e| e.piece), Some(pieces[1]));
        assert!(ctx.world.get::<Fighter>(game.avatar).unwrap().weapon.blade.is_some(), "the blows of the katana");
    }

    #[test]
    fn a_roll_cuts_a_change_short_and_it_ends_where_it_was_going() {
        let (mut game, mut ctx, _dir) = start("roll");
        let pieces = stock(&mut game, &ctx, &["espada_reta"]);
        frames(&mut game, &mut ctx, A_CHANGE);
        press(&mut game, &mut ctx, KeyCode::Digit1);
        frames(&mut game, &mut ctx, 3);
        assert!(game.bag.gear.busy());
        press(&mut game, &mut ctx, KeyCode::ControlLeft);
        assert!(game.dodge.is_some(), "it rolled");
        assert!(!game.bag.gear.busy());
        assert_eq!(game.bag.equipped.as_ref().map(|e| e.piece), Some(pieces[0]));
    }

    #[test]
    fn an_apple_is_held_offered_and_eaten_and_a_bullet_only_offered() {
        let (mut game, mut ctx, _dir) = start("apple");
        let pieces = stock(&mut game, &ctx, &["maca", "bala_de_chumbo"]);
        frames(&mut game, &mut ctx, 2);
        press(&mut game, &mut ctx, KeyCode::Digit1);
        frames(&mut game, &mut ctx, A_CHANGE);
        assert_eq!(game.bag.equipped.as_ref().map(|e| e.kind), Some(HeldKind::Thing));
        assert!(!game.bag.armed());

        ctx.input.button_down(MouseButton::Right);
        frames(&mut game, &mut ctx, 30);
        assert_eq!(overlay(&game, &ctx).as_deref(), Some(OFFER_CLIP));
        assert!(!game.player.as_ref().unwrap().aiming, "the body underneath goes on as it was");
        ctx.input.button_up(MouseButton::Right);
        frames(&mut game, &mut ctx, 30);
        assert_eq!(overlay(&game, &ctx), None);

        // Some health to get back, which comes at the bite and not before.
        ctx.world.get_mut::<Health>(game.avatar).unwrap().current = 10.0;
        ctx.input.button_down(MouseButton::Left);
        frames(&mut game, &mut ctx, 1);
        ctx.input.button_up(MouseButton::Left);
        frames(&mut game, &mut ctx, 5);
        assert_eq!(overlay(&game, &ctx).as_deref(), Some(EAT_CLIP));
        assert_eq!(ctx.world.get::<Health>(game.avatar).unwrap().current, 10.0, "before the bite");
        frames(&mut game, &mut ctx, 200);
        assert!(ctx.world.get::<Health>(game.avatar).unwrap().current > 10.0);
        assert!(game.bag.inventory.piece(pieces[0]).is_none(), "it was the only one");
        assert_eq!(game.bag.inventory.hand(), None);
        assert!(game.bag.equipped.is_none());
        assert_eq!(overlay(&game, &ctx), None);

        press(&mut game, &mut ctx, KeyCode::Digit2);
        frames(&mut game, &mut ctx, A_CHANGE);
        assert_eq!(game.bag.equipped.as_ref().map(|e| e.piece), Some(pieces[1]));
        ctx.input.button_down(MouseButton::Left);
        frames(&mut game, &mut ctx, 2);
        ctx.input.button_up(MouseButton::Left);
        assert_eq!(overlay(&game, &ctx), None, "a bullet is not food");
        ctx.input.button_down(MouseButton::Right);
        frames(&mut game, &mut ctx, 5);
        assert_eq!(overlay(&game, &ctx).as_deref(), Some(OFFER_CLIP));
        ctx.input.button_up(MouseButton::Right);
    }

    /// How it really lies now, in the world.
    fn segment(ctx: &mut Context, entity: Entity, id: &str) -> Segment {
        let mesh = ctx.world.item_mesh(id).unwrap();
        let (from, to) = ends_of(ctx.world.mesh(mesh).unwrap());
        let matrix = ctx.world.transform(entity).unwrap().matrix();
        (matrix.transform_point3(from), matrix.transform_point3(to))
    }

    #[test]
    fn long_things_on_the_body_do_not_go_through_each_other() {
        let (mut game, mut ctx, _dir) = start("apart");
        const THINGS: [&str; 11] = [
            "katana",
            "katana",
            "wakizashi",
            "adaga",
            "espada_reta",
            "zweihander",
            "espada_colossal",
            "machado_grande",
            "mosquete",
            "pistola",
            "pistola",
        ];
        let (mut worst, mut sets) = ((f32::MAX, String::new()), 0);
        for bits in 0u32..1 << THINGS.len() {
            if bits.count_ones() != 5 {
                continue;
            }
            let items: Vec<&str> = (0..THINGS.len()).filter(|i| bits & 1 << i != 0).map(|i| THINGS[i]).collect();
            let pieces = stock(&mut game, &ctx, &items);
            // Every piece got its place in the update, and the next one moves them there.
            frames(&mut game, &mut ctx, 2);
            let shapes: Vec<_> = pieces
                .iter()
                .zip(&items)
                .map(|(piece, id)| (*id, segment(&mut ctx, game.bag.gear.of(*piece).unwrap().entity, id)))
                .collect();
            for (at, (first, a)) in shapes.iter().enumerate() {
                for (second, b) in &shapes[at + 1..] {
                    let gap = apart(*a, *b);
                    if gap < worst.0 {
                        worst = (gap, format!("{first} and {second}, of {items:?}"));
                    }
                }
            }
            sets += 1;
            game.bag.inventory = Inventory::new(40, 40);
            frames(&mut game, &mut ctx, 2);
        }
        println!("{sets} sets: at worst {:.0} mm, {}", worst.0 * 1000.0, worst.1);
        assert!(worst.0 > 0.015, "{} are {} m apart", worst.1, worst.0);
    }

    #[test]
    fn what_is_too_big_is_held_at_a_size_a_hand_can_take() {
        let (_game, mut ctx, _dir) = start("size");
        let catalog = ctx.world.items.clone();
        for id in ["lanterna", "maca", "bala_de_chumbo", "moeda", "tonico"] {
            let item = catalog.get(id).unwrap();
            let mesh = ctx.world.item_mesh(id).unwrap();
            let data = ctx.world.mesh(mesh).unwrap();
            let (least, most) = bounds(data);
            let wide = (most - least).max_element() * hold_of(item, data).scale.x;
            assert!(wide < LARGEST_THING + 1e-3, "{id} is {wide} m");
        }
    }
}
