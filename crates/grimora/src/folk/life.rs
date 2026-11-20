//! The villagers' life up close (stage 6b): needs that rise with the hours, the things they
//! use (seats, beds, the anvil, the well...) reserved one person at a time, the tasks of each
//! trade with the tool in the hand, talk in small groups that form and break up, glances at
//! who passes, and a way out when a walk gets nowhere.
//!
//! Cheap on purpose: one choice per finished action, distances only, no search. Far from the
//! player the folk stay data and go where their day says ([`Places`]).

use std::collections::VecDeque;

use super::*;

/// Seconds without getting 0.3 m closer before a walk is given up; and the longest walk.
const STUCK: f32 = 6.0;
/// Seconds a seated villager talks with the hands after saying a line (`sit_talk`).
const SEATED_TALK: f32 = 3.7;
/// Seconds a walk may take before it's given up: the river may be a few minutes away.
const LONGEST_WALK: f32 = 240.0;
/// Past a room's floor (meters), still in the house: more than a wall is thick.
const HOUSE_WALL: f32 = 0.4;
/// Seconds a thing someone gave up on stays out of their choices.
const AVOID: f32 = 60.0;
/// Seconds of one game hour for how fast needs rise while the day is stopped (`--time`
/// stops the clock, not needs); otherwise the day's own hour.
const HOUR: f32 = 20.0;
/// How far a group stands from its middle, and how far one looks for company, meters.
const RING: f64 = 0.75;
const COMPANY: f64 = 20.0;
/// How long getting up from a seat or a bed takes before walking, seconds.
const GETTING_UP: f32 = 1.4;

/// What a thing to use is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Seat,
    Bed,
    Counter,
    Anvil,
    Forge,
    Stove,
    Pots,
    Varal,
    /// In front of a tavern table, where a mug is set down.
    Serve,
    Well,
    Shore,
    Field,
    Largo,
    /// A patch of a house's floor to sweep.
    Sweep,
    /// At the head of a grave, kneeling or digging.
    Grave,
    /// By the beasts' pen.
    Pen,
    /// In front of a workbench.
    Bench,
    /// By a pile of firewood, to split it.
    Logs,
    /// A bed of the sickroom, no one's: for whoever is sick.
    WardBed,
}

/// One place to use something: where one stands (seats and beds: right on them, since
/// villagers pass through those), what one faces, and who has it.
#[derive(Debug, Clone)]
pub(super) struct UseSpot {
    pub kind: Kind,
    pub at: DVec3,
    pub face: DVec3,
    pub room: Option<Room>,
    pub house: Option<usize>,
    /// A bed's: which of the house's beds, and its sleeper.
    pub bed: Option<usize>,
    pub owner: Option<usize>,
    pub taken: Option<usize>,
    /// Put right there (seats, beds), not moved to free floor.
    pub exact: bool,
}

/// What can be in a villager's hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Tool {
    Hammer,
    Broom,
    Rod,
    Bucket,
    Mug,
    Ladle,
}

impl Tool {
    pub(super) fn file(self) -> &'static str {
        match self {
            Tool::Hammer => "ferraria/Martelo-de-Ferreiro.glb",
            Tool::Broom => "higiene-e-limpeza/Vassoura-de-Palha.glb",
            Tool::Rod => "Vara_Pesca.glb",
            Tool::Bucket => "Balde-de-Madeira.glb",
            Tool::Mug => "Caneca.glb",
            Tool::Ladle => "cozinha/Concha-de-Madeira.glb",
        }
    }

    /// The house prop that is this tool, if houses keep one (taken from there, put back).
    pub(super) fn what(self) -> Option<&'static str> {
        match self {
            Tool::Hammer => Some("Martelo-de-Ferreiro"),
            Tool::Broom => Some("Vassoura-de-Palha"),
            Tool::Bucket => Some("Balde-de-Madeira"),
            Tool::Ladle => Some("Concha-de-Madeira"),
            Tool::Rod => Some("Vara_Pesca"),
            Tool::Mug => Some("Caneca"),
        }
    }

    /// Whether this is a thing of the houses: in a hand only when someone took a real one from
    /// where it is kept, never out of nothing.
    pub(super) fn real(self) -> bool {
        true
    }

    /// What they call it when it is missing.
    pub(super) fn name(self) -> &'static str {
        match self {
            Tool::Hammer => "o martelo",
            Tool::Broom => "a vassoura",
            Tool::Rod => "a vara",
            Tool::Bucket => "o balde",
            Tool::Mug => "a caneca",
            Tool::Ladle => "a concha",
        }
    }

    /// The clip over the upper body while walking with it, if it wants one.
    pub(super) fn carry_clip(self) -> Option<&'static str> {
        match self {
            Tool::Bucket => Some("carry_bucket"),
            Tool::Broom | Tool::Rod => Some("carry_long"),
            _ => None,
        }
    }

    /// From `weapon.R` to the model. Long tools stand along the model's +Y, which is the way
    /// the routine clips point them (`blade`); the hand is where the handle is held.
    pub(super) fn offset(self) -> Transform {
        let position = match self {
            Tool::Hammer => Vec3::new(0.0, -0.06, 0.0),
            Tool::Broom => Vec3::new(0.0, -1.1, 0.0),
            Tool::Rod => Vec3::new(0.0, 0.05, 0.0),
            Tool::Bucket => Vec3::new(-0.18, -0.15, -0.02),
            Tool::Mug => Vec3::new(-0.08, -0.07, -0.02),
            Tool::Ladle => Vec3::new(0.0, -0.08, 0.0),
        };
        Transform { position, ..Default::default() }
    }
}

/// What a house has to eat: raw food (grain, fish, eggs and milk), cooked meals and water
/// for the pot. Work fills it, cooking turns raw into meals, eating empties it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Pantry {
    pub raw: f32,
    pub meals: f32,
    pub water: f32,
}

impl Default for Pantry {
    fn default() -> Self {
        Pantry { raw: 4.0, meals: 2.0, water: 2.0 }
    }
}

/// The most raw food a house keeps; the rest is sold to the tavern, which feeds whoever comes.
const PANTRY_KEEPS: f32 = 12.0;

/// A tool that lives somewhere in a house (a broom in a corner, the bucket by the stove): the
/// villager walks to it, takes it, and puts it back, or leaves it where the work ended.
#[derive(Debug, Clone)]
pub(crate) struct Stashed {
    pub house: usize,
    pub tool: Tool,
    /// Where it is now (world), and how it is turned.
    pub at: DVec3,
    pub rotation: Quat,
    /// Where it is kept, and always goes back to.
    pub home: DVec3,
    pub home_rotation: Quat,
    /// Its entity's name (`house:<id>:<what>`), to find it once the house stands.
    pub name: String,
    pub entity: Option<Entity>,
    pub held_by: Option<usize>,
    /// Taken by the player: no one finds it any more.
    pub gone: bool,
    /// The house has been to where it was kept and found it gone: until then they still go
    /// for it, because nobody knows what they have not seen.
    pub missed: bool,
}

/// A house prop to hide (it went into a hand) or show again somewhere, for the world to do.
#[derive(Debug, Clone, Copy)]
pub(crate) enum PropMove {
    Hide(Entity),
    Show(Entity, DVec3, Quat),
}

/// What finishing a step does to the village.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Effect {
    /// Food brought home: the catch, the harvest, eggs and milk.
    Produce(f32),
    /// Water from the well into the house's pot.
    Water,
    /// Raw food and water made into meals.
    Cook,
    /// A meal eaten from this house's pantry.
    Eat(usize),
    /// This stashed tool taken into the hand.
    Take(usize),
    /// This stashed tool put down: back where it was, or here.
    Leave(usize, bool),
}

/// What rises with time and is eased by doing things, 0..1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Need {
    Hunger,
    Tired,
    Social,
    Duty,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Needs {
    pub hunger: f32,
    pub tired: f32,
    pub social: f32,
    pub duty: f32,
}

impl Needs {
    fn get(&mut self, need: Need) -> &mut f32 {
        match need {
            Need::Hunger => &mut self.hunger,
            Need::Tired => &mut self.tired,
            Need::Social => &mut self.social,
            Need::Duty => &mut self.duty,
        }
    }
}

