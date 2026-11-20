//! Falls drawn as curtains. Where a stream's course drops steeply, its water leaves the lip in
//! an arc, falls free of the rock and plunges into the pool at its foot, as a sheet of its own:
//! the stream's surface ([`crate::fresh_tile`]) stops at the lip and starts again just before
//! the curtain lands ([`Curtain::hidden`]), so neither ever lies over the other, and the curtain
//! is never a ramp of the ground's grid, with its stairs at the edges.
//!
//! Each vertex carries where on the sheet it is: meters across it and the seconds a drop takes
//! from the lip to there. `fs_water` slides its streaks down by those seconds at a steady pace,
//! so they speed up and stretch as the water falls, without ever bunching up over time.

use glam::{DVec2, DVec3, Vec3};
use grimora_core::{MeshData, Vertex};

use crate::StreamPoint;

/// A stretch of a course dropping more than this many meters per meter is a fall, drawn as a
/// curtain; gentler ones are rapids, drawn on the stream's surface.
const STEEPEST_RAPID: f64 = 0.6;
/// Falls lower than this, in meters, are rapids too.
const LOWEST: f64 = 1.5;
const GRAVITY: f64 = 9.81;
/// How fast the water leaves the lip: the stream's speed plus the first, between the other two,
/// in meters per second. Faster arcs further out from the rock.
const LIP_SPEED: (f64, f64, f64) = (0.8, 1.4, 3.0);
/// How far under the stream's surface the curtain runs before the lip, so the surface covers it
/// up to there, and how far upstream it starts.
const UNDER_SURFACE: f64 = 0.05;
const APPROACH: f64 = 0.6;
/// How deep under the pool's surface it plunges, out of sight.
const PLUNGE: f64 = 0.8;
/// Meters between two rows down the sheet (fewer where it is nearly straight), and columns across.
const ROW: f64 = 0.35;
const COLUMNS: usize = 12;
/// How much of the channel's half width the curtain takes at the lip, and how much wider it
/// spreads by its foot.
const WIDTH: f64 = 0.9;
const SPREADS: f64 = 0.15;
/// How far its middle bulges out past its sides, in meters, once it falls.
const BULGE: f64 = 0.3;
/// The stream's surface starts again this far (along its course) before the curtain lands, so
/// the curtain goes into it.
const SURFACE_BACK: f64 = 0.3;
/// Marks the curtain's vertices for `vs_water`, in the normal's z.
const MARK: f32 = 0.45;

/// A fall's curtain, in world coordinates.
#[derive(Debug, Clone)]
pub struct Curtain {
    /// Where the water leaves the lip, at its level.
    pub lip: DVec3,
    /// Where the curtain goes into the pool, at its level.
    pub landing: DVec3,
    pub height: f64,
    /// Half its width at the lip.
    pub half_width: f64,
    /// The stretch of the stream's course the curtain stands for, in meters along it as
    /// [`crate::FreshCourses`] counts them (from point to point, drop included): from the lip
    /// to where the surface starts again.
    pub hidden: (f64, f64),
    /// The water's level at the lip and in the pool.
    pub levels: (f64, f64),
    /// The stream's speed, for the flow its surface had over the lip.
    speed: f64,
    rows: Vec<Row>,
}

/// One row across the sheet.
#[derive(Debug, Clone, Copy)]
struct Row {
    middle: DVec3,
    /// Downstream, level and of length 1.
    ahead: DVec2,
    half: f64,
    /// Seconds a drop takes from the lip to here (below 0 before it).
    flight: f64,
    /// How far down the fall, from 0 at the lip to 1 at the pool.
    down: f64,
    /// How much of it is falling: 0 before the lip, 1 once it has dropped half a meter.
    falling: f64,
}

/// The curtains of a stream's falls, from its points, how far along each one is (as
/// [`crate::FreshCourses`] counts it) and the stream's speed.
pub fn curtains(points: &[StreamPoint], along: &[f64], speed: f64) -> Vec<Curtain> {
    let flat = |i: usize| DVec2::new(points[i].at.x, points[i].at.z);
    let steep =
        |i: usize| (points[i].at.y - points[i + 1].at.y) / flat(i).distance(flat(i + 1)).max(0.01) > STEEPEST_RAPID;
    let mut curtains = Vec::new();
    let mut i = 0;
    while i + 1 < points.len() {
        if !steep(i) {
            i += 1;
            continue;
        }
        let lip = i;
        let mut foot = i + 1;
        while foot + 1 < points.len() && steep(foot) {
            foot += 1;
        }
        i = foot;
        if points[lip].at.y - points[foot].at.y >= LOWEST {
            curtains.extend(curtain(points, along, speed, lip, foot));
        }
    }
    curtains
}

