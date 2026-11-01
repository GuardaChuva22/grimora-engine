//! glTF 2.0 model import (`.gltf` + `.bin` + images, or a single `.glb`).
//!
//! The whole default scene is baked into one [`MeshData`]: node transforms are
//! applied to the vertices and every primitive becomes a submesh with its own
//! material. Loaded models are cached by file path and modification time, so
//! hot-reloading a scene doesn't decode the same textures again.
//!
//! The first skin becomes the mesh's [`Skeleton`], with every animation that
//! targets its joints. Meshes parented under a joint (a sword in a hand) are
//! bound rigidly to that joint so they follow it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use anyhow::{Context as _, Result, bail};
use glam::{Mat3, Mat4, Quat, Vec2, Vec3, Vec4};
use grimora_core::animation::{AnimationClip, Channel, Interpolation, Joint, JointTransform, Keyframes, MAX_JOINTS};
use grimora_core::{AlphaMode, Material, MeshData, Skeleton, Submesh, TextureData, Vertex};

/// Largest texture side used when a scene doesn't say otherwise.
pub const DEFAULT_MAX_TEXTURE_SIZE: u32 = 2048;

type CacheKey = (PathBuf, u32);
type CacheEntry = (Option<SystemTime>, MeshData);

fn cache() -> &'static Mutex<HashMap<CacheKey, CacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<CacheKey, CacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Loads a model, reusing the cached copy while the file is unchanged.
/// Textures larger than `max_texture_size` are downscaled (0 = the default).
pub fn load_model(path: &Path, max_texture_size: u32) -> Result<MeshData> {
    let max = if max_texture_size == 0 { DEFAULT_MAX_TEXTURE_SIZE } else { max_texture_size };
    let key = (path.to_path_buf(), max);
    let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    if let Some((cached_mtime, mesh)) = cache().lock().unwrap().get(&key)
        && *cached_mtime == mtime
    {
        return Ok(mesh.clone());
    }
    let mesh = import(path, max)?;
    cache().lock().unwrap().insert(key, (mtime, mesh.clone()));
    Ok(mesh)
}

fn import(path: &Path, max_texture_size: u32) -> Result<MeshData> {
    let started = std::time::Instant::now();
    let (document, buffers, images) = gltf::import(path).with_context(|| format!("importing {}", path.display()))?;
    let decoded = started.elapsed();

    // Images are shared between materials (e.g. metallic-roughness + occlusion).
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    let textures: Vec<Arc<TextureData>> = images
        .iter()
        .enumerate()
        .map(|(i, image)| Arc::new(to_rgba8(image, format!("{file_name}#{i}"), max_texture_size)))
        .collect();

    let mut mesh = MeshData::new();
    // glTF's implicit default material is appended after the real ones if needed.
    let default_material = document.materials().len();
    mesh.materials = document.materials().map(|m| material(&m, &textures)).collect();

    let Some(scene) = document.default_scene().or_else(|| document.scenes().next()) else {
        bail!("{} has no scene", path.display());
    };
    let nodes = NodeTree::new(&document, &scene);
    let rig = document.skins().next().and_then(|skin| Rig::new(&document, &skin, &buffers, &nodes, path));
    if document.skins().len() > 1 {
        log::warn!("{}: only the first skin is used", path.display());
    }

    for &node_index in &nodes.order {
        let node = document.nodes().nth(node_index).expect("node index from the tree");
        let Some(gltf_mesh) = node.mesh() else { continue };
        let world = nodes.world[&node_index];
        let (placement, binding) = match (&rig, node.skin()) {
            (Some(rig), Some(skin)) if skin.index() == rig.skin => (Mat4::IDENTITY, Binding::Skinned),
            (_, Some(_)) => {
                log::warn!("{}: skipping a mesh bound to another skin", path.display());
                continue;
            }
            // Under a joint: move into that joint's bind space and follow it.
            (Some(rig), None) => match nodes.ancestors(node_index).find_map(|n| rig.joint_of_node.get(&n)) {
                Some(&joint) => (rig.rest_skinning[joint].inverse() * world, Binding::Rigid(joint as u8)),
                None => (world, Binding::None),
            },
            (None, _) => (world, Binding::None),
        };
        for primitive in gltf_mesh.primitives() {
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                log::warn!(
                    "{}: skipping a {:?} primitive (only triangles are supported)",
                    path.display(),
                    primitive.mode()
                );
                continue;
            }
            let material = primitive.material().index().unwrap_or(default_material);
            append_primitive(&mut mesh, &primitive, &buffers, placement, binding, material);
        }
    }
    mesh.skeleton = rig.map(|rig| Arc::new(rig.skeleton));

    if mesh.submeshes.iter().any(|s| s.material == default_material) {
        mesh.materials.push(Material::default());
    }
    if mesh.is_empty() {
        bail!("{} has no triangles", path.display());
    }
    let (joints, clips) = mesh.skeleton.as_ref().map_or((0, 0), |s| (s.joints.len(), s.clips.len()));
    log::info!(
        "Imported {} ({} triangles, {} textures, {joints} joints, {clips} animations) in {:.0} ms ({:.0} ms decoding)",
        path.display(),
        mesh.triangle_count(),
        textures.len(),
        started.elapsed().as_secs_f64() * 1000.0,
        decoded.as_secs_f64() * 1000.0
    );
    Ok(mesh)
}

