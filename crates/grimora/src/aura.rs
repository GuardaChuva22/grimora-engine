//! Auras on bodies: [`World::set_aura`] lights one (see `grimora_scene::AuraDesc`) on an
//! entity. The body itself changes: its own meshes (and what it wears) are drawn again a
//! hair off the skin with a living pattern (`Shading::Aura`), and what flows off it is born
//! on points of the moving skin ([`SkinPoint`]). Putting it out fades the patterns and lets
//! the particles alive burn away.

use glam::{Quat, Vec3};
use std::sync::Arc;

use grimora_core::{AlphaMode, AuraLook, AuraPattern, Material, MeshData, ParticleSystem, Rng, Shading, Transform};
use grimora_scene::AuraDesc;
use hecs::Entity;

use crate::world::{MeshId, Renderable, World, Worn};

/// An emitter following `owner`: its feet (turned only around up), or a joint.
#[derive(Debug, Clone)]
pub struct Aura {
    pub owner: Entity,
    pub joint: Option<String>,
    /// Burning out: it no longer counts as the owner's aura.
    pub dying: bool,
    /// Where on the skin its particles are born, chosen once.
    pub points: Arc<[SkinPoint]>,
}

/// A fixed place on a skinned mesh: a triangle and where in it.
#[derive(Debug, Clone, Copy)]
pub struct SkinPoint {
    pub mesh: MeshId,
    pub corners: [u32; 3],
    pub weights: Vec3,
}

/// One of `owner`'s meshes drawn again with an aura's pattern, with its skin (it is also
/// [`Worn`]).
/// Its `Renderable::emissive` is how lit it is, which fades in and out.
#[derive(Debug, Clone, Copy)]
pub struct AuraSkin {
    pub owner: Entity,
    pub dying: bool,
}

/// The box a blazing aura is marched in, around its owner's feet (see `AuraPattern::Blaze`).
#[derive(Debug, Clone, Copy)]
pub struct BlazeBox {
    pub owner: Entity,
}

/// The box around a body that a blaze burns in: half its width, and its bottom and top
/// from the feet.
const BLAZE_HALF: f32 = 1.8;
const BLAZE_BOTTOM: f32 = -0.3;
const BLAZE_TOP: f32 = 3.5;
/// Most capsules a body is made of, as in the renderer.
const FIRE_CAPSULES: usize = 16;
/// How far back the flames stream for each m/s the body moves, at most, and how fast the
/// trail follows a change of pace (per second).
const TRAIL_PER_SPEED: f32 = 0.16;
const LONGEST_TRAIL: f32 = 0.9;
const TRAIL_EASE: f32 = 6.0;
/// Skin points a bone needs nearest to it to count.
const LEAST_POINTS: u32 = 3;

const HANDS: [&str; 2] = ["hand.R", "hand.L"];
/// Seconds the patterns take to light and to go out.
const FADE_IN: f32 = 0.35;
const FADE_OUT: f32 = 0.6;
/// How many places on the skin particles come from: enough that they look born anywhere.
const SKIN_POINTS: usize = 384;

/// `mesh` with an aura's pattern: the same vertices and skin, one material.
fn skin_of(mesh: &MeshData, look: AuraLook) -> MeshData {
    let mut painted = mesh.clone();
    painted.morphs.clear();
    painted.lods.clear();
    painted.collision = None;
    painted.shadowless = true;
    painted.wind = 0.0;
    painted.materials = vec![Material {
        name: Some("aura".into()),
        shading: Shading::Aura,
        alpha_mode: AlphaMode::Blend,
        aura: Some(look),
        ..Default::default()
    }];
    for submesh in &mut painted.submeshes {
        submesh.material = 0;
    }
    painted
}

