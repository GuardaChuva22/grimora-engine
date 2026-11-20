//! Fishing from the bank: where to stand, facing the water, and the line and float that follow
//! the `fish` clip (wait, cast and reel, wait, catch), so the cast lands and the catch comes out.

use std::collections::HashMap;

use glam::{DVec2, DVec3, Quat, Vec3};
use grimora_core::{MeshData, Transform};
use grimora_world::Relief;
use hecs::Entity;

use super::Folk;
use super::life::Tool;
use crate::world::{Renderable, World};

/// Meters from the water's edge that a fisher stands, on dry ground.
const STAND: f64 = 0.6;
/// Along the bank from the boat (meters), one place to fish each.
const ALONG: [f64; 3] = [-3.5, 3.5, 7.0];
/// The farthest a cast goes (meters from where one stands).
const FARTHEST: f64 = 7.0;
/// How far a near water point is past the edge, so the float doesn't sit on the bank.
const NEAR_PAST: f64 = 0.8;

/// The `fish` clip, as `tools/import_village_clips.py` chains it: idle, idle, cast and reel,
/// idle, catch. Seconds.
const LOOP: f32 = 21.083;
const CAST: f32 = 7.133;
/// Within cast and reel: the rod goes back, whips forward, the float flies, and the reel.
const BACK: (f32, f32) = (0.5, 1.7);
const WHIP: f32 = 1.85;
const LANDS: f32 = 2.6;
const REEL: (f32, f32) = (4.4, 8.33);
/// The catch: the rod comes up (the fish out of the water), holds, and goes down again.
const CATCH: f32 = 19.033;
const YANK: (f32, f32) = (0.35, 0.8);
const LOWER: (f32, f32) = (1.35, 2.05);
/// Before the catch, the float twitches: something bites.
const BITES: f32 = 17.6;

/// How the line hangs: pieces, and the sag of a slack one (meters per meter of span).
const PIECES: usize = 5;
const SAG: f32 = 0.06;
const LINE_WIDTH: f32 = 0.012;
/// How far under the tip the float hangs out of the water.
const HANG: f32 = 0.45;

/// A place to fish from: where to stand, which way the water is and how far it goes.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ShoreLine {
    pub stand: DVec3,
    /// Flat, toward the water.
    pub toward: DVec2,
    /// Meters from `stand` to a float near the bank, and to the farthest cast.
    pub near: f64,
    pub far: f64,
    pub level: f64,
}

impl ShoreLine {
    fn water(&self, meters: f64) -> DVec3 {
        let p = DVec2::new(self.stand.x, self.stand.z) + self.toward * meters;
        DVec3::new(p.x, self.level, p.y)
    }
}

/// A line out: its pieces, its float and since when it's been shown.
pub(crate) struct Line {
    pieces: Vec<Entity>,
    float: Entity,
    shown: f32,
}

/// The places along the bank by the village's boat.
pub(crate) fn shore_lines(relief: &Relief, boat: DVec2) -> Vec<ShoreLine> {
    let out = |p: DVec2| relief.water().fresh_at(p, 0.0, 20.0).map(|f| f.out);
    let toward = |p: DVec2| -> Option<DVec2> {
        let h = 0.4;
        let gx = out(p + DVec2::X * h)? - out(p - DVec2::X * h)?;
        let gz = out(p + DVec2::Y * h)? - out(p - DVec2::Y * h)?;
        // `out` grows away from the water.
        let g = -DVec2::new(gx, gz);
        (g.length() > 1e-4).then(|| g.normalize())
    };
    // Onto the line `STAND` meters out of the water.
    let to_bank = |mut p: DVec2| -> Option<DVec2> {
        for _ in 0..8 {
            let o = out(p)?;
            if (o - STAND).abs() < 0.05 {
                break;
            }
            p += toward(p)? * (o - STAND);
        }
        Some(p)
    };
    let Some(start) = to_bank(boat) else { return Vec::new() };
    let Some(first) = toward(start) else { return Vec::new() };
    let along = first.perp();
    let mut lines = Vec::new();
    for offset in ALONG {
        let Some(stand) = to_bank(start + along * offset) else { continue };
        let Some(dir) = toward(stand) else { continue };
        // Across the water: where it starts and where it ends again.
        let (mut wet, mut dry) = (None, None);
        let mut t = 0.0;
        while t < FARTHEST + 2.0 {
            let under = out(stand + dir * t).is_some_and(|o| o < -0.2);
            if under && wet.is_none() {
                wet = Some(t);
            } else if !under && wet.is_some() {
                dry = Some(t);
                break;
            }
            t += 0.25;
        }
        let Some(wet) = wet else { continue };
        let near = wet + NEAR_PAST;
        let far = (dry.unwrap_or(FARTHEST + 2.0) - NEAR_PAST).min(FARTHEST).max(near + 0.5);
        let p = stand + dir * near;
        let Some(level) = relief.water().fresh_at(p, 0.0, 20.0).map(|f| f.level) else { continue };
        let ground = relief.height_at(stand.x, stand.y);
        lines.push(ShoreLine { stand: DVec3::new(stand.x, ground, stand.y), toward: dir, near, far, level });
    }
    lines
}

