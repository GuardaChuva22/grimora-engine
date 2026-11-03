//! Procedural low-poly meshes. Every prop has its origin at the center of its
//! base (y = 0), so placing it on the ground is just setting its position.

use std::f32::consts::{PI, TAU};

use std::path::Path;

use grimora_core::{MeshData, Vec3};
use serde::{Deserialize, Serialize};

use crate::model::{add_events, load_model};
use crate::rng::{Rng, value_noise};
use crate::texture::{Picture, decal_mesh, sprite_mesh};

/// Which mesh an object uses. Seeded shapes give a different variation per seed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MeshDesc {
    /// 1Ã—1Ã—1 box.
    Cube,
    /// Flat square on the XZ plane.
    Plane {
        #[serde(default = "one")]
        size: f32,
    },
    /// Radius 0.5, height 1.
    Cylinder {
        #[serde(default)]
        segments: u32,
    },
    /// Radius 0.5, height 1.
    Cone {
        #[serde(default)]
        segments: u32,
    },
    /// Radius 0.5, resting on the ground.
    Sphere {
        #[serde(default)]
        subdivisions: u32,
    },
    /// Conifer, about 3 units tall.
    Pine {
        #[serde(default)]
        seed: u64,
        #[serde(default)]
        tiers: u32,
    },
    /// Bare, twisted tree, about 2.5 units tall.
    DeadTree {
        #[serde(default)]
        seed: u64,
    },
    /// Lumpy boulder, about 1 unit wide.
    Rock {
        #[serde(default)]
        seed: u64,
    },
    /// Cluster of pointed crystals. Pair with `emissive` and a light.
    Crystal {
        #[serde(default)]
        seed: u64,
    },
    /// Hooded, cloaked figure with a staff, 1.9 units tall, facing -Z. The player character.
    Wanderer,
    /// Stone column on a plinth, about 3.4 units tall; `broken` snaps the top off.
    Pillar {
        #[serde(default)]
        seed: u64,
        #[serde(default)]
        broken: bool,
    },
    /// A glTF 2.0 model (`.gltf` or `.glb`), relative to the scene file.
    /// Keeps the file's own origin and scale. Textures are downscaled to
    /// `max_texture_size` (default 2048).
    Model {
        path: String,
        #[serde(default)]
        max_texture_size: u32,
        /// Animation events to add, as (clip, seconds, name), for a file that has
        /// none in its animations' `extras`.
        #[serde(default)]
        events: Vec<(String, f32, String)>,
    },
    /// Crossed upright pictures of a plant, with its base at the origin: grass, weeds...
    /// With `planes: 1` and `sway: 0.0`, a still picture: a fence, a web, a banner.
    /// It has no collider unless given one. Costs little, so scatter plenty of them.
    Sprite {
        /// Image file with a transparent background, relative to the scene file.
        texture: String,
        /// Height of the drawing itself in meters.
        #[serde(default = "one")]
        size: f32,
        /// How many crossed pictures. 2 by default.
        #[serde(default)]
        planes: u32,
        /// How far the wind moves its top, in meters; by default a fifth of `size`.
        #[serde(default)]
        sway: Option<f32>,
        /// Casts a shadow, which costs far more than the plant itself.
        #[serde(default)]
        shadow: bool,
    },
    /// A picture lying flat on the ground, with no shadow or collider: blood, moss,
    /// cracks. `size` is its width in meters. Keep it on level ground, or turn the
    /// object to put it on a wall.
    Decal {
        /// Image file with a transparent background, relative to the scene file.
        texture: String,
        #[serde(default = "one")]
        size: f32,
    },
}

fn one() -> f32 {
    1.0
}

impl MeshDesc {
    /// Same shape with another seed (unseeded shapes are returned unchanged).
    pub fn with_seed(&self, new_seed: u64) -> Self {
        let mut d = self.clone();
        match &mut d {
            Self::Pine { seed, .. }
            | Self::DeadTree { seed }
            | Self::Rock { seed }
            | Self::Crystal { seed }
            | Self::Pillar { seed, .. } => *seed = new_seed,
            Self::Cube
            | Self::Wanderer
            | Self::Plane { .. }
            | Self::Cylinder { .. }
            | Self::Cone { .. }
            | Self::Sphere { .. }
            | Self::Model { .. }
            | Self::Sprite { .. }
            | Self::Decal { .. } => {}
        }
        d
    }

