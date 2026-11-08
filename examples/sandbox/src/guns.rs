//! The flintlocks: aiming over the shoulder, a shot that takes a moment to go off, the kick, the
//! smoke, and loading one ball at a time with a ramrod. The numbers are in `items.ron`, the
//! poses in `<clip>_*.ron` (`tools/gen_gun_clips.py`), and the line itself in `World::shoot`.
//!
//! The gun's own moving parts are clips of its model (`tools/rig_guns.py`). The ball and the rod in
//! the free hand are separate things attached there, while the one in the gun's model is hidden.

use std::collections::{HashMap, VecDeque};
use std::path::Path;

use grimora::Attached;
use grimora::core::MeshData;
use grimora::prelude::*;
use grimora::scene::{GunDesc, GunGrip, Rng};

use crate::{FOE_BAR_SECONDS, HIT_SOUND, Mode, Sandbox, UPPER_BODY};

const RELOAD: &str = "reload";

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

const TEXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/textures");
const HOLE_PICTURE: &str = "impacto_bala_preto_128.png";
/// Meters across the picture, of which the hole itself is about half.
const HOLE_SIZE: f32 = 0.42;
const HOLE_SECONDS: f32 = 45.0;
/// It closes up over this long at the end, rather than be gone from one frame to the next.
const HOLE_CLOSES: f32 = 1.5;
const MOST_HOLES: usize = 48;

/// Joints that bend the body to aim up or down, parents first.
const AIM_BEND: [&str; 2] = ["spine", "chest"];
/// How far up and down it can bend, in radians: the camera can go further.
const BEND_UP: f32 = 0.75;
const BEND_DOWN: f32 = -0.85;
/// Per second, how fast the body goes to and from aiming.
const AIM_EASE: f32 = 11.0;
const AIM_UP_FADE: f32 = 0.14;
const AIM_DOWN_FADE: f32 = 0.28;
/// Seconds of staying at the shoulder after a shot.
const AIM_LINGER: f32 = 0.7;
/// How high `aim` has to be for a shot not to wait for the gun to come up.
const AIMED: f32 = 0.75;
/// A shot stays this far off the crosshair at most, in how much it can turn from the view.
const MOST_OFF_VIEW: f32 = 0.35;
/// Moving spreads a shot this many times.
const MOVING_SPREAD: f32 = 1.6;
/// How much wider the crosshair opens right after a shot, in degrees, and how fast it closes.
const BLOOM: f32 = 5.0;
const BLOOM_BACK: f32 = 3.5;

/// Per second, how fast the view opens back up from the shot's punch.
const PUNCH_BACK: f32 = 9.0;
/// Radians wider the view gets at the shot, with a `power` of 1.
const PUNCH: f32 = 0.035;
/// Per shot: how far off animals run from, in meters, and how hard it shoves what moves.
const SCARES: f32 = 32.0;
const IMPULSE: f32 = 22.0;
/// Meters between puffs of the line of smoke the ball leaves, and how many at most.
const TRAIL_EVERY: f32 = 1.3;
const TRAIL_PUFFS: usize = 8;
/// How long the barrel lights things up, and how bright.
const FLASH_SECONDS: f32 = 0.11;
const FLASH_LIGHT: f32 = 26.0;
const FLASH_RADIUS: f32 = 9.0;

// ---- things in the free hand, in the `hand.L` joint's own space
/// In the fingers, and in the middle of the fist. `tools/gen_gun_clips.py` prints these.
const IN_FINGERS: Vec3 = Vec3::new(0.0197, 0.1416, -0.0156);
const IN_FIST: Vec3 = Vec3::new(0.0268, 0.0776, -0.0117);
/// A rod through the closed fist, from the little finger to the thumb.
const FIST_AXIS: Vec3 = Vec3::new(0.1908, 0.0812, 0.9783);
/// A ball would be a speck at its true size.
const BALL_SCALE: f32 = 1.5;
const ROD_RADIUS: f32 = 0.0035;
const ROD_HEAD: f32 = 0.006;
const ROD_COLOR: Vec3 = Vec3::new(0.46, 0.43, 0.4);

/// What the character is doing with the gun.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
enum Act {
    #[default]
    Ready,
    /// Fired without aiming: the arm has to come up first.
    Raising {
        left: f32,
    },
    /// The flint fell `for_` seconds ago.
    Firing {
        for_: f32,
        fired: bool,
    },
    Reloading {
        for_: f32,
        ball: Option<Entity>,
        rod: Option<Entity>,
    },
}

struct Hole {
    entity: Entity,
    age: f32,
    place: Transform,
}

impl Guns {
    /// Moves the holes by `offset`, as the world's origin moves the other way.
    pub fn shift(&mut self, offset: Vec3) {
        for hole in &mut self.holes {
            hole.place.position += offset;
        }
    }
}

struct Flash {
    entity: Entity,
    left: f32,
    power: f32,
}

#[derive(Default)]
struct Effects {
    flash: Option<ParticleEffect>,
    pan: Option<ParticleEffect>,
    trail: Option<ParticleEffect>,
    impact: Option<ParticleEffect>,
    flesh: Option<ParticleEffect>,
    powder: Option<ParticleEffect>,
}

fn load(path: &str) -> Option<ParticleEffect> {
    load_effect(path).map_err(|err| eprintln!("[sandbox] {err:#}")).ok()
}

/// A capture of a gun in use: `--menu gun:<item>:<hold|aim|fire|reload>`.
struct Script {
    what: String,
    age: f32,
    done: bool,
}

