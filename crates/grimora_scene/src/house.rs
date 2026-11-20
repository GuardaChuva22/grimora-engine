//! Houses made from a plan: rammed-earth walls with door and window holes, packed-earth or
//! plank floors, thatched gable or hip roofs, porches, chimneys, dirt at the foot of the walls
//! and furniture from the model folder placed by a seed. Only data: the engine spawns it.
//!
//! House space: X along the house, Z across, the front wall facing +Z, the floor at y = 0 and
//! the middle of what the roofs cover at x = z = 0. The main block holds the public rooms along
//! the front, one of them with the street door; the private ones are behind them, past a long
//! partition (a plain rectangle) or in a wing under its own roof (an L or a T).

use crate::Ground;
use crate::model::load_model;
use crate::rng::Rng;
use crate::texture::{Picture, apply_texture};
use glam::{Quat, Vec2, Vec3};
use grimora_core::{MeshData, PointLight, Vertex};
use std::f32::consts::{FRAC_PI_2, PI};
use std::path::{Path, PathBuf};

/// What a shelf's place on the wall says it lets under it: whatever is lower than
/// its height above the floor, but no other shelf.
const SHELF: f32 = 1.45;
/// What a door's free floor says it lets under it: nothing, whatever the height.
const DOORWAY: f32 = -1.0;
/// Floor kept free to walk on or to reach something: nothing goes there, but a shelf may go
/// over it and an access strip may share it.
const KEPT: f32 = 0.001;
/// Free floor in front of the side of a piece one gets at it from.
const ACCESS: f32 = 0.6;
/// Half the width of a person walking, for the circulation check.
const BODY: f32 = 0.3;
/// The circulation grid's cell.
const CELL: f32 = 0.25;
/// Wall thickness.
pub const WALL: f32 = 0.3;
/// Height of a plain house's walls, floor to the eaves.
pub const WALL_HEIGHT: f32 = 2.6;
/// How far the walls and floor go under the ground, so uneven ground never shows a gap.
const FOOTING: f32 = 0.45;
/// Top of the floor.
pub const FLOOR: f32 = 0.05;
/// How far the roof reaches past the walls.
const OVERHANG: f32 = 0.55;
/// Roof rise per meter across, at most.
const PITCH: f32 = 0.8;
/// The most a roof rises over its walls: a deep house gets a flatter one.
const MOST_RISE: f32 = 2.5;
const ROOF_THICK: f32 = 0.16;
/// Door hole, and how much room is kept free in front of it on both sides.
const DOOR_WIDTH: f32 = 1.04;
const DOOR_HEIGHT: f32 = 2.12;
const DOOR_CLEAR: Vec2 = Vec2::new(1.5, 1.3);
/// Window hole: width, sill and top above the floor.
const WINDOW_WIDTH: f32 = 1.12;
const WINDOW_SILL: f32 = 0.98;
const WINDOW_TOP: f32 = 2.06;
/// Where the window model's base goes.
const WINDOW_BASE: f32 = 0.9;
/// Gap left between a piece of furniture and the wall, and between two pieces.
const GAP: f32 = 0.04;
/// Height of the dirt on the walls' feet.
const DIRT_HEIGHT: f32 = 0.85;
/// How far a porch reaches out of the front wall, and how much its roof falls each meter.
const PORCH: (f32, f32) = (1.8, 2.4);
const PORCH_FALL: f32 = 0.22;
/// Half the thickness of a porch post.
const POST: f32 = 0.08;
/// Half the width of the floor kept free from a room's way in to each of its other doors.
const PATH: f32 = 0.45;
/// Free floor kept round a table and its seats: to the walls, the doors, the counter and the
/// next table.
const AISLE: f32 = 1.2;
/// From a table's edge to the middle of a seat at it.
const SEAT_OFF: f32 = 0.45;
/// Between a tavern's counter and the wall behind it, where the keeper stands.
const BEHIND_BAR: f32 = 1.5;
/// How far a chimney stands over the roof where it comes out, and half its width.
const CHIMNEY: (f32, f32) = (0.8, 0.36);

/// What a room is for: it decides the floor and the furniture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomRole {
    /// Living room and kitchen in one: stove, a table with seats in the middle, pots.
    Hall,
    /// A living room without the stove (the chief's): hearth, table, cupboard.
    Living,
    /// Stove, a work table with the board and knives, pots, pans and a shelf.
    Kitchen,
    Bedroom,
    /// A room let to travellers: its bed is no one's.
    Lodging,
    /// The smith's porch: open in front, with forge, anvil and hammer.
    Forge,
    /// The tavern's room: counter, hearth, tables with seats, barrels.
    Tavern,
    /// The healer's room: a table, jars, potions, shelves and a basin.
    Healer,
    /// Cupboards, crates and barrels.
    Store,
    /// The healer's sickroom: narrow beds for the sick and the hurt, basins, a bucket.
    Ward,
    /// The carpenter's shop: a workbench, logs, crates.
    Workshop,
}

/// What the people of a house live by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trade {
    Fisher,
    Smith,
    Tavern,
    Healer,
    Chief,
    /// A plain home: the people of the field.
    Home,
    /// Keeps the village's beasts in the pen.
    Herder,
    /// Digs the graves and keeps the graveyard.
    Sexton,
    /// Works wood: beams, fences, furniture, coffins.
    Carpenter,
}

impl Trade {
    pub fn name(self) -> &'static str {
        match self {
            Trade::Fisher => "pescador",
            Trade::Smith => "ferreiro",
            Trade::Tavern => "taverna",
            Trade::Healer => "curandeira",
            Trade::Chief => "chefe",
            Trade::Home => "casa",
            Trade::Herder => "pastor",
            Trade::Sexton => "coveiro",
            Trade::Carpenter => "carpinteiro",
        }
    }
}

/// A room's floor inside its walls (x, z).
#[derive(Debug, Clone)]
pub struct RoomDesc {
    pub role: RoomRole,
    pub min: Vec2,
    pub max: Vec2,
}

/// A box of walls under one roof, by its inside.
#[derive(Debug, Clone, Copy)]
struct Block {
    min: Vec2,
    max: Vec2,
    /// Its ridge runs along X, else along Z.
    ridge_x: bool,
}

/// A door hole: its middle on the wall's middle line, the wall's normal, and whether a leaf
/// closes it (the street door and the bedrooms'; the rest are open doorways).
#[derive(Debug, Clone, Copy)]
struct Opening {
    at: Vec2,
    normal: Vec2,
    leaf: bool,
}

/// A straight wall: its middle line at `line` across, running along X when `along_x`, from
/// `ends.0` to `ends.1`.
#[derive(Debug, Clone, Copy)]
struct WallLine {
    along_x: bool,
    line: f32,
    ends: (f32, f32),
}

/// The house's shape seen from above: a rectangle, or a wing behind one end (L, with that end's
/// sign) or behind the middle (T).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum Shape {
    #[default]
    Rect,
    L(f32),
    T,
}

/// Which way a back room is reached: from the front room in front of it or through the one
/// before it in the row.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Reach {
    Front,
    Prev,
}

/// A trade's rooms before they are laid out.
#[derive(Debug, Clone, Default)]
struct Program {
    shape: Shape,
    length: f32,
    front_depth: f32,
    /// The public rooms along the front, with their share of the length.
    front: Vec<(RoomRole, f32)>,
    /// The private ones: their share of the length behind (a rectangle) or their depth (a wing,
    /// in a row going back, each reached through the one before).
    back: Vec<(RoomRole, f32, Reach)>,
    back_depth: f32,
    wing_width: f32,
}

/// The plan: rooms, walls, doors, windows and roofs, all laid out by the seed.
#[derive(Debug, Clone)]
pub struct HouseDesc {
    pub seed: u64,
    pub trade: Trade,
    pub rooms: Vec<RoomDesc>,
    /// Plaster over the rammed earth inside.
    pub plastered: bool,
    /// The walls' height: a tavern's are higher.
    pub height: f32,
    blocks: Vec<Block>,
    walls: Vec<WallLine>,
    /// The street door first.
    openings: Vec<Opening>,
    /// Each window's middle on its wall's middle line, and which way is out.
    windows: Vec<(Vec2, Vec2)>,
    /// Stretches of the front wall left open (the smith's porch), along X.
    open: Vec<(f32, f32)>,
    /// The front wall's middle line.
    front: f32,
    /// How far a porch reaches out of the front, if there is one.
    porch: Option<f32>,
    /// The main block's roof slopes down on all four sides.
    hip: bool,
    /// The stretch of the main block's back eave under the wing's roof, along X.
    wing_eave: Option<(f32, f32)>,
}

/// A doorway's x in `a..b`, inside the front room that overlaps it most, clear of the corners.
fn door_from(front: &[RoomDesc], a: f32, b: f32, rng: &mut Rng) -> f32 {
    let overlap = |r: &RoomDesc| r.max.x.min(b) - r.min.x.max(a);
    let best = front.iter().max_by(|r, s| overlap(r).total_cmp(&overlap(s))).expect("a front room");
    let (lo, hi) = (best.min.x.max(a), best.max.x.min(b));
    between(rng, lo, hi)
}

/// Somewhere in `a..b` at least 0.75 m from both ends, or the middle if it is too short.
fn between(rng: &mut Rng, a: f32, b: f32) -> f32 {
    if b - a > 1.6 { rng.range(a + 0.75, b - 0.75) } else { (a + b) * 0.5 }
}

impl HouseDesc {
    /// A fisherman's house.
    pub fn fisher(seed: u64) -> Self {
        Self::for_trade(Trade::Fisher, seed)
    }

    /// Each trade's rooms and the order one walks through them: the street door opens into a
    /// public room, the bedrooms are at the back. The shape, the sizes, which end is which,
    /// the porch and the roof come from the seed, so no two houses come out the same.
    pub fn for_trade(trade: Trade, seed: u64) -> Self {
        use RoomRole::*;
        let mut rng = Rng::new(seed ^ 0x40_05e);
        let rng = &mut rng;
        let side = |rng: &mut Rng| if rng.f32() < 0.5 { -1.0 } else { 1.0 };
        let roll = rng.f32();
        let program = match trade {
            Trade::Home | Trade::Fisher | Trade::Herder | Trade::Sexton => {
                let more = match trade {
                    Trade::Fisher => 0.4,
                    Trade::Sexton => -0.4,
                    _ => 0.0,
                };
                if roll < 0.6 {
                    let length = rng.range(5.0, 6.4) + more;
                    let mut back = vec![(Bedroom, 1.0, Reach::Front)];
                    if length > 5.6 && rng.f32() < 0.55 {
                        back.push((Bedroom, rng.range(0.7, 1.0), Reach::Front));
                    }
                    let front_depth = rng.range(3.4, 4.0);
                    Program {
                        length,
                        front_depth,
                        front: vec![(Hall, 1.0)],
                        back,
                        back_depth: rng.range(2.8, 3.3),
                        ..Default::default()
                    }
                } else {
                    let mut back = vec![(Bedroom, rng.range(2.8, 3.3), Reach::Front)];
                    if rng.f32() < 0.35 {
                        back.push((Bedroom, rng.range(2.6, 3.0), Reach::Prev));
                    }
                    let shape = Shape::L(side(rng));
                    let length = rng.range(4.8, 5.8) + more;
                    let front_depth = rng.range(3.6, 4.2);
                    Program {
                        shape,
                        length,
                        front_depth,
                        front: vec![(Hall, 1.0)],
                        back,
                        wing_width: rng.range(3.0, 3.6),
                        ..Default::default()
                    }
                }
            }
            Trade::Healer => {
                // The house of healing: the healer's room in front, the sickroom behind it.
                if roll < 0.6 {
                    let length = rng.range(8.0, 9.2);
                    let back =
                        vec![(Ward, 1.4, Reach::Front), (Bedroom, 0.8, Reach::Front), (Kitchen, 0.7, Reach::Front)];
                    let front_depth = rng.range(3.8, 4.4);
                    Program {
                        length,
                        front_depth,
                        front: vec![(Healer, 1.0)],
                        back,
                        back_depth: rng.range(3.4, 3.8),
                        ..Default::default()
                    }
                } else {
                    let back =
                        vec![(Ward, rng.range(4.2, 4.8), Reach::Front), (Bedroom, rng.range(2.8, 3.2), Reach::Prev)];
                    let shape = Shape::L(side(rng));
                    let length = rng.range(6.0, 6.8);
                    let front_depth = rng.range(3.8, 4.4);
                    Program {
                        shape,
                        length,
                        front_depth,
                        front: vec![(Healer, 0.6), (Kitchen, 0.4)],
                        back,
                        wing_width: rng.range(3.6, 4.0),
                        ..Default::default()
                    }
                }
            }
            Trade::Carpenter => {
                let front = vec![(Workshop, 1.1), (Hall, 0.9)];
                let back = vec![(Bedroom, 1.0, Reach::Front)];
                let length = rng.range(6.6, 7.4);
                let front_depth = rng.range(3.6, 4.0);
                Program { length, front_depth, front, back, back_depth: rng.range(2.8, 3.1), ..Default::default() }
            }
            Trade::Chief => {
                let front = vec![(Living, 1.15), (Kitchen, 0.85)];
                if roll < 0.4 {
                    let back =
                        vec![(Bedroom, 1.0, Reach::Front), (Store, 0.7, Reach::Front), (Bedroom, 0.9, Reach::Front)];
                    let length = rng.range(9.0, 10.6);
                    let front_depth = rng.range(4.0, 4.6);
                    Program { length, front_depth, front, back, back_depth: rng.range(3.2, 3.6), ..Default::default() }
                } else {
                    let shape = if roll < 0.7 { Shape::L(side(rng)) } else { Shape::T };
                    let back =
                        vec![(Bedroom, rng.range(3.4, 3.8), Reach::Front), (Bedroom, rng.range(3.0, 3.4), Reach::Prev)];
                    let length = rng.range(8.2, 9.6);
                    let front_depth = rng.range(4.2, 4.8);
                    Program {
                        shape,
                        length,
                        front_depth,
                        front,
                        back,
                        wing_width: rng.range(3.6, 4.2),
                        ..Default::default()
                    }
                }
            }
            Trade::Smith => {
                let front = vec![(Forge, 0.9), (Hall, 1.0)];
                if roll < 0.55 {
                    let back = vec![(Store, 0.9, Reach::Front), (Bedroom, 1.0, Reach::Front)];
                    let length = rng.range(8.2, 9.4);
                    let front_depth = rng.range(3.8, 4.4);
                    Program { length, front_depth, front, back, back_depth: rng.range(2.9, 3.3), ..Default::default() }
                } else {
                    // The house's wing behind the hall's end, the forge open at the other.
                    let back = vec![(Bedroom, rng.range(3.0, 3.4), Reach::Front)];
                    let length = rng.range(8.0, 9.0);
                    let front_depth = rng.range(3.8, 4.4);
                    Program {
                        shape: Shape::L(1.0),
                        length,
                        front_depth,
                        front,
                        back,
                        wing_width: rng.range(3.2, 3.8),
                        ..Default::default()
                    }
                }
            }
            Trade::Tavern => {
                let front = vec![(Tavern, 1.0)];
                if roll < 0.6 {
                    let length = rng.range(10.5, 12.5);
                    let mut back =
                        vec![(Kitchen, 1.1, Reach::Front), (Bedroom, 1.0, Reach::Prev), (Lodging, 0.85, Reach::Front)];
                    if length > 11.4 {
                        back.push((Lodging, 0.85, Reach::Front));
                    }
                    let front_depth = rng.range(5.8, 6.6);
                    Program { length, front_depth, front, back, back_depth: rng.range(3.0, 3.4), ..Default::default() }
                } else {
                    let back =
                        vec![(Kitchen, rng.range(3.4, 3.8), Reach::Front), (Bedroom, rng.range(3.0, 3.4), Reach::Prev)];
                    let length = rng.range(9.6, 11.0);
                    let front_depth = rng.range(6.0, 6.6);
                    Program {
                        shape: Shape::T,
                        length,
                        front_depth,
                        front,
                        back,
                        wing_width: rng.range(3.8, 4.2),
                        ..Default::default()
                    }
                }
            }
        };
        // A little more room than the bare minimum, so furniture and people fit.
        let mut program = program;
        program.length *= 1.12;
        program.front_depth *= 1.1;
        program.back_depth *= 1.1;
        program.wing_width *= 1.08;
        if program.shape != Shape::Rect {
            for b in &mut program.back {
                b.1 *= 1.1;
            }
        }
        let height = match trade {
            Trade::Tavern => 3.1,
            Trade::Chief => 2.8,
            _ => WALL_HEIGHT,
        };
        let fine = matches!(trade, Trade::Chief | Trade::Tavern | Trade::Healer);
        let plastered = rng.f32() < if fine { 0.75 } else { 0.4 };
        let hip = program.shape == Shape::Rect
            && rng.f32()
                < match trade {
                    Trade::Chief => 0.5,
                    Trade::Tavern => 0.4,
                    _ => 0.2,
                };
        let porch_chance = match trade {
            Trade::Tavern => 0.7,
            Trade::Chief => 0.35,
            Trade::Smith => 0.0,
            _ => 0.25,
        };
        // A porch's roof hangs off the front slope, which a hip roof does not have.
        let porch = (!hip && rng.f32() < porch_chance).then(|| rng.range(PORCH.0, PORCH.1));
        let mut desc = Self::lay_out(trade, seed, &program, rng);
        desc.height = height;
        desc.plastered = plastered;
        desc.hip = hip;
        desc.porch = porch;
        if rng.f32() < 0.5 {
            desc.map(-1.0, Vec2::ZERO);
        }
        let (min, max) = desc.cover();
        desc.map(1.0, -(min + max) * 0.5);
        desc
    }

