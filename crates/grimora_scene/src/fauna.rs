//! Animals, as data: what a species is like and where a scene keeps them. They live in
//! `grimora::fauna`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use glam::Vec3;
use grimora_core::{MeshData, ParticleEffect};
use serde::{Deserialize, Serialize};

use crate::effect::load_effect;
use crate::model::{DEFAULT_MAX_TEXTURE_SIZE, load_model};

/// A group of animals in a scene file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FaunaDesc {
    /// A species file, from the scene's directory.
    pub species: String,
    /// The middle of where they live; `y` is ignored, the species picks its own depth.
    pub position: Vec3,
    /// How far from `position` they go.
    pub radius: f32,
    pub count: u32,
    pub seed: u64,
}

impl Default for FaunaDesc {
    fn default() -> Self {
        Self { species: String::new(), position: Vec3::ZERO, radius: 40.0, count: 100, seed: 1 }
    }
}

/// How a species gets about, and so what the engine does with it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Life {
    /// In a school, under the water.
    #[default]
    Schooling,
    /// In a flock, on `flight`.
    Flying,
    /// On the ground, from a hole, on `crawl`.
    Crawling,
    /// Over the bottom, on `glide`.
    Gliding,
    /// Where the water takes it, on `drift`.
    Drifting,
    /// Cattle on the ground, in a village's pen and pasture.
    Grazing,
}

/// What a kind of animal is like: the file in `assets/fauna`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpeciesDesc {
    pub name: String,
    /// What the player reads in the bestiary.
    pub description: String,
    pub life: Life,
    /// From the species file's directory.
    pub model: String,
    /// Which way the model looks, flat on the ground.
    pub forward: Vec3,
    /// Each animal's scale is somewhere in `(smallest, largest)`.
    pub size: (f32, f32),
    pub swim: SwimDesc,
    /// Whether it casts a shadow. Worth it for a few birds, not for hundreds of fish.
    pub shadow: bool,
    /// The `name`s of the species it eats.
    pub diet: Vec<String>,
    pub school: SchoolDesc,
    pub habitat: HabitatDesc,
    pub senses: SensesDesc,
    pub needs: NeedsDesc,
    pub flight: FlightDesc,
    pub crawl: CrawlDesc,
    pub glide: GlideDesc,
    pub drift: DriftDesc,
    /// An effect, from the species file's directory, for when it goes in or out of the sand.
    pub puff: Option<String>,
    /// How much it glows in the dark: 1 glows at its color.
    pub glow: f32,
    pub cycle: CycleDesc,
    /// What it does to whoever touches it.
    pub harm: Option<HarmDesc>,
    pub sounds: SoundsDesc,
}

/// How an animal hurts. Who gets hurt, and when, is up to the kind of animal: a jellyfish stings
/// anything it touches, a ray whoever steps on it and a crab whoever corners it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HarmDesc {
    pub damage: f32,
    /// In meters, from the animal to the edge of the body.
    pub reach: f32,
    /// Seconds before it can do it again.
    pub every: f32,
    /// Whether it makes someone flinch, or only burns.
    pub staggers: bool,
    /// For whoever it happens to.
    pub message: Option<String>,
}

impl Default for HarmDesc {
    fn default() -> Self {
        Self { damage: 1.0, reach: 0.4, every: 1.5, staggers: false, message: None }
    }
}

/// The noises of a species, each from the species file's directory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundsDesc {
    /// What it says when nothing is happening, any of them.
    pub calls: Vec<PathBuf>,
    /// `(soonest, latest)`, in seconds between calls.
    pub call_every: (f32, f32),
    pub harm: Option<PathBuf>,
    pub death: Option<PathBuf>,
    /// Going in or out of the sand.
    pub dig: Option<PathBuf>,
    /// Turning to face something.
    pub facing: Option<PathBuf>,
    pub volume: f32,
}

impl Default for SoundsDesc {
    fn default() -> Self {
        Self {
            calls: Vec::new(),
            call_every: (8.0, 25.0),
            harm: None,
            death: None,
            dig: None,
            facing: None,
            volume: 1.0,
        }
    }
}

