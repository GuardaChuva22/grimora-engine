//! The fresh water drawn as one surface: every stream and pond on the same grid, tile by
//! tile with the ground (the tile's own grid), each grid point at the level of the water it belongs to ([`Hydrology::fresh_at`]).
//! Where a stream runs into a pond or another stream the grid just goes on, so there is no
//! edge, no second sheet over the first and no change of look; the ground hides the grid's
//! own edge, a little past the water's. Each vertex says which way the water flows (uv, in
//! meters per second, for the flow map in `fs_water`), how far it is into the stretch a fall's
//! curtain stands for (tangent x, in meters, above 0 inside, where `fs_water` drops it: see
//! [`crate::Curtain`]), how churned up it is below a fall (tangent y), where across the stream
//! it is (tangent z), how much of it shows (normal y) and how much of the sea's colors it takes
//! near a mouth (vertex color and weights, as a river joining another water).
//!
//! [`Hydrology::fresh_at`]: crate::Hydrology::fresh_at

use glam::{DVec2, Vec3};
use grimora_core::{MeshData, RiverPoint, Vertex, WaterLook};

use crate::fall::{Curtain, curtains};
use crate::{Relief, StreamEnd, TileKey};

/// Tiles up to this level carry the water; past it a tile's cells are wider than any stream
/// dug for it, and the water would hang over the ground.
pub const FRESH_TOP_LEVEL: u8 = 4;
/// How far up from its mouth a stream takes on the sea's colors, in meters.
const SEA_BLEND: f64 = 160.0;
/// How far before a pond a stream slows down to its stillness, in meters.
const POND_STILL: f64 = 40.0;
/// Where the ground is this far under the sea, the sea covers the water: none is drawn.
const UNDER_SEA: f64 = -1.5;
/// Past the water's edge, a sheet over ground this far under it would hang in the air.
const HANGS: f64 = 1.5;
/// How far past the ground dug for it the grid may go looking for where the ground rises out
/// of the water, in meters and in cells.
const REACH: (f64, f64) = (8.0, 2.0);
/// Marks the vertices of this surface for `vs_water`, in the normal's z.
const MARK: f32 = 0.35;
/// How far into or out of a curtain's stretch the surface counts, in meters.
const HIDDEN_REACH: f64 = 20.0;

/// What each stream's course needs, worked out once.
struct Course {
    along: Vec<f64>,
    profile: Vec<[f32; 3]>,
    curtains: Vec<Curtain>,
}

impl Course {
    /// How far into the stretch of one of its curtains `along` is, in meters: above 0 inside
    /// it, falling below 0 away from it both ways, so it never rises between two curtains. And
    /// the level the surface keeps there: the lip's near the lip, the pool's near the pool, so
    /// what is left of a triangle the stretch cuts lies flat.
    fn hidden(&self, along: f64) -> (f64, Option<f64>) {
        let mut into = -HIDDEN_REACH;
        let mut level = None;
        for curtain in &self.curtains {
            let (from, to) = curtain.hidden;
            let here = (along - from).min(to - along);
            if here > into {
                into = here;
                level =
                    (here > 0.0).then(|| if along - from < to - along { curtain.levels.0 } else { curtain.levels.1 });
            }
        }
        (into.clamp(-HIDDEN_REACH, HIDDEN_REACH), level)
    }
}

/// What every stream's course needs to draw it, worked out once for the whole world.
pub struct FreshCourses(Vec<Course>);

impl FreshCourses {
    /// Every fall's curtain, with the stream it is on.
    pub fn curtains(&self) -> impl Iterator<Item = (usize, &Curtain)> {
        self.0.iter().enumerate().flat_map(|(s, course)| course.curtains.iter().map(move |c| (s, c)))
    }
}

pub fn fresh_courses(relief: &Relief) -> FreshCourses {
    let water = relief.water();
    FreshCourses(
        water
            .streams
            .iter()
            .map(|stream| {
                let mut along = vec![0.0; stream.points.len()];
                for i in 1..stream.points.len() {
                    along[i] = along[i - 1] + stream.points[i].at.distance(stream.points[i - 1].at);
                }
                // From its first point: in the world's meters a float can't tell 1 m apart.
                let start = stream.points.first().map_or(glam::DVec3::ZERO, |p| p.at);
                let course: Vec<RiverPoint> = stream
                    .points
                    .iter()
                    .map(|p| RiverPoint { position: (p.at - start).as_vec3(), half_width: p.half_width as f32 })
                    .collect();
                Course {
                    curtains: curtains(&stream.points, &along, stream.speed),
                    profile: grimora_scene::fall_profile(&course),
                    along,
                }
            })
            .collect(),
    )
}

