//! Cattle: the herder's bulls, cows and calves. By night they lie in the pen, by day they go
//! out through its gate to graze nearby and come back at dusk, the oldest cow first. Every
//! hoof is planted on the ground and lifted in turn, the neck goes down to the grass, the jaw
//! chews the cud, the tail swats flies, and they lie down front first and get up back first.
//!
//! Each one has its own body, out of the model's shape keys (bull, calf, build, legs, horns)
//! and coats, with the joints moved to match (`Bovino_body.ron`, from
//! `tools/extract_bovino.py`). A calf keeps by its mother and suckles, a cow lows for her calf
//! and the calf answers. A blow sends the herd running and makes the bull paw the ground and
//! charge at whoever did it.

use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, TAU};
use std::path::Path;
use std::sync::Arc;

use glam::{Mat3, Mat4, Quat, Vec2, Vec3};
use grimora_audio::Emitter;
use grimora_core::{Animator, JointTransform, ParticleEffect, Rng, Skeleton, Transform};
use grimora_physics::{BodyHandle, ColliderHandle, JointHandle, LimbDesc, LimbShape, Physics, SocketDesc};
use grimora_render::{DrawItem, MeshHandle};
use grimora_scene::{SceneFauna, SpeciesDesc};
use serde::Deserialize;

use super::ecology::{Ledger, Population};
use super::encounter::{BodyPose, Carcass, FaunaEvent, PlayerBody, Wake};
use super::{Place, Threat, flat};
use crate::world::{MeshId, World};

/// The body, for keeping them apart: circles this far ahead and behind the middle, and their
/// radius, for a grown one.
const BODY_ALONG: f32 = 0.6;
const BODY_RADIUS: f32 = 0.36;
/// Past this, nothing is drawn or posed.
const DRAW_DISTANCE: f32 = 130.0;
/// Out to here from the player, each one is solid.
const SOLID_DISTANCE: f32 = 40.0;
/// Speeds, in m/s.
const WALK: f32 = 1.05;
const AMBLE: f32 = 0.35;
const RUN: f32 = 6.2;
const CHARGE: f32 = 7.8;
const FROLIC: f32 = 3.4;
/// Radians per second they turn, walking and running.
const TURN_WALK: f32 = 1.2;
const TURN_RUN: f32 = 2.3;
/// One over how long the `lie_down` and `get_up` clips take.
const LIE_SPEED: f32 = 1.0 / 2.4;
/// How fast the `walk` and `run` clips go at a playback rate of 1 (from gen_bovino_keys.py), and
/// from what speed it runs.
const WALK_CLIP: f32 = 0.82;
const RUN_CLIP: f32 = 3.84;
const RUN_FROM: f32 = 2.0;
/// The pen: how far inside its fence they keep, and how far out of it from outside.
const FENCE_MARGIN: f32 = 0.85;
/// Where they'd graze outside, past the gate (only logged for now: they stay in the pen).
const PASTURE_OUT: f32 = 15.0;
/// Fear: how fast it fades, from where they run, and how far it spreads in the herd.
const CALM_SECONDS: f32 = 9.0;
const FLEES_FROM: f32 = 0.38;
const PANIC_SPREAD: f32 = 14.0;
/// Share of a threat's radius they notice: cattle are used to people.
const NOTICE: f32 = 0.5;
/// Smaller scares than this (someone walking or running by) don't bother them.
const BIG_THREAT: f32 = 12.0;
/// How long a bull scrapes the ground before it runs, and how far past the target it goes.
const PAWING: f32 = 2.4;
const OVERRUN: f32 = 6.0;
/// Where the horns reach the target, from the root, along the body (grown bull).
const HORNS_AHEAD: f32 = 1.45;
const HORNS_REACH: f32 = 0.75;
/// Blows each one takes before it falls.
const BULL_BLOWS: f32 = 8.0;
const COW_BLOWS: f32 = 5.0;
const CALF_BLOWS: f32 = 2.0;
/// A blade is tested every frame of a swing: one blow counts once in this long.
const HURT_EVERY: f32 = 0.45;
/// How hard a blow shoves the body (m/s), and how long the `die` clip takes.
const SHOVE: f32 = 1.6;
const DIE_SECONDS: f32 = 1.9;
/// A body lies this long before it's gone.
const BODY_LASTS: f32 = 900.0;

