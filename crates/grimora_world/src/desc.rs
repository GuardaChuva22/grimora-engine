use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use glam::{DVec2, Vec3};
use grimora_core::WaterLook;
use serde::{Deserialize, Serialize};

use crate::LayerKind;

/// A generated world's `.ron`: not the content, but what to generate it from. Paths are
/// relative to the file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WorldDesc {
    /// The map's `legend.ron`; its layers sit beside it.
    pub map: PathBuf,
    /// Which layers to load; empty loads them all.
    pub layers: Vec<LayerKind>,
    /// Land masks cut from the base painting, which win over the topography's land and sea.
    pub coasts: Vec<CoastDesc>,
    /// The part of the map that is generated, in map units.
    pub window: MapWindow,
    /// Where the player starts, in map units (x east, y south); `--at` overrides it.
    pub start: DVec2,
    pub seed: u64,
    /// The pictures on the ground.
    pub ground: GroundLook,
    /// How far the land is drawn, in meters; fog and the horizon close the rest.
    pub view: f64,
    /// How the streams and ponds look; where they are comes from the ground.
    pub fresh_water: FreshWaterLook,
    /// What lives here, by biome: plants, ambience and animals (a `LifeBook` file). Empty is
    /// a bare world.
    pub life: Option<PathBuf>,
}

/// The streams' and ponds' pictures, relative to the world file, and colors.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FreshWaterLook {
    pub texture: Option<PathBuf>,
    pub foam_texture: Option<PathBuf>,
    /// Meters one repeat of the picture covers.
    pub tile: f32,
    /// Height of the ripples, in meters.
    pub waves: f32,
    pub stream: WaterLook,
    pub pond: WaterLook,
    /// The sea's look, which a stream takes on as it nears its mouth. `None`: its own.
    pub sea: Option<WaterLook>,
    /// Played where a stream falls: mist and roar, relative to the world file.
    pub fall_effect: Option<PathBuf>,
    pub fall_sound: Option<PathBuf>,
}

impl Default for FreshWaterLook {
    fn default() -> Self {
        Self {
            texture: Some(PathBuf::from("../textures/agua.png")),
            foam_texture: None,
            fall_effect: Some(PathBuf::from("../effects/waterfall.ron")),
            fall_sound: Some(PathBuf::from("../sounds/waterfall.wav")),
            tile: 3.0,
            waves: 0.03,
            stream: WaterLook {
                deep: Vec3::new(0.04, 0.26, 0.24),
                shallow: Vec3::new(0.32, 0.55, 0.4),
                clarity: 3.0,
                murk: 0.4,
                // The banks are shallow ramps, so the contact foam rings wide: keep it faint.
                foam: 0.1,
                foam_color: Vec3::new(0.72, 0.8, 0.74),
                ..Default::default()
            },
            sea: None,
            pond: WaterLook {
                deep: Vec3::new(0.03, 0.18, 0.2),
                shallow: Vec3::new(0.25, 0.45, 0.36),
                clarity: 2.5,
                murk: 0.5,
                foam: 0.08,
                foam_color: Vec3::new(0.72, 0.8, 0.74),
                ..Default::default()
            },
        }
    }
}

/// The ground's pictures, relative to the world file; `tile` is the meters one repeat covers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GroundLook {
    pub grass: PathBuf,
    pub sand: PathBuf,
    pub rock: PathBuf,
    /// Under fresh water: pebbles, and mud near the sea. The sand when not given.
    pub bed: Option<PathBuf>,
    pub mud: Option<PathBuf>,
    pub tile: f64,
}

impl Default for GroundLook {
    fn default() -> Self {
        Self {
            grass: PathBuf::from("../textures/grama1.png"),
            sand: PathBuf::from("../textures/areia_de_praia.png"),
            rock: PathBuf::from("../textures/rocha_umida_escura.png"),
            bed: None,
            mud: None,
            tile: 5.0,
        }
    }
}

impl Default for WorldDesc {
    fn default() -> Self {
        Self {
            map: PathBuf::from("nyraldar/legend.ron"),
            layers: Vec::new(),
            coasts: Vec::new(),
            window: MapWindow::default(),
            start: DVec2::ZERO,
            seed: 0,
            ground: GroundLook::default(),
            view: 16_000.0,
            fresh_water: FreshWaterLook::default(),
            life: None,
        }
    }
}

/// A rectangle of the map, in map units: `x`, `y` is its north-west corner.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct MapWindow {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl MapWindow {
    pub fn contains(&self, p: DVec2) -> bool {
        p.x >= self.x && p.y >= self.y && p.x < self.x + self.width && p.y < self.y + self.height
    }

    pub fn center(&self) -> DVec2 {
        DVec2::new(self.x + self.width * 0.5, self.y + self.height * 0.5)
    }
}

/// A land mask: a grey PNG, 0 sea and anything else land, whose north-west texel starts at
/// `x`, `y` (map units), each texel `texel` units wide.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoastDesc {
    pub file: PathBuf,
    pub x: f64,
    pub y: f64,
    pub texel: f64,
}

impl WorldDesc {
    pub fn load(path: impl AsRef<Path>) -> Result<(Self, PathBuf)> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let desc: Self = ron::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        Ok((desc, dir))
    }
}
