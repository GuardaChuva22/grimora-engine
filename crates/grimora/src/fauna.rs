//! Animals that get on with their lives whether the player looks or not.
//!
//! They aren't entities: a school is hundreds of fish that need no collider, name or save
//! of their own, so they live in plain lists, away from the physics and the [`hecs`] world.
//! How often each one thinks depends on how close the camera is (`Attention`), and a school
//! too far to see moves as one point until someone comes near.
//!
//! Fish share poses: every species has its swim worked out once into a table of skinning
//! matrices, and what each fish draws is only an index in it.

mod bird;
mod cattle;
mod crab;
mod ecology;
mod encounter;
mod jelly;
mod ray;

use std::sync::Arc;

use glam::{Mat3, Mat4, Quat, Vec3};
use grimora_core::water::Waters;
use grimora_core::{MeshData, Rng, Skeleton, Transform, Weather};
use grimora_physics::Physics;
use grimora_render::{DrawItem, MeshHandle};
use grimora_scene::{Life, SceneFauna, SpeciesDesc, SwimDesc, Terrain};

use self::bird::Flock;
use self::cattle::Herd;
pub(crate) use self::cattle::PenPlace;
use self::crab::Colony;
use self::ecology::{Ledger, Population};
use self::encounter::{BodyPose, Carcass, PlayerBody, Wake};
use self::jelly::Bloom;
use self::ray::Rays;
use crate::world::{MeshId, World};

pub use self::bird::BirdDoing;
pub use self::crab::CrabDoing;
pub use self::ecology::FaunaCount;
pub use self::encounter::{FaunaEvent, FaunaSound};
pub use self::ray::RayDoing;

/// Within this of the camera, a fish thinks every other frame.
const PRESENT_DISTANCE: f32 = 70.0;
/// Within this it thinks every `NEAR_STRIDE` frames and is drawn; beyond it, the school is
/// only a point.
const NEAR_DISTANCE: f32 = 170.0;
/// Meters from the player past which jellies, crabs, rays and birds sleep (schools already
/// think little when `Far`): a generated world's coast has groups all around it.
const AWAKE_DISTANCE: f32 = 1200.0;
const PRESENT_STRIDE: u32 = 2;
const NEAR_STRIDE: u32 = 6;

/// How many others a fish may look at in a frame, however crowded the school gets.
const MOST_CANDIDATES: u32 = 48;
const MOST_NEIGHBOURS: usize = 12;
/// Buckets of the grid that finds neighbours. A power of two.
const GRID_BUCKETS: usize = 1024;
const GRID_END: u32 = u32::MAX;

/// Steps of the swim in the pose table.
const POSE_PHASES: usize = 16;
/// The sizes of the swim, as parts of `WaveJoint::sway`.
const POSE_SWAYS: [f32; 4] = [0.0, 0.55, 1.0, 1.45];
/// How far into a turn, -1 to 1.
const POSE_BENDS: [f32; 5] = [-1.0, -0.5, 0.0, 0.5, 1.0];

/// Radians it leans into the sharpest turn.
const TURN_ROLL: f32 = 0.5;
/// How much of its heading may point up or down. Fish don't swim on end.
const STEEPEST: f32 = 0.55;
/// How far ahead it checks for shallow water: seconds of swimming, and at least these meters.
const LOOK_AHEAD_SECONDS: f32 = 3.0;
const LOOK_AHEAD_LEAST: f32 = 2.5;
/// How deep the water must be for the middle of a school, over what the fish themselves need.
const ANCHOR_MARGIN: f32 = 1.0;
/// Seconds of swimming for a school to form before anyone sees it.
const SETTLE_SECONDS: f32 = 6.0;
const SETTLE_STEP: f32 = 1.0 / 20.0;
/// A fish can come or go only further than this from whoever is looking.
const FISH_UNSEEN: f32 = 40.0;
/// Fear below this has no effect, and isn't passed on.
const FEAR_LEAST: f32 = 0.04;

/// Something that frightens animals for a frame: see [`World::scare_fauna`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Threat {
    pub position: Vec3,
    pub radius: f32,
}

/// The living things of a world.
#[derive(Debug, Default)]
pub struct Fauna {
    schools: Vec<School>,
    /// One for each species; [`School::poses`] points here.
    poses: Vec<SwimPoses>,
    flocks: Vec<Flock>,
    colonies: Vec<Colony>,
    rays: Vec<Rays>,
    blooms: Vec<Bloom>,
    /// The village's cattle.
    herds: Vec<Herd>,
    /// What died, until it's eaten or rots.
    carcasses: Vec<Carcass>,
    /// `0..1`, as of the last update, for what glows in the dark.
    daylight: f32,
    /// Game hours since the books were last looked at.
    hours: f32,
    threats: Vec<Threat>,
    /// Where they were heard from, as of the last update.
    focus: Vec3,
    /// Who there is to bite, this frame.
    player: Option<PlayerBody>,
    events: Vec<FaunaEvent>,
    sounds: Vec<FaunaSound>,
    /// Whether the last strike landed on a big animal (one that makes a blade stop for a moment).
    pub(crate) struck_big: bool,
}

impl Fauna {
    pub(crate) fn new(groups: Vec<SceneFauna>, meshes: &[MeshData]) -> Self {
        let mut fauna = Self::default();
        fauna.add(groups, meshes);
        fauna
    }

    /// Adds groups whose `mesh` is into `meshes`: the world's.
    pub(crate) fn add(&mut self, groups: Vec<SceneFauna>, meshes: &[MeshData]) {
        let fauna = self;
        for group in groups {
            let Some(skeleton) = meshes.get(group.mesh).and_then(|mesh| mesh.skeleton.as_ref()) else {
                log::error!("{}: its model has no skeleton", group.species.name);
                continue;
            };
            let mesh = MeshId(group.mesh);
            let mut poses = || {
                fauna.poses.iter().position(|poses| poses.mesh == mesh).unwrap_or_else(|| {
                    fauna.poses.push(SwimPoses::new(mesh, skeleton, &group.species.swim));
                    fauna.poses.len() - 1
                })
            };
            match group.species.life {
                Life::Schooling => {
                    let poses = poses();
                    fauna.schools.push(School::new(&group, poses));
                }
                Life::Gliding => {
                    let poses = poses();
                    fauna.rays.push(Rays::new(&group, poses));
                }
                Life::Drifting => {
                    let poses = poses();
                    fauna.blooms.push(Bloom::new(&group, poses));
                }
                Life::Flying => fauna.flocks.extend(Flock::new(&group, meshes)),
                Life::Crawling => fauna.colonies.extend(Colony::new(&group, meshes)),
                Life::Grazing => log::error!("{}: cattle come with a village pen", group.species.name),
            }
        }
    }

    /// How many animals there are, seen or not.
    pub fn population(&self) -> usize {
        self.fish_count()
            + self.flocks.iter().map(Flock::population).sum::<usize>()
            + self.crab_count()
            + self.rays.iter().map(|rays| rays.rays.len()).sum::<usize>()
            + self.jelly_count()
            + self.herds.iter().map(Herd::population).sum::<usize>()
    }