    /// Rooms, walls, doors and windows of `p`, the main block's middle at zero.
    fn lay_out(trade: Trade, seed: u64, p: &Program, rng: &mut Rng) -> Self {
        use RoomRole::*;
        let hx = p.length * 0.5;
        let (z0, z1, split) = match p.shape {
            Shape::Rect => {
                let half = (p.front_depth + WALL + p.back_depth) * 0.5;
                (-half, half, Some(half - p.front_depth - WALL * 0.5))
            }
            _ => (-p.front_depth * 0.5, p.front_depth * 0.5, None),
        };
        let front_from = split.map_or(z0, |s| s + WALL * 0.5);
        let front_line = z1 + WALL * 0.5;
        let mut walls = vec![
            WallLine { along_x: true, line: front_line, ends: (-hx - WALL, hx + WALL) },
            WallLine { along_x: true, line: z0 - WALL * 0.5, ends: (-hx - WALL, hx + WALL) },
            WallLine { along_x: false, line: -hx - WALL * 0.5, ends: (z0, z1) },
            WallLine { along_x: false, line: hx + WALL * 0.5, ends: (z0, z1) },
        ];
        let (mut openings, mut windows, mut open) = (Vec::new(), Vec::new(), Vec::new());
        // A row of rooms sharing the length by their shares, from z `a` to `b`.
        let row = |roles: &[(RoomRole, f32)], a: f32, b: f32| -> Vec<RoomDesc> {
            let total: f32 = roles.iter().map(|r| r.1).sum();
            let unit = (p.length - WALL * (roles.len() as f32 - 1.0)) / total;
            let mut x = -hx;
            roles
                .iter()
                .map(|&(role, share)| {
                    let room = RoomDesc { role, min: Vec2::new(x, a), max: Vec2::new(x + share * unit, b) };
                    x += share * unit + WALL;
                    room
                })
                .collect()
        };
        let lit = |role: RoomRole| {
            matches!(role, Hall | Living | Kitchen | Bedroom | Lodging | Healer | Tavern | Ward | Workshop)
        };
        let front = row(&p.front, front_from, z1);
        // The street door, in the first room that is not the open forge.
        let entry = front.iter().position(|r| r.role != Forge).unwrap_or(0);
        let e = &front[entry];
        let middle = (e.min.x + e.max.x) * 0.5;
        let reach = ((e.max.x - e.min.x) * 0.5 - 0.8).max(0.0);
        let spread = if trade == Trade::Tavern { 1.2 } else { 0.6 };
        let door_x = middle + rng.range(-spread, spread).clamp(-reach, reach);
        openings.push(Opening { at: Vec2::new(door_x, front_line), normal: Vec2::Y, leaf: true });
        for pair in front.windows(2) {
            let line = pair[0].max.x + WALL * 0.5;
            walls.push(WallLine { along_x: false, line, ends: (front_from, z1) });
            openings.push(Opening { at: Vec2::new(line, between(rng, front_from, z1)), normal: Vec2::X, leaf: false });
        }
        for (i, r) in front.iter().enumerate() {
            let (a, b) = (r.min.x, r.max.x);
            let middle = (a + b) * 0.5;
            let xs = match r.role {
                Forge => {
                    open.push((a, b));
                    vec![]
                }
                Store => vec![],
                Tavern if b - a >= 7.0 => vec![a + 1.3, b - 1.3],
                _ if i == entry && b - a >= 3.6 => vec![if door_x > middle { a + 1.0 } else { b - 1.0 }],
                _ if i == entry => vec![],
                _ => vec![middle],
            };
            for x in xs {
                if (x - door_x).abs() > (DOOR_WIDTH + WINDOW_WIDTH) * 0.5 + 0.3 {
                    windows.push((Vec2::new(x, front_line), Vec2::Y));
                }
            }
        }
        let mut rooms = front.clone();
        let mut blocks = vec![Block { min: Vec2::new(-hx, z0), max: Vec2::new(hx, z1), ridge_x: true }];
        let mut wing_eave = None;
        if let Some(split) = split {
            // Behind the long partition.
            let back_to = split - WALL * 0.5;
            walls.push(WallLine { along_x: true, line: split, ends: (-hx, hx) });
            let shares: Vec<(RoomRole, f32)> = p.back.iter().map(|b| (b.0, b.1)).collect();
            let back = row(&shares, z0, back_to);
            for (i, r) in back.iter().enumerate() {
                let leaf = matches!(r.role, Bedroom | Lodging | Ward);
                if i > 0 {
                    walls.push(WallLine { along_x: false, line: back[i - 1].max.x + WALL * 0.5, ends: (z0, back_to) });
                }
                if p.back[i].2 == Reach::Prev && i > 0 {
                    let at = Vec2::new(back[i - 1].max.x + WALL * 0.5, between(rng, z0, back_to));
                    openings.push(Opening { at, normal: Vec2::X, leaf });
                } else {
                    let x = door_from(&front, r.min.x, r.max.x, rng);
                    openings.push(Opening { at: Vec2::new(x, split), normal: Vec2::NEG_Y, leaf });
                }
                if lit(r.role) {
                    windows.push((Vec2::new((r.min.x + r.max.x) * 0.5, z0 - WALL * 0.5), Vec2::NEG_Y));
                }
            }
            rooms.extend(back);
        } else {
            // A wing behind, each room reached through the one before.
            let ww = p.wing_width;
            let (wx0, wx1) = match p.shape {
                Shape::L(s) if s < 0.0 => (-hx, -hx + ww),
                Shape::L(_) => (hx - ww, hx),
                _ => (-ww * 0.5, ww * 0.5),
            };
            let top = z0 - WALL;
            let mut z = top;
            let mut wing = Vec::new();
            for (k, &(role, depth, _)) in p.back.iter().enumerate() {
                let leaf = matches!(role, Bedroom | Lodging | Ward);
                if k == 0 {
                    let x = door_from(&front, wx0, wx1, rng);
                    openings.push(Opening { at: Vec2::new(x, z0 - WALL * 0.5), normal: Vec2::NEG_Y, leaf });
                } else {
                    walls.push(WallLine { along_x: true, line: z + WALL * 0.5, ends: (wx0, wx1) });
                    openings.push(Opening {
                        at: Vec2::new(between(rng, wx0, wx1), z + WALL * 0.5),
                        normal: Vec2::NEG_Y,
                        leaf,
                    });
                }
                wing.push(RoomDesc { role, min: Vec2::new(wx0, z - depth), max: Vec2::new(wx1, z) });
                z -= depth + WALL;
            }
            let bottom = z + WALL;
            walls.push(WallLine { along_x: true, line: bottom - WALL * 0.5, ends: (wx0 - WALL, wx1 + WALL) });
            walls.push(WallLine { along_x: false, line: wx0 - WALL * 0.5, ends: (bottom, top) });
            walls.push(WallLine { along_x: false, line: wx1 + WALL * 0.5, ends: (bottom, top) });
            // Windows on the side toward the yard (an L's inner corner), and at the very back.
            let toward = match p.shape {
                Shape::L(s) => -s,
                _ if rng.f32() < 0.5 => -1.0,
                _ => 1.0,
            };
            let side_line = if toward < 0.0 { wx0 - WALL * 0.5 } else { wx1 + WALL * 0.5 };
            for (k, r) in wing.iter().enumerate() {
                if !lit(r.role) {
                    continue;
                }
                windows.push((Vec2::new(side_line, (r.min.y + r.max.y) * 0.5), Vec2::new(toward, 0.0)));
                if k + 1 == wing.len() {
                    windows.push((Vec2::new((wx0 + wx1) * 0.5, bottom - WALL * 0.5), Vec2::NEG_Y));
                }
            }
            // The front rooms look out of the back too, where the wing leaves the wall free.
            for r in &front {
                if !lit(r.role) {
                    continue;
                }
                let free = [(r.min.x, r.max.x.min(wx0 - WALL)), (r.min.x.max(wx1 + WALL), r.max.x)];
                if let Some(&(a, b)) =
                    free.iter().filter(|(a, b)| b - a >= 1.8).max_by(|s, t| (s.1 - s.0).total_cmp(&(t.1 - t.0)))
                {
                    windows.push((Vec2::new((a + b) * 0.5, z0 - WALL * 0.5), Vec2::NEG_Y));
                }
            }
            blocks.push(Block { min: Vec2::new(wx0, bottom), max: Vec2::new(wx1, top), ridge_x: false });
            wing_eave = Some((wx0 - WALL, wx1 + WALL));
            rooms.extend(wing);
        }
        Self {
            seed,
            trade,
            rooms,
            plastered: false,
            height: WALL_HEIGHT,
            blocks,
            walls,
            openings,
            windows,
            open,
            front: front_line,
            porch: None,
            hip: false,
            wing_eave,
        }
    }

    /// Everything mirrored by `sx` (1 or -1) in x and then moved `by`.
    fn map(&mut self, sx: f32, by: Vec2) {
        let point = |v: Vec2| Vec2::new(v.x * sx, v.y) + by;
        let span = |(a, b): (f32, f32)| {
            let (a, b) = (a * sx + by.x, b * sx + by.x);
            (a.min(b), a.max(b))
        };
        for r in &mut self.rooms {
            let (a, b) = (point(r.min), point(r.max));
            (r.min, r.max) = (a.min(b), a.max(b));
        }
        for b in &mut self.blocks {
            let (p, q) = (point(b.min), point(b.max));
            (b.min, b.max) = (p.min(q), p.max(q));
        }
        for w in &mut self.walls {
            if w.along_x {
                w.line += by.y;
                w.ends = span(w.ends);
            } else {
                w.line = w.line * sx + by.x;
                w.ends = (w.ends.0 + by.y, w.ends.1 + by.y);
            }
        }
        for o in &mut self.openings {
            o.at = point(o.at);
            o.normal.x *= sx;
        }
        for (at, out) in &mut self.windows {
            *at = point(*at);
            out.x *= sx;
        }
        for r in &mut self.open {
            *r = span(*r);
        }
        if let Some(e) = &mut self.wing_eave {
            *e = span(*e);
        }
        self.front += by.y;
    }

    /// The box the roofs cover, seen from above.
    fn cover(&self) -> (Vec2, Vec2) {
        let reach = WALL + OVERHANG;
        let (mut min, mut max) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
        for b in &self.blocks {
            min = min.min(b.min - reach);
            max = max.max(b.max + reach);
        }
        if let Some(d) = self.porch {
            max.y = max.y.max(self.front + WALL * 0.5 + d);
        }
        (min, max)
    }

    /// Half sizes of the roofs seen from above: what a lot must hold.
    pub fn footprint(&self) -> Vec2 {
        let (min, max) = self.cover();
        (max - min) * 0.5
    }

    /// Where the street door is, on the outer face of the front wall.
    pub fn front_door(&self) -> Vec3 {
        let door = self.openings[0];
        Vec3::new(door.at.x, 0.0, door.at.y + WALL * 0.5)
    }

    /// The roofs' rise per meter: the main block's, as steep as it may be without rising more
    /// than `MOST_RISE`; the wing, narrower, has the same and stays under it.
    fn pitch(&self) -> f32 {
        let main = self.blocks[0];
        PITCH.min(MOST_RISE / ((main.max.y - main.min.y) * 0.5 + WALL))
    }

    /// The top of the roof over `p`, or `None` past every roof's walls.
    fn roof_top(&self, p: Vec2) -> Option<f32> {
        let pitch = self.pitch();
        let mut best: Option<f32> = None;
        for (i, b) in self.blocks.iter().enumerate() {
            let half = (b.max - b.min) * 0.5 + WALL;
            let d = (p - (b.min + b.max) * 0.5).abs();
            if d.x > half.x + 0.01 || d.y > half.y + 0.01 {
                continue;
            }
            let rise = if i == 0 && self.hip {
                (half.x - d.x).min(half.y - d.y)
            } else if b.ridge_x {
                half.y - d.y
            } else {
                half.x - d.x
            };
            let y = self.height + rise.max(0.0) * pitch + ROOF_THICK;
            best = Some(best.map_or(y, |v| v.max(y)));
        }
        best
    }
}

/// A model put in the house.
#[derive(Debug, Clone)]
pub struct HouseProp {
    /// What it is, from the file name (`Mesa`), or what it is used as (`Bancada`).
    pub what: String,
    pub path: PathBuf,
    pub position: Vec3,
    pub rotation: Quat,
    /// Bumped into (its hull); small things on tables are not.
    pub solid: bool,
    /// A chest: opened as a container.
    pub container: bool,
    pub light: Option<PointLight>,
    /// Where people sit, lie or stand to use it, in its own space.
    pub uses: Vec<UsePoint>,
    /// A fire burning in it: the effect and where, in its own space.
    pub flame: Option<Flame>,
}

/// A fire in a prop: a candle's, a stove's, a hearth's, a forge's coals.
#[derive(Debug, Clone)]
pub struct Flame {
    /// The effect's file name in `assets/effects`.
    pub effect: &'static str,
    /// Where it burns, in the prop's space.
    pub at: Vec3,
    /// Lit only from dusk to dawn (candles, the living room's hearth).
    pub night_only: bool,
}

/// A door: the frame model, and where the leaf's hinge is and which way the leaf looks.
#[derive(Debug, Clone)]
pub struct HouseDoor {
    pub path: PathBuf,
    pub position: Vec3,
    pub rotation: Quat,
    /// Where a person stands outside, in front of it.
    pub outside: Vec3,
}

#[derive(Debug, Clone)]
pub struct House {
    /// One textured mesh per surface kind (walls, floors, roof, thatch, dirt...).
    pub meshes: Vec<MeshData>,
    /// Walls, floor and roof as one triangle soup.
    pub collision: (Vec<Vec3>, Vec<[u32; 3]>),
    /// What each triangle of [`House::collision`] is underfoot: earth, planks, thatch...
    pub collision_grounds: Vec<Ground>,
    pub props: Vec<HouseProp>,
    pub doors: Vec<HouseDoor>,
    /// Half sizes of what the roofs cover, and the ridge's height: the roof's box.
    pub roof_half: Vec2,
    pub ridge: f32,
    pub trade: Trade,
    /// Where each bed is, for the people who sleep there.
    pub beds: Vec<Vec3>,
    /// How many sleep in each of `beds`: a double bed or a bunk 2, a single bed or a pallet 1.
    pub sleeps: Vec<usize>,
    /// Each room's floor inside its walls, (min, max) x, z in the house's space.
    pub rooms: Vec<(Vec2, Vec2)>,
    /// The top of each chimney with a fire under it, and whether that fire burns only at night.
    pub smokes: Vec<(Vec3, bool)>,
    /// How high the walls go: under that, inside a room is indoors.
    pub wall_height: f32,
    /// The sickroom's beds, no one's: for the sick and the hurt.
    pub ward_beds: Vec<Vec3>,
}

/// How a person uses a piece of furniture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Use {
    /// Sit with the hips at `at`.
    Sit,
    /// Lie with the middle of the body at `at`, the head toward `facing`.
    Lie,
    /// Stand with the feet at `at`.
    Stand,
}

/// Where a person goes to use a model, in the model's own space (its `Transform` places it):
/// how, where, which way they look (or, lying, where the head points) and what for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UsePoint {
    pub how: Use,
    pub at: Vec3,
    pub facing: Vec3,
    /// `cadeira`, `banqueta`, `banco`, `cama`, `beliche`, `enxergao`, `balcao_taverneiro`,
    /// `balcao_cliente`, `bancada`, `bigorna`, `forja`, `fogao`, `lareira` or `poco`.
    pub kind: &'static str,
}