#[derive(Default)]
pub struct Guns {
    act: Act,
    /// The gun in the hand, as seen last frame.
    held: Option<Entity>,
    /// Each gun's state, by item: one that isn't here has never been fired, and is loaded.
    pub empty: HashMap<String, bool>,
    /// From the hip to the shoulder: what `AIM_BEND` and the zoom follow.
    aim: f32,
    /// How long the camera stays at the shoulder.
    aim_left: f32,
    /// How much the upper body ignores what the legs do.
    steady: f32,
    bloom: f32,
    fov: Option<f32>,
    zoom: f32,
    punch: f32,
    /// Seconds left of the mark on the crosshair, and whether what it says is a kill.
    hit_mark: f32,
    killed: bool,
    holes: VecDeque<Hole>,
    flashes: Vec<Flash>,
    effects: Effects,
    hole_mesh: Option<MeshId>,
    rods: HashMap<u32, MeshId>,
    seed: u64,
    /// Things got reset around the character: what's in its hands has to start over.
    pub reset: bool,
    /// A `--menu gun:...` run: what to do, and on which frame to take the gun.
    pub scheduled: Option<(String, u64)>,
    script: Option<Script>,
}

impl Guns {
    /// Everything in the world went with the old one.
    pub fn forget_world(&mut self, camera: &mut Camera) {
        if let Some(fov) = self.fov {
            camera.fov_y = fov;
        }
        let keep = Self {
            empty: std::mem::take(&mut self.empty),
            scheduled: self.scheduled.take(),
            script: self.script.take(),
            seed: self.seed,
            zoom: 1.0,
            ..Default::default()
        };
        *self = keep;
    }

    fn rng(&mut self) -> Rng {
        self.seed = self.seed.wrapping_add(1);
        Rng::new(self.seed.wrapping_mul(0x9E37_79B9) ^ 0xB17)
    }

    fn loaded(&self, item: &str) -> bool {
        !self.empty.get(item).copied().unwrap_or(false)
    }
}

pub fn add_controls(controls: &mut ActionMap) {
    controls.add(RELOAD, [Binding::Key(KeyCode::KeyR), Binding::Pad(PadButton::North)]);
}

/// The names of those controls for the menu.
pub const CONTROL_NAMES: [(&str, &str); 1] = [(RELOAD, "Recarregar")];

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The effect, with what it throws "up" going towards `towards`.
fn pointed(effect: &ParticleEffect, towards: Vec3) -> ParticleEffect {
    let turn = Quat::from_rotation_arc(Vec3::Y, towards.normalize_or_zero());
    let descs: Vec<_> = effect
        .iter()
        .map(|desc| {
            let mut desc = desc.clone();
            desc.direction = turn * desc.direction;
            desc.position = turn * desc.position;
            desc
        })
        .collect();
    descs.into()
}

/// A ramrod standing on its end at the origin, with a wider head at the top.
fn rod_mesh(length: f32) -> MeshData {
    const SIDES: usize = 6;
    let rows = [(0.0, ROD_RADIUS), (length - 0.03, ROD_RADIUS), (length - 0.026, ROD_HEAD), (length, ROD_HEAD)];
    let ring = |y: f32, radius: f32, at: usize| {
        let angle = at as f32 / SIDES as f32 * std::f32::consts::TAU;
        Vec3::new(angle.cos() * radius, y, angle.sin() * radius)
    };
    let mut mesh = MeshData::new();
    for pair in rows.windows(2) {
        let ((y0, r0), (y1, r1)) = (pair[0], pair[1]);
        for side in 0..SIDES {
            let (a, b) = (ring(y0, r0, side), ring(y0, r0, side + 1));
            let (c, d) = (ring(y1, r1, side), ring(y1, r1, side + 1));
            mesh.push_triangle(a, c, d, ROD_COLOR);
            mesh.push_triangle(a, d, b, ROD_COLOR);
        }
    }
    for side in 0..SIDES {
        let top = Vec3::Y * length;
        mesh.push_triangle(top, ring(length, ROD_HEAD, side + 1), ring(length, ROD_HEAD, side), ROD_COLOR);
    }
    mesh.shadowless = true;
    mesh
}

/// Which point of the rod is in the fist at `time`, from a list of (seconds, meters).
fn rod_grip_at(keys: &[(f32, f32)], time: f32) -> f32 {
    let Some(&(_, first)) = keys.first() else { return 0.0 };
    let mut value = first;
    for pair in keys.windows(2) {
        let ((t0, v0), (t1, v1)) = (pair[0], pair[1]);
        if time >= t0 {
            value = v0 + (v1 - v0) * ((time - t0) / (t1 - t0).max(1e-4)).clamp(0.0, 1.0);
        }
    }
    value
}

fn rod_place(grip: f32) -> Transform {
    Transform {
        position: IN_FIST - FIST_AXIS * grip,
        rotation: Quat::from_rotation_arc(Vec3::Y, FIST_AXIS.normalize()),
        scale: Vec3::ONE,
    }
}

/// How far the hand has slid along to hold the gun the other way, from 0 to 1.
fn grounded_share(grip: &GunGrip, time: f32) -> f32 {
    let share = |(from, to): (f32, f32)| smooth((time - from) / (to - from).max(1e-4));
    share(grip.slide_in) - share(grip.slide_out)
}

/// The arm a one-handed gun leaves free.
const FREE_ARM: &str = "clavicle.L";

impl Sandbox {
    fn load_guns(&mut self, ctx: &mut Context) {
        if self.guns.hole_mesh.is_some() {
            return;
        }
        self.guns.effects = Effects {
            flash: load(effect!("gun_flash")),
            pan: load(effect!("gun_pan")),
            trail: load(effect!("gun_trail")),
            impact: load(effect!("bullet_impact")),
            flesh: load(effect!("bullet_flesh")),
            powder: load(effect!("gun_powder")),
        };
        let desc = MeshDesc::Decal { texture: HOLE_PICTURE.into(), size: HOLE_SIZE };
        self.guns.hole_mesh = Some(ctx.world.add_mesh(desc.build_in(Path::new(TEXTURES))));
        self.guns.zoom = 1.0;
    }