    /// How many there are of each group, in the order of the groups.
    /// The species with someone this close to the point, by the names in their files.
    pub fn species_near(&self, point: Vec3, radius: f32) -> Vec<String> {
        let name = |group: &dyn Population| group.near(point, radius).then(|| group.species().name.clone());
        let mut names = Vec::new();
        names.extend(self.schools.iter().filter_map(|group| name(group)));
        names.extend(self.flocks.iter().filter_map(|group| name(group)));
        names.extend(self.colonies.iter().filter_map(|group| name(group)));
        names.extend(self.rays.iter().filter_map(|group| name(group)));
        names.extend(self.blooms.iter().filter_map(|group| name(group)));
        names.extend(self.herds.iter().filter_map(|group| name(group)));
        names
    }

    pub fn census(&self) -> Vec<FaunaCount> {
        let count =
            |group: &dyn Population| FaunaCount { species: group.species().name.clone(), number: group.number() };
        let mut census = Vec::new();
        census.extend(self.schools.iter().map(|group| count(group)));
        census.extend(self.flocks.iter().map(|group| count(group)));
        census.extend(self.colonies.iter().map(|group| count(group)));
        census.extend(self.rays.iter().map(|group| count(group)));
        census.extend(self.blooms.iter().map(|group| count(group)));
        census.extend(self.herds.iter().map(|group| count(group)));
        census
    }

    /// Every group, in the order of [`Fauna::census`].
    fn groups(&mut self) -> Vec<&mut dyn Population> {
        let mut groups: Vec<&mut dyn Population> = Vec::new();
        groups.extend(self.schools.iter_mut().map(|group| group as &mut dyn Population));
        groups.extend(self.flocks.iter_mut().map(|group| group as &mut dyn Population));
        groups.extend(self.colonies.iter_mut().map(|group| group as &mut dyn Population));
        groups.extend(self.rays.iter_mut().map(|group| group as &mut dyn Population));
        groups.extend(self.blooms.iter_mut().map(|group| group as &mut dyn Population));
        groups.extend(self.herds.iter_mut().map(|group| group as &mut dyn Population));
        groups
    }

    /// Moves every animal, body and threat by `offset`, as the world's origin moves the other
    /// way. See [`World::shift_origin`].
    pub(crate) fn shift(&mut self, offset: Vec3) {
        for group in self.groups() {
            group.shift(offset);
        }
        for carcass in &mut self.carcasses {
            carcass.position += offset;
        }
        for threat in &mut self.threats {
            threat.position += offset;
        }
        self.focus += offset;
        if let Some(player) = &mut self.player {
            player.feet += offset;
        }
    }

    /// What has happened since the last time anyone asked.
    pub fn take_events(&mut self) -> Vec<FaunaEvent> {
        std::mem::take(&mut self.events)
    }

    /// The noises made since the last time anyone asked: the engine plays them.
    pub fn take_sounds(&mut self) -> Vec<FaunaSound> {
        std::mem::take(&mut self.sounds)
    }

