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
    /// Half-size copy using a 2Ã—2 box filter (odd edges repeat the last texel).
    /// Used for mipmaps and to shrink oversized textures.
    pub fn half(&self) -> Self {
        let (w, h) = (self.width as usize, self.height as usize);
        let (hw, hh) = ((w / 2).max(1), (h / 2).max(1));
        let mut rgba = Vec::with_capacity(hw * hh * 4);
        for y in 0..hh {
            let (y0, y1) = ((2 * y).min(h - 1), (2 * y + 1).min(h - 1));
            for x in 0..hw {
                let (x0, x1) = ((2 * x).min(w - 1), (2 * x + 1).min(w - 1));
                let texels = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)].map(|(x, y)| (y * w + x) * 4);
                let alpha: u32 = texels.iter().map(|&t| u32::from(self.rgba[t + 3])).sum();
                for c in 0..3 {
                    // What is see-through has no say in the color, which would fringe cutouts.
                    let weighted = |t: usize| {
                        let weight = if alpha == 0 { 1 } else { u32::from(self.rgba[t + 3]) };
                        (u32::from(self.rgba[t + c]) * weight, weight)
                    };
                    let (sum, total) = texels.iter().fold((0, 0), |(sum, total), &t| {
                        let (value, weight) = weighted(t);
                        (sum + value, total + weight)
                    });
                    rgba.push(((sum + total / 2) / total) as u8);
                }
                rgba.push(((alpha + 2) / 4) as u8);
            }
        }
        Self { name: self.name.clone(), width: hw as u32, height: hh as u32, rgba }
    }

    /// Gives fully transparent texels the color of what is drawn beside them.
    /// Nothing shows them with hard texels, but a filter blends them into the
    /// edge of a cutout, which would get a dark outline.
    pub fn fill_under_alpha(&mut self) {
        if !self.rgba.as_chunks::<4>().0.iter().any(|texel| texel[3] == 0) {
            return;
        }
        let (w, h) = (self.width as usize, self.height as usize);
        let source = self.rgba.clone();
        for y in 0..h {
            for x in 0..w {
                let texel = (y * w + x) * 4;
                if source[texel + 3] != 0 {
                    continue;
                }
                let (mut sum, mut total) = ([0u32; 3], 0u32);
                for ny in [y + h - 1, y, y + 1] {
                    for nx in [x + w - 1, x, x + 1] {
                        let near = (ny % h * w + nx % w) * 4;
                        let weight = u32::from(source[near + 3]);
                        for (c, value) in sum.iter_mut().enumerate() {
                            *value += u32::from(source[near + c]) * weight;
                        }
                        total += weight;
                    }
                }
                for (c, value) in sum.iter().enumerate() {
                    if let Some(mean) = (value + total / 2).checked_div(total) {
                        self.rgba[texel + c] = mean as u8;
                    }
                }
            }
        }
    }
}

impl std::fmt::Debug for TextureData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TextureData({:?}, {}x{})", self.name, self.width, self.height)
    }
}

/// How a material uses its alpha (base color alpha Ã— texture alpha).
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
    /// Glows from within, with depth: clouds and veins seen through its faces, moving.
    /// `Renderable::emissive` says how awake it is.
    Crystal,
    /// Generated ground: grass (the base picture, tinted by the vertex color), sand (the
    /// emissive picture) and rock (the occlusion picture) blended by the weights in the
    /// vertex's `tangent.xyz`, with ragged borders and two scales of each picture against the
    /// repeats. See `ground_texel` in scene.wgsl.
    Ground,
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
    /// Pixel art: follows the player's choice between filtered and hard texels
    /// (`Renderer::set_smooth_textures`). Without this, always filtered.
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
    /// sRGB emission color. On water, the picture of its foam instead.
    pub emissive_texture: Option<Arc<TextureData>>,
    /// With `Shading::Water`: what it looks like.
    pub water: Option<crate::water::WaterLook>,
    /// Water: meters one repeat of its picture covers.
    pub water_tile: f32,
    /// A river: how fast its picture moves along its uv, per second.
    pub water_flow: glam::Vec2,
    /// 1 on a river, whose uv runs along it.
    pub water_river: f32,
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
            water: None,
            water_tile: 3.0,
            water_flow: glam::Vec2::ZERO,
            water_river: 0.0,
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
    /// Follows the camera around, so it is never out of view: a sea.
    pub boundless: bool,
    /// What bumps into it, when its file gives a shape of its own (a model's `*_COL`
    /// nodes): the trunk of a tree, not its crown. Never drawn.
    pub collision: Option<Arc<CollisionMesh>>,
}

/// Welded triangles for a static collider, in the mesh's own space.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CollisionMesh {
    pub vertices: Vec<Vec3>,
    pub indices: Vec<[u32; 3]>,
}

impl CollisionMesh {
    /// The triangles of `mesh`, with the vertices that sit on the same spot joined.
    pub fn from_mesh(mesh: &MeshData) -> Self {
        let mut vertices = Vec::new();
        let mut lookup = std::collections::HashMap::new();
        let mut remap = |p: [f32; 3]| {
            *lookup.entry(p.map(f32::to_bits)).or_insert_with(|| {
                vertices.push(Vec3::from(p));
                vertices.len() as u32 - 1
            })
        };
        let welded: Vec<u32> = mesh.indices.iter().map(|&i| remap(mesh.vertices[i as usize].position)).collect();
        let indices = welded.as_chunks::<3>().0.iter().map(|t| [t[0], t[1], t[2]]).collect();
        Self { vertices, indices }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mipmaps_ignore_the_color_of_what_is_see_through() {
        let rgba = vec![200, 100, 50, 255, 0, 0, 0, 0, 0, 0, 0, 0, 200, 100, 50, 255];
        let half = TextureData { name: "cutout".into(), width: 2, height: 2, rgba }.half();
        assert_eq!(half.rgba, [200, 100, 50, 128]);

        let rgba = vec![90, 90, 90, 0, 30, 30, 30, 0, 30, 30, 30, 0, 90, 90, 90, 0];
        let half = TextureData { name: "empty".into(), width: 2, height: 2, rgba }.half();
        assert_eq!(half.rgba, [60, 60, 60, 0]);
    }

    #[test]
    fn transparent_texels_take_the_color_beside_them() {
        let rgba = vec![200, 100, 50, 255, 0, 0, 0, 0];
        let mut cutout = TextureData { name: "cutout".into(), width: 2, height: 1, rgba };
        cutout.fill_under_alpha();
        assert_eq!(cutout.rgba, [200, 100, 50, 255, 200, 100, 50, 0]);

        let rgba = vec![9, 9, 9, 0, 7, 7, 7, 0];
        let mut empty = TextureData { name: "empty".into(), width: 2, height: 1, rgba: rgba.clone() };
        empty.fill_under_alpha();
        assert_eq!(empty.rgba, rgba, "nothing drawn, nothing to copy");
    }
}
