//! The grimoire in the free hand: holding it open, the wheel to choose a spell, and the three
//! spells of ice. The book, the poses and the effects are files; what each spell does is here.
//!
//! Every piece of ice in the world is an [`IcePiece`] that rises, stands and melts or breaks by
//! itself, and a [`Shard`] is one that flies off: there are no mesh particles in the engine.

use std::path::Path;

use grimora::Attached;
use grimora::core::MeshData;
use grimora::prelude::*;
use grimora::render::ImageId;
use grimora::scene::{MeshDesc, Rng, SpellDesc, load_effect, wrap_in_texture};

use crate::{Mode, PLAYER_TEAM, Sandbox, UPPER_BODY};

pub const GRIMOIRE: &str = "grimorio_de_gelo";
const SPELL_WHEEL: &str = "spell_wheel";
const CAST_SPELL: &str = "cast_spell";

const SPEAR: &str = "lanca_de_gelo";
const STOMP: &str = "pisao_de_gelo";
const BLAST: &str = "explosao_de_gelo";

const HOLD_CLIP: &str = "book_hold";
const AIM_CLIP: &str = "spell_aim";
const THROW_CLIP: &str = "spell_throw";
const STOMP_CLIP: &str = "spell_stomp";
const BLAST_CLIP: &str = "spell_blast";

macro_rules! effect {
    ($name:literal) => {
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/effects/", $name, ".ron")
    };
}
macro_rules! sound {
    ($name:literal) => {
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sounds/", $name, ".wav")
    };
}

// ---- holding the book
/// Seconds after taking the book before its pages light up: it has to open first.
const BOOK_GLOW_AFTER: f32 = 0.75;
/// Over the middle of the open book, in its own space.
const BOOK_PAGES: Vec3 = Vec3::new(-0.085, 0.05, 0.0);
const POSE_FADE: f32 = 0.22;

// ---- the wheel
/// How fast the world goes with the wheel open.
const WHEEL_TIME: f32 = 0.22;
const WHEEL_SIZE: f32 = 440.0;
/// Texels across the wheel's pictures, which show as big square pixels.
const WHEEL_TEXELS: u32 = 176;
/// Where the ring is and where the spells sit on it, from the center to the edge.
const RING_INNER: f32 = 0.36;
const RING_OUTER: f32 = 0.93;
const RING_MIDDLE: f32 = 0.645;
/// How far the mouse has to go, in pixels, to reach the edge of the wheel's aim.
const WHEEL_REACH: f32 = 90.0;
/// Under this share of the reach, no spell is pointed at.
const WHEEL_DEAD: f32 = 0.22;

// ---- ice spear
const SPEAR_SPEED: f32 = 36.0;
/// Meters it flies dead straight, before starting to fall.
const SPEAR_STRAIGHT: f32 = 42.0;
/// Meters over which gravity takes hold after that, so that the arc doesn't start with a kink.
const SPEAR_DROP_IN: f32 = 18.0;
const SPEAR_GRAVITY: f32 = 11.0;
const SPEAR_SECONDS: f32 = 8.0;
const SPEAR_DAMAGE: f32 = 26.0;
/// How far from where the spear lands anything small is dead.
const SPEAR_KILLS: f32 = 1.0;
const SPEAR_COOLDOWN: f32 = 0.7;
/// Seconds the spear takes to grow in the hand; it can't be thrown before most of that.
const SPEAR_FORMS: f32 = 0.42;
const SPEAR_GATHERS: f32 = 0.16;
/// Into the throw, when it leaves the hand.
const THROW_RELEASE: f32 = 0.2;
/// As far as the crosshair looks for what it's on.
const AIM_RANGE: f32 = 220.0;
/// Seconds of the camera staying at the shoulder after a throw.
const AIM_LINGER: f32 = 0.5;

// ---- ice stomp
const STOMP_REACH: f32 = 9.5;
/// Half the cone's opening, in degrees.
const STOMP_ANGLE: f32 = 26.0;
const STOMP_SPEED: f32 = 15.0;
const STOMP_DAMAGE: f32 = 18.0;
const STOMP_COOLDOWN: f32 = 2.2;
/// Meters between rows of ice.
const STOMP_ROW: f32 = 0.85;

// ---- ice blast
const BLAST_RADIUS: f32 = 11.0;
const BLAST_SPEED: f32 = 17.0;
const BLAST_COOLDOWN: f32 = 6.0;
pub(crate) const FREEZE_SECONDS: f32 = 7.0;
/// Pieces of ice scattered over the area, beyond the ring around the character.
const BLAST_SCATTER: usize = 54;

// ---- ice in the world
/// How bright crystals are inside; see `crystal_glow`.
const ICE_GLOW: f32 = 0.85;
/// Their color. The heart of the circle is violet; this is the cold one.
const ICE_COLOR: Vec3 = Vec3::new(0.7, 0.9, 1.0);
const ICE_TEXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/textures/gelo1.png");
const ICE_ROUGHNESS: f32 = 0.3;
/// How much of each shape, in its own units, one copy of the picture covers.
const SPEAR_TILE: f32 = 0.55;
const CRYSTAL_TILE: f32 = 1.1;
const SHARD_TILE: f32 = 0.8;
const SHARD_GRAVITY: f32 = 11.0;
const CRYSTAL_SHAPES: u64 = 4;
const SHARD_SHAPES: u64 = 4;
const MAX_SHARDS: usize = 90;

const DIM: Vec4 = Vec4::new(0.72, 0.82, 0.92, 0.9);

/// What the character is doing with the book.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
enum Cast {
    #[default]
    Idle,
    /// The spear in the hand, waiting for a click.
    Aiming { spear: Entity, held_for: f32 },
    /// Throwing it; the entity is there until it leaves the hand.
    Throwing { spear: Option<Entity>, for_: f32 },
    /// A whole-body clip that says when it lands, by an event.
    Clip(&'static str),
}

/// A piece of ice that grows out of something and goes away by itself.
struct IcePiece {
    entity: Entity,
    /// Seconds before it starts to grow; it's small and out of sight until then.
    wait: f32,
    age: f32,
    rise: f32,
    stand: f32,
    /// What happens at the end: how long it takes, and whether it breaks instead of melting.
    end: f32,
    breaks: bool,
    broke: bool,
    /// When this dies, there's nothing left for the ice to hold.
    holds: Option<Entity>,
    place: Transform,
}

impl Spells {
    /// Moves what the spells keep in the world by `offset`, as the world's origin moves the
    /// other way.
    pub fn shift(&mut self, offset: Vec3) {
        for piece in &mut self.pieces {
            piece.place.position += offset;
        }
        for shard in &mut self.shards {
            shard.floor += offset.y;
        }
        for spear in &mut self.flying {
            spear.position += offset;
        }
        for spike in &mut self.spikes {
            spike.position += offset;
        }
    }
}

/// A splinter in the air.
struct Shard {
    entity: Entity,
    velocity: Vec3,
    spin: Vec3,
    age: f32,
    life: f32,
    floor: f32,
    size: f32,
}

/// A spear on its way.
struct Flying {
    entity: Entity,
    trail: Option<Entity>,
    velocity: Vec3,
    travelled: f32,
    age: f32,
    /// Where it was last frame: the spear's position is at the hand's place, not its tip.
    position: Vec3,
}

/// What a spell does to something once its wave reaches it.
#[derive(Debug, Clone, Copy)]
enum Reach {
    Hurt(f32),
    Freeze,
}

/// A row of the stomp, waiting for the wave to get there.
struct Spike {
    wait: f32,
    position: Vec3,
    outwards: Vec3,
    size: f32,
}

struct Smoke {
    entity: Entity,
    over: Entity,
    left: f32,
}

#[derive(Default)]
struct Effects {
    book: Option<ParticleEffect>,
    form: Option<ParticleEffect>,
    trail: Option<ParticleEffect>,
    shatter: Option<ParticleEffect>,
    stomp: Option<ParticleEffect>,
    spike: Option<ParticleEffect>,
    frost: Option<ParticleEffect>,
    ring: Option<ParticleEffect>,
    blast: Option<ParticleEffect>,
    smoke: Option<ParticleEffect>,
    breaking: Option<ParticleEffect>,
}

fn effect(path: &str) -> Option<ParticleEffect> {
    load_effect(path).map_err(|err| eprintln!("[sandbox] {err:#}")).ok()
}

/// The wheel, while the key is held.
struct Wheel {
    /// Where the mouse has moved to, from -1 to 1 on each side.
    aim: Vec2,
    pointed: Option<usize>,
    age: f32,
    world_was: f32,
}

#[derive(Default)]
struct WheelPictures {
    ring: Option<ImageId>,
    lit: Vec<Option<ImageId>>,
    icons: Vec<Option<ImageId>>,
    book: Option<ImageId>,
    made: bool,
}

#[derive(Default)]
struct Meshes {
    spear: Option<MeshId>,
    crystals: Vec<MeshId>,
    shards: Vec<MeshId>,
}

#[derive(Default)]
pub struct Spells {
    selected: usize,
    cast: Cast,
    /// Seconds until each spell can be cast again.
    cooldowns: Vec<f32>,
    wheel: Option<Wheel>,
    pictures: WheelPictures,
    /// The book in the hand, as seen last frame, and for how long.
    book: Option<Entity>,
    book_for: f32,
    glow: Option<Entity>,
    /// How long the camera stays at the shoulder.
    aim_left: f32,
    effects: Effects,
    meshes: Meshes,
    pieces: Vec<IcePiece>,
    shards: Vec<Shard>,
    flying: Vec<Flying>,
    spikes: Vec<Spike>,
    /// What a wave is on its way to, and in how many seconds it arrives.
    reaching: Vec<(f32, Entity, Reach)>,
    smokes: Vec<Smoke>,
    seed: u64,
    /// A `--menu spell:<what>` run: what to do, and on which frame to take the book.
    pub scheduled: Option<(String, u64)>,
    scripted: Option<String>,
}

impl Spells {
    /// Everything in the world went with the old one.
    pub fn forget_world(&mut self) {
        let keep = Self {
            selected: self.selected,
            cooldowns: std::mem::take(&mut self.cooldowns),
            pictures: std::mem::take(&mut self.pictures),
            scheduled: self.scheduled.take(),
            scripted: self.scripted.take(),
            seed: self.seed,
            ..Default::default()
        };
        *self = keep;
    }