/// The scene's nodes with their world matrices and parents, in file order.
struct NodeTree {
    /// Depth-first, parents before children.
    order: Vec<usize>,
    world: HashMap<usize, Mat4>,
    parent: HashMap<usize, usize>,
}

impl NodeTree {
    fn new(document: &gltf::Document, scene: &gltf::Scene) -> Self {
        let mut tree = Self { order: Vec::new(), world: HashMap::new(), parent: HashMap::new() };
        // Depth-first in file order (children are pushed reversed onto the stack).
        let mut stack: Vec<(gltf::Node, Mat4)> = scene.nodes().map(|n| (n, Mat4::IDENTITY)).collect();
        stack.reverse();
        while let Some((node, parent)) = stack.pop() {
            let world = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
            tree.order.push(node.index());
            tree.world.insert(node.index(), world);
            let first_child = stack.len();
            for child in node.children() {
                tree.parent.insert(child.index(), node.index());
                stack.push((child, world));
            }
            stack[first_child..].reverse();
        }
        // Joints outside the scene still need a matrix.
        for node in document.nodes() {
            tree.world.entry(node.index()).or_insert(Mat4::IDENTITY);
        }
        tree
    }

    /// The node itself, then its parent, grandparent...
    fn ancestors(&self, node: usize) -> impl Iterator<Item = usize> + '_ {
        std::iter::successors(Some(node), |n| self.parent.get(n).copied())
    }
}

/// The skin's skeleton and how its joints map to nodes.
struct Rig {
    skin: usize,
    skeleton: Skeleton,
    joint_of_node: HashMap<usize, usize>,
    /// Skinning matrix of each joint in the rest pose (rest global × inverse bind).
    rest_skinning: Vec<Mat4>,
}