/// The box a blaze is drawn on, with outward faces.
fn blaze_box(look: AuraLook) -> MeshData {
    let (lo, hi) = (Vec3::new(-BLAZE_HALF, BLAZE_BOTTOM, -BLAZE_HALF), Vec3::new(BLAZE_HALF, BLAZE_TOP, BLAZE_HALF));
    let c = |x: usize, y: usize, z: usize| Vec3::new([lo.x, hi.x][x], [lo.y, hi.y][y], [lo.z, hi.z][z]);
    let mut mesh = MeshData::new();
    // Each face counterclockwise seen from outside.
    let faces = [
        [c(1, 0, 0), c(1, 1, 0), c(1, 1, 1), c(1, 0, 1)],
        [c(0, 0, 1), c(0, 1, 1), c(0, 1, 0), c(0, 0, 0)],
        [c(0, 1, 0), c(0, 1, 1), c(1, 1, 1), c(1, 1, 0)],
        [c(0, 0, 1), c(0, 0, 0), c(1, 0, 0), c(1, 0, 1)],
        [c(1, 0, 1), c(1, 1, 1), c(0, 1, 1), c(0, 0, 1)],
        [c(0, 0, 0), c(0, 1, 0), c(1, 1, 0), c(1, 0, 0)],
    ];
    for [a, b, d, e] in faces {
        mesh.push_triangle(a, b, d, Vec3::ONE);
        mesh.push_triangle(a, d, e, Vec3::ONE);
    }
    skin_of(&mesh, look)
}

impl World {
    /// Lights `aura` on `owner`, putting out the one it had. `None` only puts it out.
    pub fn set_aura(&mut self, owner: Entity, aura: Option<&AuraDesc>) {
        let mine: Vec<Entity> = self
            .ecs
            .query::<(Entity, &Aura)>()
            .iter()
            .filter(|(_, a)| a.owner == owner && !a.dying)
            .map(|(e, _)| e)
            .collect();
        for entity in mine {
            if let Ok(mut a) = self.ecs.get::<&mut Aura>(entity) {
                a.dying = true;
            }
            if let Ok(mut system) = self.ecs.get::<&mut ParticleSystem>(entity) {
                system.emitting = false;
                system.despawn_when_done = true;
            }
        }
        for skin in self.ecs.query_mut::<&mut AuraSkin>() {
            if skin.owner == owner {
                skin.dying = true;
            }
        }
        let Some(aura) = aura else { return };
        let Some(feet) = self.aura_place(owner, None) else { return };
        self.add_skins(owner, &aura.skin_looks());
        let seed = owner.to_bits().get() ^ self.ecs.len() as u64;
        let points: Arc<[SkinPoint]> = self.skin_points(owner, seed).into();
        let mut body = ParticleSystem::new(aura.effect(), Vec3::ZERO, 1.0, seed);
        body.body = self.place_skin(owner, &points);
        self.ecs.spawn((feet, body, Aura { owner, joint: None, dying: false, points }));
        if let Some(hand) = aura.hand_effect() {
            for (i, joint) in HANDS.iter().enumerate() {
                let Some(at) = self.aura_place(owner, Some(joint)) else { continue };
                let system = ParticleSystem::new(hand.clone(), Vec3::ZERO, 1.0, seed ^ (i as u64 + 1) * 7919);
                let aura = Aura { owner, joint: Some((*joint).to_string()), dying: false, points: Arc::new([]) };
                self.ecs.spawn((at, system, aura));
            }
        }
        if let Some(burst) = aura.burst_effect() {
            let entity = self.play_effect(&burst, feet.position);
            if let Ok(mut t) = self.ecs.get::<&mut Transform>(entity) {
                t.rotation = feet.rotation;
            }
        }
    }