    fn rng(&mut self) -> Rng {
        self.seed = self.seed.wrapping_add(1);
        Rng::new(self.seed.wrapping_mul(0x9E37_79B9) ^ 0x51)
    }
}

/// The controls of what's in this file, added to the ones of `menu.rs`.
pub fn add_controls(controls: &mut ActionMap) {
    controls.add(SPELL_WHEEL, [Binding::Key(KeyCode::KeyG), Binding::Pad(PadButton::DPadDown)]);
    controls.add(CAST_SPELL, [Binding::Key(KeyCode::KeyC), Binding::Pad(PadButton::RightStick)]);
}

/// The names of those controls for the menu.
pub const CONTROL_NAMES: [(&str, &str); 2] = [(SPELL_WHEEL, "Roda de feitiços"), (CAST_SPELL, "Lançar feitiço")];

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Goes a bit past 1 and settles: ice shooting up.
fn overshoot(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0) - 1.0;
    1.0 + t * t * (2.2 * t + 1.2)
}

fn spell_at(spells: &[SpellDesc], at: usize) -> Option<&SpellDesc> {
    spells.get(at)
}

/// Nearest distance between two segments, and how far along the first.
fn segments(a0: Vec3, a1: Vec3, b0: Vec3, b1: Vec3) -> (f32, f32) {
    let (d1, d2, r) = (a1 - a0, b1 - b0, a0 - b0);
    let (a, e, f) = (d1.length_squared(), d2.length_squared(), d2.dot(r));
    let (c, b) = (d1.dot(r), d1.dot(d2));
    let denom = a * e - b * b;
    let mut s = if denom > 1e-6 { ((b * f - c * e) / denom).clamp(0.0, 1.0) } else { 0.0 };
    let t = (b * s + f) / e.max(1e-6);
    let t = t.clamp(0.0, 1.0);
    if a > 1e-6 {
        s = ((b * t - c) / a).clamp(0.0, 1.0);
    }
    ((a0 + d1 * s - (b0 + d2 * t)).length(), s)
}

/// Draws the ring of the wheel, or one lit part of it, as a picture.
fn wheel_picture(count: usize, lit: Option<usize>) -> Vec<u8> {
    let size = WHEEL_TEXELS as usize;
    let mut rgba = vec![0u8; size * size * 4];
    let turn = std::f32::consts::TAU;
    let part = turn / count.max(1) as f32;
    for y in 0..size {
        for x in 0..size {
            let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5) / WHEEL_TEXELS as f32 * 2.0 - Vec2::ONE;
            let r = p.length();
            // From the top, clockwise, with the first part centered there.
            let angle = (p.y.atan2(p.x) + turn * 0.25 + part * 0.5).rem_euclid(turn);
            let index = (angle / part) as usize;
            let across = (angle - (index as f32 + 0.5) * part).abs();
            let texel = 2.0 / WHEEL_TEXELS as f32;
            // A gap between the parts, the same width all the way along.
            let to_gap = (part * 0.5 - across) * r;
            // Thorns of frost along the outer edge, a different height for each.
            let sector = (angle / (turn / 60.0)) as i32;
            let hash = ((sector.wrapping_mul(7919) ^ 0x2F) % 97) as f32 / 97.0;
            let within = (angle / (turn / 60.0)).fract();
            let thorn = (1.0 - (within - 0.5).abs() * 2.0) * (0.015 + 0.05 * hash * hash);
            let outer = RING_OUTER + thorn;
            let in_ring = r > RING_INNER && r < outer && to_gap > texel * 1.2;
            let dither = ((x + y) % 2 == 0) as u8 as f32;
            let depth = ((r - RING_INNER) / (RING_OUTER - RING_INNER)).clamp(0.0, 1.0);
            let edge = r > RING_OUTER - texel * 1.2 || r < RING_INNER + texel * 1.2 || to_gap < texel * 2.2;
            let mut out = None;
            if lit.is_none_or(|lit| lit == index) && in_ring {
                let color = match lit {
                    None if edge => Vec4::new(0.42, 0.66, 0.92, 0.95),
                    None => {
                        let shade = depth + dither * 0.14;
                        let band = (shade * 5.0).floor() / 5.0;
                        Vec4::new(0.02, 0.05, 0.13, 0.84).lerp(Vec4::new(0.05, 0.13, 0.28, 0.88), band)
                    }
                    Some(_) if edge => Vec4::new(0.9, 0.98, 1.0, 1.0),
                    Some(_) => {
                        let shade = 1.0 - (depth - 0.55).abs() * 1.2 + dither * 0.12;
                        let band = (shade.clamp(0.0, 1.0) * 5.0).floor() / 5.0;
                        Vec4::new(0.04, 0.16, 0.38, 0.62).lerp(Vec4::new(0.16, 0.42, 0.7, 0.74), band)
                    }
                };
                out = Some(color);
            } else if lit.is_none() && r < RING_INNER - texel * 3.0 && r > RING_INNER - texel * 5.0 {
                out = Some(Vec4::new(0.3, 0.5, 0.75, 0.8));
            } else if lit.is_none() && r <= RING_INNER - texel * 5.0 {
                out = Some(Vec4::new(0.02, 0.04, 0.1, 0.9));
            }
            if let Some(color) = out {
                let at = (y * size + x) * 4;
                for (channel, value) in color.to_array().into_iter().enumerate() {
                    rgba[at + channel] = (value * 255.0) as u8;
                }
            }
        }
    }
    rgba
}

impl Sandbox {
    fn grimoire_spells(&self, ctx: &Context) -> Vec<SpellDesc> {
        self.bag
            .book_item()
            .and_then(|id| ctx.world.items.get(id).map(|item| item.spells().to_vec()))
            .unwrap_or_default()
    }

    fn load_spells(&mut self, ctx: &mut Context) {
        if self.spells.meshes.spear.is_some() {
            return;
        }
        self.spells.effects = Effects {
            book: effect(effect!("book_glow")),
            form: effect(effect!("ice_form")),
            trail: effect(effect!("ice_trail")),
            shatter: effect(effect!("ice_shatter")),
            stomp: effect(effect!("ice_stomp")),
            spike: effect(effect!("ice_spike")),
            frost: effect(effect!("ice_frost")),
            ring: effect(effect!("ice_ring")),
            blast: effect(effect!("ice_blast")),
            smoke: effect(effect!("ice_smoke")),
            breaking: effect(effect!("ice_break")),
        };
        let mut add = |desc: MeshDesc, tile: f32| ctx.world.add_mesh(ice_mesh(desc, tile));
        self.spells.meshes = Meshes {
            spear: Some(add(MeshDesc::IceSpear, SPEAR_TILE)),
            crystals: (0..CRYSTAL_SHAPES).map(|seed| add(MeshDesc::Crystal { seed }, CRYSTAL_TILE)).collect(),
            shards: (0..SHARD_SHAPES).map(|seed| add(MeshDesc::IceShard { seed }, SHARD_TILE)).collect(),
        };
    }

    /// Takes the wheel's key and mouse. Returns whether it's open, when the character doesn't
    /// take orders.
    pub(crate) fn run_wheel(&mut self, ctx: &mut Context, bag_open: bool) -> bool {
        let spells = self.grimoire_spells(ctx);
        let can = !bag_open && self.mode == Mode::Play && self.dead_for.is_none() && !spells.is_empty();
        let scripted = self.spells.scripted.as_deref() == Some("wheel");
        if self.spells.wheel.is_none() {
            let asks = ctx.input.action_just_pressed(SPELL_WHEEL) && self.spells.cast == Cast::Idle;
            if !(can && asks) && !(scripted && self.spells.book_for > 0.3) {
                return false;
            }
            self.spells.wheel = Some(Wheel { aim: Vec2::ZERO, pointed: None, age: 0.0, world_was: ctx.time.scale() });
            ctx.time.set_scale(WHEEL_TIME);
            ctx.audio.play_pitched(sound!("ice_tick"), 0.7, 1.0);
        }
        let held = ctx.input.action(SPELL_WHEEL) || scripted;
        let clicked = ctx.input.action_just_pressed(actions::ATTACK);
        let backed_out = ctx.input.action_just_pressed(actions::GUARD) || ctx.input.action_just_pressed(actions::PAUSE);
        let delta = ctx.input.mouse_delta();
        let stick = Vec2::new(ctx.input.pad_axis(PadAxis::RightX), -ctx.input.pad_axis(PadAxis::RightY));
        let dt = ctx.time.delta();
        let Some(wheel) = &mut self.spells.wheel else { return false };
        wheel.age += dt;
        wheel.aim += delta / WHEEL_REACH;
        if stick.length() > 0.3 {
            wheel.aim = stick;
        }
        if scripted {
            wheel.aim = Vec2::new(0.9, 0.5);
        }
        if wheel.aim.length() > 1.0 {
            wheel.aim = wheel.aim.normalize();
        }
        let before = wheel.pointed;
        wheel.pointed = (wheel.aim.length() > WHEEL_DEAD && !spells.is_empty()).then(|| {
            let turn = std::f32::consts::TAU;
            let part = turn / spells.len() as f32;
            let angle = (wheel.aim.y.atan2(wheel.aim.x) + turn * 0.25 + part * 0.5).rem_euclid(turn);
            ((angle / part) as usize).min(spells.len() - 1)
        });
        let pointed = wheel.pointed;
        let world_was = wheel.world_was;
        if pointed.is_some() && pointed != before {
            ctx.audio.play_pitched(sound!("ice_tick"), 0.5, 1.25);
        }
        if can && !backed_out && (clicked || !held) || !can || backed_out {
            if let Some(chosen) = pointed.filter(|_| can && !backed_out) {
                self.spells.selected = chosen;
                ctx.audio.play_pitched(sound!("ice_tick"), 0.8, 0.75);
            }
            self.spells.wheel = None;
            ctx.time.set_scale(world_was);
            // This frame's click was for the wheel.
            return true;
        }
        true
    }

