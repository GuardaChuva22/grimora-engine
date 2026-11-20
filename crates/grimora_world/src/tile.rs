//! The ground in square tiles of a quadtree: each level's tiles are twice as wide as the one
//! below and have as many cells, so a tile far away is as cheap as one under the feet. Keys are
//! integers on a grid fixed in the world (`i64`), so they mean the same wherever the origin is,
//! and a ring of them can be streamed in and out.

use std::collections::HashMap;

use glam::{DVec2, DVec3, Vec3, Vec4};
use grimora_core::{Material, MeshData, Shading, Submesh, Vertex};

use crate::Relief;

/// Cells along each side of a tile from level 2 up.
pub const TILE_CELLS: usize = 32;
/// Cells along each side of a level 0 and level 1 tile: 1 m cells under the feet, fine enough
/// for a brook's bed and a smooth shore, and 2 m cells around them.
pub const NEAR_CELLS: usize = 64;
/// Width of a level 0 tile, in meters.
pub const BASE_TILE: f64 = 64.0;
/// The coarsest level: 64 m × 2⁹ = 32.8 km tiles.
pub const TOP_LEVEL: u8 = 9;
/// A tile splits into four when the camera is closer than this many of its widths.
pub const SPLIT: f64 = 1.6;
/// The same for a level 1 tile into the finest ones, which are only right around the camera.
pub const FINE_SPLIT: f64 = 0.75;
/// Ground with a face this steep (normal's y below it) is rock.
const ROCK_SLOPE: f32 = 0.74;
/// From this level on, the sea floor is pushed down by `FLOOR_DROP` meters, twice as much
/// each level up.
const FLOOR_DROP_FROM: u8 = 3;
const FLOOR_DROP: f32 = 10.0;
/// Below this height the ground is sand: beaches and the sea floor.
const SAND_BELOW: f32 = 3.5;
/// Meters under the sea at which the sand starts to give way to silt, and where it is all silt.
const SILT_FROM: f32 = 14.0;
const SILT_FULL: f32 = 24.0;
/// Meters past the water over which a bed fades into the ground around it.
const BED_FADE: f64 = 4.0;
/// Between these heights a bed gives way to sand, toward the sea.
const BED_TO_SAND: (f32, f32) = (-0.6, 2.0);

/// One tile: its level and where it is on that level's grid. Its north-west corner is at
/// `x * size`, `z * size` meters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileKey {
    pub level: u8,
    pub x: i64,
    pub z: i64,
}

impl TileKey {
    pub fn size(self) -> f64 {
        BASE_TILE * f64::from(1u32 << self.level)
    }

    /// The north-west corner, in world meters.
    pub fn min(self) -> DVec2 {
        DVec2::new(self.x as f64, self.z as f64) * self.size()
    }

    pub fn center(self) -> DVec2 {
        self.min() + DVec2::splat(self.size() * 0.5)
    }

    /// Cells along each side.
    pub fn cells(self) -> usize {
        if self.level <= 1 { NEAR_CELLS } else { TILE_CELLS }
    }

    pub fn cell(self) -> f64 {
        self.size() / self.cells() as f64
    }

    /// The level 0 tile under a point.
    pub fn at(point: DVec2) -> Self {
        Self { level: 0, x: (point.x / BASE_TILE).floor() as i64, z: (point.y / BASE_TILE).floor() as i64 }
    }

    pub fn children(self) -> [TileKey; 4] {
        let (x, z, level) = (self.x * 2, self.z * 2, self.level - 1);
        [(0, 0), (1, 0), (0, 1), (1, 1)].map(|(i, j)| TileKey { level, x: x + i, z: z + j })
    }

    /// Horizontal distance from a point to the tile, 0 inside it.
    pub fn distance(self, point: DVec2) -> f64 {
        let min = self.min();
        let max = min + DVec2::splat(self.size());
        point.clamp(min, max).distance(point)
    }
}