impl SoundsDesc {
    fn resolved(&self, dir: &Path) -> Self {
        let from = |file: &Option<PathBuf>| file.as_ref().map(|file| dir.join(file));
        Self {
            calls: self.calls.iter().map(|call| dir.join(call)).collect(),
            harm: from(&self.harm),
            death: from(&self.death),
            dig: from(&self.dig),
            facing: from(&self.facing),
            ..self.clone()
        }
    }
}

impl Default for SpeciesDesc {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            life: Life::default(),
            model: String::new(),
            forward: Vec3::NEG_Z,
            size: (1.0, 1.0),
            swim: SwimDesc::default(),
            shadow: false,
            diet: Vec::new(),
            school: SchoolDesc::default(),
            habitat: HabitatDesc::default(),
            senses: SensesDesc::default(),
            needs: NeedsDesc::default(),
            flight: FlightDesc::default(),
            crawl: CrawlDesc::default(),
            glide: GlideDesc::default(),
            drift: DriftDesc::default(),
            puff: None,
            glow: 0.0,
            cycle: CycleDesc::default(),
            harm: None,
            sounds: SoundsDesc::default(),
        }
    }
}

/// How it swims, and the wave that runs down its body while it does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SwimDesc {
    pub cruise: f32,
    /// Flat out, frightened.
    pub sprint: f32,
    /// Radians per second, calm. Fear doubles it.
    pub turn: f32,
    /// Beats of the tail for each meter swum, on top of `idle_beat` per second.
    pub beats_per_meter: f32,
    pub idle_beat: f32,
    /// In the model's space: what the body bends around.
    pub axis: Vec3,
    pub wave: Vec<WaveJoint>,
}

impl Default for SwimDesc {
    fn default() -> Self {
        Self {
            cruise: 1.0,
            sprint: 4.0,
            turn: 4.0,
            beats_per_meter: 2.0,
            idle_beat: 1.5,
            axis: Vec3::Y,
            wave: Vec::new(),
        }
    }
}

/// One joint of the swimming wave.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WaveJoint {
    pub joint: String,
    /// Degrees either way, at a cruise. It adds to what the joints above it did.
    pub sway: f32,
    /// How far behind the start of the wave, in whole beats.
    pub delay: f32,
    /// Degrees at the sharpest turn.
    pub bend: f32,
    /// What it turns around, in the model's space, if it isn't `SwimDesc::axis`.
    pub axis: Option<Vec3>,
    /// `sway` moves it away from the middle of the body and back, whichever side it's on.
    pub radial: bool,
    /// Its scale at the height of the beat, and back to 1 at the bottom.
    pub squeeze: Option<Vec3>,
}

/// How it keeps with the others.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SchoolDesc {
    /// The distance it tries to keep from each neighbour.
    pub spacing: f32,
    /// How far it notices its neighbours.
    pub reach: f32,
    /// How many of them it pays attention to. Real fish follow a handful, however big the
    /// school is, and that keeps the cost of each one from growing with it.
    pub neighbours: u32,
    pub separation: f32,
    pub alignment: f32,
    pub cohesion: f32,
    /// The pull back to the middle of the school's wandering.
    pub home: f32,
    /// How hard the frightened circle, which closes them into a ball.
    pub swirl: f32,
    /// `spacing` is multiplied by this at night.
    pub night_spread: f32,
    /// How fast the whole school travels, as a part of `swim.cruise`.
    pub roam: f32,
    /// Each one's own unsteadiness, in meters per second per second.
    pub wander: f32,
}

impl Default for SchoolDesc {
    fn default() -> Self {
        Self {
            spacing: 0.4,
            reach: 1.2,
            neighbours: 7,
            separation: 2.0,
            alignment: 1.5,
            cohesion: 0.5,
            home: 0.5,
            swirl: 3.0,
            night_spread: 1.0,
            roam: 0.5,
            wander: 0.3,
        }
    }
}