/// The use points of a model called `what` (its file name, or what it is used as) whose box is
/// `min..max`; none for what is not used that way. Models have their front at +Z.
pub fn use_points(what: &str, min: Vec3, max: Vec3) -> Vec<UsePoint> {
    let point = |how, at, facing, kind| UsePoint { how, at, facing, kind };
    let middle = (min + max) * 0.5;
    let before = |gap: f32, kind| vec![point(Use::Stand, Vec3::new(middle.x, 0.0, max.z + gap), Vec3::NEG_Z, kind)];
    let seat = |h: f32| h.min(0.47);
    if what.starts_with("Cadeira") {
        vec![point(Use::Sit, Vec3::new(middle.x, seat(max.y), middle.z + 0.04), Vec3::Z, "cadeira")]
    } else if what.starts_with("Banqueta") {
        vec![point(Use::Sit, Vec3::new(middle.x, max.y, middle.z), Vec3::Z, "banqueta")]
    } else if what.starts_with("Banco") {
        let x = (max.x - min.x) * 0.25;
        [-x, x].iter().map(|&x| point(Use::Sit, Vec3::new(middle.x + x, max.y, middle.z), Vec3::Z, "banco")).collect()
    } else if what.starts_with("Cama") || what.starts_with("Beliche") || what.starts_with("Enxergao") {
        // Long along Z: the head at the wall behind (-Z); along X, at -X.
        let along_z = max.z - min.z >= max.x - min.x;
        let head = if along_z { Vec3::NEG_Z } else { Vec3::NEG_X };
        let foot = if along_z { Vec3::new(middle.x, 0.0, max.z + 0.4) } else { Vec3::new(max.x + 0.4, 0.0, middle.z) };
        let (kind, tops): (&str, Vec<f32>) = if what.starts_with("Beliche") {
            ("beliche", vec![0.45, (max.y - 0.35).max(1.2)])
        } else if what.starts_with("Enxergao") {
            ("enxergao", vec![max.y.min(0.4)])
        } else {
            ("cama", vec![(max.y * 0.55).clamp(0.35, 0.6)])
        };
        let mut points: Vec<UsePoint> =
            tops.into_iter().map(|y| point(Use::Lie, Vec3::new(middle.x, y, middle.z), head, kind)).collect();
        points.push(point(
            Use::Stand,
            foot,
            -Vec3::new(foot.x - middle.x, 0.0, foot.z - middle.z).normalize_or(Vec3::Z),
            kind,
        ));
        points
    } else if what.starts_with("Balcao") {
        let mut points = vec![point(Use::Stand, Vec3::new(middle.x, 0.0, min.z - 0.5), Vec3::Z, "balcao_taverneiro")];
        points.extend(
            [-0.9f32, 0.0, 0.9]
                .iter()
                .map(|&x| point(Use::Stand, Vec3::new(middle.x + x, 0.0, max.z + 0.45), Vec3::NEG_Z, "balcao_cliente")),
        );
        points
    } else if what.starts_with("Bancada") {
        before(0.45, "bancada")
    } else if what.starts_with("Bigorna") {
        before(0.55, "bigorna")
    } else if what.starts_with("Forja") {
        before(0.6, "forja")
    } else if what.starts_with("Fogao") {
        before(0.5, "fogao")
    } else if what.starts_with("Lareira") {
        before(0.7, "lareira")
    } else if what.starts_with("Poco") {
        let r = (max.x - min.x).max(max.z - min.z) * 0.5 + 0.45;
        [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z]
            .iter()
            .map(|&d| point(Use::Stand, Vec3::new(middle.x, 0.0, middle.z) + d * r, -d, "poco"))
            .collect()
    } else {
        Vec::new()
    }
}

/// A hole in a wall, `from..to` along it, `bottom..top` above the floor.
#[derive(Clone, Copy)]
struct Hole {
    from: f32,
    to: f32,
    bottom: f32,
    top: f32,
}

/// A rectangle on the floor plan (x, z).
#[derive(Clone, Copy, Debug)]
struct Rect {
    min: Vec2,
    max: Vec2,
}

impl Rect {
    fn around(center: Vec2, half: Vec2) -> Self {
        Self { min: center - half, max: center + half }
    }
    fn overlaps(&self, other: &Rect) -> bool {
        self.min.x < other.max.x && other.min.x < self.max.x && self.min.y < other.max.y && other.min.y < self.max.y
    }
    fn inside(&self, other: &Rect) -> bool {
        self.min.cmpge(other.min).all() && self.max.cmple(other.max).all()
    }
    fn contains(&self, p: Vec2) -> bool {
        p.cmpge(self.min).all() && p.cmple(self.max).all()
    }
}

/// A room's way in or out: a point just inside it past the door, the way into the room, and
/// the room on the other side (`None`: the street).
#[derive(Clone, Copy)]
struct Entry {
    inner: Vec2,
    into: Vec2,
    other: Option<RoomRole>,
}

/// `mesh` turned by `rotation` and moved `by`.
fn moved(mut mesh: MeshData, rotation: Quat, by: Vec3) -> MeshData {
    for v in &mut mesh.vertices {
        v.position = (rotation * Vec3::from(v.position) + by).to_array();
        v.normal = (rotation * Vec3::from(v.normal)).to_array();
    }
    mesh
}

/// Builds the house. `textures` and `models` are the folders the images and the models are in.
pub fn build_house(desc: &HouseDesc, textures: &Path, models: &Path) -> House {
    let mut rng = Rng::new(desc.seed);
    let height = desc.height;
    let pitch = desc.pitch();
    let insides = desc.blocks.iter().map(|b| Rect { min: b.min, max: b.max }).collect();
    let mut walls = Surfaces { insides, height, ..Default::default() };
    for wall in &desc.walls {
        let on = |at: Vec2| {
            let (across, along) = if wall.along_x { (at.y, at.x) } else { (at.x, at.y) };
            ((across - wall.line).abs() < 0.01 && along > wall.ends.0 && along < wall.ends.1).then_some(along)
        };
        let mut holes: Vec<Hole> = desc.openings.iter().filter_map(|o| on(o.at)).map(door_hole).collect();
        holes.extend(desc.windows.iter().filter_map(|(at, _)| on(*at)).map(window_hole));
        if wall.along_x && (wall.line - desc.front).abs() < 0.01 {
            holes.extend(desc.open.iter().map(|&(from, to)| Hole { from, to, bottom: -FOOTING, top: height }));
        }
        let (center, dir) = if wall.along_x {
            (Vec3::new(0.0, 0.0, wall.line), Vec3::X)
        } else {
            (Vec3::new(wall.line, 0.0, 0.0), Vec3::Z)
        };
        walls.wall(center, dir, wall.ends, &holes);
    }
    // The gables: on the main block's ends, on the wing's back and where it meets the main one.
    for (i, b) in desc.blocks.iter().enumerate() {
        if i == 0 && desc.hip {
            continue;
        }
        if b.ridge_x {
            let span = (b.min.y - WALL, b.max.y + WALL);
            let rise = ((b.max.y - b.min.y) * 0.5 + WALL) * pitch;
            walls.gable(b.max.x + WALL, b.max.x, Vec3::X, span, rise);
            walls.gable(b.min.x - WALL, b.min.x, Vec3::NEG_X, span, rise);
        } else {
            let span = (b.min.x - WALL, b.max.x + WALL);
            let rise = ((b.max.x - b.min.x) * 0.5 + WALL) * pitch;
            walls.gable(b.min.y - WALL, b.min.y, Vec3::NEG_Z, span, rise);
            walls.gable(b.max.y + WALL, b.max.y, Vec3::Z, span, rise);
        }
    }

    // Floors, and the sills of the doorways at floor height.
    let mut earth = MeshData::new();
    let mut planks = MeshData::new();
    for r in &desc.rooms {
        let floor = if matches!(r.role, RoomRole::Bedroom | RoomRole::Lodging | RoomRole::Living | RoomRole::Ward) {
            &mut planks
        } else {
            &mut earth
        };
        cuboid(
            floor,
            Vec3::new(r.min.x, -FOOTING, r.min.y),
            Vec3::new(r.max.x, FLOOR, r.max.y),
            &mut |_, _| true,
            &mut rng,
        );
    }
    for o in &desc.openings {
        let normal = Vec3::new(o.normal.x, 0.0, o.normal.y).abs();
        let across = Vec3::new(normal.z, 0.0, normal.x);
        let at = Vec3::new(o.at.x, 0.0, o.at.y);
        let half = across * (DOOR_WIDTH * 0.5) + normal * (WALL * 0.5);
        cuboid(&mut earth, at - half - Vec3::Y * FOOTING, at + half + Vec3::Y * FLOOR, &mut |_, _| true, &mut rng);
    }

    // Roofs, each in its block's frame: the main one's ridge along X, the wing's along Z.
    let (mut roof, mut thatch, mut posts) = (MeshData::new(), MeshData::new(), MeshData::new());
    let main = desc.blocks[0];
    let ridge = height + ((main.max.y - main.min.y) * 0.5 + WALL) * pitch + ROOF_THICK;
    for (i, b) in desc.blocks.iter().enumerate() {
        let center = (b.min + b.max) * 0.5;
        let half = (b.max - b.min) * 0.5;
        let (plan, rotation) = if b.ridge_x {
            let reach = half.x + WALL + OVERHANG;
            let mut back = vec![(-reach, reach, OVERHANG)];
            if i == 0
                && let Some((a, c)) = desc.wing_eave
            {
                // Under the wing's roof the main eave stops at the wall.
                let (a, c) = (a - center.x, c - center.x);
                back = vec![(-reach, a, OVERHANG), (a, c, 0.0), (c, reach, OVERHANG)];
                back.retain(|s| s.1 - s.0 > 1e-3);
            }
            let front_over = if i == 0 && desc.porch.is_some() { 0.0 } else { OVERHANG };
            let porch = if i == 0 { desc.porch.map(|d| (d, desc.openings[0].at.x - center.x)) } else { None };
            let plan = RoofPlan {
                half_len: half.x,
                half_w: half.y,
                ends: (OVERHANG, OVERHANG),
                verges: (true, true),
                eaves: [back, vec![(-reach, reach, front_over)]],
                hip: i == 0 && desc.hip,
                porch,
            };
            (plan, Quat::IDENTITY)
        } else {
            // Turned a quarter: its -X end is the front, against the main block, without overhang.
            let from = -(half.y + WALL);
            let to = half.y + WALL + OVERHANG;
            let eave = vec![(from, to, OVERHANG)];
            let plan = RoofPlan {
                half_len: half.y,
                half_w: half.x,
                ends: (0.0, OVERHANG),
                verges: (false, true),
                eaves: [eave.clone(), eave],
                hip: false,
                porch: None,
            };
            (plan, Quat::from_rotation_y(FRAC_PI_2))
        };
        let (r, t, p) = block_roof(&plan, height, pitch, &mut rng);
        let by = Vec3::new(center.x, 0.0, center.y);
        roof.append(&moved(r, rotation, by), Vec3::ZERO, Vec3::ONE);
        thatch.append(&moved(t, rotation, by), Vec3::ZERO, Vec3::ONE);
        posts.append(&moved(p, rotation, by), Vec3::ZERO, Vec3::ONE);
    }

    // Furniture.
    let mut plan = Plan {
        models,
        rng: Rng::new(desc.seed ^ 0xF0_51),
        taken: Vec::new(),
        props: Vec::new(),
        beds: Vec::new(),
        sleeps: Vec::new(),
        bed_props: Vec::new(),
        ward_beds: Vec::new(),
        fixed: Vec::new(),
        marks: Vec::new(),
        parent: None,
        no_shelf: Vec::new(),
    };
    for o in &desc.openings {
        let across = Vec2::new(o.normal.y, o.normal.x).abs();
        let normal = o.normal.abs();
        let half = across * DOOR_CLEAR.x * 0.5 + normal * (DOOR_CLEAR.y + WALL * 0.5);
        plan.taken.push((Rect::around(o.at, half), DOORWAY));
    }
    // An open door leaf stands across the floor by its hinge (see `Porta.glb` and the
    // engine's `DOOR_OPENS`): nobody walks through it.
    for o in desc.openings.iter().filter(|o| o.leaf) {
        let turn = Quat::from_rotation_y(o.normal.x.atan2(o.normal.y));
        let mut rect = Rect { min: Vec2::splat(f32::INFINITY), max: Vec2::splat(f32::NEG_INFINITY) };
        for c in [
            Vec3::new(-0.68, 0.0, 0.1),
            Vec3::new(-0.4, 0.0, 0.1),
            Vec3::new(-0.68, 0.0, 1.02),
            Vec3::new(-0.4, 0.0, 1.02),
        ] {
            let p = turn * c;
            let p = o.at + Vec2::new(p.x, p.z);
            rect.min = rect.min.min(p);
            rect.max = rect.max.max(p);
        }
        plan.fixed.push(rect);
    }
    for &(at, out) in &desc.windows {
        let across = Vec2::new(out.y, out.x).abs();
        let normal = out.abs();
        // Only what is taller than the sill can't go under a window.
        plan.taken
            .push((Rect::around(at, across * (WINDOW_WIDTH * 0.5 + 0.1) + normal * (WALL * 0.5 + 0.7)), WINDOW_SILL));
    }
    // Each room's ways in and out, the first the one from the street's side.
    let room_at = |p: Vec2| desc.rooms.iter().position(|r| Rect { min: r.min, max: r.max }.contains(p));
    let mut ways: Vec<Vec<Entry>> = desc.rooms.iter().map(|_| Vec::new()).collect();
    for o in &desc.openings {
        let reach = WALL * 0.5 + 0.05;
        let (a, b) = (o.at + o.normal * reach, o.at - o.normal * reach);
        let (ra, rb) = (room_at(a), room_at(b));
        if let Some(i) = ra {
            ways[i].push(Entry { inner: a, into: o.normal, other: rb.map(|j| desc.rooms[j].role) });
        }
        if let Some(i) = rb {
            ways[i].push(Entry { inner: b, into: -o.normal, other: ra.map(|j| desc.rooms[j].role) });
        }
    }
    let mut soot = MeshData::new();
    let fisher = desc.trade == Trade::Fisher;
    for (r, ways) in desc.rooms.iter().zip(&ways) {
        let rect = Rect { min: r.min, max: r.max };
        let from = plan.props.len();
        if r.role == RoomRole::Tavern {
            // The counter first, in front of the kitchen door; the ways round it after.
            if let Some(kitchen) = ways.iter().find(|w| w.other == Some(RoomRole::Kitchen)) {
                plan.counter(rect, kitchen.inner);
            }
            plan.keep_paths(ways, true, Some(RoomRole::Kitchen));
            plan.tavern(rect, &mut soot);
            plan.clear_room(rect, ways, from);
            continue;
        }
        plan.keep_paths(ways, false, None);
        match r.role {
            RoomRole::Hall => plan.hall(rect, fisher, &mut soot),
            RoomRole::Living => plan.living(rect, &mut soot),
            RoomRole::Kitchen => plan.kitchen(
                rect,
                desc.trade == Trade::Tavern,
                matches!(desc.trade, Trade::Tavern | Trade::Chief),
                &mut soot,
            ),
            RoomRole::Bedroom => plan.bedroom(rect, desc.trade),
            RoomRole::Lodging => plan.lodging(rect),
            RoomRole::Ward => plan.ward(rect),
            RoomRole::Workshop => plan.workshop(rect),
            RoomRole::Forge => plan.forge(rect, &mut soot),
            RoomRole::Healer => plan.healer(rect),
            RoomRole::Store => plan.store(rect),
            RoomRole::Tavern => {}
        }
        // Where food is made: a bucket for water and a ladle, things people come and take.
        if matches!(r.role, RoomRole::Hall | RoomRole::Kitchen) {
            if !plan.props.iter().any(|p| p.what == "Balde-de-Madeira") {
                plan.several(rect, "Balde-de-Madeira.glb", 1, &[0, 1, 2, 3]);
            }
            plan.several(rect, "cozinha/Concha-de-Madeira.glb", 1, &[0, 1, 2, 3]);
        }
        plan.clear_room(rect, ways, from);
    }
    let wing = desc.blocks.get(1).map(|b| Rect { min: b.min, max: b.max });
    plan.yard(desc.trade, Rect { min: main.min, max: main.max }, wing);
    let (beds, sleeps): (Vec<Vec3>, Vec<usize>) = plan
        .beds
        .iter()
        .zip(&plan.sleeps)
        .zip(&plan.bed_props)
        .filter(|(_, i)| !plan.gone(**i))
        .map(|((b, s), _)| (*b, *s))
        .unzip();
    let ward_beds = plan.ward_beds.iter().filter(|&&i| !plan.gone(i)).map(|&i| plan.props[i].position).collect();
    let mut props = plan.kept();

    // The anvil stands on a block of wood, its face at the height a hammer comes down to, and
    // the hammer lying on it goes up with it.
    const ANVIL_BLOCK: f32 = 0.46;
    let anvils: Vec<(Vec3, Quat)> =
        props.iter().filter(|p| p.what.starts_with("Bigorna")).map(|p| (p.position, p.rotation)).collect();
    for (at, rotation) in anvils {
        for prop in props.iter_mut() {
            let near = (prop.position.x - at.x).abs() < 0.5 && (prop.position.z - at.z).abs() < 0.5;
            if near && (prop.what.starts_with("Bigorna") || prop.what.starts_with("Martelo")) {
                prop.position.y += ANVIL_BLOCK;
            }
        }
        // Under the anvil's body, not its horn.
        let middle = at + rotation * Vec3::new(-0.1, 0.0, 0.0);
        cuboid(
            &mut posts,
            middle + Vec3::new(-0.24, 0.0, -0.24),
            middle + Vec3::new(0.24, ANVIL_BLOCK, 0.24),
            &mut |_, _| true,
            &mut rng,
        );
    }

    // A chimney on every fire's flue, up through the roof (or up the outside of the wall, for
    // a stove that smokes out of its back), and where its smoke comes out.
    let mut chimneys = MeshData::new();
    let mut smokes = Vec::new();
    let in_a_room = |p: Vec2| desc.rooms.iter().any(|r| Rect { min: r.min - 0.05, max: r.max + 0.05 }.contains(p));
    for prop in &props {
        let Some((low, high, outside)) = flue_of(&prop.what) else { continue };
        let to_rect = |low: Vec3, high: Vec3| {
            let mut rect = Rect { min: Vec2::splat(f32::INFINITY), max: Vec2::splat(f32::NEG_INFINITY) };
            for c in [
                Vec3::new(low.x, 0.0, low.z),
                Vec3::new(high.x, 0.0, low.z),
                Vec3::new(low.x, 0.0, high.z),
                Vec3::new(high.x, 0.0, high.z),
            ] {
                let p = prop.position + prop.rotation * c;
                rect.min = rect.min.min(Vec2::new(p.x, p.z));
                rect.max = rect.max.max(Vec2::new(p.x, p.z));
            }
            rect
        };
        let mut rect = to_rect(low, high);
        let back = prop.rotation * Vec3::NEG_Z;
        let back = Vec2::new(back.x, back.z);
        // The wall behind's outer face, from the prop's back.
        let wall_out = Vec2::new(prop.position.x, prop.position.z) + back * (0.55 + WALL);
        let mut bottom = low.y;
        if outside {
            // A stove's flue: into the wall, then up outside if nothing is there, else up
            // inside the wall.
            let pipe_min = prop.position + prop.rotation * Vec3::new(0.37, 0.47, -0.62);
            let pipe_max = prop.position + prop.rotation * Vec3::new(0.57, 0.71, -0.49);
            cuboid(&mut chimneys, pipe_min.min(pipe_max), pipe_min.max(pipe_max), &mut |_, _| true, &mut rng);
            // Never up through another room: any room but the stove's own that the outside
            // chimney would stand in sends it up inside the wall.
            let mine = Vec2::new(prop.position.x, prop.position.z);
            let through = desc.rooms.iter().any(|r| {
                !Rect { min: r.min, max: r.max }.contains(mine)
                    && rect.min.x < r.max.x - 0.03
                    && rect.max.x > r.min.x + 0.03
                    && rect.min.y < r.max.y - 0.03
                    && rect.max.y > r.min.y + 0.03
            });
            if through || in_a_room(wall_out + back * 0.4) {
                rect = to_rect(Vec3::new(0.27, 0.0, -0.82), Vec3::new(0.67, 0.0, -0.58));
                bottom = height - 0.4;
            } else {
                bottom = -FOOTING;
            }
        }
        let middle = (rect.min + rect.max) * 0.5;
        let roof_at = if outside { wall_out - back * 0.05 } else { middle };
        let Some(top) = desc.roof_top(roof_at).or_else(|| desc.roof_top(wall_out - back * 0.3)) else { continue };
        let over = CHIMNEY.0 + if outside { 0.35 } else { 0.0 };
        let top = top.max(height + 0.6) + over;
        cuboid(
            &mut chimneys,
            Vec3::new(rect.min.x, bottom, rect.min.y),
            Vec3::new(rect.max.x, top, rect.max.y),
            &mut |_, _| true,
            &mut rng,
        );
        if prop.flame.is_some() {
            smokes.push((Vec3::new(middle.x, top + 0.05, middle.y), prop.flame.as_ref().is_some_and(|f| f.night_only)));
        }
    }

    // Collision: walls, floors, roofs, posts and chimneys.
    // Each triangle also says what it is underfoot, for the steps.
    let mut collision = (Vec::new(), Vec::new());
    let mut collision_grounds = Vec::new();
    for (mesh, ground) in [
        (&walls.outer, Ground::Dirt),
        (&walls.inner, Ground::Dirt),
        (&earth, Ground::Dirt),
        (&planks, Ground::Wood),
        (&roof, Ground::Grass),
        (&posts, Ground::Wood),
        (&chimneys, Ground::Stone),
    ] {
        let base = collision.0.len() as u32;
        collision.0.extend(mesh.vertices.iter().map(|v| Vec3::from(v.position)));
        collision.1.extend(mesh.indices.chunks_exact(3).map(|t| [base + t[0], base + t[1], base + t[2]]));
        collision_grounds.resize(collision.1.len(), ground);
    }

    for &(at, out) in &desc.windows {
        props.push(HouseProp {
            what: "Janela-Rustica".into(),
            path: models.join("Janela-Rustica.glb"),
            position: Vec3::new(at.x, WINDOW_BASE, at.y),
            rotation: Quat::from_rotation_y(out.x.atan2(out.y)),
            solid: false,
            container: false,
            light: None,
            uses: Vec::new(),
            flame: None,
        });
    }
    let doors = desc
        .openings
        .iter()
        .filter(|o| o.leaf)
        .map(|o| {
            // The door's +Z looks out of the front, or into the room past it.
            let out = Vec3::new(o.normal.x, 0.0, o.normal.y);
            let at = Vec3::new(o.at.x, 0.0, o.at.y);
            HouseDoor {
                path: models.join("Porta.glb"),
                position: Vec3::new(at.x, FLOOR, at.z),
                rotation: Quat::from_rotation_y(out.x.atan2(out.z)),
                outside: at + out * 1.4,
            }
        })
        .collect();

    let mut meshes = Vec::new();
    let mut add = |mut mesh: MeshData, image: &str, tile: f32, shadowless: bool| {
        if mesh.is_empty() {
            return;
        }
        if let Some(picture) = Picture::load(&textures.join(image)) {
            if tile > 0.0 {
                apply_texture(&mut mesh, &picture, tile, Vec3::ONE);
            } else {
                mesh.materials = vec![grimora_core::Material { double_sided: true, ..picture.material() }];
                mesh.submeshes.clear();
            }
        }
        mesh.shadowless = shadowless;
        meshes.push(mesh);
    };
    let inside = if desc.plastered { "reboco_envelhecido.png" } else { "parede_taipa.png" };
    add(walls.outer, "parede_taipa.png", 2.2, false);
    add(walls.inner, inside, 2.0, false);
    add(earth, "piso_terra_batida.png", 1.6, false);
    add(planks, "piso_madeira_desgastada.png", 1.4, false);
    add(roof, "telhado_de_palha.png", 2.0, false);
    add(posts, "madeira_lisa.png", 1.2, false);
    add(chimneys, "tijolos_de_argila.png", 1.2, false);
    // These keep their own uv: 0 tile.
    add(thatch, "palha_de_telhas_faixa.png", 0.0, false);
    add(walls.dirt, "decal_sujeira_base_parede.png", 0.0, true);
    add(soot, "decal_fuligem.png", 0.0, true);

    let rooms = desc.rooms.iter().map(|r| (r.min, r.max)).collect();
    House {
        meshes,
        collision,
        collision_grounds,
        props,
        doors,
        roof_half: desc.footprint(),
        ridge,
        trade: desc.trade,
        beds,
        sleeps,
        rooms,
        smokes,
        wall_height: height,
        ward_beds,
    }
}