    /// Runs the gun in the hand and whatever shots have left in the world.
    pub(crate) fn update_guns(&mut self, ctx: &mut Context, dodging: bool, bag_open: bool, dt: f32) {
        self.run_gun_capture(ctx);
        self.update_gun_marks(ctx, dt);
        let held = self.bag.gun();
        if held != self.guns.held {
            self.gun_changed(ctx, held);
        }
        let catalog = ctx.world.items.clone();
        let item = self.bag.gun_item().and_then(|id| catalog.get(id));
        let (Some(held), Some(item), Some(gun)) = (held, item, item.and_then(|item| item.gun())) else {
            if !self.bag.gear.busy()
                && let Some(animator) = ctx.world.get_mut::<Animator>(self.avatar)
            {
                animator.overlay_leave(None);
            }
            self.ease_view(ctx, 1.0, dt);
            return;
        };
        if std::mem::take(&mut self.guns.reset) {
            self.end_reload(ctx, held, &item.id);
            self.guns.act = Act::Ready;
        }

        let staggered = ctx.world.get::<Health>(self.avatar).is_some_and(|health| health.staggered());
        let scripted = self.guns.script.is_some();
        let can = (self.mode == Mode::Play || scripted)
            && self.dead_for.is_none()
            && !dodging
            && !staggered
            && self.player.as_ref().is_some_and(|player| player.body.grounded && !player.swimming());
        let script = self.gun_script(dt);
        let wants_aim = can && (ctx.input.action(actions::GUARD) || script.aim);
        let pulls = can && (ctx.input.action_just_pressed(actions::ATTACK) || script.fire);
        let reloads = can && (ctx.input.action_just_pressed(RELOAD) || script.reload);

        let (hold_clip, aim_clip) = (format!("{}_hold", gun.clip), format!("{}_aim", gun.clip));
        self.guns.aim_left = (self.guns.aim_left - dt).max(0.0);
        let mut act = self.guns.act;
        match &mut act {
            Act::Ready => {
                if pulls && self.guns.loaded(&item.id) {
                    if self.guns.aim >= AIMED {
                        self.pull_trigger(ctx, held, gun);
                        act = Act::Firing { for_: 0.0, fired: false };
                    } else {
                        act = Act::Raising { left: gun.raise };
                    }
                } else if (pulls || reloads) && !self.guns.loaded(&item.id) {
                    if self.bag.inventory.count_of(&gun.ammo) > 0 {
                        act = self.begin_reload(ctx, gun);
                    } else {
                        ctx.audio.play_pitched(sound!("gun_dry"), 0.7, 1.0);
                        self.hud.notify(ctx, "Sem balas", false);
                    }
                } else if reloads {
                    self.hud.notify(ctx, "Já está carregada", false);
                }
            }
            Act::Raising { left } => {
                *left -= dt;
                if !can {
                    act = Act::Ready;
                } else if *left <= 0.0 {
                    self.pull_trigger(ctx, held, gun);
                    act = Act::Firing { for_: 0.0, fired: false };
                }
            }
            Act::Firing { for_, fired } => {
                *for_ += dt;
                if !*fired && *for_ >= gun.lock_time {
                    *fired = true;
                    self.fire(ctx, held, gun);
                    self.guns.empty.insert(item.id.clone(), true);
                    self.guns.aim_left = AIM_LINGER;
                } else if *fired {
                    let done =
                        ctx.world.get::<Animator>(self.avatar).is_none_or(|animator| animator.overlay_finished());
                    if done {
                        act = Act::Ready;
                    }
                }
            }
            Act::Reloading { .. } => {
                if can {
                    act = self.keep_reloading(ctx, held, item, gun, act, dt);
                } else {
                    self.guns.act = act;
                    self.end_reload(ctx, held, &item.id);
                    act = Act::Ready;
                }
            }
        }
        self.guns.act = act;

        let aiming = wants_aim || self.guns.aim_left > 0.0 || matches!(act, Act::Raising { .. } | Act::Firing { .. });
        let reloading = matches!(act, Act::Reloading { .. });
        if let Some(animator) = ctx.world.get_mut::<Animator>(self.avatar) {
            match act {
                Act::Ready | Act::Raising { .. } if aiming => {
                    animator.overlay_loop(&aim_clip, UPPER_BODY, AIM_UP_FADE);
                }
                Act::Ready => {
                    animator.overlay_loop(&hold_clip, UPPER_BODY, AIM_DOWN_FADE);
                }
                _ => {}
            }
            // A gun held low in one hand leaves the other arm to the legs' clip: it hangs loose
            // standing and swings with the walk, as with the light sword.
            let one_handed = gun.clip.starts_with("pistol");
            let low = matches!(act, Act::Ready) && !aiming;
            animator.overlay_leave((one_handed && low).then_some(FREE_ARM));
        }
        let goal = if aiming && !reloading { 1.0 } else { 0.0 };
        self.guns.aim += (goal - self.guns.aim) * (1.0 - (-AIM_EASE * dt).exp());
        let steady_goal = if aiming || reloading { 1.0 } else { 0.0 };
        self.guns.steady += (steady_goal - self.guns.steady) * (1.0 - (-AIM_EASE * dt).exp());
        let aim = smooth(self.guns.aim);
        let mut pitch = 0.0;
        if let Some(player) = self.player.as_mut() {
            player.aiming = aiming || reloading;
            player.shoulder = goal;
            pitch = player.camera_pitch.clamp(BEND_DOWN, BEND_UP);
        }
        if let Some(animator) = ctx.world.get_mut::<Animator>(self.avatar) {
            animator.set_bend(&AIM_BEND, pitch * aim);
            animator.set_overlay_steady(self.guns.steady);
        }
        self.ease_view(ctx, 1.0 + (gun.zoom - 1.0) * aim, dt);
        self.guns.bloom = (self.guns.bloom - BLOOM_BACK * self.guns.bloom * dt).max(0.0);
        if !bag_open && self.mode == Mode::Play {
            self.draw_gun_hud(ctx, item, gun, aiming);
        }
    }