/// Where in the water it wants to be. Depths are meters under the surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HabitatDesc {
    pub day_depth: f32,
    pub night_depth: f32,
    /// It doesn't come closer to the surface than this, more when the waves are high.
    pub below_surface: f32,
    pub above_floor: f32,
    /// It turns back from water shallower than this.
    pub shallowest: f32,
    /// Now and then it comes up to feed. Without it, it stays at its depth.
    pub feeding: Option<FeedingDesc>,
    /// How much deeper fear sends it.
    pub flee_depth: f32,
}

/// The bouts of a school coming up to feed: that's when birds can get at it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FeedingDesc {
    pub depth: f32,
    /// Seconds between bouts: (shortest, longest).
    pub every: (f32, f32),
    /// Seconds a bout lasts: (shortest, longest).
    pub lasts: (f32, f32),
}

impl Default for FeedingDesc {
    fn default() -> Self {
        Self { depth: 1.2, every: (30.0, 90.0), lasts: (20.0, 50.0) }
    }
}

impl Default for HabitatDesc {
    fn default() -> Self {
        Self {
            day_depth: 4.0,
            night_depth: 2.0,
            below_surface: 0.6,
            above_floor: 0.4,
            shallowest: 1.5,
            feeding: None,
            flee_depth: 2.5,
        }
    }
}

/// What frightens it, and how that goes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SensesDesc {
    /// How much of its neighbour's fear it catches: below 1, or nothing would ever calm down.
    pub fear_spread: f32,
    /// Seconds to calm down from the worst fright.
    pub calm_seconds: f32,
    /// How hard it swims away, in meters per second per second.
    pub flee: f32,
    /// How many times further than the danger's `radius` it notices it.
    pub notice: f32,
}

impl Default for SensesDesc {
    fn default() -> Self {
        Self { fear_spread: 0.85, calm_seconds: 5.0, flee: 14.0, notice: 1.0 }
    }
}

/// How fast it gets hungry and tired, in seconds from nothing to all of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NeedsDesc {
    pub hungry_after: f32,
    pub tired_after: f32,
    /// From exhausted to rested, standing still.
    pub rested_after: f32,
}

impl Default for NeedsDesc {
    fn default() -> Self {
        Self { hungry_after: 300.0, tired_after: 240.0, rested_after: 90.0 }
    }
}

/// A flying animal. Having `flight` in the file makes a species a flock instead of a school.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FlightDesc {
    /// Above 0, the species flies.
    pub cruise: f32,
    pub dive_speed: f32,
    pub walk: f32,
    /// Radians per second.
    pub turn: f32,
    /// Degrees it leans into a turn, and tilts back to brake.
    pub bank: f32,
    pub flare: f32,
    /// Over the land or water below: (lowest, highest).
    pub altitude: (f32, f32),
    /// From the model's origin, under its feet, to its body.
    pub body_height: f32,
    /// Seconds of gliding between bouts of beating its wings, and how long a bout is.
    pub glide: f32,
    pub beat_bout: f32,
    pub take_off: f32,
    /// How far around it can see food.
    pub sight: f32,
    /// How deep something can be and still be seen from the air.
    pub sees_down: f32,
    /// How wide it circles over food.
    pub circle: f32,
    /// How far to its side food can be for it to dive, and how likely it does, each look.
    pub dive_reach: f32,
    pub dive_chance: f32,
    /// How close to where it enters the water a fish must be to be caught, and how deep it goes.
    pub catch_radius: f32,
    pub plunge: f32,
    /// Seconds under the water.
    pub under: f32,
    /// How much hunger one catch takes away.
    pub meal: f32,
    /// Seconds on the water: (after missing, after eating).
    pub swim: (f32, f32),
}

impl Default for FlightDesc {
    fn default() -> Self {
        Self {
            cruise: 0.0,
            dive_speed: 15.0,
            walk: 0.5,
            turn: 1.1,
            bank: 30.0,
            flare: 30.0,
            altitude: (10.0, 24.0),
            body_height: 0.2,
            glide: 6.0,
            beat_bout: 1.5,
            take_off: 1.8,
            sight: 70.0,
            sees_down: 2.5,
            circle: 11.0,
            dive_reach: 12.0,
            dive_chance: 0.2,
            catch_radius: 0.8,
            plunge: 0.8,
            under: 0.35,
            meal: 0.5,
            swim: (1.5, 5.0),
        }
    }
}