/// The water over a ground tile, on the tile's own grid, so its triangles are the ground's and
/// the shore is exactly where they cross: a mesh in meters from the tile's center (at height
/// 0), with no material yet. `None` where there is no water, or the tile is coarser than
/// [`FRESH_TOP_LEVEL`]. `sea` is the look a stream takes on at its mouth.
pub fn fresh_tile(relief: &Relief, courses: &FreshCourses, sea: Option<&WaterLook>, key: TileKey) -> Option<MeshData> {
    if key.level > FRESH_TOP_LEVEL {
        return None;
    }
    let mesh = grid(relief, &courses.0, sea, key.min(), key.center(), key.cell(), key.cells());
    (!mesh.indices.is_empty()).then_some(mesh)
}

/// The surface over `n` × `n` cells `cell` meters apart from `min`.
fn grid(
    relief: &Relief,
    courses: &[Course],
    sea: Option<&WaterLook>,
    min: DVec2,
    center: DVec2,
    cell: f64,
    n: usize,
) -> MeshData {
    let water = relief.water();
    // The water goes on until the ground the tiles draw at this spacing (`Relief::height_seen`,
    // on the same grid points) stands over it: a triangle is kept if the water shows anywhere
    // in it, and the ground hides the rest, so the shore is the ground's own line.
    let wide = cell * std::f64::consts::SQRT_2 + 0.1;
    let reach = REACH.0 + REACH.1 * cell;
    let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
    let ease = |t: f64| {
        let t = t.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let mut slots: Vec<Option<u32>> = vec![None; (n + 1) * (n + 1)];
    let mut wet = vec![false; (n + 1) * (n + 1)];
    let mut mesh = MeshData::new();
    for j in 0..=n {
        for i in 0..=n {
            let p = min + DVec2::new(i as f64, j as f64) * cell;
            let Some(here) = water.fresh_at(p, wide, reach) else { continue };
            let stream = here.stream.map(|(s, k, t)| (s, k, t, &water.streams[s]));
            let to_sea = stream.is_some_and(|(.., st)| st.end == StreamEnd::Sea);
            if to_sea && relief.raw_height_at(p.x, p.y) < UNDER_SEA {
                continue;
            }
            let ground = relief.height_seen(p.x, p.y, cell);
            if here.out > 0.0 && ground < here.level - HANGS {
                continue;
            }
            wet[j * (n + 1) + i] = ground < here.level;
            let mut flow = DVec2::ZERO;
            let mut churn = 0.0f32;
            let mut hidden = -HIDDEN_REACH;
            let mut level = here.level;
            let mut join = 0.0;
            if let Some((s, k, t, st)) = stream {
                let (a, b) = (st.points[k], st.points[k + 1]);
                let along = courses[s].along[k] + (courses[s].along[k + 1] - courses[s].along[k]) * t;
                let left = courses[s].along.last().copied().unwrap_or(0.0) - along;
                let still = match st.end {
                    StreamEnd::Pond(_) => ease(left / POND_STILL),
                    _ => 1.0,
                };
                flow = DVec2::new(b.at.x - a.at.x, b.at.z - a.at.z).normalize_or_zero() * st.speed * still;
                let (fa, fb) = (courses[s].profile[k], courses[s].profile[k + 1]);
                churn = fa[1] + (fb[1] - fa[1]) * t as f32;
                let (into, kept) = courses[s].hidden(along);
                hidden = into;
                level = kept.unwrap_or(level);
                if to_sea {
                    join = ease(1.0 - left / SEA_BLEND) as f32;
                }
            }
            let at = Vec3::new((p.x - center.x) as f32, level as f32, (p.y - center.y) as f32);
            let mut vertex = Vertex::new(at, Vec3::new(1.0, 1.0, MARK), Vec3::ONE);
            vertex.uv = [flow.x as f32, flow.y as f32];
            vertex.tangent = [hidden as f32, churn, here.across.clamp(-2.0, 2.0) as f32, 0.0];
            if let Some(sea) = sea {
                vertex.color = sea.deep.to_array();
                vertex.weights = [byte(join), byte(sea.shallow.x), byte(sea.shallow.y), byte(sea.shallow.z)];
            } else {
                vertex.weights = [0; 4];
            }
            slots[j * (n + 1) + i] = Some(mesh.vertices.len() as u32);
            mesh.vertices.push(vertex);
        }
    }
    let k = |i: usize, j: usize| j * (n + 1) + i;
    for j in 0..n {
        for i in 0..n {
            let (a, b, c, d) = (k(i, j), k(i, j + 1), k(i + 1, j + 1), k(i + 1, j));
            for t in [[a, b, c], [a, c, d]] {
                if !t.iter().any(|&v| wet[v]) {
                    continue;
                }
                if let [Some(x), Some(y), Some(z)] = t.map(|v| slots[v]) {
                    mesh.indices.extend_from_slice(&[x, y, z]);
                }
            }
        }
    }
    mesh.shadowless = true;
    mesh
}