/// Where the float is, by the time in the clip.
enum Float {
    /// On the water, this far out, maybe pulled under a little.
    Water(f64, f32),
    /// Hanging under the rod's tip.
    Tip,
    /// From `a` to `b` (each `None` for the tip, else meters out), `s` 0..1 along, `arc`
    /// meters high in the middle; `taut` with no sag.
    Moving { a: Option<f64>, b: Option<f64>, s: f32, arc: f32 },
}

fn float_at(t: f32, line: &ShoreLine) -> Float {
    let (near, far) = (line.near, line.far);
    let into = |t: f32, (a, b): (f32, f32)| ((t - a) / (b - a)).clamp(0.0, 1.0);
    let c = t - CAST;
    let k = t - CATCH;
    if c < BACK.0 {
        Float::Water(near, 0.0)
    } else if c < BACK.1 {
        Float::Moving { a: Some(near), b: None, s: into(c, BACK), arc: 0.6 }
    } else if c < WHIP {
        Float::Tip
    } else if c < LANDS {
        Float::Moving { a: None, b: Some(far), s: into(c, (WHIP, LANDS)), arc: 1.4 }
    } else if c < REEL.0 {
        Float::Water(far, 0.0)
    } else if c < REEL.1 {
        let s = into(c, REEL);
        Float::Water(far + (near - far) * (s * s * (3.0 - 2.0 * s)) as f64, 0.0)
    } else if t < BITES {
        Float::Water(near, 0.0)
    } else if k < YANK.0 {
        // Twitches, then the strike.
        let dip = ((t - BITES) * 9.0).sin().max(0.0) * 0.04 + into(t, (CATCH - 0.4, CATCH + YANK.0)) * 0.08;
        Float::Water(near, dip)
    } else if k < YANK.1 {
        Float::Moving { a: Some(near), b: None, s: into(k, YANK), arc: 0.3 }
    } else if k < LOWER.0 {
        Float::Tip
    } else {
        Float::Moving { a: None, b: Some(near), s: into(k, LOWER), arc: 0.5 }
    }
}