impl Rig {
    fn new(
        document: &gltf::Document,
        skin: &gltf::Skin,
        buffers: &[gltf::buffer::Data],
        nodes: &NodeTree,
        path: &Path,
    ) -> Option<Self> {
        let joint_nodes: Vec<gltf::Node> = skin.joints().collect();
        if joint_nodes.is_empty() || joint_nodes.len() > MAX_JOINTS {
            log::warn!(
                "{}: skin has {} joints (1 to {MAX_JOINTS} supported); importing it static",
                path.display(),
                joint_nodes.len()
            );
            return None;
        }
        let joint_of_node: HashMap<usize, usize> =
            joint_nodes.iter().enumerate().map(|(i, n)| (n.index(), i)).collect();
        let reader = skin.reader(|buffer| buffers.get(buffer.index()).map(|data| &data.0[..]));
        let inverse_binds: Vec<Mat4> = reader
            .read_inverse_bind_matrices()
            .map(|m| m.map(|m| Mat4::from_cols_array_2d(&m)).collect())
            .unwrap_or_default();

        let mut root = None;
        let joints: Vec<Joint> = joint_nodes
            .iter()
            .enumerate()
            .map(|(i, node)| {
                // The nearest ancestor that is also a joint.
                let parent = nodes.ancestors(node.index()).skip(1).find_map(|n| joint_of_node.get(&n).copied());
                if parent.is_none() {
                    let above = nodes.parent.get(&node.index()).map_or(Mat4::IDENTITY, |p| nodes.world[p]);
                    match root {
                        None => root = Some(above),
                        Some(r) if !r.abs_diff_eq(above, 1e-4) => {
                            log::warn!("{}: root joints sit under different transforms", path.display());
                        }
                        Some(_) => {}
                    }
                }
                let (translation, rotation, scale) = node.transform().decomposed();
                Joint {
                    name: node.name().map_or_else(|| format!("joint{i}"), str::to_owned),
                    parent,
                    rest: JointTransform {
                        translation: Vec3::from(translation),
                        rotation: Quat::from_array(rotation).normalize(),
                        scale: Vec3::from(scale),
                    },
                    inverse_bind: inverse_binds.get(i).copied().unwrap_or(Mat4::IDENTITY),
                }
            })
            .collect();

        let clips =
            document.animations().enumerate().filter_map(|(i, a)| clip(&a, i, &joint_of_node, buffers)).collect();
        let skeleton = Skeleton::new(joints, root.unwrap_or(Mat4::IDENTITY), clips);
        let mut globals = Vec::new();
        skeleton.global_transforms(&skeleton.rest_pose(), &mut globals);
        let rest_skinning = globals.iter().zip(&skeleton.joints).map(|(g, j)| *g * j.inverse_bind).collect();
        Some(Self { skin: skin.index(), skeleton, joint_of_node, rest_skinning })
    }
}

/// Converts the channels of `animation` that target skeleton joints.
fn clip(
    animation: &gltf::Animation,
    index: usize,
    joint_of_node: &HashMap<usize, usize>,
    buffers: &[gltf::buffer::Data],
) -> Option<AnimationClip> {
    use gltf::animation::util::ReadOutputs;
    let name = animation.name().map_or_else(|| format!("animation{index}"), str::to_owned);
    let mut channels = Vec::new();
    let mut skipped = 0;
    for channel in animation.channels() {
        let Some(&joint) = joint_of_node.get(&channel.target().node().index()) else {
            skipped += 1;
            continue;
        };
        let reader = channel.reader(|buffer| buffers.get(buffer.index()).map(|data| &data.0[..]));
        let Some(times) = reader.read_inputs().map(Iterator::collect::<Vec<f32>>) else { continue };
        let interpolation = channel.sampler().interpolation();
        // Cubic splines store (in-tangent, value, out-tangent) per key: keep the values.
        let values = |n: usize| -> Vec<usize> {
            match interpolation {
                gltf::animation::Interpolation::CubicSpline => (0..n / 3).map(|k| k * 3 + 1).collect(),
                _ => (0..n).collect(),
            }
        };
        let keys = match reader.read_outputs() {
            Some(ReadOutputs::Translations(v)) => {
                let v: Vec<Vec3> = v.map(Vec3::from).collect();
                Keyframes::Translation(values(v.len()).into_iter().map(|k| v[k]).collect())
            }
            Some(ReadOutputs::Rotations(v)) => {
                let v: Vec<Quat> = v.into_f32().map(|q| Quat::from_array(q).normalize()).collect();
                Keyframes::Rotation(values(v.len()).into_iter().map(|k| v[k]).collect())
            }
            Some(ReadOutputs::Scales(v)) => {
                let v: Vec<Vec3> = v.map(Vec3::from).collect();
                Keyframes::Scale(values(v.len()).into_iter().map(|k| v[k]).collect())
            }
            // Morph target weights aren't supported.
            _ => continue,
        };
        let interpolation = match interpolation {
            gltf::animation::Interpolation::Step => Interpolation::Step,
            _ => Interpolation::Linear,
        };
        channels.push(Channel { joint, interpolation, times, keys });
    }
    if skipped > 0 {
        log::warn!("animation {name:?}: {skipped} channels target nodes outside the skeleton and were skipped");
    }
    let duration = channels.iter().filter_map(|c| c.times.last()).fold(0.0f32, |a, &b| a.max(b));
    (!channels.is_empty()).then_some(AnimationClip { name, duration, channels })
}