fn door_hole(x: f32) -> Hole {
    Hole { from: x - DOOR_WIDTH * 0.5, to: x + DOOR_WIDTH * 0.5, bottom: -FOOTING, top: FLOOR + DOOR_HEIGHT }
}

fn window_hole(x: f32) -> Hole {
    Hole {
        from: x - WINDOW_WIDTH * 0.5,
        to: x + WINDOW_WIDTH * 0.5,
        bottom: FLOOR + WINDOW_SILL - 0.05,
        top: FLOOR + WINDOW_TOP,
    }
}

#[derive(Default)]
struct Surfaces {
    outer: MeshData,
    inner: MeshData,
    dirt: MeshData,
    /// Each block's inside: faces looking into one are inside.
    insides: Vec<Rect>,
    height: f32,
}

impl Surfaces {
    /// A straight wall through `center` along `dir` (+X or +Z), `ends` along that axis, with
    /// `holes` measured along it.
    fn wall(&mut self, center: Vec3, dir: Vec3, ends: (f32, f32), holes: &[Hole]) {
        let axis = dir.abs();
        let normal = Vec3::new(axis.z, 0.0, axis.x) * (WALL * 0.5);
        let mut holes = holes.to_vec();
        holes.sort_by(|a, b| a.from.total_cmp(&b.from));
        let height = self.height;
        let base = center - axis * center.dot(axis);
        let piece = |me: &mut Self, from: f32, to: f32, bottom: f32, top: f32, rng: &mut Rng| {
            if to - from < 1e-3 || top - bottom < 1e-3 {
                return;
            }
            let min = base + axis * from - normal + Vec3::Y * bottom;
            let max = base + axis * to + normal + Vec3::Y * top;
            let mut faces_inside = Vec::new();
            cuboid_split(me, min, max, &mut faces_inside, rng);
            // Dirt on the faces along the wall that reach the ground.
            if bottom <= 0.0 {
                for (inside, face_normal) in faces_inside {
                    let lift = if inside { FLOOR } else { -0.1 };
                    let face = (min + max) * 0.5 + face_normal * (WALL * 0.5 + 0.012);
                    let half = axis * ((to - from) * 0.5);
                    dirt_band(&mut me.dirt, face, half, face_normal, lift, from);
                }
            }
        };
        let mut rng = Rng::new((center.x.to_bits() as u64) << 32 | center.z.to_bits() as u64);
        let mut at = ends.0;
        for hole in &holes {
            piece(self, at, hole.from, -FOOTING, height, &mut rng);
            piece(self, hole.from, hole.to, -FOOTING, hole.bottom, &mut rng);
            piece(self, hole.from, hole.to, hole.top, height, &mut rng);
            at = hole.to;
        }
        piece(self, at, ends.1, -FOOTING, height, &mut rng);
    }

    /// A gable: the triangle over a wall up to the ridge, its outer face at `out_at` looking
    /// along `outward`, its inner one at `in_at`; `span` is its foot across, `rise` its height.
    fn gable(&mut self, out_at: f32, in_at: f32, outward: Vec3, span: (f32, f32), rise: f32) {
        let middle = (span.0 + span.1) * 0.5;
        let height = self.height;
        for (at, out) in [(out_at, true), (in_at, false)] {
            let p = |s: f32, y: f32| if outward.x != 0.0 { Vec3::new(at, y, s) } else { Vec3::new(s, y, at) };
            let mesh = if out { &mut self.outer } else { &mut self.inner };
            let normal = if out { outward } else { -outward };
            facing(mesh, &[p(span.0, height), p(span.1, height), p(middle, height + rise)], normal, Vec3::ONE);
        }
    }
}

/// A box whose faces go to `outer` or `inner` by where they look; the faces along the wall's
/// length (not its ends, top or bottom) come back in `long`, as (inside, normal).
fn cuboid_split(me: &mut Surfaces, min: Vec3, max: Vec3, long: &mut Vec<(bool, Vec3)>, rng: &mut Rng) {
    let size = max - min;
    let thin = if size.x < size.z { Vec3::X } else { Vec3::Z };
    let mut faces = Vec::new();
    cuboid_faces(min, max, &mut faces);
    for (corners, normal) in faces {
        let middle = corners.iter().copied().sum::<Vec3>() * 0.25 + normal * 0.05;
        let inside = me.insides.iter().any(|r| r.contains(Vec2::new(middle.x, middle.z)));
        let tint = Vec3::splat(rng.range(0.86, 1.0));
        facing(if inside { &mut me.inner } else { &mut me.outer }, &corners, normal, tint);
        if normal.abs() == thin {
            long.push((inside, normal));
        }
    }
}

fn cuboid(mesh: &mut MeshData, min: Vec3, max: Vec3, keep: &mut dyn FnMut(Vec3, Vec3) -> bool, rng: &mut Rng) {
    let mut faces = Vec::new();
    cuboid_faces(min, max, &mut faces);
    for (corners, normal) in faces {
        if keep(corners[0], normal) {
            facing(mesh, &corners, normal, Vec3::splat(rng.range(0.88, 1.0)));
        }
    }
}

fn cuboid_faces(min: Vec3, max: Vec3, out: &mut Vec<([Vec3; 4], Vec3)>) {
    let p = |x: bool, y: bool, z: bool| {
        Vec3::new(if x { max.x } else { min.x }, if y { max.y } else { min.y }, if z { max.z } else { min.z })
    };
    for axis in 0..3 {
        for high in [false, true] {
            let corner = |u: bool, v: bool| match axis {
                0 => p(high, u, v),
                1 => p(u, high, v),
                _ => p(u, v, high),
            };
            let mut normal = Vec3::ZERO;
            normal[axis] = if high { 1.0 } else { -1.0 };
            out.push(([corner(false, false), corner(true, false), corner(true, true), corner(false, true)], normal));
        }
    }
}

/// A flat polygon (3 or 4 corners, in order around) turned to look along `normal`.
fn facing(mesh: &mut MeshData, corners: &[Vec3], normal: Vec3, color: Vec3) {
    let (a, b, c) = (corners[0], corners[1], corners[2]);
    let flip = (b - a).cross(c - a).dot(normal) < 0.0;
    let mut tri = |a: Vec3, b: Vec3, c: Vec3| {
        if flip { mesh.push_triangle(a, c, b, color) } else { mesh.push_triangle(a, b, c, color) }
    };
    tri(a, b, c);
    if let Some(&d) = corners.get(3) {
        tri(a, c, d);
    }
}

/// A quad with its own uv: `corners` with `uvs`, looking along `normal`, both sides drawn.
fn sheet(mesh: &mut MeshData, corners: [Vec3; 4], uvs: [Vec2; 4], normal: Vec3) {
    let base = mesh.vertices.len() as u32;
    for (corner, uv) in corners.iter().zip(uvs) {
        let mut vertex = Vertex::new(*corner, normal, Vec3::ONE);
        vertex.uv = uv.to_array();
        mesh.vertices.push(vertex);
    }
    let (a, b, c) = (corners[0], corners[1], corners[2]);
    if (b - a).cross(c - a).dot(normal) >= 0.0 {
        mesh.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    } else {
        mesh.indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
    }
}

/// The dirt band on the foot of a wall face: `face` is its middle, `half` half its length.
fn dirt_band(mesh: &mut MeshData, face: Vec3, half: Vec3, normal: Vec3, lift: f32, from: f32) {
    let low = Vec3::new(face.x, lift, face.z);
    let high = low + Vec3::Y * DIRT_HEIGHT;
    let (u0, u1) = (from / 1.8, (from + half.length() * 2.0) / 1.8);
    sheet(
        mesh,
        [low - half, low + half, high + half, high - half],
        [Vec2::new(u0, 1.0), Vec2::new(u1, 1.0), Vec2::new(u1, 0.0), Vec2::new(u0, 0.0)],
        normal,
    );
}

