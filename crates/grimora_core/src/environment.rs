//! Lighting and atmosphere settings. All colors are sRGB in `0..=1`;
//! the renderer converts them to linear space.

use std::sync::Arc;

use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::mesh::TextureData;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Environment {
    pub sun: Sun,
    pub ambient: Ambient,
    pub fog: Fog,
    pub sky: Sky,
    pub shadows: Shadows,
    pub post: Post,
    pub wind: Wind,
}

/// Directional light. `direction` points from the ground towards the sun.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sun {
    pub direction: Vec3,
    pub color: Vec3,
    pub intensity: f32,
}

/// Hemisphere ambient light: `sky` lights faces pointing up, `ground` faces pointing down.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ambient {
    pub sky: Vec3,
    pub ground: Vec3,
    pub intensity: f32,
}

/// Exponential height fog. Thicker below `base_height`, thinning out above it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fog {
    pub color: Vec3,
    pub density: f32,
    /// How fast fog thins with altitude. 0 = uniform fog.
    pub height_falloff: f32,
    pub base_height: f32,
    /// How much the fog glows when looking towards the sun.
    pub sun_scatter: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sky {
    pub zenith: Vec3,
    pub horizon: Vec3,
    pub ground: Vec3,
    /// Angular radius of the sun disc, in degrees.
    pub sun_size: f32,
    pub sun_glow: f32,
    /// Picture to draw instead of the plain sun disc, relative to the scene file.
    pub sun_texture: Option<String>,
    /// Picture of the moon, relative to the scene file.
    pub moon_texture: Option<String>,
    /// Half the width of the pictures, in degrees.
    pub sun_sprite_size: f32,
    pub moon_sprite_size: f32,
    /// How visible the stars are, from 0 to 1.
    pub stars: f32,
    /// Where the sun appears, if not where the light comes from. The day cycle
    /// sets these: the light stays above the horizon but the sun goes down.
    #[serde(skip)]
    pub sun_position: Option<Vec3>,
    #[serde(skip)]
    pub moon_position: Option<Vec3>,
    /// Where the source of the light really is, which is where its glow goes.
    #[serde(skip)]
    pub light_position: Option<Vec3>,
}

/// The pictures named by [`Sky`], once loaded.
#[derive(Debug, Clone, Default)]
pub struct SkyTextures {
    pub sun: Option<Arc<TextureData>>,
    pub moon: Option<Arc<TextureData>>,
}

/// What moves vegetation (see `MeshData::wind`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Wind {
    /// Which way it blows, on the ground: x and z.
    pub direction: glam::Vec2,
    /// 1 is a light breeze; 0 is still air.
    pub strength: f32,
    /// How fast the gusts come.
    pub speed: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Shadows {
    pub enabled: bool,
    /// How far from the camera, in world units, shadows reach.
    pub distance: f32,
    /// 0 = no darkening, 1 = full shadow.
    pub strength: f32,
    /// Shadow maps covering ever larger slices of the view (1 to 4): the first
    /// one, close to the camera, is the sharpest.
    pub cascades: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Post {
    pub exposure: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub vignette: f32,
    pub grain: f32,
    pub bloom: f32,
}

/// A local light such as a torch or a glowing crystal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PointLight {
    pub position: Vec3,
    pub color: Vec3,
    pub intensity: f32,
    pub radius: f32,
    /// 0 = steady, 1 = strong fire-like flicker.
    pub flicker: f32,
    /// Lit only at night, coming on at dusk. Needs a day cycle; without one it stays lit.
    pub night_only: bool,
}

impl Default for Sun {
    fn default() -> Self {
        Self { direction: Vec3::new(-0.4, 0.5, -0.6), color: Vec3::new(1.0, 0.85, 0.7), intensity: 3.0 }
    }
}

impl Default for Ambient {
    fn default() -> Self {
        Self { sky: Vec3::new(0.45, 0.5, 0.6), ground: Vec3::new(0.25, 0.2, 0.18), intensity: 0.6 }
    }
}

impl Default for Fog {
    fn default() -> Self {
        Self {
            color: Vec3::new(0.55, 0.6, 0.68),
            density: 0.008,
            height_falloff: 0.08,
            base_height: 0.0,
            sun_scatter: 0.6,
        }
    }
}

impl Default for Sky {
    fn default() -> Self {
        Self {
            zenith: Vec3::new(0.25, 0.38, 0.6),
            horizon: Vec3::new(0.7, 0.72, 0.75),
            ground: Vec3::new(0.3, 0.3, 0.32),
            sun_size: 1.2,
            sun_glow: 0.5,
            sun_texture: None,
            moon_texture: None,
            sun_sprite_size: 7.0,
            moon_sprite_size: 5.0,
            stars: 0.0,
            sun_position: None,
            moon_position: None,
            light_position: None,
        }
    }
}

impl Default for Wind {
    fn default() -> Self {
        Self { direction: glam::Vec2::new(1.0, 0.3), strength: 1.0, speed: 1.6 }
    }
}

impl Default for Shadows {
    fn default() -> Self {
        Self { enabled: true, distance: 120.0, strength: 0.85, cascades: 4 }
    }
}

impl Default for Post {
    fn default() -> Self {
        Self { exposure: 1.0, contrast: 1.05, saturation: 1.0, vignette: 0.3, grain: 0.03, bloom: 0.15 }
    }
}

impl Default for PointLight {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            color: Vec3::new(1.0, 0.6, 0.3),
            intensity: 4.0,
            radius: 8.0,
            flicker: 0.0,
            night_only: false,
        }
    }
}

/// Converts one sRGB channel to linear.
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

pub fn srgb_to_linear_vec(c: Vec3) -> Vec3 {
    Vec3::new(srgb_to_linear(c.x), srgb_to_linear(c.y), srgb_to_linear(c.z))
}
