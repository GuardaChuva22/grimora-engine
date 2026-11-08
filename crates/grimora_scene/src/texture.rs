//! Pictures for scenes: textured objects, sprites and water. Loaded images are
//! cached by file path and modification time, so every object that names one
//! shares the same pixels.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use anyhow::{Context as _, Result};
use grimora_core::{AlphaMode, Material, MeshData, SeaDesc, Shading, TextureData, Vec2, Vec3, Vec4, Vertex, WaterLook};
use serde::{Deserialize, Serialize};

/// A picture repeating over an object's surface (`texture:` on objects).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextureDesc {
    /// Image file, relative to the scene file.
    pub path: String,
    /// Meters one repeat of the image covers, whatever the object's size.
    pub tile: f32,
}

impl Default for TextureDesc {
    fn default() -> Self {
        Self { path: String::new(), tile: 2.0 }
    }
}

/// A sheet of water (`water:` in the scene file): no collider and no shadow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WaterDesc {
    /// Center of the surface. With a terrain, `y` is measured from the ground there.
    pub position: Vec3,
    /// Width (x) and depth (z). The terrain hides what is beyond the shore.
    pub size: Vec2,
    /// Image file, relative to the scene file; the wind drifts it.
    pub texture: Option<String>,
    /// sRGB tint.
    pub color: Vec3,
    /// See-through from 0 to 1, looking straight down; shallow angles mirror the sky.
    pub opacity: f32,
    /// Meters one repeat of the image covers.
    pub tile: f32,
    /// Height of the waves in meters.
    pub waves: f32,
    /// sRGB color of the light it gives off by itself, following the picture's ripples.
    pub glow: Vec3,
    /// Makes it a sea: endless, around the camera, with these waves. `size` no longer counts.
    pub sea: Option<SeaDesc>,
    /// Its colors and how clear it is. Without one, it comes from `color` and `opacity`.
    pub look: Option<WaterLook>,
    /// Image of its foam, relative to the scene file. Noise without one.
    pub foam_texture: Option<String>,
}

impl WaterDesc {
    /// The look of plain water, from `color` and `opacity`: this opaque at about a meter.
    pub fn plain_look(&self) -> WaterLook {
        let through = 1.0 - self.opacity.clamp(0.0, 0.995);
        WaterLook {
            deep: self.color,
            shallow: self.color,
            clarity: 1.5 / -through.ln().min(-0.01),
            murk: 0.0,
            refraction: 0.35,
            foam: 0.7,
            caustics: 0.5,
            ..Default::default()
        }
    }
}

impl Default for WaterDesc {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            size: Vec2::splat(10.0),
            texture: None,
            color: Vec3::ONE,
            opacity: 0.75,
            tile: 3.0,
            waves: 0.05,
            glow: Vec3::ZERO,
            sea: None,
            look: None,
            foam_texture: None,
        }
    }
}

type Cache = HashMap<PathBuf, (Option<SystemTime>, Arc<TextureData>)>;

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Loads an image file. Reuses the cached copy while the file is unchanged.
pub fn load_texture(path: impl AsRef<Path>) -> Result<Arc<TextureData>> {
    let path = path.as_ref();
    let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    if let Some((cached, texture)) = cache().lock().unwrap().get(path)
        && *cached == mtime
    {
        return Ok(texture.clone());
    }
    let image = image::open(path).with_context(|| format!("loading texture {}", path.display()))?.to_rgba8();
    let texture = Arc::new(TextureData {
        name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    });
    cache().lock().unwrap().insert(path.to_path_buf(), (mtime, texture.clone()));
    Ok(texture)
}

/// Like [`load_texture`] for scene contents: one that is missing or broken is
/// logged, and the scene goes on without it.
pub(crate) fn texture_or_log(path: &Path) -> Option<Arc<TextureData>> {
    load_texture(path).inspect_err(|err| log::error!("{err:#}")).ok()
}

/// How strongly an `_emission` map glows: past 1, so that its brightest parts bloom.
const EMISSION_GLOW: f32 = 2.0;

/// An image and what glows in it.
#[derive(Clone)]
pub(crate) struct Picture {
    pub texture: Arc<TextureData>,
    pub emission: Option<Arc<TextureData>>,
}

impl Picture {
    /// Loads an image and, if there is one beside it, `<name>_emission.png`.
    pub fn load(path: &Path) -> Option<Self> {
        let texture = texture_or_log(path)?;
        let emission = path.file_stem().and_then(|stem| {
            let file = format!("{}_emission.png", stem.to_string_lossy());
            let path = path.with_file_name(file);
            path.exists().then(|| texture_or_log(&path)).flatten()
        });
        Some(Self { texture, emission })
    }