/// How one block's roof is cut, in its own frame: the ridge along X over z = 0.
struct RoofPlan {
    /// Half the inside along the ridge, and across it.
    half_len: f32,
    half_w: f32,
    /// How far it reaches past the end walls at -X and +X, and whether those verges get thatch.
    ends: (f32, f32),
    verges: (bool, bool),
    /// Each eave, -Z then +Z, in stretches along X with their overhang (0: it stops at the wall).
    eaves: [Vec<(f32, f32, f32)>; 2],
    hip: bool,
    /// A porch out of the +Z wall: how far it reaches, and the street door's x (no post there).
    porch: Option<(f32, f32)>,
}

/// One block's roof: thatched slopes with thickness, the thatch cards on eaves, verges and
/// ridge, and a porch's posts.
fn block_roof(plan: &RoofPlan, height: f32, pitch: f32, rng: &mut Rng) -> (MeshData, MeshData, MeshData) {
    let (mut roof, mut strips, mut posts) = (MeshData::new(), MeshData::new(), MeshData::new());
    let half = plan.half_w + WALL;
    if plan.hip {
        hip_roof(&mut roof, &mut strips, plan.half_len + WALL, half, height, pitch, rng);
        return (roof, strips, posts);
    }
    let lift = Vec3::Y * ROOF_THICK;
    let top = height + half * pitch;
    let (x0, x1) = (-(plan.half_len + WALL + plan.ends.0), plan.half_len + WALL + plan.ends.1);
    for (k, side) in [-1.0f32, 1.0].into_iter().enumerate() {
        let up = Vec3::new(0.0, 1.0, pitch * side).normalize();
        let down = Vec3::new(0.0, -pitch, side).normalize();
        let out = Vec3::Z * side;
        let tint = Vec3::splat(rng.range(0.9, 1.0));
        for &(a, b, over) in &plan.eaves[k] {
            let low = |x: f32| Vec3::new(x, height - over * pitch, (half + over) * side);
            let high = |x: f32| Vec3::new(x, top, 0.0);
            let (la, lb, ha, hb) = (low(a), low(b), high(a), high(b));
            facing(&mut roof, &[la + lift, lb + lift, hb + lift, ha + lift], up, tint);
            facing(&mut roof, &[la, lb, hb, ha], -up, tint * 0.8);
            facing(&mut roof, &[la, lb, lb + lift, la + lift], out, tint);
            // The verges, where a stretch ends at the roof's end.
            for (end, (l, h), x_end, thatched) in
                [(-1.0f32, (la, ha), x0, plan.verges.0), (1.0, (lb, hb), x1, plan.verges.1)]
            {
                let x = if end < 0.0 { a } else { b };
                if (x - x_end).abs() > 1e-3 {
                    continue;
                }
                let o = Vec3::X * end;
                facing(&mut roof, &[l, h, h + lift, l + lift], o, tint);
                if thatched {
                    fringe(&mut strips, l + lift, h + lift, -o * 0.3, o * 0.16 - Vec3::Y * 0.36, up, rng);
                }
            }
            // Thatch hanging off the eave, where it reaches past the wall.
            if over > 0.0 {
                fringe(&mut strips, la + lift, lb + lift, -down * 0.32, out * 0.2 - Vec3::Y * 0.42, up, rng);
            }
        }
        ridge_cap(&mut strips, (x0, x1), top + ROOF_THICK, up, down, rng);
    }
    if let Some((reach, door)) = plan.porch {
        porch(&mut roof, &mut strips, &mut posts, (x0, x1), (plan.half_len + WALL, half), reach, door, height, rng);
    }
    (roof, strips, posts)
}

/// The ridge cap: cards laid down one slope (`up`, `down`) from just past the top, `x.0..x.1`.
fn ridge_cap(strips: &mut MeshData, (x0, x1): (f32, f32), top: f32, up: Vec3, down: Vec3, rng: &mut Rng) {
    let cap = up * 0.035;
    let steps = ((x1 - x0) / CARD_STEP).ceil() as usize;
    for k in 0..steps {
        let t0 = (x0 + k as f32 * CARD_STEP - CARD_WIDTH * 0.25).max(x0);
        let t1 = (t0 + CARD_WIDTH).min(x1);
        let (a, b) = (Vec3::new(t0, top, 0.0) + cap, Vec3::new(t1, top, 0.0) + cap);
        let reach = down * rng.range(0.45, 0.6);
        let over = -down * 0.06 + Vec3::Y * 0.03;
        let lift = up * (0.012 * (k % 2) as f32);
        sheet(
            strips,
            [a + over + lift, b + over + lift, b + reach + lift, a + reach + lift],
            [Vec2::new(0.0, 0.15), Vec2::new(1.0, 0.15), Vec2::new(1.0, 0.9), Vec2::new(0.0, 0.9)],
            up,
        );
    }
}

/// A roof sloping down on all four sides over walls whose outer faces are `hx`, `hz` from the
/// middle: two trapezoids and two triangles, thatch on every eave and on the short ridge.
fn hip_roof(roof: &mut MeshData, strips: &mut MeshData, hx: f32, hz: f32, height: f32, pitch: f32, rng: &mut Rng) {
    let lift = Vec3::Y * ROOF_THICK;
    let (ex, ez) = (hx + OVERHANG, hz + OVERHANG);
    let low = height - OVERHANG * pitch;
    let top = height + hz * pitch;
    let rx = (hx - hz).max(0.0);
    let tint = Vec3::splat(rng.range(0.9, 1.0));
    let corner = |sx: f32, sz: f32| Vec3::new(sx * ex, low, sz * ez);
    let ridge = |sx: f32| Vec3::new(sx * rx, top, 0.0);
    for side in [-1.0f32, 1.0] {
        let up = Vec3::new(0.0, 1.0, pitch * side).normalize();
        let down = Vec3::new(0.0, -pitch, side).normalize();
        let (a, b, c, d) = (corner(-1.0, side), corner(1.0, side), ridge(1.0), ridge(-1.0));
        facing(roof, &[a + lift, b + lift, c + lift, d + lift], up, tint);
        facing(roof, &[a, b, c, d], -up, tint * 0.8);
        facing(roof, &[a, b, b + lift, a + lift], Vec3::Z * side, tint);
        fringe(strips, a + lift, b + lift, -down * 0.32, Vec3::Z * side * 0.2 - Vec3::Y * 0.42, up, rng);
        if rx > 0.3 {
            ridge_cap(strips, (-rx, rx), top + ROOF_THICK, up, down, rng);
        }
    }
    for end in [-1.0f32, 1.0] {
        let up = Vec3::new(pitch * end, 1.0, 0.0).normalize();
        let down = Vec3::new(end, -pitch, 0.0).normalize();
        let (a, b, c) = (corner(end, -1.0), corner(end, 1.0), ridge(end));
        facing(roof, &[a + lift, b + lift, c + lift], up, tint);
        facing(roof, &[a, b, c], -up, tint * 0.8);
        facing(roof, &[a, b, b + lift, a + lift], Vec3::X * end, tint);
        fringe(strips, a + lift, b + lift, -down * 0.32, Vec3::X * end * 0.2 - Vec3::Y * 0.42, up, rng);
    }
}

/// A lean-to over the front: from the wall's top (`walls`: outer faces' half sizes) out
/// `reach`, falling `PORCH_FALL` a meter, on posts, none in front of the door at `door`.
#[allow(clippy::too_many_arguments)]
fn porch(
    roof: &mut MeshData,
    strips: &mut MeshData,
    posts: &mut MeshData,
    (x0, x1): (f32, f32),
    walls: (f32, f32),
    reach: f32,
    door: f32,
    height: f32,
    rng: &mut Rng,
) {
    let lift = Vec3::Y * ROOF_THICK;
    let z = walls.1;
    let up = Vec3::new(0.0, 1.0, PORCH_FALL).normalize();
    let down = Vec3::new(0.0, -PORCH_FALL, 1.0).normalize();
    let tint = Vec3::splat(rng.range(0.9, 1.0));
    let outer = |x: f32| Vec3::new(x, height - reach * PORCH_FALL, z + reach);
    let inner = |x: f32| Vec3::new(x, height, z);
    let (a, b, c, d) = (outer(x0), outer(x1), inner(x1), inner(x0));
    facing(roof, &[a + lift, b + lift, c + lift, d + lift], up, tint);
    facing(roof, &[a, b, c, d], -up, tint * 0.8);
    facing(roof, &[a, b, b + lift, a + lift], Vec3::Z, tint);
    for (end, (p, q)) in [(-1.0f32, (a, d)), (1.0, (b, c))] {
        let o = Vec3::X * end;
        facing(roof, &[p, q, q + lift, p + lift], o, tint);
        fringe(strips, p + lift, q + lift, -o * 0.25, o * 0.1 - Vec3::Y * 0.16, up, rng);
    }
    fringe(strips, a + lift, b + lift, -down * 0.3, Vec3::Z * 0.12 - Vec3::Y * 0.18, up, rng);
    let at = z + reach - 0.25;
    let top = height - (at - z) * PORCH_FALL;
    let span = walls.0 - 0.15;
    let n = ((2.0 * span) / 2.8).ceil().max(1.0) as usize;
    for k in 0..=n {
        let x = -span + 2.0 * span * k as f32 / n as f32;
        if (x - door).abs() < 0.9 {
            continue;
        }
        cuboid(
            posts,
            Vec3::new(x - POST, -FOOTING, at - POST),
            Vec3::new(x + POST, top, at + POST),
            &mut |_, _| true,
            rng,
        );
    }
}

/// Width of one thatch card, and how far along the edge the next one starts: they overlap.
const CARD_WIDTH: f32 = 0.7;
const CARD_STEP: f32 = 0.42;

/// Thatch cards in a row along the roof edge `from`-`to`, overlapping: see [`card`].
fn fringe(mesh: &mut MeshData, from: Vec3, to: Vec3, inward: Vec3, hang: Vec3, up: Vec3, rng: &mut Rng) {
    let length = from.distance(to);
    let dir = (to - from) / length.max(1e-3);
    let steps = (length / CARD_STEP).ceil().max(1.0) as usize;
    for k in 0..steps {
        let s0 = (k as f32 * CARD_STEP - CARD_WIDTH * 0.25).max(0.0);
        let s1 = (s0 + CARD_WIDTH).min(length);
        // Every other card a hair higher, so the overlaps never flicker.
        let lift = up * (0.015 + 0.012 * (k % 2) as f32);
        let hang = hang * rng.range(0.85, 1.15);
        card(mesh, from + dir * s0 + lift, from + dir * s1 + lift, inward, hang, up, (0.15, 0.4));
    }
}

/// One thatch card folded over a roof edge `a`-`b`: the top part lies on the roof, reaching
/// `inward`, and the rest hangs off by `hang`. `v` is where the image's top and fold are.
fn card(mesh: &mut MeshData, a: Vec3, b: Vec3, inward: Vec3, hang: Vec3, up: Vec3, (top, fold): (f32, f32)) {
    sheet(
        mesh,
        [a + inward, b + inward, b, a],
        [Vec2::new(0.0, top), Vec2::new(1.0, top), Vec2::new(1.0, fold), Vec2::new(0.0, fold)],
        up,
    );
    sheet(
        mesh,
        [a, b, b + hang, a + hang],
        [Vec2::new(0.0, fold), Vec2::new(1.0, fold), Vec2::new(1.0, 1.0), Vec2::new(0.0, 1.0)],
        up,
    );
}

/// Puts furniture in the rooms.
struct Plan<'a> {
    models: &'a Path,
    rng: Rng,
    /// Taken floor, with the height under which something may still go there.
    taken: Vec<(Rect, f32)>,
    props: Vec<HouseProp>,
    beds: Vec<Vec3>,
    sleeps: Vec<usize>,
    /// The prop of each bed in `beds`.
    bed_props: Vec<usize>,
    /// The props that are the sickroom's beds.
    ward_beds: Vec<usize>,
    /// Floor nobody walks through and nothing can take out: the open door leaves.
    fixed: Vec<Rect>,
    /// One per prop: its floor box if solid, what it sits on, and whether the circulation
    /// check took it out.
    marks: Vec<Mark>,
    /// The prop the next ones are put on.
    parent: Option<usize>,
    /// Where no shelf goes: over beds and behind tall furniture.
    no_shelf: Vec<Rect>,
}

struct Mark {
    rect: Option<Rect>,
    parent: Option<usize>,
    removed: bool,
}

/// Where a prop went: its place, its turn and its box in house space.
#[derive(Clone, Copy)]
struct Spot {
    position: Vec3,
    rotation: Quat,
    rect: Rect,
    /// Into the room, from the wall it is against.
    out: Vec2,
    top: f32,
}