    /// Every look painted over `owner`'s mesh and over each piece it wears.
    fn add_skins(&mut self, owner: Entity, looks: &[AuraLook]) {
        if looks.is_empty() {
            return;
        }
        let meshes = self.body_meshes(owner);
        let transform = self.transform(owner).unwrap_or_default();
        for (layer, look) in looks.iter().enumerate() {
            if look.pattern != AuraPattern::Blaze {
                continue;
            }
            let painted = blaze_box(*look);
            let id = match self.aura_meshes.get(&(usize::MAX, layer)) {
                Some(&id) => {
                    self.replace_mesh(id, painted);
                    id
                }
                None => {
                    let id = self.add_mesh(painted);
                    self.aura_meshes.insert((usize::MAX, layer), id);
                    id
                }
            };
            let at = self.aura_place(owner, None).unwrap_or(transform);
            let renderable = Renderable { emissive: 0.0, ..Renderable::new(id) };
            self.ecs.spawn((at, renderable, BlazeBox { owner }, AuraSkin { owner, dying: false }));
        }
        for mesh in meshes {
            for (layer, look) in looks.iter().enumerate() {
                if look.pattern == AuraPattern::Blaze {
                    continue;
                }
                let Some(source) = self.mesh(mesh) else { continue };
                if source.skeleton.is_none() && source.vertices.is_empty() {
                    continue;
                }
                let painted = skin_of(source, *look);
                let id = match self.aura_meshes.get(&(mesh.0, layer)) {
                    Some(&id) => {
                        self.replace_mesh(id, painted);
                        id
                    }
                    None => {
                        let id = self.add_mesh(painted);
                        self.aura_meshes.insert((mesh.0, layer), id);
                        id
                    }
                };
                let renderable = Renderable { emissive: 0.0, ..Renderable::new(id) };
                self.ecs.spawn((transform, renderable, Worn { parent: owner }, AuraSkin { owner, dying: false }));
            }
        }
    }

    /// True while `owner` has a lit aura.
    pub fn has_aura(&self, owner: Entity) -> bool {
        self.ecs.query::<&Aura>().iter().any(|a| a.owner == owner && !a.dying)
    }

    /// Where an aura of `owner` sits: its feet turned by its heading only, or `joint`.
    fn aura_place(&self, owner: Entity, joint: Option<&str>) -> Option<Transform> {
        if let Some(joint) = joint {
            let at = self.joint_transform(owner, joint)?;
            return Some(Transform { scale: Vec3::ONE, ..at });
        }
        let t = *self.ecs.get::<&Transform>(owner).ok()?;
        let forward = t.rotation * Vec3::Z;
        let yaw = forward.x.atan2(forward.z);
        Some(Transform { position: t.position, rotation: Quat::from_rotation_y(yaw), scale: Vec3::ONE })
    }

    /// The meshes making `owner`'s body: its own and what it wears.
    fn body_meshes(&self, owner: Entity) -> Vec<MeshId> {
        let mut meshes: Vec<MeshId> = self.ecs.get::<&Renderable>(owner).map(|r| vec![r.mesh]).unwrap_or_default();
        meshes.extend(
            self.ecs
                .query::<(&Worn, &Renderable, Option<&AuraSkin>)>()
                .iter()
                .filter(|(worn, _, skin)| worn.parent == owner && skin.is_none())
                .map(|(_, r, _)| r.mesh),
        );
        meshes
    }

    /// [`SKIN_POINTS`] places spread evenly over the surface of `owner`'s body, by area.
    fn skin_points(&self, owner: Entity, seed: u64) -> Vec<SkinPoint> {
        let mut triangles: Vec<(MeshId, [u32; 3], f32)> = Vec::new();
        for mesh in self.body_meshes(owner) {
            let Some(data) = self.mesh(mesh) else { continue };
            for corners in data.indices.chunks_exact(3) {
                let [a, b, c] = [corners[0], corners[1], corners[2]];
                let at = |i: u32| Vec3::from(data.vertices[i as usize].position);
                let area = (at(b) - at(a)).cross(at(c) - at(a)).length() * 0.5;
                if area > 0.0 {
                    triangles.push((mesh, [a, b, c], area));
                }
            }
        }
        let total: f32 = triangles.iter().map(|t| t.2).sum();
        if triangles.is_empty() || total <= 0.0 {
            return Vec::new();
        }
        let mut rng = Rng::new(seed);
        let mut sums = Vec::with_capacity(triangles.len());
        let mut sum = 0.0;
        for t in &triangles {
            sum += t.2;
            sums.push(sum);
        }
        (0..SKIN_POINTS)
            .map(|_| {
                let pick = rng.range(0.0, total);
                let (mesh, corners, _) = triangles[sums.partition_point(|&s| s < pick).min(triangles.len() - 1)];
                let (mut u, mut v) = (rng.range(0.0, 1.0), rng.range(0.0, 1.0));
                if u + v > 1.0 {
                    (u, v) = (1.0 - u, 1.0 - v);
                }
                SkinPoint { mesh, corners, weights: Vec3::new(1.0 - u - v, u, v) }
            })
            .collect()
    }