/// Which of the three it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Bull,
    Cow,
    Calf,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Doing {
    /// Standing still, chewing the cud and looking around.
    Stand {
        left: f32,
    },
    /// Head in the grass, a step now and then.
    Graze {
        left: f32,
        step_in: f32,
    },
    /// To a spot, then what comes after.
    Walk {
        to: Vec3,
        then: Then,
    },
    /// Head down at the trough.
    Drink {
        left: f32,
    },
    Lie {
        left: f32,
    },
    /// Lowing, head up.
    Low {
        left: f32,
    },
    /// Running from a point.
    Flee {
        from: Vec3,
        left: f32,
    },
    /// A calf under its mother.
    Nurse {
        left: f32,
    },
    /// A calf running about for the fun of it.
    Frolic {
        left: f32,
        turn: f32,
    },
    /// A bull scraping the ground at someone.
    Paw {
        left: f32,
        scrape_in: f32,
    },
    /// A bull running at someone, and past.
    Charge {
        at: Vec3,
        run: f32,
        hit: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Then {
    Graze,
    Stand,
    Drink,
    Lie,
    Nurse,
}

/// The joints the code moves, for one body.
#[derive(Debug, Clone)]
struct Rig {
    body: usize,
    neck: usize,
    head: usize,
}

#[derive(Debug)]
struct Beast {
    id: u32,
    kind: Kind,
    mother: Option<u32>,
    mesh: MeshId,
    skeleton: Arc<Skeleton>,
    rig: Arc<Rig>,
    position: Vec3,
    facing: Vec3,
    speed: f32,
    rotation: Quat,
    slope: f32,
    doing: Doing,
    via: Vec<Vec3>,
    think_in: f32,
    look_in: f32,
    /// Turning around something in the way: which side, for how long.
    dodge: (f32, f32),
    /// `0..1`: lying down.
    lie: f32,
    lie_goal: f32,
    /// How far the head is turned to look at something, smoothed.
    yaw: f32,
    /// Where it looks, if anywhere.
    look_at: Option<Vec3>,
    fear: f32,
    anger: f32,
    health: f32,
    hunger: f32,
    call_in: f32,
    size: f32,
    shade: f32,
    drawn: bool,
    pose: Vec<JointTransform>,
    matrices: Vec<Mat4>,
    /// Plays the clips of `Bovino_keys.ron`.
    animator: Animator,
    /// After a blow: how long until another counts, how much it still flinches and from where,
    /// and the shove it's still sliding with.
    hurt_for: f32,
    flinch: f32,
    flinch_from: Vec3,
    shove: Vec3,
    /// Seconds since the last blow, while it goes down.
    dying: Option<f32>,
    /// Its body gone limp, near the player: it falls under its own weight.
    ragdoll: Option<BeastRagdoll>,
    /// The clip it was last told to play.
    clip: &'static str,
    /// The solid body: middle, turn, half length, radius.
    body: Option<(Vec3, Quat, f32, f32)>,
    clock: f32,
}

/// The fence the herd sleeps in.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PenPlace {
    pub center: Vec3,
    pub half: Vec2,
    pub yaw: f32,
    /// Along the -Z side, where the gap in the fence is.
    pub gate_x: f32,
}

impl PenPlace {
    fn local(&self, p: Vec3) -> Vec2 {
        let q = Quat::from_rotation_y(-self.yaw) * (p - self.center);
        Vec2::new(q.x, q.z)
    }

    fn world(&self, q: Vec2) -> Vec3 {
        self.center + Quat::from_rotation_y(self.yaw) * Vec3::new(q.x, 0.0, q.y)
    }

    fn inside(&self, p: Vec3, grow: f32) -> bool {
        let q = self.local(p);
        q.x.abs() < self.half.x + grow && q.y.abs() < self.half.y + grow
    }

    /// Just in from the gate, and just out of it.
    fn gate(&self, out: f32) -> Vec3 {
        self.world(Vec2::new(self.gate_x, -self.half.y - out))
    }

    /// In the gap: free to cross the fence line.
    fn in_gate(&self, p: Vec3) -> bool {
        let q = self.local(p);
        (q.x - self.gate_x).abs() < 0.9 && (q.y + self.half.y).abs() < 2.0
    }
}

/// The herder's cattle.
#[derive(Debug)]
pub(super) struct Herd {
    pub species: Arc<SpeciesDesc>,
    beasts: Vec<Beast>,
    pen: PenPlace,
    pasture: Vec3,
    /// Out grazing, or in for the night.
    night: bool,
    rng: Rng,
    puff: Option<ParticleEffect>,
    ledger: Ledger,
    /// Bodies a calf can be born with, out of sight.
    spare_calves: Vec<(MeshId, Arc<Skeleton>, Arc<Rig>, f32)>,
    next_id: u32,
    /// Blows that didn't kill: they hurt, next update.
    struck: Vec<u32>,
    /// Where the player was last seen, for which way a blow comes from.
    player_at: Option<Vec3>,
    /// Bodies that finished falling, for the fauna to keep.
    pub(super) fallen: Vec<Carcass>,
    /// Where ragdolls came to rest, for the death to be told on the next update.
    fallen_events: Vec<Vec3>,
    colliders: Vec<(u32, ColliderHandle)>,
    globals: Vec<Mat4>,
}

impl Population for Herd {
    fn species(&self) -> &SpeciesDesc {
        &self.species
    }

    fn ledger(&mut self) -> &mut Ledger {
        &mut self.ledger
    }

    fn number(&self) -> usize {
        self.beasts.len()
    }

    fn near(&self, point: Vec3, radius: f32) -> bool {
        self.beasts.iter().any(|b| b.position.distance(point) < radius)
    }

    fn shift(&mut self, offset: Vec3) {
        self.pen.center += offset;
        self.pasture += offset;
        for b in &mut self.beasts {
            b.position += offset;
            for v in &mut b.via {
                *v += offset;
            }
            if let Some(at) = &mut b.look_at {
                *at += offset;
            }
            match &mut b.doing {
                Doing::Walk { to, .. } => *to += offset,
                Doing::Flee { from, .. } => *from += offset,
                Doing::Charge { at, .. } => *at += offset,
                _ => {}
            }
        }
    }

    /// A calf, born to a cow nobody is watching.
    fn add(&mut self, _place: &Place, focus: Vec3, young: bool) -> bool {
        if !young {
            return false;
        }
        let mothers: Vec<usize> = (0..self.beasts.len())
            .filter(|&i| self.beasts[i].kind == Kind::Cow && self.beasts[i].position.distance(focus) > 45.0)
            .collect();
        if mothers.is_empty() {
            return false;
        }
        let mother = &self.beasts[mothers[(self.rng.next_u64() % mothers.len() as u64) as usize]];
        let body = if let Some(body) = self.spare_calves.pop() {
            body
        } else if let Some(calf) = self.beasts.iter().find(|b| b.kind == Kind::Calf) {
            (calf.mesh, calf.skeleton.clone(), calf.rig.clone(), calf.size)
        } else {
            return false;
        };
        let at = mother.position + Quat::from_rotation_y(FRAC_PI_2) * mother.facing * 1.2;
        let (mother_id, facing) = (mother.id, mother.facing);
        let id = self.next_id;
        self.next_id += 1;
        let mut calf = new_beast(id, Kind::Calf, body, at, facing, &mut self.rng);
        calf.mother = Some(mother_id);
        self.beasts.push(calf);
        true
    }

    fn take_away(&mut self, focus: Vec3) -> bool {
        let unseen = self.beasts.iter().position(|b| b.kind != Kind::Bull && b.position.distance(focus) > 60.0);
        let Some(index) = unseen else { return false };
        self.beasts.swap_remove(index);
        true
    }

    fn truncate(&mut self, number: usize) {
        self.beasts.truncate(number);
    }

    /// Takes a blow, once a swing: it flinches and is shoved away from whoever struck, and the
    /// last one makes it go down (the `die` clip, then a body).
    fn strike(&mut self, hits: &mut dyn FnMut(Vec3, f32) -> bool, _dead: &mut Vec<Carcass>) {
        let from = self.player_at;
        for b in &mut self.beasts {
            if b.dying.is_some() || b.hurt_for > 0.0 {
                continue;
            }
            let at = b.position + Vec3::Y * 0.45 * b.size;
            if !hits(at, 0.6 * b.size) {
                continue;
            }
            b.hurt_for = HURT_EVERY;
            b.health -= 1.0;
            let away = from.map_or(-b.facing, |p| flat(b.position - p).normalize_or(-b.facing));
            b.flinch = 1.0;
            b.flinch_from = away;
            b.shove = away * SHOVE / b.size.max(0.5);
            if b.health <= 0.0 {
                b.dying = Some(0.0);
                b.lie_goal = 0.0;
                b.speed = 0.0;
                b.via.clear();
                b.look_at = None;
            }
            self.struck.push(b.id);
        }
    }
}

/// What a body is made of: shape keys and joints, as `tools/extract_bovino.py` wrote them.
#[derive(Debug, Deserialize)]
struct BovineBody {
    moves: HashMap<(String, String), (f32, f32, f32)>,
}

/// One body: shape key weights, size and coat.
#[derive(Debug, Clone, Copy)]
struct Shape {
    juvenile: f32,
    male: f32,
    horns: f32,
    horn_length: f32,
    build: f32,
    robust: f32,
    head: f32,
    legs: f32,
    coat: usize,
}

impl Shape {
    fn weights(&self) -> Vec<(&'static str, f32)> {
        let (j, m, h, l) = (self.juvenile, self.male, self.horns, self.horn_length);
        vec![
            ("Juvenil", j),
            ("Macho", m * (1.0 - 0.72 * j)),
            ("OcultarChifres", 1.0 - h * (1.0 - 0.63 * j)),
            ("ComprimentoChifres", h * (1.0 - 0.63 * j) * (l - 1.0)),
            ("OcultarUbere", 1.0 - (1.0 - m) * (1.0 - j)),
            ("MostrarMacho", m),
            ("Robustez", self.robust),
            ("Cabeca", self.head),
            ("Pernas", self.legs),
        ]
    }

    fn size(&self) -> f32 {
        self.build * (1.0 - 0.32 * self.juvenile)
    }

    fn of(kind: Kind, rng: &mut Rng, coat: Option<usize>) -> Self {
        let coat = match coat {
            Some(coat) if rng.f32() < 0.6 => coat,
            _ => (rng.next_u64() % 4) as usize,
        };
        match kind {
            Kind::Bull => Shape {
                juvenile: 0.0,
                male: 1.0,
                horns: 1.0,
                horn_length: rng.range(1.05, 1.35),
                build: rng.range(1.12, 1.22),
                robust: rng.range(0.4, 1.0),
                head: rng.range(-0.3, 0.5),
                legs: rng.range(-0.3, 0.2),
                coat,
            },
            Kind::Cow => Shape {
                juvenile: 0.0,
                male: 0.0,
                horns: if rng.f32() < 0.8 { 1.0 } else { 0.0 },
                horn_length: rng.range(0.65, 1.15),
                build: rng.range(0.92, 1.05),
                robust: rng.range(-0.6, 0.5),
                head: rng.range(-0.5, 0.5),
                legs: rng.range(-0.4, 0.4),
                coat,
            },
            Kind::Calf => Shape {
                juvenile: rng.range(0.75, 1.0),
                male: if rng.f32() < 0.5 { 1.0 } else { 0.0 },
                horns: 1.0,
                horn_length: 0.8,
                build: rng.range(0.95, 1.05),
                robust: rng.range(-0.4, 0.2),
                head: rng.range(-0.3, 0.3),
                legs: rng.range(-0.2, 0.3),
                coat,
            },
        }
    }
}

const COATS: [&str; 4] = ["castanha", "malhada", "ruiva", "cinza"];

/// The base skeleton with each joint moved as the shape keys move the skin around it. The
/// bind stays what it was at rest, so the shaped body still draws as the shape keys made it.
fn shaped_skeleton(base: &Skeleton, moves: &HashMap<(String, String), Vec3>, weights: &[(&str, f32)]) -> Skeleton {
    let mut globals = Vec::new();
    base.global_transforms(&base.rest_pose(), &mut globals);
    let mut shaped = globals.clone();
    for (i, joint) in base.joints.iter().enumerate() {
        let mut moved = Vec3::ZERO;
        for &(key, weight) in weights {
            if let Some(m) = moves.get(&(key.to_string(), joint.name.clone())) {
                moved += *m * weight;
            }
        }
        shaped[i].w_axis += moved.extend(0.0);
    }
    let mut joints = base.joints.clone();
    for (i, joint) in joints.iter_mut().enumerate() {
        let parent = joint.parent.map_or(base.root, |p| shaped[p]);
        let (scale, rotation, translation) = (parent.inverse() * shaped[i]).to_scale_rotation_translation();
        joint.rest = JointTransform { translation, rotation, scale };
        joint.inverse_bind = shaped[i].inverse() * globals[i] * base.joints[i].inverse_bind;
    }
    Skeleton::new(joints, base.root, base.clips.clone())
}

fn rig_of(skeleton: &Skeleton) -> Option<Rig> {
    Some(Rig { body: skeleton.joint("Body")?, neck: skeleton.joint("Neck")?, head: skeleton.joint("Head")? })
}

fn new_beast(
    id: u32,
    kind: Kind,
    (mesh, skeleton, rig, size): (MeshId, Arc<Skeleton>, Arc<Rig>, f32),
    position: Vec3,
    facing: Vec3,
    rng: &mut Rng,
) -> Beast {
    let animator = Animator::new(skeleton.clone());
    Beast {
        id,
        kind,
        mother: None,
        mesh,
        pose: skeleton.rest_pose(),
        skeleton,
        rig,
        position,
        facing,
        speed: 0.0,
        rotation: Quat::IDENTITY,
        slope: 0.0,
        doing: Doing::Stand { left: rng.range(1.0, 6.0) },
        via: Vec::new(),
        think_in: rng.range(0.0, 1.0),
        look_in: 0.0,
        dodge: (0.0, 0.0),
        lie: 0.0,
        lie_goal: 0.0,
        yaw: 0.0,
        look_at: None,
        fear: 0.0,
        anger: 0.0,
        health: match kind {
            Kind::Bull => BULL_BLOWS,
            Kind::Cow => COW_BLOWS,
            Kind::Calf => CALF_BLOWS,
        },
        hunger: rng.range(0.2, 0.8),
        call_in: rng.range(10.0, 60.0),
        size,
        shade: rng.range(0.9, 1.06),
        drawn: false,
        matrices: Vec::new(),
        animator,
        hurt_for: 0.0,
        flinch: 0.0,
        flinch_from: Vec3::Z,
        shove: Vec3::ZERO,
        dying: None,
        ragdoll: None,
        clip: "",
        body: None,
        clock: rng.range(0.0, 100.0),
    }
}

impl World {
    /// Puts the herder's cattle in their pen: a bull, cows and calves, each with a body of its
    /// own. `species` is the cattle file.
    pub(crate) fn add_herd(&mut self, species: &Path, pen: PenPlace, seed: u64) {
        let desc = grimora_scene::FaunaDesc {
            species: species.to_string_lossy().into_owned(),
            position: pen.center,
            radius: pen.half.length(),
            count: 0,
            seed,
        };
        let mut meshes = Vec::new();
        let Some(group) = grimora_scene::load_fauna(&[desc], Path::new(""), &mut meshes).pop() else { return };
        let base = meshes.swap_remove(group.mesh);
        let Some(base_skeleton) = base.skeleton.clone() else {
            log::error!("{}: no skeleton", species.display());
            return;
        };
        let model_dir = species.parent().unwrap_or(Path::new("")).join(&group.species.model);
        let model_dir = model_dir.parent().unwrap_or(Path::new("")).to_path_buf();
        let moves: HashMap<(String, String), Vec3> = match std::fs::read_to_string(model_dir.join("Bovino_body.ron"))
            .map_err(anyhow::Error::from)
            .and_then(|text| ron::from_str::<BovineBody>(&text).map_err(anyhow::Error::from))
        {
            Ok(body) => body.moves.into_iter().map(|(k, (x, y, z))| (k, Vec3::new(x, y, z))).collect(),
            Err(err) => {
                log::error!("Bovino_body.ron: {err:#}");
                HashMap::new()
            }
        };
        let coats: Vec<_> = COATS
            .iter()
            .map(|coat| grimora_scene::load_texture(model_dir.join(format!("bovino/pelagem_{coat}.png"))).ok())
            .collect();
        let mut rng = Rng::new(seed ^ 0xB0_71_10);
        let body_of = |world: &mut World, shape: &Shape| -> Option<(MeshId, Arc<Skeleton>, Arc<Rig>, f32)> {
            let weights = shape.weights();
            let mut mesh = base.morphed(&weights);
            let skeleton = Arc::new(shaped_skeleton(&base_skeleton, &moves, &weights));
            let rig = Arc::new(rig_of(&skeleton)?);
            mesh.skeleton = Some(skeleton.clone());
            if let Some(Some(coat)) = coats.get(shape.coat) {
                for material in &mut mesh.materials {
                    material.base_color_texture = Some(coat.clone());
                }
            }
            Some((world.add_mesh(mesh), skeleton, rig, shape.size()))
        };

        // One bull, three to five cows, a calf for some of them.
        let cows = 3 + (rng.next_u64() % 3) as usize;
        let calves = 1 + (rng.next_u64() % 2) as usize;
        let mut kinds = vec![(Kind::Cow, None); cows];
        kinds.insert(1, (Kind::Bull, None));
        for k in 0..calves {
            kinds.push((Kind::Calf, Some(k * 2 % cows)));
        }
        let mut beasts: Vec<Beast> = Vec::new();
        let mut cow_ids = Vec::new();
        let mut cow_coats = Vec::new();
        for (i, (kind, mother)) in kinds.into_iter().enumerate() {
            let mother_coat = mother.and_then(|m| cow_coats.get(m).copied());
            let shape = Shape::of(kind, &mut rng, mother_coat);
            let Some(body) = body_of(self, &shape) else {
                log::error!("{}: the model has no legs the way cattle have them", species.display());
                return;
            };
            let spot = Vec2::new(
                rng.range(-pen.half.x + 1.5, pen.half.x - 1.5),
                rng.range(-pen.half.y + 1.5, pen.half.y - 1.5),
            );
            let angle = rng.range(0.0, TAU);
            let mut beast =
                new_beast(i as u32, kind, body, pen.world(spot), Vec3::new(angle.sin(), 0.0, angle.cos()), &mut rng);
            if kind == Kind::Cow {
                cow_ids.push(beast.id);
                cow_coats.push(shape.coat);
            }
            beast.mother = mother.and_then(|m| cow_ids.get(m).copied());
            beasts.push(beast);
        }
        let spare_calves = (0..2)
            .filter_map(|_| {
                let shape = Shape::of(Kind::Calf, &mut rng, None);
                body_of(self, &shape)
            })
            .collect();
        let next_id = beasts.len() as u32;
        let pasture = pen.gate(PASTURE_OUT);
        log::info!(
            "Herd: {} cattle ({} cows, {} calves and a bull); pen at {:.1}, pasture at {:.1} (local)",
            beasts.len(),
            cows,
            calves,
            pen.center,
            pasture
        );
        let herd = Herd {
            species: group.species.clone(),
            beasts,
            pen,
            pasture,
            night: false,
            rng,
            puff: group.puff.clone(),
            ledger: Ledger::of(&SceneFauna { count: next_id, ..group }),
            spare_calves,
            next_id,
            struck: Vec::new(),
            player_at: None,
            fallen: Vec::new(),
            fallen_events: Vec::new(),
            colliders: Vec::new(),
            globals: Vec::new(),
        };
        self.fauna.herds.push(herd);
    }

    /// Makes the cattle near the player solid, where they stand. The land calls it each frame.
    pub(crate) fn update_cattle_bodies(&mut self, physics: &mut Physics, dt: f32) {
        let focus = self.fauna.focus;
        let mut bodies = Vec::new();
        for herd in &mut self.fauna.herds {
            let mut done = Vec::new();
            for b in &mut herd.beasts {
                // Down near the player, it goes limp: a ragdoll from the pose it was in.
                if b.dying.is_some_and(|t| t < 0.5)
                    && b.ragdoll.is_none()
                    && b.position.distance(focus) < SOLID_DISTANCE
                    && !b.pose.is_empty()
                {
                    b.ragdoll = Some(BeastRagdoll::new(b, physics));
                }
                let Some(mut ragdoll) = b.ragdoll.take() else { continue };
                ragdoll.time += dt;
                ragdoll.drive(physics);
                ragdoll.pose(b, physics);
                if ragdoll.still(physics) {
                    done.push(b.id);
                }
                b.ragdoll = Some(ragdoll);
            }
            for id in done {
                let Some(at) = herd.beasts.iter().position(|b| b.id == id) else { continue };
                let mut b = herd.beasts.remove(at);
                if let Some(ragdoll) = b.ragdoll.take() {
                    ragdoll.remove(physics);
                }
                let pose = BodyPose::Own { mesh: b.mesh, matrices: b.matrices.clone() };
                let mut body = Carcass::new(b.position, b.rotation, b.size, b.shade, pose);
                body.left = BODY_LASTS;
                body.food = 400.0;
                bodies.push(body);
                herd.fallen_events.push(b.position);
            }
            for herd_body in herd.fallen.drain(..) {
                bodies.push(herd_body);
            }
        }
        self.fauna.carcasses.append(&mut bodies);
        for herd in &mut self.fauna.herds {
            herd.colliders.retain(|&(id, handle)| {
                let keep = herd.beasts.iter().any(|b| b.id == id && b.body.is_some());
                if !keep {
                    physics.remove(handle);
                }
                keep
            });
            for b in &herd.beasts {
                let Some((center, turn, half, radius)) = b.body else { continue };
                let handle = match herd.colliders.iter().find(|(id, _)| *id == b.id) {
                    Some(&(_, handle)) => handle,
                    None => {
                        let handle = physics.add_agent(center, radius, 2.0 * (half + radius));
                        herd.colliders.push((b.id, handle));
                        handle
                    }
                };
                physics.set_collider_pose(handle, center, turn * Quat::from_rotation_x(FRAC_PI_2));
            }
        }
    }
}

/// What every beast knows about the others, this frame.
#[derive(Debug, Clone, Copy)]
struct Seen {
    id: u32,
    position: Vec3,
    facing: Vec3,
    fear: f32,
    fleeing: bool,
    standing: bool,
}

impl Herd {
    /// Blows taken and not yet answered: grows when a strike lands.
    pub(super) fn struck_count(&self) -> usize {
        self.struck.len()
    }

    pub(super) fn population(&self) -> usize {
        self.beasts.len()
    }

    /// Puts them where they'd be at this hour, standing, with nobody having seen them get there.
    pub(super) fn place(&mut self, place: &Place) {
        self.night = place.daylight < 0.45 || place.storm;
        for i in 0..self.beasts.len() {
            let at = self.pen_spot();
            let b = &mut self.beasts[i];
            b.position = at.with_y(place.floor(at.x, at.z));
            b.via.clear();
            b.doing = Doing::Stand { left: 1.0 + i as f32 };
        }
    }

    fn pen_spot(&mut self) -> Vec3 {
        let h = self.pen.half - Vec2::splat(FENCE_MARGIN + 0.6);
        // Not on the trough, at the -X end.
        let q = Vec2::new(self.rng.range(-h.x + 1.4, h.x), self.rng.range(-h.y, h.y));
        self.pen.world(q)
    }

    fn trough_spot(&mut self) -> Vec3 {
        let z = self.rng.range(-1.0, 1.0);
        self.pen.world(Vec2::new(-self.pen.half.x + 2.45, z))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn update(
        &mut self,
        place: &Place,
        physics: &Physics,
        threats: &[Threat],
        focus: Vec3,
        dt: f32,
        wake: &mut Wake,
    ) {
        let dt = dt.min(0.1);
        // They live in the pen: by night and in storms they lie down more.
        self.night = place.daylight < 0.45 || place.storm;
        let player = wake.player;
        let seen: Vec<Seen> = self
            .beasts
            .iter()
            .map(|b| Seen {
                id: b.id,
                position: b.position,
                facing: b.facing,
                fear: b.fear,
                fleeing: matches!(b.doing, Doing::Flee { .. }),
                standing: b.lie < 0.05 && !matches!(b.doing, Doing::Walk { .. } | Doing::Flee { .. }),
            })
            .collect();

        // Blows that didn't kill: a cry, the herd runs, the bull turns on whoever it was.
        self.player_at = player.map(|p| p.feet);
        for id in std::mem::take(&mut self.struck) {
            let Some(b) = self.beasts.iter().find(|b| b.id == id) else { continue };
            let at = b.position;
            let dying = b.dying.is_some();
            wake.events.push(FaunaEvent::Struck {
                species: self.species.name.clone(),
                at: at + Vec3::Y * 0.85 * b.size,
                from: -b.flinch_from,
                killed: dying,
            });
            let pitch = voice_pitch(b.kind) * if dying { 1.0 } else { 1.25 };
            let cry = if dying { self.species.sounds.death.as_ref() } else { self.species.sounds.calls.first() };
            if let Some(file) = cry {
                wake.play_pitched(&self.species, file, pitch, Emitter::loud(at));
            }
            let from = player.map_or(at, |p| p.feet);
            for b in &mut self.beasts {
                let near = b.position.distance(at) < 30.0;
                if b.kind == Kind::Bull {
                    b.anger = b.anger.max(if near { 2.6 } else { 1.4 });
                } else if near {
                    b.fear = 1.0;
                    b.lie_goal = 0.0;
                    b.doing = Doing::Flee { from, left: 9.0 };
                }
            }
        }

        for i in 0..self.beasts.len() {
            {
                let b = &mut self.beasts[i];
                b.hurt_for = (b.hurt_for - dt).max(0.0);
                b.flinch = (b.flinch - dt * 3.0).max(0.0);
                // The shove of a blow, sliding out.
                if b.shove.length_squared() > 1e-4 {
                    let mut next = b.position + b.shove * dt;
                    if self.pen.inside(b.position, 0.0) {
                        let h = self.pen.half - Vec2::splat(FENCE_MARGIN);
                        next = self.pen.world(self.pen.local(next).clamp(-h, h));
                    }
                    b.position = next.with_y(place.floor(next.x, next.z));
                    b.shove *= (-dt * 6.0).exp();
                }
                if let Some(t) = &mut b.dying {
                    *t += dt;
                    continue;
                }
            }
            self.feel(i, &seen, threats, player, dt);
            self.think(i, place, physics, &seen, player, dt, wake);
            self.act(i, place, physics, &seen, player, dt, wake);
        }
        self.keep_apart(place);
        for i in 0..self.beasts.len() {
            self.show(i, place, focus, dt, wake);
        }
        for at in std::mem::take(&mut self.fallen_events) {
            wake.events.push(FaunaEvent::Killed { species: self.species.name.clone(), at });
        }
        // Down and still: a body now.
        let species = &self.species;
        let fallen = &mut self.fallen;
        self.beasts.retain(|b| {
            if b.ragdoll.is_some() || b.dying.is_none_or(|t| t < DIE_SECONDS) || b.matrices.is_empty() {
                return true;
            }
            let pose = BodyPose::Own { mesh: b.mesh, matrices: b.matrices.clone() };
            let mut body = Carcass::new(b.position, b.rotation, b.size, b.shade, pose);
            body.left = BODY_LASTS;
            body.food = 400.0;
            fallen.push(body);
            wake.events.push(FaunaEvent::Killed { species: species.name.clone(), at: b.position });
            false
        });
    }

    /// Bodies never go through each other: each is three circles along its back, and two that
    /// overlap are pushed apart, the one lying down or standing still giving way least.
    fn keep_apart(&mut self, place: &Place) {
        let circles = |b: &Beast| -> [Vec3; 3] {
            let along = b.facing * BODY_ALONG * b.size;
            [b.position + along, b.position, b.position - along]
        };
        let give = |b: &Beast| -> f32 {
            if b.lie > 0.3 {
                0.1
            } else if b.speed > 0.1 {
                1.0
            } else {
                0.5
            }
        };
        for _ in 0..2 {
            for i in 0..self.beasts.len() {
                for j in i + 1..self.beasts.len() {
                    let (a, b) = (&self.beasts[i], &self.beasts[j]);
                    // A calf at its mother's flank is let in a little closer.
                    let family = a.mother == Some(b.id) || b.mother == Some(a.id);
                    let room = BODY_RADIUS * (a.size + b.size) * if family { 0.75 } else { 1.0 };
                    let mut push = Vec3::ZERO;
                    for pa in circles(a) {
                        for pb in circles(b) {
                            let away = flat(pa - pb);
                            let d = away.length();
                            if d < room {
                                let dir = if d > 1e-3 { away / d } else { Vec3::X };
                                push += dir * (room - d);
                            }
                        }
                    }
                    if push == Vec3::ZERO {
                        continue;
                    }
                    let push = push / 3.0;
                    let (ga, gb) = (give(a), give(b));
                    let total = (ga + gb).max(1e-3);
                    let (move_a, move_b) = (push * ga / total, -push * gb / total);
                    for (k, m) in [(i, move_a), (j, move_b)] {
                        let b = &mut self.beasts[k];
                        let mut next = b.position + m;
                        if self.pen.inside(b.position, 0.0) {
                            let h = self.pen.half - Vec2::splat(FENCE_MARGIN);
                            next = self.pen.world(self.pen.local(next).clamp(-h, h));
                        }
                        b.position = next.with_y(place.floor(next.x, next.z));
                    }
                }
            }
        }
    }

    fn feel(&mut self, i: usize, seen: &[Seen], threats: &[Threat], player: Option<PlayerBody>, dt: f32) {
        let b = &mut self.beasts[i];
        b.clock += dt;
        let notice = if b.kind == Kind::Calf { NOTICE * 1.3 } else { NOTICE };
        let mut scare: Option<(f32, Vec3)> = None;
        for threat in threats.iter().filter(|t| t.radius >= BIG_THREAT) {
            let reach = threat.radius * notice;
            let d = flat(threat.position - b.position).length();
            if d < reach {
                let s = 1.0 - d / reach;
                if scare.is_none_or(|(old, _)| s > old) {
                    scare = Some((s, threat.position));
                }
            }
        }
        if let Some((s, from)) = scare
            && b.kind != Kind::Bull
        {
            b.fear = (b.fear + s * dt * 2.2).min(1.0);
            b.look_at = Some(from);
        }
        // Panic runs through the herd.
        for other in seen {
            if other.id != b.id
                && other.fleeing
                && other.fear > 0.6
                && other.position.distance(b.position) < PANIC_SPREAD
                && b.kind != Kind::Bull
            {
                b.fear = b.fear.max(other.fear * 0.85);
            }
        }
        b.fear = (b.fear - dt / CALM_SECONDS).max(0.0);
        b.hunger = (b.hunger + dt / 400.0).min(1.0);

        // Used to people: someone close by gets a look, nothing more; the bull only gets angry
        // when the herd is hit, and calms down with time.
        if let Some(player) = player
            && b.lie < 0.5
            && flat(player.feet - b.position).length() < 4.0
            && (b.clock * 0.37 + b.id as f32).sin() > 0.3
        {
            b.look_at = Some(player.feet);
        }
        if b.kind == Kind::Bull {
            b.anger = (b.anger - dt / 45.0).max(0.0);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn think(
        &mut self,
        i: usize,
        place: &Place,
        physics: &Physics,
        seen: &[Seen],
        player: Option<PlayerBody>,
        dt: f32,
        wake: &mut Wake,
    ) {
        // Lowing, now and then: a calf calls more, and its mother answers.
        {
            let mother = self.beasts[i].mother.and_then(|m| seen.iter().find(|s| s.id == m));
            let lost = mother.is_some_and(|m| m.position.distance(self.beasts[i].position) > 12.0);
            let b = &mut self.beasts[i];
            if lost && b.call_in > 4.0 {
                b.call_in = 3.0;
            }
            b.call_in -= dt;
            let busy = matches!(b.doing, Doing::Flee { .. } | Doing::Charge { .. } | Doing::Nurse { .. });
            if b.call_in <= 0.0 && !busy {
                b.call_in = self.rng.range(25.0, 80.0);
                if b.lie < 0.5 {
                    b.doing = Doing::Low { left: 2.2 };
                    b.speed = 0.0;
                }
                let calls = &self.species.sounds.calls;
                if !calls.is_empty() {
                    let file = calls[(self.rng.next_u64() % calls.len() as u64) as usize].clone();
                    let pitch = voice_pitch(b.kind) * self.rng.range(0.94, 1.06);
                    let mouth = b.position + b.facing * 1.2 * b.size + Vec3::Y * 1.2 * b.size;
                    wake.play_pitched(&self.species, &file, pitch, Emitter::at(mouth));
                }
                if b.kind == Kind::Calf
                    && let Some(mother) = b.mother
                    && let Some(m) = self.beasts.iter_mut().find(|m| m.id == mother)
                {
                    m.call_in = m.call_in.min(self.rng.range(1.0, 2.2));
                }
            }
        }

        // Fear first: a bull doesn't run, it turns on whoever it is.
        let (kind, fear, anger, position) = {
            let b = &self.beasts[i];
            (b.kind, b.fear, b.anger, b.position)
        };
        if kind == Kind::Bull
            && anger > 1.0
            && let Some(player) = player
            && flat(player.feet - position).length() < 32.0
            && !matches!(self.beasts[i].doing, Doing::Paw { .. } | Doing::Charge { .. })
        {
            let b = &mut self.beasts[i];
            b.lie_goal = 0.0;
            b.doing = Doing::Paw { left: PAWING, scrape_in: 0.3 };
            b.look_at = Some(player.feet);
            wake.play(&self.species, self.species.sounds.facing.as_deref(), Emitter::at(position));
            return;
        }
        if kind != Kind::Bull && fear > FLEES_FROM && !matches!(self.beasts[i].doing, Doing::Flee { .. }) {
            let from = self.beasts[i].look_at.unwrap_or(position - self.beasts[i].facing);
            let b = &mut self.beasts[i];
            b.lie_goal = 0.0;
            b.doing = Doing::Flee { from, left: self.rng.range(4.0, 7.0) };
            return;
        }

        let b = &mut self.beasts[i];
        b.think_in -= dt;
        if b.think_in > 0.0 {
            return;
        }
        b.think_in = self.rng.range(0.4, 0.9);
        let done = match &mut b.doing {
            Doing::Stand { left }
            | Doing::Graze { left, .. }
            | Doing::Drink { left }
            | Doing::Lie { left }
            | Doing::Low { left }
            | Doing::Nurse { left }
            | Doing::Frolic { left, .. } => *left <= 0.0,
            Doing::Walk { .. } | Doing::Flee { .. } | Doing::Paw { .. } | Doing::Charge { .. } => false,
        };
        if !done {
            return;
        }
        self.choose(i, place, physics, seen);
    }

    /// What to do next, now that the last thing is done.
    fn choose(&mut self, i: usize, place: &Place, physics: &Physics, seen: &[Seen]) {
        let inside = self.pen.inside(self.beasts[i].position, 0.0);
        let night = self.night;
        // On the wrong side of the fence for the hour: through the gate.
        if !inside {
            let to = self.pen_spot();
            let b = &mut self.beasts[i];
            b.lie_goal = 0.0;
            b.via = if inside {
                vec![self.pen.gate(-1.4), self.pen.gate(1.8)]
            } else {
                vec![self.pen.gate(2.2), self.pen.gate(-1.6)]
            };
            b.doing = Doing::Walk { to, then: Then::Stand };
            return;
        }
        let r = self.rng.f32();
        let kind = self.beasts[i].kind;

        // A calf keeps to its mother, suckles and now and then runs about.
        if kind == Kind::Calf {
            let mother = self.beasts[i].mother.and_then(|m| seen.iter().find(|s| s.id == m)).copied();
            if let Some(m) = mother {
                let far = m.position.distance(self.beasts[i].position);
                let hungry = self.beasts[i].hunger > 0.55;
                let b = &mut self.beasts[i];
                if hungry && m.standing && r < 0.5 {
                    // Beside her flank, by the udder, facing back along her.
                    let side = Quat::from_rotation_y(FRAC_PI_2) * m.facing;
                    let to = m.position + side * 0.55 - m.facing * 0.45;
                    b.lie_goal = 0.0;
                    b.doing = Doing::Walk { to, then: Then::Nurse };
                    return;
                }
                if far > 4.5 {
                    let to = m.position + flat(b.position - m.position).normalize_or(Vec3::X) * 2.0;
                    b.lie_goal = 0.0;
                    b.doing = Doing::Walk { to, then: if night { Then::Lie } else { Then::Graze } };
                    return;
                }
                if !night && r < 0.2 && b.lie < 0.1 {
                    b.doing = Doing::Frolic { left: self.rng.range(2.5, 5.0), turn: self.rng.range(-1.6, 1.6) };
                    return;
                }
                if night || r < 0.45 {
                    b.lie_goal = 1.0;
                    b.doing = Doing::Lie { left: self.rng.range(20.0, 90.0) };
                    return;
                }
                b.doing = Doing::Stand { left: self.rng.range(3.0, 9.0) };
                return;
            }
        }

        let hunger = self.beasts[i].hunger;
        let lying = self.beasts[i].lie > 0.5;
        if night {
            if lying || r < 0.65 {
                let b = &mut self.beasts[i];
                b.lie_goal = 1.0;
                b.doing = Doing::Lie { left: self.rng.range(40.0, 160.0) };
            } else if r < 0.8 {
                let to = self.trough_spot();
                self.beasts[i].doing = Doing::Walk { to, then: Then::Drink };
            } else if r < 0.9 {
                let to = self.pen_spot();
                self.beasts[i].doing = Doing::Walk { to, then: Then::Stand };
            } else {
                self.beasts[i].doing = Doing::Stand { left: self.rng.range(4.0, 14.0) };
            }
            return;
        }
        // By day: mostly grazing, close to the others; a rest in the middle of it.
        if lying && r < 0.6 {
            self.beasts[i].doing = Doing::Lie { left: self.rng.range(20.0, 60.0) };
            return;
        }
        let b = &mut self.beasts[i];
        b.lie_goal = 0.0;
        if r < 0.1 + 0.05 * (1.0 - hunger) {
            b.lie_goal = 1.0;
            b.doing = Doing::Lie { left: self.rng.range(30.0, 90.0) };
            return;
        }
        let me = b.position;
        if r < 0.28 {
            let to = self.pen_spot();
            let clear = physics
                .raycast_scenery(me + Vec3::Y * 0.8, flat(to - me).normalize_or(Vec3::Z), me.distance(to))
                .is_none()
                && place.depth(to.x, to.z).is_none();
            if clear {
                self.beasts[i].doing = Doing::Walk { to, then: Then::Graze };
                return;
            }
        }
        if r < 0.4 {
            self.beasts[i].doing = Doing::Stand { left: self.rng.range(4.0, 15.0) };
            return;
        }
        self.beasts[i].doing =
            Doing::Graze { left: self.rng.range(10.0, 35.0) * (0.5 + hunger), step_in: self.rng.range(1.0, 4.0) };
    }

    #[allow(clippy::too_many_arguments)]
    fn act(
        &mut self,
        i: usize,
        place: &Place,
        physics: &Physics,
        seen: &[Seen],
        player: Option<PlayerBody>,
        dt: f32,
        wake: &mut Wake,
    ) {
        let doing = self.beasts[i].doing;
        let b = &mut self.beasts[i];
        let (mut goal, mut speed, mut turn) = (None, 0.0, TURN_WALK);
        match doing {
            Doing::Stand { left } => {
                b.doing = Doing::Stand { left: left - dt };
            }
            Doing::Low { left } => {
                b.doing = Doing::Low { left: left - dt };
            }
            Doing::Drink { left } => {
                b.hunger = (b.hunger - dt / 200.0).max(0.0);
                b.doing = Doing::Drink { left: left - dt };
                // Facing the trough, at the -X end.
                let trough = self.pen.world(Vec2::new(-self.pen.half.x, 0.0));
                b.facing = turn_towards(b.facing, trough - b.position, 1.0, dt);
            }
            Doing::Lie { left } => {
                b.doing = Doing::Lie { left: left - dt };
                b.lie_goal = 1.0;
            }
            Doing::Graze { left, step_in } => {
                b.hunger = (b.hunger - dt / 160.0).max(0.0);
                let step_in = step_in - dt;
                if step_in < 0.0 {
                    // A step or two forward to fresh grass, the head staying down.
                    goal = Some(b.position + b.facing * 0.9);
                    speed = AMBLE;
                    if step_in < -2.2 {
                        b.doing = Doing::Graze { left: left - dt, step_in: self.rng.range(2.0, 6.0) };
                        b.facing = Quat::from_rotation_y(self.rng.range(-0.5, 0.5)) * b.facing;
                    } else {
                        b.doing = Doing::Graze { left: left - dt, step_in };
                    }
                } else {
                    b.doing = Doing::Graze { left: left - dt, step_in };
                }
            }
            Doing::Walk { to, then } => {
                let target = b.via.first().copied().unwrap_or(to);
                goal = Some(target);
                speed = WALK * b.size.sqrt();
                if flat(target - b.position).length() < 0.7 {
                    if !b.via.is_empty() {
                        b.via.remove(0);
                    } else {
                        b.doing = match then {
                            Then::Graze => Doing::Graze { left: self.rng.range(10.0, 30.0), step_in: 2.0 },
                            Then::Stand => Doing::Stand { left: self.rng.range(2.0, 8.0) },
                            Then::Drink => Doing::Drink { left: self.rng.range(6.0, 14.0) },
                            Then::Lie => {
                                b.lie_goal = 1.0;
                                Doing::Lie { left: self.rng.range(30.0, 100.0) }
                            }
                            Then::Nurse => Doing::Nurse { left: self.rng.range(12.0, 25.0) },
                        };
                        goal = None;
                    }
                }
            }
            Doing::Nurse { left } => {
                b.hunger = (b.hunger - dt / 30.0).max(0.0);
                b.doing = Doing::Nurse { left: left - dt };
                // Under her, facing back the way she faces.
                if let Some(m) = b.mother.and_then(|m| seen.iter().find(|s| s.id == m)) {
                    b.facing = turn_towards(b.facing, -m.facing, 1.5, dt);
                    if !m.standing || m.position.distance(b.position) > 1.6 {
                        b.doing = Doing::Stand { left: 1.0 };
                    }
                }
            }
            Doing::Frolic { left, turn: bend } => {
                b.doing = Doing::Frolic { left: left - dt, turn: bend };
                goal = Some(b.position + Quat::from_rotation_y(bend) * b.facing * 3.0);
                speed = FROLIC;
                turn = TURN_RUN;
            }
            Doing::Flee { from, left } => {
                let away = flat(b.position - from).normalize_or(b.facing);
                // Running away, but towards the others, and not out of where it lives.
                let pull = flat(self.pen.center - b.position) / self.pen.half.min_element().max(1.0);
                goal = Some(b.position + (away + pull * 1.2).normalize_or(away) * 5.0);
                speed = RUN * b.size.sqrt() * (0.6 + 0.4 * b.fear);
                turn = TURN_RUN;
                let left = left - dt;
                b.doing = if left <= 0.0 && b.fear < FLEES_FROM {
                    Doing::Stand { left: self.rng.range(4.0, 10.0) }
                } else {
                    Doing::Flee { from, left }
                };
            }
            Doing::Paw { left, scrape_in } => {
                b.lie_goal = 0.0;
                if let Some(player) = player {
                    b.look_at = Some(player.feet);
                    b.facing = turn_towards(b.facing, flat(player.feet - b.position), 1.6, dt);
                }
                let mut scrape_in = scrape_in - dt;
                if scrape_in <= 0.0 {
                    scrape_in = 0.62;
                    let hoof = b.position + b.rotation * (Vec3::new(-0.24, 0.0, 0.75) * b.size);
                    if let Some(puff) = &self.puff {
                        wake.puffs.push((puff.clone(), hoof));
                    }
                    wake.play(&self.species, self.species.sounds.dig.as_deref(), Emitter::at(hoof));
                }
                let left = left - dt;
                b.doing = if left > 0.0 {
                    Doing::Paw { left, scrape_in }
                } else if let Some(player) = player {
                    wake.play(&self.species, self.species.sounds.facing.as_deref(), Emitter::at(b.position));
                    Doing::Charge { at: player.feet, run: 0.0, hit: false }
                } else {
                    Doing::Stand { left: 2.0 }
                };
            }
            Doing::Charge { at, run, hit } => {
                // It follows the target until it's close, then it's committed.
                let mut at = at;
                if let Some(player) = player
                    && flat(player.feet - b.position).length() > 5.0
                    && !hit
                {
                    at = player.feet;
                }
                let way = flat(at - b.position).normalize_or(b.facing);
                goal = Some(at + way * OVERRUN);
                speed = CHARGE * b.size.sqrt();
                turn = TURN_RUN * 0.8;
                let mut hit = hit;
                if !hit && let Some(player) = player {
                    let horns = b.position + b.facing * HORNS_AHEAD * b.size + Vec3::Y * 0.9 * b.size;
                    if player.touches(horns, HORNS_REACH * b.size) {
                        hit = true;
                        wake.harm(&self.species, horns);
                        b.anger -= 0.7;
                    }
                }
                let run = run + b.speed * dt;
                let past = flat(at - b.position).dot(b.facing) < -OVERRUN * 0.6;
                b.doing = if past || run > 45.0 {
                    b.anger -= 0.35;
                    Doing::Stand { left: self.rng.range(1.5, 3.0) }
                } else {
                    Doing::Charge { at, run, hit }
                };
                if self.rng.f32() < dt * 4.0
                    && let Some(puff) = &self.puff
                {
                    wake.puffs.push((puff.clone(), b.position));
                }
            }
        }

        // Lying down and getting up: nothing moves until it's on its feet.
        let b = &mut self.beasts[i];
        let rate = if b.fear > FLEES_FROM || b.anger > 1.0 { LIE_SPEED * 2.4 } else { LIE_SPEED };
        b.lie += (b.lie_goal - b.lie).clamp(-rate * dt, rate * dt);
        if b.lie > 0.02 {
            speed = 0.0;
        }
        self.travel(i, goal, speed, turn, place, physics, seen, player, dt);
    }

    /// Walks or runs towards `goal`, around the others and whatever is in the way, never
    /// through the pen's fence but at its gate.
    #[allow(clippy::too_many_arguments)]
    fn travel(
        &mut self,
        i: usize,
        goal: Option<Vec3>,
        speed: f32,
        turn: f32,
        place: &Place,
        physics: &Physics,
        seen: &[Seen],
        player: Option<PlayerBody>,
        dt: f32,
    ) {
        let pen = self.pen;
        let b = &mut self.beasts[i];
        let mut target_speed = 0.0;
        // Inside the pen, nothing worth going to is past the fence: a goal out there (running
        // from someone, charging) becomes the nearest point inside, so it runs along the fence
        // instead of into it. Not at the gate, which is the way out.
        let goal = goal.map(|goal| {
            if pen.inside(b.position, 0.0) && !pen.in_gate(b.position) {
                let h = pen.half - Vec2::splat(FENCE_MARGIN + 0.3);
                pen.world(pen.local(goal).clamp(-h, h)).with_y(goal.y)
            } else {
                goal
            }
        });
        if let Some(goal) = goal {
            let mut want = flat(goal - b.position).normalize_or(b.facing);
            // The others, and the player unless it's a charge.
            let charging = matches!(b.doing, Doing::Charge { .. });
            let mut push = Vec3::ZERO;
            for s in seen.iter().filter(|s| s.id != b.id) {
                let away = flat(b.position - s.position);
                let d = away.length();
                let room = 2.3 * b.size;
                if d < room && d > 0.01 {
                    push += away / d * (1.0 - d / room);
                }
            }
            if let Some(p) = player
                && !charging
            {
                let away = flat(b.position - p.feet);
                let d = away.length();
                if d < 2.5 && d > 0.01 {
                    push += away / d * (1.0 - d / 2.5) * 1.5;
                }
            }
            want = (want + push * 0.8).normalize_or(want);
            // Something solid ahead: around it, to one side, for a moment.
            b.look_in -= dt;
            if b.look_in <= 0.0 {
                b.look_in = 0.3;
                let eye = b.position + Vec3::Y * 0.75 * b.size;
                let ahead = (1.6 + b.speed * 0.5) * b.size;
                if let Some(hit) = physics.raycast_scenery(eye, b.facing, ahead)
                    && hit.normal.y < 0.6
                    && !pen.in_gate(b.position)
                {
                    let side = if b.facing.cross(hit.normal).y > 0.0 { 1.0 } else { -1.0 };
                    b.dodge = (side, 0.9);
                }
            }
            if b.dodge.1 > 0.0 {
                b.dodge.1 -= dt;
                want = Quat::from_rotation_y(b.dodge.0 * 1.2) * want;
            }
            b.facing = turn_towards(b.facing, want, turn, dt);
            // Slower in a sharp turn.
            let lined_up = b.facing.dot(want).max(0.0);
            target_speed = speed * (0.25 + 0.75 * lined_up);
            if flat(goal - b.position).length() < 0.4 && !charging {
                target_speed = 0.0;
            }
        }
        let accel = if target_speed > b.speed { 2.2 + b.speed } else { 4.0 };
        b.speed += (target_speed - b.speed).clamp(-accel * dt, accel * dt);
        let mut next = b.position + b.facing * b.speed * dt;
        // Keep to its side of the fence, except at the gate.
        if !pen.in_gate(next) {
            let was_in = pen.inside(b.position, 0.0);
            let q = pen.local(next);
            if was_in {
                let h = pen.half - Vec2::splat(FENCE_MARGIN);
                if q.x.abs() > h.x || q.y.abs() > h.y {
                    next = pen.world(q.clamp(-h, h));
                }
            } else if pen.inside(next, FENCE_MARGIN) {
                let h = pen.half + Vec2::splat(FENCE_MARGIN);
                let (dx, dy) = (h.x - q.x.abs(), h.y - q.y.abs());
                let out = if dx < dy { Vec2::new(h.x * q.x.signum(), q.y) } else { Vec2::new(q.x, h.y * q.y.signum()) };
                next = pen.world(out);
            }
        }
        // Never into deep water.
        if place.depth(next.x, next.z).is_some_and(|d| d > 0.5) {
            next = b.position;
            b.speed *= 0.5;
        }
        // Held back by the fence: the legs go as fast as it really moves.
        let moved = flat(next - b.position).length();
        if dt > 0.0 && moved < b.speed * dt * 0.98 {
            b.speed = moved / dt;
        }
        b.position = next.with_y(place.floor(next.x, next.z));
    }

    /// The pose of one: head, tail, ears and body from what it's doing, then every hoof on
    /// the ground where it was planted.
    fn show(&mut self, i: usize, place: &Place, focus: Vec3, dt: f32, wake: &mut Wake) {
        let puff = self.puff.clone();
        let gallop_puff = self.rng.f32() < dt * 5.0;
        let b = &mut self.beasts[i];
        let distance = b.position.distance(focus);
        b.drawn = distance < DRAW_DISTANCE;
        b.body = None;
        if !b.drawn || b.ragdoll.is_some() {
            return;
        }
        let size = b.size;
        // Along the slope, front to back; upright across it.
        let yaw = Quat::from_rotation_y(b.facing.x.atan2(b.facing.z));
        let reach = 1.0 * size;
        let front = b.position + b.facing * reach;
        let back = b.position - b.facing * reach;
        let slope = ((place.floor(back.x, back.z) - place.floor(front.x, front.z)) / (2.0 * reach)).atan();
        b.slope += (slope - b.slope) * (dt * 6.0).min(1.0);
        b.rotation = yaw * Quat::from_rotation_x(b.slope * 0.85);

        // Where it looks, turned on top of whatever the clip does with the head.
        let mut look = 0.0;
        if let Some(at) = b.look_at {
            let to = b.rotation.inverse() * (at - b.position);
            look = to.x.atan2(to.z).clamp(-1.0, 1.0);
            if flat(at - b.position).length() > 18.0 {
                b.look_at = None;
            }
        }
        let head_busy = matches!(
            b.doing,
            Doing::Graze { .. } | Doing::Drink { .. } | Doing::Nurse { .. } | Doing::Low { .. } | Doing::Charge { .. }
        );
        if b.lie > 0.02 || head_busy {
            look *= 0.3;
        }
        b.yaw += (look - b.yaw) * (dt * 2.5).min(1.0);

        // Which clip (tools/gen_bovino_keys.py wrote them all), and how fast it plays.
        let speed = b.speed / size;
        let quick = if b.fear > FLEES_FROM || b.anger > 1.0 { 2.4 } else { 1.0 };
        let (clip, rate, once): (&'static str, f32, bool) = if b.dying.is_some() {
            ("die", 1.0, true)
        } else if b.lie_goal > 0.5 && b.lie < 0.999 {
            ("lie_down", quick, true)
        } else if b.lie_goal < 0.5 && b.lie > 0.001 {
            ("get_up", quick, true)
        } else if b.lie >= 0.999 {
            ("lie", 1.0, false)
        } else if speed > RUN_FROM {
            let clip = if matches!(b.doing, Doing::Charge { .. }) { "charge" } else { "run" };
            (clip, (speed / RUN_CLIP).clamp(0.6, 1.5), false)
        } else if speed > 0.08 {
            ("walk", (speed / WALK_CLIP).clamp(0.4, 1.8), false)
        } else {
            match b.doing {
                Doing::Graze { .. } => ("graze", 1.0, false),
                Doing::Drink { .. } => ("drink", 1.0, false),
                Doing::Low { .. } => ("low", 1.0, true),
                Doing::Nurse { .. } => ("nurse", 1.0, false),
                Doing::Paw { .. } => ("paw", 1.0, false),
                _ => ("stand", 1.0, false),
            }
        };
        if clip != b.clip {
            let fade = if matches!(clip, "lie_down" | "get_up" | "die") || matches!(b.clip, "lie_down" | "get_up") {
                0.2
            } else {
                0.35
            };
            if once {
                b.animator.replay_once(clip, fade);
            } else {
                b.animator.play(clip, fade);
            }
            b.clip = clip;
        }
        b.animator.speed = rate;
        b.animator.update(dt);
        if speed > RUN_FROM && gallop_puff {
            if let Some(puff) = &puff {
                let behind = b.position - b.facing * 0.8 * size;
                wake.puffs.push((puff.clone(), behind));
            }
        }

        let rig = b.rig.clone();
        let skeleton = b.skeleton.clone();
        b.pose.clear();
        b.pose.extend_from_slice(b.animator.pose());
        let pose = &mut b.pose;
        // A blow throws the body away from it and the head up, for a moment.
        if b.flinch > 0.0 {
            let kick = (b.flinch * std::f32::consts::PI).sin() * b.flinch.sqrt();
            let away = b.rotation.inverse() * b.flinch_from;
            let axis = Vec3::Y.cross(away).normalize_or(Vec3::Z);
            turn_joint(&skeleton, pose, rig.head, Quat::from_rotation_x(-0.35 * kick));
            turn_joint(&skeleton, pose, rig.body, Quat::from_axis_angle(axis, 0.16 * kick));
        }
        turn_joint(&skeleton, pose, rig.head, Quat::from_rotation_y(b.yaw * 0.45));
        turn_joint(&skeleton, pose, rig.neck, Quat::from_rotation_y(b.yaw * 0.55));

        skeleton.global_transforms(pose, &mut self.globals);
        b.matrices.clear();
        b.matrices.extend(self.globals.iter().zip(&skeleton.joints).map(|(g, j)| *g * j.inverse_bind));
        let body_height = (0.95 - 0.5 * b.lie) * size;
        b.body = (distance < SOLID_DISTANCE).then(|| {
            (b.position + b.rotation * Vec3::new(0.0, body_height, 0.15 * size), b.rotation, 0.75 * size, 0.42 * size)
        });
    }

    pub(super) fn push_draws(&self, handles: &[MeshHandle], draws: &mut Vec<DrawItem>, joints: &mut Vec<Mat4>) {
        for b in self.beasts.iter().filter(|b| b.drawn && !b.matrices.is_empty()) {
            let Some(&mesh) = handles.get(b.mesh.0) else { continue };
            joints.extend_from_slice(&b.matrices);
            draws.push(DrawItem {
                mesh,
                transform: Transform { position: b.position, rotation: b.rotation, scale: Vec3::splat(b.size) },
                color: Vec3::splat(b.shade),
                emissive: 0.0,
                joints: Some((joints.len() - b.matrices.len()) as u32),
            });
        }
    }
}

fn voice_pitch(kind: Kind) -> f32 {
    match kind {
        Kind::Bull => 0.74,
        Kind::Cow => 1.0,
        Kind::Calf => 1.55,
    }
}


/// Turns `from` towards `to`, on the ground, no more than `turn` radians per second.
fn turn_towards(from: Vec3, to: Vec3, turn: f32, dt: f32) -> Vec3 {
    let to = flat(to).normalize_or(from);
    let angle = f32::atan2(from.cross(to).y, from.dot(to));
    (Quat::from_rotation_y((turn * dt).min(angle.abs()) * angle.signum()) * from).normalize_or(from)
}

/// The rotation of a joint in the model's space, as the pose has it.
fn world_rotation(skeleton: &Skeleton, pose: &[JointTransform], joint: Option<usize>) -> Quat {
    let mut rotation = Quat::IDENTITY;
    let mut at = joint;
    while let Some(j) = at {
        rotation = pose[j].rotation * rotation;
        at = skeleton.joints[j].parent;
    }
    skeleton.root.to_scale_rotation_translation().1 * rotation
}

/// Turns a joint by `turn`, in the model's space, about where it is.
fn turn_joint(skeleton: &Skeleton, pose: &mut [JointTransform], joint: usize, turn: Quat) {
    let parent = world_rotation(skeleton, pose, skeleton.joints[joint].parent);
    let now = parent * pose[joint].rotation;
    pose[joint].rotation = (parent.inverse() * (turn * now)).normalize();
}


/// A dead one's body as physics: the trunk, neck, head and two pieces a leg, joined at limited
/// joints whose muscles give out over a moment, so it buckles, tips and settles under its own
/// weight, pushed the way the blow came.
#[derive(Debug)]
struct BeastRagdoll {
    /// Each limb's body, and where it was in the world when it went limp.
    limbs: Vec<(BodyHandle, Mat4)>,
    joints: Vec<JointHandle>,
    /// Which limb each bone rides on.
    owner: Vec<usize>,
    /// The bones in the model's space when it went limp.
    start: Vec<Mat4>,
    /// The model's place in the world: fixed, the bodies move inside it.
    model: Mat4,
    time: f32,
}

/// Muscle tone after death: the legs hold this long, giving out, and then only friction is left.
const RAGDOLL_HOLDS: f32 = 0.7;
/// Settled for good after this long, or once every limb sleeps.
const RAGDOLL_LONGEST: f32 = 9.0;

impl BeastRagdoll {
    fn new(b: &Beast, physics: &mut Physics) -> Self {
        let skeleton = &b.skeleton;
        let mut start = Vec::new();
        skeleton.global_transforms(&b.pose, &mut start);
        let model = Mat4::from_scale_rotation_translation(Vec3::splat(b.size), b.rotation, b.position);
        let at = |m: Vec3| model.transform_point3(m);
        let joint = |name: &str| skeleton.joint(name);
        let head_of = |j: usize| start[j].w_axis.truncate();
        let mass = b.size.powi(3);

        // (bone, shape from..to in the model's space, radius, kg)
        let mut parts: Vec<(usize, Vec3, Vec3, f32, f32)> = Vec::new();
        let Some(body) = joint("Body") else { return Self::empty(model) };
        parts.push((body, Vec3::new(0.0, 0.98, -0.78), Vec3::new(0.0, 1.0, 0.5), 0.34, 260.0));
        let (neck, head) = (joint("Neck"), joint("Head"));
        if let (Some(neck), Some(head)) = (neck, head) {
            parts.push((neck, head_of(neck), head_of(head), 0.15, 18.0));
            let muzzle = start[head].transform_point3(start[head].inverse().transform_point3(head_of(head)))
                + (start[head].transform_vector3(Vec3::Y)).normalize_or(Vec3::Z) * 0.42;
            parts.push((head, head_of(head), muzzle, 0.13, 22.0));
        }
        for leg in ["Fore", "Hind"] {
            for side in ["L", "R"] {
                let (Some(upper), Some(lower), Some(hoof)) = (
                    joint(&format!("{leg}Upper.{side}")),
                    joint(&format!("{leg}Lower.{side}")),
                    joint(&format!("{leg}Hoof.{side}")),
                ) else {
                    continue;
                };
                let foot = head_of(hoof) + (head_of(hoof) - head_of(lower)).normalize_or(Vec3::NEG_Y) * 0.1;
                let thick = if leg == "Hind" { 0.1 } else { 0.08 };
                parts.push((upper, head_of(upper), head_of(lower), thick, 14.0));
                parts.push((lower, head_of(lower), foot, 0.05, 6.0));
            }
        }

        // The push of the blow, and a roll that tips it away from it.
        let away = b.flinch_from;
        let linear = away * 1.2 + b.shove * 0.6 + Vec3::Y * 0.3;
        let spin = Vec3::Y.cross(away) * 1.6;
        let mut limbs = Vec::new();
        for &(bone, from, to, radius, kg) in &parts {
            let frame = model * start[bone];
            let (_, rotation, position) = frame.to_scale_rotation_translation();
            let local = |m: Vec3| rotation.inverse() * (at(m) - position);
            let handle = physics.add_limb(&LimbDesc {
                shape: LimbShape::Capsule { from: local(from), to: local(to), radius: radius * b.size },
                mass: kg * mass,
                position,
                rotation,
                linear,
                angular: spin,
            });
            limbs.push((handle, Mat4::from_rotation_translation(rotation, position)));
        }
        let limb_of = |bone: usize| parts.iter().position(|p| p.0 == bone);
        // Every bone rides on its own limb, or on the nearest one above it; the root on the trunk.
        let mut owner = vec![0; skeleton.joints.len()];
        for &j in skeleton.order() {
            owner[j] = limb_of(j).unwrap_or_else(|| skeleton.joints[j].parent.map_or(0, |p| owner[p]));
        }

        // Joints: at each limb's own bone, the bend about the body's left-right axis.
        let side_axis = b.rotation * Vec3::X;
        let mut joints = Vec::new();
        for (child, &(bone, ..)) in parts.iter().enumerate().skip(1) {
            let Some(parent_bone) = skeleton.joints[bone].parent else { continue };
            let parent = owner[parent_bone];
            let name = skeleton.joints[bone].name.as_str();
            let limits = if name.contains("Lower") && name.starts_with("Fore") {
                [[-0.05, 1.9], [-0.05, 0.05], [-0.12, 0.12]]
            } else if name.contains("Lower") {
                [[-1.9, 0.05], [-0.05, 0.05], [-0.12, 0.12]]
            } else if name.contains("Upper") {
                [[-0.9, 0.9], [-0.15, 0.15], [-0.3, 0.3]]
            } else if name == "Neck" {
                [[-0.4, 1.1], [-0.5, 0.5], [-0.35, 0.35]]
            } else {
                [[-0.6, 0.8], [-0.4, 0.4], [-0.3, 0.3]]
            };
            let place = at(head_of(bone));
            let (_, child_turn, _) = (model * start[bone]).to_scale_rotation_translation();
            let along = (child_turn * Vec3::Y).normalize_or(Vec3::NEG_Y);
            let x = (side_axis - along * side_axis.dot(along)).normalize_or(Vec3::X);
            let frame = Quat::from_mat3(&Mat3::from_cols(x, along, x.cross(along))).normalize();
            let in_limb = |limb: usize| {
                let (_, turn, pos) = limbs[limb].1.to_scale_rotation_translation();
                (turn.inverse() * (place - pos), (turn.inverse() * frame).normalize())
            };
            joints.push(physics.add_socket(&SocketDesc {
                parent: limbs[parent].0,
                child: limbs[child].0,
                parent_frame: in_limb(parent),
                child_frame: in_limb(child),
                limits,
            }));
        }
        Self { limbs, joints, owner, start, model, time: 0.0 }
    }

    fn empty(model: Mat4) -> Self {
        Self { limbs: Vec::new(), joints: Vec::new(), owner: Vec::new(), start: Vec::new(), model, time: 0.0 }
    }

    /// The legs hold for a moment and give out; then the joints only drag.
    fn drive(&self, physics: &mut Physics) {
        let tone = (1.0 - self.time / RAGDOLL_HOLDS).max(0.0);
        for &joint in &self.joints {
            physics.set_muscle(joint, Quat::IDENTITY, 60.0 * tone * tone, 3.0 + 4.0 * tone, 2500.0 * tone + 40.0);
        }
    }

    /// The skinning matrices from where the limbs are now.
    fn pose(&self, b: &mut Beast, physics: &Physics) {
        if self.limbs.is_empty() {
            return;
        }
        // The physics moves it now: nothing else may slide where it is drawn from.
        b.position = self.model.w_axis.truncate();
        b.shove = Vec3::ZERO;
        let back = self.model.inverse();
        let moved: Vec<Mat4> = self
            .limbs
            .iter()
            .map(|&(handle, was)| {
                let now = physics
                    .body_pose(handle)
                    .map_or(was, |(position, rotation)| Mat4::from_rotation_translation(rotation, position));
                back * now * was.inverse() * self.model
            })
            .collect();
        b.matrices.clear();
        b.matrices.extend(
            self.start
                .iter()
                .zip(&self.owner)
                .zip(&b.skeleton.joints)
                .map(|((start, &limb), joint)| moved[limb] * *start * joint.inverse_bind),
        );
    }

    fn still(&self, physics: &Physics) -> bool {
        self.limbs.is_empty()
            || self.time > RAGDOLL_LONGEST
            || (self.time > 2.0 && self.limbs.iter().all(|&(handle, _)| physics.body_sleeping(handle)))
    }

    fn remove(self, physics: &mut Physics) {
        for joint in self.joints {
            physics.remove_joint(joint);
        }
        for (handle, _) in self.limbs {
            physics.remove_body(handle);
        }
    }
}
