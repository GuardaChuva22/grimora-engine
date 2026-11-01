use grimora_core::{MeshData, Vec3};
use serde::{Deserialize, Serialize};

use crate::rng::{Rng, fbm};

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
        Self { desc, heights, cell }
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
    pub fn height_at(&self, x: f32, z: f32) -> f32 {
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
    flat_h + (h - flat_h) * t
}

#[cfg(test)]
mod tests {
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
}