/// Where a step goes.
#[derive(Debug, Clone, Copy)]
pub(super) enum Target {
    Spot(usize),
    Point { at: DVec3, face: DVec3 },
    Group(u64),
}

/// One thing to do: go there (carrying something, maybe), stay `secs` playing `clip` with
/// `tool` in the hand, and ease a need.
#[derive(Debug, Clone)]
pub(super) struct Step {
    pub target: Target,
    pub secs: f32,
    pub clip: Option<&'static str>,
    pub tool: Option<Tool>,
    pub carry: Option<Tool>,
    pub eases: Option<(Need, f32)>,
    pub effect: Option<Effect>,
}

impl Step {
    fn at(target: Target, secs: f32, clip: Option<&'static str>) -> Step {
        Step { target, secs, clip, tool: None, carry: None, eases: None, effect: None }
    }
    fn does(mut self, effect: Effect) -> Step {
        self.effect = Some(effect);
        self
    }
    fn tool(mut self, tool: Tool) -> Step {
        self.tool = Some(tool);
        self
    }
    fn carry(mut self, tool: Tool) -> Step {
        self.carry = Some(tool);
        self
    }
    fn eases(mut self, need: Need, by: f32) -> Step {
        self.eases = Some((need, by));
        self
    }
}

/// What someone is doing now.
#[derive(Debug, Clone)]
pub(super) enum Act {
    /// `doors`: the houses (bits) whose street door this walk has reached, never gone back to.
    Walk {
        step: Step,
        left: f32,
        best: f64,
        since: f32,
        retried: bool,
        doors: u64,
    },
    Use {
        step: Step,
        left: f32,
    },
    Pause {
        left: f32,
        look: Option<Vec3>,
    },
}

/// A few people talking: they stand round `center`, one speaks while the others listen.
pub(super) struct Group {
    pub members: Vec<usize>,
    pub center: DVec3,
    pub left: f32,
    pub speaker: usize,
    pub turn: f32,
}

/// What a person has of this life.
#[derive(Debug, Default)]
pub(super) struct Life {
    pub needs: Needs,
    pub plan: VecDeque<Step>,
    pub act: Option<Act>,
    /// What should be in the hand, and what is (its entity).
    pub want_tool: Option<Tool>,
    pub tool: Option<(Entity, Tool)>,
    /// A spot given up on, and for how long it stays out.
    pub avoid: Option<(usize, f32)>,
    pub bed: Option<usize>,
    /// Game hours left being sick, in a sickroom bed.
    pub sick: f32,
}

fn seated(clip: Option<&str>) -> bool {
    matches!(clip, Some("sit_idle" | "sit_talk" | "eat" | "sleep"))
}

/// Lying asleep, in a bed or on the floor beside it.
fn sleeping(clip: Option<&str>) -> bool {
    matches!(clip, Some("sleep" | "sleep_floor"))
}

/// Where to stand to reach `thing` coming from `from` (world): a step short of it.
fn before(thing: DVec3, from: DVec3) -> DVec3 {
    let mut to = from - thing;
    to.y = 0.0;
    thing + to.normalize_or(DVec3::X) * 0.55
}

fn facing_to(from: Vec3, to: Vec3) -> f32 {
    let look = to - from;
    (-look.x).atan2(-look.z)
}

impl Folk {
    /// The things to use in the village: from the houses' furniture and the village's places
    /// (`well`, `field`, `boat`, `largo`, world x, z), all on `ground`.
    pub(super) fn make_spots(
        &mut self,
        houses: &[Built],
        largo: DVec2,
        well: DVec2,
        field: Option<DVec2>,
        shore: &[super::fishing::ShoreLine],
        graves: &[(DVec2, f64)],
        pen: Option<DVec2>,
        ground: &dyn Fn(DVec2) -> DVec3,
    ) {
        let mut spots = Vec::new();
        fn add(
            spots: &mut Vec<UseSpot>,
            kind: Kind,
            at: DVec3,
            face: DVec3,
            room: Option<Room>,
            house: Option<usize>,
            exact: bool,
        ) {
            spots.push(UseSpot { kind, at, face, room, house, bed: None, owner: None, taken: None, exact });
        }
        for (h, b) in houses.iter().enumerate() {
            let trade = b.lot.trade;
            let mut pots = 0;
            let mut tables = 0;
            let mut logs = 0;
            let house_name = format!("house:{:016x}", b.lot.id.0);
            // The tools that live in this house, where they are.
            for prop in &b.house.props {
                for tool in [Tool::Hammer, Tool::Broom, Tool::Bucket, Tool::Ladle, Tool::Rod, Tool::Mug] {
                    if tool.what() == Some(prop.what.as_str()) {
                        let (at, rotation) =
                            (b.world(prop.position), Quat::from_rotation_y(b.lot.yaw as f32) * prop.rotation);
                        self.stash.push(Stashed {
                            house: h,
                            tool,
                            at,
                            rotation,
                            home: at,
                            home_rotation: rotation,
                            name: format!("{house_name}:{}", prop.what),
                            entity: None,
                            held_by: None,
                            gone: false,
                            missed: false,
                        });
                    }
                }
            }
            for prop in &b.house.props {
                let front = prop.rotation * Vec3::Z;
                let side = prop.rotation * Vec3::X;
                let room = b.room_of(prop.position);
                let what = prop.what.as_str();
                // A seat: the hips go back onto it, so the root is a little in front.
                let seat = |at: Vec3| (b.world(at + front * 0.25), b.world(at + front * 2.25));
                if what.starts_with("Cadeira") || what.starts_with("Banqueta") {
                    let (at, face) = seat(prop.position);
                    add(&mut spots, Kind::Seat, at, face, room, Some(h), true);
                } else if what.starts_with("Banco") {
                    for x in [-0.35, 0.35] {
                        let (at, face) = seat(prop.position + side * x);
                        add(&mut spots, Kind::Seat, at, face, room, Some(h), true);
                    }
                } else if what.starts_with("Mesa") && trade == Trade::Tavern {
                    if let Some(s) = b.before("Mesa", tables) {
                        add(&mut spots, Kind::Serve, s.at, s.face, s.room, Some(h), false);
                    }
                    tables += 1;
                } else {
                    let kind = match what {
                        w if w.starts_with("Balcao") => Some(Kind::Counter),
                        w if w.starts_with("Bigorna") => Some(Kind::Anvil),
                        w if w.starts_with("Forja") => Some(Kind::Forge),
                        w if w.starts_with("Fogao") => Some(Kind::Stove),
                        w if w.starts_with("Varal") => Some(Kind::Varal),
                        w if w.starts_with("Bancada") => Some(Kind::Bench),
                        w if w.starts_with("Lenha") && logs == 0 => {
                            logs += 1;
                            Some(Kind::Logs)
                        }
                        w if trade == Trade::Healer
                            && (w.starts_with("Pote") || w.starts_with("Jarra") || w.starts_with("Prateleira"))
                            && pots < 3 =>
                        {
                            pots += 1;
                            Some(Kind::Pots)
                        }
                        _ => None,
                    };
                    let Some(kind) = kind else { continue };
                    let spot = if kind == Kind::Counter {
                        b.behind("Balcao")
                    } else {
                        let ahead = front * BEFORE as f32;
                        Some(Spot { at: b.world(prop.position + ahead * 0.6), face: b.world(prop.position), room })
                    };
                    if let Some(s) = spot {
                        add(&mut spots, kind, s.at, s.face, s.room, Some(h), false);
                    }
                }
            }
            // Beds: one place to lie per sleeper, lying along the bed with the head at the
            // headboard (the bed's -Z, against the wall) and the feet into the room.
            for (k, (&pos, &sleeps)) in b.house.beds.iter().zip(&b.house.sleeps).enumerate() {
                let prop = b
                    .house
                    .props
                    .iter()
                    .filter(|p| ["Cama", "Beliche", "Enxergao"].iter().any(|w| p.what.starts_with(w)))
                    .min_by(|x, y| x.position.distance_squared(pos).total_cmp(&y.position.distance_squared(pos)));
                let (rotation, double) =
                    prop.map_or((Quat::IDENTITY, false), |p| (p.rotation, p.what.starts_with("Cama-Casal")));
                let front = rotation * Vec3::Z;
                let side = rotation * Vec3::X;
                let base = prop.map_or(pos, |p| p.position);
                let offsets: &[f32] = if double { &[-0.33, 0.33] } else { &[0.0, 0.0] };
                for &x in offsets.iter().take(sleeps.max(1)) {
                    let at = base - front * 0.075 + side * x;
                    spots.push(UseSpot {
                        kind: Kind::Bed,
                        at: b.world(at),
                        face: b.world(at + front * 3.0),
                        room: b.room_of(base),
                        house: Some(h),
                        bed: Some(k),
                        owner: None,
                        taken: None,
                        exact: true,
                    });
                }
            }
            // The sickroom's beds: lie along them, as in one's own.
            for &pos in &b.house.ward_beds {
                let prop = b
                    .house
                    .props
                    .iter()
                    .min_by(|x, y| x.position.distance_squared(pos).total_cmp(&y.position.distance_squared(pos)));
                let front = prop.map_or(Vec3::Z, |p| p.rotation * Vec3::Z);
                let at = pos - front * 0.075;
                spots.push(UseSpot {
                    kind: Kind::WardBed,
                    at: b.world(at),
                    face: b.world(at + front * 3.0),
                    room: b.room_of(pos),
                    house: Some(h),
                    bed: None,
                    owner: None,
                    taken: None,
                    exact: true,
                });
            }
            // A patch of floor to sweep in the first room.
            if let Some(&(min, max)) = b.house.rooms.first() {
                let middle = (min + max) * 0.5;
                let at = Vec3::new(middle.x, 0.0, middle.y);
                let mut add_sweep = |at: Vec3| {
                    spots.push(UseSpot {
                        kind: Kind::Sweep,
                        at: b.world(at),
                        face: b.world(at + Vec3::Z),
                        room: b.room_of(at),
                        house: Some(h),
                        bed: None,
                        owner: None,
                        taken: None,
                        exact: false,
                    })
                };
                add_sweep(at);
            }
        }
        let mut ring = |kind: Kind, middle: DVec2, radius: f64, n: usize, face_in: bool| {
            for k in 0..n {
                let a = k as f64 / n as f64 * std::f64::consts::TAU + 0.4;
                let at = ground(middle + DVec2::new(a.cos(), a.sin()) * radius);
                let face = if face_in {
                    ground(middle)
                } else {
                    ground(middle + DVec2::new(a.cos(), a.sin()) * (radius + 3.0))
                };
                spots.push(UseSpot {
                    kind,
                    at,
                    face,
                    room: None,
                    house: None,
                    bed: None,
                    owner: None,
                    taken: None,
                    exact: false,
                });
            }
        };
        ring(Kind::Well, well, 1.2, 4, true);
        ring(Kind::Largo, largo, 5.0, 8, false);
        if let Some(field) = field {
            ring(Kind::Field, field, 5.0, 6, true);
        }
        if let Some(pen) = pen {
            ring(Kind::Pen, pen, 5.6, 4, true);
        }
        // On the bank, facing the water: the float goes where they look.
        for line in shore {
            spots.push(UseSpot {
                kind: Kind::Shore,
                at: line.stand,
                face: line.stand + DVec3::new(line.toward.x, 0.0, line.toward.y) * 4.0,
                room: None,
                house: None,
                bed: None,
                owner: None,
                taken: None,
                exact: true,
            });
        }
        // At the foot of each grave, looking at its marker.
        for &(at, yaw) in graves {
            let toward = DVec2::new(yaw.sin(), yaw.cos());
            // At the foot of the grave, looking to its head (toward the cross at the back).
            let foot = ground(at - toward * 1.3);
            spots.push(UseSpot {
                kind: Kind::Grave,
                at: foot,
                face: ground(at + toward),
                room: None,
                house: None,
                bed: None,
                owner: None,
                taken: None,
                exact: false,
            });
        }
        self.spots = spots;
        self.pantry = vec![Pantry::default(); houses.len()];
    }

