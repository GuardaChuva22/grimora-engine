use bytemuck::{Pod, Zeroable};
use glam::Vec3;

/// GPU vertex layout shared by every mesh. Colors are sRGB in `0..=1`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 3],
}

/// CPU-side triangle mesh, ready to upload to the renderer.
#[derive(Debug, Clone, Default)]
pub struct MeshData {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl MeshData {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a flat-shaded triangle (counter-clockwise when seen from outside).
    /// Each face gets its own vertices, which gives the faceted low-poly look.
    pub fn push_triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: Vec3) {
        let normal = (b - a).cross(c - a).normalize_or_zero();
        let base = self.vertices.len() as u32;
        for p in [a, b, c] {
            self.vertices.push(Vertex { position: p.to_array(), normal: normal.to_array(), color: color.to_array() });
        }
        self.indices.extend_from_slice(&[base, base + 1, base + 2]);
    }

    /// Adds a flat quad `a b c d` (counter-clockwise).
    pub fn push_quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, color: Vec3) {
        self.push_triangle(a, b, c, color);
        self.push_triangle(a, c, d, color);
    }

    /// Appends another mesh, transformed by `offset` and uniform `scale`.
    pub fn append(&mut self, other: &MeshData, offset: Vec3, scale: Vec3) {
        let base = self.vertices.len() as u32;
        self.vertices.extend(other.vertices.iter().map(|v| Vertex {
            position: (Vec3::from(v.position) * scale + offset).to_array(),
            normal: (Vec3::from(v.normal) / scale).normalize_or_zero().to_array(),
            color: v.color,
        }));
        self.indices.extend(other.indices.iter().map(|i| i + base));
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}