/// The course seen from above, from a point before the lip on: meters from the lip (below 0
/// before it), place, level and half width at each of its points.
struct Plan {
    stations: Vec<(f64, DVec2, f64, f64)>,
    /// Which point of the stream the first station is.
    first: usize,
}

impl Plan {
    /// Place, way downstream, level and half width at `d` meters from the lip. Before the first
    /// station and past the last the course goes on straight and level.
    fn at(&self, d: f64) -> (DVec2, DVec2, f64, f64) {
        let s = &self.stations;
        let k = s.iter().rposition(|st| st.0 <= d).unwrap_or(0).min(s.len() - 2);
        let (a, b) = (s[k], s[k + 1]);
        let ahead = (b.1 - a.1).normalize_or_zero();
        let t = (d - a.0) / (b.0 - a.0).max(1e-6);
        let level = if t < 0.0 {
            a.2
        } else if t > 1.0 {
            b.2
        } else {
            a.2 + (b.2 - a.2) * t
        };
        let half = a.3 + (b.3 - a.3) * t.clamp(0.0, 1.0);
        (a.1 + ahead * (d - a.0), ahead, level, half)
    }

    /// How far along the stream (as [`crate::FreshCourses`] counts it) `d` meters from the lip is.
    fn along(&self, along: &[f64], d: f64) -> f64 {
        let s = &self.stations;
        let k = s.iter().rposition(|st| st.0 <= d).unwrap_or(0).min(s.len() - 2);
        let t = ((d - s[k].0) / (s[k + 1].0 - s[k].0).max(1e-6)).clamp(0.0, 1.0);
        let (i, j) = (self.first + k, self.first + k + 1);
        along[i] + (along[j] - along[i]) * t
    }
}

fn curtain(points: &[StreamPoint], along: &[f64], speed: f64, lip: usize, foot: usize) -> Option<Curtain> {
    let flat = |i: usize| DVec2::new(points[i].at.x, points[i].at.z);
    let (top, pool) = (points[lip].at.y, points[foot].at.y);
    let height = top - pool;
    let first = lip.saturating_sub(1);
    let last = (foot + 2).min(points.len() - 1);
    let mut stations = Vec::new();
    let mut d = -flat(lip).distance(flat(first));
    for k in first..=last {
        if k > first {
            d += flat(k).distance(flat(k - 1));
        }
        stations.push((d, flat(k), points[k].at.y, points[k].half_width));
    }
    if stations.len() < 2 {
        return None;
    }
    let plan = Plan { stations, first };
    // Out over the lip in an arc, never through the rock: where the course drops slower than the
    // arc, the water runs down it instead.
    let v0 = (speed + LIP_SPEED.0).clamp(LIP_SPEED.1, LIP_SPEED.2);
    let arc = |d: f64| if d <= 0.0 { top } else { top - GRAVITY * d * d / (2.0 * v0 * v0) };
    let water = |d: f64| arc(d).max(plan.at(d).2);
    let fell = plan.stations.last().map_or(0.0, |s| s.0) + 30.0;
    let mut land = None;
    let mut d = 0.0;
    while d < fell {
        if water(d) <= pool + 0.02 {
            land = Some(d);
            break;
        }
        d += 0.01;
    }
    let land = land?;
    // Rows from just upstream of the lip to the landing, closer where it bends.
    let mut rows: Vec<Row> = Vec::new();
    let row_at = |d: f64, y: f64, flight: f64| {
        let (place, ahead, _, half) = plan.at(d);
        let down = ((top - y) / height).clamp(0.0, 1.0);
        let falling = ((top - y) / 0.5).clamp(0.0, 1.0);
        let falling = falling * falling * (3.0 - 2.0 * falling);
        let half = half * WIDTH * (1.0 + SPREADS * down);
        Row { middle: DVec3::new(place.x, y - UNDER_SURFACE, place.y), ahead, half, flight, down, falling }
    };
    let step = 0.01;
    let mut d = -APPROACH;
    let mut flight = -APPROACH / v0;
    let mut last = DVec3::new(plan.at(d).0.x, water(d), plan.at(d).0.y);
    let mut way = 0.0;
    rows.push(row_at(d, water(d), flight));
    let mut turned = DVec3::ZERO;
    while d < land {
        d = (d + step).min(land);
        let y = water(d);
        let place = plan.at(d).0;
        let here = DVec3::new(place.x, y, place.y);
        let moved = here.distance(last);
        // Speed from the drop so far: a drop of water falling, whichever way it goes.
        let fast = (v0 * v0 + 2.0 * GRAVITY * (top - y).max(0.0)).sqrt();
        flight += moved / fast;
        way += moved;
        let heading = (here - last).normalize_or_zero();
        if turned == DVec3::ZERO {
            turned = heading;
        }
        // A new row every `ROW` meters, or sooner where the sheet bends.
        if way >= ROW || heading.dot(turned) < 0.97 || d >= land {
            rows.push(row_at(d, y, flight));
            way = 0.0;
            turned = heading;
        }
        last = here;
    }
    // Then straight down into the pool, out of sight.
    let end = rows.last().copied()?;
    let mut under = row_at(land, pool - PLUNGE, flight + (2.0 * PLUNGE / GRAVITY).sqrt() * 0.5);
    under.middle += DVec3::new(end.ahead.x, 0.0, end.ahead.y) * 0.1;
    rows.push(under);
    let landing = {
        let place = plan.at(land).0;
        DVec3::new(place.x, pool, place.y)
    };
    let hidden = (along[lip], plan.along(along, land) - SURFACE_BACK);
    Some(Curtain {
        lip: points[lip].at,
        landing,
        height,
        half_width: points[lip].half_width * WIDTH,
        hidden: (hidden.0, hidden.1.max(hidden.0)),
        levels: (top, pool),
        speed,
        rows,
    })
}