impl Plan<'_> {
    fn bounds(&self, file: &str) -> Option<(Vec3, Vec3)> {
        load_model(&self.models.join(file), 0).ok()?.bounds()
    }

    /// One of `seats`, by the seed, with its box.
    fn pick_seat<'s>(&mut self, seats: &[&'s str]) -> Option<(&'s str, (Vec3, Vec3))> {
        let file = seats[(self.rng.f32() * seats.len() as f32) as usize % seats.len()];
        self.bounds(file).map(|b| (file, b))
    }

    fn push(&mut self, file: &str, spot: &Spot, solid: bool, light: Option<PointLight>) {
        let what = Path::new(file).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        self.push_as(&what, file, spot, solid, light);
    }

    /// Like [`Plan::push`], named as what it is used for.
    fn push_as(&mut self, what: &str, file: &str, spot: &Spot, solid: bool, light: Option<PointLight>) {
        let uses = self.bounds(file).map(|(min, max)| use_points(what, min, max)).unwrap_or_default();
        let lies = ["Cama", "Beliche", "Enxergao"].iter().any(|b| what.starts_with(b));
        if solid && (lies || spot.top > 1.3) {
            self.no_shelf.push(Rect { min: spot.rect.min - 0.25, max: spot.rect.max + 0.25 });
        }
        self.marks.push(Mark { rect: solid.then_some(spot.rect), parent: self.parent, removed: false });
        let what = what.to_owned();
        let flame = light.as_ref().and_then(|l| flame_of(&what, l.night_only));
        // The light shines from the fire, a little out into the room.
        let light = light.map(|mut l| {
            if let Some(f) = &flame {
                let lift = if what.starts_with("Vela") { Vec3::new(0.0, 0.05, 0.0) } else { Vec3::new(0.0, 0.3, 0.35) };
                l.position = f.at + lift;
            }
            l
        });
        self.props.push(HouseProp {
            uses,
            container: what == "Bau",
            what,
            path: self.models.join(file),
            position: spot.position,
            rotation: spot.rotation,
            solid,
            light,
            flame,
        });
    }

    /// Against a wall of `room`, facing in; the first free place from where the seed says.
    /// `walls` are the sides it may go on: 0 front (+Z), 1 back, 2 left (-X), 3 right.
    fn against_wall(&mut self, room: Rect, file: &str, walls: &[usize], solid: bool) -> Option<Spot> {
        self.against_wall_at(room, file, walls, solid, false)
    }

    /// [`Plan::against_wall`], or (`corner`) only near the ends of each wall.
    fn against_wall_at(&mut self, room: Rect, file: &str, walls: &[usize], solid: bool, corner: bool) -> Option<Spot> {
        let (min, max) = self.bounds(file)?;
        let (sides, all) = access(file);
        // A fire never goes under a window: its soot and its flue would cross it.
        let height = if is_fire(file) { 3.0 } else { max.y - min.y };
        let start = self.rng.f32();
        let first = (self.rng.f32() * walls.len() as f32) as usize;
        for w in 0..walls.len() {
            let wall = walls[(first + w) % walls.len()];
            let (out, along) = match wall {
                0 => (Vec2::NEG_Y, Vec2::X),
                1 => (Vec2::Y, Vec2::X),
                2 => (Vec2::X, Vec2::Y),
                _ => (Vec2::NEG_X, Vec2::Y),
            };
            let line = match wall {
                0 => room.max.y,
                1 => room.min.y,
                2 => room.min.x,
                _ => room.max.x,
            };
            let span = if along == Vec2::X { (room.min.x, room.max.x) } else { (room.min.y, room.max.y) };
            let length = span.1 - span.0;
            let steps = if corner { 30 } else { (length / 0.1) as usize };
            for k in 0..steps {
                let s = if corner {
                    let d = (k / 2) as f32 * 0.1;
                    if k % 2 == 0 { span.0 + d } else { span.1 - d }
                } else {
                    span.0 + ((start + k as f32 / steps as f32).fract()) * length
                };
                let wall_point = if along == Vec2::X { Vec2::new(s, line) } else { Vec2::new(line, s) };
                let yaw = out.x.atan2(out.y);
                let rotation = Quat::from_rotation_y(yaw);
                let side = rotation * Vec3::X;
                let side = Vec2::new(side.x, side.z);
                let position = wall_point + out * (GAP - min.z) - side * ((min.x + max.x) * 0.5);
                let rect = turned_rect(rotation, min, max, position);
                if !rect.inside(&room) || self.blocked(&rect, height) {
                    continue;
                }
                // The side one gets at it from faces free floor, never a wall or another piece.
                let free: Vec<Rect> = sides
                    .iter()
                    .map(|&d| strip(min, max, d))
                    .map(|(a, b)| turned_rect(rotation, a, b, position))
                    .filter(|s| s.inside(&room) && !self.taken.iter().any(|(t, u)| *u == 0.0 && t.overlaps(s)))
                    .collect();
                if !sides.is_empty() && (free.is_empty() || (all && free.len() < sides.len())) {
                    continue;
                }
                let spot = Spot { position: Vec3::new(position.x, FLOOR, position.y), rotation, rect, out, top: max.y };
                if solid {
                    self.taken.push((grow(rect), 0.0));
                }
                for s in free.into_iter().take(if all { usize::MAX } else { 1 }) {
                    self.taken.push((s, KEPT));
                }
                return Some(spot);
            }
        }
        None
    }

    fn blocked(&self, rect: &Rect, height: f32) -> bool {
        self.taken.iter().any(|(taken, under)| height > *under && taken.overlaps(rect))
    }

    /// Small things on top of `spot`, each in its own place on it.
    fn on_top(&mut self, spot: &Spot, files: &[(&str, Option<PointLight>)]) {
        let below = self.props.iter().rposition(|p| p.position == spot.position);
        self.parent = below;
        self.on_top_inner(spot, files);
        self.parent = None;
    }

    fn on_top_inner(&mut self, spot: &Spot, files: &[(&str, Option<PointLight>)]) {
        let mut used: Vec<Rect> = Vec::new();
        for (file, light) in files {
            let Some((min, max)) = self.bounds(file) else { continue };
            let area = Rect { min: spot.rect.min + 0.08, max: spot.rect.max - 0.08 };
            for _ in 0..12 {
                let at = Vec2::new(self.rng.range(area.min.x, area.max.x), self.rng.range(area.min.y, area.max.y));
                let rotation = Quat::from_rotation_y(self.rng.range(0.0, std::f32::consts::TAU));
                let rect = turned_rect(rotation, min, max, at);
                if !rect.inside(&spot.rect) || used.iter().any(|u| u.overlaps(&rect)) {
                    continue;
                }
                used.push(rect);
                let place = Spot {
                    position: Vec3::new(at.x, spot.position.y + spot.top, at.y),
                    rotation,
                    rect,
                    out: spot.out,
                    top: max.y,
                };
                self.push(file, &place, false, light.clone());
                break;
            }
        }
    }

    /// The stove against a wall, a pan on it and soot behind.
    fn stove(&mut self, room: Rect, soot: &mut MeshData) -> Option<Spot> {
        let file = "cozinha/Fogao-de-Barro.glb";
        let stove = self.against_wall(room, file, &[1, 2, 3, 0], true)?;
        self.push(file, &stove, true, Some(fire_light(2.5, 4.5)));
        self.on_top(&stove, &[("Panela-de-Ferro.glb", None)]);
        Self::soot(soot, room, &stove, 0.9);
        Some(stove)
    }

    /// A hearth against one of `walls`, its fire lit, soot behind.
    fn hearth(&mut self, room: Rect, file: &str, walls: &[usize], light: Option<PointLight>, soot: &mut MeshData) {
        if let Some(fire) = self.against_wall(room, file, walls, true) {
            self.push(file, &fire, true, light);
            Self::soot(soot, room, &fire, 1.1);
        }
    }

    /// A table near a place in `room` the seed picks, with `things` on it and 2 to `most` (up
    /// to 4) seats facing it, `SEAT_OFF` out: a bench (the chance `bench`) or two of `seats`
    /// on each long side, maybe one at an end. The set keeps `AISLE` from walls, doors, paths,
    /// the counter and every other set (`tight`: half that, if nothing else fits); nothing
    /// overlaps. `false` when the room has no room left for one.
    fn table_middle(
        &mut self,
        room: Rect,
        things: &[(&str, Option<PointLight>)],
        seats: &[&str],
        bench: f32,
        most: usize,
        tight: bool,
    ) -> bool {
        let file = "Mesa.glb";
        let Some((min, max)) = self.bounds(file) else { return false };
        let size = room.max - room.min;
        let target = (room.min + room.max) * 0.5
            + Vec2::new(self.rng.range(-0.3, 0.3) * size.x, self.rng.range(-0.3, 0.3) * size.y);
        let lengthwise = if size.x >= size.y { 0.0 } else { FRAC_PI_2 };
        let first = if self.rng.f32() < 0.25 { lengthwise + FRAC_PI_2 } else { lengthwise };
        // Who sits where in the table's frame: (file, box, place, turn, at an end).
        let mut seating: Vec<(&str, (Vec3, Vec3), Vec3, f32, bool)> = Vec::new();
        let benches = [self.rng.f32() < bench, self.rng.f32() < bench];
        for (slot, x) in [-0.38f32, 0.38].into_iter().enumerate() {
            for (k, s) in [-1.0f32, 1.0].into_iter().enumerate() {
                let turn = if s > 0.0 { PI } else { 0.0 };
                if benches[k] {
                    let f = "mobiliario/Banco-de-Madeira.glb";
                    if slot == 0
                        && let Some(b) = self.bounds(f)
                    {
                        seating.push((f, b, Vec3::new(0.0, 0.0, s * (max.z + SEAT_OFF)), turn, false));
                    }
                } else if let Some((f, b)) = self.pick_seat(seats) {
                    seating.push((f, b, Vec3::new(x, 0.0, s * (max.z + SEAT_OFF)), turn, false));
                }
            }
        }
        let e = if self.rng.f32() < 0.5 { -1.0f32 } else { 1.0 };
        if let Some((f, b)) = self.pick_seat(seats) {
            seating.push((f, b, Vec3::new(e * (max.x + SEAT_OFF), 0.0, 0.0), -e * FRAC_PI_2, true));
        }
        seating.truncate(most.clamp(2, 4));
        let jitter: Vec<f32> = seating.iter().map(|_| self.rng.range(-0.12, 0.12)).collect();
        let step = 0.2;
        let mut points = Vec::new();
        for i in 0..=(size.x / step) as i32 {
            for j in 0..=(size.y / step) as i32 {
                points.push(room.min + Vec2::new(i as f32, j as f32) * step);
            }
        }
        points.sort_by(|a, b| a.distance_squared(target).total_cmp(&b.distance_squared(target)));
        let aisles: &[f32] = if tight { &[AISLE, ACCESS] } else { &[AISLE, AISLE * 0.75, ACCESS] };
        // A seat needs `ACCESS` behind it to get out: from the walls, and from everything else.
        let inner = Rect { min: room.min + ACCESS, max: room.max - ACCESS };
        for &aisle in aisles {
            // All the seats, then without the end one, then one long side.
            let ends: Vec<usize> = (0..seating.len()).filter(|&k| !seating[k].4).collect();
            let variants: [Vec<usize>; 3] =
                [(0..seating.len()).collect(), ends.clone(), ends.iter().copied().step_by(2).collect()];
            for &at in &points {
                for yaw in [first, first + FRAC_PI_2] {
                    let turn = Quat::from_rotation_y(yaw);
                    let rect = turned_rect(turn, min, max, at);
                    if !rect.inside(&inner) {
                        continue;
                    }
                    let spots: Vec<(&str, Spot)> = seating
                        .iter()
                        .zip(&jitter)
                        .map(|(&(f, (smin, smax), local, y, _), j)| {
                            let rotation = turn * Quat::from_rotation_y(y + j);
                            let p = turn * local;
                            let position = at + Vec2::new(p.x, p.z);
                            let out = rotation * Vec3::Z;
                            let rect = turned_rect(rotation, smin, smax, position);
                            (
                                f,
                                Spot {
                                    position: Vec3::new(position.x, FLOOR, position.y),
                                    rotation,
                                    rect,
                                    out: Vec2::new(out.x, out.z),
                                    top: smax.y,
                                },
                            )
                        })
                        .collect();
                    for variant in &variants {
                        if variant.len() < 2 {
                            continue;
                        }
                        let mut set = rect;
                        for &k in variant {
                            set = Rect { min: set.min.min(spots[k].1.rect.min), max: set.max.max(spots[k].1.rect.max) };
                        }
                        let around = Rect { min: set.min - aisle, max: set.max + aisle };
                        let hit = self
                            .taken
                            .iter()
                            .any(|(t, u)| *u < 1.0 && t.overlaps(if *u == 0.0 { &around } else { &set }));
                        if !set.inside(&inner) || hit {
                            continue;
                        }
                        let out = turn * Vec3::Z;
                        let table = Spot {
                            position: Vec3::new(at.x, FLOOR, at.y),
                            rotation: turn,
                            rect,
                            out: Vec2::new(out.x, out.z),
                            top: max.y,
                        };
                        self.push(file, &table, true, None);
                        for &k in variant {
                            let (f, seat) = spots[k];
                            self.push(f, &seat, true, None);
                        }
                        self.taken.push((set, 0.0));
                        self.taken.push((Rect { min: set.min - ACCESS, max: set.max + ACCESS }, KEPT));
                        self.on_top(&table, things);
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Keeps the floor free from a room's first way in to each other one: straight in, then
    /// along the far door's wall (or, `along_near`, along the near one first).
    fn keep_paths(&mut self, ways: &[Entry], along_near: bool, skip: Option<RoomRole>) {
        let Some(first) = ways.first() else { return };
        let p = first.inner + first.into * 0.6;
        for way in &ways[1..] {
            if skip.is_some() && way.other == skip {
                continue;
            }
            let q = way.inner + way.into * 0.6;
            let corner =
                if (first.into.y.abs() > 0.5) != along_near { Vec2::new(p.x, q.y) } else { Vec2::new(q.x, p.y) };
            for (a, b) in [(p, corner), (corner, q)] {
                self.taken.push((Rect { min: a.min(b) - PATH, max: a.max(b) + PATH }, KEPT));
            }
        }
    }

    /// The tavern's counter across the back of its room with the kitchen door (`door`, just
    /// inside the room) behind one end: barrels and a shelf of mugs against the wall behind it,
    /// the other end open for the keeper.
    fn counter(&mut self, room: Rect, door: Vec2) {
        let file = "mobiliario/Balcao-de-Taverna.glb";
        let Some((min, max)) = self.bounds(file) else { return };
        let z = room.min.y + BEHIND_BAR - min.z;
        let first = if self.rng.f32() < 0.5 { -1.0f32 } else { 1.0 };
        for side in [first, -first] {
            let x = door.x + side * 0.55;
            let rect = turned_rect(Quat::IDENTITY, min, max, Vec2::new(x, z));
            let behind =
                Rect { min: Vec2::new(rect.min.x - 0.1, room.min.y), max: Vec2::new(rect.max.x + 0.1, rect.min.y) };
            // The keeper's way in, past the end by the door.
            let end = if side > 0.0 { rect.min.x } else { rect.max.x };
            let (a, b) = if side > 0.0 { (end - 0.9, end) } else { (end, end + 0.9) };
            let way = Rect { min: Vec2::new(a, room.min.y), max: Vec2::new(b, rect.max.y + 0.3) };
            let whole = Rect { min: behind.min, max: rect.max };
            if !whole.inside(&room) || !way.inside(&room) || self.blocked_but(&whole, door) {
                continue;
            }
            let spot =
                Spot { position: Vec3::new(x, FLOOR, z), rotation: Quat::IDENTITY, rect, out: Vec2::Y, top: max.y };
            self.push(file, &spot, true, None);
            self.on_top(
                &spot,
                &[
                    ("Caneca.glb", None),
                    ("Caneca.glb", None),
                    ("cozinha/Jarra-de-Barro.glb", None),
                    ("cozinha/Garrafa-Barriguda-Ambar.glb", None),
                    ("fogo-e-iluminacao/Vela-com-Castical.glb", Some(candle(3.0, 6.0))),
                ],
            );
            // Barrels at the closed end, against the wall, and a shelf over them.
            let against = |me: &Self, f: &str, x: f32, y: f32| {
                let (fmin, fmax) = me.bounds(f)?;
                let position = Vec2::new(x, room.min.y + GAP - fmin.z);
                Some(Spot {
                    position: Vec3::new(position.x, y, position.y),
                    rotation: Quat::IDENTITY,
                    rect: turned_rect(Quat::IDENTITY, fmin, fmax, position),
                    out: Vec2::Y,
                    top: fmax.y,
                })
            };
            for dx in [1.15f32, 0.5] {
                if let Some(barrel) = against(self, "Barril.glb", x + side * dx, FLOOR) {
                    self.push("Barril.glb", &barrel, true, None);
                }
            }
            if let Some(shelf) = against(self, "mobiliario/Prateleira-Avulsa.glb", x + side * 0.85, FLOOR + SHELF) {
                self.push("mobiliario/Prateleira-Avulsa.glb", &shelf, false, None);
                self.on_top(
                    &shelf,
                    &[
                        ("Caneca.glb", None),
                        ("Caneca.glb", None),
                        ("Caneca.glb", None),
                        ("cozinha/Garrafa-de-Vinho-Oliva.glb", None),
                        ("cozinha/Garrafa-de-Vidro-Vazia.glb", None),
                    ],
                );
            }
            self.taken.push((grow(rect), 0.0));
            self.taken.push((behind, KEPT));
            self.taken.push((way, KEPT));
            return;
        }
    }

    /// Like [`Plan::blocked`], but not by what is kept free around `point`.
    fn blocked_but(&self, rect: &Rect, point: Vec2) -> bool {
        self.taken.iter().any(|(taken, under)| *under < 1.2 && taken.overlaps(rect) && !taken.contains(point))
    }

    fn hall(&mut self, room: Rect, fisher: bool, soot: &mut MeshData) {
        self.stove(room, soot);
        for _ in 0..2 {
            if let Some(pot) = self.against_wall(room, "cozinha/Pote-de-Barro.glb", &[1, 2, 3], true) {
                self.push("cozinha/Pote-de-Barro.glb", &pot, true, None);
            }
        }
        self.table_middle(
            room,
            &[
                ("fogo-e-iluminacao/Vela-com-Castical.glb", Some(candle(4.0, 7.0))),
                ("Prato.glb", None),
                ("Prato.glb", None),
                ("Caneca.glb", None),
                ("Pao.glb", None),
            ],
            &["Cadeira.glb", "mobiliario/Banqueta-de-Madeira.glb"],
            0.3,
            4,
            true,
        );
        if self.rng.f32() < 0.4 {
            self.washbasin(room, false);
        }
        self.shelf(room, &[("cozinha/Jarra-de-Barro.glb", None), ("cozinha/Tigela-de-Barro.glb", None)]);
        if let Some(broom) = self.against_wall(room, "higiene-e-limpeza/Vassoura-de-Palha.glb", &[0, 1, 2, 3], true) {
            self.push("higiene-e-limpeza/Vassoura-de-Palha.glb", &broom, false, None);
        }
        if fisher {
            for file in ["mobiliario/Cesto-de-Armazenamento.glb", "Balde-de-Madeira.glb"] {
                if let Some(spot) = self.against_wall(room, file, &[0, 1, 2, 3], true) {
                    self.push(file, &spot, true, None);
                }
            }
            // The rod leans on the wall, its foot a little out.
            if let Some(rod) = self.against_wall(room, "Vara_Pesca.glb", &[0, 1, 2, 3], true) {
                let axis = Vec3::new(rod.out.y, 0.0, -rod.out.x);
                let lean = Quat::from_axis_angle(axis, -0.12);
                let out = Vec3::new(rod.out.x, 0.0, rod.out.y);
                let spot =
                    Spot { rotation: lean * rod.rotation, position: rod.position + out * 0.2 + Vec3::Y * 0.2, ..rod };
                self.push("Vara_Pesca.glb", &spot, false, None);
            }
        }
    }

    /// The chief's living room: a hearth, a long table with chairs, a cupboard and a bench.
    fn living(&mut self, room: Rect, soot: &mut MeshData) {
        self.hearth(
            room,
            "fogo-e-iluminacao/Lareira-Com-Lenha.glb",
            &[2, 3, 1],
            Some(PointLight { night_only: true, ..fire_light(3.0, 6.0) }),
            soot,
        );
        self.table_middle(
            room,
            &[
                ("fogo-e-iluminacao/Vela-com-Castical.glb", Some(candle(4.0, 7.0))),
                ("cozinha/Jarra-de-Barro.glb", None),
                ("Caneca.glb", None),
                ("Caneca.glb", None),
                ("Prato.glb", None),
            ],
            &["Cadeira.glb"],
            0.2,
            4,
            true,
        );
        self.several(room, "Armario.glb", 1, &[1, 2, 3]);
        self.several(room, "mobiliario/Banco-de-Madeira.glb", 1, &[0, 1, 2, 3]);
        self.several(room, "mobiliario/Cesto-de-Armazenamento.glb", 1, &[0, 1, 2, 3]);
        self.shelf(
            room,
            &[("cozinha/Garrafa-de-Vinho-Oliva.glb", None), ("Caneca.glb", None), ("cozinha/Jarra-de-Barro.glb", None)],
        );
    }

    /// A kitchen: the stove (and a hearth in a big one), a work table with the board, knives
    /// and bowls, pots, barrels and a shelf.
    fn kitchen(&mut self, room: Rect, big: bool, wash: bool, soot: &mut MeshData) {
        self.stove(room, soot);
        if big {
            self.hearth(room, "fogo-e-iluminacao/Lareira-Com-Lenha.glb", &[1, 2, 3], Some(fire_light(4.0, 6.5)), soot);
        }
        if let Some(work) = self.against_wall(room, "Mesa.glb", &[1, 2, 3, 0], true) {
            self.push_as("Bancada", "Mesa.glb", &work, true, None);
            let mut things = vec![
                ("cozinha/Tabua-de-Cortar.glb", None),
                ("cozinha/Faca-de-Cozinha.glb", None),
                ("cozinha/Tigela-de-Barro.glb", None),
                ("cozinha/Concha-de-Madeira.glb", None),
                ("cozinha/Colher-de-Madeira.glb", None),
            ];
            if big {
                things.extend([("cozinha/Cutelo-de-Acougueiro.glb", None), ("Queijo.glb", None), ("Pao.glb", None)]);
            }
            self.on_top(&work, &things);
        }
        self.several(room, "cozinha/Pote-de-Barro.glb", 2, &[1, 2, 3]);
        self.several(room, "Barril.glb", if big { 2 } else { 1 }, &[1, 2, 3, 0]);
        self.several(room, "Balde-de-Madeira.glb", 1, &[0, 1, 2, 3]);
        if big {
            self.several(room, "Caixote.glb", 2, &[0, 1, 2, 3]);
        }
        if wash || self.rng.f32() < 0.5 {
            self.washbasin(room, wash);
        }
        self.shelf(
            room,
            &[
                ("cozinha/Jarra-de-Barro.glb", None),
                ("cozinha/Garrafa-de-Vinho-Oliva.glb", None),
                ("cozinha/Tigela-de-Barro.glb", None),
            ],
        );
    }

    /// A room let to travellers: a narrow bed, a stool, a chamber pot, a candle on a shelf.
    fn lodging(&mut self, room: Rect) {
        let bed = if self.rng.f32() < 0.5 {
            "mobiliario/Cama-Solteiro-Sem-Manta.glb"
        } else {
            "mobiliario/Cama-Simples-com-Manta.glb"
        };
        if let Some(spot) = self.against_wall(room, bed, &[1, 2, 3, 0], true) {
            self.push(bed, &spot, true, None);
            self.chamber_pot(room, bed, &spot);
        }
        self.several(room, "mobiliario/Banqueta-de-Madeira.glb", 1, &[0, 1, 2, 3]);
        self.shelf(room, &[("fogo-e-iluminacao/Vela-com-Castical.glb", Some(candle(2.0, 4.0)))]);
    }

    fn bedroom(&mut self, room: Rect, trade: Trade) {
        let bed = match trade {
            Trade::Chief => "mobiliario/Cama-Casal-Manta-Azul.glb",
            Trade::Tavern => "mobiliario/Cama-Casal-Sem-Manta.glb",
            _ if self.rng.f32() < 0.6 => "mobiliario/Cama-Casal-Sem-Manta.glb",
            _ if self.rng.f32() < 0.5 => "mobiliario/Cama-Simples-com-Manta.glb",
            _ => "mobiliario/Cama-Solteiro-Manta-Dobrada.glb",
        };
        if let Some(spot) = self.against_wall(room, bed, &[1, 2, 3, 0], true) {
            self.push(bed, &spot, true, None);
            self.bed_props.push(self.props.len() - 1);
            self.chamber_pot(room, bed, &spot);
            self.beds.push(spot.position);
            self.sleeps.push(if bed.contains("Casal") { 2 } else { 1 });
        }
        // Room for the rest of the family, if it fits: up to two more, bunks or pallets.
        let extra = (self.rng.f32() * 3.0) as usize;
        for _ in 0..extra {
            let (more, sleeps) = match self.rng.f32() {
                r if r < 0.3 => ("mobiliario/Beliche-Manta-Verde.glb", 2),
                r if r < 0.5 => ("mobiliario/Beliche-Sem-Manta.glb", 2),
                _ => ("mobiliario/Enxergao-Sobre-Estrado.glb", 1),
            };
            if let Some(spot) = self.against_wall(room, more, &[2, 3, 1, 0], true) {
                self.push(more, &spot, true, None);
                self.bed_props.push(self.props.len() - 1);
                self.chamber_pot(room, more, &spot);
                self.beds.push(spot.position);
                self.sleeps.push(sleeps);
            }
        }
        if let Some(chest) = self.against_wall(room, "Bau.glb", &[0, 1, 2, 3], true) {
            self.push("Bau.glb", &chest, true, None);
        }
        if self.rng.f32() < 0.55 {
            self.washbasin(room, false);
        }
        let things: &[(&str, Option<PointLight>)] = match trade {
            Trade::Fisher => &[("cozinha/Pote-de-Barro.glb", None), ("Caneca.glb", None)],
            _ => &[("Caneca.glb", None)],
        };
        self.shelf(room, things);
    }

    /// A shelf on a wall, over whatever is under it but not over a door, a window or another
    /// shelf, and `things` on it.
    /// A chamber pot half under the bed `file` at `spot`, by the head, on the side one gets in;
    /// beside it on the floor when the bed lies on the floor (a pallet has no under).
    fn chamber_pot(&mut self, room: Rect, file: &str, spot: &Spot) {
        let pot = "higiene-e-limpeza/Penico-de-Ceramica.glb";
        let (Some((min, max)), Some((pmin, pmax))) = (self.bounds(file), self.bounds(pot)) else { return };
        let low = file.contains("Enxergao");
        let first_right = file.contains("Beliche") || self.rng.f32() < 0.5;
        let turn = self.rng.range(0.0, std::f32::consts::TAU);
        let rotation = spot.rotation * Quat::from_rotation_y(turn);
        for right in [first_right, !first_right] {
            let side = match (low, right) {
                (false, true) => max.x - 0.1,
                (false, false) => min.x + 0.1,
                (true, true) => max.x + 0.22,
                (true, false) => min.x - 0.22,
            };
            let position = spot.position + spot.rotation * Vec3::new(side, 0.0, min.z + 0.5);
            if low {
                let rect = turned_rect(rotation, pmin, pmax, Vec2::new(position.x, position.z));
                if !rect.inside(&room) || self.taken.iter().any(|(t, under)| *under == 0.0 && t.overlaps(&rect)) {
                    continue;
                }
            }
            let place = Spot { position, rotation, ..*spot };
            self.push(pot, &place, false, None);
            return;
        }
    }

    /// A washstand in a corner: a crate with the basin on it (`full`: the jug too, and a
    /// bucket beside it).
    fn washbasin(&mut self, room: Rect, full: bool) {
        let stand = "Caixote.glb";
        let Some(spot) = self.against_wall_at(room, stand, &[1, 2, 3, 0], true, true) else { return };
        self.push_as("Lavatorio", stand, &spot, true, None);
        let mut things = vec![("higiene-e-limpeza/Bacia-de-Lavar.glb", None)];
        if full {
            things.push(("cozinha/Jarra-de-Barro.glb", None));
        }
        self.on_top(&spot, &things);
        if full && let Some(bucket) = self.against_wall_at(room, "Balde-de-Madeira.glb", &[1, 2, 3, 0], true, true) {
            self.push("Balde-de-Madeira.glb", &bucket, true, None);
        }
    }

    /// Takes out what keeps a person (`BODY` each way) from walking from the room's first way
    /// in to every other one and to every seat and work place in it, put there from `from` on:
    /// decoration first, then storage, then seats; never beds, tables, the counter or fires.
    fn clear_room(&mut self, room: Rect, ways: &[Entry], from: usize) {
        let Some(first) = ways.first() else { return };
        let start = first.inner + first.into * 0.4;
        let mut targets: Vec<(Vec2, f32, Option<usize>)> =
            ways[1..].iter().map(|w| (w.inner + w.into * 0.4, 0.45, None)).collect();
        for i in from..self.props.len() {
            let prop = &self.props[i];
            if !room.contains(Vec2::new(prop.position.x, prop.position.z)) {
                continue;
            }
            for u in &prop.uses {
                let reach = match u.how {
                    Use::Sit => 0.8,
                    Use::Stand => 0.5,
                    // From beside a bed: its middle is half its width in.
                    Use::Lie => 1.05,
                };
                let p = prop.position + prop.rotation * u.at;
                targets.push((Vec2::new(p.x, p.z), reach, Some(i)));
            }
        }
        let mut missing = self.unreached(room, start, &targets);
        while missing > 0 {
            let mut candidates: Vec<(u8, usize)> = (from..self.props.len())
                .filter(|&i| !self.marks[i].removed && self.marks[i].rect.is_some_and(|r| r.overlaps(&room)))
                .filter_map(|i| rank(&self.props[i].what).filter(|&r| r <= 3).map(|r| (r, i)))
                .collect();
            candidates.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
            let mut better = false;
            for (_, i) in candidates {
                self.marks[i].removed = true;
                let now = self.unreached(room, start, &targets);
                if now < missing {
                    missing = now;
                    better = true;
                    break;
                }
                self.marks[i].removed = false;
            }
            if !better {
                break;
            }
        }
        // Still shut in: the extra beds (bunks, pallets) go, one at a time, while it helps.
        while missing > 0 {
            let extra: Vec<usize> = (from..self.props.len())
                .filter(|&i| {
                    !self.marks[i].removed && ["Beliche", "Enxergao"].iter().any(|b| self.props[i].what.starts_with(b))
                })
                .filter(|&i| room.contains(Vec2::new(self.props[i].position.x, self.props[i].position.z)))
                .collect();
            let mut better = false;
            for i in extra.into_iter().rev() {
                self.marks[i].removed = true;
                let now = self.unreached(room, start, &targets);
                if now < missing {
                    missing = now;
                    better = true;
                    break;
                }
                self.marks[i].removed = false;
            }
            if !better {
                break;
            }
        }
        // Seats still out of reach go.
        if missing > 0 {
            let cells = self.walkable(room, start);
            for &(p, reach, owner) in &targets {
                if let Some(i) = owner
                    && rank(&self.props[i].what) == Some(3)
                    && !cells.iter().any(|c| c.distance(p) <= reach)
                {
                    self.marks[i].removed = true;
                }
            }
        }
    }

    /// How many of `targets` (a place, how near one must get, whose it is) no one walking from
    /// `start` gets to.
    fn unreached(&self, room: Rect, start: Vec2, targets: &[(Vec2, f32, Option<usize>)]) -> usize {
        let cells = self.walkable(room, start);
        targets
            .iter()
            .filter(|(p, reach, owner)| {
                !owner.is_some_and(|i| self.marks[i].removed) && !cells.iter().any(|c| c.distance(*p) <= *reach)
            })
            .count()
    }

    /// The middles of the `CELL` grid cells of `room` someone walking from `start` gets to.
    fn walkable(&self, room: Rect, start: Vec2) -> Vec<Vec2> {
        let nx = ((room.max.x - room.min.x) / CELL).floor().max(1.0) as i32;
        let ny = ((room.max.y - room.min.y) / CELL).floor().max(1.0) as i32;
        let centre = |k: i32| room.min + Vec2::new(((k % nx) as f32 + 0.5) * CELL, ((k / nx) as f32 + 0.5) * CELL);
        let walls = Rect { min: room.min + BODY * 0.8, max: room.max - BODY * 0.8 };
        let solid: Vec<Rect> = self
            .marks
            .iter()
            .filter(|m| !m.removed)
            .filter_map(|m| m.rect)
            .filter(|r| r.overlaps(&room))
            .chain(self.fixed.iter().copied())
            .map(|r| Rect { min: r.min - BODY, max: r.max + BODY })
            .collect();
        let free: Vec<bool> =
            (0..nx * ny).map(|k| walls.contains(centre(k)) && !solid.iter().any(|r| r.contains(centre(k)))).collect();
        let Some(first) = (0..nx * ny)
            .filter(|&k| free[k as usize])
            .min_by(|&a, &b| centre(a).distance(start).total_cmp(&centre(b).distance(start)))
        else {
            return Vec::new();
        };
        if centre(first).distance(start) > 0.7 {
            return Vec::new();
        }
        let mut seen = vec![false; (nx * ny) as usize];
        let mut queue = vec![first];
        seen[first as usize] = true;
        let mut out = Vec::new();
        while let Some(k) = queue.pop() {
            out.push(centre(k));
            let (x, y) = (k % nx, k / nx);
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (a, b) = (x + dx, y + dy);
                if a < 0 || b < 0 || a >= nx || b >= ny {
                    continue;
                }
                let n = (b * nx + a) as usize;
                if free[n] && !seen[n] {
                    seen[n] = true;
                    queue.push(n as i32);
                }
            }
        }
        out
    }

    /// Whether the circulation check took prop `i` out, or what it was on.
    fn gone(&self, mut i: usize) -> bool {
        loop {
            if self.marks[i].removed {
                return true;
            }
            match self.marks[i].parent {
                Some(p) => i = p,
                None => return false,
            }
        }
    }

    /// The props the circulation check left, with what was on them.
    fn kept(&self) -> Vec<HouseProp> {
        self.props.iter().enumerate().filter(|(i, _)| !self.gone(*i)).map(|(_, p)| p.clone()).collect()
    }

    fn shelf(&mut self, room: Rect, things: &[(&str, Option<PointLight>)]) {
        let taken = std::mem::take(&mut self.taken);
        self.taken = taken
            .iter()
            .filter(|(_, under)| *under != 0.0 && *under != KEPT)
            .map(|&(rect, _)| (rect, DOORWAY))
            .collect();
        self.taken.extend(self.no_shelf.iter().map(|&r| (r, DOORWAY)));
        let shelf = self.against_wall(room, "mobiliario/Prateleira-Avulsa.glb", &[1, 2, 3], false);
        self.taken = taken;
        if let Some(mut shelf) = shelf {
            shelf.position.y = FLOOR + SHELF;
            self.push("mobiliario/Prateleira-Avulsa.glb", &shelf, false, None);
            self.on_top(&shelf, things);
            self.taken.push((grow(shelf.rect), SHELF));
        }
    }

    /// Up against a wall, `n` of them.
    fn several(&mut self, room: Rect, file: &str, n: usize, walls: &[usize]) {
        for _ in 0..n {
            if let Some(spot) = self.against_wall(room, file, walls, true) {
                self.push(file, &spot, true, None);
            }
        }
    }

    /// Soot on the wall behind `spot`, `half` wide each way.
    fn soot(soot: &mut MeshData, room: Rect, spot: &Spot, half: f32) {
        let wall = Vec3::new(spot.position.x, 0.0, spot.position.z) - Vec3::new(spot.out.x, 0.0, spot.out.y) * 0.5;
        let wall = snap_to_wall(wall, room, spot.out);
        let side = Vec3::new(spot.out.y, 0.0, -spot.out.x) * half;
        let (low, high) = (wall + Vec3::Y * 0.7, wall + Vec3::Y * 2.5);
        let normal = Vec3::new(spot.out.x, 0.0, spot.out.y);
        sheet(
            soot,
            [low - side, low + side, high + side, high - side],
            [Vec2::new(0.0, 1.0), Vec2::new(1.0, 1.0), Vec2::new(1.0, 0.0), Vec2::new(0.0, 0.0)],
            normal,
        );
    }

    fn forge(&mut self, room: Rect, soot: &mut MeshData) {
        if let Some(forge) = self.against_wall(room, "ferraria/Forja.glb", &[1, 2, 3], true) {
            let fire = PointLight {
                position: Vec3::new(0.3, 0.9, 0.0),
                color: Vec3::new(1.0, 0.42, 0.12),
                intensity: 4.0,
                radius: 6.0,
                flicker: 0.7,
                night_only: false,
            };
            self.push("ferraria/Forja.glb", &forge, true, Some(fire));
            Self::soot(soot, room, &forge, 1.2);
        }
        if let Some(anvil) = self.against_wall(room, "ferraria/Bigorna.glb", &[2, 3, 1], true) {
            self.push("ferraria/Bigorna.glb", &anvil, true, None);
            self.on_top(&anvil, &[("ferraria/Martelo-de-Ferreiro.glb", None)]);
        }
        self.several(room, "Balde-de-Madeira.glb", 1, &[1, 2, 3]);
        self.several(room, "Barril.glb", 1, &[1, 2, 3]);
    }

    /// The tavern's room, its counter already in: a hearth on an end wall, tables with chairs,
    /// stools and benches in the middle, barrels and a crate.
    fn tavern(&mut self, room: Rect, soot: &mut MeshData) {
        self.hearth(room, "fogo-e-iluminacao/Lareira-Com-Lenha.glb", &[2, 3], Some(fire_light(5.0, 8.0)), soot);
        let size = room.max - room.min;
        let tables = ((size.x * size.y) / 9.0).round().clamp(2.0, 5.0) as usize;
        for _ in 0..tables {
            let mut things: Vec<(&str, Option<PointLight>)> = vec![("Caneca.glb", None)];
            if self.rng.f32() < 0.6 {
                things.push(("fogo-e-iluminacao/Vela-com-Castical.glb", Some(candle(2.5, 5.0))));
            }
            if self.rng.f32() < 0.5 {
                things.push(("Caneca.glb", None));
            }
            if self.rng.f32() < 0.4 {
                things.push(("Prato.glb", None));
            }
            if self.rng.f32() < 0.3 {
                things.push(("cozinha/Garrafa-de-Vinho-Oliva.glb", None));
            }
            if !self.table_middle(room, &things, &["Cadeira.glb", "mobiliario/Banqueta-de-Madeira.glb"], 0.5, 4, false)
            {
                break;
            }
        }
        self.several(room, "Barril.glb", 2, &[0, 2, 3]);
        self.several(room, "mobiliario/Banqueta-de-Madeira.glb", 1, &[0, 2, 3]);
        self.several(room, "Caixote.glb", 1, &[2, 3]);
    }

    /// The healer's room: a table with a seat on each side, shelves of jars and potions, pots,
    /// the basin, a bench to wait on.
    fn healer(&mut self, room: Rect) {
        self.table_middle(
            room,
            &[
                ("fogo-e-iluminacao/Vela-com-Castical.glb", Some(candle(3.5, 6.5))),
                ("cozinha/Jarra-de-Barro.glb", None),
                ("cozinha/Tigela-de-Barro.glb", None),
                ("cozinha/Frasco-Quadrado-Azul.glb", None),
            ],
            &["Cadeira.glb"],
            0.0,
            2,
            true,
        );
        self.shelf(
            room,
            &[
                ("cozinha/Garrafa-Pocao-Estriada.glb", None),
                ("cozinha/Frasco-Quadrado-Azul.glb", None),
                ("cozinha/Cantil-de-Vidro-Fume.glb", None),
            ],
        );
        self.shelf(
            room,
            &[
                ("cozinha/Pote-de-Barro.glb", None),
                ("cozinha/Garrafa-Barriguda-Ambar.glb", None),
                ("cozinha/Tigela-de-Barro.glb", None),
            ],
        );
        self.several(room, "higiene-e-limpeza/Bacia-de-Lavar.glb", 1, &[0, 1, 2, 3]);
        self.several(room, "mobiliario/Banco-de-Madeira.glb", 1, &[0, 2, 3]);
        self.several(room, "cozinha/Pote-de-Barro.glb", 2, &[0, 1, 2, 3]);
        self.several(room, "higiene-e-limpeza/Vassoura-de-Palha.glb", 1, &[0, 1, 2, 3]);
    }

    /// The sickroom: two or three narrow beds, a stool, a full washstand, a basket of cloth,
    /// potions and a candle on a shelf.
    fn ward(&mut self, room: Rect) {
        let beds = 2 + (self.rng.f32() < 0.6) as usize;
        for k in 0..beds {
            let bed = match k % 3 {
                0 => "mobiliario/Cama-Solteiro-Sem-Manta.glb",
                1 => "mobiliario/Cama-Simples-com-Manta.glb",
                _ => "mobiliario/Enxergao-Sobre-Estrado.glb",
            };
            if let Some(spot) = self.against_wall(room, bed, &[1, 2, 3, 0], true) {
                self.push(bed, &spot, true, None);
                self.ward_beds.push(self.props.len() - 1);
                self.chamber_pot(room, bed, &spot);
            }
        }
        self.washbasin(room, true);
        self.several(room, "mobiliario/Banqueta-de-Madeira.glb", beds.min(2), &[0, 1, 2, 3]);
        self.several(room, "mobiliario/Cesto-de-Armazenamento.glb", 1, &[0, 1, 2, 3]);
        self.shelf(
            room,
            &[
                ("fogo-e-iluminacao/Vela-com-Castical.glb", Some(candle(2.5, 5.0))),
                ("cozinha/Garrafa-Pocao-Estriada.glb", None),
                ("cozinha/Tigela-de-Barro.glb", None),
            ],
        );
    }

    /// The carpenter's shop: the workbench with tools on it, a log, crates, a barrel.
    fn workshop(&mut self, room: Rect) {
        if let Some(bench) = self.against_wall(room, "Mesa.glb", &[1, 2, 3], true) {
            self.push_as("Bancada", "Mesa.glb", &bench, true, None);
            self.on_top(&bench, &[("ferraria/Martelo-de-Ferreiro.glb", None), ("cozinha/Faca-de-Cozinha.glb", None)]);
        }
        self.several(room, "Tronco-Oco.glb", 1, &[1, 2, 3]);
        self.several(room, "Caixote.glb", 2, &[0, 1, 2, 3]);
        self.several(room, "Barril.glb", 1, &[1, 2, 3]);
        self.several(room, "mobiliario/Banqueta-de-Madeira.glb", 1, &[0, 1, 2, 3]);
        self.several(room, "higiene-e-limpeza/Vassoura-de-Palha.glb", 1, &[0, 1, 2, 3]);
    }

    fn store(&mut self, room: Rect) {
        self.several(room, "Armario.glb", 1, &[1, 2, 3]);
        self.several(room, "Guarda-Roupa.glb", 1, &[1, 2, 3, 0]);
        self.several(room, "Caixote.glb", 2, &[0, 1, 2, 3]);
        self.several(room, "Barril.glb", 2, &[0, 1, 2, 3]);
    }

    /// What goes outside the main block (`main`, its inside): the fisher's net on an end wall
    /// (the one away from an L's wing), a clothesline behind where the wing leaves room,
    /// firewood stacked against the other end wall.
    fn yard(&mut self, trade: Trade, main: Rect, wing: Option<Rect>) {
        let end = match wing {
            Some(w) if (w.min.x - main.min.x).abs() < 0.01 => 1.0f32,
            Some(w) if (w.max.x - main.max.x).abs() < 0.01 => -1.0,
            _ if self.rng.f32() < 0.5 => -1.0,
            _ => 1.0,
        };
        let face = |end: f32| if end > 0.0 { main.max.x + WALL } else { main.min.x - WALL };
        let middle = (main.min.y + main.max.y) * 0.5;
        let place = |position: Vec3, rotation: Quat| Spot {
            position,
            rotation,
            rect: Rect { min: Vec2::ZERO, max: Vec2::ZERO },
            out: Vec2::ZERO,
            top: 0.0,
        };
        if trade == Trade::Fisher
            && let Some((min, _)) = self.bounds("Rede-de-Pesca.glb")
        {
            let at = Vec3::new(face(end) + end * (GAP - min.z), 0.15, middle);
            self.push("Rede-de-Pesca.glb", &place(at, Quat::from_rotation_y(end * FRAC_PI_2)), false, None);
            // The rod leans on the same wall, by the net: taken to the water and brought back.
            if self.bounds("Vara_Pesca.glb").is_some() {
                let at = Vec3::new(face(end) + end * 0.22, 0.0, (middle + 1.5).min(main.max.y - 0.4));
                self.push("Vara_Pesca.glb", &place(at, Quat::from_rotation_z(end * 0.16)), false, None);
            }
        }
        if matches!(trade, Trade::Fisher | Trade::Healer | Trade::Home | Trade::Tavern | Trade::Herder | Trade::Sexton)
        {
            let free = match wing {
                None => Some((main.min.x + 1.5, main.max.x - 1.5)),
                Some(w) => {
                    let reach = WALL + OVERHANG + 1.2;
                    let (a, b) = ((main.min.x, w.min.x - reach), (w.max.x + reach, main.max.x));
                    let best = if a.1 - a.0 > b.1 - b.0 { a } else { b };
                    (best.1 - best.0 > 0.4).then_some(best)
                }
            };
            if let Some((a, b)) = free {
                let x = if b > a { self.rng.range(a, b) } else { (a + b) * 0.5 };
                let at = Vec3::new(x, 0.0, main.min.y - (WALL + OVERHANG + 1.8));
                self.push("higiene-e-limpeza/Varal-de-Madeira.glb", &place(at, Quat::IDENTITY), false, None);
            }
        }
        if matches!(trade, Trade::Smith | Trade::Chief | Trade::Home | Trade::Tavern | Trade::Carpenter | Trade::Healer)
        {
            // Logs pointing out of the other end wall, in a pile three high.
            let face = face(-end);
            let start = self.rng.range(main.min.y + 0.2, main.max.y - 1.2);
            for layer in 0..3 {
                for k in 0..(6 - layer) {
                    let z = start + (k as f32 + layer as f32 * 0.5) * 0.15;
                    let at = Vec3::new(face - end * 0.28, layer as f32 * 0.12, z);
                    let turn = Quat::from_rotation_y(self.rng.range(-0.08, 0.08));
                    self.push("fogo-e-iluminacao/Lenha-Avulsa.glb", &place(at, turn), false, None);
                }
            }
        }
    }
}

/// A candle's flame: lit from dusk to dawn.
fn candle(intensity: f32, radius: f32) -> PointLight {
    PointLight {
        position: Vec3::new(0.0, 0.26, 0.0),
        color: Vec3::new(1.0, 0.6, 0.3),
        intensity,
        radius,
        flicker: 0.35,
        night_only: true,
    }
}

/// Whether `file` (or a prop's name) is a fire: a stove, a hearth, a forge.
fn is_fire(file: &str) -> bool {
    let name = Path::new(file).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    ["Fogao", "Lareira", "Forja"].iter().any(|f| name.starts_with(f))
}

/// The fire burning in a prop called `what`, if it has one: where it is in the model.
fn flame_of(what: &str, night_only: bool) -> Option<Flame> {
    let (effect, at) = if what.starts_with("Vela") {
        ("candle.ron", Vec3::new(0.0, 0.205, 0.0))
    } else if what.starts_with("Fogao") {
        // In the firebox, seen through its mouth on the left of the front.
        ("stove_fire.ron", Vec3::new(-0.38, 0.33, 0.18))
    } else if what.starts_with("Lareira-Com-Lenha") {
        ("hearth_fire.ron", Vec3::new(0.0, 0.11, 0.0))
    } else if what.starts_with("Forja") {
        ("forge_fire.ron", Vec3::new(-0.02, 0.84, 0.0))
    } else {
        return None;
    };
    Some(Flame { effect, at, night_only })
}

/// Where the smoke of a fire `what` leaves the house: the flue in its model's space (min,
/// max), from where it rises to the roof, and whether it runs up outside the wall.
fn flue_of(what: &str) -> Option<(Vec3, Vec3, bool)> {
    if what.starts_with("Lareira") {
        // On top of the hearth's own stone chimney.
        Some((Vec3::new(-0.47, 2.2, -0.62), Vec3::new(0.47, 2.2, 0.11), false))
    } else if what.starts_with("Forja") {
        // On the hood's duct.
        Some((Vec3::new(-0.2, 1.95, -0.62), Vec3::new(0.2, 1.95, -0.04), false))
    } else if what.starts_with("Fogao") {
        // The stove's smoke leaves by its back, low on the right: through the wall into a
        // chimney built up the outside.
        Some((Vec3::new(0.2, 0.2, -1.3), Vec3::new(0.74, 0.2, -0.62), true))
    } else {
        None
    }
}

/// A fire in a stove or a hearth.
fn fire_light(intensity: f32, radius: f32) -> PointLight {
    PointLight {
        position: Vec3::new(0.0, 0.5, 0.3),
        color: Vec3::new(1.0, 0.42, 0.13),
        intensity,
        radius,
        flicker: 0.6,
        night_only: false,
    }
}

/// The sides (in the model's space) one gets at a model from, and whether it needs all of them
/// or just one: a bunk's ladder, a bed's long sides, the front of a cupboard, a chest, a fire.
fn access(file: &str) -> (Vec<Vec2>, bool) {
    let name = Path::new(file).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    if name.starts_with("Beliche") {
        (vec![Vec2::X], true)
    } else if name.starts_with("Cama") || name.starts_with("Enxergao") {
        (vec![Vec2::X, Vec2::NEG_X], false)
    } else if ["Armario", "Guarda-Roupa", "Bau", "Fogao", "Forja", "Bigorna", "Lareira", "Mesa", "Caixote"]
        .iter()
        .any(|n| name.starts_with(n))
    {
        (vec![Vec2::Y], true)
    } else {
        (Vec::new(), false)
    }
}

/// The strip of floor `ACCESS` deep along the side `d` (x, z) of the box `min..max`.
fn strip(min: Vec3, max: Vec3, d: Vec2) -> (Vec3, Vec3) {
    if d.x > 0.5 {
        (Vec3::new(max.x, min.y, min.z), Vec3::new(max.x + ACCESS, max.y, max.z))
    } else if d.x < -0.5 {
        (Vec3::new(min.x - ACCESS, min.y, min.z), Vec3::new(min.x, max.y, max.z))
    } else {
        (Vec3::new(min.x, min.y, max.z), Vec3::new(max.x, max.y, max.z + ACCESS))
    }
}

/// How readily the circulation check takes a prop out: 0 decoration, 1 storage, 3 seats, 4
/// work places; never beds, tables or the counter.
fn rank(what: &str) -> Option<u8> {
    let is = |names: &[&str]| names.iter().any(|n| what.starts_with(n));
    if is(&["Cama", "Beliche", "Enxergao", "Mesa", "Balcao"]) {
        None
    } else if is(&["Fogao", "Forja", "Bigorna", "Lareira", "Bancada"]) {
        Some(4)
    } else if is(&["Cadeira", "Banqueta", "Banco"]) {
        Some(3)
    } else if is(&["Armario", "Guarda-Roupa", "Bau", "Barril", "Caixote", "Lavatorio", "Prateleira"]) {
        Some(1)
    } else {
        Some(0)
    }
}

fn grow(rect: Rect) -> Rect {
    Rect { min: rect.min - GAP, max: rect.max + GAP }
}

/// The floor box of a model turned by `rotation` and put at `at`.
fn turned_rect(rotation: Quat, min: Vec3, max: Vec3, at: Vec2) -> Rect {
    let mut rect = Rect { min: Vec2::splat(f32::INFINITY), max: Vec2::splat(f32::NEG_INFINITY) };
    for corner in [
        Vec3::new(min.x, 0.0, min.z),
        Vec3::new(max.x, 0.0, min.z),
        Vec3::new(min.x, 0.0, max.z),
        Vec3::new(max.x, 0.0, max.z),
    ] {
        let p = rotation * corner;
        let p = Vec2::new(p.x, p.z) + at;
        rect.min = rect.min.min(p);
        rect.max = rect.max.max(p);
    }
    rect
}

/// The point on the wall of `room` behind `at`, a hair in front of it.
fn snap_to_wall(at: Vec3, room: Rect, out: Vec2) -> Vec3 {
    let mut at = at;
    if out.x > 0.5 {
        at.x = room.min.x + 0.012;
    } else if out.x < -0.5 {
        at.x = room.max.x - 0.012;
    } else if out.y > 0.5 {
        at.z = room.min.y + 0.012;
    } else {
        at.z = room.max.y - 0.012;
    }
    at
}
