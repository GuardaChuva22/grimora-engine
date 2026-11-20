//! Seas and rivers as meshes, the map of what lies under a sea, and hulls that float:
//! what `SceneDesc::water` and `SceneDesc::rivers` turn into.

use std::sync::Arc;

use grimora_core::{
    AlphaMode, HullMap, Material, MeshData, RiverPoint, Shading, TextureData, Vec2, Vec3, Vec4, Vertex, WaterBed,
    WaterLook, water::HULL_MAP_SIZE,
};
use serde::{Deserialize, Serialize};

use crate::Terrain;
use crate::texture::{WaterDesc, srgb_to_linear};

/// A river, as written in scene files: it flows from its first point to its last.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RiverDesc {
    /// Its middle. `y` is how far the water is above the ground there, so it follows the
    /// land, but it never runs uphill.
    pub points: Vec<Vec3>,
    pub width: f32,
    /// How far under the water the terrain is dug.
    pub depth: f32,
    /// Meters per second.
    pub speed: f32,
    /// Image file, relative to the scene file; the flow carries it.
    pub texture: Option<String>,
    /// Image of its foam, relative to the scene file. Noise without one.
    pub foam_texture: Option<String>,
    /// Meters one repeat of the image covers.
    pub tile: f32,
    /// Height of the ripples in meters.
    pub waves: f32,
    pub look: WaterLook,
    /// Effect file, relative to the scene file, that plays at the foot of each waterfall. A
    /// stretch of the river is a waterfall if it drops steeply enough.
    pub fall_effect: Option<String>,
    /// Sound file that loops at the foot of each waterfall.
    pub fall_sound: Option<String>,
}

impl Default for RiverDesc {
    fn default() -> Self {
        Self {
            points: Vec::new(),
            width: 6.0,
            depth: 1.4,
            speed: 1.5,
            texture: None,
            foam_texture: None,
            tile: 3.0,
            waves: 0.03,
            look: WaterLook {
                deep: Vec3::new(0.04, 0.24, 0.22),
                shallow: Vec3::new(0.3, 0.5, 0.36),
                clarity: 3.0,
                murk: 0.4,
                ..Default::default()
            },
            fall_effect: None,
            fall_sound: None,
        }
    }
}

/// A smooth strip through a river's points, with the water level at each step.
pub(crate) fn river_course(desc: &RiverDesc, ground: impl Fn(f32, f32) -> f32) -> Vec<RiverPoint> {
    const STEP: f32 = 2.0;
    let points = &desc.points;
    let mut course: Vec<RiverPoint> = Vec::new();
    let half = desc.width.max(0.1) * 0.5;
    for i in 0..points.len().saturating_sub(1) {
        let (p0, p1) = (points[i.saturating_sub(1)], points[i]);
        let (p2, p3) = (points[i + 1], points[(i + 2).min(points.len() - 1)]);
        let steps = (p1.distance(p2) / STEP).ceil().max(1.0) as u32;
        for s in 0..steps {
            let t = s as f32 / steps as f32;
            let (t2, t3) = (t * t, t * t * t);
            let p = 0.5
                * (p1 * 2.0
                    + (p2 - p0) * t
                    + (p0 * 2.0 - p1 * 5.0 + p2 * 4.0 - p3) * t2
                    + (p1 * 3.0 - p0 - p2 * 3.0 + p3) * t3);
            let mut level = ground(p.x, p.z) + p.y;
            if let Some(last) = course.last() {
                level = level.min(last.position.y);
            }
            course.push(RiverPoint { position: Vec3::new(p.x, level, p.z), half_width: half });
        }
    }
    if let Some(&end) = points.last() {
        let level = course
            .last()
            .map_or(ground(end.x, end.z) + end.y, |last| last.position.y.min(ground(end.x, end.z) + end.y));
        course.push(RiverPoint { position: Vec3::new(end.x, level, end.z), half_width: half });
    }
    course
}

/// A river's drop per meter on the ground from which it starts falling, and from which it
/// falls for good.
const FALL_FROM: f32 = 0.35;
const FALL_SOLID: f32 = 0.85;
/// Falls lower than this, in meters, are rapids: no effect or sound of their own.
const FALL_LOWEST: f32 = 1.5;