    /// Generates the mesh. Model paths are resolved from the current directory;
    /// scenes use [`MeshDesc::build_in`] to resolve them from the scene file.
    pub fn build(&self) -> MeshData {
        self.build_in(Path::new(""))
    }

    /// Like [`MeshDesc::build`], with model paths relative to `dir`. A model that
    /// fails to load is logged and replaced by a magenta cube, so a scene with a
    /// broken reference still opens.
    pub fn build_in(&self, dir: &Path) -> MeshData {
        let white = Vec3::ONE;
        match *self {
            Self::Model { ref path, max_texture_size, ref events } => {
                match load_model(&dir.join(path), max_texture_size) {
                    Ok(mut mesh) => {
                        if !events.is_empty() {
                            add_events(&mut mesh, events);
                        }
                        mesh
                    }
                    Err(err) => {
                        log::error!("{err:#}");
                        cube(Vec3::new(0.0, 0.5, 0.0), Vec3::ONE, Vec3::new(1.0, 0.0, 1.0))
                    }
                }
            }
            Self::Sprite { ref texture, size, planes, sway, shadow } => {
                let planes = if planes == 0 { 2 } else { planes };
                let sway = sway.unwrap_or(size * 0.2);
                match Picture::load(&dir.join(texture)) {
                    Some(picture) => sprite_mesh(&picture, size, planes, sway, shadow),
                    None => cube(Vec3::new(0.0, 0.5, 0.0), Vec3::ONE, Vec3::new(1.0, 0.0, 1.0)),
                }
            }
            Self::Decal { ref texture, size } => match Picture::load(&dir.join(texture)) {
                Some(picture) => decal_mesh(&picture, size),
                None => cube(Vec3::new(0.0, 0.5, 0.0), Vec3::ONE, Vec3::new(1.0, 0.0, 1.0)),
            },
            Self::Cube => cube(Vec3::new(0.0, 0.5, 0.0), Vec3::ONE, white),
            Self::Plane { size } => {
                let h = size * 0.5;
                let mut m = MeshData::new();
                m.push_quad(
                    Vec3::new(-h, 0.0, -h),
                    Vec3::new(-h, 0.0, h),
                    Vec3::new(h, 0.0, h),
                    Vec3::new(h, 0.0, -h),
                    white,
                );
                m
            }
            Self::Cylinder { segments } => {
                let mut m = MeshData::new();
                tube(&mut m, Vec3::ZERO, Vec3::Y, 0.5, 0.5, segments_or(segments, 10), 0.0, white, None);
                m
            }
            Self::Cone { segments } => {
                let mut m = MeshData::new();
                tube(&mut m, Vec3::ZERO, Vec3::Y, 0.5, 0.0, segments_or(segments, 10), 0.0, white, None);
                m
            }
            Self::Sphere { subdivisions } => {
                let mut m = MeshData::new();
                blob(&mut m, Vec3::new(0.0, 0.5, 0.0), Vec3::splat(0.5), subdivisions.min(4), |_| 1.0, white, None);
                m
            }
            Self::Pine { seed, tiers } => {
                let tiers = if tiers == 0 { 4 } else { tiers.min(8) };
                pine(seed, tiers, 0).with_lods([(0.04, pine(seed, tiers, 1))])
            }
            Self::DeadTree { seed } => dead_tree(seed, 0).with_lods([(0.03, dead_tree(seed, 1))]),
            Self::Rock { seed } => rock(seed, 0).with_lods([(0.03, rock(seed, 1))]),
            Self::Crystal { seed } => crystal(seed),
            Self::Pillar { seed, broken } => pillar(seed, broken),
            Self::Wanderer => wanderer(),
        }
    }
}

fn segments_or(segments: u32, default: u32) -> u32 {
    if segments < 3 { default } else { segments.min(64) }
}

fn shade(color: Vec3, rng: Option<&mut Rng>, amount: f32) -> Vec3 {
    match rng {
        Some(rng) => color * (1.0 + rng.jitter(amount)),
        None => color,
    }
}

