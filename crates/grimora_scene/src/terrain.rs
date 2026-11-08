use grimora_core::{Material, MeshData, RiverPoint, Submesh, Vec2, Vec3, Vertex};
use serde::{Deserialize, Serialize};

use crate::ground::{Ground, TerrainGrounds};
use crate::rng::{Rng, fbm};
use crate::texture::Picture;

/// Meters between the cross sections of a path.
const PATH_STEP: f32 = 1.5;

/// A square heightfield terrain centered on the origin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerrainDesc {
    /// Side length in world units.
    pub size: f32,
    /// Number of cells per side. Low values give the chunky low-poly look.
    pub resolution: u32,
    /// Maximum height of the rolling noise.
    pub height: f32,
    /// Noise features per 100 world units.
    pub frequency: f32,
    pub seed: u32,
    /// Extra height added towards the borders, turning the map into a valley.
    pub edge_rise: f32,
    /// Radius around the origin that is flattened (for a village, a camp...).
    pub flat_radius: f32,
    pub grass_color: Vec3,
    pub dirt_color: Vec3,
    pub rock_color: Vec3,
    /// Pictures for the ground instead of plain colors.
    pub textures: Option<TerrainTextures>,
    /// Holes in the ground to fill with `water`. Scattered things stay out of them.
    pub ponds: Vec<Pond>,
    /// High ground with steep sides. A river that runs off one makes a waterfall.
    pub plateaus: Vec<Plateau>,
    /// River beds dug into the ground; the scene's `rivers` fill this in.
    #[serde(skip)]
    pub carves: Vec<Carve>,
    /// Trails laid over the ground. Scattered things stay off them too.
    pub paths: Vec<TerrainPath>,
    /// What a step sounds like on each layer (flat, slope, rock), say sand on a beach.
    pub grounds: TerrainGrounds,
}

/// A trail following the ground. It is only a picture: the terrain under it is what
/// holds things up.
/// A river's bed: it lowers the ground along a course to under the water's level.
#[derive(Debug, Clone, PartialEq)]
pub struct Carve {
    pub points: Vec<RiverPoint>,
    pub depth: f32,
}

impl Carve {
    fn dig(&self, ground: f32, x: f32, z: f32) -> f32 {
        let p = Vec2::new(x, z);
        let mut result = ground;
        for pair in self.points.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            let from = Vec2::new(a.position.x, a.position.z);
            let along = Vec2::new(b.position.x, b.position.z) - from;
            let t = ((p - from).dot(along) / along.length_squared().max(1e-5)).clamp(0.0, 1.0);
            let gap = (from + along * t).distance(p);
            let half = a.half_width.max(0.1);
            let bed = a.position.y + (b.position.y - a.position.y) * t - self.depth;
            let keep = smooth(half * 0.6, half * 1.5, gap);
            result = result.min(bed.min(ground) + (ground - bed.min(ground)) * keep);
        }
        result
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerrainPath {
    /// `(x, z)` points it curves through.
    pub points: Vec<Vec2>,
    /// In meters. The edges wander around it, so it looks worn and not drawn with a ruler.
    pub width: f32,
    /// Image file, relative to the scene file.
    pub texture: String,
    /// Meters one repeat of the image covers.
    pub tile: f32,
    /// What a step on it sounds like.
    pub ground: Ground,
}

impl Default for TerrainPath {
    fn default() -> Self {
        Self { points: Vec::new(), width: 3.0, texture: String::new(), tile: 3.0, ground: Ground::Dirt }
    }
}

impl TerrainPath {
    /// The curve through `points`, about `step` meters between each two points of it.
    fn curve(&self, step: f32) -> Vec<Vec2> {
        let points = &self.points;
        let at = |i: isize| points[i.clamp(0, points.len() as isize - 1) as usize];
        let mut curve = Vec::new();
        for i in 0..points.len() as isize - 1 {
            let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
            let steps = (p1.distance(p2) / step).ceil().max(1.0) as u32;
            for k in 0..steps {
                // Catmull-Rom passes through every point.
                let t = k as f32 / steps as f32;
                curve.push(
                    0.5 * (p1 * 2.0
                        + (p2 - p0) * t
                        + (p0 * 2.0 - p1 * 5.0 + p2 * 4.0 - p3) * t * t
                        + (p1 * 3.0 - p0 - p2 * 3.0 + p3) * t * t * t),
                );
            }
        }
        curve.extend(points.last());
        curve
    }
}

/// One picture per kind of ground, relative to the scene file; the slope and
/// height of each face choose it, as they choose the colors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerrainTextures {
    pub grass: String,
    pub dirt: String,
    pub rock: String,
    /// Meters one repeat of a picture covers.
    pub tile: f32,
}

