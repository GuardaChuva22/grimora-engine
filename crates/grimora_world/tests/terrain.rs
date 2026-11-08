use std::path::PathBuf;
use std::time::Instant;

use glam::{DVec2, DVec3};
use grimora_world::{
    MacroMap, Relief, TileKey, TileLook, build_tile, map_to_world, mesh_hash, tile_heights, tiles_around,
};

fn island() -> Relief {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/world/island.ron");
    Relief::new(MacroMap::load(path).expect("island loads"))
}

fn start(relief: &Relief) -> DVec2 {
    let p = map_to_world(relief.map().desc.start);
    DVec2::new(p.x, p.z)
}

#[test]
fn the_start_is_land_and_the_sea_around_is_sea() {
    let relief = island();
    let at = start(&relief);
    let h = relief.height_at(at.x, at.y);
    assert!(h > 5.0, "start at {h} m");
    // 20 km out, in every direction, is open sea.
    for (dx, dz) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
        let h = relief.height_at(at.x + dx * 20_000.0, at.y + dz * 20_000.0);
        assert!(h < -10.0, "{dx},{dz}: {h} m");
    }
}

#[test]
fn tiles_are_the_same_in_any_order_and_thread() {
    let relief = island();
    let at = start(&relief);
    let keys = tiles_around(DVec3::new(at.x, 50.0, at.y), 2.0, 16_000.0);
    assert!(keys.len() > 20 && keys.len() < 400, "{} tiles", keys.len());
    let look = TileLook { tile: 5.0, ..Default::default() };
    let some: Vec<TileKey> = keys.iter().copied().step_by(keys.len() / 12).collect();
    let forward: Vec<u64> = some.iter().map(|&k| mesh_hash(&build_tile(&relief, k, &look))).collect();
    let backward: Vec<u64> = std::thread::scope(|s| {
        let (relief, look) = (&relief, &look);
        let handles: Vec<_> =
            some.iter().rev().map(|&k| s.spawn(move || mesh_hash(&build_tile(relief, k, look)))).collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let backward: Vec<u64> = backward.into_iter().rev().collect();
    assert_eq!(forward, backward);
}

#[test]
fn neighbors_share_their_edge() {
    let relief = island();
    let at = start(&relief);
    let a = TileKey::at(at);
    let b = TileKey { x: a.x + 1, ..a };
    let (ha, hb) = (tile_heights(&relief, a), tile_heights(&relief, b));
    let n = a.cells() + 1;
    for j in 0..n {
        assert_eq!(ha[j * n + a.cells()], hb[j * n], "row {j}");
    }
    // A coarser tile's edge vertices are on the finer one's.
    let coarse = TileKey { level: 1, x: a.x.div_euclid(2), z: a.z.div_euclid(2) };
    let hc = tile_heights(&relief, coarse);
    let fine = coarse.children()[0];
    let hf = tile_heights(&relief, fine);
    let step = (coarse.cell() / fine.cell()) as usize;
    for i in 0..=coarse.cells() / 2 {
        assert_eq!(hc[i], hf[i * step]);
    }
}

#[test]
fn the_view_is_covered_once() {
    let at = DVec3::new(9_443_000.0, 30.0, 3_546_000.0);
    let keys = tiles_around(at, 30.0, 12_000.0);
    // Sampled points within the view fall in exactly one tile.
    for k in 0..200 {
        let angle = k as f64 * 2.399;
        let r = (k as f64 / 200.0).sqrt() * 11_000.0;
        let p = DVec2::new(at.x + angle.cos() * r, at.z + angle.sin() * r);
        let inside = keys.iter().filter(|&&key| {
            key.distance(p) == 0.0 && {
                let max = key.min() + DVec2::splat(key.size());
                p.x < max.x && p.y < max.y
            }
        });
        assert_eq!(inside.count(), 1, "{p}");
    }
}

/// `cargo test --release -p grimora_world --test terrain cost -- --nocapture`
#[test]
fn cost() {
    let relief = island();
    let at = start(&relief);
    let look = TileLook { tile: 5.0, ..Default::default() };
    let keys = tiles_around(DVec3::new(at.x, 50.0, at.y), 2.0, 16_000.0);
    let started = Instant::now();
    let mut vertices = 0;
    for &k in &keys {
        vertices += build_tile(&relief, k, &look).vertices.len();
    }
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    println!("{} tiles, {vertices} vertices, {ms:.1} ms ({:.2} ms per tile)", keys.len(), ms / keys.len() as f64);
    let mut by_level = [0; 9];
    for k in &keys {
        by_level[k.level as usize] += 1;
    }
    println!("by level: {by_level:?}");
    let h = relief.height_at(at.x, at.y);
    let mut highest = f64::MIN;
    for j in -40..=40 {
        for i in -40..=40 {
            highest = highest.max(relief.height_at(at.x + i as f64 * 150.0, at.y + j as f64 * 150.0));
        }
    }
    println!("start {h:.1} m, highest within 6 km {highest:.0} m");
}