    /// Shows the image as pixel art, cut out where it is transparent.
    pub fn material(&self) -> Material {
        let cut_out = self.texture.rgba.as_chunks::<4>().0.iter().any(|texel| texel[3] < 128);
        Material {
            base_color_texture: Some(self.texture.clone()),
            pixelated: true,
            double_sided: cut_out,
            alpha_mode: if cut_out { AlphaMode::Mask { cutoff: 0.5 } } else { AlphaMode::Opaque },
            emissive: if self.emission.is_some() { Vec3::splat(EMISSION_GLOW) } else { Vec3::ZERO },
            emissive_texture: self.emission.clone(),
            ..Default::default()
        }
    }
}

/// Puts `texture` on a mesh, the image projected from whichever side each face
/// looks at. `scale` is the object's, so repeats keep their size in the world.
pub(crate) fn apply_texture(mesh: &mut MeshData, picture: &Picture, tile: f32, scale: Vec3) {
    let tile = tile.max(0.01);
    for vertex in &mut mesh.vertices {
        let p = Vec3::from(vertex.position) * scale;
        let n = Vec3::from(vertex.normal).abs();
        let uv = if n.y >= n.x && n.y >= n.z {
            Vec2::new(p.x, p.z)
        } else if n.x >= n.z {
            Vec2::new(p.z, -p.y)
        } else {
            Vec2::new(p.x, -p.y)
        };
        vertex.uv = (uv / tile).to_array();
    }
    mesh.materials = vec![picture.material()];
    mesh.submeshes.clear();
    for lod in &mut mesh.lods {
        apply_texture(&mut lod.mesh, picture, tile, scale);
    }
}

/// Wraps `mesh` in the image at `path`, as `ObjectDesc::texture` does, for a mesh the game makes
/// at run time. Without the image, the mesh stays as it was.
pub fn wrap_in_texture(mesh: &mut MeshData, path: &Path, tile: f32) {
    if let Some(picture) = Picture::load(path) {
        apply_texture(mesh, &picture, tile, Vec3::ONE);
    }
}

/// The part of an image that is not fully transparent, as `(u0, v0, u1, v1)`.
pub(crate) fn drawn_area(texture: &TextureData) -> (f32, f32, f32, f32) {
    let (w, h) = (texture.width as usize, texture.height as usize);
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if texture.rgba[(y * w + x) * 4 + 3] > 0 {
                (x0, y0) = (x0.min(x), y0.min(y));
                (x1, y1) = (x1.max(x + 1), y1.max(y + 1));
            }
        }
    }
    if x0 >= x1 {
        return (0.0, 0.0, 1.0, 1.0);
    }
    (x0 as f32 / w as f32, y0 as f32 / h as f32, x1 as f32 / w as f32, y1 as f32 / h as f32)
}