    /// The wheel, on top of everything else this frame.
    pub(crate) fn draw_wheel(&mut self, ctx: &mut Context) {
        let Some(wheel) = &self.spells.wheel else { return };
        let (pointed, age, aim) = (wheel.pointed, wheel.age, wheel.aim);
        let spells = self.grimoire_spells(ctx);
        self.make_pictures(ctx, &spells);
        let appear = smooth(age / 0.14);
        let ui = &mut ctx.ui;
        let screen = ui.screen();
        ui.rect(screen, Vec4::new(0.01, 0.02, 0.06, 0.5 * appear));
        let center = ui.at(Anchor::Center, Vec2::new(0.0, -14.0));
        let size = WHEEL_SIZE * (0.92 + 0.08 * appear);
        let rect = Rect::anchored(center, Vec2::splat(size), Anchor::Center);
        let tint = Vec4::new(1.0, 1.0, 1.0, appear);
        if let Some(ring) = self.spells.pictures.ring {
            ui.image(rect, ring, tint);
        }
        let current = pointed.unwrap_or(self.spells.selected);
        if let Some(Some(lit)) = self.spells.pictures.lit.get(current) {
            let tint = if pointed.is_some() { tint } else { Vec4::new(0.75, 0.8, 0.9, 0.55 * appear) };
            ui.image(rect, *lit, tint);
        }
        let radius = size * 0.5;
        for (at, spell) in spells.iter().enumerate() {
            let angle = at as f32 / spells.len() as f32 * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
            let middle = center + Vec2::new(angle.cos(), angle.sin()) * radius * RING_MIDDLE;
            let on = at == current;
            let icon_size = if on { 80.0 } else { 64.0 };
            let shade = if on { 1.0 } else { 0.72 };
            let icon = Rect::anchored(middle - Vec2::Y * 12.0, Vec2::splat(icon_size), Anchor::Center);
            if let Some(Some(image)) = self.spells.pictures.icons.get(at) {
                ui.image(icon, *image, Vec4::new(shade, shade, shade, appear));
            }
            let color = if on { Vec4::ONE } else { DIM };
            let style = TextStyle::new(16.0)
                .color(color * Vec4::new(1.0, 1.0, 1.0, appear))
                .outline(Vec4::new(0.0, 0.0, 0.05, 0.9 * appear))
                .anchor(Anchor::Top);
            let names = spell.name.rsplit_once(" de ").map(|(first, last)| (first.to_owned(), format!("de {last}")));
            let (first, second) = names.unwrap_or_else(|| (spell.name.clone(), String::new()));
            let under = middle + Vec2::Y * (icon_size * 0.5 - 8.0);
            ui.text(under, &first, &style);
            ui.text(under + Vec2::Y * 16.0, &second, &style);
        }
        // The book in the middle, and a mark for where the mouse points.
        if let Some(book) = self.spells.pictures.book {
            ui.image(Rect::anchored(center, Vec2::splat(64.0), Anchor::Center), book, tint);
        }
        if aim.length() > 0.02 {
            let mark = center + aim.normalize_or_zero() * radius * (RING_INNER - 0.06) * aim.length().min(1.0);
            ui.rect(Rect::anchored(mark, Vec2::splat(5.0), Anchor::Center), Vec4::new(0.85, 0.96, 1.0, appear));
        }
        // What the spell in question does.
        if let Some(spell) = spell_at(&spells, current) {
            let style = TextStyle::new(16.0)
                .color(DIM * Vec4::new(1.0, 1.0, 1.0, appear))
                .outline(Vec4::new(0.0, 0.0, 0.05, 0.9 * appear))
                .anchor(Anchor::Top)
                .wrap(1000.0);
            let at = center + Vec2::Y * (radius + 12.0);
            let width = ui.measure(&spell.description, &style).x + crate::hud::STRIP_FADE * 2.0 + 24.0;
            crate::hud::strip(ui, Rect::anchored(at - Vec2::Y * 4.0, Vec2::new(width, 28.0), Anchor::Top), appear);
            ui.text(at, &spell.description, &style);
        }
    }

    fn make_pictures(&mut self, ctx: &mut Context, spells: &[SpellDesc]) {
        if self.spells.pictures.made {
            return;
        }
        let ui = &mut ctx.ui;
        let catalog = ctx.world.items.clone();
        let side = WHEEL_TEXELS;
        let pictures = &mut self.spells.pictures;
        pictures.made = true;
        pictures.ring = ui.add_image(side, side, &wheel_picture(spells.len(), None));
        pictures.lit =
            (0..spells.len()).map(|at| ui.add_image(side, side, &wheel_picture(spells.len(), Some(at)))).collect();
        let picture = |path: std::path::PathBuf, ui: &mut Ui| {
            let texture = grimora::scene::load_texture(path).map_err(|err| eprintln!("[sandbox] {err:#}")).ok()?;
            ui.add_image(texture.width, texture.height, &texture.rgba)
        };
        pictures.icons = spells.iter().map(|spell| picture(catalog.spell_icon_path(spell), ui)).collect();
        pictures.book = catalog.get(GRIMOIRE).and_then(|item| picture(catalog.icon_path(item), ui));
    }

    /// While the book is out: what's selected, what to press, how long until it's ready, and
    /// the crosshair if there's something to throw.
    fn draw_book_hud(&mut self, ctx: &mut Context, spells: &[SpellDesc]) {
        self.make_pictures(ctx, spells);
        let Some(spell) = spell_at(spells, self.spells.selected) else { return };
        let cooling = self.spells.cooldowns.get(self.spells.selected).copied().unwrap_or(0.0);
        let total = match spell.id.as_str() {
            SPEAR => SPEAR_COOLDOWN,
            STOMP => STOMP_COOLDOWN,
            _ => BLAST_COOLDOWN,
        };
        let wheel_key = crate::hud::first_control(&ctx.input, SPELL_WHEEL).to_uppercase();
        let cast_key = crate::hud::first_control(&ctx.input, CAST_SPELL).to_uppercase();
        let aiming = matches!(self.spells.cast, Cast::Aiming { .. });
        let attack_key = crate::hud::first_control(&ctx.input, actions::ATTACK).to_uppercase();
        let guard_key = crate::hud::first_control(&ctx.input, actions::GUARD).to_uppercase();
        let icon = self.spells.pictures.icons.get(self.spells.selected).copied().flatten();
        let ui = &mut ctx.ui;
        let (first, second) = if aiming {
            (format!("{attack_key}: ARREMESSAR"), format!("{guard_key}: DESFAZER"))
        } else {
            (format!("{cast_key}: LANÇAR"), format!("SEGURE {wheel_key}: FEITIÇOS"))
        };
        let dim = crate::hud::equipped_dim();
        let lines = [(first, dim), (second, dim)];
        let covered = if cooling > 0.0 { cooling / total } else { 0.0 };
        crate::hud::draw_equipped(
            ui,
            &crate::hud::Equipped { icon, name: &spell.name, lines: &lines, covered, alpha: 1.0 },
        );

        if aiming || matches!(self.spells.cast, Cast::Throwing { .. }) {
            let middle = ui.at(Anchor::Center, Vec2::ZERO);
            let line = Vec4::new(0.85, 0.96, 1.0, 0.9);
            let shadow = Vec4::new(0.0, 0.02, 0.08, 0.7);
            for (long, offset) in [
                (Vec2::new(10.0, 2.0), Vec2::new(-12.0, 0.0)),
                (Vec2::new(10.0, 2.0), Vec2::new(12.0, 0.0)),
                (Vec2::new(2.0, 10.0), Vec2::new(0.0, -12.0)),
                (Vec2::new(2.0, 10.0), Vec2::new(0.0, 12.0)),
                (Vec2::new(2.0, 2.0), Vec2::ZERO),
            ] {
                ui.rect(Rect::anchored(middle + offset, long + Vec2::splat(2.0), Anchor::Center), shadow);
                ui.rect(Rect::anchored(middle + offset, long, Anchor::Center), line);
            }
        }
    }

