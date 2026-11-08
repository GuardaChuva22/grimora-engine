//! What the ground is made of, for the sound a step makes on it.

use serde::{Deserialize, Serialize};

/// The stuff underfoot. Terrain layers, paths and objects each name one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Ground {
    Grass,
    #[default]
    Dirt,
    Sand,
    Mud,
    Stone,
    Wood,
    Metal,
}

impl Ground {
    pub const ALL: [Ground; 7] =
        [Ground::Grass, Ground::Dirt, Ground::Sand, Ground::Mud, Ground::Stone, Ground::Wood, Ground::Metal];

    /// The folder its step sounds are in, under `assets/sounds/steps/<feet>/`.
    pub fn folder(self) -> &'static str {
        match self {
            Ground::Grass => "grama",
            Ground::Dirt => "terra_seca",
            Ground::Sand => "areia",
            Ground::Mud => "lama",
            Ground::Stone => "pedra",
            Ground::Wood => "madeira",
            Ground::Metal => "metal",
        }
    }

    /// A guess from a picture's file name (`tabuas_de_madeira.png` is wood), for objects
    /// that don't say what they are.
    pub fn from_picture(path: &str) -> Option<Ground> {
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path).to_lowercase();
        let has = |words: &[&str]| words.iter().any(|word| name.contains(word));
        Some(if has(&["madeira", "tabua", "wood", "plank"]) {
            Ground::Wood
        } else if has(&["ferro", "metal", "iron"]) {
            Ground::Metal
        } else if has(&["areia", "sand"]) {
            Ground::Sand
        } else if has(&["grama", "grass"]) {
            Ground::Grass
        } else if has(&["lama", "mud", "pantano"]) {
            Ground::Mud
        } else if has(&["terra", "dirt"]) {
            Ground::Dirt
        } else {
            return None;
        })
    }

    /// Rain turns soft ground into mud; sand, stone, wood and metal only get wet.
    pub fn soaked(self, wet: f32) -> Ground {
        match self {
            Ground::Grass | Ground::Dirt if wet > MUD_FROM => Ground::Mud,
            other => other,
        }
    }
}

/// How wet the ground has to be (`Precipitation::wet`) before dirt and grass sound like mud.
pub const MUD_FROM: f32 = 0.6;

/// What each terrain layer is, in the order of [`TerrainTextures`](crate::TerrainTextures).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerrainGrounds {
    pub grass: Ground,
    pub dirt: Ground,
    pub rock: Ground,
}

impl Default for TerrainGrounds {
    fn default() -> Self {
        Self { grass: Ground::Grass, dirt: Ground::Dirt, rock: Ground::Stone }
    }
}