/// The tiles to draw for a camera at `eye` (world meters) standing `above` meters over the
/// ground, out to `view` meters: finer ones near it and coarser ones away, covering everything
/// in reach once, with no overlaps. Sorted, so the same view always gives the same list.
pub fn tiles_around(eye: DVec3, above: f64, view: f64) -> Vec<TileKey> {
    let top = BASE_TILE * f64::from(1u32 << TOP_LEVEL);
    let at = DVec2::new(eye.x, eye.z);
    let lo = ((at - DVec2::splat(view)) / top).floor();
    let hi = ((at + DVec2::splat(view)) / top).floor();
    let mut out = Vec::new();
    let mut stack = Vec::new();
    for z in lo.y as i64..=hi.y as i64 {
        for x in lo.x as i64..=hi.x as i64 {
            stack.push(TileKey { level: TOP_LEVEL, x, z });
        }
    }
    let above = above.max(0.0);
    while let Some(key) = stack.pop() {
        let flat = key.distance(at);
        if flat > view {
            continue;
        }
        let distance = (flat * flat + above * above).sqrt();
        let split = if key.level == 1 { FINE_SPLIT } else { SPLIT };
        if key.level > 0 && distance < split * key.size() {
            stack.extend(key.children());
        } else {
            out.push(key);
        }
    }
    out.sort();
    out
}

/// The ground's three pictures, in the order the tile's submeshes use them.
#[derive(Debug, Clone, Default)]
pub struct TileLook {
    pub grass: Material,
    pub sand: Material,
    pub rock: Material,
    /// The beds of streams and ponds: pebbles, and mud near the sea.
    pub bed: Material,
    pub mud: Material,
    /// Meters one repeat of a picture covers.
    pub tile: f64,
}

/// The heights of a tile's grid, `(cells + 1)²`, row by row from the north-west corner:
/// [`Relief::height_seen`], which is [`Relief::height_at`] on a level 0 tile.
pub fn tile_heights(relief: &Relief, key: TileKey) -> Vec<f32> {
    let (min, cell) = (key.min(), key.cell());
    let cells = key.cells();
    let n = cells + 1;
    let mut heights = Vec::with_capacity(n * n);
    for j in 0..n {
        for i in 0..n {
            heights.push(relief.height_seen(min.x + i as f64 * cell, min.y + j as f64 * cell, cell) as f32);
        }
    }
    heights
}