impl World {
    /// A line and float for everyone fishing with a rod in hand, gone when they stop.
    pub(crate) fn update_fishing(&mut self, folk: &mut Folk, dt: f32) {
        let origin = self.origin();
        let mut fishing = HashMap::new();
        for (i, person) in folk.people.iter().enumerate() {
            let (Some(entity), Some((rod, Tool::Rod))) = (person.entity, person.life.tool) else { continue };
            let Ok(animator) = self.ecs.get::<&grimora_core::Animator>(entity) else { continue };
            if animator.current() != Some("fish") {
                continue;
            }
            let t = animator.time().rem_euclid(LOOP);
            drop(animator);
            let Some(here) = self.transform(entity).map(|t| origin.to_world(t.position)) else { continue };
            let Some(shore) = folk
                .shore
                .iter()
                .min_by(|a, b| a.stand.distance_squared(here).total_cmp(&b.stand.distance_squared(here)))
            else {
                continue;
            };
            fishing.insert(i, (rod, t, *shore));
        }
        // Lines of those who stopped go away.
        let gone: Vec<usize> = folk.lines.keys().copied().filter(|i| !fishing.contains_key(i)).collect();
        for i in gone {
            if let Some(line) = folk.lines.remove(&i) {
                for e in line.pieces.into_iter().chain([line.float]) {
                    let _ = self.ecs.despawn(e);
                }
            }
        }
        if fishing.is_empty() {
            return;
        }
        let (thread, bob) = *folk.line_meshes.get_or_insert_with(|| (self.add_mesh(thread()), self.add_mesh(float())));

        for (i, (rod, t, shore)) in fishing {
            let Some(rod_at) = self.transform(rod) else { continue };
            let mesh = self.get::<Renderable>(rod).map(|r| r.mesh.0);
            let tip_local = mesh
                .and_then(|m| self.bounds_of(m))
                .map_or(Vec3::Y * 2.0, |(lo, hi)| Vec3::new((lo.x + hi.x) * 0.5, hi.y, (lo.z + hi.z) * 0.5));
            let tip = rod_at.matrix().transform_point3(tip_local);
            let water = |m: f64| origin.to_local(shore.water(m));
            let hang = tip - Vec3::Y * HANG;
            let line = folk.lines.entry(i).or_insert_with(|| Line {
                pieces: (0..PIECES).map(|_| self.spawn(thread, Transform::IDENTITY)).collect(),
                float: self.spawn(bob, Transform::IDENTITY),
                shown: 0.0,
            });
            line.shown += dt;
            let mut state = float_at(t, &shore);
            // Just out: dropped from the tip into the water, not appearing there.
            if line.shown < 0.7
                && let Float::Water(m, _) = state
            {
                state = Float::Moving { a: None, b: Some(m), s: line.shown / 0.7, arc: 0.3 };
            }
            let bobbing = (line.shown * 2.1).sin() * 0.012;
            let (float_at, slack) = match state {
                Float::Water(m, dip) => (water(m) + Vec3::Y * (0.01 + bobbing - dip), true),
                Float::Tip => (hang, false),
                Float::Moving { a, b, s, arc } => {
                    let end = |m: Option<f64>| m.map_or(hang, water);
                    let p = end(a).lerp(end(b), s) + Vec3::Y * arc * 4.0 * s * (1.0 - s);
                    (p, false)
                }
            };
            let sag = if slack { SAG * tip.distance(float_at) } else { 0.0 };
            let point = |s: f32| tip.lerp(float_at, s) - Vec3::Y * sag * 4.0 * s * (1.0 - s);
            for (k, &piece) in line.pieces.iter().enumerate() {
                let (a, b) = (point(k as f32 / PIECES as f32), point((k + 1) as f32 / PIECES as f32));
                let span = b - a;
                let length = span.length().max(1e-4);
                let transform = Transform {
                    position: a,
                    rotation: Quat::from_rotation_arc(Vec3::Y, span / length),
                    scale: Vec3::new(LINE_WIDTH, length, LINE_WIDTH),
                };
                if let Ok(mut at) = self.ecs.get::<&mut Transform>(piece) {
                    *at = transform;
                }
            }
            if let Ok(mut at) = self.ecs.get::<&mut Transform>(line.float) {
                *at = Transform::from_position(float_at);
            }
        }
    }
}

/// A unit of line: a thin square tube from y 0 to 1, one wide.
fn thread() -> MeshData {
    let mut mesh = MeshData::new();
    let color = Vec3::new(0.82, 0.8, 0.72);
    let corner = |k: usize, y: f32| {
        let a = k as f32 * std::f32::consts::FRAC_PI_2;
        Vec3::new(a.cos() * 0.5, y, a.sin() * 0.5)
    };
    for k in 0..4 {
        let (a0, a1) = (corner(k, 0.0), corner((k + 1) % 4, 0.0));
        let (b0, b1) = (corner(k, 1.0), corner((k + 1) % 4, 1.0));
        mesh.push_triangle(a0, b1, a1, color);
        mesh.push_triangle(a0, b0, b1, color);
    }
    mesh.shadowless = true;
    mesh
}

/// The float: a small diamond, red over white.
fn float() -> MeshData {
    let mut mesh = MeshData::new();
    let (red, white) = (Vec3::new(0.78, 0.12, 0.08), Vec3::new(0.92, 0.9, 0.82));
    let r = 0.035;
    let (top, bottom) = (Vec3::Y * r * 1.3, -Vec3::Y * r);
    let ring = |k: usize| {
        let a = k as f32 * std::f32::consts::FRAC_PI_2;
        Vec3::new(a.cos() * r, 0.0, a.sin() * r)
    };
    for k in 0..4 {
        let (a, b) = (ring(k), ring((k + 1) % 4));
        mesh.push_triangle(top, b, a, red);
        mesh.push_triangle(bottom, a, b, white);
    }
    mesh.shadowless = true;
    mesh
}
