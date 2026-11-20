//! The village's people in the world. Who they are and their day come from
//! [`grimora_world::residents`]; here they become NPCs (the chartest model, tinted and scaled)
//! that walk where their day sends them, through doors that open for them. Only those within
//! [`FOLK_NEAR`] of the player are entities: the rest are data, and go straight where their day
//! says (a level of detail of the simulation), coming in from the edge of that circle and
//! walking, never popping up in front of the player.

use std::path::PathBuf;

use glam::{DQuat, DVec2, DVec3, Quat, Vec3, Vec3Swizzles};
use grimora_core::{Animator, MeshData, Skeleton, Transform, WorldTransform};
use grimora_physics::Physics;
use grimora_scene::{House, Interactable, Trade};
use grimora_world::{Bond, Doing, Post, Relief, Resident};
use std::collections::HashMap;

use crate::Health;
use crate::land::Land;
use crate::npc::{Agent, AgentStatus, Brain};
use crate::society::{self, Relation, SAYING, Saying};
use crate::world::{Entity, MeshId, Name, Renderable, World};
use crate::{GenId, Generated};

mod fishing;
mod life;
use grimora_world::path_seed;
use life::{Group, Life, Needs, Pantry, PropMove, Stashed, Tool, UseSpot};

/// Villagers closer than this to the player (meters, flat) are entities.
pub const FOLK_NEAR: f64 = 70.0;
/// And farther than this they go back to being data.
const FOLK_FAR: f64 = 90.0;
/// A clock jump bigger than this (hours: a load, a rest) puts everyone where their day says.
const JUMP: f32 = 0.5;
/// How far in front of a piece of furniture one stands to use it, meters.
const BEFORE: f64 = 1.0;
const MODEL: &str = "../models/chartest/chartest.gltf";
/// The walk the chartest's `sword_walk` was made for, and its run.
const CLIP_SPEEDS: (f32, f32) = (1.2, 5.5);

/// The component of a villager: who they are.
#[derive(Debug, Clone)]
pub struct Villager {
    pub resident: Resident,
}

/// A place to be: where to stand and what to look at (world).
#[derive(Debug, Clone, Copy)]
struct Spot {
    at: DVec3,
    face: DVec3,
    /// The room it must stay in: a place inside a house is never moved through a wall.
    room: Option<Room>,
}

/// A room's floor in the world: the house's middle and turn, and the room's rect in it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Room {
    at: DVec3,
    yaw: f64,
    min: glam::Vec2,
    max: glam::Vec2,
}

impl Room {
    fn contains(&self, p: DVec3) -> bool {
        self.within(p, -0.1)
    }

    /// Whether `p` is in the room grown by `margin` meters each way (shrunk if negative): grown
    /// by a wall's thickness, the doorway between two rooms is in both.
    pub(crate) fn within(&self, p: DVec3, margin: f32) -> bool {
        let local = DQuat::from_rotation_y(self.yaw).inverse() * (p - self.at);
        let (x, z) = (local.x as f32, local.z as f32);
        x > self.min.x - margin && x < self.max.x + margin && z > self.min.y - margin && z < self.max.y + margin
    }

    /// A point of its floor, `u` and `v` 0..1 across it.
    fn point(&self, u: f64, v: f64) -> DVec3 {
        let x = self.min.x as f64 + 0.5 + (self.max.x - self.min.x - 1.0).max(0.0) as f64 * u;
        let z = self.min.y as f64 + 0.5 + (self.max.y - self.min.y - 1.0).max(0.0) as f64 * v;
        self.at + DQuat::from_rotation_y(self.yaw) * DVec3::new(x, 0.0, z)
    }
}

/// The places of one person's day.
#[derive(Debug, Clone, Copy)]
struct Places {
    bed: Spot,
    table: Spot,
    home: Spot,
    work: Spot,
    leisure: Spot,
}

impl Places {
    fn of(&self, doing: Doing) -> Spot {
        match doing {
            Doing::Sleep => self.bed,
            Doing::Eat => self.table,
            Doing::Home => self.home,
            Doing::Work => self.work,
            Doing::Leisure => self.leisure,
        }
    }
}

pub(crate) struct Placed {
    pub(crate) resident: Resident,
    /// Which house (index in `Folk::rooms`), and their friends and foes (indices).
    house: usize,
    bonds: Vec<(usize, Bond)>,
    /// Seconds left running away from the player, or fighting them.
    pub(crate) fear: f32,
    pub(crate) angry: f32,
    /// Seconds until they say something to whoever is with them, greet the player again, or
    /// scold them again for being in the house at night.
    chat: f32,
    /// Seconds left of talking with the hands, seated, after saying something.
    talking: f32,
    greeted: f32,
    scolded: f32,
    /// Seconds they go on watching the player after seeing something they don't like (a
    /// weapon in the hand, someone the village is alarmed about), and until they say so again.
    wary: f32,
    warned: f32,
    /// Their hair and hat, attached to their head; they go when they go.
    hair: Vec<Entity>,
    /// Their hat, which they take off indoors and in bed, and their hair with and without it.
    hat: Option<HatOn>,
    places: Places,
    /// Where they are (world) while data; where they were when they stopped being an entity.
    at: DVec3,
    entity: Option<Entity>,
    /// Where they were last sent.
    going: Option<DVec3>,
    /// How many choices they made, for the seed of the next.
    moves: u64,
    /// Needs, what they do now and next, and what is in their hand (stage 6b).
    life: Life,
}

/// A house as built: where it stands, its turn and its plan.
pub(crate) struct Built<'a> {
    pub lot: &'a grimora_world::Lot,
    pub house: &'a House,
    pub at: DVec3,
}

impl Built<'_> {
    fn world(&self, local: Vec3) -> DVec3 {
        self.at + DQuat::from_rotation_y(self.lot.yaw) * local.as_dvec3()
    }

    /// Where to stand to use the prop called `what` (its `nth`), and the prop: in front of
    /// it, its +Z.
    fn before(&self, what: &str, nth: usize) -> Option<Spot> {
        let prop = self.house.props.iter().filter(|p| p.what.starts_with(what)).nth(nth)?;
        let ahead = prop.rotation * Vec3::Z * BEFORE as f32;
        Some(Spot {
            at: self.world(prop.position + ahead * 0.5),
            face: self.world(prop.position),
            room: self.room_of(prop.position),
        })
    }

    /// Behind the prop called `what`, looking over it: where the keeper stands at a counter.
    fn behind(&self, what: &str) -> Option<Spot> {
        let prop = self.house.props.iter().find(|p| p.what.starts_with(what))?;
        let back = prop.rotation * Vec3::NEG_Z * 0.95;
        let ahead = prop.rotation * Vec3::Z * 2.0;
        Some(Spot {
            at: self.world(prop.position + back),
            face: self.world(prop.position + ahead),
            room: self.room_of(prop.position),
        })
    }

    /// Inside, by the table, or the middle of the house.
    fn inside(&self) -> Spot {
        self.before("Mesa", 0).unwrap_or(Spot { at: self.at, face: self.at + DVec3::Z, room: self.room_of(Vec3::ZERO) })
    }

    /// The room `local` (house space) is in.
    fn room_of(&self, local: Vec3) -> Option<Room> {
        let (min, max) = *self.house.rooms.iter().find(|(min, max)| {
            local.x >= min.x - 0.05 && local.x <= max.x + 0.05 && local.z >= min.y - 0.05 && local.z <= max.y + 0.05
        })?;
        Some(Room { at: self.at, yaw: self.lot.yaw, min, max })
    }
}