    /// The field of view: narrower for aiming, and punched wider by a shot.
    fn ease_view(&mut self, ctx: &mut Context, zoom: f32, dt: f32) {
        // Eased here as well, so that putting the gun away while aiming does not jump.
        self.guns.zoom += (zoom - self.guns.zoom) * (1.0 - (-AIM_EASE * 2.0 * dt).exp());
        self.guns.punch *= (-PUNCH_BACK * dt).exp();
        let at_rest = (self.guns.zoom - 1.0).abs() < 1e-3 && self.guns.punch < 1e-3;
        if at_rest {
            if let Some(fov) = self.guns.fov.take() {
                ctx.camera.fov_y = fov;
            }
            return;
        }
        let fov = *self.guns.fov.get_or_insert(ctx.camera.fov_y);
        ctx.camera.fov_y = fov * self.guns.zoom + self.guns.punch;
    }

    fn gun_changed(&mut self, ctx: &mut Context, held: Option<Entity>) {
        self.guns.act = Act::Ready;
        self.guns.aim = 0.0;
        self.guns.aim_left = 0.0;
        self.guns.held = held;
        if let Some(player) = self.player.as_mut() {
            player.aiming = false;
            player.shoulder = 0.0;
        }
        let Some(held) = held else {
            if let Some(animator) = ctx.world.get_mut::<Animator>(self.avatar) {
                animator.overlay_stop(AIM_DOWN_FADE);
                animator.set_bend(&AIM_BEND, 0.0);
                animator.set_overlay_steady(0.0);
            }
            return;
        };
        self.load_guns(ctx);
        let loaded = self.bag.gun_item().is_none_or(|id| self.guns.loaded(id));
        if let Some(animator) = ctx.world.get_mut::<Animator>(held) {
            animator.replay_once(if loaded { "Engatilhada" } else { "Disparada" }, 0.0);
        }
        ctx.audio.play_pitched(sound!("gun_cock"), 0.3, 1.2);
    }

    fn pull_trigger(&mut self, ctx: &mut Context, held: Entity, gun: &GunDesc) {
        if let Some(animator) = ctx.world.get_mut::<Animator>(held) {
            animator.replay_once("Disparar", 0.0);
        }
        ctx.audio.play_pitched(sound!("gun_flint"), 0.8, 1.0);
        let pan = ctx.world.transform(held).map(|place| place.matrix().transform_point3(gun.pan));
        if let (Some(look), Some(pan)) = (self.guns.effects.pan.clone(), pan) {
            ctx.world.play_effect(&look, pan);
        }
    }

    /// The charge goes off: where the shot goes, what it hits, and what that all looks like.
    fn fire(&mut self, ctx: &mut Context, held: Entity, gun: &GunDesc) {
        let place = ctx.world.transform(held).unwrap_or_default();
        let muzzle = place.matrix().transform_point3(gun.muzzle);
        let barrel = (place.rotation * Vec3::X).normalize_or_zero();
        let look = ctx.camera.forward();
        let eye = ctx.camera.position;
        let moving = self.player.as_ref().is_some_and(|player| player.velocity().length() > 0.5);
        let mut rng = self.guns.rng();

        // A shot goes for what's under the crosshair, give or take how steady the gun is.
        let spread = if self.guns.aim >= AIMED { gun.aimed_spread } else { gun.spread };
        let spread = spread * if moving { MOVING_SPREAD } else { 1.0 };
        let right = look.cross(Vec3::Y).normalize_or_zero();
        let up = right.cross(look);
        let (angle, off) = (rng.range(0.0, std::f32::consts::TAU), rng.range(0.0, 1.0_f32).sqrt());
        let off = off * spread.to_radians().tan();
        let view_line = (look + (right * angle.cos() + up * angle.sin()) * off).normalize_or_zero();
        // Whatever is behind the character, between it and the camera, isn't what it's shooting at.
        let body = self.player.as_ref().map_or(eye, |player| player.body.center());
        let skip = (body - eye).dot(look).max(0.0) + 0.6;
        let from_view = eye + look * skip;
        let aim_point = ctx
            .physics
            .raycast_scenery(from_view, view_line, gun.range)
            .map_or(from_view + view_line * gun.range, |hit| hit.point);
        let mut line = (aim_point - muzzle).normalize_or_zero();
        if line.dot(look) < MOST_OFF_VIEW {
            line = view_line;
        }
        let shot = ctx.world.shoot(
            &mut ctx.physics,
            muzzle,
            line,
            ShotDesc {
                range: gun.range,
                damage: gun.damage,
                full_damage_to: gun.full_damage_to,
                impulse: IMPULSE * gun.power,
                shooter: Some(self.avatar),
            },
        );

        // ---- at the gun
        if let Some(effect) = &self.guns.effects.flash {
            let mut system = ParticleSystem::new(pointed(effect, barrel), Vec3::ZERO, gun.power, rng.next_u64());
            system.despawn_when_done = true;
            ctx.world.ecs.spawn((Transform::from_position(muzzle), system));
        }
        let light = PointLight {
            position: Vec3::ZERO,
            color: Vec3::new(1.0, 0.72, 0.42),
            intensity: FLASH_LIGHT * gun.power,
            radius: FLASH_RADIUS * gun.power.sqrt(),
            flicker: 0.0,
            night_only: false,
        };
        let entity = ctx.world.ecs.spawn((Transform::from_position(muzzle), Light(light)));
        self.guns.flashes.push(Flash { entity, left: FLASH_SECONDS, power: gun.power });
        let shot_sound = if gun.power > 1.2 { sound!("musket_shot") } else { sound!("pistol_shot") };
        ctx.audio.play_pitched(shot_sound, 1.0, rng.range(0.96, 1.04));
        ctx.world.scare_fauna(muzzle, SCARES * gun.power);
        if let Some(animator) = ctx.world.get_mut::<Animator>(self.avatar) {
            animator.overlay_once(&format!("{}_fire", gun.clip), UPPER_BODY, 0.03);
        }
        if let Some(player) = self.player.as_mut() {
            let aside = gun.recoil.y * rng.jitter(1.0);
            player.recoil(gun.recoil.x.to_radians(), aside.to_radians());
            player.shake(gun.shake);
            let back = Vec3::new(-look.x, 0.0, -look.z).normalize_or_zero();
            player.push(back * gun.shove);
        }
        self.guns.punch = PUNCH * gun.power;
        self.guns.bloom = BLOOM * gun.power;

        // ---- on the way
        let distance = shot.end.distance(muzzle);
        if let Some(puff) = self.guns.effects.trail.clone() {
            for at in 0..TRAIL_PUFFS {
                let along = 1.0 + at as f32 * TRAIL_EVERY;
                if along < distance {
                    ctx.world.play_effect(&puff, muzzle + line * along);
                }
            }
        }
        if shot.animals > 0 {
            self.guns.hit_mark = 0.2;
        }

        // ---- where it ended
        match shot.target {
            ShotTarget::Nothing | ShotTarget::Water => {}
            ShotTarget::Scenery | ShotTarget::Thing(_) => {
                if let Some(effect) = &self.guns.effects.impact {
                    ctx.world.play_effect(&pointed(effect, shot.normal), shot.end);
                }
                let ricochet = rng.range(0.0, 1.0) < 0.22;
                let sound = if ricochet { sound!("bullet_ricochet") } else { sound!("bullet_hit") };
                ctx.audio.play_pitched_at(sound, 0.95, rng.range(0.9, 1.12), Emitter::at(shot.end));
                ctx.world.scare_fauna(shot.end, 6.0);
                if shot.target == ShotTarget::Scenery {
                    self.make_hole(ctx, shot.end, shot.normal, &mut rng);
                }
            }
            ShotTarget::Hurt { target, killed, .. } => {
                if let Some(effect) = &self.guns.effects.flesh {
                    ctx.world.play_effect(&pointed(effect, shot.normal), shot.end);
                }
                ctx.audio.play_at(HIT_SOUND, 0.9, Emitter::at(shot.end));
                ctx.audio.play_pitched_at(sound!("squelch"), 0.5, rng.range(0.9, 1.1), Emitter::at(shot.end));
                self.guns.hit_mark = 0.26;
                self.guns.killed = killed;
                if !killed && ctx.world.get::<Fighter>(target).is_some() {
                    self.foe = Some((target, FOE_BAR_SECONDS));
                }
            }
        }
    }