    /// Where each body is.
    pub fn carcass_positions(&self) -> impl Iterator<Item = Vec3> + '_ {
        self.carcasses.iter().map(|carcass| carcass.position)
    }

    /// What's left of a frame, for the game to take when it wants. A game that doesn't is no
    /// reason for this to grow.
    fn keep(&mut self, events: Vec<FaunaEvent>, sounds: Vec<FaunaSound>) {
        self.events.extend(events);
        self.sounds.extend(sounds);
        let extra = self.events.len().saturating_sub(MOST_KEPT);
        self.events.drain(..extra);
        let extra = self.sounds.len().saturating_sub(MOST_KEPT);
        self.sounds.drain(..extra);
    }

    /// How many crabs there are.
    pub fn crab_count(&self) -> usize {
        self.colonies.iter().map(Colony::population).sum()
    }

    /// How many jellyfish there are, alive.
    pub fn jelly_count(&self) -> usize {
        self.blooms.iter().map(|bloom| bloom.jellies.len()).sum()
    }

    /// What's dead on the sand.
    pub fn carcass_count(&self) -> usize {
        self.carcasses.len()
    }

    /// Where each crab is.
    pub fn crab_positions(&self) -> impl Iterator<Item = Vec3> + '_ {
        self.colonies.iter().flat_map(Colony::positions)
    }

    /// What each crab is up to.
    pub fn crab_doings(&self) -> impl Iterator<Item = CrabDoing> + '_ {
        self.colonies.iter().flat_map(Colony::doings)
    }

    /// Times crabs have eaten, and squared up to each other.
    pub fn crab_meals(&self) -> u32 {
        self.colonies.iter().map(Colony::meals).sum()
    }

    pub fn crab_standoffs(&self) -> u32 {
        self.colonies.iter().map(Colony::standoffs).sum()
    }

    /// How far the worst planted crab's foot is from the ground, and how much of the length of
    /// a leg the worst one asks for.
    pub fn crab_feet(&self, terrain: Option<&Terrain>, waters: &Waters) -> (f32, f32) {
        let place = Place { waters, terrain, land: None, daylight: 1.0, storm: false };
        let off = self.colonies.iter().map(|colony| colony.feet_off_ground(&place)).fold(0.0, f32::max);
        let stretch = self.colonies.iter().map(Colony::longest_stretch).fold(0.0, f32::max);
        (off, stretch)
    }

    /// What each ray is up to.
    pub fn ray_doings(&self) -> impl Iterator<Item = RayDoing> + '_ {
        self.rays.iter().flat_map(Rays::doings)
    }

    /// Where each ray is.
    pub fn ray_positions(&self) -> impl Iterator<Item = Vec3> + '_ {
        self.rays.iter().flat_map(|rays| rays.rays.iter().map(|ray| ray.position))
    }

    /// Crabs the rays have caught so far.
    pub fn ray_meals(&self) -> u32 {
        self.rays.iter().map(Rays::meals).sum()
    }

    /// Where each jellyfish is.
    pub fn jelly_positions(&self) -> impl Iterator<Item = Vec3> + '_ {
        self.blooms.iter().flat_map(|bloom| bloom.jellies.iter().map(|jelly| jelly.position))
    }

    /// How many fish there are, seen or not.
    pub fn fish_count(&self) -> usize {
        self.schools.iter().map(|school| school.fish.len()).sum()
    }

    /// Where each bird is.
    pub fn bird_positions(&self) -> impl Iterator<Item = Vec3> + '_ {
        self.flocks.iter().flat_map(Flock::positions)
    }

    /// What each bird is up to, in the order of [`Fauna::bird_positions`].
    pub fn bird_doings(&self) -> impl Iterator<Item = BirdDoing> + '_ {
        self.flocks.iter().flat_map(Flock::doings)
    }

    /// How hungry each bird is, `0..1`, in the order of [`Fauna::bird_positions`].
    pub fn bird_hungers(&self) -> impl Iterator<Item = f32> + '_ {
        self.flocks.iter().flat_map(Flock::hungers)
    }

    /// Every time a bird has gone down on something so far.
    pub fn bird_dives(&self) -> u32 {
        self.flocks.iter().map(Flock::dives).sum()
    }

    /// Everything every bird has caught so far.
    pub fn bird_meals(&self) -> u32 {
        self.flocks.iter().map(Flock::meals).sum()
    }

    /// Every animal, wherever it is.
    pub fn positions(&self) -> impl Iterator<Item = Vec3> + '_ {
        self.schools.iter().flat_map(|school| school.fish.iter().map(|fish| fish.position))
    }

    /// How frightened each animal is, `0..1`, in the order of [`Fauna::positions`].
    pub fn fears(&self) -> impl Iterator<Item = f32> + '_ {
        self.schools.iter().flat_map(|school| school.fish.iter().map(|fish| fish.fear))
    }

    /// Adds what the camera can see to a frame.
    pub(crate) fn push_draws(&self, handles: &[MeshHandle], draws: &mut Vec<DrawItem>, joints: &mut Vec<Mat4>) {
        // Only the poses somebody is in go to the GPU.
        let mut sent: Vec<Vec<u32>> = self.poses.iter().map(|table| vec![u32::MAX; table.count()]).collect();
        let mut pose_of = |poses: usize, pose: usize, joints: &mut Vec<Mat4>| -> u32 {
            let slot = &mut sent[poses][pose];
            if *slot == u32::MAX {
                *slot = joints.len() as u32;
                joints.extend_from_slice(self.poses[poses].pose(pose));
            }
            *slot
        };
        for rays in &self.rays {
            let Some(&mesh) = handles.get(self.poses[rays.poses].mesh.0) else { continue };
            for ray in rays.rays.iter().filter(|ray| ray.drawn) {
                let sway =
                    if ray.buried() { 0 } else { sway_for(ray.velocity.length() / rays.species.swim.cruise.max(0.01)) };
                draws.push(DrawItem {
                    mesh,
                    transform: Transform {
                        position: ray.position,
                        rotation: ray.rotation,
                        scale: Vec3::splat(ray.size),
                    },
                    color: Vec3::splat(ray.shade),
                    emissive: 0.0,
                    joints: Some(pose_of(rays.poses, pose_index(ray.phase, sway, ray.bend), joints)),
                });
            }
        }
        for bloom in &self.blooms {
            let Some(&mesh) = handles.get(self.poses[bloom.poses].mesh.0) else { continue };
            for jelly in bloom.jellies.iter().filter(|jelly| jelly.drawn) {
                draws.push(DrawItem {
                    mesh,
                    transform: Transform {
                        position: jelly.position,
                        rotation: jelly.rotation,
                        scale: Vec3::splat(jelly.size),
                    },
                    color: Vec3::splat(jelly.shade),
                    emissive: jelly::glow(bloom.species.glow, self.daylight, jelly.phase),
                    joints: Some(pose_of(bloom.poses, pose_index(jelly.pose_phase(), 2, 0.0), joints)),
                });
            }
        }
        for carcass in self.carcasses.iter().filter(|carcass| carcass.scale() != Vec3::ZERO) {
            let (mesh, pose) = match &carcass.pose {
                BodyPose::Table { poses, pose } => (self.poses[*poses].mesh, Some(pose_of(*poses, *pose, joints))),
                BodyPose::Own { mesh, matrices } if matrices.is_empty() => (*mesh, None),
                BodyPose::Own { mesh, matrices } => {
                    joints.extend_from_slice(matrices);
                    (*mesh, Some((joints.len() - matrices.len()) as u32))
                }
            };
            let Some(&mesh) = handles.get(mesh.0) else { continue };
            draws.push(DrawItem {
                mesh,
                transform: Transform { position: carcass.position, rotation: carcass.rotation, scale: carcass.scale() },
                color: Vec3::splat(carcass.shade * 0.8),
                emissive: 0.0,
                joints: pose,
            });
        }
        for school in &self.schools {
            if school.attention == Attention::Far {
                continue;
            }
            let Some(&mesh) = handles.get(self.poses[school.poses].mesh.0) else {
                continue;
            };
            for fish in &school.fish {
                draws.push(DrawItem {
                    mesh,
                    transform: Transform {
                        position: fish.position,
                        rotation: fish.rotation,
                        scale: Vec3::splat(fish.size),
                    },
                    color: Vec3::splat(fish.shade),
                    emissive: 0.0,
                    joints: Some(pose_of(school.poses, fish_pose(fish, &school.species.swim), joints)),
                });
            }
        }
        for flock in &self.flocks {
            flock.push_draws(handles, draws, joints);
        }
        for colony in &self.colonies {
            colony.push_draws(handles, draws, joints);
        }
        for herd in &self.herds {
            herd.push_draws(handles, draws, joints);
        }
    }
}

impl World {
    /// Moves every animal. `focus` is where someone is looking from. The engine calls this
    /// each frame, unless the game is paused. The physics is only asked what's in the way.
    /// Goes through days of births and deaths without anyone moving: time that went by unseen.
    pub fn pass_fauna_days(&mut self, days: f32, focus: Vec3) {
        let place = Place::of(
            &self.waters,
            self.terrain.as_ref(),
            crate::land::floor(&self.land, self.origin()),
            self.day_cycle.as_ref(),
        );
        for _ in 0..(days * 24.0).ceil() as u32 {
            for group in self.fauna.groups() {
                ecology::pass(group, &place, focus, 1.0 / 24.0);
            }
        }
    }

    /// Makes the groups as many as these, from a save. One that doesn't match stays as it is, and
    /// an old save with no numbers changes nothing.
    pub fn restore_fauna(&mut self, census: &[FaunaCount]) {
        let place = Place::of(
            &self.waters,
            self.terrain.as_ref(),
            crate::land::floor(&self.land, self.origin()),
            self.day_cycle.as_ref(),
        );
        for (group, count) in self.fauna.groups().into_iter().zip(census) {
            if group.species().name == count.species {
                ecology::set_number(group, &place, count.number);
            }
        }
    }

