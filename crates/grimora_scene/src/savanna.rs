//! Savanna shapes: flat-crowned acacias, bottle-trunked baobabs and termite mounds. Like the
//! other generators, every level of detail draws the same random numbers, so what a coarse
//! level keeps is where the fine one has it.

use std::f32::consts::TAU;

use grimora_core::{AlphaMode, Material, MeshData, Shading, Submesh, Vec2, Vec3};

use crate::rng::Rng;
use crate::shapes::{blob, branch_card, tube};
use crate::texture::{Picture, apply_texture};

const BARK_COLOR: Vec3 = Vec3::new(0.27, 0.19, 0.14);

/// A limb as a chain of tubes through `points`, from radius `r0` to `r1`.
fn limb(m: &mut MeshData, points: &[Vec3], r0: f32, r1: f32, sides: u32, color: Vec3) {
    let n = points.len().saturating_sub(1).max(1) as f32;
    for (k, pair) in points.windows(2).enumerate() {
        let (a, b) = (r0 + (r1 - r0) * k as f32 / n, r0 + (r1 - r0) * (k + 1) as f32 / n);
        // Each piece starts a little back inside the last, so the joints don't open.
        let back = (pair[1] - pair[0]).normalize_or(Vec3::Y) * a * 0.5;
        tube(m, pair[0] - back, pair[1], a, b, sides, k as f32 * 0.7, color, None);
    }
}

/// The wood: textured by `bark` (or plain bark color) as the first submesh.
fn finish_wood(m: &mut MeshData, bark: Option<&Picture>, tile: f32) {
    if let Some(bark) = bark {
        apply_texture(m, bark, tile, Vec3::ONE);
    } else {
        m.materials = vec![Default::default()];
    }
    m.submeshes.push(Submesh { indices: 0..m.indices.len() as u32, material: 0 });
}