impl Default for TerrainTextures {
    fn default() -> Self {
        Self { grass: String::new(), dirt: String::new(), rock: String::new(), tile: 4.0 }
    }
}

/// A round dip in the terrain.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pond {
    pub x: f32,
    pub z: f32,
    pub radius: f32,
    /// How far down the middle goes.
    pub depth: f32,
}

/// High ground with a steep side all around, a little uneven.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Plateau {
    pub x: f32,
    pub z: f32,
    /// How far its flat top reaches.
    pub radius: f32,
    pub height: f32,
    /// How far on the ground its side takes to come down; the lower, the steeper.
    pub edge: f32,
}

impl Default for Plateau {
    fn default() -> Self {
        Self { x: 0.0, z: 0.0, radius: 30.0, height: 12.0, edge: 4.0 }
    }
}

impl Default for Pond {
    fn default() -> Self {
        Self { x: 0.0, z: 0.0, radius: 8.0, depth: 1.5 }
    }
}

impl Pond {
    fn contains(&self, x: f32, z: f32) -> bool {
        (x - self.x).hypot(z - self.z) < self.radius
    }
}

impl Default for TerrainDesc {
    fn default() -> Self {
        Self {
            size: 200.0,
            resolution: 80,
            height: 8.0,
            frequency: 2.5,
            seed: 1,
            edge_rise: 20.0,
            flat_radius: 12.0,
            grass_color: Vec3::new(0.22, 0.26, 0.16),
            dirt_color: Vec3::new(0.28, 0.22, 0.17),
            rock_color: Vec3::new(0.32, 0.31, 0.33),
            textures: None,
            ponds: Vec::new(),
            plateaus: Vec::new(),
            carves: Vec::new(),
            paths: Vec::new(),
            grounds: TerrainGrounds::default(),
        }
    }
}

/// Generated terrain. Keeps the grid heights so gameplay code can query
/// the exact surface height that is rendered.
#[derive(Debug, Clone)]
pub struct Terrain {
    pub desc: TerrainDesc,
    heights: Vec<f32>,
    cell: f32,
    /// How far the world's origin has moved it: the queries take local positions, and the
    /// heights they give are local too. See [`Terrain::shift`].
    shifted: Vec3,
}

impl Terrain {
    pub fn new(desc: TerrainDesc) -> Self {
        let n = desc.resolution.max(1) as usize + 1;
        let cell = desc.size / desc.resolution.max(1) as f32;
        let half = desc.size * 0.5;
        let mut heights = Vec::with_capacity(n * n);
        for j in 0..n {
            for i in 0..n {
                let x = i as f32 * cell - half;
                let z = j as f32 * cell - half;
                heights.push(sample_height(&desc, x, z));
            }
        }
        Self { desc, heights, cell, shifted: Vec3::ZERO }
    }

    fn grid(&self, i: usize, j: usize) -> f32 {
        let n = self.desc.resolution as usize + 1;
        self.heights[j.min(n - 1) * n + i.min(n - 1)]
    }

    fn corner(&self, i: usize, j: usize) -> Vec3 {
        let half = self.desc.size * 0.5;
        Vec3::new(i as f32 * self.cell - half, self.grid(i, j), j as f32 * self.cell - half)
    }

    /// Height of the rendered surface at world `(x, z)`. Clamped at the borders.
    /// How long each side is, in meters.
    pub fn size(&self) -> f32 {
        self.desc.size
    }

    /// Moves the terrain by `offset` for its queries, as the world's origin moves the other
    /// way. Its meshes are entities, which move by themselves.
    pub fn shift(&mut self, offset: Vec3) {
        self.shifted += offset;
    }