    pub fn update_fauna(&mut self, physics: &Physics, focus: Vec3, dt: f32) {
        // Whatever falls in the water frightens them.
        let splashes = self
            .splashes
            .iter()
            .map(|&(position, strength)| Threat { position, radius: 5.0 + 4.0 * strength.clamp(0.0, 3.0) });
        self.fauna.threats.extend(splashes);
        let place = Place::of(
            &self.waters,
            self.terrain.as_ref(),
            crate::land::floor(&self.land, self.origin()),
            self.day_cycle.as_ref(),
        );
        let threats = std::mem::take(&mut self.fauna.threats);
        self.fauna.daylight = place.daylight;
        self.fauna.focus = focus;
        let mut wake = Wake::new(focus, self.fauna.player.take());
        let Fauna { schools, flocks, colonies, rays, blooms, herds, carcasses, .. } = &mut self.fauna;
        for school in schools.iter_mut() {
            school.update(&place, &threats, focus, dt);
        }
        // Past AWAKE_DISTANCE a group sleeps where it is, unseen: only those around the player
        // live, wherever in the world it goes. The day's ecology still counts them all.
        let awake = |group: &dyn Population| group.near(focus, AWAKE_DISTANCE);
        for bloom in blooms.iter_mut().filter(|g| awake(&**g)) {
            bloom.update(&place, focus, dt, carcasses, &mut wake);
        }
        // Crabs also run from a ray that's after one of them.
        let scares: Vec<Threat> = threats.iter().copied().chain(rays.iter().flat_map(Rays::hunters)).collect();
        for colony in colonies.iter_mut().filter(|g| awake(&**g)) {
            colony.update(&place, physics, carcasses, &scares, focus, dt, &mut wake);
        }
        for group in rays.iter_mut().filter(|g| awake(&**g)) {
            group.update(&place, colonies, &threats, focus, dt, &mut wake);
        }
        for flock in flocks.iter_mut().filter(|g| awake(&**g)) {
            flock.update(&place, physics, schools, &threats, focus, dt, &mut wake);
        }
        for herd in herds.iter_mut().filter(|g| awake(&**g)) {
            herd.update(&place, physics, &threats, focus, dt, &mut wake);
            carcasses.append(&mut herd.fallen);
        }
        carcasses.retain_mut(|carcass| carcass.update(&place, dt));
        if let Some(day) = self.day_cycle.as_ref().filter(|day| day.day_length > 0.0) {
            self.fauna.hours += dt / day.day_length * 24.0;
            while self.fauna.hours >= 1.0 {
                self.fauna.hours -= 1.0;
                for group in self.fauna.groups() {
                    ecology::pass(group, &place, focus, 1.0 / 24.0);
                }
            }
        }
        self.fauna.threats = wake.threats;
        self.fauna.keep(wake.events, wake.sounds);
        for (position, strength) in wake.splashes {
            self.splash(position, strength);
        }
        for (effect, position) in wake.puffs {
            self.play_effect(&effect, position);
        }
    }

    /// Frightens the animals around a point, for this frame: call it every frame for as long
    /// as the danger is there. The more they would notice it, the larger `radius` should be.
    pub fn scare_fauna(&mut self, position: Vec3, radius: f32) {
        self.fauna.threats.push(Threat { position, radius });
    }

    /// Says where the one body animals can hurt is, for this frame: call it every frame for as
    /// long as there's someone there. What they do comes out of [`Fauna::take_events`].
    pub fn touch_fauna(&mut self, feet: Vec3, height: f32) {
        self.fauna.player = Some(PlayerBody { feet, height });
    }

    /// Kills the animals a blow around a point gets to, and returns how many.
    pub fn strike_fauna(&mut self, center: Vec3, radius: f32) -> usize {
        self.strike_fauna_by(&mut |at, size| at.distance(center) < radius + size)
    }

    /// Kills whichever animals `hits`, given where one is and how wide, says a blow gets to, and
    /// returns how many. Those down their holes or under the sand are out of its reach.
    pub fn strike_fauna_by(&mut self, hits: &mut dyn FnMut(Vec3, f32) -> bool) -> usize {
        let mut wake = Wake::new(self.fauna.focus, None);
        let mut dead = Vec::new();
        let before: usize = self.fauna.herds.iter().map(|h| h.struck_count()).sum();
        for group in self.fauna.groups() {
            let from = dead.len();
            group.strike(hits, &mut dead);
            for body in &dead[from..] {
                wake.death(group.species(), body.position);
            }
        }
        let killed = dead.len();
        let after: usize = self.fauna.herds.iter().map(|h| h.struck_count()).sum();
        self.fauna.struck_big = after > before;
        self.fauna.carcasses.append(&mut dead);
        self.fauna.threats.append(&mut wake.threats);
        self.fauna.keep(wake.events, wake.sounds);
        killed
    }

    /// Swims every school for a few seconds, so nobody sees them form.
    pub(crate) fn settle_fauna(&mut self, physics: &Physics) {
        let place = Place::of(
            &self.waters,
            self.terrain.as_ref(),
            crate::land::floor(&self.land, self.origin()),
            self.day_cycle.as_ref(),
        );
        self.fauna.daylight = place.daylight;
        let Fauna { schools, flocks, colonies, rays, blooms, herds, .. } = &mut self.fauna;
        for herd in herds {
            herd.place(&place);
        }
        for school in schools.iter_mut() {
            school.go_to_depth(&place);
        }
        for flock in flocks {
            flock.place(&place, physics);
        }
        for colony in colonies {
            colony.place(&place);
        }
        for group in rays {
            group.place(&place);
        }
        for bloom in blooms {
            bloom.place(&place);
        }
        for school in &mut self.fauna.schools {
            let home = school.home;
            for _ in 0..(SETTLE_SECONDS / SETTLE_STEP) as u32 {
                school.update(&place, &[], home, SETTLE_STEP);
            }
        }
    }
}

/// How many events and sounds wait for someone to take them.
const MOST_KEPT: usize = 64;
/// How far from its middle a blow gets a fish, times its size.
const FISH_RADIUS: f32 = 0.08;

/// What a fish needs to know about where it is.
struct Place<'a> {
    waters: &'a Waters,
    terrain: Option<&'a Terrain>,
    /// A generated world's ground, and where the origin is in it.
    land: Option<(&'a grimora_world::Relief, grimora_core::Origin)>,
    /// `0..1`.
    daylight: f32,
    /// Too rough to be out in.
    storm: bool,
}

impl<'a> Place<'a> {
    fn of(
        waters: &'a Waters,
        terrain: Option<&'a Terrain>,
        land: Option<(&'a grimora_world::Relief, grimora_core::Origin)>,
        day: Option<&grimora_core::DayCycle>,
    ) -> Self {
        Self {
            waters,
            terrain,
            land,
            daylight: day.map_or(1.0, |day| day.daylight()),
            storm: day.is_some_and(|day| matches!(day.weather(), Weather::Storm)),
        }
    }

    /// The height of whatever is at the bottom of the air: water, or land.
    fn surface(&self, x: f32, z: f32) -> f32 {
        let floor = self.floor(x, z);
        self.waters.calm_at(x, z).map_or(floor, |(level, _)| level.max(floor))
    }

    fn floor(&self, x: f32, z: f32) -> f32 {
        if let Some((relief, origin)) = self.land {
            let at = origin.to_world(Vec3::new(x, 0.0, z));
            let ground = relief.height_at(at.x, at.z);
            return origin.to_local(grimora_core::WorldPos::new(at.x, ground, at.z)).y;
        }
        self.terrain.map_or(f32::NEG_INFINITY, |terrain| terrain.height_at(x, z))
    }

    /// How deep the water is at a point: `None` if there isn't any.
    fn depth(&self, x: f32, z: f32) -> Option<f32> {
        let (level, _) = self.waters.calm_at(x, z)?;
        Some(level - self.floor(x, z))
    }
}

/// How much thinking a school is worth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Attention {
    Present,
    Near,
    Far,
}