impl Curtain {
    /// The sheet, in meters from `origin`, with no material yet.
    pub fn mesh(&self, origin: DVec3) -> MeshData {
        let mut mesh = MeshData::new();
        for row in &self.rows {
            let side = DVec2::new(-row.ahead.y, row.ahead.x);
            let flow = row.ahead * self.speed;
            for c in 0..=COLUMNS {
                let u = c as f64 / COLUMNS as f64 * 2.0 - 1.0;
                // Falling, its middle stands out from its sides, as water heaped up by the lip.
                let out = row.ahead * BULGE * (1.0 - u * u) * row.falling;
                let across = side * u * row.half;
                let at = row.middle + DVec3::new(across.x + out.x, 0.0, across.y + out.y) - origin;
                // Its color says where on the sheet it is: meters across, seconds of flight.
                let mut vertex = Vertex::new(
                    at.as_vec3(),
                    Vec3::new(1.0, 1.0, MARK),
                    Vec3::new((u * row.half) as f32, row.flight as f32, 0.0),
                );
                vertex.uv = [flow.x as f32, flow.y as f32];
                vertex.tangent = [row.falling as f32, 0.0, u as f32, row.down as f32];
                vertex.weights = [0; 4];
                mesh.vertices.push(vertex);
            }
        }
        let across = (COLUMNS + 1) as u32;
        for r in 0..self.rows.len().saturating_sub(1) as u32 {
            for c in 0..COLUMNS as u32 {
                let a = r * across + c;
                let (b, d, e) = (a + 1, a + across, a + across + 1);
                // Facing up over the lip, and out from the rock once it falls.
                mesh.indices.extend_from_slice(&[a, b, d, b, e, d]);
            }
        }
        mesh.shadowless = true;
        mesh
    }

    /// Points down the middle of its face, a little in front of it, `down` of the way to the pool.
    pub fn face_at(&self, down: f64) -> DVec3 {
        let row = self.rows.iter().find(|r| r.down >= down).or(self.rows.last()).copied().unwrap_or(Row {
            middle: self.lip,
            ahead: DVec2::X,
            half: 0.0,
            flight: 0.0,
            down: 0.0,
            falling: 0.0,
        });
        row.middle + DVec3::new(row.ahead.x, 0.0, row.ahead.y) * (BULGE + 0.4)
    }

    /// How many rows down the sheet.
    pub fn rows(&self) -> usize {
        self.rows.len()
    }
}
