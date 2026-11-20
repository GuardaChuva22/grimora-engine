//! The island's plants and animals: placed the same for the same seed, never in the water, and
//! thicker along it.

use std::path::PathBuf;
use std::time::Instant;

use glam::DVec2;
use grimora_world::{FloraKey, LifeBook, MacroMap, Relief, fauna_places, flora_around, place_plants};

fn assets() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/world")
}

fn island() -> Relief {
    Relief::new(MacroMap::load(assets().join("island.ron")).expect("island loads"))
}

fn book() -> LifeBook {
    LifeBook::load(&assets().join("life.ron")).expect("life loads")
}

fn plant(book: &LifeBook, name: &str) -> u16 {
    book.plants.iter().position(|p| p.name == name).expect("the plant is in the book") as u16
}

/// A point by a stream, well inland.
fn by_a_stream(relief: &Relief) -> DVec2 {
    let stream = relief.water().streams.iter().max_by_key(|s| s.points.len()).expect("a stream");
    let at = stream.points[stream.points.len() / 3].at;
    DVec2::new(at.x, at.z)
}

#[test]
fn the_same_seed_places_the_same_plants_in_any_order() {
    let (a, b) = (island(), island());
    let book = book();
    let mut keys = flora_around(&book, by_a_stream(&a));
    let started = Instant::now();
    let first: Vec<_> = keys.iter().map(|&k| place_plants(&a, &book, k)).collect();
    println!("{} cells in {:.0} ms", keys.len(), started.elapsed().as_secs_f64() * 1000.0);
    keys.reverse();
    let second: Vec<_> = std::thread::scope(|s| {
        let handles: Vec<_> = keys
            .iter()
            .map(|&k| {
                s.spawn({
                    let (b, book) = (&b, &book);
                    move || place_plants(b, book, k)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let second: Vec<_> = second.into_iter().rev().collect();
    assert_eq!(first, second);
    assert!(first.iter().map(Vec::len).sum::<usize>() > 1000, "a savanna has plants");
}

#[test]
fn nothing_grows_in_the_water_or_on_the_beach() {
    let relief = island();
    let book = book();
    let water = relief.water();
    let mut eyes = vec![by_a_stream(&relief)];
    eyes.extend(water.ponds.iter().map(|p| p.center));
    eyes.extend(grimora_world::coast_places(&relief, 3000.0).iter().map(|(at, _)| *at));
    let mut checked = 0;
    for eye in eyes {
        for key in flora_around(&book, eye) {
            let desc = &book.plants[key.plant as usize];
            for spot in place_plants(&relief, &book, key) {
                let (x, z) = (spot.at.x, spot.at.z);
                assert!(!relief.is_bed(x, z), "{} in a stream or pond at {x:.0}, {z:.0}", desc.name);
                assert!(water.fresh_distance(x, z) >= desc.water.gap, "{} too near the water", desc.name);
                assert!(water.coast_distance(x, z) > 0.0, "{} in the sea", desc.name);
                assert!(relief.height_at(x, z) >= desc.above, "{} on the beach at {x:.0}, {z:.0}", desc.name);
                checked += 1;
            }
        }
    }
    println!("{checked} plants checked");
    assert!(checked > 1000);
}

#[test]
fn trees_and_reeds_crowd_the_water() {
    let relief = island();
    let book = book();
    let water = relief.water();
    let count = |name: &str, eye: DVec2| {
        let plant = plant(&book, name);
        let (mut near, mut far) = (0usize, 0usize);
        for key in flora_around(&book, eye).into_iter().filter(|k| k.plant == plant) {
            for spot in place_plants(&relief, &book, key) {
                if water.fresh_distance(spot.at.x, spot.at.z) < 40.0 { near += 1 } else { far += 1 }
            }
        }
        (near, far)
    };
    let eye = by_a_stream(&relief);
    let (near, far) = count("juncos", eye);
    println!("reeds: {near} by the water, {far} away");
    assert!(near > 50 && far == 0, "reeds only by the water");
    // Trees: per square meter of ground near the water against the rest, over a whole cell grid.
    let plant = plant(&book, "acacia");
    let (mut near, mut far, mut near_area, mut far_area) = (0.0, 0.0, 0.0, 0.0);
    for key in flora_around(&book, eye).into_iter().filter(|k| k.plant == plant) {
        let cell = book.plants[plant as usize].cell();
        for i in 0..16 {
            for j in 0..16 {
                let p =
                    (DVec2::new(key.x as f64, key.z as f64) + DVec2::new(i as f64 + 0.5, j as f64 + 0.5) / 16.0) * cell;
                if water.coast_distance(p.x, p.y) <= 0.0 || relief.height_at(p.x, p.y) < 4.5 {
                    continue;
                }
                if water.fresh_distance(p.x, p.y) < 40.0 { near_area += 1.0 } else { far_area += 1.0 }
            }
        }
        for spot in place_plants(&relief, &book, key) {
            if water.fresh_distance(spot.at.x, spot.at.z) < 40.0 { near += 1.0 } else { far += 1.0 }
        }
    }
    let ratio = (near / near_area) / (far / far_area);
    println!("trees: {near} near, {far} far, {ratio:.1} times as thick by the water");
    assert!(ratio > 2.5);
}

#[test]
fn a_cell_is_its_own() {
    // A cell's copies don't depend on what else is asked for.
    let relief = island();
    let book = book();
    let key = FloraKey { plant: plant(&book, "capim"), ..flora_around(&book, by_a_stream(&relief))[0] };
    let alone = place_plants(&relief, &book, key);
    let s = by_a_stream(&relief);
    println!(
        "stream at {:.1}, {:.1}, ground {:.1}",
        s.x - 4476.0 * grimora_world::METERS_PER_UNIT,
        s.y - 1681.0 * grimora_world::METERS_PER_UNIT,
        relief.height_at(s.x, s.y)
    );
    for other in flora_around(&book, by_a_stream(&relief)).into_iter().take(20) {
        place_plants(&relief, &book, other);
    }
    assert_eq!(alone, place_plants(&relief, &book, key));
}

#[test]
fn animals_live_at_sea_and_in_the_deep_ponds() {
    let relief = island();
    let book = book();
    let places = fauna_places(&relief, &book);
    let water = relief.water();
    for place in &places {
        let coast = water.coast_distance(place.at.x, place.at.z);
        let species = place.species.file_stem().unwrap().to_string_lossy();
        println!("{} at {:.0}, {:.0}: {coast:.0} m from the coast", place.name, place.at.x, place.at.z);
        if place.name.contains("pond") {
            assert!(relief.is_bed(place.at.x, place.at.z), "{} not in its pond", place.name);
        } else if species == "caranguejo" {
            assert!(coast.abs() < 120.0, "crabs on the shore");
        } else {
            assert!(coast < 0.0, "{} at sea", place.name);
        }
    }
    assert!(places.iter().any(|p| p.name.starts_with("gaivota")));
    assert!(places.iter().any(|p| p.name.starts_with("sardinha")));
    assert_eq!(places, fauna_places(&island(), &book), "same seed, same places");
}

#[test]
fn the_sea_floor_goes_down_in_steps_and_things_grow_on_it() {
    let (relief, book) = (island(), book());
    let start = glam::DVec2::new(4476.0, 1681.0) * grimora_world::METERS_PER_UNIT;
    // The nearest way out to sea from the start, and a point 300 m off that shore.
    let mut found = None;
    'far: for step in 1..400 {
        for turn in 0..16 {
            let dir = glam::DVec2::from_angle(turn as f64 * std::f64::consts::TAU / 16.0);
            let p = start + dir * (step as f64 * 50.0);
            if relief.shore_at(p.x, p.y) < -300.0 {
                found = Some((p, dir));
                break 'far;
            }
        }
    }
    let (reef, out) = found.expect("there is sea around the island");
    let unit = reef / grimora_world::METERS_PER_UNIT;
    println!("reef: --at {:.4},{:.4}", unit.x, unit.y);
    let depth = |p: glam::DVec2| -relief.height_at(p.x, p.y);
    for off in [0.0, 100.0, 300.0, 600.0] {
        let p = reef + out * off;
        println!("{:.0} m off the shore: {:.1} m deep", -relief.shore_at(p.x, p.y), depth(p));
    }
    assert!(depth(reef) > 6.0 && depth(reef + out * 600.0) > 40.0, "a terrace, then the open sea");

    let mut grown = 0;
    let mut thickest = (0, reef);
    for name in ["coral_recife", "kelp", "algas", "rocha_do_fundo"] {
        let index = plant(&book, name);
        let cell = book.plants[index as usize].cell();
        let (cx, cz) = ((reef.x / cell).floor() as i64, (reef.y / cell).floor() as i64);
        for (dx, dz) in (-6..=6).flat_map(|x| (-6..=6).map(move |z| (x, z))) {
            let spots = place_plants(&relief, &book, FloraKey { plant: index, x: cx + dx, z: cz + dz });
            if name == "coral_recife" && spots.len() > thickest.0 {
                thickest = (spots.len(), glam::DVec2::new(spots[0].at.x, spots[0].at.z));
            }
            for spot in spots {
                assert!(spot.at.y < -0.5, "{name} out of the water at {}", spot.at.y);
                grown += 1;
            }
        }
    }
    let unit = thickest.1 / grimora_world::METERS_PER_UNIT;
    println!("{grown} things; corals thickest at --at {:.5},{:.5}, {:.1} m deep", unit.x, unit.y, depth(thickest.1));
    assert!(grown > 50, "only {grown} things grow there");
}