/// Everyone of the village, entity or not.
#[derive(Default)]
pub(crate) struct Folk {
    pub(crate) people: Vec<Placed>,
    /// Each house's rooms, for whether the player is inside.
    rooms: Vec<Vec<Room>>,
    /// Just outside each house's street door (world): the way in and out, so nobody tries to
    /// reach a bed through the wall behind it.
    fronts: Vec<DVec3>,
    /// Real seconds of a game hour now, 0 with the day stopped.
    pub(crate) hour_secs: f32,
    /// Where to fish from, by the boat.
    shore: Vec<fishing::ShoreLine>,
    /// The lines out now, by person.
    lines: HashMap<usize, fishing::Line>,
    /// A piece of line and a float.
    line_meshes: Option<(MeshId, MeshId)>,
    /// The lines over heads now.
    pub(crate) says: Vec<Saying>,
    /// Where each villager entity stood last frame (local).
    positions: Vec<Option<Vec3>>,
    model: Option<PathBuf>,
    /// The chartest as loaded, with its shape keys, and its skeleton.
    base: Option<(MeshData, std::sync::Arc<Skeleton>)>,
    /// A body baked for each set of shape-key weights (a tenth apart), shared.
    bodies: HashMap<Vec<(&'static str, i8)>, MeshId>,
    /// Each hair and hat model, `None` if its file failed.
    hairs: HashMap<&'static str, Option<MeshId>>,
    /// Each piece of clothing as loaded, with the body's shape keys; `None` if its file failed.
    clothes: HashMap<&'static str, Option<MeshData>>,
    /// A piece baked for a body's shape-key weights, as `bodies`.
    cut: HashMap<(&'static str, Vec<(&'static str, i8)>), MeshId>,
    /// Each hair pressed under the hat ([`Crown`]), by style; `None` if it could not be.
    pressed: HashMap<&'static str, Option<MeshId>>,
    /// Inside the hat's crown, loaded with the first hat.
    crown: Option<Option<Crown>>,
    /// From the head joint to the model's space at bind: hairs are modeled on the body.
    hair_offset: Transform,
    failed: bool,
    hour: Option<f32>,
    /// Whether each place was moved to free floor yet (needs the houses' colliders).
    settled: bool,
    tries: u32,
    /// The things to use (seats, beds, the anvil, the well...), and who has each.
    spots: Vec<UseSpot>,
    /// People talking in small groups, by id.
    groups: HashMap<u64, Group>,
    next_group: u64,
    /// Each tool model, `None` if its file failed.
    tools: HashMap<Tool, Option<MeshId>>,
    /// What each house has to eat, by house.
    pub(crate) pantry: Vec<Pantry>,
    /// The tools kept in the houses, taken and put down by the villagers.
    pub(crate) stash: Vec<Stashed>,
    /// Props to hide or show again, for the world.
    prop_moves: Vec<PropMove>,
    /// Seconds the village stays alarmed about the player (someone was struck, or killed):
    /// whoever sees them runs or stands guard. Each one still has to see them.
    pub(crate) alarm: f32,
    /// Whether the player has a weapon in the hand, as the game last said.
    pub(crate) player_armed: bool,
}

impl Folk {
    pub(crate) fn entity_of(&self, i: usize) -> Option<Entity> {
        self.people.get(i)?.entity
    }

    /// `i` says `text` over their head (what they said before goes).
    pub(crate) fn say(&mut self, i: usize, text: &str) {
        self.says.retain(|saying| saying.who != i);
        self.says.push(Saying { who: i, text: text.to_owned(), left: SAYING });
    }

    /// Someone of another house to tell a rumor about, and their foe if they have one.
    pub(crate) fn rumor_about(&self, teller: GenId, roll: u64) -> (Option<&Resident>, Option<&Resident>) {
        let Some(me) = self.people.iter().position(|p| p.resident.id == teller) else { return (None, None) };
        let others: Vec<usize> =
            (0..self.people.len()).filter(|&j| self.people[j].house != self.people[me].house).collect();
        if others.is_empty() {
            return (None, None);
        }
        let about = others[(roll % others.len() as u64) as usize];
        let foe =
            self.people[about].bonds.iter().find(|(_, b)| *b == Bond::Foe).map(|&(j, _)| &self.people[j].resident);
        (Some(&self.people[about].resident), foe)
    }

    fn relation(&self, a: usize, b: usize) -> Relation {
        if self.people[a].house == self.people[b].house {
            return Relation::Family;
        }
        match self.people[a].bonds.iter().find(|(j, _)| *j == b) {
            Some((_, Bond::Friend)) => Relation::Friend,
            Some((_, Bond::Foe)) => Relation::Foe,
            None => Relation::Neighbour,
        }
    }
}

/// Two people this close (meters) and standing face each other.
const TALK: f32 = 2.6;
/// Meters kept between two people sent to the same kind of place.
const APART: f64 = 0.9;

impl Places {
    fn slot(&mut self, doing: Doing) -> &mut Spot {
        match doing {
            Doing::Sleep => &mut self.bed,
            Doing::Eat => &mut self.table,
            Doing::Home => &mut self.home,
            Doing::Work => &mut self.work,
            Doing::Leisure => &mut self.leisure,
        }
    }
}

impl Folk {
    /// Moves every place to the nearest floor where a person fits, not in a wall or on the
    /// furniture, and apart from the others sent to that kind of place.
    fn settle(&mut self, physics: &mut Physics, origin: &grimora_core::Origin) {
        physics.refresh();
        let shape = grimora_physics::NavShape::new(0.38, 1.8);
        let mut missing = 0;
        for doing in [Doing::Sleep, Doing::Eat, Doing::Home, Doing::Work, Doing::Leisure] {
            let mut claimed: Vec<DVec3> = Vec::new();
            for person in &mut self.people {
                let spot = person.places.slot(doing);
                let center = spot.at;
                let free = (0..=10).find_map(|ring| {
                    let radius = ring as f64 * 0.4;
                    let n = if ring == 0 { 1 } else { 6 + ring * 3 };
                    (0..n).find_map(|k| {
                        let a = k as f64 / n as f64 * std::f64::consts::TAU;
                        let p = center + DVec3::new(a.cos() * radius, 0.0, a.sin() * radius);
                        let floor = physics.stands_at(origin.to_local(p), &shape)?;
                        let floor = origin.to_world(floor);
                        let inside = spot.room.is_none_or(|room| room.contains(floor));
                        (inside && claimed.iter().all(|c| c.distance(floor) >= APART)).then_some(floor)
                    })
                });
                match free {
                    Some(floor) => {
                        claimed.push(floor);
                        spot.at = floor;
                    }
                    None => {
                        missing += 1;
                        if self.tries >= 30 {
                            log::warn!("Folk: no free floor for {} near {center:?} ({doing:?})", person.resident.name);
                        }
                    }
                }
            }
        }
        self.tries += 1;
        if missing == 0 || self.tries > 30 {
            for person in &mut self.people {
                person.at = person.places.home.at;
            }
            self.settle_spots(physics, origin);
            self.settled = true;
        }
    }
}