/// How a species' numbers change, all in game days. Left out, they never do: only what's caught
/// is gone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CycleDesc {
    /// Young per animal, per day, with room and food for them.
    pub births: f32,
    /// How long one lives, on average; 0 is until something gets it.
    pub lives: f32,
    /// How many a place can feed, as a multiple of the group's `count`.
    pub room: f32,
    /// How long one lasts with nothing to eat; 0 never starves.
    pub starves_in: f32,
    /// How often one comes from elsewhere, while there's room; 0 nobody does.
    pub arrives_every: f32,
    /// Its size when it's born, as a share of its full size, and how long until it gets there.
    pub newborn: f32,
    pub grows_in: f32,
}

impl CycleDesc {
    /// Whether anything ever changes.
    pub fn alive(&self) -> bool {
        self.births > 0.0 || self.lives > 0.0 || self.starves_in > 0.0 || self.arrives_every > 0.0
    }
}

impl Default for CycleDesc {
    fn default() -> Self {
        Self { births: 0.0, lives: 0.0, room: 1.0, starves_in: 0.0, arrives_every: 0.0, newborn: 1.0, grows_in: 0.0 }
    }
}

/// Something on legs, with a hole to go to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CrawlDesc {
    pub walk: f32,
    pub run: f32,
    /// Radians per second.
    pub turn: f32,
    /// How far a foot may be left behind before it steps, and how high it lifts.
    pub step: f32,
    pub lift: f32,
    /// Where a hole can be: (under the level, over the level), in meters from it.
    pub shore: (f32, f32),
    /// How far it goes from its hole.
    pub roam: f32,
    pub smell: f32,
    /// It stands its ground against what's closer than this, if its hole is further.
    pub cornered: f32,
    /// How close another may come before they square up.
    pub territory: f32,
    /// The share of them that come out by day too.
    pub bold: f32,
}

impl Default for CrawlDesc {
    fn default() -> Self {
        Self {
            walk: 0.3,
            run: 1.5,
            turn: 2.4,
            step: 0.08,
            lift: 0.05,
            shore: (-1.0, 2.0),
            roam: 7.0,
            smell: 12.0,
            cornered: 1.5,
            territory: 0.7,
            bold: 0.25,
        }
    }
}

/// Something that lies on the bottom and glides over it, with `SwimDesc`'s speeds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GlideDesc {
    /// How high over the bottom it cruises: (lowest, highest).
    pub over_floor: (f32, f32),
    /// The least water it lies in.
    pub shallowest: f32,
    pub smell: f32,
    pub catch_radius: f32,
    pub roam: f32,
    /// Seconds it stays down: (shortest, longest).
    pub rest: (f32, f32),
    /// How much hunger one catch takes away.
    pub meal: f32,
}

impl Default for GlideDesc {
    fn default() -> Self {
        Self {
            over_floor: (0.35, 1.3),
            shallowest: 0.75,
            smell: 15.0,
            catch_radius: 0.45,
            roam: 16.0,
            rest: (6.0, 16.0),
            meal: 0.6,
        }
    }
}

/// Something the water carries, beating to stay up.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DriftDesc {
    /// Beats per second, at its depth.
    pub beat: f32,
    /// In meters per second: how hard a beat pushes, how much it sinks between them.
    pub thrust: f32,
    pub sink: f32,
    pub drag: f32,
    /// In meters per second: the current it's in, and how a storm's wind pushes it.
    pub current: f32,
    pub storm_push: f32,
    pub day_depth: f32,
    pub night_depth: f32,
    /// Water shallower than this makes it turn back, and under the second it's on the sand.
    pub shallowest: f32,
    pub stranded: f32,
    /// Seconds on the sand until there's nothing left, and seconds of eating in it.
    pub rots_in: f32,
    pub food: f32,
}