    /// The mark a ball leaves on something that stays where it is.
    fn make_hole(&mut self, ctx: &mut Context, at: Vec3, normal: Vec3, rng: &mut Rng) {
        let Some(mesh) = self.guns.hole_mesh else { return };
        let turn =
            Quat::from_rotation_arc(Vec3::Y, normal) * Quat::from_rotation_y(rng.range(0.0, std::f32::consts::TAU));
        let place = Transform { position: at, rotation: turn, scale: Vec3::splat(rng.range(0.85, 1.2)) };
        let entity = ctx.world.spawn(mesh, place);
        self.guns.holes.push_back(Hole { entity, age: 0.0, place });
        // The oldest of too many starts to close up now, instead of disappearing.
        let extra = self.guns.holes.len().saturating_sub(MOST_HOLES);
        for hole in self.guns.holes.iter_mut().take(extra) {
            hole.age = hole.age.max(HOLE_SECONDS - HOLE_CLOSES);
        }
    }

    /// What stays in the world after a shot, with or without a gun in the hand.
    fn update_gun_marks(&mut self, ctx: &mut Context, dt: f32) {
        self.guns.hit_mark = (self.guns.hit_mark - dt).max(0.0);
        for flash in &mut self.guns.flashes {
            flash.left -= dt;
            let share = (flash.left / FLASH_SECONDS).clamp(0.0, 1.0);
            if let Ok(light) = ctx.world.ecs.query_one_mut::<&mut Light>(flash.entity) {
                light.0.intensity = FLASH_LIGHT * flash.power * share * share;
            }
        }
        self.guns.flashes.retain(|flash| {
            let goes = flash.left <= 0.0;
            if goes {
                let _ = ctx.world.ecs.despawn(flash.entity);
            }
            !goes
        });
        for hole in &mut self.guns.holes {
            hole.age += dt;
            let left = HOLE_SECONDS - hole.age;
            if left < HOLE_CLOSES
                && let Ok(transform) = ctx.world.ecs.query_one_mut::<&mut Transform>(hole.entity)
            {
                transform.scale = hole.place.scale * smooth(left / HOLE_CLOSES).max(0.001);
            }
        }
        while self.guns.holes.front().is_some_and(|hole| hole.age >= HOLE_SECONDS) {
            if let Some(hole) = self.guns.holes.pop_front() {
                ctx.world.despawn(hole.entity, &mut ctx.physics);
            }
        }
    }

    fn begin_reload(&mut self, ctx: &mut Context, gun: &GunDesc) -> Act {
        if let Some(animator) = ctx.world.get_mut::<Animator>(self.avatar) {
            animator.overlay_once(&format!("{}_reload", gun.clip), UPPER_BODY, 0.2);
        }
        self.guns.aim_left = 0.0;
        Act::Reloading { for_: 0.0, ball: None, rod: None }
    }

