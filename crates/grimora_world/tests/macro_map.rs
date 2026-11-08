//! The macro map against what the compendium shows: the expected bands, biomes and nations
//! come from the site's own hover masks, not from the grids under test.

use std::path::PathBuf;

use glam::DVec2;
use grimora_world::{MacroMap, MapSample, PointGroup, map_to_world, world_to_map};

fn assets() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/world")
}

fn island() -> MacroMap {
    MacroMap::load(assets().join("island.ron")).expect("island world")
}

fn ids<'a>(s: &MapSample<'a>) -> (Option<&'a str>, Option<&'a str>, Option<&'a str>) {
    (s.band.map(|b| b.id.as_str()), s.biome.map(|b| b.id.as_str()), s.nation.map(|n| n.id.as_str()))
}

fn place(map: &MacroMap, name: &str) -> DVec2 {
    map.points.iter().find(|p| p.name == name && p.group == PointGroup::Place).expect(name).position()
}

#[test]
fn compendium_places_fall_in_their_band_biome_and_nation() {
    let map = island();
    let cases = [
        ("Bakardia", "land_plateau", "floresta_temperada_decidua", "ARCANOCRACIA_DE_NYXOS"),
        ("Porto Thivora", "land_low", "oceanico", "REINO_DE_MUR_VAT"),
        ("Nattvik", "land_low", "calota_polar", "BORGRUM"),
    ];
    for (name, band, biome, nation) in cases {
        // The points are pixel corners of the map: sample the pixel they name.
        let s = map.sample(map_to_world(place(&map, name) + 0.5));
        assert!(s.land, "{name} is on land");
        assert_eq!(ids(&s), (Some(band), Some(biome), Some(nation)), "{name}");
    }
}

#[test]
fn ports_are_on_the_coast() {
    let map = island();
    let ports: Vec<_> = map.points.iter().filter(|p| p.kind == "Porto").collect();
    assert!(ports.len() > 300, "the compendium has hundreds of ports");
    let on_coast = ports
        .iter()
        .filter(|p| {
            let at = p.position() + 0.5;
            (-3..=3).any(|dy| (-3..=3).any(|dx| !map.sample_map(at + DVec2::new(f64::from(dx), f64::from(dy))).land))
        })
        .count();
    // A few ports sit on rivers far inland; nearly all touch the sea.
    assert!(on_coast * 100 >= ports.len() * 90, "{on_coast} of {} ports touch the sea", ports.len());
}

#[test]
fn the_test_island_is_low_tropical_savanna_with_no_nation() {
    let map = island();
    let start = map.desc.start;
    assert_eq!(start, DVec2::new(4476.0, 1681.0));
    assert!(map.window().contains(start));
    let s = map.sample(map_to_world(start));
    assert!(s.land);
    assert_eq!(ids(&s), (Some("land_low"), Some("savana_tropical"), None));
    assert!(s.water.is_none());
    // No marked place on it, and the nearest is far away on the mainland.
    assert!(map.points_near(map_to_world(start), 20_000.0).is_empty());
    // The window's corners are open sea.
    for corner in [(4472.2, 1677.2), (4479.8, 1677.2), (4472.2, 1684.8), (4479.8, 1684.8)] {
        let s = map.sample_map(DVec2::from(corner));
        assert!(!s.land, "{corner:?} is sea");
        assert!(s.band.is_some_and(|b| !b.land) && s.water.is_some() && s.biome.is_none(), "{corner:?}");
    }
}

#[test]
fn the_coast_follows_the_painting_not_the_topography() {
    let map = island();
    let topography = MacroMap::open(assets().join("nyraldar/legend.ron"), &[]).unwrap();
    let painting = image::open(assets().join("nyraldar/coast_island.png")).unwrap().into_luma8();
    let (mut land, mut differ) = (0, 0);
    // Every texel of the painting's mask (half a unit), over the whole window.
    for ty in 8..24 {
        for tx in 8..24 {
            let p = DVec2::new(4468.0 + (f64::from(tx) + 0.5) * 0.5, 1673.0 + (f64::from(ty) + 0.5) * 0.5);
            let s = map.sample_map(p);
            assert_eq!(s.land, painting.get_pixel(tx, ty).0[0] != 0, "{p}");
            land += usize::from(s.land);
            differ += usize::from(topography.sample_map(p).land != s.land);
            // The band always agrees with land or water, and land always has a biome.
            assert_eq!(s.band.unwrap().land, s.land, "{p}");
            assert_eq!(s.biome.is_some(), s.land, "{p}");
            assert!(s.nation.is_none());
        }
    }
    assert!(differ > 0, "the painting and the topography draw different coasts");
    let km2 = land as f64 * (0.5 * grimora_world::METERS_PER_UNIT / 1000.0).powi(2);
    // About 11 km across, and longer than wide.
    assert!((60.0..140.0).contains(&km2), "{km2:.0} km² of land");
}

#[test]
fn world_and_map_positions_round_trip() {
    let p = DVec2::new(4476.25, 1681.75);
    let w = map_to_world(p);
    assert!((w.x - 4476.25 * 2_000_000.0 / 948.0).abs() < 1e-6);
    assert_eq!(w.y, 0.0);
    assert!((world_to_map(w) - p).length() < 1e-9);
}
