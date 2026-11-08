//! The island's fresh water: streams that only go down, end somewhere, and are dug into the
//! ground the tiles are made of.

use std::path::PathBuf;
use std::time::Instant;

use glam::DVec2;
use std::collections::HashMap;

use grimora_world::{MacroMap, Relief, StreamEnd, TileKey, tile_heights};

fn island() -> Relief {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/world/island.ron");
    Relief::new(MacroMap::load(path).expect("island loads"))
}

#[test]
fn streams_run_downhill_into_the_sea_or_a_pond() {
    let started = Instant::now();
    let relief = island();
    let water = relief.water();
    let took = started.elapsed();
    let mut to_sea = 0;
    for (s, stream) in water.streams.iter().enumerate() {
        let points = &stream.points;
        let length = (points.len() - 1) as f64 * grimora_world::hydro_step();
        println!(
            "stream {s}: {:.0} m, {:.1} -> {:.1} m, half {:.1}..{:.1}, {:.2} m/s, {:?}",
            length,
            points[0].at.y,
            points[points.len() - 1].at.y,
            points[0].half_width,
            points[points.len() - 1].half_width,
            stream.speed,
            stream.end
        );
        assert!(points.len() >= 2, "stream {s} has no course");
        for pair in points.windows(2) {
            assert!(pair[1].at.y <= pair[0].at.y + 1e-9, "stream {s} climbs: {:?} then {:?}", pair[0].at, pair[1].at);
        }
        assert_ne!(stream.end, StreamEnd::Edge, "the island's streams stay on the island");
        if let StreamEnd::Stream(into) = stream.end {
            assert!(into < s, "a stream runs into one found before it");
            let last = points[points.len() - 1].at;
            let joined = water.streams[into]
                .points
                .iter()
                .map(|p| DVec2::new(p.at.x - last.x, p.at.z - last.z).length())
                .fold(f64::MAX, f64::min);
            assert!(joined < 60.0, "stream {s} ends {joined} m from the one it joins");
        }
        if stream.end == StreamEnd::Sea {
            to_sea += 1;
            assert!(points[points.len() - 1].at.y < 0.2, "stream {s} meets the sea at its level");
        }
        // The water lies in its bed: the dug ground is under it along the whole course.
        for p in points.iter().step_by(5) {
            let ground = relief.height_at(p.at.x, p.at.z);
            // Under the sea a mouth's channel shoals onto the sea floor.
            let shoaling = stream.end == StreamEnd::Sea && p.at.y < 0.0;
            assert!(
                ground < p.at.y - if shoaling { 0.25 } else { 0.5 },
                "stream {s}: ground {ground} over water {}",
                p.at.y
            );
            assert!(relief.is_bed(p.at.x, p.at.z));
        }
    }
    for (i, pond) in water.ponds.iter().enumerate() {
        println!("pond {i}: r {:.0} m at {:.1} m", pond.radius, pond.level);
        let ground = relief.height_at(pond.center.x, pond.center.y);
        assert!(ground < pond.level - 1.0, "pond {i} has a bottom under its water");
    }
    println!("{} streams ({to_sea} to the sea), {} ponds in {took:?}", water.streams.len(), water.ponds.len());
    assert!(water.streams.len() >= 3, "a few streams on the island");
    assert!(to_sea >= 2, "streams reach the coast");
}

#[test]
fn the_same_seed_finds_the_same_water() {
    let (a, b) = (island(), island());
    assert_eq!(a.water().streams, b.water().streams);
    assert_eq!(a.water().ponds, b.water().ponds);
}

/// Streams lie in their valleys: the ground just past the banks is close to the water, so
/// there are no trenches, and the water never stands over the ground beside it.
#[test]
fn streams_run_in_shallow_beds() {
    let relief = island();
    let mut cuts = Vec::new();
    let mut floods = 0usize;
    let mut centers = Vec::new();
    for stream in &relief.water().streams {
        for pair in stream.points.windows(2).step_by(3) {
            let (a, b) = (pair[0], pair[1]);
            let dir = DVec2::new(b.at.x - a.at.x, b.at.z - a.at.z).normalize();
            let side = DVec2::new(-dir.y, dir.x);
            centers.push(relief.raw_height_at(a.at.x, a.at.z) - a.at.y);
            for sign in [-1.0, 1.0] {
                let p = DVec2::new(a.at.x, a.at.z) + side * sign * (a.half_width + 4.0);
                let ground = relief.height_at(p.x, p.y);
                cuts.push(ground - a.at.y);
                if ground < a.at.y + 0.05 && a.at.y > 1.0 {
                    floods += 1;
                }
            }
        }
    }
    centers.sort_by(f64::total_cmp);
    let c = |q: f64| centers[((centers.len() - 1) as f64 * q) as usize];
    println!("raw ground over the water at the middle: median {:.2}, 90% {:.2}, 99% {:.2}", c(0.5), c(0.9), c(0.99));
    cuts.sort_by(f64::total_cmp);
    let at = |q: f64| cuts[((cuts.len() - 1) as f64 * q) as usize];
    println!(
        "bank 4 m out over the water: median {:.2}, 90% {:.2}, 99% {:.2}, most {:.2}; {floods} of {} low",
        at(0.5),
        at(0.9),
        at(0.99),
        at(1.0),
        cuts.len()
    );
    assert!(at(0.9) < 2.0, "trenches: 90% of banks under {:.2} m", at(0.9));
    assert!(floods * 100 < cuts.len(), "water over its banks at {floods} places");
}