    /// One frame of loading. Each thing happens when the clip gets to it.
    fn keep_reloading(
        &mut self,
        ctx: &mut Context,
        held: Entity,
        item: &ItemDesc,
        gun: &GunDesc,
        act: Act,
        dt: f32,
    ) -> Act {
        let Act::Reloading { for_, mut ball, mut rod } = act else { return act };
        let now = for_ + dt;
        let plan = &gun.reload;
        let passed = |time: f32| for_ < time && now >= time;

        if passed(plan.ball.0)
            && let Some(mesh) = ctx.world.item_mesh(&gun.ammo)
        {
            let place = Transform { position: IN_FINGERS, rotation: Quat::IDENTITY, scale: Vec3::splat(BALL_SCALE) };
            ball = Some(ctx.world.attach(self.avatar, "hand.L", mesh, place));
            ctx.audio.play_pitched(sound!("ball_drop"), 0.2, 1.4);
        }
        if passed(plan.ball.1) {
            if let Some(ball) = ball.take() {
                ctx.world.despawn(ball, &mut ctx.physics);
            }
            ctx.audio.play_pitched(sound!("ball_drop"), 0.8, 1.0);
            let muzzle = ctx.world.transform(held).map(|place| place.matrix().transform_point3(gun.muzzle));
            if let (Some(look), Some(muzzle)) = (self.guns.effects.powder.clone(), muzzle) {
                ctx.world.play_effect(&look, muzzle);
            }
        }
        if passed(plan.rod.0) {
            let key = plan.rod_length.to_bits();
            let mesh = match self.guns.rods.get(&key) {
                Some(&mesh) => mesh,
                None => {
                    let mesh = ctx.world.add_mesh(rod_mesh(plan.rod_length));
                    self.guns.rods.insert(key, mesh);
                    mesh
                }
            };
            rod = Some(ctx.world.attach(self.avatar, "hand.L", mesh, rod_place(rod_grip_at(&plan.rod_grip, now))));
            if let Some(animator) = ctx.world.get_mut::<Animator>(held) {
                animator.replay_once("Sem vareta", 0.0);
            }
            ctx.audio.play_pitched(sound!("ramrod"), 0.5, 1.25);
        }
        if let Some(rod) = rod
            && let Ok(attached) = ctx.world.ecs.query_one_mut::<&mut Attached>(rod)
        {
            attached.offset = rod_place(rod_grip_at(&plan.rod_grip, now));
        }
        if plan.strokes.iter().any(|&stroke| passed(stroke - 0.1)) {
            ctx.audio.play_pitched(sound!("ramrod"), 0.8, 1.0);
        }
        if passed(plan.rod.1) {
            if let Some(rod) = rod.take() {
                ctx.world.despawn(rod, &mut ctx.physics);
            }
            if let Some(animator) = ctx.world.get_mut::<Animator>(held) {
                animator.replay_once("Disparada", 0.0);
            }
            ctx.audio.play_pitched(sound!("ramrod"), 0.45, 0.8);
        }
        if let Some(grip) = &plan.grounded {
            self.set_grounded(ctx, held, item, grip, grounded_share(grip, now));
        }
        if passed(plan.cock) {
            if let Some(animator) = ctx.world.get_mut::<Animator>(held) {
                animator.replay_once("Engatilhar", 0.0);
            }
            ctx.audio.play_pitched(sound!("gun_cock"), 0.9, 1.0);
            if self.bag.inventory.take_item(&gun.ammo, 1) > 0 {
                self.guns.empty.insert(item.id.clone(), false);
            }
        }
        let done = ctx.world.get::<Animator>(self.avatar).is_none_or(|animator| {
            animator.overlay_finished() || animator.overlay_current() != Some(&format!("{}_reload", gun.clip))
        });
        if done && now > 0.3 {
            return Act::Ready;
        }
        Act::Reloading { for_: now, ball, rod }
    }

    /// Slides the gun in the hand between its two grips.
    fn set_grounded(&mut self, ctx: &mut Context, held: Entity, item: &ItemDesc, grip: &GunGrip, share: f32) {
        let Some(base) = item.hold() else { return };
        let other = Quat::from_rotation_y(grip.turn.y.to_radians())
            * Quat::from_rotation_x(grip.turn.x.to_radians())
            * Quat::from_rotation_z(grip.turn.z.to_radians());
        if let Ok(attached) = ctx.world.ecs.query_one_mut::<&mut Attached>(held) {
            attached.offset.position = base.position.lerp(grip.at, share);
            attached.offset.rotation = base.rotation.slerp(other, share);
        }
    }

    /// Stops loading, with everything back where it was.
    fn end_reload(&mut self, ctx: &mut Context, held: Entity, id: &str) {
        if let Act::Reloading { ball, rod, .. } = self.guns.act {
            for thing in [ball, rod].into_iter().flatten() {
                ctx.world.despawn(thing, &mut ctx.physics);
            }
        }
        let catalog = ctx.world.items.clone();
        if let Some(base) = catalog.get(id).and_then(|item| item.hold())
            && let Ok(attached) = ctx.world.ecs.query_one_mut::<&mut Attached>(held)
        {
            attached.offset = base;
        }
        let loaded = self.guns.loaded(id);
        if let Some(animator) = ctx.world.get_mut::<Animator>(held) {
            animator.replay_once(if loaded { "Engatilhada" } else { "Disparada" }, 0.0);
        }
    }