    /// Runs the book and every piece of ice out there. Returns whether a spell has the body
    /// busy, and it can't walk.
    pub(crate) fn update_spells(&mut self, ctx: &mut Context, dodging: bool, bag_open: bool, dt: f32) -> bool {
        self.run_capture(ctx);
        self.update_ice(ctx, dt);
        if let Some(animator) = ctx.world.get_mut::<Animator>(self.avatar) {
            let reading = animator.overlay_current() == Some(HOLD_CLIP);
            animator.set_hand_placed(reading);
        }
        let book = self.bag.book();
        if book != self.spells.book {
            self.book_changed(ctx, book);
        }
        let Some(book) = book else { return false };
        self.spells.book_for += dt;
        for cooldown in &mut self.spells.cooldowns {
            *cooldown = (*cooldown - dt).max(0.0);
        }
        let spells = self.grimoire_spells(ctx);
        self.spells.cooldowns.resize(spells.len(), 0.0);
        self.update_glow(ctx, book);

        let staggered = ctx.world.get::<Health>(self.avatar).is_some_and(|health| health.staggered());
        let can = (self.mode == Mode::Play || self.spells.scripted.is_some())
            && self.dead_for.is_none()
            && !dodging
            && !staggered
            && self.player.as_ref().is_some_and(|player| player.body.grounded && !player.swimming());
        let busy = match self.spells.cast {
            Cast::Idle => {
                if can && ctx.input.action_just_pressed(CAST_SPELL) {
                    self.begin_selected(ctx, &spells);
                }
                let wants_hold = self.spells.cast == Cast::Idle;
                if wants_hold && let Some(animator) = ctx.world.get_mut::<Animator>(self.avatar) {
                    animator.overlay_loop(HOLD_CLIP, UPPER_BODY, POSE_FADE);
                }
                matches!(self.spells.cast, Cast::Clip(_))
            }
            Cast::Aiming { spear, held_for } => {
                self.keep_aiming(ctx, spear, held_for + dt, can, dt);
                false
            }
            Cast::Throwing { spear, for_ } => {
                self.keep_throwing(ctx, spear, for_ + dt);
                false
            }
            Cast::Clip(clip) => self.keep_clip(ctx, clip, staggered, dodging),
        };
        self.spells.aim_left = (self.spells.aim_left - dt).max(0.0);
        let aiming = matches!(self.spells.cast, Cast::Aiming { .. } | Cast::Throwing { .. });
        if let Some(player) = self.player.as_mut() {
            player.aiming = aiming;
            player.shoulder = if aiming || self.spells.aim_left > 0.0 { 1.0 } else { 0.0 };
        }
        if !bag_open && self.mode == Mode::Play {
            self.draw_book_hud(ctx, &spells);
        }
        busy
    }

    fn book_changed(&mut self, ctx: &mut Context, book: Option<Entity>) {
        if let Cast::Aiming { spear, .. } | Cast::Throwing { spear: Some(spear), .. } = self.spells.cast {
            ctx.world.despawn(spear, &mut ctx.physics);
        }
        self.spells.cast = Cast::Idle;
        self.spells.aim_left = 0.0;
        self.spells.book = book;
        self.spells.book_for = 0.0;
        if let Some(glow) = self.spells.glow.take()
            && let Ok(system) = ctx.world.ecs.query_one_mut::<&mut ParticleSystem>(glow)
        {
            system.emitting = false;
            system.despawn_when_done = true;
            let _ = ctx.world.ecs.remove_one::<Light>(glow);
        }
        if let Some(player) = self.player.as_mut() {
            player.aiming = false;
            player.shoulder = 0.0;
        }
        let Some(book) = book else {
            if let Some(animator) = ctx.world.get_mut::<Animator>(self.avatar) {
                animator.overlay_stop(POSE_FADE);
            }
            return;
        };
        self.load_spells(ctx);
        if let Some(animator) = ctx.world.get_mut::<Animator>(book) {
            animator.replay_once("Abrir", 0.0);
        }
        if let Some(animator) = ctx.world.get_mut::<Animator>(self.avatar) {
            animator.overlay_loop(HOLD_CLIP, UPPER_BODY, POSE_FADE);
        }
        ctx.audio.play_pitched(sound!("ice_tick"), 0.6, 0.6);
    }

    /// The pages light up once the book is open, with the glow and a small cold light over them.
    fn update_glow(&mut self, ctx: &mut Context, book: Entity) {
        let Some(place) = ctx.world.transform(book).map(|t| t.matrix().transform_point3(BOOK_PAGES)) else { return };
        if self.spells.glow.is_none() && self.spells.book_for > BOOK_GLOW_AFTER {
            let Some(look) = self.spells.effects.book.clone() else { return };
            let system = ParticleSystem::new(look, Vec3::ZERO, 1.0, self.spells.seed);
            let light = PointLight {
                position: Vec3::ZERO,
                color: Vec3::new(0.45, 0.75, 1.0),
                intensity: 1.6,
                radius: 2.6,
                flicker: 0.12,
                night_only: false,
            };
            let entity = ctx.world.ecs.spawn((Transform::from_position(place), system, Light(light)));
            self.spells.glow = Some(entity);
        }
        if let Some(glow) = self.spells.glow
            && let Ok(transform) = ctx.world.ecs.query_one_mut::<&mut Transform>(glow)
        {
            transform.position = place;
        }
    }

    fn begin_selected(&mut self, ctx: &mut Context, spells: &[SpellDesc]) {
        let selected = self.spells.selected;
        let Some(spell) = spell_at(spells, selected) else { return };
        if self.spells.cooldowns.get(selected).copied().unwrap_or(0.0) > 0.0 {
            return;
        }
        let forward = self.player.as_ref().map(|player| {
            let (sy, cy) = player.camera_yaw.sin_cos();
            Vec3::new(-sy, 0.0, -cy)
        });
        match spell.id.as_str() {
            SPEAR => self.begin_spear(ctx),
            STOMP | BLAST => {
                let (clip, cooldown) =
                    if spell.id == STOMP { (STOMP_CLIP, STOMP_COOLDOWN) } else { (BLAST_CLIP, BLAST_COOLDOWN) };
                if let (Some(player), Some(forward)) = (self.player.as_mut(), forward) {
                    player.turn_to(forward);
                }
                if let Some(animator) = ctx.world.get_mut::<Animator>(self.avatar) {
                    animator.overlay_stop(0.1);
                    animator.replay_once(clip, 0.14);
                    animator.speed = 1.0;
                }
                self.spells.cooldowns[selected] = cooldown;
                self.spells.cast = Cast::Clip(clip);
            }
            other => eprintln!("[sandbox] a spell nobody wrote: {other}"),
        }
    }

    fn begin_spear(&mut self, ctx: &mut Context) {
        let Some(mesh) = self.spells.meshes.spear else { return };
        // Turned over in the fist: `weapon.R` points out by the thumb, and the tip goes by the little finger.
        let in_fist = Transform { rotation: Quat::from_rotation_x(std::f32::consts::PI), ..scaled(0.02) };
        let spear = ctx.world.attach(self.avatar, "weapon.R", mesh, in_fist);
        ice_look(ctx, spear, ICE_GLOW * 1.8);
        if let Some(animator) = ctx.world.get_mut::<Animator>(self.avatar) {
            animator.overlay_loop(AIM_CLIP, UPPER_BODY, 0.2);
        }
        ctx.audio.play_pitched(sound!("ice_form"), 0.8, 1.0);
        self.spells.cast = Cast::Aiming { spear, held_for: 0.0 };
    }

    fn keep_aiming(&mut self, ctx: &mut Context, spear: Entity, held_for: f32, can: bool, dt: f32) {
        // The cold gathers where the hand is going to be, once the arm is up.
        if held_for >= SPEAR_GATHERS
            && held_for - dt < SPEAR_GATHERS
            && let (Some(look), Some(hand)) =
                (&self.spells.effects.form, ctx.world.joint_transform(self.avatar, "weapon.R"))
        {
            ctx.world.play_effect(look, hand.position);
        }
        // It grows from the hand out to its full length.
        let grown = overshoot(held_for / SPEAR_FORMS).max(0.02);
        if let Ok(attached) = ctx.world.ecs.query_one_mut::<&mut Attached>(spear) {
            attached.offset.scale = Vec3::new(grown.min(1.0), grown, grown.min(1.0));
        }
        if let Ok(renderable) = ctx.world.ecs.query_one_mut::<&mut Renderable>(spear) {
            renderable.emissive = ICE_GLOW * (1.0 + 1.2 * (1.0 - held_for / SPEAR_FORMS).max(0.0));
        }
        let cancelled =
            !can || ctx.input.action_just_pressed(actions::GUARD) || ctx.input.action_just_pressed(CAST_SPELL);
        if cancelled {
            // It breaks in the hand rather than disappear.
            let at = ctx.world.transform(spear).map(|t| t.position);
            ctx.world.despawn(spear, &mut ctx.physics);
            if let Some(at) = at {
                self.throw_shards(ctx, at, Vec3::Y, 8, 1.8, 0.09);
                if let Some(look) = &self.spells.effects.breaking {
                    ctx.world.play_effect(look, at);
                }
            }
            ctx.audio.play_pitched(sound!("ice_tick"), 0.8, 0.5);
            self.spells.cast = Cast::Idle;
            return;
        }
        let throws = ctx.input.action_just_pressed(actions::ATTACK)
            || self.spells.scripted.as_deref() == Some("throw") && held_for > 1.0;
        if throws && held_for >= SPEAR_FORMS * 0.8 {
            if let Some(animator) = ctx.world.get_mut::<Animator>(self.avatar) {
                animator.overlay_once(THROW_CLIP, UPPER_BODY, 0.05);
            }
            self.spells.cast = Cast::Throwing { spear: Some(spear), for_: 0.0 };
            if self.spells.scripted.is_some() {
                self.spells.scripted = Some("done".into());
            }
            return;
        }
        self.spells.cast = Cast::Aiming { spear, held_for };
    }