/// A place where a river falls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RiverFall {
    /// Where it starts falling, on the water.
    pub top: Vec3,
    /// Where it lands, on the water.
    pub foot: Vec3,
    pub height: f32,
    pub half_width: f32,
}

/// How much of each point of a course is falling, `0..1`.
fn falling(course: &[RiverPoint]) -> Vec<f32> {
    let drop = |i: usize| {
        let (a, b) = (course[i].position, course[(i + 1).min(course.len() - 1)].position);
        (a.y - b.y) / (a.with_y(0.0).distance(b.with_y(0.0))).max(0.01)
    };
    let smooth = |x: f32| {
        let t = ((x - FALL_FROM) / (FALL_SOLID - FALL_FROM)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    (0..course.len()).map(|i| smooth(drop(i).max(drop(i.saturating_sub(1))))).collect()
}

/// The falls of a course, from the top down.
pub fn river_falls(course: &[RiverPoint]) -> Vec<RiverFall> {
    let fall = falling(course);
    let mut falls = Vec::new();
    let mut top = None;
    for i in 0..course.len() {
        match (top, fall[i] > 0.5) {
            (None, true) => top = Some(i),
            (Some(start), false) => {
                let height: f32 = course[start].position.y - course[i].position.y;
                if height >= FALL_LOWEST {
                    let at = course[i];
                    falls.push(RiverFall {
                        top: course[start].position,
                        foot: at.position,
                        height,
                        half_width: at.half_width,
                    });
                }
                top = None;
            }
            _ => {}
        }
    }
    falls
}

/// The material of a body of water with this look, pictures and tile.
pub fn water_material(
    look: &WaterLook,
    texture: Option<Arc<TextureData>>,
    foam: Option<Arc<TextureData>>,
    tile: f32,
) -> Material {
    let [r, g, b] = look.deep.to_array().map(srgb_to_linear);
    Material {
        name: Some("water".into()),
        base_color: Vec4::new(r, g, b, 1.0),
        roughness: 0.2,
        alpha_mode: AlphaMode::Blend,
        shading: Shading::Water,
        pixelated: true,
        base_color_texture: texture,
        emissive_texture: foam,
        water: Some(*look),
        water_tile: tile.max(0.01),
        ..Default::default()
    }
}

/// What a course's water does at each point: how much of it is falling, how churned up it
/// still is below a fall, and how far down its fall it is. `fs_water` draws these.
pub fn fall_profile(course: &[RiverPoint]) -> Vec<[f32; 3]> {
    if course.is_empty() {
        return Vec::new();
    }
    let fall = falling(course);
    // How far down its fall each point is, and how stirred up the water still is below one.
    let mut down = vec![0.0f32; course.len()];
    let mut churn = vec![0.0f32; course.len()];
    let mut height = 0.0f32;
    let mut past = f32::MAX;
    for i in 0..course.len() {
        let before = course[i.saturating_sub(1)].position;
        if fall[i] > 0.5 {
            height += before.y - course[i].position.y;
            past = 0.0;
        } else if past < f32::MAX {
            past += before.with_y(0.0).distance(course[i].position.with_y(0.0));
        }
        if fall[i] <= 0.5 && past < f32::MAX {
            churn[i] = (1.0 - past / (height * 1.6).clamp(5.0, 26.0)).max(0.0);
            if churn[i] <= 0.0 {
                height = 0.0;
                past = f32::MAX;
            }
        }
    }
    let mut run = 0.0f32;
    for i in 0..course.len() {
        if fall[i] > 0.5 {
            run += course[i.saturating_sub(1)].position.y - course[i].position.y;
            let mut end = i;
            while end + 1 < course.len() && fall[end + 1] > 0.5 {
                end += 1;
            }
            let total = run + course[i].position.y - course[end].position.y;
            down[i] = run / total.max(0.01);
        } else {
            run = 0.0;
        }
    }
    (0..course.len()).map(|i| [fall[i], churn[i], down[i]]).collect()
}

/// A strip along a river's course. Its uv runs along the water, in repeats of the picture,
/// which is how the picture can flow around the bends without stretching.
pub fn river_mesh(
    desc: &RiverDesc,
    course: &[RiverPoint],
    texture: Option<Arc<TextureData>>,
    foam: Option<Arc<TextureData>>,
) -> MeshData {
    const ACROSS: u32 = 8;
    let tile = desc.tile.max(0.01);
    let mut mesh = MeshData::new();
    let mut along = 0.0;
    let profile = fall_profile(course);
    for (i, point) in course.iter().enumerate() {
        let before = course[i.saturating_sub(1)].position;
        let after = course[(i + 1).min(course.len() - 1)].position;
        let dir = (after - before).with_y(0.0).normalize_or(Vec3::X);
        let side = Vec3::new(-dir.z, 0.0, dir.x);
        if i > 0 {
            along += before.distance(point.position);
        }
        for c in 0..=ACROSS {
            let f = c as f32 / ACROSS as f32 * 2.0 - 1.0;
            // A little wider than the river: the banks hide its edge.
            let across = f * point.half_width * 1.3;
            let mut vertex = Vertex::new(point.position + side * across, Vec3::new(1.0, 1.0, 0.0), Vec3::ONE);
            vertex.uv = [across / tile, along / tile];
            // These two are not a tangent: `vs_water` reads them as what a fall is doing.
            let [fall, churn, down] = profile[i];
            vertex.tangent = [fall, churn, f * 1.3, down];
            mesh.vertices.push(vertex);
        }
    }
    let row = ACROSS + 1;
    for j in 0..course.len().saturating_sub(1) as u32 {
        for i in 0..ACROSS {
            let (a, b, c, d) = (j * row + i, (j + 1) * row + i, (j + 1) * row + i + 1, j * row + i + 1);
            mesh.indices.extend_from_slice(&[a, b, c, a, c, d]);
        }
    }
    let mut material = water_material(&desc.look, texture, foam, tile);
    material.water_flow = Vec2::new(0.0, desc.speed / tile);
    material.water_river = 1.0;
    mesh.materials = vec![material];
    mesh.wind = desc.waves.max(0.0);
    mesh.shadowless = true;
    mesh
}

/// Rings around the camera, each with cells twice the size of the one inside it, all on one
/// grid. The whole mesh moves in steps of the largest cell, so each vertex keeps to one spot of
/// the world and the waves don't crawl through it. Where a ring meets a larger one, every other
/// vertex of its edge folds onto its neighbor, leaving no gaps. See `vs_water`.
pub(crate) fn sea_mesh(
    desc: &WaterDesc,
    texture: Option<Arc<TextureData>>,
    foam: Option<Arc<TextureData>>,
) -> MeshData {
    const LEVELS: u32 = 7;
    const FIRST_CELL: f32 = 0.6;
    const CELLS: i32 = 48;
    let step = FIRST_CELL * (1u32 << (LEVELS - 1)) as f32;
    let mut mesh = MeshData::new();
    for level in 0..LEVELS {
        let cell = FIRST_CELL * (1u32 << level) as f32;
        let first = mesh.vertices.len() as u32;
        let side = 2 * CELLS + 1;
        for j in -CELLS..=CELLS {
            for i in -CELLS..=CELLS {
                let at = Vec3::new(i as f32 * cell, 0.0, j as f32 * cell);
                mesh.vertices.push(Vertex::new(at, Vec3::new(step, 1.0, 1.0), Vec3::ONE));
            }
        }
        let index = |i: i32, j: i32| {
            // The vertices of the edge that don't exist on the next, larger ring.
            let i2 = if j.abs() == CELLS && i.rem_euclid(2) == 1 { i - 1 } else { i };
            let j2 = if i.abs() == CELLS && j.rem_euclid(2) == 1 { j - 1 } else { j };
            first + ((j2 + CELLS) * side + i2 + CELLS) as u32
        };
        for j in -CELLS..CELLS {
            for i in -CELLS..CELLS {
                let hole = -CELLS / 2..CELLS / 2;
                if level > 0 && hole.contains(&i) && hole.contains(&j) {
                    continue;
                }
                let (a, b, c, d) = (index(i, j), index(i, j + 1), index(i + 1, j + 1), index(i + 1, j));
                for t in [[a, b, c], [a, c, d]] {
                    if t[0] != t[1] && t[1] != t[2] && t[0] != t[2] {
                        mesh.indices.extend_from_slice(&t);
                    }
                }
            }
        }
    }
    let reach = desc.sea.as_ref().map_or(0.0, |s| s.reach);
    far_sea_rings(&mut mesh, FIRST_CELL * (1u32 << LEVELS) as f32, CELLS, step, reach);
    let look = desc.look.unwrap_or_else(|| desc.plain_look());
    mesh.materials = vec![water_material(&look, texture, foam, desc.tile)];
    mesh.shadowless = true;
    mesh.boundless = true;
    mesh
}

/// Rings past the waved ones, out to `reach`, for a horizon kilometers away. They follow the
/// camera in steps of their own largest cell, so they can't meet the waved rings edge to edge:
/// they start well inside them instead, a little lower, where the waves hide them. Their waves
/// fade with the distance (`wave_keeps`), so they lie flat.
fn far_sea_rings(mesh: &mut MeshData, first_cell: f32, cells: i32, near_step: f32, reach: f32) {
    const DROP: f32 = -1.2;
    let near_extent = first_cell * 0.5 * cells as f32;
    let mut levels = 0;
    while near_extent * (1u32 << levels) as f32 * 2.0 < reach && levels < 8 {
        levels += 1;
    }
    if levels == 0 {
        return;
    }
    let step = first_cell * (1u32 << (levels - 1)) as f32;
    // Whatever the two grids' steps, the far rings' hole stays inside the waved rings.
    let hole = near_extent - step - near_step;
    for level in 0..levels {
        let cell = first_cell * (1u32 << level) as f32;
        let first = mesh.vertices.len() as u32;
        let side = 2 * cells + 1;
        for j in -cells..=cells {
            for i in -cells..=cells {
                let at = Vec3::new(i as f32 * cell, DROP, j as f32 * cell);
                mesh.vertices.push(Vertex::new(at, Vec3::new(step, 1.0, 1.0), Vec3::ONE));
            }
        }
        let index = |i: i32, j: i32| {
            let i2 = if j.abs() == cells && i.rem_euclid(2) == 1 { i - 1 } else { i };
            let j2 = if i.abs() == cells && j.rem_euclid(2) == 1 { j - 1 } else { j };
            first + ((j2 + cells) * side + i2 + cells) as u32
        };
        for j in -cells..cells {
            for i in -cells..cells {
                let inside = if level == 0 {
                    let lo = |n: i32| n as f32 * cell >= -hole && (n + 1) as f32 * cell <= hole;
                    lo(i) && lo(j)
                } else {
                    let hole = -cells / 2..cells / 2;
                    hole.contains(&i) && hole.contains(&j)
                };
                if inside {
                    continue;
                }
                let (a, b, c, d) = (index(i, j), index(i, j + 1), index(i + 1, j + 1), index(i + 1, j));
                for t in [[a, b, c], [a, c, d]] {
                    if t[0] != t[1] && t[1] != t[2] && t[0] != t[2] {
                        mesh.indices.extend_from_slice(&t);
                    }
                }
            }
        }
    }
}

/// How deep a sea at `level` is over a terrain.
pub(crate) fn water_bed(terrain: &Terrain, level: f32) -> WaterBed {
    const MOST_SAMPLES: f32 = 512.0;
    const FINEST: f32 = 1.0;
    let size = terrain.size();
    let samples = (size / FINEST).clamp(2.0, MOST_SAMPLES) as u32;
    let half = size * 0.5;
    let mut depths = Vec::with_capacity((samples * samples) as usize);
    for j in 0..samples {
        for i in 0..samples {
            let at = |n: u32| n as f32 / (samples - 1) as f32 * size - half;
            depths.push(level - terrain.height_at(at(i), at(j)));
        }
    }
    WaterBed { min: Vec2::splat(-half), max: Vec2::splat(half), width: samples, height: samples, depths }
}

/// The underside of a mesh over a grid, for a hull that keeps the water out of it.
fn hull_map(points: &[Vec3], indices: &[u32], deck: f32, min: Vec3, max: Vec3) -> HullMap {
    let size = HULL_MAP_SIZE as usize;
    let mut map = HullMap {
        min: Vec2::new(min.x, min.z),
        max: Vec2::new(max.x, max.z),
        deck: deck.min(max.y),
        low: vec![HullMap::OFF; size * size],
    };
    let scale = Vec2::splat(size as f32) / (map.max - map.min).max(Vec2::splat(1e-5));
    let texel = |p: Vec3| (Vec2::new(p.x, p.z) - map.min) * scale;
    let mut low = std::mem::take(&mut map.low);
    let mut stamp = |at: Vec2, y: f32| {
        let (x, z) = (at.x.floor() as isize, at.y.floor() as isize);
        if x >= 0 && z >= 0 && (x as usize) < size && (z as usize) < size {
            let cell = &mut low[z as usize * size + x as usize];
            *cell = cell.min(y);
        }
    };
    for triangle in indices.as_chunks::<3>().0 {
        let [a, b, c] = triangle.map(|i| points[i as usize]);
        // Above the deck it's a mast or a sail, which keeps no water out.
        if a.y > deck && b.y > deck && c.y > deck {
            continue;
        }
        let (ta, tb, tc) = (texel(a), texel(b), texel(c));
        // Along its edges first, as a wall seen from above is too thin to cover a texel.
        for (from, to, y0, y1) in [(ta, tb, a.y, b.y), (tb, tc, b.y, c.y), (tc, ta, c.y, a.y)] {
            let steps = (from.distance(to) * 2.0).ceil().max(1.0) as u32;
            for s in 0..=steps {
                let t = s as f32 / steps as f32;
                stamp(from.lerp(to, t), y0 + (y1 - y0) * t);
            }
        }
        let area = (tb - ta).perp_dot(tc - ta);
        if area.abs() < 1e-5 {
            continue;
        }
        let (lo, hi) = (ta.min(tb).min(tc).floor(), ta.max(tb).max(tc).ceil());
        for z in lo.y.max(0.0) as usize..(hi.y.max(0.0) as usize).min(size) {
            for x in lo.x.max(0.0) as usize..(hi.x.max(0.0) as usize).min(size) {
                let p = Vec2::new(x as f32 + 0.5, z as f32 + 0.5);
                let u = (p - ta).perp_dot(tc - ta) / area;
                let v = (tb - ta).perp_dot(p - ta) / area;
                if u >= 0.0 && v >= 0.0 && u + v <= 1.0 {
                    stamp(p, a.y + (b.y - a.y) * u + (c.y - a.y) * v);
                }
            }
        }
    }
    map.low = low;
    map
}

/// A body that floats, as written in scene files. Its shape comes from its mesh.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FloatDesc {
    /// How high the hull keeps water out, in the model's own units: the deck, the top of a
    /// rowboat's sides. What is above doesn't count, like masts. `None` is the whole mesh.
    pub deck: Option<f32>,
    /// Size of the pieces the volume is cut into, in meters. `None` is a sixteenth of its length.
    pub cell: Option<f32>,
    /// Kilograms; the more, the lower it floats. `None` comes from `fill`.
    pub mass: Option<f32>,
    /// How much of its volume ends up under water, when `mass` isn't given. Above 1, it sinks.
    pub fill: f32,
    /// How low its weight sits, between the middle of the hull (0) and its bottom (1). The
    /// lower, the better it rights itself.
    pub ballast: f32,
    /// How fast the water slows it per second, along the model's X, Y and Z.
    pub drag: Vec3,
    /// It's open or has rooms in it, like a boat: the water isn't drawn inside it, nor can
    /// anyone swim there, up to `deck` or the top of the mesh.
    pub hollow: bool,
}

impl Default for FloatDesc {
    fn default() -> Self {
        Self {
            deck: None,
            cell: None,
            mass: None,
            fill: 0.45,
            ballast: 0.3,
            drag: Vec3::new(0.5, 1.2, 0.9),
            hollow: false,
        }
    }
}

/// A piece of a floating body's volume, in its local frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FloatCell {
    pub center: Vec3,
    pub half_height: f32,
    pub volume: f32,
    /// Cells of the same column ask about the water once.
    pub column: u32,
}

