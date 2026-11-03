use std::ops::Range;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::{Vec3, Vec4};

use crate::animation::Skeleton;
use crate::bounds::BoundingSphere;

/// GPU vertex layout shared by every mesh. Colors are sRGB in `0..=1`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 3],
    pub uv: [f32; 2],
    /// xyz = tangent direction, w = bitangent sign. Only needed with normal maps.
    pub tangent: [f32; 4],
    /// Skinned meshes: up to four joints moving this vertex...
    pub joints: [u8; 4],
    /// ...and how much each one counts (0..=255, summing to 255). All zero = not skinned.
    pub weights: [u8; 4],
}

impl Vertex {
    /// An untextured vertex (no UV, no tangent).
    pub fn new(position: Vec3, normal: Vec3, color: Vec3) -> Self {
        Self {
            position: position.to_array(),
            normal: normal.to_array(),
            color: color.to_array(),
            uv: [0.0; 2],
            tangent: [0.0; 4],
            joints: [0; 4],
            weights: [0; 4],
        }
    }
}

/// Decoded RGBA8 image used by a material.
#[derive(Clone)]
pub struct TextureData {
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes, row by row.
    pub rgba: Vec<u8>,
}

impl TextureData {
    /// Half-size copy using a 2×2 box filter (odd edges repeat the last texel).
    /// Used for mipmaps and to shrink oversized textures.
    pub fn half(&self) -> Self {
        let (w, h) = (self.width as usize, self.height as usize);
        let (hw, hh) = ((w / 2).max(1), (h / 2).max(1));
        let mut rgba = Vec::with_capacity(hw * hh * 4);
        for y in 0..hh {
            let (y0, y1) = ((2 * y).min(h - 1), (2 * y + 1).min(h - 1));
            for x in 0..hw {
                let (x0, x1) = ((2 * x).min(w - 1), (2 * x + 1).min(w - 1));
                for c in 0..4 {
                    let texel = |x: usize, y: usize| u32::from(self.rgba[(y * w + x) * 4 + c]);
                    let sum = texel(x0, y0) + texel(x1, y0) + texel(x0, y1) + texel(x1, y1);
                    rgba.push(((sum + 2) / 4) as u8);
                }
            }
        }
        Self { name: self.name.clone(), width: hw as u32, height: hh as u32, rgba }
    }
}

impl std::fmt::Debug for TextureData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TextureData({:?}, {}x{})", self.name, self.width, self.height)
    }
}

/// How a material uses its alpha (base color alpha × texture alpha).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum AlphaMode {
    /// Alpha is ignored.
    #[default]
    Opaque,
    /// Cut out where alpha is below `cutoff`: foliage, torn cloth, fences.
    /// Edges are smoothed with alpha-to-coverage under MSAA.
    Mask { cutoff: f32 },
    /// See-through surfaces, really blended with what is behind them: drawn after
    /// everything opaque, from the farthest to the nearest object. Inside one
    /// object nothing is sorted, so keep them simple shapes.
    Blend,
}

/// How a surface is shaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shading {
    /// Lit like any solid thing.
    #[default]
    Standard,
    /// Lit from above on both faces, as if the sprites were part of the ground cover.
    Foliage,
    /// Waves on the vertices (`MeshData::wind` is their height), scrolling
    /// texture and the sky reflected at low angles. Use with `AlphaMode::Blend`.
    Water,
}

/// Surface appearance, modeled on glTF's metallic-roughness materials.
///
/// The defaults describe a plain matte surface, which is what procedural
/// low-poly meshes use: their color comes from vertex colors.
#[derive(Debug, Clone)]
pub struct Material {
    pub name: Option<String>,
    /// Linear RGBA multiplier for the base color texture (glTF convention).
    pub base_color: Vec4,
    /// 0 = dielectric (stone, wood, cloth), 1 = metal.
    pub metallic: f32,
    /// 0 = mirror, 1 = fully rough.
    pub roughness: f32,
    pub normal_scale: f32,
    pub occlusion_strength: f32,
    /// Render both faces of each triangle (thin geometry like leaves or cloth).
    pub double_sided: bool,
    pub alpha_mode: AlphaMode,
    pub shading: Shading,
    /// Keeps the texels sharp up close instead of blurring them: pixel art.
    pub pixelated: bool,
    /// Linear light emitted by the surface, multiplied by `emissive_texture`.
    /// Unaffected by lighting; values above 1 feed the bloom.
    pub emissive: Vec3,
    /// sRGB color.
    pub base_color_texture: Option<Arc<TextureData>>,
    /// Tangent-space normal map.
    pub normal_texture: Option<Arc<TextureData>>,
    /// glTF packing: green = roughness, blue = metallic.
    pub metallic_roughness_texture: Option<Arc<TextureData>>,
    /// Red channel = ambient occlusion.
    pub occlusion_texture: Option<Arc<TextureData>>,
    /// sRGB emission color.
    pub emissive_texture: Option<Arc<TextureData>>,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            name: None,
            base_color: Vec4::ONE,
            metallic: 0.0,
            roughness: 1.0,
            normal_scale: 1.0,
            occlusion_strength: 1.0,
            double_sided: false,
            alpha_mode: AlphaMode::Opaque,
            shading: Shading::Standard,
            pixelated: false,
            emissive: Vec3::ZERO,
            base_color_texture: None,
            normal_texture: None,
            metallic_roughness_texture: None,
            occlusion_texture: None,
            emissive_texture: None,
        }
    }
}