impl Folk {
    /// Makes the people of every house and the places of their days. `largo`, `field` and
    /// `boat` are world x, z.
    pub(crate) fn add_village(
        &mut self,
        houses: &[Built],
        largo: DVec2,
        well: DVec2,
        field: Option<DVec2>,
        boat: Option<DVec2>,
        graves: &[(DVec2, f64)],
        pen: Option<DVec2>,
        relief: &Relief,
        life_dir: &std::path::Path,
    ) {
        self.model.get_or_insert_with(|| life_dir.join(MODEL));
        let ground = |p: DVec2| DVec3::new(p.x, relief.height_at(p.x, p.y), p.y);
        // Somewhere outside near `p`, a little apart for each person.
        let near = |p: DVec2, id: GenId, reach: f64| {
            let a = (id.0 % 6283) as f64 / 1000.0;
            let r = reach * (0.4 + ((id.0 >> 16) % 1000) as f64 / 1600.0);
            Spot { at: ground(p + DVec2::new(a.cos(), a.sin()) * r), face: ground(p), room: None }
        };
        let tavern = houses.iter().find(|b| b.lot.trade == Trade::Tavern);
        // Everyone first, then who is whose friend or foe: friends spend the evening in the
        // same place.
        let mut everyone: Vec<(usize, Resident)> = houses
            .iter()
            .enumerate()
            .flat_map(|(h, b)| {
                grimora_world::residents(b.lot.id, b.lot.trade, &b.house.sleeps).into_iter().map(move |r| (h, r))
            })
            .collect();
        let mut bonds = vec![Vec::new(); everyone.len()];
        for i in 0..everyone.len() {
            for j in i + 1..everyone.len() {
                if everyone[i].0 == everyone[j].0 {
                    continue;
                }
                if let Some(bond) = grimora_world::bond(&everyone[i].1, &everyone[j].1) {
                    bonds[i].push((j, bond));
                    bonds[j].push((i, bond));
                    if bond == Bond::Friend && (everyone[i].1.tavern || everyone[j].1.tavern) {
                        for k in [i, j] {
                            everyone[k].1.tavern = everyone[k].1.age >= 16;
                        }
                    }
                    log::info!("  {} e {}: {bond:?}", everyone[i].1.name, everyone[j].1.name);
                }
            }
        }
        self.rooms = houses
            .iter()
            .map(|b| b.house.rooms.iter().map(|&(min, max)| Room { at: b.at, yaw: b.lot.yaw, min, max }).collect())
            .collect();
        // The street door is the first, and the only one in an outer wall with a leaf.
        self.fronts = houses
            .iter()
            .map(|b| b.world(b.house.doors.first().map_or(Vec3::Z * FRONT_STEP, |door| door.outside)))
            .collect();
        self.shore = boat.map(|b| fishing::shore_lines(relief, b)).unwrap_or_default();
        if boat.is_some() && self.shore.is_empty() {
            log::warn!("Folk: no bank to fish from by the boat");
        }
        let shore = self.shore.clone();
        self.make_spots(houses, largo, well, field, &shore, graves, pen, &ground);
        for ((h, resident), bonds) in everyone.into_iter().zip(bonds) {
            let built = &houses[h];
            let home = built.inside();
            {
                let id = resident.id;
                let bed = resident.bed.and_then(|b| {
                    let pos = *built.house.beds.get(b)?;
                    // Beside the bed, not in it.
                    Some(Spot { at: built.world(pos), face: built.world(pos), room: built.room_of(pos) })
                });
                let table = built.before("Mesa", 0);
                let square = near(largo, id, 6.0);
                let trade = match built.lot.trade {
                    Trade::Smith => built.before("Bigorna", 0).or_else(|| built.before("Forja", 0)),
                    Trade::Fisher => shore
                        .first()
                        .map(|s| Spot {
                            at: s.stand,
                            face: s.stand + DVec3::new(s.toward.x, 0.0, s.toward.y) * 4.0,
                            room: None,
                        })
                        .or_else(|| boat.map(|b| near(b, id, 2.5))),
                    Trade::Tavern => built.behind("Balcao"),
                    Trade::Healer => built.before("Mesa", 0),
                    Trade::Chief => Some(near(largo, id, 3.0)),
                    Trade::Home => field.map(|f| near(f, id, 8.0)),
                    Trade::Herder => pen.map(|p| near(p, id, 6.5)).or_else(|| field.map(|f| near(f, id, 8.0))),
                    Trade::Sexton => graves.first().map(|g| near(g.0, id, 3.0)),
                    Trade::Carpenter => built.before("Bancada", 0).or_else(|| built.before("Mesa", 0)),
                };
                let work = match resident.post {
                    Post::Trade => trade,
                    Post::Field => field.map(|f| near(f, id, 8.0)),
                    Post::Largo => Some(square),
                    Post::Home => None,
                }
                .unwrap_or(home);
                let at_tavern = || {
                    let t = tavern?;
                    let tables = t.house.props.iter().filter(|p| p.what.starts_with("Mesa")).count().max(1);
                    t.before("Mesa", (id.0 % tables as u64) as usize).or_else(|| t.before("Balcao", 0))
                };
                let keeps_it = built.lot.trade == Trade::Tavern && resident.post == Post::Trade;
                let leisure = if keeps_it {
                    work
                } else if resident.tavern {
                    at_tavern().unwrap_or(square)
                } else {
                    square
                };
                let places = Places { bed: bed.unwrap_or(home), table: table.unwrap_or(home), home, work, leisure };
                log::info!(
                    "  {} ({}, {} anos): acorda {:.1}, trabalha {:.1}-{:.1}, {} até {:.1}, dorme {:.1}",
                    resident.name,
                    resident.role,
                    resident.age,
                    resident.day.wake,
                    resident.day.work,
                    resident.day.done,
                    if resident.tavern { "taverna" } else { "largo" },
                    resident.day.home,
                    resident.day.sleep
                );
                self.people.push(Placed {
                    at: places.home.at,
                    places,
                    resident,
                    house: h,
                    bonds,
                    fear: 0.0,
                    angry: 0.0,
                    chat: 3.0,
                    talking: 0.0,
                    greeted: 0.0,
                    scolded: 0.0,
                    wary: 0.0,
                    warned: 0.0,
                    entity: None,
                    going: None,
                    moves: 0,
                    life: Life { needs: first_needs(id), ..Default::default() },
                    hair: Vec::new(),
                    hat: None,
                });
            }
        }
        self.give_beds();
        log::info!("Folk: {} things to use", self.spots.len());
    }
}