/// The tile's mesh, around its center at height 0: one grid with smooth normals, each vertex
/// weighing grass, sand and rock by its height, its slope and the water's beds for the
/// `Shading::Ground` material, and a skirt hanging from each edge so a coarser neighbor never
/// leaves a crack.
pub fn build_tile(relief: &Relief, key: TileKey, look: &TileLook) -> MeshData {
    let cells = key.cells();
    let n = cells + 1;
    let (min, cell, half) = (key.min(), key.cell(), key.size() * 0.5);
    // One more row and column on every side, for the normals at the edges.
    let side = n + 2;
    let mut heights = Vec::with_capacity(side * side);
    for j in 0..side {
        for i in 0..side {
            let (x, z) = (min.x + (i as f64 - 1.0) * cell, min.y + (j as f64 - 1.0) * cell);
            heights.push(relief.height_seen(x, z, cell) as f32);
        }
    }
    // Far off, the depth buffer can't tell a shallow sea floor from the sea over it. Nobody
    // sees that far into the water, so far tiles drop their floor out of the way.
    let real = heights.clone();
    if key.level >= FLOOR_DROP_FROM {
        let drop = FLOOR_DROP * f64::from(1u32 << (key.level - FLOOR_DROP_FROM)) as f32;
        for h in heights.iter_mut().filter(|h| **h < 0.0) {
            let t = (-*h / 3.0).min(1.0);
            *h -= drop * t * t * (3.0 - 2.0 * t);
        }
    }
    let height = |i: usize, j: usize| heights[(j + 1) * side + i + 1];
    let around = |i: usize, j: usize| heights[j * side + i];
    let local =
        |i: usize, j: usize| Vec3::new((i as f64 * cell - half) as f32, height(i, j), (j as f64 * cell - half) as f32);
    // The pictures repeat every `tile` meters: start from where the tile sits in that repeat,
    // so neighbors line up and far tiles don't lose `f32` precision.
    let repeat = look.tile.max(0.01);
    let span = repeat * 256.0;
    let uv_origin = DVec2::new(min.x.rem_euclid(span), min.y.rem_euclid(span));
    let uv = |p: Vec3| {
        let u = (uv_origin.x + f64::from(p.x) + half) / repeat;
        let v = (uv_origin.y + f64::from(p.z) + half) / repeat;
        [u as f32, v as f32]
    };
    let mut tints = HashMap::new();
    let mut mesh = MeshData::new();
    for j in 0..n {
        for i in 0..n {
            let p = local(i, j);
            let normal = Vec3::new(
                around(i, j + 1) - around(i + 2, j + 1),
                2.0 * cell as f32,
                around(i + 1, j) - around(i + 1, j + 2),
            )
            .normalize();
            let world = DVec2::new(min.x + i as f64 * cell, min.y + j as f64 * cell);
            let tint = *tints
                .entry(Relief::biome_cell(world.x, world.y))
                .or_insert_with(|| relief.biome_tint_at(world.x, world.y));
            let mut vertex = Vertex::new(p, normal, tint * relief.wash(world.x, world.y));
            vertex.uv = uv(p);
            // By the floor's own depth, not where a far tile drops it to.
            let depth = real[(j + 1) * side + i + 1];
            let mut weights = ground_weights(normal, depth, bed_share(relief, world, depth));
            // The village's trail is bare sand, under any water.
            if let Some(village) = relief.village() {
                let trail = village.trail_share(world, cell) * (1.0 - weights.w);
                weights = weights.lerp(Vec4::new(0.0, 1.0, 0.0, weights.w), trail);
            }
            vertex.tangent = weights.to_array();
            mesh.vertices.push(vertex);
        }
    }
    let at = |i: usize, j: usize| (j * n + i) as u32;
    for j in 0..cells {
        for i in 0..cells {
            let (a, b, c, d) = (at(i, j), at(i, j + 1), at(i + 1, j + 1), at(i + 1, j));
            mesh.indices.extend_from_slice(&[a, b, c, a, c, d]);
        }
    }
    // Skirts: lit as if they faced up, so through a crack they look like the ground beside it.
    let drop = (cell * 1.5 + 4.0) as f32 + key.level.saturating_sub(1) as f32 * 6.0;
    let edges: [Vec<(usize, usize)>; 4] = [
        (0..n).map(|i| (i, 0)).collect(),
        (0..n).map(|i| (i, cells)).collect(),
        (0..n).map(|j| (0, j)).collect(),
        (0..n).map(|j| (cells, j)).collect(),
    ];
    for edge in &edges {
        for pair in edge.windows(2) {
            let first = mesh.vertices.len() as u32;
            for &(i, j) in pair {
                let top = mesh.vertices[at(i, j) as usize];
                let mut hanging = top;
                hanging.position[1] -= drop;
                for mut v in [top, hanging] {
                    v.normal = [0.0, 1.0, 0.0];
                    mesh.vertices.push(v);
                }
            }
            // p, pd, q, qd; both sides, since which one faces out depends on the edge.
            let (p, pd, q, qd) = (first, first + 1, first + 2, first + 3);
            mesh.indices.extend_from_slice(&[p, pd, qd, p, qd, q, p, qd, pd, p, q, qd]);
        }
    }
    mesh.materials = vec![Material {
        name: Some("ground".into()),
        base_color_texture: look.grass.base_color_texture.clone(),
        emissive_texture: look.sand.base_color_texture.clone(),
        occlusion_texture: look.rock.base_color_texture.clone(),
        metallic_roughness_texture: look.bed.base_color_texture.clone(),
        normal_texture: look.mud.base_color_texture.clone(),
        occlusion_strength: 0.0,
        shading: Shading::Ground,
        pixelated: true,
        // Meters a picture repeats in: `ground_texel` draws the rock seen from the side by it.
        water_tile: look.tile as f32,
        ..Default::default()
    }];
    mesh.submeshes = vec![Submesh { indices: 0..mesh.indices.len() as u32, material: 0 }];
    mesh
}

/// How much of grass, sand and rock a point of the ground is, by its slope, its height and
/// whether fresh water lies over it. The shader bites into the borders between them.
/// How much of a point is the fresh water's bed: all of it under the water, less over a few
/// meters past it, and none down by the sea, where the bed gives way to the beach's sand.
fn bed_share(relief: &Relief, at: DVec2, height: f32) -> f32 {
    let near = if relief.is_bed(at.x, at.y) {
        1.0
    } else {
        let past = relief.water().fresh_distance(at.x, at.y);
        (1.0 - past / BED_FADE).clamp(0.0, 1.0)
    } as f32;
    let t = ((height - BED_TO_SAND.0) / (BED_TO_SAND.1 - BED_TO_SAND.0)).clamp(0.0, 1.0);
    near * near * t * t * (3.0 - 2.0 * t)
}

