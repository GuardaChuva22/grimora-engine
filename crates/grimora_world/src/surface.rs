//! The fresh water drawn as one surface: every stream and pond on the same grid, chunk by
//! chunk, each grid point at the level of the water it belongs to ([`Hydrology::fresh_at`]).
//! Where a stream runs into a pond or another stream the grid just goes on, so there is no
//! edge, no second sheet over the first and no change of look; the ground hides the grid's
//! own edge, a little past the water's. Each vertex says which way the water flows (uv, in
//! meters per second, for the flow map in `fs_water`), what a fall does there (tangent, as
//! `river_mesh` writes it), how much of it shows (normal y) and how much of the sea's colors
//! it takes near a mouth (vertex color and weights, as a river joining another water).
//!
//! [`Hydrology::fresh_at`]: crate::Hydrology::fresh_at

use glam::{DVec2, Vec3};
use grimora_core::{MeshData, RiverPoint, Vertex, WaterLook};

use crate::{Relief, StreamEnd};

/// Side of a chunk of the surface, in meters.
pub const FRESH_CHUNK: f64 = 128.0;
/// Each level of detail: the grid's spacing and below what screen size it is used. A level
/// takes over a little before the ground's tiles get as coarse, so the water always covers
/// the ground they dig for it.
const DETAIL: [(f64, f32); 4] = [(2.0, f32::INFINITY), (8.0, 0.4), (16.0, 0.2), (32.0, 0.1)];
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

/// One chunk of the surface: a mesh in meters from `center` (at height 0), with its levels of
/// detail, and no material yet.
pub struct FreshChunk {
    pub center: DVec2,
    pub mesh: MeshData,
}

/// What each stream's course needs, worked out once.
struct Course {
    along: Vec<f64>,
    profile: Vec<[f32; 3]>,
}

/// Every chunk of the fresh water, made on every core. `sea` is the look a stream takes on at
/// its mouth.
pub fn fresh_chunks(relief: &Relief, sea: Option<WaterLook>) -> Vec<FreshChunk> {
    let water = relief.water();
    let courses: Vec<Course> = water
        .streams
        .iter()
        .map(|stream| {
            let mut along = vec![0.0; stream.points.len()];
            for i in 1..stream.points.len() {
                along[i] = along[i - 1] + stream.points[i].at.distance(stream.points[i - 1].at);
            }
            let course: Vec<RiverPoint> = stream
                .points
                .iter()
                .map(|p| RiverPoint { position: p.at.as_vec3(), half_width: p.half_width as f32 })
                .collect();
            Course { along, profile: grimora_scene::fall_profile(&course) }
        })
        .collect();
    // Every chunk some water comes near.
    let mut keys = std::collections::BTreeSet::new();
    let mut mark = |lo: DVec2, hi: DVec2| {
        let (a, b) = ((lo / FRESH_CHUNK).floor(), (hi / FRESH_CHUNK).floor());
        for z in a.y as i64..=b.y as i64 {
            for x in a.x as i64..=b.x as i64 {
                keys.insert((x, z));
            }
        }
    };
    let margin = DETAIL[0].0 * 3.0;
    for stream in &water.streams {
        for p in &stream.points {
            let at = DVec2::new(p.at.x, p.at.z);
            let r = DVec2::splat(p.half_width + margin);
            mark(at - r, at + r);
        }
    }
    for pond in &water.ponds {
        let r = DVec2::splat(pond.reach() + margin);
        mark(pond.center - r, pond.center + r);
    }
    let keys: Vec<(i64, i64)> = keys.into_iter().collect();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let per = keys.len().div_ceil(threads).max(1);
    let courses = &courses;
    std::thread::scope(|scope| {
        keys.chunks(per)
            .map(|part| {
                scope.spawn(move || {
                    part.iter().filter_map(|&key| chunk(relief, courses, sea.as_ref(), key)).collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .flat_map(|job| job.join().expect("a fresh water chunk"))
            .collect()
    })
}

fn chunk(relief: &Relief, courses: &[Course], sea: Option<&WaterLook>, key: (i64, i64)) -> Option<FreshChunk> {
    let min = DVec2::new(key.0 as f64, key.1 as f64) * FRESH_CHUNK;
    let center = min + DVec2::splat(FRESH_CHUNK * 0.5);
    let mut meshes = DETAIL.iter().map(|&(cell, _)| grid(relief, courses, sea, min, center, cell));
    let base = meshes.next()?;
    if base.indices.is_empty() {
        return None;
    }
    let lods: Vec<(f32, MeshData)> =
        meshes.zip(&DETAIL[1..]).filter(|(m, _)| !m.indices.is_empty()).map(|(m, &(_, size))| (size, m)).collect();
    Some(FreshChunk { center, mesh: base.with_lods(lods) })
}

/// The surface over a chunk on a grid `cell` meters apart.
fn grid(relief: &Relief, courses: &[Course], sea: Option<&WaterLook>, min: DVec2, center: DVec2, cell: f64) -> MeshData {
    let water = relief.water();
    let n = (FRESH_CHUNK / cell).round() as usize;
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
            let mut fall = [0.0f32; 3];
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
                fall = [0, 1, 2].map(|c| fa[c] + (fb[c] - fa[c]) * t as f32);
                if to_sea {
                    join = ease(1.0 - left / SEA_BLEND) as f32;
                }
            }
            let at = Vec3::new((p.x - center.x) as f32, here.level as f32, (p.y - center.y) as f32);
            let mut vertex = Vertex::new(at, Vec3::new(1.0, 1.0, MARK), Vec3::ONE);
            vertex.uv = [flow.x as f32, flow.y as f32];
            vertex.tangent = [fall[0], fall[1], here.across.clamp(-2.0, 2.0) as f32, fall[2]];
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