impl World {
    /// Sends each villager where their day says at this hour: the near ones walk (as
    /// entities), the far ones are put there. Opens the doors they walk through. Those
    /// [`World::despawn`] removed (dead, moved away) stay gone.
    pub(crate) fn update_folk(&mut self, land: &mut Land, focus: DVec3, physics: &mut Physics, dt: f32) {
        let folk = &mut land.folk;
        if folk.people.is_empty() || folk.failed {
            return;
        }
        let hour = self.time_of_day().unwrap_or(9.0);
        let hour_secs = self.day_cycle.as_ref().map_or(0.0, |day| day.day_length / 24.0);
        let jumped = folk.hour.is_none_or(|last| {
            let d = (hour - last).rem_euclid(24.0);
            d.min(24.0 - d) > JUMP
        });
        folk.hour = Some(hour);
        folk.hour_secs = hour_secs;
        let origin = self.origin();
        let flat = |p: DVec3| DVec2::new(p.x - focus.x, p.z - focus.z).length();
        if !folk.settled {
            // Only with the player by the village: elsewhere its ground has no colliders.
            if folk.people.iter().all(|p| flat(p.places.home.at) > FOLK_FAR) {
                return;
            }
            folk.settle(physics, &origin);
            if !folk.settled {
                return;
            }
        }
        let mut walkers = Vec::new();
        let last = std::mem::take(&mut folk.positions);
        folk.positions = vec![None; folk.people.len()];
        for saying in &mut folk.says {
            saying.left -= dt;
        }
        folk.says.retain(|saying| saying.left > 0.0);
        let player = origin.to_local(focus);
        let night = !(6.0..19.0).contains(&hour);
        let raining = self.environment.precipitation.rain > 0.25 || self.environment.precipitation.snow > 0.25;
        let reputation = self.generated.reputation;
        let mut calm = Vec::new();
        let mut scolds = 0;
        let mut died = Vec::new();
        let mut rage = Vec::new();
        folk.alarm = (folk.alarm - dt).max(0.0);
        let (alarm, armed) = (folk.alarm, folk.player_armed);
        for i in 0..folk.people.len() {
            if folk.people[i].entity.is_none()
                && (folk.people[i].life.act.is_some() || folk.people[i].life.tool.is_some())
            {
                folk.forget(i);
                folk.people[i].life.tool = None;
            }
            let person = &mut folk.people[i];
            let id = person.resident.id;
            if self.generated.is_removed(id) {
                // The dead lie where they fell until nobody is near.
                if let Some(entity) = person.entity
                    && self.ecs.get::<&Transform>(entity).map_or(true, |t| flat(origin.to_world(t.position)) > FOLK_FAR)
                {
                    crate::world::despawn(&mut self.ecs, entity, physics);
                    person.entity = None;
                    person.hat = None;
                    for hair in person.hair.drain(..) {
                        let _ = self.ecs.despawn(hair);
                    }
                }
                continue;
            }
            let spot = person.places.of(person.resident.day.doing(hour));
            if let Some(entity) = person.entity
                && !self.ecs.contains(entity)
            {
                person.entity = None;
                person.hat = None;
                for hair in person.hair.drain(..) {
                    let _ = self.ecs.despawn(hair);
                }
            }
            match person.entity {
                Some(entity) => {
                    let Ok((agent, transform, health, brain)) =
                        self.ecs.query_one_mut::<(&mut Agent, &Transform, Option<&Health>, Option<&Brain>)>(entity)
                    else {
                        continue;
                    };
                    if health.is_some_and(|h| h.is_dead()) {
                        // Stays dead: saved by its id.
                        self.generated.removed.insert(id);
                        died.push((i, entity));
                        folk.alarm = folk.alarm.max(society::MURDER_ALARM);
                        continue;
                    }
                    let here = origin.to_world(transform.position);
                    if flat(here) > FOLK_FAR {
                        person.at = here;
                        crate::world::despawn(&mut self.ecs, entity, physics);
                        person.entity = None;
                        person.hat = None;
                        for hair in person.hair.drain(..) {
                            let _ = self.ecs.despawn(hair);
                        }
                        continue;
                    }
                    walkers.push(crate::house::Walker {
                        at: transform.position,
                        path: agent.path().iter().take(6).copied().collect(),
                    });
                    folk.positions[i] = Some(transform.position);
                    let asleep = folk.asleep(i);
                    let person = &mut folk.people[i];
                    person.greeted -= dt;
                    person.scolded -= dt;
                    person.chat -= dt;
                    // Fighting the player, or running from them: the day waits.
                    if person.angry > 0.0 || brain.is_some() {
                        person.angry -= dt;
                        if person.angry <= 0.0 {
                            calm.push(entity);
                            agent.running = false;
                            person.going = None;
                        }
                        continue;
                    }
                    if person.fear > 0.0 {
                        person.fear -= dt;
                        if person.fear <= 0.0 {
                            agent.running = false;
                            person.going = None;
                        }
                        continue;
                    }
                    let near_player = transform.position.distance(player);
                    let doing = person.resident.day.doing(hour);
                    // What they see of the player, with their own eyes: a weapon in the hand,
                    // or the one the village is alarmed about. Unseen, nothing happens.
                    let stand = transform.position;
                    let eyes = stand + Vec3::Y * 1.6 * transform.scale.y;
                    let noticed = !asleep
                        && (alarm > 0.0 || armed)
                        && society::sees(physics, eyes, agent.facing, player + Vec3::Y * 1.2, night);
                    let strong = society::strong(&person.resident);
                    let child = person.resident.age < 14;
                    let away = Vec3::new(stand.x - player.x, 0.0, stand.z - player.z).normalize_or_zero();
                    person.warned -= dt;
                    if noticed
                        && !strong
                        && ((alarm > 0.0 && near_player < 12.0) || (child && armed && near_player < 7.0))
                    {
                        // Not one to stand up to that: away, running.
                        let cry = person.warned <= 0.0;
                        folk.forget(i);
                        let person = &mut folk.people[i];
                        person.fear = society::FEAR;
                        person.warned = 20.0;
                        person.going = None;
                        agent.rest_clip = None;
                        agent.running = true;
                        agent.go_to(stand + away * 18.0);
                        if cry {
                            folk.say(
                                i,
                                if alarm > 0.0 {
                                    ["É ele! Corram!", "Ele voltou!", "Socorro!"][i % 3]
                                } else {
                                    "Mãe!"
                                },
                            );
                        }
                        continue;
                    }
                    if noticed && near_player < if alarm > 0.0 { 14.0 } else { 7.0 } {
                        person.wary = 3.0;
                    }
                    if person.wary > 0.0 {
                        person.wary -= dt;
                        let warn = person.warned <= 0.0;
                        if warn {
                            person.warned = 20.0;
                        }
                        if strong && alarm > 0.0 && armed && near_player < 5.0 {
                            // Warned, armed and still coming: the strong ones don't wait.
                            person.angry = society::ANGER;
                            person.wary = 0.0;
                            folk.forget(i);
                            agent.rest_clip = None;
                            agent.running = true;
                            rage.push((entity, stand, agent.facing, id.0));
                            folk.say(i, "Eu avisei!");
                            continue;
                        }
                        let using = matches!(person.life.act, Some(life::Act::Use { .. }));
                        if warn {
                            let line = if alarm > 0.0 {
                                ["Não chegue perto.", "Estou de olho em você.", "Fique onde está."][i % 3]
                            } else {
                                ["Guarde essa arma.", "Pra que isso na mão?", "Aqui ninguém anda de arma na mão."]
                                    [i % 3]
                            };
                            folk.say(i, line);
                        }
                        if !using {
                            // Eyes on the player, and a step back if they come too close.
                            if near_player < 2.5 {
                                if agent.status() != AgentStatus::Moving {
                                    agent.go_to(stand + away * 3.0);
                                }
                            } else {
                                agent.stop();
                                agent.look_at(player);
                            }
                            continue;
                        }
                    }
                    let person = &mut folk.people[i];
                    let roll = path_seed(&[id.0, person.moves, (hour * 60.0) as u64]);
                    let mut line: Option<String> = None;
                    // In their house at night: they want the player out.
                    let at_home = matches!(doing, Doing::Sleep | Doing::Home | Doing::Eat);
                    if night
                        && !asleep
                        && at_home
                        && near_player < 10.0
                        && person.scolded <= 0.0
                        && folk.rooms[person.house].iter().any(|room| room.contains(focus))
                    {
                        person.scolded = 10.0;
                        scolds += 1;
                        line = Some(
                            [
                                "O que faz na minha casa a esta hora? Saia!",
                                "Fora daqui! É noite!",
                                "Saia já da minha casa!",
                            ][(roll % 3) as usize]
                                .into(),
                        );
                    } else if !night
                        && near_player < 3.0
                        && person.greeted <= 0.0
                        && doing != Doing::Sleep
                        && !asleep
                        && alarm <= 0.0
                    {
                        person.greeted = 90.0;
                        line = Some(society::greeting(reputation, hour, roll).into());
                    }
                    if let Some(line) = line {
                        let person = &mut folk.people[i];
                        person.chat = person.chat.max(3.0);
                        folk.say(i, &line);
                    }
                    let target = origin.to_local(spot.at);
                    if jumped {
                        folk.forget(i);
                        agent.teleport(physics, target, agent.facing);
                        agent.stop();
                    }
                    folk.live(i, agent, transform.position, &last, &origin, physics, doing, raining, dt);
                }
                None => {
                    if jumped || flat(spot.at) > FOLK_NEAR {
                        // Nobody sees it: straight there.
                        person.at = spot.at;
                    } else if flat(person.at) > FOLK_NEAR {
                        // Going somewhere the player is: it comes in from the circle's edge.
                        let dir = DVec2::new(person.at.x - focus.x, person.at.z - focus.z).normalize_or_zero();
                        let edge = DVec2::new(focus.x, focus.z) + dir * (FOLK_NEAR - 2.0);
                        person.at = DVec3::new(edge.x, land.relief.height_at(edge.x, edge.y), edge.y);
                    }
                    if flat(person.at) < FOLK_NEAR {
                        if folk.base.is_none() {
                            let Some(path) = &folk.model else { return };
                            match grimora_scene::load_model(path, 0) {
                                Ok(mesh) if mesh.skeleton.is_some() => {
                                    let skeleton = mesh.skeleton.clone().unwrap();
                                    if let Some(head) = skeleton.joint("head") {
                                        let (scale, rotation, position) =
                                            skeleton.joints[head].inverse_bind.to_scale_rotation_translation();
                                        folk.hair_offset = Transform { position, rotation, scale };
                                    }
                                    log::info!("Folk: {} shape keys on the body", mesh.morphs.len());
                                    folk.base = Some((mesh, skeleton));
                                }
                                _ => {
                                    log::warn!("Folk: no skinned model at {}", path.display());
                                    folk.failed = true;
                                    return;
                                }
                            }
                        }
                        // This person's body: baked once for their shape keys, shared with
                        // anyone shaped the same.
                        let shape = grimora_world::body_shape(&folk.people[i].resident);
                        // Tenths; a man's `Female_Breasts` is below 0 (see `body_shape`).
                        let key: Vec<(&'static str, i8)> = shape
                            .iter()
                            .map(|&(k, w)| (k, (w * 10.0).round().clamp(-100.0, 100.0) as i8))
                            .filter(|&(_, w)| w != 0)
                            .collect();
                        let mesh = match folk.bodies.get(&key) {
                            Some(&mesh) => mesh,
                            None => {
                                let (base, _) = folk.base.as_ref().unwrap();
                                let weights: Vec<(&str, f32)> =
                                    key.iter().map(|&(k, w)| (k, w as f32 / 10.0)).collect();
                                let mut body = base.morphed(&weights);
                                if folk.people[i].resident.woman {
                                    dress_as_woman(&mut body, folk.model.as_deref());
                                }
                                let mesh = self.add_mesh(body);
                                folk.bodies.insert(key.clone(), mesh);
                                mesh
                            }
                        };
                        let hair = grimora_world::hair(&folk.people[i].resident).and_then(|(style, color)| {
                            let model = folk.model.as_ref()?.with_file_name("hairs").join(format!("{style}.glb"));
                            let mesh = *folk.hairs.entry(style).or_insert_with(|| {
                                match grimora_scene::load_model(&model, 0) {
                                    Ok(mesh) => Some(self.add_mesh(mesh)),
                                    Err(error) => {
                                        log::warn!("Folk: no hair {}: {error:#}", model.display());
                                        None
                                    }
                                }
                            });
                            Some((style, mesh?, color))
                        });
                        let hat = grimora_world::hat(&folk.people[i].resident).and_then(|(piece, color)| {
                            let model = folk.model.as_ref()?.with_file_name(format!("{piece}.glb"));
                            let mesh = *folk.hairs.entry(piece).or_insert_with(|| {
                                match grimora_scene::load_model(&model, 0) {
                                    Ok(mesh) => Some(self.add_mesh(mesh)),
                                    Err(error) => {
                                        log::warn!("Folk: no hat {}: {error:#}", model.display());
                                        None
                                    }
                                }
                            });
                            if folk.crown.is_none() {
                                folk.crown =
                                    Some(grimora_scene::load_model(&model, 0).ok().and_then(|hat| Crown::of(&hat)));
                            }
                            Some((mesh?, color))
                        });
                        // Under a hat the hair is the one pressed by it (built once per style).
                        let pressed = match (&hair, &hat, &folk.crown) {
                            (Some((style, _, _)), Some(_), Some(Some(crown))) => {
                                let style = *style;
                                let file =
                                    folk.model.as_ref().map(|m| m.with_file_name("hairs").join(format!("{style}.glb")));
                                *folk.pressed.entry(style).or_insert_with(|| {
                                    let mut mesh = grimora_scene::load_model(&file?, 0).ok()?;
                                    crown.press(&mut mesh);
                                    Some(self.add_mesh(mesh))
                                })
                            }
                            _ => None,
                        };
                        // Clothes have the body's shape keys: each piece gets the same weights.
                        let mut wears = Vec::new();
                        for (piece, color) in grimora_world::clothes(&folk.people[i].resident) {
                            let cut = (piece, key.clone());
                            let mesh = match folk.cut.get(&cut) {
                                Some(&mesh) => Some(mesh),
                                None => {
                                    let file = folk.model.as_ref().map(|m| m.with_file_name(format!("{piece}.glb")));
                                    let base = folk.clothes.entry(piece).or_insert_with(|| {
                                        let file = file?;
                                        grimora_scene::load_model(&file, 0)
                                            .map_err(|error| {
                                                log::warn!("Folk: no clothes {}: {error:#}", file.display())
                                            })
                                            .ok()
                                    });
                                    base.as_ref().map(|base| {
                                        let weights: Vec<(&str, f32)> =
                                            key.iter().map(|&(k, w)| (k, w as f32 / 10.0)).collect();
                                        let mesh = self.add_mesh(base.morphed(&weights));
                                        folk.cut.insert(cut, mesh);
                                        mesh
                                    })
                                }
                            };
                            if let Some(mesh) = mesh {
                                wears.push((mesh, color));
                            }
                        }
                        let skeleton = folk.base.as_ref().unwrap().1.clone();
                        let offset = folk.hair_offset;
                        let person = &mut folk.people[i];
                        let entity = spawn(self, person, spot, mesh, skeleton, physics);
                        person.entity = Some(entity);
                        person.going = None;
                        let mut worn_hair = None;
                        if let Some((_, mesh, color)) = hair {
                            let hair = self.attach(entity, "head", pressed.unwrap_or(mesh), offset);
                            worn_hair = pressed.map(|pressed| (hair, mesh, pressed));
                            if let Ok(mut renderable) = self.ecs.get::<&mut Renderable>(hair) {
                                renderable.color = (Vec3::from(color) * 2.2).min(Vec3::splat(1.2));
                            }
                            folk.people[i].hair.push(hair);
                        }
                        if let Some((mesh, color)) = hat {
                            let hat = self.attach(entity, "head", mesh, offset);
                            if let Ok(mut renderable) = self.ecs.get::<&mut Renderable>(hat) {
                                renderable.color = Vec3::from(color);
                            }
                            folk.people[i].hair.push(hat);
                            folk.people[i].hat = Some(HatOn { hat, hair: worn_hair, on: true });
                        }
                        // Worn pieces bend with the body and go when it goes (`update_attachments`).
                        for (mesh, color) in wears {
                            let piece = self.dress(entity, mesh);
                            if let Ok(mut renderable) = self.ecs.get::<&mut Renderable>(piece) {
                                renderable.color = Vec3::from(color);
                            }
                        }
                    }
                }
            }
        }
        for (entity, home, facing, seed) in rage {
            let notice = grimora_scene::Notice { radius: 30.0, reaction: grimora_scene::Reaction::Attack };
            let brain = Brain::new(grimora_scene::Behavior::Stand, Some(notice), home, facing, seed);
            let _ = self.ecs.insert_one(entity, brain);
        }
        for entity in calm {
            let _ = self.ecs.remove_one::<Brain>(entity);
        }
        folk.update_groups(dt);
        // The houses' own tools: found once the houses stand, gone if the player took them,
        // hidden while in a hand and shown again where they were put down.
        if folk.stash.iter().any(|s| s.entity.is_none() && !s.gone) {
            let mut found: Vec<(Entity, String, Vec3)> = self
                .ecs
                .query::<(Entity, &Name, &Transform)>()
                .iter()
                .filter(|(_, name, _)| name.0.starts_with("house:"))
                .map(|(e, name, t)| (e, name.0.clone(), t.position))
                .collect();
            for s in folk.stash.iter_mut().filter(|s| s.entity.is_none() && !s.gone) {
                let local = origin.to_local(s.at);
                if let Some(k) = found
                    .iter()
                    .enumerate()
                    .filter(|(_, (_, name, _))| *name == s.name)
                    .min_by(|a, b| a.1.2.distance_squared(local).total_cmp(&b.1.2.distance_squared(local)))
                    .map(|(k, _)| k)
                {
                    s.entity = Some(found.swap_remove(k).0);
                }
            }
        }
        for s in folk.stash.iter_mut() {
            if let Some(e) = s.entity
                && !self.ecs.contains(e)
            {
                s.entity = None;
                s.gone = true;
            }
        }
        // A thing of a house that the player took and left lying somewhere: whoever of that
        // house sees it knows it, and it is theirs again, fetched and taken home when wanted.
        if folk.stash.iter().any(|s| s.gone) {
            let lying: Vec<(Entity, String, Vec3, Quat)> = self
                .ecs
                .query::<(Entity, &crate::items::Pickup, &Transform)>()
                .iter()
                .filter(|(_, pickup, _)| pickup.key.starts_with("dropped#"))
                .map(|(e, pickup, t)| (e, pickup.item.clone(), t.position, t.rotation))
                .collect();
            let mut back: Vec<(usize, usize, Entity, Vec3, Quat)> = Vec::new();
            for (k, s) in folk.stash.iter().enumerate().filter(|(_, s)| s.gone) {
                let Some(item) = s.tool.what().and_then(crate::house::house_item) else { continue };
                for (e, id, at, rotation) in &lying {
                    if id != item || back.iter().any(|b| b.2 == *e) {
                        continue;
                    }
                    let seer = (0..folk.people.len()).find(|&i| {
                        folk.people[i].house == s.house
                            && !folk.asleep(i)
                            && folk.positions[i].is_some_and(|p| {
                                p.distance(*at) < 12.0 && !physics.line_blocked(p + Vec3::Y * 1.5, *at + Vec3::Y * 0.2)
                            })
                    });
                    if let Some(i) = seer {
                        back.push((k, i, *e, *at, *rotation));
                        break;
                    }
                }
            }
            for (k, i, e, at, rotation) in back {
                let s = &mut folk.stash[k];
                s.entity = Some(e);
                s.at = origin.to_world(at);
                s.rotation = rotation;
                s.gone = false;
                s.missed = false;
                let line = format!("Olha {} aí! Quem largou isso aqui?", s.tool.name());
                folk.say(i, &line);
            }
        }
        for change in std::mem::take(&mut folk.prop_moves) {
            match change {
                PropMove::Hide(e) => {
                    if let Ok(collider) = self.ecs.remove_one::<crate::world::Collider>(e) {
                        physics.remove(collider.0);
                    }
                    let _ = self.ecs.remove_one::<Interactable>(e);
                    if let Ok(mut r) = self.ecs.get::<&mut Renderable>(e) {
                        r.visible = false;
                    }
                }
                PropMove::Show(e, at, rotation) => {
                    if let Ok(mut t) = self.ecs.get::<&mut Transform>(e) {
                        t.position = origin.to_local(at);
                        t.rotation = rotation;
                    }
                    if let Ok(mut r) = self.ecs.get::<&mut Renderable>(e) {
                        r.visible = true;
                    }
                    if self.ecs.get::<&crate::items::Pickup>(e).is_ok() {
                        let _ = self.ecs.insert_one(e, Interactable { prompt: "pegar".into(), radius: 2.2 });
                    }
                }
            }
        }
        // Walking with something heavy or long: the arms carry it.
        for person in &folk.people {
            let Some(entity) = person.entity else { continue };
            let walking = matches!(person.life.act, Some(life::Act::Walk { .. }));
            let clip = person.life.tool.and_then(|(_, t)| t.carry_clip()).filter(|_| walking);
            if let Ok(mut animator) = self.ecs.get::<&mut Animator>(entity) {
                match clip {
                    Some(clip) => {
                        animator.overlay_loop(clip, "spine", 0.3);
                    }
                    None if animator.overlay_current().is_some_and(|c| c.starts_with("carry_")) => {
                        animator.overlay_stop(0.3)
                    }
                    None => {}
                }
            }
        }
        // Tools: what each should hold, in the right hand.
        let objects = folk.model.as_ref().and_then(|m| Some(m.parent()?.parent()?.join("objects")));
        // The dead: what they held falls where they fell, nobody talks to them, and what they
        // were saying stops.
        for (i, entity) in died {
            folk.fall(i);
            folk.says.retain(|saying| saying.who != i);
            let _ = self.ecs.remove_one::<Interactable>(entity);
        }
        for i in 0..folk.people.len() {
            let Some(entity) = folk.people[i].entity else { continue };
            // A thing of the houses is in the hand only if a real one was taken.
            let want = folk.people[i]
                .life
                .want_tool
                .filter(|&t| !t.real() || folk.stash.iter().any(|s| s.held_by == Some(i) && s.tool == t));
            if folk.people[i].life.tool.map(|(_, t)| t) == want {
                continue;
            }
            if let Some((old, _)) = folk.people[i].life.tool.take() {
                let _ = self.ecs.despawn(old);
            }
            let Some(tool) = want else { continue };
            let mesh = *folk.tools.entry(tool).or_insert_with(|| {
                let file = objects.as_ref()?.join(tool.file());
                match grimora_scene::load_model(&file, 0) {
                    Ok(mesh) => Some(self.add_mesh(mesh)),
                    Err(error) => {
                        log::warn!("Folk: no tool {}: {error:#}", file.display());
                        None
                    }
                }
            });
            if let Some(mesh) = mesh {
                let held = self.attach(entity, "weapon.R", mesh, tool.offset());
                folk.people[i].life.tool = Some((held, tool));
            }
        }
        self.update_fishing(folk, dt);
        // Hats come off indoors and in bed, and the hair stands up again.
        let mut hats = Vec::new();
        for person in &folk.people {
            let (Some(entity), Some(hat)) = (person.entity, &person.hat) else { continue };
            let Ok(transform) = self.ecs.get::<&Transform>(entity) else { continue };
            let here = origin.to_world(transform.position);
            let off = person.resident.day.doing(hour) == Doing::Sleep
                || folk.rooms.iter().flatten().any(|room| room.contains(here));
            if off == hat.on {
                hats.push((hat.hat, hat.hair, !off));
            }
        }
        for (hat, hair, on) in hats {
            if let Ok(mut attached) = self.ecs.get::<&mut crate::world::Attached>(hat) {
                attached.hidden = !on;
            }
            if let Some((hair, plain, pressed)) = hair
                && let Ok(mut renderable) = self.ecs.get::<&mut Renderable>(hair)
            {
                renderable.mesh = if on { pressed } else { plain };
            }
            if let Some(person) = folk.people.iter_mut().find(|p| p.hat.as_ref().is_some_and(|h| h.hat == hat)) {
                person.hat.as_mut().unwrap().on = on;
            }
        }
        self.doors_for(&walkers);
        self.change_reputation(-2 * scolds);
    }
}

/// Where the way in and out of a house is, in its space, if it has no door.
const FRONT_STEP: f32 = 4.0;

/// The model's `is_female` also swaps the body's texture: the same, with this picture.
const FEMALE_TEXTURE: &str = "Chartest-Female-v01-128.png";

fn dress_as_woman(body: &mut MeshData, model: Option<&std::path::Path>) {
    let Some(path) = model.map(|m| m.with_file_name(FEMALE_TEXTURE)) else { return };
    match grimora_scene::load_texture(&path) {
        Ok(texture) => {
            for material in &mut body.materials {
                if material.base_color_texture.is_some() {
                    material.base_color_texture = Some(texture.clone());
                }
            }
        }
        Err(error) => log::warn!("Folk: no female texture {}: {error:#}", path.display()),
    }
}

fn spawn(
    world: &mut World,
    person: &Placed,
    spot: Spot,
    mesh: MeshId,
    skeleton: std::sync::Arc<Skeleton>,
    physics: &mut Physics,
) -> Entity {
    let r = &person.resident;
    let look = spot.face - person.at;
    let facing = if look.length_squared() > 1e-4 { (-look.x).atan2(-look.z) as f32 } else { 0.0 };
    let made =
        WorldTransform { position: person.at, rotation: Quat::from_rotation_y(facing), scale: Vec3::splat(r.height) };
    let transform = world.origin().transform_to_local(&made);
    let mut agent = Agent::new(physics, transform.position, facing, 0.3 * r.height, 1.8 * r.height);
    // Everyone walks at their own pace.
    let pace = (path_seed(&[r.id.0, 77]) % 1000) as f32 / 1000.0;
    agent.walk_speed = 1.2 * r.height.min(1.0) * (0.86 + 0.26 * pace);
    agent.clip_speeds = CLIP_SPEEDS;
    agent.clip_prefix = "sword_".into();
    agent.body.through_doors = true;
    let mut animator = Animator::new(skeleton);
    animator.play("sword_idle", 0.0);
    let (health, fighter) = crate::combat::components(&society::combat_of(r));
    let entity = world.ecs.spawn((
        transform,
        Renderable { color: Vec3::from(r.tint), ..Renderable::new(mesh) },
        Name(key(r.id)),
        animator,
        agent,
        Interactable { prompt: "falar".into(), radius: 2.2 },
        Villager { resident: r.clone() },
        Generated { id: r.id, made },
        health,
    ));
    if let Some(fighter) = fighter {
        let _ = world.ecs.insert_one(entity, fighter);
    }
    entity
}

/// The name of a villager's entity: what talking to them is keyed by.
pub fn key(id: GenId) -> String {
    format!("morador:{:016x}", id.0)
}

/// A hat on someone's head: the hat, and their hair (its entity, as it is and pressed by the
/// hat), so that taking the hat off lets the hair stand.
pub(crate) struct HatOn {
    hat: Entity,
    hair: Option<(Entity, MeshId, MeshId)>,
    on: bool,
}

/// Cells across a hat's crown map.
const CROWN_CELLS: usize = 64;
/// Hair pressed by a hat stays this far under its inside, meters.
const UNDER_HAT: f32 = 0.005;

/// Inside a hat, as the hull map (`HullMap`) does for a boat: the lowest point of the hat over
/// each place seen from above, in the space hat and hair are both modeled in (the bare head's,
/// at bind; neither has shape keys). Hair above that, less [`UNDER_HAT`], is inside the hat.
pub(crate) struct Crown {
    min: glam::Vec2,
    cell: f32,
    low: Vec<f32>,
}

impl Crown {
    fn of(hat: &MeshData) -> Option<Crown> {
        let (mut min, mut max) = (glam::Vec2::splat(f32::MAX), glam::Vec2::splat(f32::MIN));
        for v in &hat.vertices {
            let p = glam::Vec2::new(v.position[0], v.position[2]);
            min = min.min(p);
            max = max.max(p);
        }
        let cell = (max - min).max_element() / CROWN_CELLS as f32;
        if cell.is_nan() || cell <= 0.0 {
            return None;
        }
        let mut crown = Crown { min, cell, low: vec![f32::INFINITY; CROWN_CELLS * CROWN_CELLS] };
        for t in hat.indices.chunks_exact(3) {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| Vec3::from(hat.vertices[i as usize].position));
            let area = (b.x - a.x) * (c.z - a.z) - (c.x - a.x) * (b.z - a.z);
            if area.abs() < 1e-9 {
                continue;
            }
            let lo = (a.min(b).min(c).xz() - min) / cell;
            let hi = (a.max(b).max(c).xz() - min) / cell;
            for z in (lo.y.floor().max(0.0) as usize)..=(hi.y.ceil() as usize).min(CROWN_CELLS - 1) {
                for x in (lo.x.floor().max(0.0) as usize)..=(hi.x.ceil() as usize).min(CROWN_CELLS - 1) {
                    let p = min + (glam::Vec2::new(x as f32, z as f32) + 0.5) * cell;
                    let wb = ((p.x - a.x) * (c.z - a.z) - (c.x - a.x) * (p.y - a.z)) / area;
                    let wc = ((b.x - a.x) * (p.y - a.z) - (p.x - a.x) * (b.z - a.z)) / area;
                    let wa = 1.0 - wb - wc;
                    if wa < -1e-4 || wb < -1e-4 || wc < -1e-4 {
                        continue;
                    }
                    let y = a.y * wa + b.y * wb + c.y * wc;
                    let low = &mut crown.low[z * CROWN_CELLS + x];
                    *low = low.min(y);
                }
            }
        }
        Some(crown)
    }