/// The volume of a floating body, cut into pieces, and what it weighs.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FloatHull {
    pub cells: Vec<FloatCell>,
    pub columns: u32,
    pub volume: f32,
    pub mass: f32,
    pub center_of_mass: Vec3,
    /// Principal moments around the center of mass, for the model's X, Y and Z.
    pub inertia: Vec3,
    pub drag: Vec3,
    /// What keeps the water out, if it's hollow.
    pub map: Option<Arc<HullMap>>,
}

impl FloatHull {
    /// Finds the lowest and highest point of the mesh over each column of a grid, which
    /// leaves a boat's inside full: the water stays out of it, as long as it stays upright.
    pub fn build(desc: &FloatDesc, mesh: &MeshData, scale: Vec3) -> Self {
        const MOST_LAYERS: u32 = 5;
        let scale = scale.abs();
        let deck = desc.deck.map_or(f32::MAX, |deck| deck * scale.y);
        let points: Vec<Vec3> = mesh.vertices.iter().map(|v| Vec3::from(v.position) * scale).collect();
        let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for &p in &points {
            if p.y <= deck {
                min = min.min(p);
                max = max.max(p);
            }
        }
        if min.x > max.x {
            log::warn!("nothing of this mesh is below its deck: it will not float");
            return Self::default();
        }
        let cell = desc.cell.unwrap_or_else(|| (max.x - min.x).max(max.z - min.z) / 16.0).max(0.02);
        let (nx, nz) =
            (((max.x - min.x) / cell).ceil().max(1.0) as usize, ((max.z - min.z) / cell).ceil().max(1.0) as usize);
        let (dx, dz) = ((max.x - min.x) / nx as f32, (max.z - min.z) / nz as f32);
        let mut low = vec![f32::MAX; nx * nz];
        let mut high = vec![f32::MIN; nx * nz];
        for triangle in mesh.indices.as_chunks::<3>().0 {
            let [a, b, c] = triangle.map(|i| points[i as usize]);
            let area = (b.x - a.x) * (c.z - a.z) - (c.x - a.x) * (b.z - a.z);
            if area.abs() < 1e-7 {
                continue;
            }
            let (x0, x1) = (a.x.min(b.x).min(c.x), a.x.max(b.x).max(c.x));
            let (z0, z1) = (a.z.min(b.z).min(c.z), a.z.max(b.z).max(c.z));
            let from_x = (((x0 - min.x) / dx - 0.5).floor().max(0.0)) as usize;
            let from_z = (((z0 - min.z) / dz - 0.5).floor().max(0.0)) as usize;
            for k in from_z..nz {
                let z = min.z + (k as f32 + 0.5) * dz;
                if z > z1 {
                    break;
                }
                for i in from_x..nx {
                    let x = min.x + (i as f32 + 0.5) * dx;
                    if x > x1 {
                        break;
                    }
                    let u = ((x - a.x) * (c.z - a.z) - (c.x - a.x) * (z - a.z)) / area;
                    let v = ((b.x - a.x) * (z - a.z) - (x - a.x) * (b.z - a.z)) / area;
                    if u < -1e-4 || v < -1e-4 || u + v > 1.0001 {
                        continue;
                    }
                    let y = a.y + (b.y - a.y) * u + (c.y - a.y) * v;
                    low[k * nx + i] = low[k * nx + i].min(y);
                    high[k * nx + i] = high[k * nx + i].max(y);
                }
            }
        }
        let mut hull = Self { drag: desc.drag, ..Default::default() };
        for k in 0..nz {
            for i in 0..nx {
                let (bottom, top) = (low[k * nx + i], high[k * nx + i].min(deck));
                if top - bottom < 1e-3 {
                    continue;
                }
                let layers = (((top - bottom) / cell).ceil() as u32).clamp(1, MOST_LAYERS);
                let height = (top - bottom) / layers as f32;
                for layer in 0..layers {
                    let center = Vec3::new(
                        min.x + (i as f32 + 0.5) * dx,
                        bottom + (layer as f32 + 0.5) * height,
                        min.z + (k as f32 + 0.5) * dz,
                    );
                    hull.cells.push(FloatCell {
                        center,
                        half_height: height * 0.5,
                        volume: dx * dz * height,
                        column: hull.columns,
                    });
                }
                hull.columns += 1;
            }
        }
        hull.weigh(desc, min.y);
        if desc.hollow {
            hull.map = Some(Arc::new(hull_map(&points, &mesh.indices, deck, min, max)));
        }
        hull
    }