#[derive(Debug, Clone, Copy)]
enum Binding {
    None,
    /// Every vertex follows this joint.
    Rigid(u8),
    /// Joints and weights come from the primitive.
    Skinned,
}

fn append_primitive(
    mesh: &mut MeshData,
    primitive: &gltf::Primitive,
    buffers: &[gltf::buffer::Data],
    world: Mat4,
    binding: Binding,
    material: usize,
) {
    let reader = primitive.reader(|buffer| buffers.get(buffer.index()).map(|data| &data.0[..]));
    let Some(positions) = reader.read_positions() else {
        return;
    };
    let positions: Vec<Vec3> = positions.map(|p| world.transform_point3(Vec3::from(p))).collect();
    let count = positions.len();
    let mut indices: Vec<u32> = match reader.read_indices() {
        Some(indices) => indices.into_u32().collect(),
        None => (0..count as u32).collect(),
    };
    indices.truncate(indices.len() / 3 * 3);
    if indices.iter().any(|&i| i as usize >= count) {
        log::warn!("skipping a primitive with out-of-range indices");
        return;
    }
    // A mirroring transform flips the triangles inside out.
    if world.determinant() < 0.0 {
        for tri in indices.as_chunks_mut::<3>().0 {
            tri.swap(1, 2);
        }
    }

    let normal_matrix = Mat3::from_mat4(world).inverse().transpose();
    let normals: Vec<Vec3> = match reader.read_normals() {
        Some(normals) => normals.map(|n| (normal_matrix * Vec3::from(n)).normalize_or_zero()).collect(),
        None => smooth_normals(&positions, &indices),
    };
    let uvs: Vec<Vec2> = match reader.read_tex_coords(0) {
        Some(uvs) => uvs.into_f32().map(Vec2::from).collect(),
        None => vec![Vec2::ZERO; count],
    };
    let tangents: Vec<Vec4> = match reader.read_tangents() {
        Some(tangents) => tangents
            .map(|t| {
                let dir = (Mat3::from_mat4(world) * Vec3::new(t[0], t[1], t[2])).normalize_or_zero();
                dir.extend(if world.determinant() < 0.0 { -t[3] } else { t[3] })
            })
            .collect(),
        None => generate_tangents(&positions, &normals, &uvs, &indices),
    };
    // Vertex colors are linear in glTF and sRGB in Grimora.
    let colors: Vec<Vec3> = match reader.read_colors(0) {
        Some(colors) => colors.into_rgb_f32().map(|c| linear_to_srgb(Vec3::from(c))).collect(),
        None => vec![Vec3::ONE; count],
    };
    let skin: Vec<([u8; 4], [u8; 4])> = match binding {
        Binding::None => vec![([0; 4], [0; 4]); count],
        Binding::Rigid(joint) => vec![([joint, 0, 0, 0], [255, 0, 0, 0]); count],
        Binding::Skinned => {
            let joints: Vec<[u16; 4]> = reader.read_joints(0).map_or_else(Vec::new, |j| j.into_u16().collect());
            let weights: Vec<[f32; 4]> = reader.read_weights(0).map_or_else(Vec::new, |w| w.into_f32().collect());
            (0..count)
                .map(|i| match (joints.get(i), weights.get(i)) {
                    (Some(j), Some(w)) => pack_weights(*j, *w),
                    _ => ([0; 4], [0; 4]),
                })
                .collect()
        }
    };

    let base = mesh.vertices.len() as u32;
    mesh.vertices.extend((0..count).map(|i| Vertex {
        position: positions[i].to_array(),
        normal: normals[i].to_array(),
        color: colors[i].to_array(),
        uv: uvs[i].to_array(),
        tangent: tangents[i].to_array(),
        joints: skin[i].0,
        weights: skin[i].1,
    }));
    let start = mesh.indices.len() as u32;
    mesh.indices.extend(indices.iter().map(|i| i + base));
    mesh.submeshes.push(Submesh { indices: start..mesh.indices.len() as u32, material });
}

