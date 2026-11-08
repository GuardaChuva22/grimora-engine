//! Who is born, who dies and who arrives, counted in game days. Each group of animals keeps its
//! own books: what's owed builds up here, and is paid in real animals when one can be added or
//! taken away without anyone seeing it happen.

use glam::Vec3;
use grimora_scene::{CycleDesc, SceneFauna, SpeciesDesc};
use serde::{Deserialize, Serialize};

use super::Place;
use super::encounter::Carcass;

/// Debts never grow past this many animals, plus this share of the group: someone watching for a
/// long time shouldn't make a crowd appear, or a mass die, the moment they look away.
const MOST_OWED: f32 = 3.0;
const MOST_OWED_SHARE: f32 = 0.2;
/// How hungry is too hungry to breed, and from where it counts as going without.
const BREEDS_UNDER: f32 = 0.5;
const STARVING: f32 = 0.99;

/// How many of a group are left, for the save.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FaunaCount {
    /// Must still match on load, so a save from before the scene changed fits the wrong group.
    pub species: String,
    pub number: usize,
}

/// What a group owes, in animals.
#[derive(Debug, Clone, Default)]
pub(super) struct Ledger {
    births: f32,
    deaths: f32,
    arrivals: f32,
    /// As many as this place can feed.
    room: usize,
}

impl Ledger {
    pub(super) fn of(group: &SceneFauna) -> Self {
        let room = (group.count as f32 * group.species.cycle.room).ceil() as usize;
        Self { room, ..Self::default() }
    }
}

/// A group of animals, whatever kind, as the books see it.
pub(super) trait Population {
    fn species(&self) -> &SpeciesDesc;
    fn ledger(&mut self) -> &mut Ledger;
    fn number(&self) -> usize;
    /// Whether any of them is this close to the point.
    fn near(&self, point: Vec3, radius: f32) -> bool;
    /// Moves every one of them and every place they know by `offset`, as the world's origin
    /// moves the other way.
    fn shift(&mut self, offset: Vec3);
    /// `0..1`: how many are eating well enough to breed.
    fn fed(&self) -> f32 {
        1.0
    }
    /// How many have gone without food as long as they can.
    fn starving(&self) -> usize {
        0
    }
    /// One more, small if `young`. False if it can't be done now without being seen.
    fn add(&mut self, place: &Place, focus: Vec3, young: bool) -> bool;
    /// One fewer. False if it can't be done now without being seen.
    fn take_away(&mut self, focus: Vec3) -> bool;
    /// `share` of the way to their full size, for those not there yet.
    fn grow(&mut self, _share: f32) {}
    /// Down to this many at once, for a save.
    fn truncate(&mut self, number: usize);
    /// Kills whichever `hits`, given a place and a radius, says a blow reached, leaving the
    /// bodies in `dead`.
    fn strike(&mut self, hits: &mut dyn FnMut(Vec3, f32) -> bool, dead: &mut Vec<Carcass>);
}

/// `0..1`, how many of these hungers are low enough to breed.
pub(super) fn fed_from(hungers: impl Iterator<Item = f32>) -> f32 {
    let (mut fed, mut count) = (0.0, 0);
    for hunger in hungers {
        fed += ((1.0 - hunger) / (1.0 - BREEDS_UNDER)).min(1.0);
        count += 1;
    }
    if count == 0 { 1.0 } else { fed / count as f32 }
}

/// How many of these hungers are at the end of what they can take.
pub(super) fn starving_from(hungers: impl Iterator<Item = f32>) -> usize {
    hungers.filter(|hunger| *hunger >= STARVING).count()
}

/// How far a young animal is from its full size, after `days`, in shares of that size.
pub(super) fn growth(cycle: &CycleDesc, days: f32) -> f32 {
    if cycle.grows_in > 0.0 { (1.0 - cycle.newborn) * days / cycle.grows_in } else { 1.0 }
}

/// `days` go by for a group.
pub(super) fn pass(group: &mut dyn Population, place: &Place, focus: Vec3, days: f32) {
    let cycle = group.species().cycle.clone();
    if !cycle.alive() {
        return;
    }
    let number = group.number() as f32;
    let (fed, starving) = (group.fed(), group.starving() as f32);
    let ledger = group.ledger();
    let room = ledger.room;
    let most = MOST_OWED + number * MOST_OWED_SHARE;

    let space = (1.0 - number / room.max(1) as f32).max(0.0);
    ledger.births = (ledger.births + cycle.births * number * space * fed * days).min(most);
    let natural = if cycle.lives > 0.0 { number / cycle.lives } else { 0.0 };
    let hunger = if cycle.starves_in > 0.0 { starving / cycle.starves_in } else { 0.0 };
    ledger.deaths = (ledger.deaths + (natural + hunger) * days).min(most);
    if cycle.arrives_every > 0.0 && number < room as f32 {
        ledger.arrivals = (ledger.arrivals + days / cycle.arrives_every).min(most);
    }

    let (mut births, mut deaths, mut arrivals) = (ledger.births, ledger.deaths, ledger.arrivals);
    while deaths >= 1.0 && group.number() > 0 && group.take_away(focus) {
        deaths -= 1.0;
    }
    while births >= 1.0 && group.number() < room && group.add(place, focus, true) {
        births -= 1.0;
    }
    while arrivals >= 1.0 && group.number() < room && group.add(place, focus, false) {
        arrivals -= 1.0;
    }
    let ledger = group.ledger();
    (ledger.births, ledger.deaths, ledger.arrivals) = (births, deaths, arrivals);
    group.grow(growth(&cycle, days));
}

/// Makes a group this many, from a save, at once and whoever is looking.
pub(super) fn set_number(group: &mut dyn Population, place: &Place, number: usize) {
    group.truncate(number);
    // Nobody is anywhere: the scene hasn't been seen yet.
    let nowhere = Vec3::splat(1.0e6);
    let mut tries = number * 2 + 8;
    while group.number() < number && tries > 0 {
        group.add(place, nowhere, false);
        tries -= 1;
    }
}