/// Axis-aligned box centered on `center`.
pub fn cube(center: Vec3, size: Vec3, color: Vec3) -> MeshData {
    let mut m = MeshData::new();
    let h = size * 0.5;
    let p = |x: f32, y: f32, z: f32| center + Vec3::new(x * h.x, y * h.y, z * h.z);
    // +X, -X, +Y, -Y, +Z, -Z (counter-clockwise seen from outside)
    m.push_quad(p(1., -1., 1.), p(1., -1., -1.), p(1., 1., -1.), p(1., 1., 1.), color);
    m.push_quad(p(-1., -1., -1.), p(-1., -1., 1.), p(-1., 1., 1.), p(-1., 1., -1.), color);
    m.push_quad(p(-1., 1., 1.), p(1., 1., 1.), p(1., 1., -1.), p(-1., 1., -1.), color);
    m.push_quad(p(-1., -1., -1.), p(1., -1., -1.), p(1., -1., 1.), p(-1., -1., 1.), color);
    m.push_quad(p(-1., -1., 1.), p(1., -1., 1.), p(1., 1., 1.), p(-1., 1., 1.), color);
    m.push_quad(p(1., -1., -1.), p(-1., -1., -1.), p(-1., 1., -1.), p(1., 1., -1.), color);
    m
}

/// A (possibly tapered) prism from `start` to `end`. `r1 = 0` makes a cone.
#[allow(clippy::too_many_arguments)]
pub fn tube(
    m: &mut MeshData,
    start: Vec3,
    end: Vec3,
    r0: f32,
    r1: f32,
    segments: u32,
    phase: f32,
    color: Vec3,
    mut rng: Option<&mut Rng>,
) {
    let axis = (end - start).normalize_or(Vec3::Y);
    let u = axis.any_orthonormal_vector();
    // `u Ã— v = -axis` keeps the winding counter-clockwise from outside.
    let v = u.cross(axis);
    let ring = |center: Vec3, r: f32, i: u32| {
        let a = phase + i as f32 / segments as f32 * TAU;
        center + (u * a.cos() + v * a.sin()) * r
    };
    for i in 0..segments {
        let (a0, a1) = (ring(start, r0, i), ring(start, r0, i + 1));
        let c = shade(color, rng.as_deref_mut(), 0.06);
        if r1 <= 0.0 {
            m.push_triangle(a1, a0, end, c);
        } else {
            let (b0, b1) = (ring(end, r1, i), ring(end, r1, i + 1));
            m.push_quad(a1, a0, b0, b1, c);
            m.push_triangle(end, b1, b0, c);
        }
        m.push_triangle(start, a0, a1, c);
    }
}

/// [`tube`] for a level of detail: `lod` 0 is the full tube; higher levels halve
/// the sides per level (at least 4, or 3 for branches), widened so the outline stays as wide on
/// average, and drop thin tubes (twigs) altogether.
/// Either way `rng` advances as for the full tube, so whatever is generated
/// next comes out the same in every level.
#[allow(clippy::too_many_arguments)]
fn detail_tube(
    m: &mut MeshData,
    start: Vec3,
    end: Vec3,
    r0: f32,
    r1: f32,
    segments: u32,
    phase: f32,
    color: Vec3,
    rng: &mut Rng,
    lod: u32,
) {
    if lod == 0 {
        tube(m, start, end, r0, r1, segments, phase, color, Some(rng));
        return;
    }
    if r0 >= 0.03 {
        // Branches are thin enough that a triangle reads the same.
        let min_sides = if r0 < 0.1 { 3 } else { 4 };
        let sides = (segments >> lod).max(min_sides).min(segments);
        // A polygon's mean width is its perimeter over Ï€: keep that of the full tube.
        let perimeter = |n: u32| n as f32 * (PI / n as f32).sin();
        let widen = perimeter(segments) / perimeter(sides);
        tube(m, start, end, r0 * widen, r1 * widen, sides, phase, color, None);
    }
    for _ in 0..segments {
        shade(color, Some(rng), 0.06);
    }
}