    fn keep_throwing(&mut self, ctx: &mut Context, spear: Option<Entity>, for_: f32) {
        let mut spear = spear;
        if for_ >= THROW_RELEASE
            && let Some(held) = spear.take()
        {
            self.release_spear(ctx, held);
        }
        let done = ctx
            .world
            .get::<Animator>(self.avatar)
            .is_none_or(|animator| animator.overlay_current() != Some(THROW_CLIP) || animator.overlay_finished());
        self.spells.cast = if done && spear.is_none() { Cast::Idle } else { Cast::Throwing { spear, for_ } };
    }

    /// From the hand towards whatever is under the crosshair.
    fn release_spear(&mut self, ctx: &mut Context, held: Entity) {
        let place = ctx.world.transform(held);
        ctx.world.despawn(held, &mut ctx.physics);
        let (Some(place), Some(mesh)) = (place, self.spells.meshes.spear) else { return };
        let look = ctx.camera.forward();
        let eye = ctx.camera.position;
        let mut target = eye + look * AIM_RANGE;
        if let Some(hit) = ctx.physics.raycast_scenery(eye + look, look, AIM_RANGE) {
            target = hit.point;
        }
        let mut direction = (target - place.position).normalize_or_zero();
        // Something right in front of the character doesn't get to fling it sideways.
        if direction.dot(look) < 0.5 {
            direction = look;
        }
        let rotation = Quat::from_rotation_arc(Vec3::Y, direction);
        let entity = ctx.world.spawn(mesh, Transform { position: place.position, rotation, scale: Vec3::ONE });
        ice_look(ctx, entity, ICE_GLOW * 1.4);
        let trail = self.spells.effects.trail.clone().map(|effect| {
            let system = ParticleSystem::new(effect, Vec3::ZERO, 1.0, self.spells.seed);
            ctx.world.ecs.spawn((Transform::from_position(place.position), system))
        });
        self.spells.flying.push(Flying {
            entity,
            trail,
            velocity: direction * SPEAR_SPEED,
            travelled: 0.0,
            age: 0.0,
            position: place.position,
        });
        self.spells.aim_left = AIM_LINGER;
        if let Some(cooldown) = self.spells.cooldowns.get_mut(self.spells.selected) {
            *cooldown = SPEAR_COOLDOWN;
        }
        if let Some(player) = self.player.as_mut() {
            player.shake(0.03);
        }
        ctx.audio.play_pitched(sound!("ice_throw"), 0.9, 1.0);
    }

    fn keep_clip(&mut self, ctx: &mut Context, clip: &'static str, staggered: bool, dodging: bool) -> bool {
        let Some(animator) = ctx.world.get::<Animator>(self.avatar) else { return false };
        let (landed_stomp, landed_blast) = (animator.fired("stomp"), animator.fired("blast"));
        let over = staggered || dodging || animator.current() != Some(clip) || animator.finished();
        drop(animator);
        if landed_stomp {
            self.land_stomp(ctx);
        }
        if landed_blast {
            self.land_blast(ctx);
        }
        if over {
            self.spells.cast = Cast::Idle;
            return false;
        }
        true
    }

    /// The right foot came down: the ground takes the wave from there, forwards.
    fn land_stomp(&mut self, ctx: &mut Context) {
        let Some(player) = self.player.as_mut() else { return };
        player.shake(0.14);
        let feet = player.body.position;
        let forward = Quat::from_rotation_y(player.facing) * Vec3::NEG_Z;
        let foot = ctx.world.joint_transform(self.avatar, "foot.R").map_or(feet, |foot| foot.position);
        let origin = Vec3::new(foot.x, feet.y, foot.z);
        if let Some(look) = &self.spells.effects.stomp {
            ctx.world.play_effect(look, origin + Vec3::Y * 0.03);
        }
        ctx.audio.play_pitched(sound!("ice_stomp"), 1.0, 1.0);

        let mut rng = self.spells.rng();
        let side = Vec3::new(-forward.z, 0.0, forward.x);
        let wide = STOMP_ANGLE.to_radians().tan();
        let mut row = 1.0;
        let mut spikes = Vec::new();
        while row < STOMP_REACH {
            let along = row / STOMP_REACH;
            let across = row * wide;
            // Thin at the far end, where the wave is running out.
            let count = (1 + (across / 0.78) as i32).max(1);
            for k in -count..=count {
                if rng.range(0.0, 1.0) < 0.12 {
                    continue;
                }
                let k = k as f32 + rng.jitter(0.32);
                let at = origin + forward * (row + rng.jitter(0.22)) + side * (k / count as f32 * across);
                let middle = 1.0 - (k.abs() / count as f32).min(1.0);
                let rise = (along * 3.0).min(1.0) * (1.0 - along * along * 0.75);
                let size = (0.24 + 0.58 * rise) * (0.5 + 0.5 * middle) * rng.range(0.7, 1.25);
                let outwards = (forward * 0.8 + side * (k / count as f32) * 0.5).normalize_or_zero();
                spikes.push(Spike { wait: (at - origin).length() / STOMP_SPEED, position: at, outwards, size });
            }
            row += STOMP_ROW;
        }
        self.spells.spikes.extend(spikes);

        let cos = STOMP_ANGLE.to_radians().cos();
        ctx.world.strike_fauna_by(&mut |at, size| {
            let flat = Vec3::new(at.x - origin.x, 0.0, at.z - origin.z);
            let inside = flat.normalize_or_zero().dot(forward) >= cos * 0.92 || flat.length() < 1.0;
            inside && flat.length() - size < STOMP_REACH && (at.y - origin.y).abs() < 2.5
        });
        let mut hurt = Vec::new();
        for (entity, transform, health) in ctx.world.ecs.query::<(Entity, &Transform, &Health)>().iter() {
            let to = transform.position - origin;
            let flat = Vec3::new(to.x, 0.0, to.z);
            let near = flat.length() - health.radius;
            let inside = flat.normalize_or_zero().dot(forward) >= cos * 0.92 || flat.length() < 1.0;
            if health.team != PLAYER_TEAM && !health.is_dead() && near < STOMP_REACH && to.y.abs() < 2.5 && inside {
                hurt.push((flat.length() / STOMP_SPEED, entity, Reach::Hurt(STOMP_DAMAGE)));
            }
        }
        self.spells.reaching.extend(hurt);
    }

    /// The burst: a ring of ice right around, and everyone within reach held fast as it arrives.
    fn land_blast(&mut self, ctx: &mut Context) {
        let Some(player) = self.player.as_mut() else { return };
        player.shake(0.2);
        let feet = player.body.position;
        ctx.world.strike_fauna(feet, BLAST_RADIUS);
        if let Some(look) = &self.spells.effects.blast {
            ctx.world.play_effect(look, feet + Vec3::Y * 0.03);
        }
        ctx.audio.play_pitched(sound!("ice_blast"), 1.0, 1.0);
        let mut rng = self.spells.rng();
        let count = 18;
        for i in 0..count {
            let angle = i as f32 / count as f32 * std::f32::consts::TAU + rng.jitter(0.12);
            let out = Vec3::new(angle.cos(), 0.0, angle.sin());
            let reach = rng.range(1.7, 2.6);
            self.spells.spikes.push(Spike {
                wait: reach / BLAST_SPEED,
                position: feet + out * reach,
                outwards: out,
                size: rng.range(0.2, 0.48),
            });
        }
        for _ in 0..BLAST_SCATTER {
            let angle = rng.range(0.0, std::f32::consts::TAU);
            let out = Vec3::new(angle.cos(), 0.0, angle.sin());
            let along = rng.range(0.0, 1.0_f32).sqrt();
            let reach = 3.0 + (BLAST_RADIUS - 3.4) * along;
            self.spells.spikes.push(Spike {
                wait: reach / BLAST_SPEED,
                position: feet + out * reach,
                outwards: out,
                size: rng.range(0.1, 0.34) * (1.0 - along * 0.5),
            });
        }
        let mut reached = Vec::new();
        for (entity, transform, _) in ctx.world.ecs.query::<(Entity, &Transform, &Agent)>().iter() {
            let to = transform.position - feet;
            if entity != self.avatar && to.length() < BLAST_RADIUS {
                reached.push((to.length() / BLAST_SPEED, entity, Reach::Freeze));
            }
        }
        self.spells.reaching.extend(reached);
    }

    /// What a piece of ice needs to be seen.
    fn add_piece(&mut self, ctx: &mut Context, rng: &mut Rng, piece: PieceDesc) {
        let Some(&mesh) = self.spells.meshes.crystals.get(rng.range(0.0, CRYSTAL_SHAPES as f32) as usize) else {
            return;
        };
        let entity = ctx.world.spawn(mesh, Transform { scale: Vec3::splat(0.001), ..piece.place });
        ice_look(ctx, entity, ICE_GLOW);
        self.spells.pieces.push(IcePiece {
            entity,
            wait: piece.wait,
            age: 0.0,
            rise: piece.rise,
            stand: piece.stand,
            end: piece.end,
            breaks: piece.breaks,
            broke: false,
            holds: piece.holds,
            place: piece.place,
        });
    }

    /// Splinters off `at`, fanned around `towards`.
    fn throw_shards(&mut self, ctx: &mut Context, at: Vec3, towards: Vec3, count: usize, speed: f32, size: f32) {
        let mut rng = self.spells.rng();
        let floor = ctx
            .physics
            .raycast_scenery(at + Vec3::Y * 0.5, Vec3::NEG_Y, 8.0)
            .map_or(at.y - 8.0, |hit| hit.point.y + 0.02);
        for _ in 0..count {
            if self.spells.shards.len() >= MAX_SHARDS {
                return;
            }
            let Some(&mesh) = self.spells.meshes.shards.get(rng.range(0.0, SHARD_SHAPES as f32) as usize) else {
                return;
            };
            let random = Vec3::new(rng.jitter(1.0), rng.jitter(1.0), rng.jitter(1.0));
            let direction = (towards + random * 0.8 + Vec3::Y * 0.45).normalize_or_zero();
            let size = size * rng.range(0.55, 1.5);
            let entity =
                ctx.world.spawn(mesh, Transform { position: at, scale: Vec3::splat(size), ..Default::default() });
            ice_look(ctx, entity, ICE_GLOW * 1.1);
            self.spells.shards.push(Shard {
                entity,
                velocity: direction * speed * rng.range(0.45, 1.25),
                spin: Vec3::new(rng.jitter(12.0), rng.jitter(12.0), rng.jitter(12.0)),
                age: 0.0,
                life: rng.range(0.9, 1.7),
                floor,
                size,
            });
        }
    }