    /// Gives each sleeper their place in their bed.
    pub(super) fn give_beds(&mut self) {
        for i in 0..self.people.len() {
            let (house, bed) = (self.people[i].house, self.people[i].resident.bed);
            let Some(bed) = bed else { continue };
            if let Some(s) = self
                .spots
                .iter()
                .position(|s| s.kind == Kind::Bed && s.house == Some(house) && s.bed == Some(bed) && s.owner.is_none())
            {
                self.spots[s].owner = Some(i);
                self.people[i].life.bed = Some(s);
            }
        }
    }

    /// Moves the standing places to free floor, as [`Folk::settle`] does for the day's.
    pub(super) fn settle_spots(&mut self, physics: &Physics, origin: &grimora_core::Origin) {
        let shape = grimora_physics::NavShape::new(0.38, 1.8);
        for spot in self.spots.iter_mut().filter(|s| !s.exact) {
            let center = spot.at;
            let free = (0..=8).find_map(|ring| {
                let radius = ring as f64 * 0.35;
                let n = if ring == 0 { 1 } else { 6 + ring * 3 };
                (0..n).find_map(|k| {
                    let a = k as f64 / n as f64 * std::f64::consts::TAU;
                    let p = center + DVec3::new(a.cos() * radius, 0.0, a.sin() * radius);
                    let floor = origin.to_world(physics.stands_at(origin.to_local(p), &shape)?);
                    spot.room.is_none_or(|room| room.contains(floor)).then_some(floor)
                })
            });
            if let Some(floor) = free {
                spot.at = floor;
            }
        }
    }

    /// The nearest free spot of `kind` (in `house`, if given) to `near`.
    fn free(&self, i: usize, kind: Kind, house: Option<usize>, near: DVec3) -> Option<usize> {
        let avoid = self.people[i].life.avoid.map(|(s, _)| s);
        self.spots
            .iter()
            .enumerate()
            .filter(|(k, s)| {
                s.kind == kind
                    && (house.is_none() || s.house == house)
                    && s.taken.is_none_or(|t| t == i)
                    && s.owner.is_none_or(|o| o == i)
                    && Some(*k) != avoid
            })
            .min_by(|a, b| a.1.at.distance_squared(near).total_cmp(&b.1.at.distance_squared(near)))
            .map(|(k, _)| k)
    }

    fn release(&mut self, i: usize) {
        for spot in &mut self.spots {
            if spot.taken == Some(i) {
                spot.taken = None;
            }
        }
    }

    /// Drops what `i` was doing (they went away, or the clock jumped).
    pub(crate) fn forget(&mut self, i: usize) {
        self.release(i);
        self.drop_held(i);
        let life = &mut self.people[i].life;
        life.plan.clear();
        life.act = None;
        life.want_tool = None;
        for group in self.groups.values_mut() {
            group.members.retain(|&m| m != i);
        }
    }

    /// Whether `i` is lying asleep: sees nothing, greets nobody.
    pub(crate) fn asleep(&self, i: usize) -> bool {
        matches!(&self.people[i].life.act, Some(Act::Use { step, .. }) if sleeping(step.clip))
    }

    /// `i` died: what was in the hand falls right there, and the rest is let go of.
    pub(super) fn fall(&mut self, i: usize) {
        if let Some(k) = self.stash.iter().position(|s| s.held_by == Some(i)) {
            self.put_down(i, k, false);
        }
        self.forget(i);
    }