    fn draw_gun_hud(&mut self, ctx: &mut Context, item: &ItemDesc, gun: &GunDesc, aiming: bool) {
        let catalog = ctx.world.items.clone();
        let icon = self.bag.icon(&catalog, &mut ctx.ui, &item.id);
        let balls = self.bag.inventory.count_of(&gun.ammo);
        let loaded = self.guns.loaded(&item.id);
        let reload_key = crate::hud::first_control(&ctx.input, RELOAD).to_uppercase();
        let progress = match self.guns.act {
            Act::Reloading { for_, .. } => {
                let name = format!("{}_reload", gun.clip);
                let seconds = ctx.world.get::<Animator>(self.avatar).and_then(|animator| {
                    let skeleton = animator.skeleton();
                    skeleton.clip(&name).map(|at| skeleton.clips[at].duration)
                });
                Some((for_ / seconds.unwrap_or(1.0)).clamp(0.0, 1.0))
            }
            _ => None,
        };
        let fov = ctx.camera.fov_y;
        let ui = &mut ctx.ui;
        let (dim, warn) = (crate::hud::equipped_dim(), crate::hud::equipped_warn());
        let (state, color) = match (progress.is_some(), loaded) {
            (true, _) => ("CARREGANDO...".to_string(), dim),
            (false, true) => (format!("CARREGADA · {balls} BALAS"), ui.theme.text),
            (false, false) if balls > 0 => (format!("VAZIA · {balls} BALAS"), warn),
            (false, false) => ("VAZIA · SEM BALAS".to_string(), warn),
        };
        let mut lines = vec![(state, color)];
        if !loaded && progress.is_none() {
            lines.push((format!("{reload_key}: RECARREGAR"), dim));
        }
        let covered = progress.map_or(0.0, |share| 1.0 - share);
        let equipped = crate::hud::Equipped { icon, name: &item.name, lines: &lines, covered, alpha: 1.0 };
        crate::hud::draw_equipped(ui, &equipped);

        // The crosshair opens with how far a shot can stray, as it would look on the screen.
        let raised = self.guns.aim > 0.15 || aiming;
        if !raised {
            return;
        }
        let middle = ui.at(Anchor::Center, Vec2::ZERO);
        let fade = smooth(self.guns.aim * 2.0);
        let strays = gun.aimed_spread + (gun.spread - gun.aimed_spread) * (1.0 - self.guns.aim) + self.guns.bloom;
        let gap = 5.0 + strays.to_radians().tan() / (fov * 0.5).tan() * ui.size().y * 0.5;
        let line = Vec4::new(0.97, 0.92, 0.82, 0.95 * fade);
        let shadow = Vec4::new(0.02, 0.01, 0.02, 0.7 * fade);
        for (long, offset) in [
            (Vec2::new(8.0, 2.0), Vec2::new(-(gap + 4.0), 0.0)),
            (Vec2::new(8.0, 2.0), Vec2::new(gap + 4.0, 0.0)),
            (Vec2::new(2.0, 8.0), Vec2::new(0.0, -(gap + 4.0))),
            (Vec2::new(2.0, 8.0), Vec2::new(0.0, gap + 4.0)),
            (Vec2::new(2.0, 2.0), Vec2::ZERO),
        ] {
            ui.rect(Rect::anchored(middle + offset, long + Vec2::splat(2.0), Anchor::Center), shadow);
            ui.rect(Rect::anchored(middle + offset, long, Anchor::Center), line);
        }
        if self.guns.hit_mark > 0.0 {
            let color = if self.guns.killed { Vec4::new(1.0, 0.25, 0.15, 1.0) } else { Vec4::new(1.0, 0.97, 0.9, 1.0) };
            let color = color * Vec4::new(1.0, 1.0, 1.0, smooth(self.guns.hit_mark / 0.1));
            let out = gap + 7.0;
            for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                for step in 0..3 {
                    let at = middle + Vec2::new(x, y) * (out + step as f32 * 2.0);
                    ui.rect(Rect::anchored(at, Vec2::splat(2.0), Anchor::Center), color);
                }
            }
        }
    }

    fn run_gun_capture(&mut self, ctx: &mut Context) {
        let Some((what, frame)) = &self.guns.scheduled else { return };
        if ctx.time.frame() < *frame {
            return;
        }
        let what = what.clone();
        self.guns.scheduled = None;
        let (item, what) = what.split_once(':').unwrap_or((what.as_str(), "hold"));
        let catalog = ctx.world.items.clone();
        self.bag.put_in_hand(&catalog, item);
        if let Some(ammo) = catalog.get(item).and_then(|desc| desc.gun()).map(|gun| gun.ammo.clone()) {
            self.bag.inventory.add(&catalog, &ammo, 5);
        }
        if what == "reload" {
            self.guns.empty.insert(item.to_owned(), true);
        }
        self.guns.script = Some(Script { what: what.to_owned(), age: 0.0, done: false });
    }

    /// What a capture wants to happen this frame.
    fn gun_script(&mut self, dt: f32) -> ScriptAsks {
        let Some(script) = &mut self.guns.script else { return ScriptAsks::default() };
        script.age += dt;
        let due = script.age > 0.8 && !script.done;
        let mut asks = ScriptAsks { aim: matches!(script.what.as_str(), "aim" | "fire"), ..Default::default() };
        if due {
            match script.what.as_str() {
                "fire" => asks.fire = true,
                "reload" => asks.reload = true,
                _ => {}
            }
            script.done = true;
        }
        asks
    }
}

#[derive(Default)]
struct ScriptAsks {
    aim: bool,
    fire: bool,
    reload: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DEFAULT_SCENE;
    use grimora::EngineConfig;
    use std::path::PathBuf;

    const DT: f32 = 1.0 / 60.0;

    struct Folder(PathBuf);

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn start(name: &str) -> (Sandbox, Context, Folder) {
        let dir = std::env::temp_dir().join(format!("grimora_guns_{name}_{}", std::process::id()));
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
    }

    fn hold_gun(game: &mut Sandbox, ctx: &mut Context, id: &str, balls: u32) {
        let catalog = ctx.world.items.clone();
        game.bag.put_in_hand(&catalog, id);
        game.bag.inventory.add(&catalog, "bala_de_chumbo", balls);
        frames(game, ctx, 3);
        assert!(game.bag.gun().is_some(), "{id} is in the hand");
    }