fn material(m: &gltf::Material, textures: &[Arc<TextureData>]) -> Material {
    let texture = |info: gltf::texture::Texture, tex_coord: u32| {
        if tex_coord != 0 {
            log::warn!("material {:?}: only the first UV set is supported", m.name());
            return None;
        }
        textures.get(info.source().index()).cloned()
    };
    let pbr = m.pbr_metallic_roughness();
    let normal = m.normal_texture();
    let occlusion = m.occlusion_texture();
    Material {
        name: m.name().map(str::to_owned),
        base_color: Vec4::from(pbr.base_color_factor()),
        metallic: pbr.metallic_factor(),
        roughness: pbr.roughness_factor(),
        normal_scale: normal.as_ref().map_or(1.0, |n| n.scale()),
        occlusion_strength: occlusion.as_ref().map_or(1.0, |o| o.strength()),
        double_sided: m.double_sided(),
        alpha_mode: match m.alpha_mode() {
            gltf::material::AlphaMode::Opaque => AlphaMode::Opaque,
            gltf::material::AlphaMode::Mask => AlphaMode::Mask { cutoff: m.alpha_cutoff().unwrap_or(0.5) },
            gltf::material::AlphaMode::Blend => AlphaMode::Blend,
        },
        emissive: Vec3::from(m.emissive_factor()) * m.emissive_strength().unwrap_or(1.0),
        base_color_texture: pbr.base_color_texture().and_then(|t| texture(t.texture(), t.tex_coord())),
        normal_texture: normal.as_ref().and_then(|t| texture(t.texture(), t.tex_coord())),
        metallic_roughness_texture: pbr.metallic_roughness_texture().and_then(|t| texture(t.texture(), t.tex_coord())),
        occlusion_texture: occlusion.as_ref().and_then(|t| texture(t.texture(), t.tex_coord())),
        emissive_texture: m.emissive_texture().and_then(|t| texture(t.texture(), t.tex_coord())),
    }
}

/// Converts any glTF pixel format to RGBA8, halving it until it fits `max_size`.
fn to_rgba8(image: &gltf::image::Data, name: String, max_size: u32) -> TextureData {
    use gltf::image::Format;
    use image::{DynamicImage, ImageBuffer};
    let (w, h) = (image.width, image.height);
    let pixels = image.pixels.clone();
    // Common 8-bit formats go through `image` (fast, optimized even in dev builds).
    let dynamic = match image.format {
        Format::R8 => ImageBuffer::from_raw(w, h, pixels).map(DynamicImage::ImageLuma8),
        Format::R8G8B8 => ImageBuffer::from_raw(w, h, pixels).map(DynamicImage::ImageRgb8),
        Format::R8G8B8A8 => ImageBuffer::from_raw(w, h, pixels).map(DynamicImage::ImageRgba8),
        _ => ImageBuffer::from_raw(w, h, convert_slow(image)).map(DynamicImage::ImageRgba8),
    };
    let Some(dynamic) = dynamic else {
        log::warn!("texture {name} has the wrong size, using white");
        return TextureData { name, width: 1, height: 1, rgba: vec![255; 4] };
    };
    let rgba = dynamic.into_rgba8();
    let mut texture = TextureData { name, width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() };
    // A box filter is ~20x faster than `image`'s resize and looks the same at 2:1.
    while texture.width.max(texture.height) > max_size.max(1) {
        texture = texture.half();
    }
    texture
}