    /// What `i` does next, by the most pressing need that has something free to do it with,
    /// weighted by what their day says of this hour.
    fn choose(
        &mut self,
        i: usize,
        doing: Doing,
        here: DVec3,
        physics: &Physics,
        origin: &grimora_core::Origin,
        roll: u64,
    ) -> Vec<Step> {
        let p = &self.people[i];
        let n = p.life.needs;
        let u = |k: u64| (path_seed(&[roll, k]) >> 11) as f32 / (1u64 << 53) as f32;
        let (w_sleep, w_eat, w_work, w_social) = match doing {
            Doing::Sleep => (5.0, 0.05, 0.0, 0.0),
            Doing::Eat => (0.1, 3.0, 0.1, 0.6),
            Doing::Work => (0.05, 0.4, 2.5, 0.3),
            Doing::Leisure => (0.2, 0.8, 0.1, 2.5),
            Doing::Home => (0.5 * n.tired, 0.8, 1.0, 1.0),
        };
        let mut wants = [
            (Need::Tired, n.tired.max(if doing == Doing::Sleep { 0.6 } else { 0.0 }) * w_sleep),
            (Need::Hunger, n.hunger * w_eat),
            (Need::Duty, n.duty * w_work),
            (Need::Social, n.social * w_social),
        ];
        for (k, w) in wants.iter_mut().enumerate() {
            w.1 += u(k as u64) * 0.15;
        }
        wants.sort_by(|a, b| b.1.total_cmp(&a.1));
        let house = p.house;
        // The sick lie in the sickroom, or at home if there is none, until it passes.
        if p.life.sick > 0.0 && doing != Doing::Sleep {
            let bed =
                self.free(i, Kind::WardBed, None, here).map(Target::Spot).or_else(|| p.life.bed.map(Target::Spot));
            if let Some(bed) = bed {
                return vec![Step::at(bed, 40.0, Some("sleep")).eases(Need::Tired, 0.2)];
            }
        }
        for (need, _) in wants {
            let steps = match need {
                Need::Tired => self.sleep(i),
                Need::Hunger => self.eat(i, doing, here),
                Need::Duty => self.work(i, doing, here, u(7)),
                Need::Social => self.talk(i, doing, here, physics, origin, u(8)),
            };
            if !steps.is_empty() {
                return steps;
            }
        }
        // Nothing to do: sit at home a while, or stand on the largo.
        match self.free(i, Kind::Seat, Some(house), here) {
            Some(seat) if doing != Doing::Work => {
                vec![Step::at(Target::Spot(seat), 20.0 + 20.0 * u(9), Some("sit_idle"))]
            }
            _ => {
                let spot = self.people[i].places.of(doing);
                let a = u(10) as f64 * std::f64::consts::TAU;
                let at = spot.at + DVec3::new(a.cos(), 0.0, a.sin()) * (0.5 + 2.0 * u(11) as f64);
                // Indoors: somewhere in the room, never through its walls.
                let at = spot.room.map_or(at, |room| room.point(u(13) as f64, u(14) as f64));
                vec![Step::at(Target::Point { at, face: spot.face }, 8.0 + 10.0 * u(12), None)]
            }
        }
    }

    fn sleep(&self, i: usize) -> Vec<Step> {
        let avoided = self.people[i].life.avoid.map(|(s, _)| s);
        match self.people[i].life.bed.filter(|&bed| Some(bed) != avoided) {
            Some(bed) => vec![Step::at(Target::Spot(bed), 45.0, Some("sleep")).eases(Need::Tired, 0.35)],
            None => {
                let home = self.people[i].places.bed;
                vec![
                    Step::at(Target::Point { at: home.at, face: home.face }, 45.0, Some("sleep_floor"))
                        .eases(Need::Tired, 0.3),
                ]
            }
        }
    }

    fn eat(&self, i: usize, doing: Doing, here: DVec3) -> Vec<Step> {
        let p = &self.people[i];
        let tavern = self.tavern_house();
        let mut house = if doing == Doing::Leisure && p.resident.tavern { tavern.unwrap_or(p.house) } else { p.house };
        let mut steps = Vec::new();
        let pantry = self.pantry.get(house).copied().unwrap_or_default();
        if pantry.meals < 1.0 {
            if pantry.raw >= 1.0 && house == p.house {
                // Nothing cooked: water from the well if the pot is dry, then the stove.
                if pantry.water < 1.0 {
                    steps.extend(self.fetch_water(i, here));
                }
                if self.has(i, Tool::Ladle)
                    && let Some(stove) = self.free(i, Kind::Stove, Some(house), here)
                {
                    steps.extend(self.fetch(i, Tool::Ladle, here));
                    steps.push(Step::at(Target::Spot(stove), 18.0, Some("stir")).tool(Tool::Ladle).does(Effect::Cook));
                }
            } else if let Some(t) =
                tavern.filter(|&t| t != house && self.pantry.get(t).is_some_and(|q| q.meals >= 1.0 || q.raw >= 1.0))
            {
                // Nothing at home: the tavern feeds whoever comes.
                house = t;
            } else {
                // Nothing anywhere: a crust at the table, and back to work.
                let table = p.places.table;
                return vec![
                    Step::at(Target::Point { at: table.at, face: table.face }, 8.0, None).eases(Need::Hunger, 0.15),
                ];
            }
        }
        match self.free(i, Kind::Seat, Some(house), here) {
            Some(seat) => steps.push(
                Step::at(Target::Spot(seat), 25.0, Some("eat")).eases(Need::Hunger, 0.7).does(Effect::Eat(house)),
            ),
            None => {
                let table = p.places.table;
                steps.push(
                    Step::at(Target::Point { at: table.at, face: table.face }, 15.0, None)
                        .eases(Need::Hunger, 0.5)
                        .does(Effect::Eat(house)),
                );
            }
        }
        steps
    }

    pub(super) fn tavern_house(&self) -> Option<usize> {
        self.spots.iter().find(|s| s.kind == Kind::Counter).and_then(|s| s.house)
    }