#[derive(Debug, Clone)]
struct Fish {
    position: Vec3,
    velocity: Vec3,
    /// What it decided last time it thought, in meters per second per second.
    steer: Vec3,
    rotation: Quat,
    /// Where in the beat of its tail it is, `0..1`.
    phase: f32,
    /// How far into a turn, -1 to 1; positive is to its left.
    bend: f32,
    /// `0..1`.
    fear: f32,
    /// What it last saw of the water above and the floor below.
    level: f32,
    floor: f32,
    flow: Vec3,
    size: f32,
    full_size: f32,
    shade: f32,
    /// Makes each one a little different in how it swims and where it stays, -1 to 1.
    habit: f32,
}

/// What a fish decided, kept apart so all of them decide from the same moment.
#[derive(Debug)]
struct Thought {
    fish: u32,
    steer: Vec3,
    fear: f32,
    level: f32,
    floor: f32,
    flow: Vec3,
}

#[derive(Debug)]
struct School {
    species: Arc<SpeciesDesc>,
    poses: usize,
    fish: Vec<Fish>,
    home: Vec3,
    radius: f32,
    /// The point the school is travelling around, and where that's heading.
    anchor: Vec3,
    destination: Vec3,
    /// How far the anchor went while nobody could see them.
    drift: Vec3,
    attention: Attention,
    /// Counts the frames, to take turns thinking.
    tick: u32,
    rng: Rng,
    grid: Grid,
    thoughts: Vec<Thought>,
    /// The middle of the fish, as of the last time they thought.
    center: Vec3,
    /// Coming up to feed, and until when, or how long until it does.
    feeding: bool,
    feed_timer: f32,
    ledger: Ledger,
}

impl Population for School {
    fn shift(&mut self, offset: Vec3) {
        self.home += offset;
        self.anchor += offset;
        self.destination += offset;
        self.center += offset;
        for fish in &mut self.fish {
            fish.position += offset;
        }
    }

    fn near(&self, point: Vec3, radius: f32) -> bool {
        self.anchor.distance(point) < radius
    }

    fn species(&self) -> &SpeciesDesc {
        &self.species
    }

    fn ledger(&mut self) -> &mut Ledger {
        &mut self.ledger
    }

    fn number(&self) -> usize {
        self.fish.len()
    }

    /// Young come from among the others, where one more small fish won't be noticed. The first
    /// ones back come from somewhere else, where nobody is looking.
    fn add(&mut self, place: &Place, focus: Vec3, young: bool) -> bool {
        let (position, heading) = if self.fish.is_empty() {
            if self.anchor.distance(focus) < FISH_UNSEEN {
                return false;
            }
            let (x, z) = (self.anchor.x, self.anchor.z);
            let Some((level, _)) = place.waters.calm_at(x, z) else { return false };
            let depth = self.species.habitat.day_depth.min(level - place.floor(x, z) - ANCHOR_MARGIN);
            (Vec3::new(x, level - depth.max(ANCHOR_MARGIN), z), Vec3::NEG_Z)
        } else {
            let other = &self.fish[self.rng.next_u64() as usize % self.fish.len()];
            (other.position, other.velocity.normalize_or(Vec3::NEG_Z))
        };
        let spacing = self.species.school.spacing;
        let offset = Vec3::new(self.rng.range(-1.0, 1.0), self.rng.range(-0.5, 0.5), self.rng.range(-1.0, 1.0));
        let mut fish = new_fish(&self.species, &mut self.rng, position + offset * spacing, heading);
        if young {
            fish.size *= self.species.cycle.newborn;
        }
        if self.fish.is_empty() {
            self.center = fish.position;
        }
        self.fish.push(fish);
        true
    }

    fn take_away(&mut self, focus: Vec3) -> bool {
        let farthest = self
            .fish
            .iter()
            .enumerate()
            .map(|(index, fish)| (index, fish.position.distance(focus)))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        match farthest {
            Some((index, far)) if far > FISH_UNSEEN || self.attention == Attention::Far => {
                self.fish.swap_remove(index);
                true
            }
            _ => false,
        }
    }

    fn grow(&mut self, share: f32) {
        for fish in &mut self.fish {
            fish.size = (fish.size + fish.full_size * share).min(fish.full_size);
        }
    }

    fn truncate(&mut self, number: usize) {
        self.fish.truncate(number);
    }

    /// Dead, a fish comes up and floats belly up.
    fn strike(&mut self, hits: &mut dyn FnMut(Vec3, f32) -> bool, dead: &mut Vec<Carcass>) {
        if self.attention == Attention::Far {
            return;
        }
        let (poses, swim) = (self.poses, &self.species.swim);
        self.fish.retain(|fish| {
            let struck = hits(fish.position, FISH_RADIUS * fish.size);
            if struck {
                let pose = BodyPose::Table { poses, pose: fish_pose(fish, swim) };
                let belly_up = fish.rotation * Quat::from_rotation_z(std::f32::consts::PI);
                dead.push(Carcass {
                    floats: true,
                    ..Carcass::new(fish.position, belly_up, fish.size, fish.shade, pose)
                });
            }
            !struck
        });
    }
}

/// A grown fish, with nothing worked out yet about where it is.
fn new_fish(species: &SpeciesDesc, rng: &mut Rng, position: Vec3, heading: Vec3) -> Fish {
    let size = rng.range(species.size.0, species.size.1);
    Fish {
        position,
        velocity: heading * species.swim.cruise,
        steer: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        phase: rng.f32(),
        bend: 0.0,
        fear: 0.0,
        level: position.y,
        floor: f32::NEG_INFINITY,
        flow: Vec3::ZERO,
        size,
        full_size: size,
        shade: rng.range(0.9, 1.1),
        habit: rng.range(-1.0, 1.0),
    }
}

impl School {
    fn new(group: &SceneFauna, poses: usize) -> Self {
        let species = group.species.clone();
        let mut rng = Rng::new(group.seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xFA07A);
        let spread = species.school.spacing * (group.count as f32).cbrt();
        let feed_timer = species.habitat.feeding.as_ref().map_or(0.0, |feeding| rng.range(0.0, feeding.every.1));
        let fish = (0..group.count)
            .map(|_| {
                let offset = Vec3::new(rng.range(-1.0, 1.0), rng.range(-0.5, 0.5), rng.range(-1.0, 1.0)) * spread;
                let heading = Vec3::new(rng.range(-1.0, 1.0), 0.0, rng.range(-1.0, 1.0)).normalize_or(Vec3::NEG_Z);
                new_fish(&species, &mut rng, group.position + offset, heading)
            })
            .collect();
        Self {
            poses,
            fish,
            home: group.position,
            radius: group.radius,
            anchor: group.position,
            destination: group.position,
            drift: Vec3::ZERO,
            attention: Attention::Far,
            tick: 0,
            rng,
            grid: Grid::default(),
            thoughts: Vec::new(),
            center: group.position,
            feeding: false,
            feed_timer,
            ledger: Ledger::of(group),
            species,
        }
    }