    /// The inside of the hat over `p`, if the hat is there.
    fn low_at(&self, p: Vec3) -> Option<f32> {
        let c = (p.xz() - self.min) / self.cell;
        if c.x < 0.0 || c.y < 0.0 || c.x >= CROWN_CELLS as f32 || c.y >= CROWN_CELLS as f32 {
            return None;
        }
        let low = self.low[c.y as usize * CROWN_CELLS + c.x as usize];
        low.is_finite().then_some(low)
    }

    /// Presses `hair` under the hat: every vertex inside goes down to just under it, and
    /// triangles left wholly inside go (nothing would see them, and a tip could poke out).
    fn press(&self, hair: &mut MeshData) {
        let mut pressed = vec![false; hair.vertices.len()];
        for (i, v) in hair.vertices.iter_mut().enumerate() {
            let p = Vec3::from(v.position);
            if let Some(low) = self.low_at(p)
                && p.y > low - UNDER_HAT
            {
                v.position[1] = low - UNDER_HAT;
                pressed[i] = true;
            }
        }
        let mut indices = Vec::with_capacity(hair.indices.len());
        for sub in &mut hair.submeshes {
            let start = indices.len() as u32;
            for t in hair.indices[sub.indices.start as usize..sub.indices.end as usize].chunks_exact(3) {
                if !t.iter().all(|&i| pressed[i as usize]) {
                    indices.extend_from_slice(t);
                }
            }
            sub.indices = start..indices.len() as u32;
        }
        hair.indices = indices;
    }
}