    pub fn height_at(&self, x: f32, z: f32) -> f32 {
        let (x, z) = (x - self.shifted.x, z - self.shifted.z);
        self.shifted.y + self.height_unshifted(x, z)
    }

    fn height_unshifted(&self, x: f32, z: f32) -> f32 {
        let half = self.desc.size * 0.5;
        let res = self.desc.resolution as f32;
        let gx = ((x + half) / self.cell).clamp(0.0, res - 1e-3);
        let gz = ((z + half) / self.cell).clamp(0.0, res - 1e-3);
        let (i, j) = (gx.floor() as usize, gz.floor() as usize);
        let (fx, fz) = (gx.fract(), gz.fract());
        let (h00, h10, h01, h11) = (self.grid(i, j), self.grid(i + 1, j), self.grid(i, j + 1), self.grid(i + 1, j + 1));
        // Same diagonal split as `mesh()`: triangles (00, 01, 11) and (00, 11, 10).
        if fz >= fx { h00 + (h11 - h01) * fx + (h01 - h00) * fz } else { h00 + (h10 - h00) * fx + (h11 - h10) * fz }
    }

    /// Faceted terrain mesh, colored by slope and height.
    pub fn mesh(&self) -> MeshData {
        let res = self.desc.resolution as usize;
        let mut mesh = MeshData::new();
        let mut rng = Rng::new(u64::from(self.desc.seed) + 77);
        for j in 0..res {
            for i in 0..res {
                let p00 = self.corner(i, j);
                let p10 = self.corner(i + 1, j);
                let p01 = self.corner(i, j + 1);
                let p11 = self.corner(i + 1, j + 1);
                for (a, b, c) in [(p00, p01, p11), (p00, p11, p10)] {
                    let color = self.face_color(a, b, c, &mut rng);
                    mesh.push_triangle(a, b, c, color);
                }
            }
        }
        mesh
    }

    /// [`Terrain::mesh`] cut into square chunks of `quads` × `quads` grid cells
    /// (smaller at the far edges), so each chunk can be culled on its own. The
    /// triangles and colors are exactly those of the whole mesh.
    pub fn chunk_meshes(&self, quads: usize) -> Vec<MeshData> {
        let res = self.desc.resolution as usize;
        let quads = quads.max(1);
        let per_side = res.div_ceil(quads);
        let whole = self.mesh();
        let mut chunks = vec![MeshData::new(); per_side * per_side];
        // `mesh()` emits two triangles (six vertices) per grid cell, row by row.
        for (cell, vertices) in whole.vertices.as_chunks::<6>().0.iter().enumerate() {
            let (i, j) = (cell % res, cell / res);
            let chunk = &mut chunks[(j / quads) * per_side + i / quads];
            let base = chunk.vertices.len() as u32;
            chunk.vertices.extend_from_slice(vertices);
            chunk.indices.extend(base..base + 6);
        }
        chunks
    }

    /// [`Terrain::chunk_meshes`] with `materials` (grass, dirt, rock) instead of
    /// colors, whose pictures repeat every `tile` meters. Vertices stay in the
    /// same order; each chunk draws one group of faces per material.
    pub fn textured_chunks(&self, quads: usize, materials: [Material; 3], tile: f32) -> Vec<MeshData> {
        let tile = tile.max(0.01);
        let mut rng = Rng::new(u64::from(self.desc.seed) + 78);
        let mut chunks = self.chunk_meshes(quads);
        for chunk in &mut chunks {
            let mut layers: [Vec<u32>; 3] = Default::default();
            for (face, vertices) in chunk.vertices.as_chunks_mut::<3>().0.iter_mut().enumerate() {
                let [a, b, c] = [0, 1, 2].map(|i| Vec3::from(vertices[i].position));
                let brightness = 1.0 + rng.jitter(0.07);
                for vertex in vertices.iter_mut() {
                    vertex.uv = [vertex.position[0] / tile, vertex.position[2] / tile];
                    vertex.color = [brightness; 3];
                }
                let first = face as u32 * 3;
                layers[self.face_layer(a, b, c)].extend(first..first + 3);
            }
            chunk.materials = materials.to_vec();
            chunk.indices.clear();
            chunk.submeshes.clear();
            for (material, layer) in layers.iter().enumerate().filter(|(_, layer)| !layer.is_empty()) {
                let start = chunk.indices.len() as u32;
                chunk.indices.extend_from_slice(layer);
                chunk.submeshes.push(Submesh { indices: start..chunk.indices.len() as u32, material });
            }
        }
        chunks
    }