    /// Moves the fish, all together, to where they want to be at this hour. The file says
    /// where the school lives, but only the species says how deep.
    fn go_to_depth(&mut self, place: &Place) {
        let Some((level, _)) = place.waters.calm_at(self.home.x, self.home.z) else {
            log::warn!("{}: no water at {}", self.species.name, self.home);
            return;
        };
        let habitat = &self.species.habitat;
        let floor = place.floor(self.home.x, self.home.z) + habitat.above_floor;
        let depth = habitat.night_depth + (habitat.day_depth - habitat.night_depth) * place.daylight;
        let height = (level - depth).max(floor.min(level - habitat.below_surface));
        for fish in &mut self.fish {
            fish.position.y += height - self.home.y;
            fish.level = level;
        }
        self.home.y = height;
        self.anchor.y = height;
        self.destination.y = height;
    }

    fn update(&mut self, place: &Place, threats: &[Threat], focus: Vec3, dt: f32) {
        if self.fish.is_empty() {
            return;
        }
        self.tick = self.tick.wrapping_add(1);
        self.update_feeding(dt);
        let before = self.anchor;
        let loose = self.loose_radius(place);
        self.move_anchor(place, threats, loose, dt);

        let distance = flat(self.anchor - focus).length() - loose;
        let frightened =
            threats.iter().any(|threat| flat(threat.position - self.anchor).length() < threat.radius + loose);
        self.attention = if frightened || distance < PRESENT_DISTANCE {
            Attention::Present
        } else if distance < NEAR_DISTANCE {
            Attention::Near
        } else {
            Attention::Far
        };
        if self.attention == Attention::Far {
            self.drift += self.anchor - before;
            return;
        }
        if self.drift != Vec3::ZERO {
            for fish in &mut self.fish {
                fish.position += self.drift;
            }
            self.drift = Vec3::ZERO;
        }

        let stride = if self.attention == Attention::Present { PRESENT_STRIDE } else { NEAR_STRIDE };
        self.think(place, threats, stride, loose);
        self.swim(dt);
    }

    /// Bouts of coming up to feed. It stops when something scares it, as long as it lasts.
    fn update_feeding(&mut self, dt: f32) {
        let Some(feeding) = &self.species.habitat.feeding else { return };
        self.feed_timer -= dt;
        if self.feed_timer > 0.0 {
            return;
        }
        self.feeding = !self.feeding;
        let (shortest, longest) = if self.feeding { feeding.lasts } else { feeding.every };
        self.feed_timer = self.rng.range(shortest, longest);
    }

    /// The middle of the fish.
    fn center_now(&self) -> Vec3 {
        self.center
    }

    /// Whether anyone passing above could tell it's there: close enough to see, and to the
    /// surface. A school nobody is near hasn't been worked out, so it can't be.
    /// Whether something could catch one of these now, or there's just nothing there to catch.
    fn huntable(&self) -> bool {
        self.attention != Attention::Far || self.fish.is_empty()
    }

    fn visible_from_above(&self, sees_down: f32) -> bool {
        if self.attention == Attention::Far || self.fish.is_empty() {
            return false;
        }
        let level = self.fish[0].level;
        level - self.center.y < sees_down
    }

    /// Where the nearest fish close enough to the surface is, that's within `reach` of a point.
    fn fish_near_surface(&self, from: Vec3, reach: f32, sees_down: f32) -> Option<Vec3> {
        self.fish
            .iter()
            .filter(|fish| fish.level - fish.position.y < sees_down)
            .map(|fish| (fish.position, flat(fish.position - from).length()))
            .filter(|(_, far)| *far < reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(position, _)| position)
    }

    /// Something took the fish closest to `at`, if any was within reach. It's gone.
    fn take_fish_at(&mut self, at: Vec3, radius: f32, depth: f32) -> bool {
        let caught = self
            .fish
            .iter()
            .enumerate()
            .filter(|(_, fish)| at.y - fish.position.y < depth)
            .map(|(index, fish)| (index, flat(fish.position - at).length()))
            .filter(|(_, far)| *far < radius)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let Some((index, _)) = caught else { return false };
        self.fish.swap_remove(index);
        true
    }

    /// How far the fish spread from the anchor when calm.
    fn loose_radius(&self, place: &Place) -> f32 {
        let school = &self.species.school;
        school.spacing * spread_at(school.night_spread, place.daylight) * (self.fish.len() as f32).cbrt()
    }

    fn move_anchor(&mut self, place: &Place, threats: &[Threat], loose: f32, dt: f32) {
        let swim = &self.species.swim;
        let shallowest = self.species.habitat.shallowest + ANCHOR_MARGIN;
        let mut next = self.anchor;
        for threat in threats {
            let away = flat(self.anchor - threat.position);
            let reach = threat.radius + loose;
            if away.length() < reach {
                let away = away.normalize_or(Vec3::X);
                next += away * swim.sprint * 0.6 * dt;
                self.destination = self.anchor + away * reach;
            }
        }
        let to = flat(self.destination - self.anchor);
        if to.length() < 1.0 {
            self.destination = self.next_destination(place);
        } else if next == self.anchor {
            next += to.normalize() * swim.cruise * self.species.school.roam * dt;
        }
        // Never lead it up a beach, or the school would have to follow.
        let deep = |point: Vec3| place.depth(point.x, point.z).is_some_and(|depth| depth >= shallowest);
        if deep(next) || !deep(self.anchor) {
            self.anchor = next;
        } else {
            self.destination = self.next_destination(place);
        }
    }

    /// Somewhere deep enough, within its range; or staying put if it can't find any.
    fn next_destination(&mut self, place: &Place) -> Vec3 {
        let shallowest = self.species.habitat.shallowest + ANCHOR_MARGIN;
        for _ in 0..12 {
            let angle = self.rng.range(0.0, std::f32::consts::TAU);
            let far = self.radius * self.rng.f32().sqrt();
            let point = self.home + Vec3::new(angle.cos(), 0.0, angle.sin()) * far;
            if place.depth(point.x, point.z).is_some_and(|depth| depth >= shallowest) {
                return point;
            }
        }
        self.anchor
    }

    /// Lets a part of the school decide what to do: the rest keeps doing what it decided.
    fn think(&mut self, place: &Place, threats: &[Threat], stride: u32, loose: f32) {
        let school = &self.species.school;
        let reach = school.reach * spread_at(school.night_spread, place.daylight);
        self.grid.fill(&self.fish, reach, place.waters.origin.world());
        let center = self.fish.iter().map(|fish| fish.position).sum::<Vec3>() / self.fish.len().max(1) as f32;
        self.center = center;

        let mut thoughts = std::mem::take(&mut self.thoughts);
        thoughts.clear();
        for i in 0..self.fish.len() as u32 {
            if i % stride == self.tick % stride {
                thoughts.push(self.decide(i, place, threats, center, reach, loose));
            }
        }
        for thought in &thoughts {
            let fish = &mut self.fish[thought.fish as usize];
            fish.steer = thought.steer;
            fish.fear = fish.fear.max(thought.fear);
            fish.level = thought.level;
            fish.floor = thought.floor;
            fish.flow = thought.flow;
        }
        self.thoughts = thoughts;
    }

