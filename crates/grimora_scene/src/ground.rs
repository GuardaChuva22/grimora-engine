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
        Ground::from_name(path.rsplit(['/', '\\']).next().unwrap_or(path))
    }

    /// A guess from a material's or a picture's name, in Portuguese or English. Metal and
    /// wood come first: a stone well's planks, an iron-bound chest's boards, say what they are.
    pub fn from_name(name: &str) -> Option<Ground> {
        let name = name.to_lowercase();
        let has = |words: &[&str]| words.iter().any(|word| name.contains(word));
        Some(if has(&["ferro", "metal", "iron", "steel", "bronze", "cobre", "copper", "grade", "chapa"]) {
            Ground::Metal
        } else if has(&[
            "madeira",
            "tabua",
            "wood",
            "plank",
            "timber",
            "deck",
            "hull",
            "trim",
            "crate",
            "seat",
            "endgrain",
            "cupboard",
            "bark",
            "casca",
            "tronco",
            "caule",
            "cepo",
            "aduela",
            "boia",
            "barril",
            "door",
            "porta",
            "piso_madeira",
            "lenha",
            "galho",
            "branch",
            "log",
        ]) {
            Ground::Wood
        } else if has(&["areia", "sand", "duna"]) {
            Ground::Sand
        } else if has(&["lama", "mud", "pantano", "lodo", "swamp"]) {
            Ground::Mud
        } else if has(&[
            "pedra", "stone", "rock", "rocha", "tijolo", "brick", "laje", "slab", "marmore", "marble", "carved",
            "groove", "laterita", "cascalho", "gravel", "seixo", "pebble", "calcada", "cobble", "telha", "pilar",
            "altar", "matacao",
        ]) {
            Ground::Stone
        } else if has(&[
            "grama", "grass", "capim", "palha", "straw", "thatch", "folha", "leaf", "musgo", "moss", "feno",
        ]) {
            Ground::Grass
        } else if has(&[
            "terra",
            "dirt",
            "barro",
            "clay",
            "taipa",
            "cupinzeiro",
            "mound",
            "ash",
            "cinza",
            "solo",
            "soil",
        ]) {
            Ground::Dirt
        } else {
            return None;
        })
    }

    /// What one stands on when on top of `mesh`: each material's guess, weighed by how much
    /// of its surface faces up. `None` when no material says.
    pub fn of_mesh(mesh: &grimora_core::MeshData) -> Option<Ground> {
        let guesses: Vec<Option<Ground>> = mesh
            .materials
            .iter()
            .map(|m| {
                let named = m.name.as_deref().and_then(Ground::from_name);
                named.or_else(|| m.base_color_texture.as_ref().and_then(|t| Ground::from_picture(&t.name)))
            })
            .collect();
        let mut weight = [0.0f32; Ground::ALL.len()];
        let mut weigh = |material: usize, indices: &[u32]| {
            let Some(Some(ground)) = guesses.get(material) else { return };
            let slot = Ground::ALL.iter().position(|g| g == ground).unwrap_or(0);
            for t in indices.chunks_exact(3) {
                let p = |i: u32| glam::Vec3::from(mesh.vertices[i as usize].position);
                let up = (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])).y.abs();
                // Faces seen from the side still count a little: a log's flank, a rock's face.
                weight[slot] += up + 0.01;
            }
        };
        if mesh.submeshes.is_empty() {
            weigh(0, &mesh.indices);
        } else {
            for sub in &mesh.submeshes {
                weigh(sub.material, &mesh.indices[sub.indices.start as usize..sub.indices.end as usize]);
            }
        }
        let (best, most) = weight.iter().enumerate().fold((0, 0.0), |b, (i, &w)| if w > b.1 { (i, w) } else { b });
        (most > 0.0).then_some(Ground::ALL[best])
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