    /// The walk to where `tool` is kept in `i`'s house and taking it; nothing if it's already
    /// in the hand or the house keeps none (then it just appears in the hand).
    fn fetch(&self, i: usize, tool: Tool, here: DVec3) -> Option<Step> {
        let house = self.people[i].house;
        if self.stash.iter().any(|s| s.held_by == Some(i) && s.tool == tool) {
            return None;
        }
        let (k, s) = self
            .stash
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                s.house == house
                    && s.tool == tool
                    && s.held_by.is_none()
                    && if s.gone { !s.missed } else { s.entity.is_some() }
            })
            .min_by(|a, b| a.1.at.distance_squared(here).total_cmp(&b.1.at.distance_squared(here)))?;
        Some(Step::at(Target::Point { at: before(s.at, here), face: s.at }, 1.4, Some("pick_up")).does(Effect::Take(k)))
    }

    /// Whether `i`'s house has `tool` to hand: kept and still there, or not kept at all.
    fn has(&self, i: usize, tool: Tool) -> bool {
        let house = self.people[i].house;
        let kept: Vec<&Stashed> = self.stash.iter().filter(|s| s.house == house && s.tool == tool).collect();
        (kept.is_empty() && !tool.real())
            || kept.iter().any(|s| !(s.gone && s.missed) && (s.held_by.is_none() || s.held_by == Some(i)))
    }

    /// To the well with the bucket, drawing, and the water home to the stove.
    fn fetch_water(&self, i: usize, here: DVec3) -> Vec<Step> {
        let p = &self.people[i];
        if !self.has(i, Tool::Bucket) {
            return Vec::new();
        }
        let Some(well) = self.free(i, Kind::Well, None, here) else { return Vec::new() };
        let mut steps: Vec<Step> = self.fetch(i, Tool::Bucket, here).into_iter().collect();
        steps.push(Step::at(Target::Spot(well), 7.0, Some("well_draw")).tool(Tool::Bucket).carry(Tool::Bucket));
        let back = self
            .free(i, Kind::Stove, Some(p.house), here)
            .map(Target::Spot)
            .unwrap_or(Target::Point { at: p.places.home.at, face: p.places.home.face });
        steps.push(
            Step::at(back, 3.0, None)
                .tool(Tool::Bucket)
                .carry(Tool::Bucket)
                .does(Effect::Water)
                .eases(Need::Duty, 0.25),
        );
        steps
    }

    /// The tasks of `i`'s work, in order; at home, the chores.
    fn work(&self, i: usize, doing: Doing, here: DVec3, pick: f32) -> Vec<Step> {
        let p = &self.people[i];
        let house = Some(p.house);
        let spot = |kind, house| self.free(i, kind, house, here).map(Target::Spot);
        let mut steps = Vec::new();
        let chores = doing == Doing::Home || matches!(p.resident.post, Post::Home);
        let child = p.resident.age < 14;
        if child {
            // Children play on the largo: running from one side to another.
            for k in 0..3 {
                let n = self.spots.iter().filter(|s| s.kind == Kind::Largo).count().max(1);
                let which = (path_seed(&[p.resident.id.0, k, (pick * 1000.0) as u64]) % n as u64) as usize;
                if let Some(s) = self.spots.iter().enumerate().filter(|(_, s)| s.kind == Kind::Largo).nth(which) {
                    steps.push(
                        Step::at(Target::Point { at: s.1.at, face: s.1.face }, 3.0, None).eases(Need::Duty, 0.15),
                    );
                }
            }
            return steps;
        }
        if !chores {
            match (p.resident.post, p.resident.trade) {
                (Post::Field, _) | (_, Trade::Home) => {
                    for _ in 0..2 {
                        if let Some(t) = spot(Kind::Field, None) {
                            steps.push(
                                Step::at(t, 35.0, Some(if pick < 0.5 { "tend" } else { "dig" }))
                                    .eases(Need::Duty, 0.3)
                                    .does(Effect::Produce(1.5)),
                            );
                            break;
                        }
                    }
                }
                (_, Trade::Smith) => {
                    if let Some(t) = spot(Kind::Forge, house) {
                        steps.push(Step::at(t, 10.0, None));
                    }
                    if self.has(i, Tool::Hammer)
                        && let Some(t) = spot(Kind::Anvil, house)
                    {
                        steps.extend(self.fetch(i, Tool::Hammer, here));
                        steps.push(Step::at(t, 25.0, Some("hammer")).tool(Tool::Hammer).eases(Need::Duty, 0.3));
                    }
                    if pick < 0.5
                        && self.has(i, Tool::Bucket)
                        && let Some(t) = spot(Kind::Well, None)
                    {
                        steps.extend(self.fetch(i, Tool::Bucket, here));
                        steps.push(Step::at(t, 6.0, Some("well_draw")).tool(Tool::Bucket).carry(Tool::Bucket));
                        if let Some(t) = spot(Kind::Forge, house) {
                            steps.push(
                                Step::at(t, 5.0, None).tool(Tool::Bucket).carry(Tool::Bucket).eases(Need::Duty, 0.2),
                            );
                        }
                    }
                }
                (_, Trade::Fisher) => {
                    if self.has(i, Tool::Rod)
                        && let Some(t) = spot(Kind::Shore, None)
                    {
                        steps.extend(self.fetch(i, Tool::Rod, here));
                        steps.push(
                            Step::at(t, 50.0 + 40.0 * pick, Some("fish"))
                                .tool(Tool::Rod)
                                .carry(Tool::Rod)
                                .eases(Need::Duty, 0.5)
                                .does(Effect::Produce(2.5)),
                        );
                    }
                }
                (_, Trade::Tavern)
                    if pick < 0.35 && self.pantry.get(p.house).is_some_and(|q| q.raw >= 1.0 && q.meals < 6.0) =>
                {
                    // The pot for whoever comes to eat.
                    if self.pantry[p.house].water < 1.0 {
                        steps.extend(self.fetch_water(i, here));
                    }
                    if self.has(i, Tool::Ladle)
                        && let Some(t) = spot(Kind::Stove, house)
                    {
                        steps.extend(self.fetch(i, Tool::Ladle, here));
                        steps.push(
                            Step::at(t, 20.0, Some("stir")).tool(Tool::Ladle).does(Effect::Cook).eases(Need::Duty, 0.3),
                        );
                    }
                }
                (_, Trade::Tavern) => {
                    if let Some(t) = spot(Kind::Counter, house) {
                        steps.push(Step::at(t, 15.0 + 10.0 * pick, None));
                    }
                    if self.has(i, Tool::Mug)
                        && let Some(t) = spot(Kind::Serve, house)
                    {
                        steps.extend(self.fetch(i, Tool::Mug, here));
                        steps.push(Step::at(t, 3.0, None).tool(Tool::Mug).carry(Tool::Mug).eases(Need::Duty, 0.25));
                    }
                }
                (_, Trade::Healer) if self.sick_in_ward().is_some() => {
                    // By the bed of the sick: a hand on the brow, the bowl.
                    let (at, face) = self.sick_in_ward().unwrap();
                    steps.push(Step::at(Target::Point { at, face }, 20.0, Some("tend")).eases(Need::Duty, 0.3));
                    if let Some(t) = spot(Kind::Pots, house) {
                        steps.push(Step::at(t, 8.0, Some("stir")).eases(Need::Duty, 0.1));
                    }
                }
                (_, Trade::Herder) => {
                    // Water for the beasts, then the pen: feeding, milking, eggs.
                    steps.extend(self.fetch_water(i, here).into_iter().take(2));
                    if let Some(t) = spot(Kind::Pen, None) {
                        let carrying = steps.last().and_then(|s| s.carry);
                        let mut step =
                            Step::at(t, 30.0, Some("tend")).eases(Need::Duty, 0.4).does(Effect::Produce(1.5));
                        if let Some(c) = carrying {
                            step = step.tool(c).carry(c);
                        }
                        steps.push(step);
                    }
                }
                (_, Trade::Sexton) => {
                    if let Some(t) = spot(Kind::Grave, None) {
                        steps.push(Step::at(t, 30.0, Some("dig")).eases(Need::Duty, 0.4));
                    }
                    if pick < 0.5
                        && let Some(t) = spot(Kind::Grave, None)
                    {
                        steps.push(
                            Step::at(t, 15.0, Some(if pick < 0.25 { "kneel_pray" } else { "sweep" }))
                                .eases(Need::Duty, 0.2),
                        );
                    }
                }
                (_, Trade::Carpenter) => {
                    if pick < 0.4
                        && let Some(t) = spot(Kind::Logs, house)
                    {
                        steps.push(Step::at(t, 20.0, Some("chop")).eases(Need::Duty, 0.3));
                    }
                    if self.has(i, Tool::Hammer)
                        && let Some(t) = spot(Kind::Bench, house)
                    {
                        steps.extend(self.fetch(i, Tool::Hammer, here));
                        steps.push(Step::at(t, 30.0, Some("hammer")).tool(Tool::Hammer).eases(Need::Duty, 0.4));
                    }
                }
                (_, Trade::Healer) => {
                    if let Some(t) = spot(Kind::Pots, house) {
                        steps.push(Step::at(t, 15.0, Some("tend")).eases(Need::Duty, 0.2));
                    }
                    let table = p.places.work;
                    steps.push(
                        Step::at(Target::Point { at: table.at, face: table.face }, 10.0, None).eases(Need::Duty, 0.1),
                    );
                }
                (_, Trade::Chief) => {
                    // Walks the largo and looks round.
                    for k in 0..2 {
                        let n = self.spots.iter().filter(|s| s.kind == Kind::Largo).count().max(1);
                        let which = (path_seed(&[p.resident.id.0, k, (pick * 1000.0) as u64]) % n as u64) as usize;
                        if let Some((_, s)) =
                            self.spots.iter().enumerate().filter(|(_, s)| s.kind == Kind::Largo).nth(which)
                        {
                            steps.push(
                                Step::at(Target::Point { at: s.at, face: s.face }, 12.0, None).eases(Need::Duty, 0.2),
                            );
                        }
                    }
                }
            }
        }
        if steps.is_empty() {
            // Chores: sweep, fetch water, hang the washing.
            let chore = (pick * 3.0) as usize;
            for k in 0..3 {
                match (chore + k) % 3 {
                    0 => {
                        if self.has(i, Tool::Broom)
                            && let Some(t) = spot(Kind::Sweep, house)
                        {
                            steps.extend(self.fetch(i, Tool::Broom, here));
                            steps.push(
                                Step::at(t, 25.0, Some("sweep"))
                                    .tool(Tool::Broom)
                                    .carry(Tool::Broom)
                                    .eases(Need::Duty, 0.3),
                            );
                        }
                    }
                    1 => steps.extend(self.fetch_water(i, here)),
                    _ => {
                        if let Some(t) = spot(Kind::Varal, house) {
                            steps.push(Step::at(t, 15.0, None).eases(Need::Duty, 0.25));
                        }
                    }
                }
                if !steps.is_empty() {
                    break;
                }
            }
        }
        steps
    }

    /// Company: a seat at the tavern for those who go there, else a small group with whoever
    /// is near and free.
    fn talk(
        &mut self,
        i: usize,
        doing: Doing,
        here: DVec3,
        physics: &Physics,
        origin: &grimora_core::Origin,
        pick: f32,
    ) -> Vec<Step> {
        if doing == Doing::Sleep {
            return Vec::new();
        }
        // Now and then the old and the grieving go to the graves instead.
        let mourns = self.people[i].resident.age >= 40 && pick < 0.12 || pick < 0.04;
        if mourns && let Some(g) = self.free(i, Kind::Grave, None, here) {
            let how = if pick < 0.06 { "kneel_pray" } else { "pray" };
            return vec![Step::at(Target::Spot(g), 25.0, Some(how)).eases(Need::Social, 0.35)];
        }
        let p = &self.people[i];
        if doing == Doing::Leisure && p.resident.tavern {
            let tavern = self.spots.iter().find(|s| s.kind == Kind::Counter).and_then(|s| s.house);
            if let Some(seat) = tavern.and_then(|h| self.free(i, Kind::Seat, Some(h), here)) {
                return vec![
                    Step::at(Target::Spot(seat), 40.0 + 30.0 * pick, Some("sit_idle")).eases(Need::Social, 0.5),
                ];
            }
        }
        // Join a group near that has room, or start one with the free people near.
        let near = |j: usize| self.people[j].entity.is_some() && self.people[j].at.distance(here) < COMPANY;
        if let Some((&id, _)) = self
            .groups
            .iter()
            .find(|(_, g)| g.members.len() < 3 && g.center.distance(here) < COMPANY && !g.members.contains(&i))
        {
            self.groups.get_mut(&id).unwrap().members.push(i);
            return vec![Step::at(Target::Group(id), 60.0, None).eases(Need::Social, 0.5)];
        }
        let mut mates: Vec<usize> = (0..self.people.len())
            .filter(|&j| j != i && near(j))
            .filter(|&j| {
                let q = &self.people[j];
                q.fear <= 0.0
                    && q.angry <= 0.0
                    && q.life.plan.is_empty()
                    && matches!(q.life.act, None | Some(Act::Pause { .. }))
                    && !matches!(q.resident.day.doing(self.hour.unwrap_or(9.0)), Doing::Sleep | Doing::Work)
                    && !self.groups.values().any(|g| g.members.contains(&j))
            })
            .collect();
        if mates.is_empty() {
            return Vec::new();
        }
        mates.sort_by(|&a, &b| self.people[a].at.distance(here).total_cmp(&self.people[b].at.distance(here)));
        mates.truncate(if pick < 0.5 { 1 } else { 2 });
        let mut middle = here;
        for &j in &mates {
            middle += self.people[j].at;
        }
        middle /= (mates.len() + 1) as f64;
        let shape = grimora_physics::NavShape::new(0.38, 1.8);
        let center = physics.stands_at(origin.to_local(middle), &shape).map_or(here, |f| origin.to_world(f));
        self.next_group += 1;
        let id = self.next_group;
        let mut members = vec![i];
        members.extend(&mates);
        let secs = 20.0 + 40.0 * pick;
        for &j in &mates {
            let life = &mut self.people[j].life;
            life.plan.clear();
            life.plan.push_back(Step::at(Target::Group(id), 60.0, None).eases(Need::Social, 0.5));
            life.act = None;
        }
        self.groups.insert(id, Group { members, center, left: secs, speaker: i, turn: 1.0 });
        vec![Step::at(Target::Group(id), 60.0, None).eases(Need::Social, 0.5)]
    }

    /// Where a step goes and what it faces (world), for `i`.
    /// Where to walk now on the way from `here` to `goal` (world): just outside the street door
    /// of a house that one of them is in and the other isn't, until there; then `goal`. A door
    /// reached is added to `doors` and not walked back to, or the step past it, not yet in the
    /// room, would send them back out to it.
    fn way_to(&self, here: DVec3, goal: DVec3, doors: &mut u64) -> DVec3 {
        // The whole house, its walls and doorways too: in a doorway between two rooms one is
        // still inside, or the way would turn back out to the street door there.
        let inside = |h: usize, p: DVec3| self.rooms[h].iter().any(|room| room.within(p, HOUSE_WALL));
        for (h, &front) in self.fronts.iter().enumerate().take(64) {
            if *doors & (1 << h) != 0 || inside(h, here) == inside(h, goal) {
                continue;
            }
            if DVec2::new(front.x - here.x, front.z - here.z).length() < 1.2 {
                *doors |= 1 << h;
                continue;
            }
            return front;
        }
        goal
    }

    fn place_of(&self, i: usize, target: Target) -> Option<(DVec3, DVec3, bool)> {
        match target {
            Target::Spot(s) => self.spots.get(s).map(|s| (s.at, s.face, s.exact)),
            Target::Point { at, face } => Some((at, face, false)),
            Target::Group(id) => {
                let group = self.groups.get(&id)?;
                let k = group.members.iter().position(|&m| m == i)?;
                let n = group.members.len().max(2) as f64;
                let a = k as f64 / n * std::f64::consts::TAU + (id % 7) as f64;
                Some((group.center + DVec3::new(a.cos(), 0.0, a.sin()) * RING, group.center, false))
            }
        }
    }

    /// One frame of `i`'s life, as an entity: needs rise, and the action in hand goes on.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn live(
        &mut self,
        i: usize,
        agent: &mut Agent,
        here: Vec3,
        last: &[Option<Vec3>],
        origin: &grimora_core::Origin,
        physics: &mut Physics,
        doing: Doing,
        raining: bool,
        dt: f32,
    ) {
        let hours = dt / if self.hour_secs > 0.0 { self.hour_secs } else { HOUR };
        {
            let life = &mut self.people[i].life;
            let n = &mut life.needs;
            n.hunger = (n.hunger + 0.11 * hours).min(1.0);
            n.social = (n.social + 0.2 * hours).min(1.0);
            n.duty = (n.duty + if doing == Doing::Work { 0.35 } else { 0.03 } * hours).min(1.0);
            if !matches!(&life.act, Some(Act::Use { step, .. }) if sleeping(step.clip)) {
                n.tired = (n.tired + 0.05 * hours).min(1.0);
            }
            if life.sick > 0.0 {
                life.sick = (life.sick - hours).max(0.0);
            }
        }
        {
            // About one in thirty a day falls ill, for a day or two.
            let (id, moves) = (self.people[i].resident.id.0, self.people[i].moves);
            if self.people[i].life.sick <= 0.0
                && ((path_seed(&[id, (moves << 8) ^ (dt.to_bits() as u64)]) % 1_000_000) as f32) < hours * 1500.0
            {
                self.people[i].life.sick = 18.0 + (id % 30) as f32;
                log::info!("Folk: {} ficou doente", self.people[i].resident.name);
            }
            let life = &mut self.people[i].life;
            if let Some((_, left)) = &mut life.avoid {
                *left -= dt;
                if *left <= 0.0 {
                    life.avoid = None;
                }
            }
        }
        let world_here = origin.to_world(here);
        self.people[i].at = world_here;
        let id = self.people[i].resident.id.0;
        let roll = path_seed(&[id, self.people[i].moves]);
        // Rain: nothing is done outdoors. What's in hand goes back, and they hurry in.
        if raining {
            let life = &self.people[i].life;
            let out = match &life.act {
                Some(Act::Walk { step, .. }) | Some(Act::Use { step, .. }) => self.outdoors(i, step),
                _ => false,
            } || life.plan.front().is_some_and(|s| self.outdoors(i, s));
            if out {
                self.shelter(i, world_here, roll);
            }
        }
        agent.running =
            raining && !self.indoors(world_here) && !matches!(self.people[i].life.act, Some(Act::Use { .. }));
        match self.people[i].life.act.take() {
            None => {
                agent.rest_clip = None;
                let next = self.people[i].life.plan.pop_front();
                match next {
                    Some(step) => self.start(i, step, agent, origin),
                    None => {
                        self.people[i].moves += 1;
                        let steps = self.choose(i, doing, world_here, physics, origin, roll);
                        if raining && steps.iter().any(|s| self.outdoors(i, s)) && self.inside(i, roll).is_some() {
                            self.shelter(i, world_here, roll);
                        } else {
                            self.people[i].life.plan.extend(steps);
                        }
                        // A short pause between things: a look round first.
                        let u = (roll >> 20) as f32 / (1u64 << 44) as f32;
                        self.people[i].life.act = Some(Act::Pause { left: 0.8 + 3.0 * u.fract(), look: None });
                    }
                }
            }
            Some(Act::Pause { left, look }) => {
                agent.rest_clip = None;
                // Turn to whoever passes close, or look somewhere.
                let passer = last
                    .iter()
                    .enumerate()
                    .filter_map(|(j, w)| Some((j, (*w)?)))
                    .filter(|&(j, w)| j != i && w.distance(here) < 5.0)
                    .min_by(|a, b| a.1.distance(here).total_cmp(&b.1.distance(here)))
                    .map(|(_, w)| w);
                let look = look.or(passer).unwrap_or_else(|| {
                    let a = ((roll >> 8) % 6283) as f32 / 1000.0;
                    here + Vec3::new(a.cos(), 0.0, a.sin()) * 3.0
                });
                agent.look_at(look);
                let left = left - dt;
                self.people[i].life.act = (left > 0.0).then_some(Act::Pause { left, look: Some(look) });
            }
            Some(Act::Walk { step, left, best, since, retried, doors }) => {
                let Some((at, face, exact)) = self.place_of(i, step.target) else {
                    self.give_up(i, &step);
                    return;
                };
                let goal = origin.to_local(at);
                let d = DVec2::new(at.x - world_here.x, at.z - world_here.z).length();
                // A seat or a bed is walked right up to: nobody sits down from a step away.
                let precise = exact || seated(step.clip);
                let stopped = matches!(agent.status(), AgentStatus::Arrived | AgentStatus::Idle);
                let close = d < if precise { 0.12 } else { 0.45 };
                if !close && stopped && d < 1.3 && precise {
                    // The path ended short of it: the last step in, at a walk.
                    let to = Vec3::new(goal.x - here.x, 0.0, goal.z - here.z);
                    let stride = (agent.walk_speed * dt).min(to.length());
                    let next = here + to.normalize_or_zero() * stride;
                    agent.teleport(physics, next, facing_to(here, goal));
                    self.people[i].life.act = Some(Act::Walk { step, left: left - dt, best, since, retried, doors });
                    return;
                }
                let arrived = close || (stopped && d < 1.3 && !precise);
                if arrived {
                    let look = origin.to_local(face);
                    if exact || d > 0.1 {
                        agent.teleport(physics, if exact { goal } else { here }, facing_to(goal, look));
                    }
                    agent.stop();
                    agent.look_at(look);
                    agent.rest_clip = step.clip.map(Into::into);
                    self.people[i].life.want_tool = step.tool;
                    let secs = step.secs;
                    self.people[i].life.act = Some(Act::Use { step, left: secs });
                    return;
                }
                // Into or out of a house, by its street door: a path search straight at a bed
                // ends at the wall behind it.
                let mut doors = doors;
                let way = self.way_to(world_here, at, &mut doors);
                let aim = origin.to_local(way);
                // Getting nearer: to the door while going by it (`best` starts over there).
                let gap = DVec2::new(way.x - world_here.x, way.z - world_here.z).length();
                let (mut best, mut since, mut retried) = (best, since + dt, retried);
                if agent.target().is_some_and(|t| t.distance(aim) > 0.3) {
                    (best, retried) = (f64::MAX, false);
                }
                if gap < best - 0.3 {
                    best = gap;
                    since = 0.0;
                }
                let failed = agent.status() == AgentStatus::Failed;
                if failed && !retried {
                    agent.go_to(aim);
                    self.people[i].life.act =
                        Some(Act::Walk { step, left: left - dt, best, since, retried: true, doors });
                    return;
                }
                if since > STUCK || left <= 0.0 || failed {
                    // Nobody is put where they could not walk: a bed out of reach is slept
                    // beside (`sleep`), until the way is free again.
                    self.give_up(i, &step);
                    return;
                }
                if agent.target().is_none_or(|t| t.distance(aim) > 0.3) {
                    agent.go_to(aim);
                }
                self.people[i].life.act = Some(Act::Walk { step, left: left - dt, best, since, retried, doors });
            }
            Some(Act::Use { step, left }) => {
                let mut left = left - dt;
                if let Some((_, face, _)) = self.place_of(i, step.target) {
                    agent.look_at(origin.to_local(face));
                }
                agent.rest_clip = step.clip.map(Into::into);
                if let Target::Group(gid) = step.target {
                    match self.groups.get(&gid) {
                        Some(group) => {
                            // Listeners either nod along or just stand there, each by turns.
                            let nods = (i + group.speaker) % 3 != 0;
                            agent.rest_clip = Some(
                                if group.speaker == i {
                                    "talk"
                                } else if nods {
                                    "listen"
                                } else {
                                    "sword_idle"
                                }
                                .into(),
                            );
                            if group.speaker != i
                                && let Some(Some(speaker)) = last.get(group.speaker)
                            {
                                agent.look_at(*speaker);
                            }
                        }
                        None => left = 0.0,
                    }
                } else if seated(step.clip) && !sleeping(step.clip) {
                    // Seated with others: now and then a word to the nearest.
                    let person = &mut self.people[i];
                    person.chat -= dt;
                    if person.chat <= 0.0 {
                        person.chat = 12.0 + (roll % 9) as f32;
                        let mate = last
                            .iter()
                            .enumerate()
                            .filter_map(|(j, w)| Some((j, (*w)?)))
                            .filter(|&(j, w)| j != i && w.distance(here) < TALK)
                            .min_by(|a, b| a.1.distance(here).total_cmp(&b.1.distance(here)));
                        if let Some((j, _)) = mate {
                            let text = society::chat_line(&self.people[i].resident, self.relation(i, j), roll);
                            self.say(i, text);
                            self.people[i].talking = SEATED_TALK;
                        }
                    }
                    // Talks with the hands while the line is said, if just sitting.
                    let person = &mut self.people[i];
                    person.talking -= dt;
                    if person.talking > 0.0 && step.clip == Some("sit_idle") {
                        agent.rest_clip = Some("sit_talk".into());
                    }
                }
                if sleeping(step.clip) && doing == Doing::Sleep {
                    // Sleeps until the day says otherwise.
                    left = left.max(1.0);
                }
                if left > 0.0 {
                    self.people[i].life.act = Some(Act::Use { step, left });
                    return;
                }
                // Done: the need eased, the thing freed, the hand emptied (or keeps what is
                // carried to the next step), and up from a seat before walking.
                if let Some((need, by)) = step.eases {
                    let v = self.people[i].life.needs.get(need);
                    *v = (*v - by).max(0.0);
                }
                if let Some(effect) = step.effect {
                    self.apply(i, effect);
                }
                if let Target::Spot(s) = step.target
                    && self.spots[s].taken == Some(i)
                {
                    self.spots[s].taken = None;
                }
                // What's held and no longer wanted goes down: back where it was, mostly, or
                // right here, where it stays until someone wants it.
                let next = self.people[i].life.plan.front().map(|s| (s.tool, s.carry));
                if let Some(k) = self.stash.iter().position(|s| s.held_by == Some(i))
                    && !next.is_some_and(|(tool, carry)| {
                        tool == Some(self.stash[k].tool) || carry == Some(self.stash[k].tool)
                    })
                    && step.effect != Some(Effect::Leave(k, true))
                    && step.effect != Some(Effect::Leave(k, false))
                {
                    let put = self.put_back(k, world_here);
                    self.people[i].life.plan.push_front(put);
                }
                let next_carry = self.people[i].life.plan.front().and_then(|s| s.carry);
                self.people[i].life.want_tool = next_carry;
                agent.rest_clip = None;
                if seated(step.clip) {
                    self.people[i].life.act = Some(Act::Pause { left: GETTING_UP, look: None });
                }
            }
        }
    }

    fn start(&mut self, i: usize, step: Step, agent: &mut Agent, origin: &grimora_core::Origin) {
        if let Target::Spot(s) = step.target {
            if self.spots[s].taken.is_some_and(|t| t != i) {
                return;
            }
            self.spots[s].taken = Some(i);
        }
        let Some((at, _, _)) = self.place_of(i, step.target) else { return };
        agent.go_to(origin.to_local(at));
        self.people[i].life.want_tool = step.carry;
        self.people[i].life.act =
            Some(Act::Walk { step, left: LONGEST_WALK, best: f64::MAX, since: 0.0, retried: false, doors: 0 });
    }

    /// What finishing a step does.
    fn apply(&mut self, i: usize, effect: Effect) {
        let h = self.people[i].house;
        match effect {
            Effect::Produce(n) => {
                let tavern = self.tavern_house();
                let pantry = &mut self.pantry[h];
                pantry.raw += n;
                if pantry.raw > PANTRY_KEEPS {
                    let spare = pantry.raw - PANTRY_KEEPS;
                    pantry.raw = PANTRY_KEEPS;
                    if let Some(t) = tavern.filter(|&t| t != h) {
                        self.pantry[t].raw = (self.pantry[t].raw + spare).min(PANTRY_KEEPS * 2.0);
                    }
                }
            }
            Effect::Water => self.pantry[h].water = (self.pantry[h].water + 4.0).min(10.0),
            Effect::Cook => {
                let pantry = &mut self.pantry[h];
                let n = pantry.raw.min(2.0);
                pantry.raw -= n;
                pantry.water = (pantry.water - 1.0).max(0.0);
                pantry.meals += n * 2.0;
            }
            Effect::Eat(house) => {
                if let Some(pantry) = self.pantry.get_mut(house) {
                    pantry.meals = (pantry.meals - 1.0).max(0.0);
                }
            }
            Effect::Take(k) => {
                if let Some(s) = self.stash.get_mut(k).filter(|s| s.held_by.is_none() && !s.gone) {
                    s.held_by = Some(i);
                    let tool = s.tool;
                    if let Some(e) = s.entity {
                        self.prop_moves.push(PropMove::Hide(e));
                    }
                    // Carried from here on, not left behind and found again in the hand.
                    if let Some(next) = self.people[i].life.plan.front_mut()
                        && next.tool == Some(tool)
                    {
                        next.carry = Some(tool);
                    }
                } else {
                    // It isn't there any more (someone took it): now they know, say so, and
                    // the work doesn't happen.
                    if let Some(s) = self.stash.get_mut(k).filter(|s| s.gone && !s.missed) {
                        s.missed = true;
                        let line = format!("Cadê {}? Estava bem aqui.", s.tool.name());
                        self.say(i, &line);
                    }
                    let life = &mut self.people[i].life;
                    life.plan.clear();
                    life.want_tool = None;
                }
            }
            // Always back where it is kept: nothing is left lying in the street.
            Effect::Leave(k, _) => self.put_down(i, k, true),
        }
    }

    /// `i` lets go of stashed tool `k`: back where it was kept, or on the floor here.
    fn put_down(&mut self, i: usize, k: usize, back: bool) {
        let here = self.people[i].at;
        let yaw = (path_seed(&[self.people[i].resident.id.0, self.people[i].moves, 91]) % 6283) as f32 / 1000.0;
        let Some(s) = self.stash.get_mut(k).filter(|s| s.held_by == Some(i)) else { return };
        s.held_by = None;
        if back {
            s.at = s.home;
            s.rotation = s.home_rotation;
        } else {
            let side = DVec3::new(yaw.cos() as f64, 0.0, yaw.sin() as f64) * 0.4;
            s.at = here + side;
            // A broom lies down; the rest stands.
            s.rotation = if s.tool == Tool::Broom {
                Quat::from_rotation_y(yaw) * Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)
            } else {
                Quat::from_rotation_y(yaw)
            };
            if s.tool == Tool::Broom {
                s.at.y += 0.04;
            }
        }
        if let Some(e) = s.entity {
            self.prop_moves.push(PropMove::Show(e, s.at, s.rotation));
        }
    }

    /// Whatever stashed tool `i` holds goes back where it is kept (they gave up, or left):
    /// never dropped in the street.
    pub(super) fn drop_held(&mut self, i: usize) {
        if let Some(k) = self.stash.iter().position(|s| s.held_by == Some(i)) {
            self.put_down(i, k, true);
        }
    }

    /// The step that takes stashed tool `k` back to where it is kept.
    fn put_back(&self, k: usize, here: DVec3) -> Step {
        let s = &self.stash[k];
        Step::at(Target::Point { at: before(s.home, here), face: s.home }, 1.2, Some("put_down"))
            .tool(s.tool)
            .carry(s.tool)
            .does(Effect::Leave(k, true))
    }

    /// Whether `p` is inside some room of the village.
    fn indoors(&self, p: DVec3) -> bool {
        self.rooms.iter().flatten().any(|r| r.contains(p))
    }

    /// Whether `step` is done out in the open (putting a tool back never is: it ends at home).
    fn outdoors(&self, i: usize, step: &Step) -> bool {
        if matches!(step.effect, Some(Effect::Leave(..))) {
            return false;
        }
        self.place_of(i, step.target).is_some_and(|(at, _, _)| !self.indoors(at))
    }

    /// Somewhere inside `i`'s own house.
    fn inside(&self, i: usize, roll: u64) -> Option<DVec3> {
        let rooms = self.rooms.get(self.people[i].house).filter(|r| !r.is_empty())?;
        let u = |k: u64| (path_seed(&[roll, k, 77]) >> 11) as f64 / (1u64 << 53) as f64;
        Some(rooms[(roll % rooms.len() as u64) as usize].point(u(1), u(2)))
    }

    /// Out of the rain: what was being done is dropped, the tool taken home, and in they go.
    fn shelter(&mut self, i: usize, here: DVec3, roll: u64) {
        let Some(inside) = self.inside(i, roll) else { return };
        for group in self.groups.values_mut() {
            group.members.retain(|&m| m != i);
        }
        self.release(i);
        let mut plan = std::collections::VecDeque::new();
        if let Some(k) = self.stash.iter().position(|s| s.held_by == Some(i)) {
            plan.push_back(self.put_back(k, here));
        }
        plan.push_back(Step::at(Target::Point { at: inside, face: inside + DVec3::Z }, 25.0, None));
        let life = &mut self.people[i].life;
        life.want_tool = plan.front().and_then(|s| s.carry);
        life.plan = plan;
        life.act = None;
    }

    /// Where to stand by a sick person's bed in the sickroom, and what to face.
    fn sick_in_ward(&self) -> Option<(DVec3, DVec3)> {
        self.spots
            .iter()
            .filter(|s| s.kind == Kind::WardBed && s.taken.is_some_and(|t| self.people[t].life.sick > 0.0))
            .map(|s| {
                let mut ahead = s.face - s.at;
                ahead.y = 0.0;
                let ahead = ahead.normalize_or(DVec3::Z);
                let side = DVec3::new(-ahead.z, 0.0, ahead.x);
                (s.at + side * 0.8 + ahead * 0.7, s.at + ahead * 0.7)
            })
            .next()
    }

    /// The walk went nowhere: let go of it, keep away from it for a while, and pick again.
    fn give_up(&mut self, i: usize, step: &Step) {
        if let Target::Spot(s) = step.target {
            self.people[i].life.avoid = Some((s, AVOID));
        }
        if let Target::Group(id) = step.target
            && let Some(group) = self.groups.get_mut(&id)
        {
            group.members.retain(|&m| m != i);
        }
        self.release(i);
        self.drop_held(i);
        let life = &mut self.people[i].life;
        life.plan.clear();
        life.want_tool = None;
        life.act = Some(Act::Pause { left: 2.0, look: None });
    }

    /// The groups' clocks: who speaks next, and the end of the talk.
    pub(super) fn update_groups(&mut self, dt: f32) {
        let mut lines = Vec::new();
        self.groups.retain(|_, g| {
            g.left -= dt;
            g.turn -= dt;
            g.left > 0.0 && g.members.len() >= 2
        });
        for (&id, group) in &mut self.groups {
            if group.turn > 0.0 {
                continue;
            }
            let k = group.members.iter().position(|&m| m == group.speaker).map_or(0, |k| (k + 1) % group.members.len());
            group.speaker = group.members[k];
            let roll = path_seed(&[id, group.left as u64, group.speaker as u64]);
            group.turn = 4.0 + (roll % 5) as f32;
            let to = group.members[(k + 1) % group.members.len()];
            lines.push((group.speaker, to, roll));
        }
        for (who, to, roll) in lines {
            let text = society::chat_line(&self.people[who].resident, self.relation(who, to), roll);
            self.say(who, text);
        }
    }
}
