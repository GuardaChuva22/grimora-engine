//! Draws the macro map's answers as a PNG, one panel per layer: the top row is the world's
//! window, the bottom row a wide view around it. Every pixel asks `MacroMap::sample_map`, so
//! the image shows what the engine answers, not the source paintings.
//!
//!     cargo run -p world_map -- --world assets/world/island.ron [--at x,z] --screenshot out.png
//!
//! Panels: land and water, altitude band, biome, nation, water body. White cross: the start
//! (or `--at`); white dots: compendium points; white frame (bottom row): the window.

use anyhow::{Context, Result};
use glam::DVec2;
use grimora::EngineConfig;
use grimora_world::{MacroMap, MapSample};
use image::{Rgb, RgbImage};

const PANEL: u32 = 256;
const GAP: u32 = 4;
/// Map units across the wide view.
const WIDE: f64 = 400.0;

const SEA: [u8; 3] = [40, 70, 120];
const LAND: [u8; 3] = [90, 90, 80];

type Paint = fn(&MapSample) -> [u8; 3];

const PANELS: [Paint; 5] = [
    |s| if s.land { [70, 150, 80] } else { SEA },
    |s| s.band.map_or([0, 0, 0], |b| b.color),
    |s| s.biome.map_or(if s.land { [255, 0, 255] } else { SEA }, |b| b.color),
    |s| s.nation.map_or(if s.land { LAND } else { SEA }, |n| n.color),
    |s| s.water.map_or(LAND, |w| w.color),
];

fn main() -> Result<()> {
    let config = EngineConfig::default().with_cli()?;
    let world = config.world.context("--world <file.ron> is needed")?;
    let out = config.screenshot.unwrap_or_else(|| "screenshots/world_map.png".into());
    let map = MacroMap::load(&world)?;
    let at = config.at.unwrap_or(map.desc.start);
    let window = map.window();

    let s = map.sample_map(at);
    println!(
        "[world] at {:.2},{:.2}: {} | band {} | biome {} | nation {} | water {}",
        at.x,
        at.y,
        if s.land { "land" } else { "water" },
        s.band.map_or("-", |b| &b.name),
        s.biome.map_or("-", |b| &b.name),
        s.nation.map_or("-", |n| &n.name),
        s.water.map_or("-", |w| if w.name.is_empty() { &w.id } else { &w.name }),
    );
    for (p, d) in map.points_near_map(at, 100.0).iter().take(5) {
        println!("[world] near: {} ({}) {:.0} km", p.name, p.kind, d * grimora_world::METERS_PER_UNIT / 1000.0);
    }

    let columns = PANELS.len() as u32;
    let mut img = RgbImage::new(columns * (PANEL + GAP) - GAP, 2 * PANEL + GAP);
    let views =
        [(DVec2::new(window.x, window.y), window.width.max(window.height)), (window.center() - WIDE * 0.5, WIDE)];
    for (row, &(corner, span)) in views.iter().enumerate() {
        let units_per_px = span / f64::from(PANEL);
        let y0 = row as u32 * (PANEL + GAP);
        for py in 0..PANEL {
            for px in 0..PANEL {
                let p = corner + (DVec2::new(f64::from(px), f64::from(py)) + 0.5) * units_per_px;
                let s = map.sample_map(p);
                for (i, paint) in PANELS.iter().enumerate() {
                    img.put_pixel(i as u32 * (PANEL + GAP) + px, y0 + py, Rgb(paint(&s)));
                }
            }
        }
        let to_px = |p: DVec2| ((p - corner) / units_per_px).floor();
        for i in 0..columns {
            let x0 = i * (PANEL + GAP);
            let mut dot = |p: DVec2, color: [u8; 3]| {
                let q = to_px(p);
                if q.x >= 0.0 && q.y >= 0.0 && q.x < f64::from(PANEL) && q.y < f64::from(PANEL) {
                    img.put_pixel(x0 + q.x as u32, y0 + q.y as u32, Rgb(color));
                }
            };
            for point in &map.points {
                dot(point.position(), [255, 255, 255]);
            }
            if row == 1 {
                let (a, b) =
                    (DVec2::new(window.x, window.y), DVec2::new(window.x + window.width, window.y + window.height));
                let n = 64;
                for k in 0..=n {
                    let t = f64::from(k) / f64::from(n);
                    for p in [DVec2::new(a.x + (b.x - a.x) * t, a.y), DVec2::new(a.x + (b.x - a.x) * t, b.y)] {
                        dot(p, [255, 255, 255]);
                    }
                    for p in [DVec2::new(a.x, a.y + (b.y - a.y) * t), DVec2::new(b.x, a.y + (b.y - a.y) * t)] {
                        dot(p, [255, 255, 255]);
                    }
                }
            }
            for k in -4..=4 {
                let d = f64::from(k) * units_per_px;
                let color = if k == 0 { [255, 0, 0] } else { [255, 255, 255] };
                dot(at + DVec2::new(d, 0.0), color);
                dot(at + DVec2::new(0.0, d), color);
            }
        }
    }
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    img.save(&out).with_context(|| format!("writing {}", out.display()))?;
    println!("[world] wrote {}", out.display());
    Ok(())
}