    /// Where `points` are on `owner`'s skin now, in the world, with the skin's normal.
    fn place_skin(&self, owner: Entity, points: &[SkinPoint]) -> Vec<(Vec3, Vec3)> {
        let mut query = self.ecs.query_one::<(&Transform, &grimora_core::Animator)>(owner);
        let Ok((transform, animator)) = query.get() else { return Vec::new() };
        let model = transform.matrix();
        let matrices = animator.skinning_matrices();
        let mut out = Vec::with_capacity(points.len());
        for point in points {
            let Some(data) = self.mesh(point.mesh) else { continue };
            let (mut at, mut normal) = (Vec3::ZERO, Vec3::ZERO);
            for (corner, share) in point.corners.iter().zip(point.weights.to_array()) {
                let vertex = &data.vertices[*corner as usize];
                let total: f32 = vertex.weights.iter().map(|&w| f32::from(w)).sum();
                let skin = if total > 0.0 {
                    let mut m = glam::Mat4::ZERO;
                    for (&joint, &weight) in vertex.joints.iter().zip(&vertex.weights) {
                        if let Some(j) = matrices.get(joint as usize) {
                            m += *j * (f32::from(weight) / total);
                        }
                    }
                    m
                } else {
                    glam::Mat4::IDENTITY
                };
                let full = model * skin;
                at += full.transform_point3(Vec3::from(vertex.position)) * share;
                normal += full.transform_vector3(Vec3::from(vertex.normal)) * share;
            }
            out.push((at, normal.normalize_or(Vec3::Y)));
        }
        out
    }

    /// Keeps every aura on its owner; one whose owner is gone burns out where it is. The
    /// engine calls this with the attachments, after the animations.
    pub fn update_auras(&mut self) {
        let boxes: Vec<(Entity, Entity)> =
            self.ecs.query::<(Entity, &BlazeBox)>().iter().map(|(e, b)| (e, b.owner)).collect();
        let mut burning = None;
        for (entity, owner) in boxes {
            if let Some(at) = self.aura_place(owner, None) {
                if let Ok(mut t) = self.ecs.get::<&mut Transform>(entity) {
                    *t = at;
                }
                burning.get_or_insert(owner);
            }
        }
        let auras: Vec<(Entity, Aura)> =
            self.ecs.query::<(Entity, &Aura)>().iter().map(|(e, a)| (e, a.clone())).collect();
        self.environment.fire_body.clear();
        if let Some(owner) = burning {
            let points =
                auras.iter().find(|(_, a)| a.owner == owner && a.joint.is_none()).map(|(_, a)| a.points.clone());
            let points: Arc<[SkinPoint]> = match points {
                Some(p) if !p.is_empty() => p,
                _ => self.skin_points(owner, 7).into(),
            };
            let skin = self.place_skin(owner, &points);
            self.environment.fire_body = self.body_capsules(owner, &skin);
            if let Some(feet) = self.aura_place(owner, None) {
                self.environment.fire_anchor = feet.position;
            }
        }
        for (entity, aura) in auras {
            let skin = if aura.points.is_empty() { Vec::new() } else { self.place_skin(aura.owner, &aura.points) };
            match self.aura_place(aura.owner, aura.joint.as_deref()) {
                Some(at) => {
                    if let Ok(mut t) = self.ecs.get::<&mut Transform>(entity) {
                        *t = at;
                    }
                    if let Ok(mut system) = self.ecs.get::<&mut ParticleSystem>(entity) {
                        system.body = skin;
                    }
                }
                None => {
                    if let Ok(mut system) = self.ecs.get::<&mut ParticleSystem>(entity) {
                        system.emitting = false;
                        system.despawn_when_done = true;
                    }
                }
            }
        }
    }