    /// Whether `(x, z)` is inside a pond.
    pub fn in_pond(&self, x: f32, z: f32) -> bool {
        let (x, z) = (x - self.shifted.x, z - self.shifted.z);
        self.desc.ponds.iter().any(|pond| pond.contains(x, z))
    }

    /// Whether `(x, z)` is on a path, or less than `margin` meters away from one.
    pub fn on_path(&self, x: f32, z: f32, margin: f32) -> bool {
        let point = Vec2::new(x - self.shifted.x, z - self.shifted.z);
        self.desc.paths.iter().any(|path| {
            let reach = path.width * 0.5 + margin;
            path.curve(PATH_STEP).iter().any(|p| p.distance_squared(point) < reach * reach)
        })
    }

    /// What the ground is made of at world `(x, z)`: a path's, or the layer the
    /// face there is drawn with.
    pub fn ground_at(&self, x: f32, z: f32) -> Ground {
        let (x, z) = (x - self.shifted.x, z - self.shifted.z);
        let point = Vec2::new(x, z);
        let path = self.desc.paths.iter().find(|path| {
            let reach = path.width * 0.5;
            path.curve(PATH_STEP).iter().any(|p| p.distance_squared(point) < reach * reach)
        });
        if let Some(path) = path {
            return path.ground;
        }
        let half = self.desc.size * 0.5;
        let res = self.desc.resolution as f32;
        let gx = ((x + half) / self.cell).clamp(0.0, res - 1e-3);
        let gz = ((z + half) / self.cell).clamp(0.0, res - 1e-3);
        let (i, j) = (gx.floor() as usize, gz.floor() as usize);
        let (p00, p10, p01, p11) =
            (self.corner(i, j), self.corner(i + 1, j), self.corner(i, j + 1), self.corner(i + 1, j + 1));
        // Same diagonal split as `mesh()`.
        let layer =
            if gz.fract() >= gx.fract() { self.face_layer(p00, p01, p11) } else { self.face_layer(p00, p11, p10) };
        let grounds = &self.desc.grounds;
        [grounds.grass, grounds.dirt, grounds.rock][layer]
    }

    /// The mesh of a path: a strip resting on the ground, with `picture` running along it.
    pub(crate) fn path_mesh(&self, path: &TerrainPath, picture: &Picture, seed: u64) -> MeshData {
        const ACROSS: u32 = 4;
        const LIFT: f32 = 0.07;
        let curve = path.curve(PATH_STEP);
        let tile = path.tile.max(0.01);
        let half = path.width * 0.5;
        let mut rng = Rng::new(seed);
        let mut mesh = MeshData::new();
        let mut travelled = 0.0;
        for (i, &point) in curve.iter().enumerate() {
            let (before, after) = (curve[i.saturating_sub(1)], curve[(i + 1).min(curve.len() - 1)]);
            let side = (after - before).normalize_or(Vec2::X).perp();
            if i > 0 {
                travelled += point.distance(before);
            }
            let (left, right) = (half * (1.0 + rng.jitter(0.28)), half * (1.0 + rng.jitter(0.28)));
            for k in 0..=ACROSS {
                let f = k as f32 / ACROSS as f32;
                let offset = -left + (left + right) * f;
                let at = point + side * offset;
                let position = Vec3::new(at.x, self.height_at(at.x, at.y) + LIFT, at.y);
                let mut vertex = Vertex::new(position, Vec3::Y, Vec3::ONE);
                vertex.uv = [offset / tile, travelled / tile];
                mesh.vertices.push(vertex);
            }
        }
        let row = ACROSS + 1;
        for i in 0..curve.len().saturating_sub(1) as u32 {
            for k in 0..ACROSS {
                let (a, b) = (i * row + k, (i + 1) * row + k);
                mesh.indices.extend_from_slice(&[a, a + 1, b + 1, a, b + 1, b]);
            }
        }
        mesh.materials = vec![Material { double_sided: true, ..picture.material() }];
        mesh.shadowless = true;
        mesh
    }