/// Where someone's needs start, by the seed.
fn first_needs(id: GenId) -> Needs {
    let u = |k: u64| (path_seed(&[id.0, 900 + k]) % 1000) as f32 / 1000.0;
    Needs { hunger: 0.2 + 0.4 * u(0), tired: 0.1 + 0.2 * u(1), social: 0.3 + 0.5 * u(2), duty: 0.4 + 0.5 * u(3) }
}

/// What the debug panel shows of the village.
impl World {
    /// Every villager: name, what they are, and where (local, the entity's when there is one).
    pub fn villagers(&self) -> Vec<(String, String, Vec3)> {
        let Some(land) = self.ground() else { return Vec::new() };
        let origin = self.origin();
        land.folk
            .people
            .iter()
            .map(|p| {
                let at = p
                    .entity
                    .and_then(|e| self.ecs.get::<&Transform>(e).ok().map(|t| t.position))
                    .unwrap_or_else(|| origin.to_local(p.at));
                (p.resident.name.clone(), p.resident.role.clone(), at)
            })
            .collect()
    }

    /// Over each villager entity's head (local): who they are, what they do, their needs and
    /// what is in their hand.
    /// What each house of the village has to eat: its trade, raw food, meals and water.
    pub fn village_pantries(&self) -> Vec<(String, f32, f32, f32)> {
        let Some(land) = self.ground() else { return Vec::new() };
        let folk = &land.folk;
        folk.pantry
            .iter()
            .enumerate()
            .map(|(h, p)| {
                let trade = folk.people.iter().find(|q| q.house == h).map_or("?", |q| q.resident.trade.name());
                (trade.to_owned(), p.raw, p.meals, p.water)
            })
            .collect()
    }