    /// The spear hit something: it breaks there, and ice grows out of the place.
    fn shatter_spear(&mut self, ctx: &mut Context, at: Vec3, normal: Vec3, direction: Vec3, grow: bool) {
        if let Some(look) = &self.spells.effects.shatter {
            ctx.world.play_effect(look, at + normal * 0.05);
        }
        if let Some(look) = &self.spells.effects.frost
            && normal.y > 0.5
        {
            ctx.world.play_effect(look, at + normal * 0.03);
            if let Some(ring) = &self.spells.effects.ring {
                ctx.world.play_effect(ring, at + normal * 0.05);
            }
        }
        let bounce = (direction - normal * direction.dot(normal) * 1.5).normalize_or_zero();
        self.throw_shards(ctx, at + normal * 0.08, normal + bounce * 0.6, 28, 8.0, 0.15);
        ctx.audio.play_at(sound!("ice_shatter"), 1.0, Emitter::loud(at));
        let distance = self.player.as_ref().map_or(0.0, |player| (player.body.position - at).length());
        if let Some(player) = self.player.as_mut()
            && distance < 12.0
        {
            player.shake(0.06 * (1.0 - distance / 12.0));
        }
        if !grow {
            return;
        }
        let mut rng = self.spells.rng();
        for i in 0..8 {
            let spread = if i == 0 { 0.0 } else { 0.6 };
            let lean =
                (normal + Vec3::new(rng.jitter(spread), rng.jitter(spread), rng.jitter(spread))).normalize_or_zero();
            let aside = Vec3::new(rng.jitter(0.22), rng.jitter(0.22), rng.jitter(0.22));
            let aside = aside - normal * aside.dot(normal);
            let size = if i == 0 { rng.range(0.8, 0.95) } else { rng.range(0.3, 0.62) };
            let place = Transform {
                position: at + aside - normal * 0.06,
                rotation: Quat::from_rotation_arc(Vec3::Y, lean)
                    * Quat::from_rotation_y(rng.range(0.0, std::f32::consts::TAU)),
                scale: Vec3::splat(size),
            };
            let stand = rng.range(2.4, 3.2);
            self.add_piece(
                ctx,
                &mut rng,
                PieceDesc { wait: i as f32 * 0.012, rise: 0.13, stand, end: 0.3, breaks: true, holds: None, place },
            );
        }
        // The cold cracks out over the ground: long low veins going away from the hit.
        if normal.y > 0.5 {
            for i in 0..9 {
                let around = i as f32 / 9.0 * std::f32::consts::TAU + rng.jitter(0.3);
                let out = Vec3::new(around.cos(), 0.0, around.sin());
                let from = rng.range(0.35, 0.8);
                let place = Transform {
                    position: at + out * from - Vec3::Y * 0.03,
                    rotation: Quat::from_rotation_arc(Vec3::Y, (out + Vec3::Y * 0.16).normalize()),
                    scale: Vec3::new(0.2, rng.range(0.7, 1.35), 0.2),
                };
                let stand = rng.range(1.9, 2.8);
                self.add_piece(
                    ctx,
                    &mut rng,
                    PieceDesc {
                        wait: 0.015 + from * 0.05,
                        rise: 0.12,
                        stand,
                        end: 0.3,
                        breaks: true,
                        holds: None,
                        place,
                    },
                );
            }
        }
    }

    /// Ice over the legs of `target` and cold pouring off it, for as long as it's frozen.
    fn freeze_up(&mut self, ctx: &mut Context, target: Entity) {
        if !ctx.world.freeze(target, FREEZE_SECONDS) {
            return;
        }
        let Some(feet) = ctx.world.transform(target).map(|t| t.position) else { return };
        let mut rng = self.spells.rng();
        ctx.audio.play_pitched_at(sound!("ice_tick"), 0.8, 0.8, Emitter::at(feet));
        let stands = FREEZE_SECONDS - 0.6;
        for (joint, base_size, count) in [
            ("foot.L", 0.44, 4),
            ("foot.R", 0.44, 4),
            ("shin.L", 0.24, 3),
            ("shin.R", 0.24, 3),
            ("thigh.L", 0.16, 2),
            ("thigh.R", 0.16, 2),
        ] {
            let Some(bone) = ctx.world.joint_transform(target, joint) else { continue };
            for k in 0..count {
                let on_foot = joint.starts_with("foot");
                let around = rng.range(0.0, std::f32::consts::TAU);
                let out = Vec3::new(around.cos(), 0.0, around.sin());
                let (position, lean) = if on_foot {
                    let ground =
                        Vec3::new(bone.position.x, feet.y - 0.02, bone.position.z) + out * rng.range(0.02, 0.16);
                    (ground, (Vec3::Y + out * 0.32).normalize())
                } else {
                    // A thigh's joint is at the hip: its ice sits halfway down, against the leg.
                    let down = if joint.starts_with("thigh") { 0.24 } else { 0.0 };
                    let place = bone.position + Vec3::Y * (rng.jitter(0.1) - down) + out * 0.05;
                    (place, (Vec3::Y + out * 0.6).normalize())
                };
                let size = base_size * rng.range(0.75, 1.25);
                let place = Transform {
                    position,
                    rotation: Quat::from_rotation_arc(Vec3::Y, lean) * Quat::from_rotation_y(around),
                    scale: Vec3::splat(size),
                };
                self.add_piece(
                    ctx,
                    &mut rng,
                    PieceDesc {
                        // From the ground up the legs.
                        wait: if on_foot { k as f32 * 0.03 } else { 0.1 + k as f32 * 0.04 },
                        rise: 0.22,
                        stand: stands,
                        end: 0.26,
                        breaks: true,
                        holds: Some(target),
                        place,
                    },
                );
            }
        }
        if let Some(look) = self.spells.effects.smoke.clone() {
            let system = ParticleSystem::new(look, Vec3::ZERO, 1.0, self.spells.seed);
            let entity = ctx.world.ecs.spawn((Transform::from_position(feet), system));
            self.spells.smokes.push(Smoke { entity, over: target, left: FREEZE_SECONDS });
        }
    }

    /// Moves everything thrown or grown by a spell. It runs with or without the book out.
    fn update_ice(&mut self, ctx: &mut Context, dt: f32) {
        self.update_flying(ctx, dt);
        self.update_spikes(ctx, dt);
        self.update_reaching(ctx, dt);
        self.update_pieces(ctx, dt);
        self.update_shards(ctx, dt);
        self.update_smokes(ctx, dt);
    }

    fn update_flying(&mut self, ctx: &mut Context, dt: f32) {
        let mut flying = std::mem::take(&mut self.spells.flying);
        flying.retain_mut(|spear| {
            spear.age += dt;
            let drop_in = ((spear.travelled - SPEAR_STRAIGHT) / SPEAR_DROP_IN).clamp(0.0, 1.0);
            spear.velocity += Vec3::NEG_Y * SPEAR_GRAVITY * drop_in * drop_in * dt;
            let step = spear.velocity * dt;
            let direction = step.normalize_or_zero();
            // The tip reaches a target first, and a meter of shaft comes after it.
            let tip = spear.position + direction * 1.13;
            let next_tip = tip + step;
            let scenery = ctx.physics.raycast_scenery(tip, direction, step.length());
            let mut hit: Option<(f32, Vec3, Vec3, Option<Entity>)> =
                scenery.map(|hit| (hit.distance / step.length().max(1e-5), hit.point, hit.normal, None));
            for (entity, transform, health) in ctx.world.ecs.query::<(Entity, &Transform, &Health)>().iter() {
                if health.team == PLAYER_TEAM || health.is_dead() {
                    continue;
                }
                let foot = transform.position + Vec3::Y * 0.15;
                let top = transform.position + Vec3::Y * 1.65;
                let (distance, along) = segments(tip, next_tip, foot, top);
                if distance < health.radius.max(0.3) && hit.is_none_or(|h| along < h.0) {
                    let point = tip + step * along;
                    hit = Some((along, point, -direction, Some(entity)));
                }
            }
            spear.travelled += step.length();
            spear.position += step;
            let rotation = Quat::from_rotation_arc(Vec3::Y, direction);
            if let Ok(transform) = ctx.world.ecs.query_one_mut::<&mut Transform>(spear.entity) {
                transform.position = spear.position;
                transform.rotation = rotation;
            }
            if let Some(trail) = spear.trail
                && let Ok(transform) = ctx.world.ecs.query_one_mut::<&mut Transform>(trail)
            {
                transform.position = spear.position + direction * 0.7;
            }
            let timed_out = spear.age > SPEAR_SECONDS;
            if hit.is_none() && !timed_out {
                return true;
            }
            ctx.world.despawn(spear.entity, &mut ctx.physics);
            if let Some(trail) = spear.trail
                && let Ok(system) = ctx.world.ecs.query_one_mut::<&mut ParticleSystem>(trail)
            {
                system.emitting = false;
                system.despawn_when_done = true;
            }
            let (point, normal, target) = hit.map_or((spear.position, -direction, None), |h| (h.1, h.2, h.3));
            ctx.world.strike_fauna(point, SPEAR_KILLS);
            if let Some(target) = target {
                ctx.world.damage(target, SPEAR_DAMAGE, &mut ctx.physics);
            }
            self.shatter_spear(ctx, point, normal, direction, target.is_none() && !timed_out);
            false
        });
        self.spells.flying.append(&mut flying);
    }