    #[test]
    fn each_gun_has_its_clips_and_moving_parts() {
        let (mut game, mut ctx, _dir) = start("clips");
        for id in ["pistola", "mosquete"] {
            hold_gun(&mut game, &mut ctx, id, 0);
            let catalog = ctx.world.items.clone();
            let gun = catalog.get(id).and_then(|item| item.gun()).unwrap();
            let body = ctx.world.get::<Animator>(game.avatar).unwrap();
            for part in ["hold", "aim", "fire", "reload"] {
                let name = format!("{}_{part}", gun.clip);
                assert!(body.skeleton().clip(&name).is_some(), "the character has {name}");
            }
            let held = ctx.world.get::<Animator>(game.bag.gun().unwrap()).unwrap();
            for clip in ["Engatilhada", "Disparar", "Disparada", "Sem vareta", "Engatilhar"] {
                assert!(held.skeleton().clip(clip).is_some(), "{id} has {clip}");
            }
        }
    }

    #[test]
    fn the_barrel_points_where_the_crosshair_is() {
        let (mut game, mut ctx, _dir) = start("aim");
        for (id, most) in [("pistola", 2.0), ("mosquete", 2.0)] {
            hold_gun(&mut game, &mut ctx, id, 0);
            game.guns.script = Some(Script { what: "aim".into(), age: 0.0, done: false });
            for pitch in [-0.3, 0.0, 0.3] {
                game.player.as_mut().unwrap().camera_pitch = pitch;
                frames(&mut game, &mut ctx, 90);
                let held = ctx.world.transform(game.bag.gun().unwrap()).unwrap();
                let barrel = held.rotation * Vec3::X;
                let look = ctx.camera.forward();
                let angle = barrel.angle_between(look).to_degrees();
                assert!(angle < most, "{id} at pitch {pitch}: {angle} degrees off");
            }
            game.guns.script = None;
        }
    }

    #[test]
    fn a_shot_empties_it_and_a_ball_loads_it_again() {
        let (mut game, mut ctx, _dir) = start("cycle");
        hold_gun(&mut game, &mut ctx, "pistola", 2);
        assert!(game.guns.loaded("pistola"), "a new gun comes loaded");

        press(&mut game, &mut ctx, KeyCode::KeyF);
        frames(&mut game, &mut ctx, 90);
        assert!(!game.guns.loaded("pistola"), "it went off");
        assert_eq!(game.guns.act, Act::Ready);
        assert_eq!(game.bag.inventory.count_of("bala_de_chumbo"), 2, "firing doesn't take balls, loading does");

        press(&mut game, &mut ctx, KeyCode::KeyR);
        assert!(matches!(game.guns.act, Act::Reloading { .. }));
        frames(&mut game, &mut ctx, 60 * 4);
        assert_eq!(game.guns.act, Act::Ready, "it finished loading");
        assert!(game.guns.loaded("pistola"));
        assert_eq!(game.bag.inventory.count_of("bala_de_chumbo"), 1);
    }

    #[test]
    fn rolling_stops_the_loading_without_using_the_ball() {
        let (mut game, mut ctx, _dir) = start("cancel");
        hold_gun(&mut game, &mut ctx, "mosquete", 1);
        game.guns.empty.insert("mosquete".into(), true);
        press(&mut game, &mut ctx, KeyCode::KeyR);
        frames(&mut game, &mut ctx, 130);
        let Act::Reloading { rod, .. } = game.guns.act else { panic!("it's loading: {:?}", game.guns.act) };
        let rod = rod.expect("the rod is in the hand by now");
        press(&mut game, &mut ctx, KeyCode::ControlLeft);
        frames(&mut game, &mut ctx, 3);
        assert_eq!(game.guns.act, Act::Ready, "the roll stopped it");
        assert!(ctx.world.get::<Attached>(rod).is_none(), "the rod went back");
        assert_eq!(game.bag.inventory.count_of("bala_de_chumbo"), 1);
        assert!(!game.guns.loaded("mosquete"));
    }

    #[test]
    fn with_no_balls_it_only_clicks() {
        let (mut game, mut ctx, _dir) = start("dry");
        hold_gun(&mut game, &mut ctx, "pistola", 0);
        game.guns.empty.insert("pistola".into(), true);
        press(&mut game, &mut ctx, KeyCode::KeyR);
        assert_eq!(game.guns.act, Act::Ready);
    }

    #[test]
    fn the_shot_hits_the_first_thing_on_the_line() {
        let (mut game, mut ctx, _dir) = start("line");
        let foe = ctx.world.find("hostile").expect("the scene has an enemy");
        let at = ctx.world.transform(foe).unwrap().position + Vec3::Y;
        let before = ctx.world.get::<Health>(foe).unwrap().current;
        let desc =
            ShotDesc { range: 30.0, damage: 10.0, full_damage_to: 30.0, impulse: 0.0, shooter: Some(game.avatar) };
        let from = at + Vec3::new(5.0, 0.0, 0.0);
        let shot = ctx.world.shoot(&mut ctx.physics, from, at - from, desc);
        assert!(matches!(shot.target, ShotTarget::Hurt { target, damage: 10.0, .. } if target == foe), "{shot:?}");
        assert_eq!(ctx.world.get::<Health>(foe).unwrap().current, before - 10.0);
        assert!(shot.end.distance(at) < 1.0, "it stopped there: {}", shot.end);

        // Straight down from there: it reaches the ground, and says which way the ground faces.
        let above = from + Vec3::Y * 2.0;
        let shot = ctx.world.shoot(&mut ctx.physics, above, Vec3::NEG_Y, desc);
        assert_eq!(shot.target, ShotTarget::Scenery);
        assert!(shot.normal.y > 0.5, "{shot:?}");

        // Up in the air there's nothing to hit.
        let shot = ctx.world.shoot(&mut ctx.physics, above + Vec3::Y * 100.0, Vec3::Y, desc);
        assert_eq!(shot.target, ShotTarget::Nothing);
        frames(&mut game, &mut ctx, 1);
    }
}