    /// Fills every house's pantry (for testing).
    pub fn fill_pantries(&mut self) {
        if let Some(land) = self.land.as_mut() {
            for p in &mut land.folk.pantry {
                *p = life::Pantry { raw: 10.0, meals: 6.0, water: 8.0 };
            }
        }
    }

    pub fn villager_thoughts(&self) -> Vec<(Vec3, String)> {
        let Some(land) = self.ground() else { return Vec::new() };
        let mut out = Vec::new();
        for p in &land.folk.people {
            let Some(entity) = p.entity else { continue };
            let Ok(t) = self.ecs.get::<&Transform>(entity) else { continue };
            let act = match &p.life.act {
                Some(life::Act::Walk { step, .. }) => format!("indo {}", step.clip.unwrap_or("andar")),
                Some(life::Act::Use { step, left }) => format!("{} {:.0}s", step.clip.unwrap_or("parado"), left),
                Some(life::Act::Pause { .. }) => "olhando".into(),
                None => "pensando".into(),
            };
            let n = p.life.needs;
            let hand = p.life.tool.map(|(_, t)| format!(" · mão: {t:?}")).unwrap_or_default();
            let hand = if p.life.sick > 0.0 { format!("{hand} · DOENTE {:.0}h", p.life.sick) } else { hand };
            let text = format!(
                "{} ({})\n{act}{hand} · {} na fila\nfome {:.0} sono {:.0} conversa {:.0} dever {:.0}",
                p.resident.name,
                p.resident.role,
                p.life.plan.len(),
                n.hunger * 100.0,
                n.tired * 100.0,
                n.social * 100.0,
                n.duty * 100.0
            );
            out.push((t.position + Vec3::Y * (2.2 * t.scale.y + 0.3), text));
        }
        out
    }

    /// The village's places to go to (local, on the ground), named.
    pub fn village_places(&self) -> Vec<(String, Vec3)> {
        let Some(land) = self.ground() else { return Vec::new() };
        let Some(village) = land.relief.village() else { return Vec::new() };
        let origin = self.origin();
        let at = |p: DVec2| origin.to_local(DVec3::new(p.x, land.relief.height_at(p.x, p.y) + 0.3, p.y));
        let layout = &village.layout;
        let mut places = vec![("Largo".to_owned(), at(layout.largo.center)), ("Poço".to_owned(), at(layout.well))];
        if let Some(field) = layout.field {
            places.push(("Roça".to_owned(), at(field.center)));
        }
        if let Some((boat, _)) = layout.boat {
            places.push(("Bote".to_owned(), at(boat)));
        }
        for extra in &layout.extras {
            places.push((extra.name().to_owned(), at(extra.center)));
        }
        for lot in &layout.lots {
            places.push((format!("Casa: {}", lot.trade.name()), at(lot.door)));
        }
        places
    }
}