#[test]
#[ignore]
fn where_streams_cut() {
    let relief = island();
    for (s, stream) in relief.water().streams.iter().enumerate() {
        let n = stream.points.len();
        let mut run = String::new();
        for (i, p) in stream.points.iter().enumerate().step_by(n / 40 + 1) {
            let cut = relief.raw_height_at(p.at.x, p.at.z) - p.at.y;
            run += &format!("{:.0}:{:+.1} ", i as f64 / n as f64 * 100.0, cut);
        }
        println!("stream {s} {:?}: {run}", stream.end);
    }
}

/// The ground as a tile draws it at a point: the two triangles of its grid cell.
fn drawn_height(tiles: &mut HashMap<TileKey, Vec<f32>>, relief: &Relief, level: u8, p: DVec2) -> f64 {
    let size = TileKey { level, x: 0, z: 0 }.size();
    let key = TileKey { level, x: (p.x / size).floor() as i64, z: (p.y / size).floor() as i64 };
    let heights = tiles.entry(key).or_insert_with(|| tile_heights(relief, key));
    let (n, cell) = (key.cells() + 1, key.cell());
    let local = (p - key.min()) / cell;
    let (i, j) = ((local.x.floor() as usize).min(n - 2), (local.y.floor() as usize).min(n - 2));
    let (fx, fz) = (local.x - i as f64, local.y - j as f64);
    let h = |i: usize, j: usize| f64::from(heights[j * n + i]);
    let (a, b, c, d) = (h(i, j), h(i, j + 1), h(i + 1, j + 1), h(i + 1, j));
    if fz >= fx { a + fx * (c - b) + fz * (b - a) } else { a + fx * (d - a) + fz * (c - d) }
}

#[test]
fn the_drawn_ground_stays_under_every_stream() {
    let relief = island();
    let mut tiles = HashMap::new();
    let mut worst = (f64::MIN, String::new());
    let mut over = [0; 4];
    let mut checked = 0;
    for (s, stream) in relief.water().streams.iter().enumerate() {
        for (i, pair) in stream.points.windows(2).enumerate() {
            let (a, b) = (pair[0], pair[1]);
            let dir = (DVec2::new(b.at.x, b.at.z) - DVec2::new(a.at.x, a.at.z)).normalize_or_zero();
            let side = DVec2::new(-dir.y, dir.x);
            for across in [0.0, -0.6, 0.6] {
                let p = DVec2::new(a.at.x, a.at.z) + side * a.half_width * across;
                for level in 0..=3u8 {
                    // A coarse cell can't draw a fall: next to one its ground is a ramp, and
                    // the falling water is drawn down the face.
                    let cell = TileKey { level, x: 0, z: 0 }.cell();
                    let reach = (cell * 1.5 / 4.0).ceil() as usize;
                    let (lo, hi) = (i.saturating_sub(reach), (i + reach).min(stream.points.len() - 1));
                    if level > 0 && stream.points[lo].at.y - stream.points[hi].at.y > cell * 0.6 {
                        continue;
                    }
                    let ground = drawn_height(&mut tiles, &relief, level, p);
                    let above = ground - (a.at.y - 0.03);
                    checked += 1;
                    if above > 0.0 {
                        over[level as usize] += 1;
                    }
                    if above > worst.0 {
                        worst = (above, format!("stream {s} point {i} across {across} level {level}"));
                    }
                }
            }
        }
    }
    println!("{checked} samples, {over:?} by level with ground over the water; worst {:.3} m at {}", worst.0, worst.1);
    assert_eq!(over, [0; 4], "the ground shows through a stream: worst {:.3} m at {}", worst.0, worst.1);
}

#[test]
fn the_village_stands_on_flat_dry_ground_for_a_reason() {
    let (a, b) = (island(), island());
    let (va, vb) = (a.village().expect("a village"), b.village().expect("a village"));
    assert_eq!((va.id, va.center), (vb.id, vb.center));
    println!("village at {:.0}, {:.0}: {}", va.center.x, va.center.y, va.reason);
    assert!(!a.is_bed(va.center.x, va.center.y));
    for k in 0..8 {
        let t = k as f64 * std::f64::consts::TAU / 8.0;
        let p = va.center + DVec2::new(t.cos(), t.sin()) * 30.0;
        assert!((a.height_at(p.x, p.y) - va.level).abs() < 0.1);
    }
}