fn ground_weights(normal: Vec3, height: f32, bed: f32) -> Vec4 {
    let smooth = |e0: f32, e1: f32, x: f32| {
        let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let rock = smooth(ROCK_SLOPE + 0.1, ROCK_SLOPE - 0.06, normal.y);
    let sand = smooth(SAND_BELOW + 1.2, SAND_BELOW - 0.6, height);
    let land = Vec4::new((1.0 - sand) * (1.0 - rock), sand * (1.0 - rock), rock, 0.0);
    let wet_rock = rock * 0.25;
    let ground = land.lerp(Vec4::new(0.0, 0.0, wet_rock, 1.0 - wet_rock), bed);
    // Deep down the sand gives out, and the open sea's floor is silt, with the rock showing.
    let silt = smooth(SILT_FROM, SILT_FULL, -height);
    let bare = rock * 0.8;
    ground.lerp(Vec4::new(0.0, 0.0, bare, 1.0 - bare), silt)
}

/// What a step at world `(x, z)` lands on, by the same weights that pick the pictures of the
/// finest tile there: grass, the beach's sand, rock, the stream's bed (pebbles, mud by the
/// sea), the open sea's silt, and the village's trail and streets, which are bare earth.
pub fn ground_at(relief: &Relief, x: f64, z: f64) -> grimora_scene::Ground {
    use grimora_scene::Ground;
    const STEP: f64 = 1.0;
    let h = |x: f64, z: f64| relief.height_at(x, z) as f32;
    let normal =
        Vec3::new(h(x - STEP, z) - h(x + STEP, z), 2.0 * STEP as f32, h(x, z - STEP) - h(x, z + STEP)).normalize();
    let height = h(x, z);
    let at = DVec2::new(x, z);
    let weights = ground_weights(normal, height, bed_share(relief, at, height));
    if let Some(village) = relief.village()
        && village.trail_share(at, STEP) * (1.0 - weights.w) > 0.5
    {
        return Ground::Dirt;
    }
    let most = weights.max_element();
    if most == weights.x {
        Ground::Grass
    } else if most == weights.y {
        Ground::Sand
    } else if most == weights.z {
        Ground::Stone
    } else {
        // The shader's `muddy`: mud down by the sea, pebbles up the streams.
        let muddy = ((6.0 - height) / 4.0).clamp(0.0, 1.0);
        if muddy > 0.5 { Ground::Mud } else { Ground::Stone }
    }
}

/// A collider for a tile: its grid as triangles, around its center, without skirts.
pub fn tile_collider(relief: &Relief, key: TileKey) -> (Vec<Vec3>, Vec<[u32; 3]>) {
    let heights = tile_heights(relief, key);
    let cells = key.cells();
    let n = cells + 1;
    let (cell, half) = (key.cell(), key.size() * 0.5);
    let mut vertices = Vec::with_capacity(n * n);
    for j in 0..n {
        for i in 0..n {
            vertices.push(Vec3::new(
                (i as f64 * cell - half) as f32,
                heights[j * n + i],
                (j as f64 * cell - half) as f32,
            ));
        }
    }
    let index = |i: usize, j: usize| (j * n + i) as u32;
    let mut triangles = Vec::with_capacity(cells * cells * 2);
    for j in 0..cells {
        for i in 0..cells {
            let (a, b, c, d) = (index(i, j), index(i, j + 1), index(i + 1, j + 1), index(i + 1, j));
            triangles.push([a, b, c]);
            triangles.push([a, c, d]);
        }
    }
    (vertices, triangles)
}

/// A digest of a mesh's vertices and indices, to check that a tile comes out the same.
pub fn mesh_hash(mesh: &MeshData) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut eat = |bytes: &[u8]| {
        for &b in bytes {
            h = (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3);
        }
    };
    for v in &mesh.vertices {
        eat(&float_bytes(&v.position));
        eat(&float_bytes(&v.normal));
        eat(&float_bytes(&v.color));
        eat(&float_bytes(&v.uv));
    }
    for i in &mesh.indices {
        eat(&i.to_le_bytes());
    }
    for s in &mesh.submeshes {
        eat(&s.indices.start.to_le_bytes());
        eat(&(s.material as u32).to_le_bytes());
    }
    h
}

fn float_bytes(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_bits().to_le_bytes()).collect()
}