impl Default for DriftDesc {
    fn default() -> Self {
        Self {
            beat: 0.4,
            thrust: 0.6,
            sink: 0.19,
            drag: 1.3,
            current: 0.14,
            storm_push: 1.0,
            day_depth: 5.0,
            night_depth: 2.0,
            shallowest: 3.0,
            stranded: 0.3,
            rots_in: 240.0,
            food: 30.0,
        }
    }
}

/// A [`FaunaDesc`] with its files loaded.
#[derive(Debug, Clone)]
pub struct SceneFauna {
    pub species: Arc<SpeciesDesc>,
    /// Into [`Scene::meshes`](crate::Scene::meshes). Groups of one species share it.
    pub mesh: usize,
    pub position: Vec3,
    pub radius: f32,
    pub count: u32,
    pub seed: u64,
    /// `SpeciesDesc::puff`, loaded.
    pub puff: Option<ParticleEffect>,
}

/// Reads a species file.
pub fn load_species(path: impl AsRef<Path>) -> Result<SpeciesDesc> {
    let path = path.as_ref();
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    parse_species(&text).with_context(|| format!("parsing {}", path.display()))
}

/// Parses the text of a species file.
pub fn parse_species(text: &str) -> Result<SpeciesDesc> {
    let options = ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
    Ok(options.from_str(text)?)
}

/// Loads the groups of a scene, adding each species' model to `meshes` once. One that
/// doesn't load is only logged: the scene goes on without it.
pub fn load_fauna(groups: &[FaunaDesc], dir: &Path, meshes: &mut Vec<MeshData>) -> Vec<SceneFauna> {
    let mut loaded: Vec<(&str, Arc<SpeciesDesc>, usize)> = Vec::new();
    let mut fauna = Vec::new();
    for group in groups {
        let known = loaded.iter().find(|(path, ..)| *path == group.species);
        let (species, mesh) = match known {
            Some((_, species, mesh)) => (species.clone(), *mesh),
            None => match load_species_and_model(&dir.join(&group.species), meshes) {
                Ok((species, mesh)) => {
                    loaded.push((&group.species, species.clone(), mesh));
                    (species, mesh)
                }
                Err(err) => {
                    log::error!("{err:#}");
                    continue;
                }
            },
        };
        let puff = species.puff.as_ref().and_then(|puff| {
            let file = dir.join(&group.species);
            let path = file.parent().unwrap_or(Path::new("")).join(puff);
            load_effect(&path).map_err(|err| log::error!("{err:#}")).ok()
        });
        fauna.push(SceneFauna {
            puff,
            species,
            mesh,
            position: group.position,
            radius: group.radius.max(1.0),
            count: group.count,
            seed: group.seed,
        });
    }
    fauna
}

fn load_species_and_model(path: &Path, meshes: &mut Vec<MeshData>) -> Result<(Arc<SpeciesDesc>, usize)> {
    let mut species = load_species(path)?;
    let dir = path.parent().unwrap_or(Path::new(""));
    species.sounds = species.sounds.resolved(dir);
    let model = dir.join(&species.model);
    let mut mesh = load_model(&model, DEFAULT_MAX_TEXTURE_SIZE)?;
    mesh.shadowless = !species.shadow;
    meshes.push(mesh);
    Ok((Arc::new(species), meshes.len() - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlisted_fields_use_their_defaults() {
        let species = parse_species(r#"(name: "x", swim: (cruise: 2.0), school: (spacing: 1.0))"#).unwrap();
        assert_eq!(species.swim.cruise, 2.0);
        assert_eq!(species.swim.sprint, SwimDesc::default().sprint);
        assert_eq!(species.school.spacing, 1.0);
        assert_eq!(species.habitat, HabitatDesc::default());
    }

    #[test]
    fn the_sardine_file_loads() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fauna/sardinha.ron");
        let species = load_species(path).unwrap();
        assert_eq!(species.name, "sardinha");
        assert_eq!(species.swim.wave.len(), 5);
    }
}
