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
    pub surface: SurfaceLook,
    pub precipitation: Precipitation,
    /// Set each frame from the world's water.
    #[serde(skip)]
    pub water: crate::water::WaterFrame,
    /// The body a blazing aura burns around, set each frame: capsules along its bones
    /// (two ends and a radius), most important first.
    #[serde(skip)]
    pub fire_body: Vec<(glam::Vec3, glam::Vec3, f32)>,
    /// The burning body's feet, which its fire's turbulence rides with, and where its
    /// flames trail to as it moves (m), set each frame.
    #[serde(skip)]
    pub fire_anchor: glam::Vec3,
    #[serde(skip)]
    pub fire_trail: glam::Vec3,
    /// Set each frame from the world's origin: what local position zero is in the world, for
    /// the patterns drawn fixed in it.
    #[serde(skip)]
    pub origin: crate::Origin,
}

/// What is falling and what it has done to the ground. A day cycle sets all of this, and
/// a scene without one can still be written with snow on the ground.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Precipitation {
    /// How hard it rains, from 0 to 1.
    pub rain: f32,
    /// How hard it snows, from 0 to 1.
    pub snow: f32,
    /// How wet surfaces are, from 0 to 1.
    pub wet: f32,
    /// How much snow lies on what faces up, from 0 to 1.
    pub snow_cover: f32,
    /// How bright the lightning is right now.
    #[serde(skip)]
    pub flash: f32,
}

/// What every lit surface goes through, on top of its own textures.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SurfaceLook {
    /// Pushes a texture's texels away from its average color. 1 leaves it alone.
    pub contrast: f32,
    /// How colorful surfaces are. 1 leaves them alone.
    pub saturation: f32,
    /// How much large stains, fixed in the world, break up the surfaces, from 0 to 1.
    /// They hide the repeats of a texture and are streaked down walls and cliffs.
    pub mottle: f32,
    /// Meters that one large stain measures, roughly.
    pub mottle_size: f32,
    /// What the dark and the bright parts of the stains multiply the color by.
    pub mottle_dark: Vec3,
    pub mottle_light: Vec3,
    /// How many mip levels sharper textures are kept. Far away and at grazing angles
    /// they stay crunchy instead of blurring into flat color, at the price of some shimmer.
    pub crisp: f32,
    /// How much the stains keep their color, from 0 (gray) to 1. The day cycle sets it,
    /// by the hour: what makes the night dreamlike only looks painted in the sun.
    #[serde(skip)]
    pub stain_color: f32,
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
    pub mist: Mist,
    /// The air itself, past the fog: how far a person sees along the ground, in meters, before
    /// everything has melted into the sky's haze. Near things stay clear and the haze grows
    /// faster with distance, as in a painting. 0 (the default) leaves it out.
    pub air: f32,
    /// How high the air thins to a third, in meters: a mountain's top is clearer than its foot.
    pub air_height: f32,
}

/// Banks of mist lying low in the world, thick in patches, that drift with the wind.
/// `amount` 0 (the default) leaves it out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Mist {
    /// How much it hides per meter inside a bank.
    pub amount: f32,
    /// World height of its top, whatever the ground does. It fills the low places.
    pub top: f32,
    /// How wide the banks are, in meters.
    pub size: f32,
    /// How fast they drift, in meters per second at wind direction length 1.
    pub speed: f32,
    /// Its color, as a multiple of the fog's.
    pub brightness: f32,
    /// A multiple of `amount`. The day cycle sets it, by the hour.
    #[serde(skip)]
    pub strength: f32,
}