    /// Mass, center and moments, as if the mass filled the volume evenly, then lowered.
    fn weigh(&mut self, desc: &FloatDesc, bottom: f32) {
        const WATER: f32 = 1000.0;
        self.volume = self.cells.iter().map(|c| c.volume).sum();
        if self.volume <= 0.0 {
            return;
        }
        self.mass = desc.mass.unwrap_or(self.volume * WATER * desc.fill.max(0.001)).max(0.001);
        let middle = self.cells.iter().map(|c| c.center * c.volume).sum::<Vec3>() / self.volume;
        let ballast = desc.ballast.clamp(0.0, 1.0);
        self.center_of_mass = middle.with_y(middle.y + (bottom - middle.y) * ballast);
        let density = self.mass / self.volume;
        let mut inertia = Vec3::ZERO;
        for c in &self.cells {
            let d = c.center - middle;
            let m = c.volume * density;
            inertia += m * Vec3::new(d.y * d.y + d.z * d.z, d.x * d.x + d.z * d.z, d.x * d.x + d.y * d.y);
        }
        self.inertia = inertia.max(Vec3::splat(self.mass * 0.01));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ring_of_the_sea_meets_the_next_without_gaps() {
        let mesh = sea_mesh(&WaterDesc::default(), None, None);
        assert!(mesh.boundless && mesh.shadowless);
        // Every triangle faces up, and no edge is used more than twice.
        let mut edges = std::collections::HashMap::new();
        for t in mesh.indices.as_chunks::<3>().0 {
            let p = t.map(|i| Vec3::from(mesh.vertices[i as usize].position));
            assert!((p[1] - p[0]).cross(p[2] - p[0]).y > 0.0, "{p:?}");
            for k in 0..3 {
                let key = |v: Vec3| ((v.x * 10.0).round() as i32, (v.z * 10.0).round() as i32);
                let (a, b) = (key(p[k]), key(p[(k + 1) % 3]));
                *edges.entry(if a < b { (a, b) } else { (b, a) }).or_insert(0) += 1;
            }
        }
        // Only the outermost edge of all is open, so nothing can be seen through it.
        let open: Vec<_> = edges.iter().filter(|&(_, &n)| n == 1).map(|(k, _)| k).collect();
        let reach = open.iter().map(|((x, z), _)| x.abs().max(z.abs())).min().unwrap();
        assert!(reach as f32 / 10.0 > 1800.0, "an open edge at {reach}");
        assert!(mesh.indices.len() / 3 < 130_000, "{}", mesh.indices.len() / 3);
    }

    #[test]
    fn a_hull_is_cut_into_its_real_volume() {
        // A closed box: 2 × 1 × 4 meters.
        let mut mesh = MeshData::new();
        let corner = |x: f32, y: f32, z: f32| Vec3::new(x, y, z * 2.0);
        for (a, b, c, d) in [
            // Top and bottom, then the sides; which way they face doesn't matter here.
            ((-1, 1, -1), (-1, 1, 1), (1, 1, 1), (1, 1, -1)),
            ((-1, 0, -1), (1, 0, -1), (1, 0, 1), (-1, 0, 1)),
        ] {
            let p = |(x, y, z): (i32, i32, i32)| corner(x as f32, y as f32, z as f32);
            mesh.push_triangle(p(a), p(b), p(c), Vec3::ONE);
            mesh.push_triangle(p(a), p(c), p(d), Vec3::ONE);
        }
        let desc = FloatDesc { cell: Some(0.25), fill: 0.5, ..Default::default() };
        let hull = FloatHull::build(&desc, &mesh, Vec3::ONE);
        assert!((hull.volume - 8.0).abs() < 0.2, "{}", hull.volume);
        assert!((hull.mass - hull.volume * 500.0).abs() < 1.0);
        // Weight sits under the middle, and the moments are longest around the long axes.
        let middle = hull.cells.iter().map(|c| c.center.y * c.volume).sum::<f32>() / hull.volume;
        assert!(hull.center_of_mass.y < middle);
        assert!(hull.inertia.x > hull.inertia.z);

        // Above the deck doesn't count.
        let half = FloatHull::build(&FloatDesc { deck: Some(0.5), cell: Some(0.25), ..desc }, &mesh, Vec3::ONE);
        assert!((half.volume - 4.0).abs() < 0.2, "{}", half.volume);
    }
}