/// Icosphere scaled by `radius` per axis, each vertex pushed out by `displace(dir)`.
pub fn blob(
    m: &mut MeshData,
    center: Vec3,
    radius: Vec3,
    subdivisions: u32,
    displace: impl Fn(Vec3) -> f32,
    color: Vec3,
    mut rng: Option<&mut Rng>,
) {
    let t = (1.0 + 5f32.sqrt()) / 2.0;
    let mut verts: Vec<Vec3> = [
        (-1., t, 0.),
        (1., t, 0.),
        (-1., -t, 0.),
        (1., -t, 0.),
        (0., -1., t),
        (0., 1., t),
        (0., -1., -t),
        (0., 1., -t),
        (t, 0., -1.),
        (t, 0., 1.),
        (-t, 0., -1.),
        (-t, 0., 1.),
    ]
    .iter()
    .map(|&(x, y, z)| Vec3::new(x, y, z).normalize())
    .collect();
    let mut faces: Vec<[usize; 3]> = vec![
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    for _ in 0..subdivisions {
        let mut cache = std::collections::HashMap::new();
        let mut mid = |a: usize, b: usize, verts: &mut Vec<Vec3>| {
            *cache.entry((a.min(b), a.max(b))).or_insert_with(|| {
                verts.push(((verts[a] + verts[b]) * 0.5).normalize());
                verts.len() - 1
            })
        };
        faces = faces
            .iter()
            .flat_map(|&[a, b, c]| {
                let ab = mid(a, b, &mut verts);
                let bc = mid(b, c, &mut verts);
                let ca = mid(c, a, &mut verts);
                [[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]]
            })
            .collect();
    }
    let positions: Vec<Vec3> = verts.iter().map(|&d| center + d * radius * displace(d)).collect();
    for [a, b, c] in faces {
        let col = shade(color, rng.as_deref_mut(), 0.08);
        m.push_triangle(positions[a], positions[b], positions[c], col);
    }
}

fn pine(seed: u64, tiers: u32, lod: u32) -> MeshData {
    let mut rng = Rng::new(seed);
    let mut m = MeshData::new();
    let bark = Vec3::new(0.27, 0.19, 0.14);
    detail_tube(&mut m, Vec3::ZERO, Vec3::new(0.0, 0.9, 0.0), 0.13, 0.09, 6, 0.0, bark, &mut rng, lod);

    let needle_dark = Vec3::new(0.12, 0.2, 0.15);
    let needle_light = Vec3::new(0.2, 0.3, 0.19);
    let lean = Vec3::new(rng.jitter(0.08), 0.0, rng.jitter(0.08));
    for k in 0..tiers {
        let f = k as f32 / tiers.max(1) as f32;
        let y0 = 0.55 + f * 2.0;
        let height = 1.15 - f * 0.45;
        let radius = (1.0 - f * 0.65) * rng.range(0.9, 1.1);
        let base = Vec3::new(0.0, y0, 0.0) + lean * f * 3.0;
        let tip = base + Vec3::new(rng.jitter(0.05), height, rng.jitter(0.05));
        let color = needle_dark.lerp(needle_light, f);
        let phase = rng.range(0.0, TAU);
        detail_tube(&mut m, base, tip, radius, 0.0, 7, phase, color, &mut rng, lod);
    }
    m
}

fn dead_tree(seed: u64, lod: u32) -> MeshData {
    let mut rng = Rng::new(seed);
    let mut m = MeshData::new();
    let wood = Vec3::new(0.24, 0.21, 0.2);
    let top = Vec3::new(rng.jitter(0.3), 2.4, rng.jitter(0.3));
    let bend = Vec3::new(rng.jitter(0.25), 1.2, rng.jitter(0.25));
    detail_tube(&mut m, Vec3::ZERO, bend, 0.2, 0.13, 5, 0.0, wood, &mut rng, lod);
    detail_tube(&mut m, bend, top, 0.13, 0.03, 5, 0.3, wood, &mut rng, lod);

    let branches = 3 + (rng.next_u64() % 3) as u32;
    for b in 0..branches {
        let t = rng.range(0.35, 0.9);
        let start = if t < 0.5 { bend * (t / 0.5) } else { bend.lerp(top, (t - 0.5) / 0.5) };
        let angle = b as f32 / branches as f32 * TAU + rng.jitter(0.6);
        let rise = rng.range(0.4, 1.1);
        let dir = Vec3::new(angle.cos(), rise, angle.sin()).normalize();
        let len = rng.range(0.5, 1.1) * (1.2 - t * 0.5);
        let end = start + dir * len;
        detail_tube(&mut m, start, end, 0.06, 0.012, 4, 0.0, wood, &mut rng, lod);
        if rng.f32() > 0.4 {
            let twig = end + (dir + Vec3::new(rng.jitter(0.8), 0.3, rng.jitter(0.8))).normalize() * len * 0.5;
            detail_tube(&mut m, end, twig, 0.02, 0.005, 3, 0.0, wood, &mut rng, lod);
        }
    }
    m
}

fn rock(seed: u64, lod: u32) -> MeshData {
    let mut rng = Rng::new(seed);
    let mut m = MeshData::new();
    let s = seed as u32;
    let squash = Vec3::new(rng.range(0.5, 0.65), rng.range(0.32, 0.45), rng.range(0.45, 0.6));
    let displace = move |d: Vec3| 0.8 + 0.45 * value_noise(d.x * 1.7 + d.y * 0.7 + 3.0, d.z * 1.7 - d.y * 1.1 + 5.0, s);
    // The coarse level keeps the 12 corner vertices of the fine one, so the outline barely changes.
    let subdivisions = if lod == 0 { 1 } else { 0 };
    let center = Vec3::new(0.0, squash.y * 0.6, 0.0);
    blob(&mut m, center, squash, subdivisions, displace, Vec3::new(0.36, 0.35, 0.37), Some(&mut rng));
    m
}

fn crystal(seed: u64) -> MeshData {
    let mut rng = Rng::new(seed);
    let mut m = MeshData::new();
    let color = Vec3::new(0.85, 0.9, 1.0);
    let count = 3 + (rng.next_u64() % 3) as u32;
    for i in 0..count {
        let main = i == 0;
        let angle = rng.range(0.0, TAU);
        let tilt = if main { rng.jitter(0.1) } else { rng.range(0.35, 0.8) };
        let dir = Vec3::new(angle.cos() * tilt.sin(), tilt.cos(), angle.sin() * tilt.sin()).normalize();
        let base = if main { Vec3::ZERO } else { Vec3::new(angle.cos(), 0.0, angle.sin()) * 0.15 };
        let length = if main { rng.range(1.3, 1.8) } else { rng.range(0.5, 1.0) };
        let radius = if main { 0.22 } else { rng.range(0.08, 0.14) };
        let body_end = base + dir * length * 0.75;
        let phase = rng.range(0.0, TAU);
        tube(&mut m, base - dir * 0.1, body_end, radius, radius, 6, phase, color, Some(&mut rng));
        tube(&mut m, body_end, base + dir * length, radius, 0.0, 6, phase, color, Some(&mut rng));
    }
    m
}

fn wanderer() -> MeshData {
    let mut rng = Rng::new(1);
    let mut m = MeshData::new();
    let cloak = Vec3::new(0.36, 0.11, 0.1);
    let hood = Vec3::new(0.3, 0.09, 0.09);
    let wood = Vec3::new(0.3, 0.22, 0.15);
    let v = Vec3::new;

    // Cloak and shoulders.
    tube(&mut m, v(0.0, 0.02, 0.0), v(0.0, 1.3, 0.0), 0.4, 0.2, 8, 0.2, cloak, Some(&mut rng));
    tube(&mut m, v(0.0, 1.3, 0.0), v(0.0, 1.42, 0.0), 0.2, 0.16, 8, 0.2, cloak, Some(&mut rng));
    // Hood, its pointed tip falling back, and the shadowed face.
    blob(&mut m, v(0.0, 1.56, 0.02), v(0.19, 0.21, 0.21), 1, |_| 1.0, hood, Some(&mut rng));
    tube(&mut m, v(0.0, 1.66, 0.08), v(0.0, 1.9, 0.34), 0.13, 0.0, 6, 0.0, hood, Some(&mut rng));
    blob(&mut m, v(0.0, 1.54, -0.13), v(0.11, 0.12, 0.06), 0, |_| 1.0, Vec3::splat(0.04), None);
    // Arms reaching forward.
    for side in [-1.0, 1.0] {
        tube(&mut m, v(0.2 * side, 1.34, 0.0), v(0.3 * side, 0.92, -0.16), 0.08, 0.06, 5, 0.0, cloak, Some(&mut rng));
    }
    // Staff in the right hand.
    tube(&mut m, v(0.33, 0.0, -0.2), v(0.33, 1.95, -0.2), 0.03, 0.025, 5, 0.0, wood, Some(&mut rng));
    m
}

fn pillar(seed: u64, broken: bool) -> MeshData {
    let mut rng = Rng::new(seed);
    let mut m = MeshData::new();
    let stone = Vec3::new(0.46, 0.44, 0.42);
    let plinth = cube(Vec3::new(0.0, 0.15, 0.0), Vec3::new(1.0, 0.3, 1.0), stone * 0.9);
    m.append(&plinth, Vec3::ZERO, Vec3::ONE);

    let height = if broken { rng.range(1.2, 2.4) } else { 3.0 };
    let column_top = Vec3::new(0.0, 0.3 + height, 0.0);
    tube(&mut m, Vec3::new(0.0, 0.3, 0.0), column_top, 0.36, 0.32, 8, 0.2, stone, Some(&mut rng));
    if broken {
        // A jagged cap: a short, crooked cone stands in for the fracture.
        let tip = column_top + Vec3::new(rng.jitter(0.15), rng.range(0.15, 0.35), rng.jitter(0.15));
        tube(&mut m, column_top, tip, 0.32, 0.0, 5, rng.range(0.0, TAU), stone * 0.85, Some(&mut rng));
    } else {
        let cap = cube(column_top + Vec3::new(0.0, 0.1, 0.0), Vec3::new(0.9, 0.2, 0.9), stone * 0.95);
        m.append(&cap, Vec3::ZERO, Vec3::ONE);
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prop_lods_are_lighter_with_the_same_outline() {
        for desc in [MeshDesc::Pine { seed: 3, tiers: 0 }, MeshDesc::DeadTree { seed: 2 }, MeshDesc::Rock { seed: 5 }] {
            let mesh = desc.build();
            let [lod] = &mesh.lods[..] else { panic!("{desc:?} should have one LOD") };
            assert!(lod.mesh.indices.len() * 10 <= mesh.indices.len() * 7, "{desc:?}: LOD isn't much lighter");
            let (full, coarse) = (mesh.bounding_sphere(), lod.mesh.bounding_sphere());
            assert!(full.center.distance(coarse.center) < 0.15 * full.radius, "{desc:?}: LOD moved");
            assert!((full.radius - coarse.radius).abs() < 0.15 * full.radius, "{desc:?}: LOD changed size");
        }
    }

    #[test]
    fn every_shape_builds() {
        let shapes = [
            MeshDesc::Cube,
            MeshDesc::Plane { size: 2.0 },
            MeshDesc::Cylinder { segments: 0 },
            MeshDesc::Cone { segments: 6 },
            MeshDesc::Sphere { subdivisions: 2 },
            MeshDesc::Pine { seed: 1, tiers: 0 },
            MeshDesc::DeadTree { seed: 2 },
            MeshDesc::Rock { seed: 3 },
            MeshDesc::Crystal { seed: 4 },
            MeshDesc::Pillar { seed: 5, broken: true },
            MeshDesc::Wanderer,
        ];
        for shape in shapes {
            let mesh = shape.build();
            assert!(!mesh.is_empty(), "{shape:?} is empty");
            assert!(mesh.vertices.iter().all(|v| v.position.iter().all(|c| c.is_finite())));
        }
    }

    #[test]
    fn cube_normals_point_outwards() {
        let mesh = MeshDesc::Cube.build();
        for v in &mesh.vertices {
            let from_center = Vec3::from(v.position) - Vec3::new(0.0, 0.5, 0.0);
            assert!(Vec3::from(v.normal).dot(from_center) > 0.0);
        }
    }

    #[test]
    fn tube_normals_point_outwards() {
        let mut mesh = MeshData::new();
        tube(&mut mesh, Vec3::ZERO, Vec3::new(0.3, 1.0, 0.2), 0.5, 0.4, 8, 0.0, Vec3::ONE, None);
        let axis = Vec3::new(0.3, 1.0, 0.2).normalize();
        for tri in mesh.vertices.chunks(3) {
            let centroid = tri.iter().map(|v| Vec3::from(v.position)).sum::<Vec3>() / 3.0;
            let mid_axis = axis * centroid.dot(axis);
            let normal = Vec3::from(tri[0].normal);
            let outward = (centroid - mid_axis).dot(normal) > -1e-4 || normal.dot(axis).abs() > 0.9;
            assert!(outward, "inward face at {centroid}");
        }
    }
}