    /// Which picture a face gets: 0 grass, 1 dirt, 2 rock.
    fn face_layer(&self, a: Vec3, b: Vec3, c: Vec3) -> usize {
        let normal = (b - a).cross(c - a).normalize_or(Vec3::Y);
        let slope = 1.0 - normal.y;
        let height = (a.y + b.y + c.y) / 3.0;
        if slope > 0.2 || height > self.desc.height + 13.0 {
            2
        } else if slope > 0.08 {
            1
        } else {
            0
        }
    }

    fn face_color(&self, a: Vec3, b: Vec3, c: Vec3, rng: &mut Rng) -> Vec3 {
        let normal = (b - a).cross(c - a).normalize_or(Vec3::Y);
        let slope = 1.0 - normal.y;
        let height = (a.y + b.y + c.y) / 3.0;
        let d = &self.desc;
        let mut color = d.grass_color.lerp(d.dirt_color, smooth(0.04, 0.12, slope));
        color = color.lerp(d.rock_color, smooth(0.15, 0.3, slope).max(smooth(d.height + 8.0, d.height + 18.0, height)));
        color * (1.0 + rng.jitter(0.06))
    }
}

fn smooth(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn sample_height(desc: &TerrainDesc, x: f32, z: f32) -> f32 {
    let f = desc.frequency / 100.0;
    let mut h = fbm(x * f, z * f, desc.seed, 5) * desc.height;

    // Rise towards the borders so the playable area sits in a valley.
    let half = desc.size * 0.5;
    let edge = (x.abs().max(z.abs()) / half).clamp(0.0, 1.0);
    h += smooth(0.55, 1.0, edge) * desc.edge_rise * (0.6 + 0.8 * fbm(x * f * 2.0, z * f * 2.0, desc.seed + 9, 3));

    // Blend towards a flat clearing at the center.
    let r = (x * x + z * z).sqrt();
    let flat_h = fbm(0.0, 0.0, desc.seed, 5) * desc.height;
    let t = smooth(desc.flat_radius, desc.flat_radius * 2.0, r);
    let mut ground = flat_h + (h - flat_h) * t;

    for plateau in &desc.plateaus {
        let wander = (fbm(x * 0.03, z * 0.03, desc.seed + 21, 3) - 0.5) * plateau.radius * 0.35;
        let d = (x - plateau.x).hypot(z - plateau.z) + wander;
        ground += plateau.height * (1.0 - smooth(plateau.radius, plateau.radius + plateau.edge.max(0.1), d));
    }
    for pond in &desc.ponds {
        let d = (x - pond.x).hypot(z - pond.z);
        ground -= pond.depth * (1.0 - smooth(pond.radius * 0.45, pond.radius, d));
    }
    for carve in &desc.carves {
        ground = carve.dig(ground, x, z);
    }
    ground
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use grimora_core::TextureData;

    use super::*;

    #[test]
    fn height_matches_grid_corners() {
        let terrain = Terrain::new(TerrainDesc { resolution: 16, size: 64.0, ..Default::default() });
        let p = terrain.corner(5, 7);
        assert!((terrain.height_at(p.x, p.z) - p.y).abs() < 1e-3);
    }

    #[test]
    fn mesh_has_two_triangles_per_cell() {
        let terrain = Terrain::new(TerrainDesc { resolution: 8, ..Default::default() });
        assert_eq!(terrain.mesh().triangle_count(), 8 * 8 * 2);
    }

    #[test]
    fn chunks_split_the_mesh_exactly() {
        let terrain = Terrain::new(TerrainDesc { resolution: 10, size: 40.0, ..Default::default() });
        let chunks = terrain.chunk_meshes(4);
        // 10 cells per side in chunks of 4: 3 × 3 chunks, the last row and column 2 cells wide.
        assert_eq!(chunks.len(), 9);
        assert_eq!(chunks[0].triangle_count(), 4 * 4 * 2);
        assert_eq!(chunks[8].triangle_count(), 2 * 2 * 2);
        let mut whole: Vec<_> = terrain.mesh().vertices.iter().map(|v| format!("{v:?}")).collect();
        let mut split: Vec<_> = chunks.iter().flat_map(|c| &c.vertices).map(|v| format!("{v:?}")).collect();
        whole.sort();
        split.sort();
        assert_eq!(whole, split);
        // Chunk (1, 0) covers x in 16..32 of the -20..20 grid, z in -20..-4.
        let bounds = chunks[1].bounding_sphere();
        assert!(bounds.center.x > -4.0 && bounds.center.x < 12.0 && bounds.center.z < -4.0, "{bounds:?}");
    }

    #[test]
    fn textured_chunks_keep_the_shape_and_group_faces_by_picture() {
        let material = || Material { pixelated: true, ..Default::default() };
        let desc = TerrainDesc { resolution: 10, size: 40.0, height: 30.0, ..Default::default() };
        let terrain = Terrain::new(desc);
        let chunks = terrain.textured_chunks(4, [material(), material(), material()], 2.0);
        let plain = terrain.chunk_meshes(4);
        let mut layers_used = [false; 3];
        for (textured, plain) in chunks.iter().zip(&plain) {
            assert_eq!(textured.vertices.len(), plain.vertices.len());
            assert_eq!(textured.indices.len(), plain.indices.len());
            for (a, b) in textured.vertices.iter().zip(&plain.vertices) {
                assert_eq!(a.position, b.position, "the collider and the ground stay the same");
                assert_eq!(a.uv, [a.position[0] / 2.0, a.position[2] / 2.0]);
            }
            assert_eq!(textured.materials.len(), 3);
            let drawn: u32 = textured.submeshes.iter().map(|s| s.indices.len() as u32).sum();
            assert_eq!(drawn as usize, textured.indices.len());
            for sub in &textured.submeshes {
                layers_used[sub.material] = true;
                assert!(textured.material(sub.material).pixelated);
            }
        }
        assert_eq!(layers_used, [true, true, true], "this much relief has all three");
    }

    #[test]
    fn paths_follow_the_ground_and_keep_things_off() {
        let path = TerrainPath {
            points: vec![Vec2::new(0.0, 0.0), Vec2::new(10.0, 4.0), Vec2::new(20.0, 0.0)],
            width: 2.0,
            ..Default::default()
        };
        let curve = path.curve(PATH_STEP);
        assert_eq!(curve.first(), path.points.first());
        assert_eq!(curve.last(), path.points.last());
        assert!(curve.iter().any(|p| p.distance(path.points[1]) < 1e-3), "it goes through every point");

        let terrain = Terrain::new(TerrainDesc { paths: vec![path.clone()], ..Default::default() });
        assert!(terrain.on_path(10.0, 4.0, 0.0) && !terrain.on_path(10.0, 8.0, 0.0));
        assert!(terrain.on_path(10.0, 8.0, 5.0), "with a margin, what is beside it counts too");

        let picture = Picture {
            texture: Arc::new(TextureData { name: "p".into(), width: 1, height: 1, rgba: vec![255; 4] }),
            emission: None,
        };
        let mesh = terrain.path_mesh(&path, &picture, 1);
        for v in &mesh.vertices {
            let [x, y, z] = v.position;
            assert!((y - terrain.height_at(x, z)).abs() < 0.2, "the strip rests on the ground");
        }
        assert!(mesh.indices.iter().all(|&i| (i as usize) < mesh.vertices.len()));
        assert!(!mesh.indices.is_empty() && mesh.shadowless);
    }

    #[test]
    fn ponds_dig_into_the_ground() {
        let base = TerrainDesc { resolution: 64, size: 64.0, flat_radius: 30.0, edge_rise: 0.0, ..Default::default() };
        let pond = Pond { x: 8.0, z: -4.0, radius: 6.0, depth: 2.0 };
        let dug = Terrain::new(TerrainDesc { ponds: vec![pond], ..base.clone() });
        let flat = Terrain::new(base);
        assert!((flat.height_at(8.0, -4.0) - dug.height_at(8.0, -4.0) - 2.0).abs() < 1e-3);
        assert!((flat.height_at(-20.0, 10.0) - dug.height_at(-20.0, 10.0)).abs() < 1e-3, "away from it, nothing");
        assert!(dug.in_pond(10.0, -4.0) && !dug.in_pond(15.0, -4.0));
    }
}