/// Crossed upright pictures with their base at the origin: plants seen from any
/// side. `size` is the height of the drawing itself (empty margins don't count),
/// and it sways `sway` meters at the top. Single flat picture far away.
pub(crate) fn sprite_mesh(picture: &Picture, size: f32, planes: u32, sway: f32, shadow: bool) -> MeshData {
    let texture = &picture.texture;
    let level = |planes: u32| {
        let (u0, v0, u1, v1) = drawn_area(texture);
        let aspect = (u1 - u0) * texture.width as f32 / ((v1 - v0) * texture.height as f32).max(1.0);
        let half = size * aspect * 0.5;
        let mut mesh = MeshData::new();
        for plane in 0..planes.max(1) {
            let angle = plane as f32 / planes.max(1) as f32 * std::f32::consts::PI;
            let side = Vec3::new(angle.cos(), 0.0, angle.sin()) * half;
            let base = mesh.vertices.len() as u32;
            for (position, u, v) in
                [(-side, u0, v1), (side, u1, v1), (side + Vec3::Y * size, u1, v0), (-side + Vec3::Y * size, u0, v0)]
            {
                // Facing up: lit like the ground it grows from, from any side.
                let mut vertex = Vertex::new(position, Vec3::Y, Vec3::ONE);
                vertex.uv = [u, v];
                mesh.vertices.push(vertex);
            }
            mesh.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        mesh.materials = vec![Material {
            double_sided: true,
            alpha_mode: AlphaMode::Mask { cutoff: 0.5 },
            shading: Shading::Foliage,
            ..picture.material()
        }];
        mesh.wind = sway / (size * size).max(1e-3);
        mesh.shadowless = !shadow;
        mesh
    };
    let far = level(1);
    level(planes).with_lods([(0.03, far)])
}

/// A picture lying flat, `size` meters wide, centered on the origin and barely
/// above it: blood, moss, cracks. Fits flat ground; on a slope, part of it sinks.
/// Turn the object to put it on a wall.
pub(crate) fn decal_mesh(picture: &Picture, size: f32) -> MeshData {
    const LIFT: f32 = 0.025;
    let half = size * 0.5;
    let mut mesh = MeshData::new();
    for (x, z, u, v) in
        [(-half, -half, 0.0, 0.0), (-half, half, 0.0, 1.0), (half, half, 1.0, 1.0), (half, -half, 1.0, 0.0)]
    {
        let mut vertex = Vertex::new(Vec3::new(x, LIFT, z), Vec3::Y, Vec3::ONE);
        vertex.uv = [u, v];
        mesh.vertices.push(vertex);
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    mesh.materials = vec![Material { alpha_mode: AlphaMode::Mask { cutoff: 0.5 }, ..picture.material() }];
    mesh.shadowless = true;
    mesh
}

/// A grid of about one quad per meter and a half, for the waves to move. A very
/// large sheet, like a sea, gets larger quads, with `MAX_CELLS` of them per side.
pub fn water_mesh(desc: &WaterDesc, texture: Option<Arc<TextureData>>, foam: Option<Arc<TextureData>>) -> MeshData {
    const CELL: f32 = 1.5;
    const MAX_CELLS: f32 = 256.0;
    let size = desc.size.max(Vec2::splat(0.1));
    let cells = |side: f32| (side / CELL).ceil().clamp(1.0, MAX_CELLS) as u32;
    let (nx, nz) = (cells(size.x), cells(size.y));
    // Read by `vs_water`: how much to stretch the waves for the grid to carry them.
    let waves = (CELL * nx as f32 / size.x).min(CELL * nz as f32 / size.y).min(1.0);
    let tile = desc.tile.max(0.01);
    let mut mesh = MeshData::new();
    for j in 0..=nz {
        for i in 0..=nx {
            let (fx, fz) = (i as f32 / nx as f32, j as f32 / nz as f32);
            let (x, z) = ((fx - 0.5) * size.x, (fz - 0.5) * size.y);
            let mut vertex = Vertex::new(Vec3::new(x, 0.0, z), Vec3::new(waves, 1.0, 0.0), Vec3::ONE);
            vertex.uv = [x / tile, z / tile];
            mesh.vertices.push(vertex);
        }
    }
    let row = nx + 1;
    for j in 0..nz {
        for i in 0..nx {
            let (a, b, c, d) = (j * row + i, (j + 1) * row + i, (j + 1) * row + i + 1, j * row + i + 1);
            mesh.indices.extend_from_slice(&[a, b, c, a, c, d]);
        }
    }
    let [r, g, b] = desc.color.to_array().map(srgb_to_linear);
    mesh.materials = vec![Material {
        name: Some("water".into()),
        base_color: Vec4::new(r, g, b, desc.opacity.clamp(0.0, 1.0)),
        emissive: Vec3::from_array(desc.glow.to_array().map(srgb_to_linear)),
        roughness: 0.2,
        alpha_mode: AlphaMode::Blend,
        shading: Shading::Water,
        pixelated: true,
        base_color_texture: texture,
        emissive_texture: foam,
        water: Some(desc.look.unwrap_or_else(|| desc.plain_look())),
        water_tile: tile,
        ..Default::default()
    }];
    mesh.wind = desc.waves.max(0.0);
    mesh.shadowless = true;
    mesh
}

pub(crate) fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 4×4, with only the two middle texels of the bottom row drawn.
    fn plant() -> Picture {
        let mut rgba = vec![0; 4 * 4 * 4];
        for texel in [13, 14] {
            rgba[texel * 4 + 3] = 255;
        }
        Picture { texture: Arc::new(TextureData { name: "plant".into(), width: 4, height: 4, rgba }), emission: None }
    }

    #[test]
    fn sprites_stand_on_the_base_of_the_drawing() {
        let mesh = sprite_mesh(&plant(), 2.0, 2, 0.2, false);
        assert_eq!(mesh.triangle_count(), 4);
        assert_eq!(mesh.lods[0].mesh.triangle_count(), 2, "one picture from afar");
        let (min, max) = mesh.bounds().unwrap();
        assert_eq!((min.y, max.y), (0.0, 2.0));
        assert!((max.x - 2.0).abs() < 1e-4, "2 texels wide for 1 tall: {max}");
        assert_eq!(mesh.vertices[0].uv, [0.25, 1.0], "cut to the drawing, with no empty margin");
        assert_eq!(mesh.vertices[2].uv, [0.75, 0.75]);
        assert!(mesh.vertices.iter().all(|v| v.normal == [0.0, 1.0, 0.0]));
        assert!((mesh.wind * 4.0 - 0.2).abs() < 1e-5, "sways 0.2 at the top");
        assert!(mesh.shadowless);
    }

    #[test]
    fn textures_keep_their_size_on_scaled_objects() {
        let mut mesh = MeshData::new();
        mesh.push_quad(Vec3::ZERO, Vec3::Y, Vec3::new(1.0, 1.0, 0.0), Vec3::X, Vec3::ONE);
        apply_texture(&mut mesh, &plant(), 2.0, Vec3::new(4.0, 6.0, 1.0));
        let far = mesh.vertices.iter().map(|v| Vec2::from(v.uv).abs()).fold(Vec2::ZERO, Vec2::max);
        assert_eq!(far, Vec2::new(2.0, 3.0), "4 m by 6 m at 2 m per repeat");
        assert!(mesh.material(0).pixelated && mesh.material(0).base_color_texture.is_some());
    }

    #[test]
    fn decals_lie_flat_and_cast_no_shadow() {
        let mesh = decal_mesh(&plant(), 3.0);
        let (min, max) = mesh.bounds().unwrap();
        assert_eq!((min.x, max.x, min.z, max.z), (-1.5, 1.5, -1.5, 1.5));
        assert!(min.y > 0.0 && min.y == max.y, "just above the ground");
        let [a, b, c] = [0, 1, 2].map(|i| Vec3::from(mesh.vertices[mesh.indices[i] as usize].position));
        assert!((b - a).cross(c - a).y > 0.0, "facing up");
        assert!(mesh.shadowless);
        assert!(matches!(mesh.material(0).alpha_mode, AlphaMode::Mask { .. }));
    }

    #[test]
    fn pictures_find_their_emission_and_cut_out_their_background() {
        let dir = std::env::temp_dir().join(format!("grimora_picture_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255])).save(dir.join("wall.png")).unwrap();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 0])).save(dir.join("web.png")).unwrap();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([9, 0, 0, 255])).save(dir.join("web_emission.png")).unwrap();

        let wall = Picture::load(&dir.join("wall.png")).unwrap().material();
        assert_eq!(wall.alpha_mode, AlphaMode::Opaque);
        assert!(wall.emissive_texture.is_none() && wall.emissive == Vec3::ZERO && !wall.double_sided);

        let web = Picture::load(&dir.join("web.png")).unwrap().material();
        assert!(matches!(web.alpha_mode, AlphaMode::Mask { .. }) && web.double_sided);
        assert!(web.emissive_texture.is_some() && web.emissive.x > 1.0);
        assert!(web.pixelated);

        assert!(Picture::load(&dir.join("missing.png")).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn water_faces_up_and_moves() {
        let mesh = water_mesh(&WaterDesc { size: Vec2::new(6.0, 3.0), waves: 0.1, ..Default::default() }, None, None);
        assert_eq!(mesh.triangle_count(), 4 * 2 * 2);
        for tri in mesh.indices.chunks(3) {
            let [a, b, c] = [0, 1, 2].map(|i| Vec3::from(mesh.vertices[tri[i] as usize].position));
            assert!((b - a).cross(c - a).y > 0.0, "counter-clockwise from above");
        }
        assert_eq!(mesh.wind, 0.1);
        assert_eq!(mesh.material(0).shading, Shading::Water);
        assert_eq!(mesh.material(0).alpha_mode, AlphaMode::Blend);
        assert!(mesh.shadowless);
    }

    #[test]
    fn loading_caches_and_notices_missing_files() {
        let dir = std::env::temp_dir().join(format!("grimora_texture_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.png");
        image::RgbaImage::from_pixel(2, 3, image::Rgba([1, 2, 3, 4])).save(&path).unwrap();
        let first = load_texture(&path).unwrap();
        assert_eq!((first.width, first.height, &first.rgba[..4]), (2, 3, &[1, 2, 3, 4][..]));
        assert!(Arc::ptr_eq(&first, &load_texture(&path).unwrap()));
        assert!(load_texture(dir.join("missing.png")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