    fn update_spikes(&mut self, ctx: &mut Context, dt: f32) {
        let mut spikes = std::mem::take(&mut self.spells.spikes);
        let mut rng = self.spells.rng();
        spikes.retain_mut(|spike| {
            spike.wait -= dt;
            if spike.wait > 0.0 {
                return true;
            }
            let from = spike.position + Vec3::Y * 1.8;
            let Some(ground) = ctx.physics.raycast_scenery(from, Vec3::NEG_Y, 4.0) else { return false };
            // Not on a tree's branch or a wall's top: this is ice that runs along the ground.
            if ground.normal.y < 0.55 || ground.point.y > spike.position.y + 1.2 {
                return false;
            }
            let lean = (ground.normal * 0.5 + Vec3::Y + spike.outwards * 0.62).normalize();
            let place = Transform {
                position: ground.point - Vec3::Y * 0.04,
                rotation: Quat::from_rotation_arc(Vec3::Y, lean)
                    * Quat::from_rotation_y(rng.range(0.0, std::f32::consts::TAU)),
                scale: Vec3::splat(spike.size),
            };
            let stand = rng.range(1.5, 2.2);
            self.add_piece(
                ctx,
                &mut rng,
                PieceDesc { wait: 0.0, rise: 0.11, stand, end: 0.28, breaks: true, holds: None, place },
            );
            if let Some(look) = &self.spells.effects.spike {
                ctx.world.play_effect(look, ground.point + Vec3::Y * 0.04);
            }
            if let Some(look) = &self.spells.effects.frost {
                ctx.world.play_effect(look, ground.point + Vec3::Y * 0.025);
            }
            false
        });
        self.spells.spikes.append(&mut spikes);
    }

    fn update_reaching(&mut self, ctx: &mut Context, dt: f32) {
        let mut reaching = std::mem::take(&mut self.spells.reaching);
        reaching.retain_mut(|(wait, target, what)| {
            *wait -= dt;
            if *wait > 0.0 {
                return true;
            }
            match *what {
                Reach::Hurt(amount) => {
                    ctx.world.damage(*target, amount, &mut ctx.physics);
                }
                Reach::Freeze => self.freeze_up(ctx, *target),
            }
            false
        });
        self.spells.reaching.append(&mut reaching);
    }

    fn update_pieces(&mut self, ctx: &mut Context, dt: f32) {
        let mut pieces = std::mem::take(&mut self.spells.pieces);
        pieces.retain_mut(|piece| {
            if piece.wait > 0.0 {
                piece.wait -= dt;
                return true;
            }
            piece.age += dt;
            let gone = piece.holds.is_some_and(|holds| {
                !ctx.world.ecs.contains(holds) || ctx.world.get::<Health>(holds).is_some_and(|h| h.is_dead())
            });
            if gone && piece.age < piece.rise + piece.stand {
                piece.stand = (piece.age - piece.rise).max(0.0);
            }
            let ending = piece.age - piece.rise - piece.stand;
            if ending >= 0.0 && piece.breaks && !piece.broke {
                piece.broke = true;
                let middle = piece.place.position + piece.place.rotation * Vec3::Y * piece.place.scale.y * 0.6;
                let size = piece.place.scale.y;
                let count = 2 + (size * 10.0) as usize;
                self.throw_shards(ctx, middle, Vec3::Y * 0.4, count.min(8), 1.8 + size * 4.0, 0.08 + size * 0.2);
                if size > 0.2
                    && let Some(look) = &self.spells.effects.breaking
                {
                    ctx.world.play_effect(look, middle);
                }
            }
            let grown = if ending >= 0.0 {
                let out = 1.0 - smooth(ending / piece.end);
                // Broken: what's left collapses. Melted: it sinks as it goes.
                out * out
            } else {
                overshoot(piece.age / piece.rise)
            };
            let flash = (1.0 - piece.age / (piece.rise * 2.5)).max(0.0);
            let sink = if ending >= 0.0 { smooth(ending / piece.end) * piece.place.scale.y * 0.25 } else { 0.0 };
            let (place, entity) = (piece.place, piece.entity);
            if let Ok((transform, renderable)) =
                ctx.world.ecs.query_one_mut::<(&mut Transform, &mut Renderable)>(entity)
            {
                transform.position = place.position - Vec3::Y * sink;
                transform.rotation = place.rotation;
                // Up first and out after: it shoots out of the ground.
                let across = (grown * 1.1).min(grown.powf(0.8));
                transform.scale = Vec3::new(place.scale.x * across, place.scale.y * grown, place.scale.z * across);
                renderable.emissive = ICE_GLOW * (1.0 + 2.0 * flash);
            }
            if ending >= piece.end {
                ctx.world.despawn(entity, &mut ctx.physics);
                return false;
            }
            true
        });
        self.spells.pieces.append(&mut pieces);
    }

    fn update_shards(&mut self, ctx: &mut Context, dt: f32) {
        let mut shards = std::mem::take(&mut self.spells.shards);
        shards.retain_mut(|shard| {
            shard.age += dt;
            shard.velocity.y -= SHARD_GRAVITY * dt;
            let Ok(transform) = ctx.world.ecs.query_one_mut::<&mut Transform>(shard.entity) else { return false };
            transform.position += shard.velocity * dt;
            if transform.position.y < shard.floor {
                transform.position.y = shard.floor;
                shard.velocity.y = shard.velocity.y.abs() * 0.3;
                shard.velocity.x *= 0.55;
                shard.velocity.z *= 0.55;
                shard.spin *= 0.5;
            }
            transform.rotation = Quat::from_scaled_axis(shard.spin * dt) * transform.rotation;
            // It's already small when it goes.
            let t = shard.age / shard.life;
            transform.scale = Vec3::splat(shard.size * (1.0 - smooth((t - 0.45) / 0.55)));
            if t >= 1.0 {
                ctx.world.despawn(shard.entity, &mut ctx.physics);
                return false;
            }
            true
        });
        self.spells.shards.append(&mut shards);
    }

    fn update_smokes(&mut self, ctx: &mut Context, dt: f32) {
        let mut smokes = std::mem::take(&mut self.spells.smokes);
        smokes.retain_mut(|smoke| {
            smoke.left -= dt;
            let still_frozen = ctx.world.is_frozen(smoke.over);
            if smoke.left > 0.6 && (still_frozen || smoke.left > FREEZE_SECONDS - 0.1) {
                return true;
            }
            if let Ok(system) = ctx.world.ecs.query_one_mut::<&mut ParticleSystem>(smoke.entity) {
                system.emitting = false;
                system.despawn_when_done = true;
            }
            false
        });
        self.spells.smokes.append(&mut smokes);
    }

    /// A `--menu spell:<what>` run takes the book by itself and does `what`.
    fn run_capture(&mut self, ctx: &mut Context) {
        if let Some((what, frame)) = &self.spells.scheduled
            && ctx.time.frame() >= *frame
        {
            let what = what.clone();
            self.spells.scheduled = None;
            self.bag.put_book_in_hand(&ctx.world.items.clone());
            self.spells.scripted = Some(what);
        }
        let Some(what) = self.spells.scripted.clone() else { return };
        if self.spells.book.is_none() || self.spells.book_for < 1.3 || what == "hold" || what == "wheel" {
            return;
        }
        let spells = self.grimoire_spells(ctx);
        match what.as_str() {
            "aim" | "throw" if self.spells.cast == Cast::Idle => {
                self.spells.selected = 0;
                self.begin_selected(ctx, &spells);
                if what == "aim" {
                    self.spells.scripted = Some("aiming".into());
                }
            }
            "stomp" => {
                self.spells.selected = 1;
                self.begin_selected(ctx, &spells);
                self.spells.scripted = Some("done".into());
            }
            "blast" => {
                self.spells.selected = 2;
                self.begin_selected(ctx, &spells);
                self.spells.scripted = Some("done".into());
            }
            _ => {}
        }
    }
}

struct PieceDesc {
    wait: f32,
    rise: f32,
    stand: f32,
    end: f32,
    breaks: bool,
    holds: Option<Entity>,
    place: Transform,
}

/// A shape in the ice picture, smooth enough to catch the sun.
fn ice_mesh(desc: MeshDesc, tile: f32) -> MeshData {
    fn polish(mesh: &mut MeshData) {
        for material in &mut mesh.materials {
            material.roughness = ICE_ROUGHNESS;
        }
        for lod in &mut mesh.lods {
            polish(&mut lod.mesh);
        }
    }
    let mut mesh = desc.build();
    wrap_in_texture(&mut mesh, Path::new(ICE_TEXTURE), tile);
    polish(&mut mesh);
    mesh
}

fn scaled(scale: f32) -> Transform {
    Transform { scale: Vec3::splat(scale), ..Default::default() }
}