/// A range of a mesh's indices drawn with one material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submesh {
    pub indices: Range<u32>,
    /// Index into [`MeshData::materials`].
    pub material: usize,
}

/// CPU-side triangle mesh, ready to upload to the renderer.
///
/// Without `submeshes`, the whole mesh is drawn with the default material.
#[derive(Debug, Clone, Default)]
pub struct MeshData {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub materials: Vec<Material>,
    pub submeshes: Vec<Submesh>,
    /// Present on animated meshes; vertices then carry joints and weights.
    pub skeleton: Option<Arc<Skeleton>>,
    /// Simpler versions drawn when the object looks small, from most to least
    /// detailed (see [`MeshLod`]).
    pub lods: Vec<MeshLod>,
    /// How much the wind moves it: meters at one unit above its origin, growing
    /// with the square of the height, so the base stays put. 0 stands still.
    pub wind: f32,
    /// Casts no shadow: grass that would cost more than it shows, or water.
    pub shadowless: bool,
}

/// A lower-detail stand-in for a mesh.
#[derive(Debug, Clone, Default)]
pub struct MeshLod {
    /// Used once the object's bounding sphere covers less than this fraction of
    /// the screen height (0.05 = 5%, about 36 pixels at 720p).
    pub screen_size: f32,
    pub mesh: MeshData,
}

impl MeshData {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sphere around every vertex in the bind pose.
    pub fn bounding_sphere(&self) -> BoundingSphere {
        BoundingSphere::from_points(self.vertices.iter().map(|v| Vec3::from(v.position)))
    }

    /// Adds lower-detail versions, each `(screen_size, mesh)` (see [`MeshLod`]).
    pub fn with_lods(mut self, lods: impl IntoIterator<Item = (f32, MeshData)>) -> Self {
        self.lods.extend(lods.into_iter().map(|(screen_size, mesh)| MeshLod { screen_size, mesh }));
        self.lods.sort_by(|a, b| b.screen_size.total_cmp(&a.screen_size));
        self
    }

    /// Adds a flat-shaded triangle (counter-clockwise when seen from outside).
    /// Each face gets its own vertices, which gives the faceted low-poly look.
    pub fn push_triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: Vec3) {
        let normal = (b - a).cross(c - a).normalize_or_zero();
        let base = self.vertices.len() as u32;
        for p in [a, b, c] {
            self.vertices.push(Vertex::new(p, normal, color));
        }
        self.indices.extend_from_slice(&[base, base + 1, base + 2]);
    }

    /// Adds a flat quad `a b c d` (counter-clockwise).
    pub fn push_quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, color: Vec3) {
        self.push_triangle(a, b, c, color);
        self.push_triangle(a, c, d, color);
    }

    /// Appends another mesh's geometry, transformed by `offset` and `scale`.
    /// Its materials are ignored: the geometry joins this mesh's.
    pub fn append(&mut self, other: &MeshData, offset: Vec3, scale: Vec3) {
        let base = self.vertices.len() as u32;
        self.vertices.extend(other.vertices.iter().map(|v| Vertex {
            position: (Vec3::from(v.position) * scale + offset).to_array(),
            normal: (Vec3::from(v.normal) / scale).normalize_or_zero().to_array(),
            ..*v
        }));
        self.indices.extend(other.indices.iter().map(|i| i + base));
    }

    /// Submeshes to draw, with a single default-material submesh standing in
    /// when none were set. Pair with [`MeshData::material`].
    pub fn draw_ranges(&self) -> Vec<Submesh> {
        if self.submeshes.is_empty() {
            vec![Submesh { indices: 0..self.indices.len() as u32, material: 0 }]
        } else {
            self.submeshes.clone()
        }
    }

    /// The material at `index`, or the default material.
    pub fn material(&self, index: usize) -> Material {
        self.materials.get(index).cloned().unwrap_or_default()
    }

    /// Axis-aligned bounds `(min, max)`, or `None` for an empty mesh.
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut points = self.vertices.iter().map(|v| Vec3::from(v.position));
        let first = points.next()?;
        Some(points.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}