/// Per-pixel conversion for the rare 2-channel, 16-bit and float formats.
fn convert_slow(image: &gltf::image::Data) -> Vec<u8> {
    use gltf::image::Format;
    let (channels, bytes) = match image.format {
        Format::R8 => (1, 1),
        Format::R8G8 => (2, 1),
        Format::R8G8B8 => (3, 1),
        Format::R8G8B8A8 => (4, 1),
        Format::R16 => (1, 2),
        Format::R16G16 => (2, 2),
        Format::R16G16B16 => (3, 2),
        Format::R16G16B16A16 => (4, 2),
        Format::R32G32B32FLOAT => (3, 4),
        Format::R32G32B32A32FLOAT => (4, 4),
    };
    let sample = |px: &[u8], c: usize| -> u8 {
        let s = &px[c * bytes..(c + 1) * bytes];
        match bytes {
            1 => s[0],
            2 => (u16::from_ne_bytes([s[0], s[1]]) >> 8) as u8,
            _ => (f32::from_ne_bytes([s[0], s[1], s[2], s[3]]).clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
        }
    };
    let mut rgba = Vec::with_capacity(image.width as usize * image.height as usize * 4);
    for px in image.pixels.chunks_exact(channels * bytes) {
        let value = |c| sample(px, c);
        rgba.extend_from_slice(&match channels {
            1 => [value(0), value(0), value(0), 255],
            2 => [value(0), value(1), 0, 255],
            3 => [value(0), value(1), value(2), 255],
            _ => [value(0), value(1), value(2), value(3)],
        });
    }
    rgba
}

/// Quantizes four joint weights to bytes that sum to exactly 255. Joints past
/// [`MAX_JOINTS`] can't be stored and lose their weight.
fn pack_weights(joints: [u16; 4], weights: [f32; 4]) -> ([u8; 4], [u8; 4]) {
    let mut w = weights.map(|w| w.max(0.0));
    for (weight, &joint) in w.iter_mut().zip(&joints) {
        if usize::from(joint) >= MAX_JOINTS {
            *weight = 0.0;
        }
    }
    let sum: f32 = w.iter().sum();
    if sum <= 0.0 {
        return ([0; 4], [0; 4]);
    }
    let mut bytes = w.map(|w| (w / sum * 255.0).round() as u8);
    // Rounding can leave the total off by one or two: settle it on the heaviest joint.
    let heaviest = (0..4).max_by(|&a, &b| w[a].total_cmp(&w[b])).unwrap_or(0);
    let total: i32 = bytes.iter().map(|&b| i32::from(b)).sum();
    bytes[heaviest] = (i32::from(bytes[heaviest]) + 255 - total).clamp(0, 255) as u8;
    (joints.map(|j| j.min(255) as u8), bytes)
}

/// Area-weighted vertex normals, for files that don't provide any.
fn smooth_normals(positions: &[Vec3], indices: &[u32]) -> Vec<Vec3> {
    let mut normals = vec![Vec3::ZERO; positions.len()];
    for tri in indices.as_chunks::<3>().0 {
        let [a, b, c] = tri.map(|i| i as usize);
        let n = (positions[b] - positions[a]).cross(positions[c] - positions[a]);
        for i in [a, b, c] {
            normals[i] += n;
        }
    }
    normals.into_iter().map(|n| n.try_normalize().unwrap_or(Vec3::Y)).collect()
}

/// Per-vertex tangents from UV gradients, for normal-mapped files without them.
fn generate_tangents(positions: &[Vec3], normals: &[Vec3], uvs: &[Vec2], indices: &[u32]) -> Vec<Vec4> {
    let mut tangents = vec![Vec3::ZERO; positions.len()];
    let mut bitangents = vec![Vec3::ZERO; positions.len()];
    for tri in indices.as_chunks::<3>().0 {
        let [a, b, c] = tri.map(|i| i as usize);
        let (e1, e2) = (positions[b] - positions[a], positions[c] - positions[a]);
        let (d1, d2) = (uvs[b] - uvs[a], uvs[c] - uvs[a]);
        let det = d1.x * d2.y - d2.x * d1.y;
        if det.abs() < 1e-12 {
            continue;
        }
        let t = (e1 * d2.y - e2 * d1.y) / det;
        let bt = (e2 * d1.x - e1 * d2.x) / det;
        for i in [a, b, c] {
            tangents[i] += t;
            bitangents[i] += bt;
        }
    }
    (0..positions.len())
        .map(|i| {
            let n = normals[i];
            let t =
                (tangents[i] - n * n.dot(tangents[i])).try_normalize().unwrap_or_else(|| n.any_orthonormal_vector());
            let sign = if n.cross(t).dot(bitangents[i]) < 0.0 { -1.0 } else { 1.0 };
            t.extend(sign)
        })
        .collect()
}

fn linear_to_srgb(c: Vec3) -> Vec3 {
    c.map(|x| if x <= 0.003_130_8 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes a small glTF to a temp folder: one textured, alpha-masked, emissive
    /// triangle used by two nodes under a translated parent, one scaled by 2 and
    /// one mirrored.
    fn write_fixture() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("grimora_gltf_fixture_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let mut bin = Vec::new();
        let floats: [&[f32]; 3] = [
            &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0], // positions
            &[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0], // normals
            &[0.0, 0.0, 1.0, 0.0, 0.0, 1.0],                // uvs
        ];
        for f in floats.iter().flat_map(|a| a.iter()) {
            bin.extend_from_slice(&f.to_le_bytes());
        }
        std::fs::write(dir.join("tri.bin"), &bin).unwrap();
        image::RgbaImage::from_pixel(8, 4, image::Rgba([200, 100, 50, 255])).save(dir.join("tex.png")).unwrap();

        let gltf = r#"{
            "asset": { "version": "2.0" },
            "extensionsUsed": ["KHR_materials_emissive_strength"],
            "scene": 0,
            "scenes": [ { "nodes": [0] } ],
            "nodes": [
                { "translation": [0, 1, 0], "children": [1, 2] },
                { "mesh": 0, "scale": [2, 2, 2] },
                { "mesh": 0, "scale": [-1, 1, 1] }
            ],
            "meshes": [ { "primitives": [ {
                "attributes": { "POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2 },
                "material": 0
            } ] } ],
            "materials": [ {
                "doubleSided": true,
                "alphaMode": "MASK",
                "alphaCutoff": 0.3,
                "emissiveFactor": [1, 0.5, 0.25],
                "emissiveTexture": { "index": 0 },
                "extensions": { "KHR_materials_emissive_strength": { "emissiveStrength": 2 } },
                "pbrMetallicRoughness": {
                    "baseColorFactor": [0.5, 0.5, 0.5, 1],
                    "metallicFactor": 0.25,
                    "roughnessFactor": 0.6,
                    "baseColorTexture": { "index": 0 },
                    "metallicRoughnessTexture": { "index": 0 }
                },
                "normalTexture": { "index": 0, "scale": 0.5 },
                "occlusionTexture": { "index": 0, "strength": 0.7 }
            } ],
            "textures": [ { "source": 0 } ],
            "images": [ { "uri": "tex.png" } ],
            "buffers": [ { "uri": "tri.bin", "byteLength": 96 } ],
            "bufferViews": [
                { "buffer": 0, "byteOffset": 0, "byteLength": 36 },
                { "buffer": 0, "byteOffset": 36, "byteLength": 36 },
                { "buffer": 0, "byteOffset": 72, "byteLength": 24 }
            ],
            "accessors": [
                { "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                  "min": [0, 0, 0], "max": [1, 1, 0] },
                { "bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3" },
                { "bufferView": 2, "componentType": 5126, "count": 3, "type": "VEC2" }
            ]
        }"#;
        let path = dir.join("tri.gltf");
        std::fs::write(&path, gltf).unwrap();
        path
    }

    #[test]
    fn loads_a_gltf() {
        let path = write_fixture();
        let mesh = load_model(&path, 4).unwrap();
        assert_eq!(mesh.vertices.len(), 6);
        assert_eq!(mesh.triangle_count(), 2);
        assert_eq!(mesh.submeshes.len(), 2);
        assert_eq!(mesh.materials.len(), 1);

        // Node transforms are baked: parent +1 Y, children scaled by 2 and mirrored.
        let (lo, hi) = mesh.bounds().unwrap();
        assert!(lo.abs_diff_eq(Vec3::new(-1.0, 1.0, 0.0), 1e-5), "{lo}");
        assert!(hi.abs_diff_eq(Vec3::new(2.0, 3.0, 0.0), 1e-5), "{hi}");

        // The mirrored copy has its winding flipped, so both triangles face their normals.
        for tri in mesh.indices.as_chunks::<3>().0 {
            let [a, b, c] = tri.map(|i| Vec3::from(mesh.vertices[i as usize].position));
            let normal = Vec3::from(mesh.vertices[tri[0] as usize].normal);
            assert!((b - a).cross(c - a).dot(normal) > 0.0);
        }

        // No TANGENT attribute: tangents are generated from the UVs. Nodes keep file
        // order, so the scaled copy comes first; the mirrored one is left-handed.
        let tangent = |i: usize| Vec4::from(mesh.vertices[i].tangent);
        assert!(tangent(0).abs_diff_eq(Vec4::new(1.0, 0.0, 0.0, 1.0), 1e-5), "{}", tangent(0));
        assert!(tangent(3).abs_diff_eq(Vec4::new(-1.0, 0.0, 0.0, -1.0), 1e-5), "{}", tangent(3));

        let material = mesh.material(0);
        assert!(material.double_sided);
        assert_eq!((material.metallic, material.roughness), (0.25, 0.6));
        assert_eq!((material.normal_scale, material.occlusion_strength), (0.5, 0.7));
        assert!(material.base_color.abs_diff_eq(Vec4::new(0.5, 0.5, 0.5, 1.0), 1e-6));
        assert_eq!(material.alpha_mode, AlphaMode::Mask { cutoff: 0.3 });
        assert!(material.emissive.abs_diff_eq(Vec3::new(2.0, 1.0, 0.5), 1e-6), "{}", material.emissive);
        // Halved until it fits 4 px; every slot reuses the single image.
        let base = material.base_color_texture.clone().unwrap();
        assert_eq!((base.width, base.height), (4, 2));
        assert_eq!(&base.rgba[..4], &[200, 100, 50, 255]);
        for other in [
            &material.normal_texture,
            &material.metallic_roughness_texture,
            &material.occlusion_texture,
            &material.emissive_texture,
        ] {
            assert!(Arc::ptr_eq(&base, other.as_ref().unwrap()));
        }

        // Cached: a second load is identical.
        assert_eq!(load_model(&path, 4).unwrap().vertices, mesh.vertices);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn imports_skin_and_animations() {
        let path = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/models/wanderer/wanderer.gltf"));
        let mesh = load_model(path, 0).unwrap();
        let skeleton = mesh.skeleton.clone().expect("the wanderer is rigged");
        assert_eq!(skeleton.joints.len(), 17);
        for name in ["idle", "walk", "run", "jump"] {
            assert!(skeleton.clip(name).is_some(), "missing clip {name}");
        }
        for v in &mesh.vertices {
            assert_eq!(v.weights.iter().map(|&w| w as u32).sum::<u32>(), 255);
            assert!(v.joints.iter().all(|&j| (j as usize) < skeleton.joints.len()));
        }

        // The bind pose leaves the mesh where it was modeled.
        let mut animator = grimora_core::Animator::new(skeleton);
        for m in animator.skinning_matrices() {
            assert!(m.abs_diff_eq(Mat4::IDENTITY, 1e-5), "{m}");
        }
        // Walking swings the legs; the lantern joint follows the left hand.
        let rest_foot = animator.joint_transform("foot.L").unwrap();
        assert!(animator.play("walk", 0.0));
        animator.update(0.2);
        let foot = animator.joint_transform("foot.L").unwrap();
        assert!(foot.w_axis.distance(rest_foot.w_axis) > 0.1);
        let hand = animator.joint_transform("hand.L").unwrap().w_axis;
        let lantern = animator.joint_transform("lantern").unwrap().w_axis;
        assert!((hand.distance(lantern) - 0.07).abs() < 1e-4);
    }

    #[test]
    fn missing_file_is_an_error() {
        assert!(load_model(Path::new("does/not/exist.gltf"), 0).is_err());
    }
}