/// The leaves: one submesh per picture, cut out and lit like foliage.
fn finish_leaves(m: &mut MeshData, cards: &[(Picture, Vec2)], indices: Vec<Vec<u32>>) {
    for ((picture, _), indices) in cards.iter().zip(indices) {
        if indices.is_empty() {
            continue;
        }
        let start = m.indices.len() as u32;
        m.indices.extend(indices);
        m.submeshes.push(Submesh { indices: start..m.indices.len() as u32, material: m.materials.len() });
        m.materials.push(Material {
            double_sided: true,
            alpha_mode: AlphaMode::Mask { cutoff: 0.5 },
            shading: Shading::Foliage,
            ..picture.material()
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn card(
    m: &mut MeshData,
    indices: &mut [Vec<u32>],
    cards: &[(Picture, Vec2)],
    which: usize,
    start: Vec3,
    along: Vec3,
    up: Vec3,
    length: f32,
) {
    let (picture, stem) = &cards[which];
    let first = m.vertices.len() as u32;
    branch_card(m, picture, *stem, start, along, up, length);
    indices[which].extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
}

/// A flat-topped acacia about 6 units tall: a trunk that forks low into limbs leaning out, and
/// on their ends a crown of branch pictures lying nearly flat, layered, wider than tall.
pub fn acacia(seed: u64, cards: &[(Picture, Vec2)], bark: Option<&Picture>, lod: u32) -> MeshData {
    let mut rng = Rng::new(seed);
    let mut m = MeshData::new();
    let wood = if bark.is_some() { Vec3::splat(0.55) } else { BARK_COLOR };
    let sides = (6 >> lod).max(3);
    let height = rng.range(5.2, 6.6);
    let spread = rng.range(3.6, 5.0);
    let lean = Vec3::new(rng.jitter(0.6), 0.0, rng.jitter(0.6));
    let fork = Vec3::new(0.0, rng.range(1.4, 2.4), 0.0) + lean * 0.4;
    limb(&mut m, &[Vec3::ZERO, lean * 0.15 + Vec3::Y * fork.y * 0.5, fork], 0.3, 0.2, sides, wood);

    // Limbs out to the crown, each forking once near its end.
    let limbs = 2 + (rng.next_u64() % 3) as u32;
    let first = rng.range(0.0, TAU);
    let mut tips = Vec::new();
    for k in 0..limbs {
        let angle = first + k as f32 / limbs as f32 * TAU + rng.jitter(0.5);
        let out = Vec3::new(angle.cos(), 0.0, angle.sin());
        let reach = spread * rng.range(0.35, 0.6);
        let top = fork + out * reach + Vec3::Y * (height - fork.y - rng.range(0.4, 0.9));
        let bend = fork.lerp(top, 0.5) + out * rng.range(0.1, 0.5) - Vec3::Y * rng.range(0.0, 0.4);
        limb(&mut m, &[fork, bend, top], 0.17, 0.09, sides, wood);
        for side in [-1.0f32, 1.0] {
            let twist = angle + side * rng.range(0.35, 0.8);
            let dir = Vec3::new(twist.cos(), rng.range(0.15, 0.45), twist.sin()).normalize();
            let end = top + dir * spread * rng.range(0.25, 0.4);
            if lod < 2 {
                limb(&mut m, &[top, end], 0.07, 0.03, sides.min(4), wood);
            }
            tips.push(end);
        }
        tips.push(top);
    }
    finish_wood(&mut m, bark, 1.2);

    // The crown: cards from every tip out over the top, lying almost flat, in two or three
    // layers so it has some depth from the side and holes from below.
    let mut indices = vec![Vec::new(); cards.len()];
    let center = tips.iter().copied().sum::<Vec3>() / tips.len().max(1) as f32;
    let crown_y = tips.iter().map(|t| t.y).fold(f32::MIN, f32::max);
    for (t, &tip) in tips.iter().enumerate() {
        let count = 12;
        let start_angle = rng.range(0.0, TAU);
        for c in 0..count {
            let angle = start_angle + c as f32 / count as f32 * TAU + rng.jitter(0.4);
            let out = Vec3::new(angle.cos(), 0.0, angle.sin());
            // Mostly outward from the middle of the crown, so it spreads wide.
            let away = (Vec3::new(tip.x - center.x, 0.0, tip.z - center.z)).normalize_or_zero();
            let flat = (out + away * 0.8).normalize_or(out);
            let droop = rng.range(-0.18, 0.08);
            let along = (flat + Vec3::Y * droop).normalize();
            let length = spread * rng.range(0.42, 0.66);
            let layer = rng.range(-0.15, 0.8);
            let roll = rng.jitter(0.35);
            let which = (rng.next_u64() % cards.len().max(1) as u64) as usize;
            if (t * count + c) % (1 << lod) != 0 || cards.is_empty() {
                continue;
            }
            // The picture's top faces up (a flat card), turned a little about its length.
            let side = along.cross(Vec3::Y).normalize_or(Vec3::X);
            let up = side * roll.cos() + Vec3::Y * roll.sin();
            let start = Vec3::new(tip.x, crown_y.min(tip.y + 0.4) + layer, tip.z) - along * 0.3;
            card(&mut m, &mut indices, cards, which, start, along, up, length * (1.0 + 0.3 * lod as f32));
        }
    }
    finish_leaves(&mut m, cards, indices);
    m.wind = 0.12 / (height * height);
    m
}

/// A baobab about 7 units tall: a fat bottle of a trunk, short thick limbs spreading on top
/// like roots, and a few tufts of leaf pictures at their ends.
pub fn baobab(seed: u64, cards: &[(Picture, Vec2)], bark: Option<&Picture>, lod: u32) -> MeshData {
    let mut rng = Rng::new(seed);
    let mut m = MeshData::new();
    let wood = if bark.is_some() { Vec3::splat(0.62) } else { Vec3::new(0.42, 0.36, 0.33) };
    let sides = (12 >> lod).max(6);
    let height = rng.range(5.5, 7.5);
    let girth = rng.range(1.1, 1.6);
    let lean = Vec3::new(rng.jitter(0.3), 0.0, rng.jitter(0.3));
    // The bottle: wide low, widest a third up, narrowing to the crown.
    let profile = [(0.0, 1.05), (0.12, 1.12), (0.35, 1.0), (0.6, 0.78), (0.82, 0.55), (1.0, 0.42)];
    let at = |f: f32| Vec3::Y * f * height + lean * f * f;
    for pair in profile.windows(2) {
        let ((f0, r0), (f1, r1)) = (pair[0], pair[1]);
        tube(&mut m, at(f0), at(f1), r0 * girth, r1 * girth, sides, f0 * 3.0, wood, None);
    }
    let top = at(1.0);
    let limbs = 5 + (rng.next_u64() % 4) as u32;
    let first = rng.range(0.0, TAU);
    let mut tips = Vec::new();
    for k in 0..limbs {
        let angle = first + k as f32 / limbs as f32 * TAU + rng.jitter(0.4);
        let out = Vec3::new(angle.cos(), 0.0, angle.sin());
        let base = top - Vec3::Y * rng.range(0.0, 0.5) + out * girth * 0.25;
        let rise = rng.range(0.5, 1.1);
        let length = rng.range(1.6, 3.0);
        let end = base + (out + Vec3::Y * rise).normalize() * length;
        let bend = base.lerp(end, 0.5) + Vec3::Y * rng.jitter(0.3);
        limb(&mut m, &[base, bend, end], girth * 0.22, 0.07, (sides / 2).max(4), wood);
        // A crooked twig or two out of each limb end.
        for _ in 0..2 {
            let dir = (out + Vec3::new(rng.jitter(0.9), rng.range(0.3, 0.9), rng.jitter(0.9))).normalize();
            let twig = end + dir * rng.range(0.6, 1.3);
            if lod == 0 {
                limb(&mut m, &[end, twig], 0.06, 0.02, 3, wood);
            }
            tips.push(twig);
        }
    }
    finish_wood(&mut m, bark, 1.6);

    let mut indices = vec![Vec::new(); cards.len()];
    for (t, &tip) in tips.iter().enumerate() {
        for c in 0..3 {
            let angle = rng.range(0.0, TAU);
            let along = Vec3::new(angle.cos(), rng.range(-0.2, 0.5), angle.sin()).normalize();
            let upright = (Vec3::Y - along * along.y).normalize();
            let roll = rng.jitter(1.2);
            let up = upright * roll.cos() + along.cross(upright) * roll.sin();
            let length = rng.range(0.9, 1.5);
            let which = (rng.next_u64() % cards.len().max(1) as u64) as usize;
            if (t * 3 + c) % (1 << lod) != 0 || cards.is_empty() {
                continue;
            }
            card(&mut m, &mut indices, cards, which, tip - along * 0.2, along, up, length * (1.0 + 0.3 * lod as f32));
        }
    }
    finish_leaves(&mut m, cards, indices);
    m.wind = 0.05 / (height * height);
    m
}

/// A termite mound about 2 units tall: a lumpy earthen spire with smaller ones leaning on it.
/// Its color comes from the texture it is given (or plain red earth).
pub fn mound(seed: u64, lod: u32) -> MeshData {
    let mut rng = Rng::new(seed);
    let mut m = MeshData::new();
    let earth = Vec3::new(0.55, 0.32, 0.2);
    let sides = if lod == 0 { 9 } else { 5 };
    let spires = 1 + (rng.next_u64() % 3) as u32;
    for k in 0..spires {
        let main = k == 0;
        let height = if main { rng.range(1.8, 2.8) } else { rng.range(0.7, 1.5) };
        let width = if main { rng.range(0.55, 0.75) } else { rng.range(0.3, 0.45) };
        let angle = rng.range(0.0, TAU);
        let out = Vec3::new(angle.cos(), 0.0, angle.sin());
        let foot = if main { Vec3::ZERO } else { out * rng.range(0.45, 0.75) };
        let lean = out * if main { rng.range(0.0, 0.15) } else { rng.range(0.15, 0.35) };
        // A swollen foot, then rings narrowing unevenly to a blunt, crooked top.
        let rings = [(0.0, 1.25), (0.15, 1.0), (0.38, 0.78), (0.6, 0.6), (0.82, 0.38), (1.0, 0.16)];
        let mut last: Option<(Vec3, f32)> = None;
        for &(f, r) in &rings {
            let wobble = Vec3::new(rng.jitter(0.08), 0.0, rng.jitter(0.08)) * height;
            let at = foot - Vec3::Y * 0.15 + Vec3::Y * f * height + lean * f * f * height + wobble * f;
            let r = r * width * rng.range(0.88, 1.12);
            if let Some((from, r0)) = last {
                let phase = rng.range(0.0, TAU);
                tube(&mut m, from, at, r0, r, sides, phase, earth, Some(&mut rng));
            }
            last = Some((at, r));
        }
        // Lumps on the side of the big one.
        if main {
            for _ in 0..3 {
                let a = rng.range(0.0, TAU);
                let up = rng.range(0.1, 0.45) * height;
                let at = Vec3::new(a.cos(), 0.0, a.sin()) * width * 0.75 + Vec3::Y * up;
                let size = Vec3::splat(width * rng.range(0.35, 0.55));
                blob(&mut m, at, size, 0, |_| 1.0, earth, Some(&mut rng));
            }
        }
    }
    m
}