fn ice_look(ctx: &mut Context, entity: Entity, glow: f32) {
    if let Ok(renderable) = ctx.world.ecs.query_one_mut::<&mut Renderable>(entity) {
        renderable.color = ICE_COLOR;
        renderable.emissive = glow;
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::DEFAULT_SCENE;

    const DT: f32 = 1.0 / 60.0;

    struct Folder(PathBuf);

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The sandbox with the grimoire open in the hand.
    fn start(name: &str) -> (Sandbox, Context, Folder) {
        let dir = std::env::temp_dir().join(format!("grimora_spells_{name}_{}", std::process::id()));
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
        game.bag.put_book_in_hand(&ctx.world.items.clone());
        frames(&mut game, &mut ctx, 90);
        assert!(game.spells.book.is_some(), "the book isn't in the hand");
        (game, ctx, Folder(dir))
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

    fn select(game: &mut Sandbox, id: &str, ctx: &Context) {
        let spells = game.grimoire_spells(ctx);
        game.spells.selected = spells.iter().position(|spell| spell.id == id).unwrap();
    }

    #[test]
    fn the_wheel_chooses_on_release_and_on_click() {
        let (mut game, mut ctx, _dir) = start("wheel");
        let normal = ctx.time.scale();
        assert_eq!(game.spells.selected, 0);

        ctx.input.key_down(KeyCode::KeyG);
        frames(&mut game, &mut ctx, 2);
        assert!(game.spells.wheel.is_some());
        assert!(ctx.time.scale() < normal, "the world slows while choosing");

        // The second spell sits to the lower right.
        ctx.input.add_mouse_delta(Vec2::new(WHEEL_REACH * 0.9, WHEEL_REACH * 0.5));
        frames(&mut game, &mut ctx, 2);
        ctx.input.key_up(KeyCode::KeyG);
        frames(&mut game, &mut ctx, 2);
        assert!(game.spells.wheel.is_none());
        assert_eq!(game.spells.selected, 1);
        assert_eq!(ctx.time.scale(), normal);

        // Holding it and clicking chooses too, and the click doesn't go on to a blow.
        ctx.input.key_down(KeyCode::KeyG);
        frames(&mut game, &mut ctx, 2);
        ctx.input.add_mouse_delta(Vec2::new(-WHEEL_REACH * 0.9, WHEEL_REACH * 0.5));
        frames(&mut game, &mut ctx, 2);
        ctx.input.button_down(MouseButton::Left);
        frames(&mut game, &mut ctx, 2);
        ctx.input.button_up(MouseButton::Left);
        ctx.input.key_up(KeyCode::KeyG);
        frames(&mut game, &mut ctx, 2);
        assert_eq!(game.spells.selected, 2);
        assert!(game.spells.cast == Cast::Idle);
    }

    #[test]
    fn the_spear_is_readied_thrown_and_breaks_on_what_it_hits() {
        let (mut game, mut ctx, _dir) = start("spear");
        select(&mut game, SPEAR, &ctx);
        press(&mut game, &mut ctx, KeyCode::KeyC);
        assert!(matches!(game.spells.cast, Cast::Aiming { .. }));
        frames(&mut game, &mut ctx, 60);
        assert!(game.player.as_ref().unwrap().shoulder > 0.0, "the camera goes to the shoulder");

        // The ground right ahead of the character, so that it lands in this test.
        game.player.as_mut().unwrap().camera_pitch = -0.35;
        frames(&mut game, &mut ctx, 3);
        press(&mut game, &mut ctx, KeyCode::KeyF);
        assert!(matches!(game.spells.cast, Cast::Throwing { .. }));
        frames(&mut game, &mut ctx, (THROW_RELEASE / DT) as u32 + 2);
        assert_eq!(game.spells.flying.len(), 1, "it has left the hand");
        assert!(game.spells.cooldowns[0] > 0.0);

        frames(&mut game, &mut ctx, 120);
        assert!(game.spells.flying.is_empty(), "it hit the ground");
        assert!(!game.spells.pieces.is_empty(), "ice grew where it hit");

        // Everything it left melts away, and nothing stays in the world.
        frames(&mut game, &mut ctx, 360);
        assert!(game.spells.pieces.is_empty());
        assert!(game.spells.shards.is_empty());
    }

    #[test]
    fn a_spear_flies_straight_for_a_long_way_before_it_falls() {
        let (mut game, mut ctx, _dir) = start("drop");
        let mesh = game.spells.meshes.spear.unwrap();
        // Up above everything, flying level.
        let start = Vec3::new(0.0, 400.0, 0.0);
        let entity = ctx.world.spawn(mesh, Transform::from_position(start));
        game.spells.flying.push(Flying {
            entity,
            trail: None,
            velocity: Vec3::NEG_Z * SPEAR_SPEED,
            travelled: 0.0,
            age: 0.0,
            position: start,
        });
        let height_at = |game: &mut Sandbox, ctx: &mut Context, meters: f32| {
            while game.spells.flying[0].travelled < meters {
                frames(game, ctx, 1);
            }
            game.spells.flying[0].position.y
        };
        let early = height_at(&mut game, &mut ctx, SPEAR_STRAIGHT);
        assert!((early - start.y).abs() < 0.01, "it dropped {} m within its straight stretch", start.y - early);
        let just_after = height_at(&mut game, &mut ctx, SPEAR_STRAIGHT + SPEAR_DROP_IN * 0.5);
        let later = height_at(&mut game, &mut ctx, SPEAR_STRAIGHT + SPEAR_DROP_IN * 2.0);
        assert!(just_after < start.y, "it should have begun to fall");
        assert!(start.y - later > (start.y - just_after) * 4.0, "the fall should build up");
    }

    #[test]
    fn the_stomp_runs_ahead_in_a_cone_and_hurts_what_it_reaches() {
        let (mut game, mut ctx, _dir) = start("stomp");
        select(&mut game, STOMP, &ctx);
        let player = game.player.as_ref().unwrap();
        let (feet, forward) = (player.body.position, Quat::from_rotation_y(player.facing) * Vec3::NEG_Z);
        let hostile = ctx.world.find("hostile").unwrap();
        let life = ctx.world.get::<Health>(hostile).unwrap().current;
        press(&mut game, &mut ctx, KeyCode::KeyC);
        assert_eq!(game.spells.cast, Cast::Clip(STOMP_CLIP));
        let mut farthest = 0.0_f32;
        for _ in 0..120 {
            if let Some(transform) = ctx.world.get_mut::<Transform>(hostile) {
                transform.position = feet + forward * 5.0;
            }
            frames(&mut game, &mut ctx, 1);
            for piece in &game.spells.pieces {
                let to = piece.place.position - feet;
                let flat = Vec3::new(to.x, 0.0, to.z);
                farthest = farthest.max(flat.dot(forward));
                let cone = (STOMP_ANGLE + 16.0).to_radians().cos();
                assert!(flat.length() < 1.6 || flat.normalize().dot(forward) > cone, "ice outside the cone at {to}");
            }
        }
        assert!(farthest > STOMP_REACH * 0.7, "it only got {farthest} m out");
        assert!(ctx.world.get::<Health>(hostile).unwrap().current < life, "the wave went through it");
    }

    #[test]
    fn the_blast_holds_everyone_near_for_a_while() {
        let (mut game, mut ctx, _dir) = start("blast");
        select(&mut game, BLAST, &ctx);
        let feet = game.player.as_ref().unwrap().body.position;
        let near = ctx.world.find("watcher").unwrap();
        let far = ctx.world.find("timid").unwrap();
        assert!((ctx.world.transform(near).unwrap().position - feet).length() < BLAST_RADIUS);
        assert!((ctx.world.transform(far).unwrap().position - feet).length() > BLAST_RADIUS);

        press(&mut game, &mut ctx, KeyCode::KeyC);
        frames(&mut game, &mut ctx, 110);
        assert!(ctx.world.is_frozen(near), "the wave has passed it");
        assert!(!ctx.world.is_frozen(far));
        let color = ctx.world.get::<Renderable>(near).unwrap().color;
        let before = ctx.world.transform(near).unwrap().position;
        assert!(game.spells.pieces.iter().any(|piece| piece.holds == Some(near)), "ice on its legs");
        assert!(game.spells.smokes.iter().any(|smoke| smoke.over == near), "cold pouring off it");

        // It can't go anywhere, even with something to run from, and comes back to itself after.
        frames(&mut game, &mut ctx, 180);
        assert!(ctx.world.is_frozen(near));
        assert!(color.x < color.z, "it should look cold");
        assert!((ctx.world.transform(near).unwrap().position - before).length() < 0.01);
        frames(&mut game, &mut ctx, (FREEZE_SECONDS * 60.0) as u32);
        assert!(!ctx.world.is_frozen(near));
        let color = ctx.world.get::<Renderable>(near).unwrap().color;
        assert!((color.x - color.z).abs() < 0.01, "the cold left its color: {color}");
        assert!(!game.spells.pieces.iter().any(|piece| piece.holds == Some(near)));
    }

    #[test]
    fn a_spear_breaking_far_away_sounds_far_away() {
        let (mut game, mut ctx, _dir) = start("sound");
        ctx.audio = Audio::headless();
        frames(&mut game, &mut ctx, 1);
        let feet = game.player.as_ref().unwrap().body.position;
        let mut heard_from = |at: Vec3| {
            game.shatter_spear(&mut ctx, at, Vec3::Y, Vec3::NEG_Z, false);
            frames(&mut game, &mut ctx, 1);
            ctx.audio.world_sounds().find(|(emitter, _)| emitter.position == at).unwrap().1
        };
        // Close to the character is close, however far behind the camera is.
        let near = heard_from(feet);
        assert_eq!(near.gain, 1.0);
        assert!(near.delay < 0.01);
        let far = heard_from(feet + Vec3::new(60.0, 3.0, 0.0));
        assert!(far.gain < 0.1, "{far:?}");
        assert!(far.cutoff < near.cutoff * 0.2, "{far:?}");
        assert!(far.delay > 0.15, "{far:?}");
    }
}