impl Default for Mist {
    fn default() -> Self {
        Self { amount: 0.0, top: 5.0, size: 18.0, speed: 0.4, brightness: 1.5, strength: 1.0 }
    }
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
    /// How far the sun or moon lights up the sky around it.
    pub halo: f32,
    pub clouds: Clouds,
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

/// Marbled clouds and a glow of color behind them, painted on the sky.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Clouds {
    /// How much of the sky they cover, from 0 (no clouds) to 1.
    pub amount: f32,
    /// Their color in the thin and shaded parts, and where the light catches them.
    pub dark: Vec3,
    pub light: Vec3,
    /// The glow in patches behind the clouds, which also gathers the faintest stars.
    pub nebula: Vec3,
    /// How many clouds fit in the sky: more makes them smaller.
    pub scale: f32,
    /// How fast they drift with the wind.
    pub speed: f32,
    /// Their shape, from 0 (marbled wisps) to 1 (separate round heaps).
    pub puff: f32,
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
    /// How hard it blows at the moment, as a multiple of `strength`. The wind (`WindSim`) sets this.
    #[serde(skip)]
    pub scale: f32,
    /// How fast the plants flutter.
    pub speed: f32,
    /// Degrees the wind wanders either side of `direction` over the day (twice that in a storm).
    pub wander: f32,
    /// How gusty it is, as a multiple of the weather's own.
    pub gusty: f32,
    /// The weather's wind as a multiple of the scene's; the day cycle sets it.
    #[serde(skip)]
    pub weather: f32,
    /// Which way it blows now (unit, x and z), and how hard in m/s, before gusts.
    #[serde(skip)]
    pub blowing: glam::Vec2,
    #[serde(skip)]
    pub speed_ms: f32,
    /// How much gusts swing it: see `wind::gust_at`.
    #[serde(skip)]
    pub gustiness: f32,
    /// Where the gust pattern is, in local space (`wind::gust_pattern`).
    #[serde(skip)]
    pub gust_shift: glam::Vec2,
    /// How far the clouds and the mist have drifted, in seconds of the scene's wind.
    #[serde(skip)]
    pub aloft: glam::Vec2,
    /// The plants' flutter clock.
    #[serde(skip)]
    pub flutter: f32,
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
    /// How much bright colors keep their hue instead of going pale, from 0 to 1.
    pub vivid: f32,
    /// How crisp the edges and texels are. 0 leaves the image alone.
    pub sharpen: f32,
    /// Color added to the darkest parts of the image, so blacks lean to a hue.
    pub shadow_tint: Vec3,
    /// Shafts of light streaming from the sun (and, fainter, the moon) past what stands in front
    /// of it. 0 leaves them out.
    pub rays: f32,
    /// How much what is far away is laid on in brush strokes, from 0 to 1, between `paint_from`
    /// and `paint_to` meters from the camera.
    pub paint: f32,
    pub paint_from: f32,
    pub paint_to: f32,
}

/// A room of a house in the world: what is inside it is indoors, lit only by its own lights
/// and dimly by the sky. The renderer uses the nearest ones.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoomBox {
    /// The middle of its floor (local).
    pub center: Vec3,
    /// The house's +X in world x, z.
    pub axis: glam::Vec2,
    /// Half its size along the house's +X and +Z.
    pub half: glam::Vec2,
    /// The world y of the walls' top.
    pub walls: f32,
}

impl RoomBox {
    /// Whether `p` is inside it, under the walls' top.
    pub fn contains(&self, p: Vec3) -> bool {
        let d = p - self.center;
        let flat = glam::Vec2::new(d.x, d.z);
        d.y > -0.8
            && p.y < self.walls
            && flat.dot(self.axis).abs() < self.half.x + 0.12
            && flat.dot(glam::Vec2::new(-self.axis.y, self.axis.x)).abs() < self.half.y + 0.12
    }
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
            mist: Mist::default(),
            air: 0.0,
            air_height: 1500.0,
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
            halo: 0.0,
            clouds: Clouds::default(),
            sun_position: None,
            moon_position: None,
            light_position: None,
        }
    }
}

impl Default for Clouds {
    fn default() -> Self {
        Self {
            amount: 0.0,
            dark: Vec3::new(0.2, 0.22, 0.3),
            light: Vec3::new(0.7, 0.72, 0.78),
            nebula: Vec3::ZERO,
            scale: 1.0,
            speed: 1.0,
            puff: 0.0,
        }
    }
}

impl Default for SurfaceLook {
    fn default() -> Self {
        Self {
            contrast: 1.0,
            saturation: 1.0,
            mottle: 0.0,
            mottle_size: 8.0,
            mottle_dark: Vec3::splat(0.5),
            mottle_light: Vec3::splat(1.5),
            crisp: 0.0,
            stain_color: 1.0,
        }
    }
}

impl Default for Wind {
    fn default() -> Self {
        Self {
            direction: glam::Vec2::new(1.0, 0.3),
            strength: 1.0,
            scale: 1.0,
            speed: 1.6,
            wander: 50.0,
            gusty: 1.0,
            weather: 1.0,
            blowing: glam::Vec2::new(1.0, 0.3).normalize(),
            speed_ms: 2.5,
            gustiness: 0.3,
            gust_shift: glam::Vec2::ZERO,
            aloft: glam::Vec2::ZERO,
            flutter: 0.0,
        }
    }
}

impl Default for Shadows {
    fn default() -> Self {
        Self { enabled: true, distance: 120.0, strength: 0.85, cascades: 4 }
    }
}

impl Default for Post {
    fn default() -> Self {
        Self {
            exposure: 1.0,
            contrast: 1.05,
            saturation: 1.0,
            vignette: 0.3,
            grain: 0.03,
            bloom: 0.15,
            vivid: 0.0,
            sharpen: 0.0,
            shadow_tint: Vec3::ZERO,
            rays: 0.0,
            paint: 0.0,
            paint_from: 120.0,
            paint_to: 1500.0,
        }
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