    /// `owner`'s body as capsules along its bones, as thick as the skin around each: every
    /// point of `skin` belongs to the nearest bone, and a bone is as wide as its points are
    /// far from it on average. The bones with most skin come first.
    fn body_capsules(&self, owner: Entity, skin: &[(Vec3, Vec3)]) -> Vec<(Vec3, Vec3, f32)> {
        let mut query = self.ecs.query_one::<(&Transform, &grimora_core::Animator)>(owner);
        let Ok((transform, animator)) = query.get() else { return Vec::new() };
        let model = transform.matrix();
        let skeleton = animator.skeleton();
        let at: Vec<Vec3> = skeleton
            .joints
            .iter()
            .zip(animator.skinning_matrices())
            .map(|(joint, skin)| (model * *skin * joint.inverse_bind.inverse()).w_axis.truncate())
            .collect();
        let named = |j: usize| skeleton.joints[j].name.as_str();
        let bones: Vec<(Vec3, Vec3)> = skeleton
            .joints
            .iter()
            .enumerate()
            .filter_map(|(j, joint)| {
                let parent = joint.parent?;
                let (a, b) = (at.get(parent)?, at.get(j)?);
                (a.distance(*b) > 0.02 && named(j) != "root" && named(parent) != "root").then_some((*a, *b))
            })
            .collect();
        if bones.is_empty() {
            return Vec::new();
        }
        let away = |p: Vec3, (a, b): (Vec3, Vec3)| {
            let ab = b - a;
            let h = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
            p.distance(a + ab * h)
        };
        let mut sums = vec![(0.0f32, 0u32); bones.len()];
        for (point, _) in skin {
            let (best, d) = bones
                .iter()
                .enumerate()
                .map(|(i, &bone)| (i, away(*point, bone)))
                .fold((0, f32::MAX), |best, x| if x.1 < best.1 { x } else { best });
            sums[best].0 += d;
            sums[best].1 += 1;
        }
        let mut capsules: Vec<(u32, (Vec3, Vec3, f32))> = bones
            .iter()
            .zip(&sums)
            .filter(|(_, s)| s.1 >= LEAST_POINTS)
            .map(|(&(a, b), &(sum, n))| (n, (a, b, sum / n as f32 * 0.9)))
            .collect();
        capsules.sort_by(|x, y| y.0.cmp(&x.0));
        capsules.into_iter().take(FIRE_CAPSULES).map(|(_, c)| c).collect()
    }

    /// Fades the painted patterns in, and out when put out, removing them once dark.
    pub(crate) fn fade_auras(&mut self, dt: f32) {
        // The burning body's pace, eased, streams its flames back.
        let anchor = self.environment.fire_anchor;
        let (last, trail) = self.fire_trail;
        if dt > 0.0 && !self.environment.fire_body.is_empty() {
            let moved = if last.distance(anchor) > 3.0 { glam::Vec3::ZERO } else { anchor - last };
            let wanted = (-moved / dt * TRAIL_PER_SPEED).clamp_length_max(LONGEST_TRAIL);
            let eased = trail + (wanted - trail) * (1.0 - (-TRAIL_EASE * dt).exp());
            self.fire_trail = (anchor, eased);
            self.environment.fire_trail = eased;
        } else {
            self.fire_trail = (anchor, trail);
        }
        let mut dark = Vec::new();
        for (entity, skin, renderable) in self.ecs.query_mut::<(Entity, &AuraSkin, &mut Renderable)>() {
            if skin.dying {
                renderable.emissive -= dt / FADE_OUT;
                if renderable.emissive <= 0.0 {
                    dark.push(entity);
                }
            } else {
                renderable.emissive = (renderable.emissive + dt / FADE_IN).min(1.0);
            }
        }
        for entity in dark {
            let _ = self.ecs.despawn(entity);
        }
    }
}