    fn decide(&self, i: u32, place: &Place, threats: &[Threat], center: Vec3, reach: f32, loose: f32) -> Thought {
        let me = &self.fish[i as usize];
        let SpeciesDesc { school, habitat, senses, swim, .. } = &*self.species;
        let spacing = school.spacing * spread_at(school.night_spread, place.daylight);
        let position = me.position;

        // The neighbours: the closest of a few, however many there are around.
        let wanted = (school.neighbours as usize).clamp(1, MOST_NEIGHBOURS);
        let mut near = [(f32::MAX, 0u32); MOST_NEIGHBOURS];
        let mut found = 0;
        self.grid.around(position, |other| {
            if other == i {
                return;
            }
            let distance = self.fish[other as usize].position.distance_squared(position);
            if distance > reach * reach {
                return;
            }
            if let Some(slot) = near[..wanted].iter().position(|&(d, _)| distance < d) {
                near.copy_within(slot..wanted - 1, slot + 1);
                near[slot] = (distance, other);
                found = (found + 1).min(wanted);
            }
        });

        let mut fear = 0.0_f32;
        let (mut apart, mut together, mut along) = (Vec3::ZERO, Vec3::ZERO, Vec3::ZERO);
        for &(distance, other) in &near[..found] {
            let other = &self.fish[other as usize];
            let distance = distance.sqrt().max(0.01);
            if distance < spacing {
                apart += (position - other.position) / distance * (spacing - distance) / spacing;
            }
            together += other.position;
            along += other.velocity;
            fear = fear.max(other.fear * senses.fear_spread);
        }
        if fear < FEAR_LEAST {
            fear = 0.0;
        }

        let mut steer = apart * school.separation * 2.0;
        for threat in threats {
            let away = position - threat.position;
            let distance = away.length();
            let radius = threat.radius * senses.notice;
            if distance < radius {
                let close = 1.0 - distance / radius;
                fear = fear.max((close * 2.0).min(1.0));
                steer += away / distance.max(0.01) * close * senses.flee;
            }
        }
        let scared = me.fear.max(fear);

        if found > 0 {
            let count = found as f32;
            steer += (along / count - me.velocity) * school.alignment * (1.0 - 0.5 * scared);
            steer += (together / count - position) * school.cohesion * (1.0 + 2.0 * scared);
        }

        // Back to the group, and around it when frightened: that closes them into a ball.
        let to_anchor = flat(self.anchor - position);
        let off = to_anchor.length();
        if off > loose {
            steer += to_anchor / off * (off - loose).min(10.0) * school.home;
        }
        if scared > 0.25 {
            let from = flat(position - center);
            steer += Vec3::new(-from.z, 0.0, from.x).normalize_or(Vec3::ZERO) * school.swirl * scared;
        }
        // Nobody swims in a perfectly straight line.
        let time = self.tick as f32 * 0.02 + me.habit * 40.0;
        steer += Vec3::new((time * 0.9).sin(), (time * 0.6).sin() * 0.3, (time * 1.3).cos()) * school.wander;

        // The water it's in.
        let (level, flow) = place.waters.calm_at(position.x, position.z).unwrap_or((position.y + 1.0, Vec3::ZERO));
        let floor = place.floor(position.x, position.z);
        let highest = level - habitat.below_surface * place.waters.wave_scale.max(1.0);
        let lowest = floor + habitat.above_floor;
        let usual_depth = habitat.night_depth + (habitat.day_depth - habitat.night_depth) * place.daylight;
        let calm_depth = match (&habitat.feeding, self.feeding) {
            (Some(feeding), true) => feeding.depth.min(usual_depth),
            _ => usual_depth,
        };
        let wanted_depth = calm_depth + habitat.flee_depth * scared;
        let target = (level - wanted_depth + me.habit * spacing * 2.0).clamp(lowest.min(highest), highest);
        steer.y += (target - position.y).clamp(-2.0, 2.0) * 0.9;
        if position.y > highest {
            steer.y -= (position.y - highest) * 8.0;
        }
        if position.y < lowest {
            steer.y += (lowest - position.y) * 8.0;
        }

        // Shallow water ahead, or a beach: turn towards the deeper side.
        let look = (me.velocity.length() * LOOK_AHEAD_SECONDS).max(LOOK_AHEAD_LEAST);
        let heading = flat(me.velocity).normalize_or(Vec3::NEG_Z);
        let ahead = position + heading * look;
        let depth_ahead = place.depth(ahead.x, ahead.z).unwrap_or(0.0);
        if depth_ahead < habitat.shallowest {
            let tight = ((habitat.shallowest - depth_ahead) / habitat.shallowest).min(1.0);
            let slope = Vec3::new(
                place.floor(position.x - look, position.z) - place.floor(position.x + look, position.z),
                0.0,
                place.floor(position.x, position.z - look) - place.floor(position.x, position.z + look),
            );
            steer += (slope.normalize_or(-heading) - heading * 0.5) * swim.sprint * (1.0 + 3.0 * tight);
        }
        // The inside of a boat.
        if place.waters.in_hull(position + heading * 0.5) {
            steer += (Vec3::NEG_Y - heading) * swim.sprint;
        }

        Thought { fish: i, steer, fear, level, floor, flow }
    }

    /// Everyone moves, thinking or not.
    fn swim(&mut self, dt: f32) {
        let SpeciesDesc { swim, senses, forward, .. } = &*self.species;
        // The model, turned around Y so that it looks where -Z does.
        let facing = Quat::from_rotation_y(f32::atan2(forward.x, -forward.z));
        for fish in &mut self.fish {
            fish.fear = (fish.fear - dt / senses.calm_seconds.max(0.01)).max(0.0);

            let old = fish.velocity.normalize_or(Vec3::NEG_Z);
            let wanted = (fish.velocity + fish.steer * dt).normalize_or(old);
            let most = swim.turn * (1.0 + fish.fear) * dt;
            let angle = old.angle_between(wanted);
            let mut heading = if angle > most { old.lerp(wanted, most / angle).normalize_or(old) } else { wanted };
            heading.y = heading.y.clamp(-STEEPEST, STEEPEST);
            let heading = heading.normalize_or(old);

            let cruise = swim.cruise * (1.0 + fish.habit * 0.1);
            let target = cruise + (swim.sprint - cruise) * fish.fear;
            let speed = fish.velocity.length();
            let speed = speed + (target - speed).clamp(-swim.sprint * dt, swim.sprint * 3.0 * dt);
            fish.velocity = heading * speed;
            fish.position += (fish.velocity + fish.flow) * dt;
            let top = fish.level - 0.15;
            fish.position.y = fish.position.y.min(top).max((fish.floor + 0.05).min(top));

            let turning = f32::atan2(old.cross(heading).y, old.dot(heading)) / (swim.turn * dt).max(1e-4);
            fish.bend += (turning.clamp(-1.0, 1.0) - fish.bend) * (dt * 10.0).min(1.0);
            fish.phase = (fish.phase + (swim.idle_beat + swim.beats_per_meter * speed) * dt).fract();

            let right = heading.cross(Vec3::Y).normalize_or(Vec3::X);
            let up = right.cross(heading);
            let look = Quat::from_mat3(&Mat3::from_cols(right, up, -heading));
            fish.rotation = Quat::from_axis_angle(heading, -fish.bend * TURN_ROLL) * look * facing;
        }
    }
}

fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

/// How much further apart everything is at this hour.
fn spread_at(night_spread: f32, daylight: f32) -> f32 {
    night_spread + (1.0 - night_spread) * daylight
}

/// Finds who is near whom: fish in the cells of a hashed grid, each cell a linked list.
#[derive(Debug, Default)]
struct Grid {
    cell: f32,
    /// The origin: cells are counted in the world, so the same fish share one wherever it is.
    origin: grimora_core::WorldPos,
    heads: Vec<u32>,
    next: Vec<u32>,
}

impl Grid {
    fn fill(&mut self, fish: &[Fish], cell: f32, origin: grimora_core::WorldPos) {
        self.cell = cell.max(0.01);
        self.origin = origin;
        self.heads.clear();
        self.heads.resize(GRID_BUCKETS, GRID_END);
        self.next.clear();
        self.next.resize(fish.len(), GRID_END);
        for (i, fish) in fish.iter().enumerate() {
            let bucket = self.bucket(self.cell_of(fish.position));
            self.next[i] = self.heads[bucket];
            self.heads[bucket] = i as u32;
        }
    }

    fn cell_of(&self, position: Vec3) -> [i32; 3] {
        let cell = ((position.as_dvec3() + self.origin) / f64::from(self.cell)).floor();
        // Wrapping is fine: the cell only picks a bucket, and its neighbours stay its neighbours.
        [cell.x as i64 as i32, cell.y as i64 as i32, cell.z as i64 as i32]
    }

    fn bucket(&self, [x, y, z]: [i32; 3]) -> usize {
        let hash = (x as u32).wrapping_mul(73_856_093)
            ^ (y as u32).wrapping_mul(19_349_663)
            ^ (z as u32).wrapping_mul(83_492_791);
        hash as usize & (GRID_BUCKETS - 1)
    }

    /// Calls `visit` with who may be within a cell of a point: its own cell first, and no
    /// more than `MOST_CANDIDATES`, so that a tight ball costs what a loose school does.
    fn around(&self, position: Vec3, mut visit: impl FnMut(u32)) {
        const ORDER: [i32; 3] = [0, -1, 1];
        let [cx, cy, cz] = self.cell_of(position);
        let mut left = MOST_CANDIDATES;
        for dx in ORDER {
            for dy in ORDER {
                for dz in ORDER {
                    let mut at =
                        self.heads[self.bucket([cx.wrapping_add(dx), cy.wrapping_add(dy), cz.wrapping_add(dz)])];
                    while at != GRID_END {
                        if left == 0 {
                            return;
                        }
                        left -= 1;
                        visit(at);
                        at = self.next[at as usize];
                    }
                }
            }
        }
    }
}

/// A species' swim, worked out once for every fish to share.
#[derive(Debug)]
struct SwimPoses {
    mesh: MeshId,
    /// Matrices in each pose.
    joints: u32,
    matrices: Vec<Mat4>,
}

impl SwimPoses {
    fn new(mesh: MeshId, skeleton: &Skeleton, swim: &SwimDesc) -> Self {
        let rest = skeleton.rest_pose();
        let mut globals = Vec::new();
        skeleton.global_transforms(&rest, &mut globals);
        // The axis is in the model's space and a joint turns in its own.
        let wave: Vec<_> = swim
            .wave
            .iter()
            .filter_map(|joint| {
                let Some(index) = skeleton.joint(&joint.joint) else {
                    log::warn!("swim: the model has no joint {:?}", joint.joint);
                    return None;
                };
                let (_, rotation, head) = globals[index].to_scale_rotation_translation();
                // Out from the middle of the body: turning a bone that hangs down.
                let around = if joint.radial {
                    let out = flat(head).normalize_or(Vec3::X);
                    out.cross(Vec3::Y)
                } else {
                    joint.axis.unwrap_or(swim.axis)
                };
                let axis = (rotation.inverse() * around).normalize_or(Vec3::Y);
                Some((index, axis, joint.sway.to_radians(), joint.delay, joint.bend.to_radians(), joint.squeeze))
            })
            .collect();

        let mut matrices = Vec::new();
        let mut pose = rest.clone();
        for &bend in &POSE_BENDS {
            for &sway in &POSE_SWAYS {
                for phase in 0..POSE_PHASES {
                    let phase = phase as f32 / POSE_PHASES as f32;
                    for &(index, axis, joint_sway, delay, joint_bend, squeeze) in &wave {
                        let beat = ((phase - delay) * std::f32::consts::TAU).sin();
                        // A left turn takes the tail to the left: against the rotation of the turn.
                        let angle = joint_sway * sway * beat - joint_bend * bend;
                        pose[index].rotation = rest[index].rotation * Quat::from_axis_angle(axis, angle);
                        if let Some(squeeze) = squeeze {
                            let closed = (beat * 0.5 + 0.5) * sway.min(1.0);
                            pose[index].scale = rest[index].scale * Vec3::ONE.lerp(squeeze, closed);
                        }
                    }
                    skeleton.global_transforms(&pose, &mut globals);
                    matrices.extend(globals.iter().zip(&skeleton.joints).map(|(g, j)| *g * j.inverse_bind));
                }
            }
        }
        Self { mesh, joints: skeleton.joints.len() as u32, matrices }
    }

    /// How many poses there are.
    fn count(&self) -> usize {
        self.matrices.len() / self.joints.max(1) as usize
    }

    /// One pose's matrices.
    fn pose(&self, pose: usize) -> &[Mat4] {
        let joints = self.joints as usize;
        &self.matrices[pose * joints..(pose + 1) * joints]
    }
}

/// Which pose of a table: `phase` is `0..1`, `sway` an index in `POSE_SWAYS`, `bend` -1 to 1.
fn pose_index(phase: f32, sway: usize, bend: f32) -> usize {
    let phase = ((phase * POSE_PHASES as f32) as usize).min(POSE_PHASES - 1);
    let bend = ((bend + 1.0) * 0.5 * (POSE_BENDS.len() - 1) as f32).round() as usize;
    let bend = bend.min(POSE_BENDS.len() - 1);
    (bend * POSE_SWAYS.len() + sway) * POSE_PHASES + phase
}

/// How wide it swims, as an index in `POSE_SWAYS`, for how many times its cruise it's going at.
fn sway_for(effort: f32) -> usize {
    if effort < 0.8 {
        1
    } else if effort < 1.8 {
        2
    } else {
        3
    }
}

fn fish_pose(fish: &Fish, swim: &SwimDesc) -> usize {
    pose_index(fish.phase, sway_for(fish.velocity.length() / swim.cruise.max(0.01)), fish.bend)
}
