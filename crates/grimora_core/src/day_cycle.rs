//! The clock of the world: where the sun and moon are and what the sky, light
//! and fog look like at each hour. [`DayCycle::apply`] writes that into an
//! [`Environment`], leaving alone what the hours don't touch (shadows, most of
//! post-processing...).

use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::Rng;
use crate::environment::{Environment, Precipitation};

const HOURS: f32 = 24.0;
/// The light never comes from lower than this (sine of about 8 degrees): near
/// the horizon, shadows would stretch across the whole map and shimmer.
const MIN_LIGHT_ELEVATION: f32 = 0.14;
/// The sun lights the world from this elevation (sine) up, fading in over
/// `LIGHT_FADE`. The moon only comes on as the sun gets as far below the horizon,
/// so at the swap both are dark.
const LIGHT_HORIZON: f32 = 0.01;
const LIGHT_FADE: f32 = 0.1;
/// Seconds rain takes to wet the ground, and the ground to dry after it.
const GETS_WET: f32 = 20.0;
const DRIES: f32 = 90.0;
/// Seconds snow takes to settle, and to melt after it stops.
const SNOW_SETTLES: f32 = 45.0;
const SNOW_MELTS: f32 = 90.0;
/// Each span of this many seconds has at most one lightning strike.
const STRIKE_SPAN: f32 = 11.0;
/// Seconds from the flash to the thunder.
const THUNDER_DELAY: f32 = 1.4;

/// How a scene's day runs (`day_cycle:` in the scene file).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DayCycleDesc {
    /// The hour the scene starts at, from 0 to 24.
    pub time: f32,
    /// Real seconds a whole day takes. 0 stops the clock.
    pub day_length: f32,
    /// Where the sun rises, in degrees around the vertical: 0 is +X, 90 is -Z.
    pub sunrise: f32,
    /// How far from straight overhead the sun passes at noon, in degrees.
    pub tilt: f32,
    /// The same for the moon, if not `tilt`: a low moon and a high sun can share a sky.
    pub moon_tilt: Option<f32>,
    /// How thick the air of this place is, as a multiple of the fog of each hour. Below 1
    /// for wide views, as over the sea and the desert.
    pub haze: f32,
    /// What the day is like, when `keys` is empty.
    pub weather: Weather,
    /// What each hour looks like, interpolated between them. Empty uses
    /// [`DayKey::for_weather`].
    pub keys: Vec<DayKey>,
}

/// What the weather is. [`DayCycle::change_weather`] changes it during the game. Clear and
/// overcast differ only by day, and rain and snow also change the night.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Weather {
    /// Open sky and a golden sun: long warm light, cool shadows, far-off haze and
    /// scattered white clouds. For the sea, the desert and whatever should look vast.
    #[default]
    Clear,
    /// A low lilac ceiling with no hard shadows, under which colors glow.
    Overcast,
    /// The same hours dulled and darkened, with rain falling and the ground wet.
    Rain,
    /// A bruised blue sky, heavy with fog, with hard rain, strong wind and lightning.
    Storm,
    /// A pale cold day, with snow falling and settling on what faces up.
    Snow,
}

impl Weather {
    pub const ALL: [Self; 5] = [Self::Clear, Self::Overcast, Self::Rain, Self::Storm, Self::Snow];
}

impl Default for DayCycleDesc {
    fn default() -> Self {
        Self {
            time: 9.0,
            day_length: 600.0,
            sunrise: 20.0,
            tilt: 35.0,
            moon_tilt: None,
            haze: 1.0,
            weather: Weather::default(),
            keys: Vec::new(),
        }
    }
}

/// The look of one hour. Colors are sRGB in `0..=1`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DayKey {
    pub hour: f32,
    pub sun_color: Vec3,
    pub sun_intensity: f32,
    pub moon_color: Vec3,
    pub moon_intensity: f32,
    pub ambient_sky: Vec3,
    pub ambient_ground: Vec3,
    pub ambient_intensity: f32,
    pub fog_color: Vec3,
    pub fog_density: f32,
    pub zenith: Vec3,
    pub horizon: Vec3,
    pub ground: Vec3,
    pub exposure: f32,
    /// How strongly the sky glows around the sun or moon.
    pub glow: f32,
    /// How visible the stars are, from 0 to 1.
    pub stars: f32,
    /// How much of the sky is cloud, from 0 to 1.
    pub cloud_amount: f32,
    pub cloud_dark: Vec3,
    pub cloud_light: Vec3,
    pub nebula: Vec3,
    /// The shape of the clouds: 0 marbled wisps, 1 separate round heaps.
    pub cloud_puff: f32,
    /// How much of the scene's low mist there is, as a multiple.
    pub mist: f32,
    /// How much the stains on surfaces keep their color, from 0 (gray) to 1.
    pub stain_color: f32,
    /// How hard it rains, from 0 to 1.
    pub rain: f32,
    /// How hard it snows, from 0 to 1.
    pub snow: f32,
    /// The chance of a lightning strike in each span of [`STRIKE_SPAN`] seconds.
    pub lightning: f32,
    /// How hard the wind blows, as a multiple of the scene's.
    pub wind: f32,
}

impl Default for DayKey {
    fn default() -> Self {
        Self::base().swap_remove(3)
    }
}

impl DayKey {
    /// The hours of a day with this weather, sorted by hour.
    pub fn for_weather(weather: Weather) -> Vec<Self> {
        match weather {
            Weather::Clear => Self::clear(),
            Weather::Overcast => Self::overcast(),
            Weather::Rain => Self::rain(),
            Weather::Storm => Self::storm(),
            Weather::Snow => Self::snow(),
        }
    }

    /// A dream of a day: ultramarine nights under a pale moon that turns the land
    /// emerald, dawns and dusks the red of blood, and a bruised, stormy day between.
    fn base() -> Vec<Self> {
        let c = Vec3::new;
        let night = Self {
            hour: 0.0,
            sun_color: c(1.0, 0.3, 0.2),
            sun_intensity: 0.0,
            moon_color: c(0.6, 0.92, 0.82),
            moon_intensity: 1.5,
            ambient_sky: c(0.2, 0.42, 0.64),
            ambient_ground: c(0.07, 0.22, 0.25),
            ambient_intensity: 1.4,
            fog_color: c(0.09, 0.11, 0.38),
            fog_density: 0.014,
            zenith: c(0.02, 0.015, 0.14),
            horizon: c(0.1, 0.11, 0.48),
            ground: c(0.02, 0.03, 0.09),
            exposure: 1.35,
            glow: 0.1,
            stars: 1.0,
            cloud_amount: 0.56,
            cloud_dark: c(0.03, 0.03, 0.2),
            cloud_light: c(0.38, 0.4, 0.98),
            nebula: c(0.2, 0.03, 0.42),
            cloud_puff: 0.0,
            mist: 1.0,
            stain_color: 1.0,
            rain: 0.0,
            snow: 0.0,
            lightning: 0.0,
            wind: 1.0,
        };
        let dawn = Self {
            hour: 6.2,
            sun_color: c(1.0, 0.36, 0.26),
            sun_intensity: 1.7,
            moon_intensity: 0.0,
            ambient_sky: c(0.42, 0.22, 0.5),
            ambient_ground: c(0.2, 0.08, 0.14),
            ambient_intensity: 0.7,
            fog_color: c(0.42, 0.13, 0.26),
            fog_density: 0.018,
            zenith: c(0.1, 0.04, 0.3),
            horizon: c(0.88, 0.16, 0.2),
            ground: c(0.14, 0.05, 0.1),
            exposure: 1.25,
            stars: 0.15,
            cloud_amount: 0.6,
            cloud_dark: c(0.2, 0.02, 0.12),
            cloud_light: c(0.98, 0.28, 0.24),
            nebula: c(0.36, 0.02, 0.2),
            ..night.clone()
        };
        let morning = Self {
            hour: 9.0,
            sun_color: c(0.86, 0.96, 0.8),
            sun_intensity: 1.9,
            ambient_sky: c(0.36, 0.46, 0.64),
            ambient_ground: c(0.12, 0.16, 0.18),
            ambient_intensity: 0.65,
            fog_color: c(0.25, 0.33, 0.47),
            fog_density: 0.012,
            zenith: c(0.09, 0.13, 0.38),
            horizon: c(0.38, 0.45, 0.62),
            ground: c(0.12, 0.15, 0.2),
            exposure: 1.1,
            stars: 0.0,
            cloud_amount: 0.72,
            cloud_dark: c(0.09, 0.11, 0.28),
            cloud_light: c(0.46, 0.52, 0.76),
            nebula: c(0.04, 0.06, 0.2),
            ..dawn.clone()
        };
        let noon = Self {
            hour: 13.0,
            sun_color: c(0.92, 0.98, 0.86),
            sun_intensity: 2.2,
            ambient_sky: c(0.4, 0.5, 0.68),
            zenith: c(0.1, 0.15, 0.42),
            horizon: c(0.42, 0.5, 0.66),
            exposure: 1.05,
            ..morning.clone()
        };
        let afternoon = Self {
            hour: 16.5,
            sun_color: c(1.0, 0.6, 0.42),
            sun_intensity: 2.0,
            ambient_sky: c(0.38, 0.3, 0.6),
            ambient_ground: c(0.16, 0.1, 0.18),
            fog_color: c(0.3, 0.19, 0.42),
            fog_density: 0.013,
            zenith: c(0.09, 0.06, 0.32),
            horizon: c(0.68, 0.3, 0.46),
            cloud_dark: c(0.15, 0.06, 0.3),
            cloud_light: c(0.95, 0.5, 0.6),
            nebula: c(0.14, 0.03, 0.26),
            exposure: 1.2,
            ..noon.clone()
        };
        let dusk = Self {
            hour: 17.9,
            sun_color: c(1.0, 0.28, 0.2),
            sun_intensity: 1.6,
            ambient_sky: c(0.36, 0.18, 0.5),
            ambient_ground: c(0.18, 0.06, 0.14),
            ambient_intensity: 1.1,
            fog_color: c(0.36, 0.06, 0.2),
            fog_density: 0.018,
            zenith: c(0.15, 0.02, 0.24),
            horizon: c(0.96, 0.12, 0.16),
            ground: c(0.12, 0.03, 0.08),
            exposure: 1.25,
            stars: 0.2,
            cloud_amount: 0.62,
            cloud_dark: c(0.24, 0.01, 0.08),
            cloud_light: c(1.0, 0.2, 0.15),
            nebula: c(0.4, 0.02, 0.2),
            ..afternoon.clone()
        };
        let evening = Self { hour: 19.8, ..night.clone() };
        let before_dawn = Self {
            hour: 5.1,
            fog_color: c(0.14, 0.09, 0.32),
            horizon: c(0.24, 0.1, 0.42),
            zenith: c(0.03, 0.02, 0.16),
            stars: 0.7,
            moon_intensity: 0.9,
            cloud_light: c(0.55, 0.3, 0.85),
            ..night.clone()
        };
        vec![night, before_dawn, dawn, morning, noon, afternoon, dusk, evening]
    }

    /// The bruised day of [`Self::base`] with what makes it a storm, and no moon or stars.
    fn storm() -> Vec<Self> {
        Self::base()
            .into_iter()
            .map(|key| Self {
                moon_intensity: key.moon_intensity * 0.5,
                stars: 0.0,
                cloud_amount: key.cloud_amount.max(0.8),
                rain: 1.0,
                lightning: 0.6,
                wind: 2.2,
                ..key
            })
            .collect()
    }

    /// An overcast day dulled towards a cold gray of the same brightness at each hour,
    /// so a rainy dusk is still red and a rainy night still blue.
    fn rain() -> Vec<Self> {
        let dull = |color: Vec3, amount: f32, dark: f32| {
            color.lerp(Vec3::new(0.82, 0.92, 1.16) * color.dot(Vec3::new(0.3, 0.5, 0.2)), amount) * dark
        };
        Self::overcast()
            .into_iter()
            .map(|key| Self {
                sun_intensity: key.sun_intensity * 0.55,
                moon_intensity: key.moon_intensity * 0.45,
                ambient_sky: dull(key.ambient_sky, 0.4, 0.92),
                fog_color: dull(key.fog_color, 0.6, 0.62),
                fog_density: key.fog_density * 1.3,
                zenith: dull(key.zenith, 0.55, 0.62),
                horizon: dull(key.horizon, 0.6, 0.6),
                ground: dull(key.ground, 0.4, 0.7),
                glow: key.glow * 0.4,
                stars: 0.0,
                cloud_amount: key.cloud_amount.max(0.88),
                cloud_dark: dull(key.cloud_dark, 0.4, 0.45),
                cloud_light: dull(key.cloud_light, 0.55, 0.5),
                nebula: key.nebula * 0.3,
                cloud_puff: 0.2,
                mist: key.mist.max(0.7),
                rain: 1.0,
                wind: 1.5,
                ..key
            })
            .collect()
    }

    /// A pale cold day under a white and lilac sky, with the blood-red dusk still on the
    /// snow, and the moon turning it all the color of the night.
    fn snow() -> Vec<Self> {
        let c = Vec3::new;
        let mut keys = Self::base();
        let morning = Self {
            hour: 8.5,
            sun_color: c(1.0, 0.92, 0.84),
            sun_intensity: 1.7,
            moon_intensity: 0.0,
            ambient_sky: c(0.56, 0.66, 0.94),
            ambient_ground: c(0.3, 0.34, 0.52),
            ambient_intensity: 0.85,
            fog_color: c(0.56, 0.64, 0.84),
            fog_density: 0.0065,
            zenith: c(0.08, 0.18, 0.54),
            horizon: c(0.66, 0.74, 0.9),
            ground: c(0.5, 0.54, 0.68),
            exposure: 0.95,
            glow: 0.4,
            stars: 0.0,
            cloud_amount: 0.7,
            cloud_dark: c(0.36, 0.42, 0.66),
            cloud_light: c(0.92, 0.94, 1.0),
            nebula: Vec3::ZERO,
            cloud_puff: 0.7,
            mist: 0.35,
            stain_color: 0.35,
            ..keys[0].clone()
        };
        let dawn = Self {
            hour: 6.2,
            sun_color: c(1.0, 0.62, 0.6),
            sun_intensity: 1.2,
            horizon: c(0.96, 0.62, 0.7),
            zenith: c(0.16, 0.2, 0.5),
            fog_color: c(0.62, 0.52, 0.66),
            cloud_dark: c(0.3, 0.26, 0.5),
            cloud_light: c(1.0, 0.74, 0.78),
            ..morning.clone()
        };
        let noon = Self { hour: 13.0, sun_intensity: 1.9, ..morning.clone() };
        let afternoon = Self {
            hour: 16.3,
            sun_color: c(1.0, 0.74, 0.62),
            horizon: c(0.96, 0.76, 0.72),
            fog_color: c(0.74, 0.68, 0.8),
            cloud_dark: c(0.44, 0.4, 0.66),
            cloud_light: c(1.0, 0.86, 0.82),
            ..noon.clone()
        };
        let dusk = Self { cloud_puff: 0.4, mist: 0.7, ..keys[6].clone() };
        keys.splice(2..7, [dawn, morning, noon, afternoon, dusk]);
        keys.into_iter()
            .map(|key| Self {
                // By night the snow gives back so much light that the moon must give less.
                moon_intensity: key.moon_intensity * 0.32,
                ambient_intensity: key.ambient_intensity * if key.moon_intensity > 0.0 { 0.7 } else { 1.0 },
                stars: key.stars * 0.6,
                cloud_amount: key.cloud_amount.max(0.6),
                snow: 1.0,
                wind: 1.1,
                ..key
            })
            .collect()
    }

    /// The day of a vast and ancient land: everything gold in a long low light, deep
    /// blue overhead and pale haze far away. The night stays the dream it is.
    fn clear() -> Vec<Self> {
        let c = Vec3::new;
        let mut keys = Self::base();
        let storm_dusk = keys[6].clone();
        let dawn = Self {
            hour: 6.2,
            sun_color: c(1.0, 0.52, 0.3),
            sun_intensity: 2.4,
            moon_intensity: 0.0,
            ambient_sky: c(0.5, 0.38, 0.58),
            ambient_ground: c(0.22, 0.13, 0.14),
            ambient_intensity: 0.7,
            fog_color: c(0.7, 0.46, 0.5),
            fog_density: 0.008,
            zenith: c(0.07, 0.14, 0.46),
            horizon: c(1.0, 0.58, 0.4),
            ground: c(0.24, 0.14, 0.14),
            exposure: 1.15,
            glow: 0.5,
            stars: 0.1,
            cloud_amount: 0.5,
            cloud_dark: c(0.36, 0.14, 0.28),
            cloud_light: c(1.0, 0.68, 0.46),
            nebula: Vec3::ZERO,
            cloud_puff: 0.8,
            mist: 0.7,
            stain_color: 0.35,
            ..keys[0].clone()
        };
        let morning = Self {
            hour: 8.5,
            sun_color: c(1.0, 0.84, 0.58),
            sun_intensity: 3.1,
            ambient_sky: c(0.42, 0.56, 0.82),
            ambient_ground: c(0.27, 0.22, 0.15),
            ambient_intensity: 0.72,
            fog_color: c(0.6, 0.72, 0.88),
            fog_density: 0.0042,
            zenith: c(0.01, 0.12, 0.5),
            horizon: c(0.74, 0.84, 0.86),
            ground: c(0.3, 0.27, 0.22),
            exposure: 1.0,
            glow: 0.35,
            stars: 0.0,
            cloud_amount: 0.55,
            cloud_dark: c(0.46, 0.54, 0.76),
            cloud_light: c(1.0, 0.98, 0.9),
            mist: 0.12,
            stain_color: 0.3,
            ..dawn.clone()
        };
        let noon = Self {
            hour: 13.0,
            sun_color: c(1.0, 0.9, 0.7),
            sun_intensity: 3.4,
            zenith: c(0.0, 0.1, 0.48),
            horizon: c(0.66, 0.82, 0.9),
            cloud_amount: 0.5,
            mist: 0.04,
            exposure: 0.95,
            ..morning.clone()
        };
        let afternoon = Self {
            hour: 16.3,
            sun_color: c(1.0, 0.77, 0.48),
            sun_intensity: 3.3,
            ambient_sky: c(0.58, 0.6, 0.84),
            ambient_ground: c(0.38, 0.27, 0.16),
            fog_color: c(0.84, 0.74, 0.6),
            fog_density: 0.0042,
            zenith: c(0.03, 0.12, 0.46),
            horizon: c(1.0, 0.76, 0.48),
            ground: c(0.3, 0.2, 0.14),
            cloud_dark: c(0.5, 0.38, 0.5),
            cloud_light: c(1.0, 0.86, 0.6),
            cloud_amount: 0.55,
            mist: 0.1,
            exposure: 1.18,
            ..noon.clone()
        };
        // Only at the very end does it turn to blood.
        let dusk = Self { cloud_puff: 0.6, mist: 0.6, stain_color: 0.7, ..storm_dusk };
        keys.splice(2..7, [dawn, morning, noon, afternoon, dusk]);
        keys
    }

    /// A low lilac ceiling, white where the sun is behind it. There are no hard
    /// shadows, and the dull light makes every color in the land look lit from within.
    fn overcast() -> Vec<Self> {
        let c = Vec3::new;
        let mut keys = Self::base();
        let storm_dusk = keys[6].clone();
        let morning = Self {
            hour: 8.5,
            sun_color: c(0.95, 0.9, 0.95),
            sun_intensity: 1.1,
            moon_intensity: 0.0,
            ambient_sky: c(0.62, 0.62, 0.86),
            ambient_ground: c(0.26, 0.22, 0.24),
            ambient_intensity: 1.15,
            fog_color: c(0.52, 0.5, 0.68),
            fog_density: 0.0095,
            zenith: c(0.3, 0.3, 0.6),
            horizon: c(0.72, 0.68, 0.84),
            ground: c(0.26, 0.24, 0.32),
            exposure: 1.05,
            glow: 0.5,
            stars: 0.0,
            cloud_amount: 0.72,
            cloud_dark: c(0.3, 0.3, 0.56),
            cloud_light: c(0.96, 0.94, 1.0),
            nebula: Vec3::ZERO,
            cloud_puff: 0.85,
            mist: 0.3,
            stain_color: 0.5,
            ..keys[0].clone()
        };
        let dawn = Self {
            hour: 6.2,
            sun_color: c(1.0, 0.56, 0.5),
            sun_intensity: 0.9,
            horizon: c(0.86, 0.46, 0.56),
            zenith: c(0.16, 0.14, 0.42),
            fog_color: c(0.5, 0.32, 0.46),
            cloud_dark: c(0.2, 0.14, 0.38),
            cloud_light: c(0.96, 0.6, 0.66),
            stars: 0.1,
            exposure: 1.15,
            ..morning.clone()
        };
        let noon = Self { hour: 13.0, sun_intensity: 1.25, ..morning.clone() };
        let afternoon = Self {
            hour: 16.3,
            sun_color: c(1.0, 0.78, 0.72),
            horizon: c(0.8, 0.62, 0.72),
            cloud_light: c(1.0, 0.86, 0.84),
            cloud_dark: c(0.3, 0.24, 0.5),
            fog_color: c(0.54, 0.42, 0.6),
            ..noon.clone()
        };
        let dusk = Self { cloud_puff: 0.3, mist: 0.9, stain_color: 0.8, ..storm_dusk };
        keys.splice(2..7, [dawn, morning, noon, afternoon, dusk]);
        keys
    }

    fn lerp(&self, other: &Self, t: f32) -> Self {
        let num = |a: f32, b: f32| a + (b - a) * t;
        Self {
            hour: self.hour,
            sun_color: self.sun_color.lerp(other.sun_color, t),
            sun_intensity: num(self.sun_intensity, other.sun_intensity),
            moon_color: self.moon_color.lerp(other.moon_color, t),
            moon_intensity: num(self.moon_intensity, other.moon_intensity),
            ambient_sky: self.ambient_sky.lerp(other.ambient_sky, t),
            ambient_ground: self.ambient_ground.lerp(other.ambient_ground, t),
            ambient_intensity: num(self.ambient_intensity, other.ambient_intensity),
            fog_color: self.fog_color.lerp(other.fog_color, t),
            fog_density: num(self.fog_density, other.fog_density),
            zenith: self.zenith.lerp(other.zenith, t),
            horizon: self.horizon.lerp(other.horizon, t),
            ground: self.ground.lerp(other.ground, t),
            exposure: num(self.exposure, other.exposure),
            glow: num(self.glow, other.glow),
            stars: num(self.stars, other.stars),
            cloud_amount: num(self.cloud_amount, other.cloud_amount),
            cloud_dark: self.cloud_dark.lerp(other.cloud_dark, t),
            cloud_light: self.cloud_light.lerp(other.cloud_light, t),
            nebula: self.nebula.lerp(other.nebula, t),
            cloud_puff: num(self.cloud_puff, other.cloud_puff),
            mist: num(self.mist, other.mist),
            stain_color: num(self.stain_color, other.stain_color),
            rain: num(self.rain, other.rain),
            snow: num(self.snow, other.snow),
            lightning: num(self.lightning, other.lightning),
            wind: num(self.wind, other.wind),
        }
    }
}

/// A running day. The engine advances it with the world clock, so pausing stops it.
#[derive(Debug, Clone, PartialEq)]
pub struct DayCycle {
    /// Hours since midnight, from 0 to 24.
    hour: f32,
    /// Real seconds a whole day takes. 0 stops the clock.
    pub day_length: f32,
    east: Vec3,
    noon: Vec3,
    /// Where the moon is at midnight.
    midnight: Vec3,
    haze: f32,
    /// Sorted by hour.
    keys: Vec<DayKey>,
    weather: Weather,
    change: Option<WeatherChange>,
    /// Seconds this day has run for, which lightning goes by.
    clock: f32,
    /// How wet the ground is, from 0 to 1. It lags behind the rain.
    wet: f32,
    /// How much snow has settled, from 0 to 1. It lags behind the snowfall.
    snow_cover: f32,
    /// The span whose strike has already thundered.
    thundered: Option<u64>,
}

/// A change of weather under way, from how things looked when it started.
#[derive(Debug, Clone, PartialEq)]
struct WeatherChange {
    from: DayKey,
    seconds: f32,
    passed: f32,
}

impl DayCycle {
    pub fn new(desc: &DayCycleDesc) -> Self {
        let mut keys = if desc.keys.is_empty() { DayKey::for_weather(desc.weather) } else { desc.keys.clone() };
        keys.sort_by(|a, b| a.hour.total_cmp(&b.hour));
        let (sin, cos) = desc.sunrise.to_radians().sin_cos();
        let east = Vec3::new(cos, 0.0, -sin);
        let south = Vec3::new(sin, 0.0, cos);
        let (sin, cos) = desc.tilt.clamp(0.0, 80.0).to_radians().sin_cos();
        let noon = Vec3::Y * cos + south * sin;
        let (sin, cos) = desc.moon_tilt.unwrap_or(desc.tilt).clamp(0.0, 80.0).to_radians().sin_cos();
        let midnight = Vec3::Y * cos + south * sin;
        let mut day = Self {
            hour: 0.0,
            day_length: desc.day_length.max(0.0),
            east,
            noon,
            midnight,
            haze: desc.haze.max(0.0),
            keys,
            weather: desc.weather,
            change: None,
            clock: 0.0,
            wet: 0.0,
            snow_cover: 0.0,
            thundered: None,
        };
        day.set_hour(desc.time);
        day.settle();
        day
    }

    pub fn hour(&self) -> f32 {
        self.hour
    }

    pub fn weather(&self) -> Weather {
        self.weather
    }

    /// Changes what the day is like, at once, with the ground already as it would
    /// end up. This replaces any keys of its own.
    pub fn set_weather(&mut self, weather: Weather) {
        self.keys = DayKey::for_weather(weather);
        self.weather = weather;
        self.change = None;
        self.settle();
    }

    /// Turns what the day is like into `weather` over `seconds`, starting from how it
    /// looks now, even in the middle of another change. The ground follows at its own
    /// pace. This replaces any keys of its own.
    pub fn change_weather(&mut self, weather: Weather, seconds: f32) {
        if seconds <= 0.0 {
            self.set_weather(weather);
            return;
        }
        let from = self.key();
        self.keys = DayKey::for_weather(weather);
        self.weather = weather;
        self.change = Some(WeatherChange { from, seconds, passed: 0.0 });
    }

    /// Puts the ground as this weather would leave it.
    fn settle(&mut self) {
        let key = self.key();
        self.wet = if key.rain > 0.0 { 1.0 } else { 0.0 };
        self.snow_cover = if key.snow > 0.0 { 1.0 } else { 0.0 };
    }

    /// Moves the clock to `hour`; values past 24 wrap around.
    pub fn set_hour(&mut self, hour: f32) {
        self.hour = hour.rem_euclid(HOURS);
    }

    /// Runs the clock for `dt` real seconds. A stopped clock still runs the weather.
    pub fn advance(&mut self, dt: f32) {
        if self.day_length > 0.0 {
            self.set_hour(self.hour + dt / self.day_length * HOURS);
        }
        self.clock += dt;
        if let Some(change) = &mut self.change {
            change.passed += dt;
            if change.passed >= change.seconds {
                self.change = None;
            }
        }
        let key = self.key();
        self.wet = lag(self.wet, key.rain > 0.05, dt, GETS_WET, DRIES);
        self.snow_cover = lag(self.snow_cover, key.snow > 0.05, dt, SNOW_SETTLES, SNOW_MELTS);
    }

    /// The latest strike of this span, if any: its span and the seconds since it.
    fn last_strike(&self) -> Option<(u64, f32)> {
        let span = (self.clock / STRIKE_SPAN) as u64;
        let mut rng = Rng::new(span.wrapping_mul(31).wrapping_add(7));
        if rng.f32() >= self.key().lightning {
            return None;
        }
        let at = (span as f32 + rng.range(0.0, 0.8)) * STRIKE_SPAN;
        (self.clock >= at).then_some((span, self.clock - at))
    }

    /// How bright the lightning is now: a flash, and a second one right after it.
    fn flash(&self) -> f32 {
        self.last_strike().map_or(0.0, |(_, since)| (-since * 8.0).exp() + 0.7 * (-(since - 0.16).abs() * 14.0).exp())
    }

    /// Whether a strike's thunder has just arrived. It only says so once.
    pub fn take_thunder(&mut self) -> bool {
        match self.last_strike() {
            Some((span, since)) if since > THUNDER_DELAY && self.thundered != Some(span) => {
                self.thundered = Some(span);
                true
            }
            _ => false,
        }
    }

    /// From the ground towards the sun. It rises at 6, peaks at 12 and sets at 18.
    pub fn sun_position(&self) -> Vec3 {
        let (sin, cos) = ((self.hour - 6.0) / HOURS * std::f32::consts::TAU).sin_cos();
        (self.east * cos + self.noon * sin).normalize_or(Vec3::Y)
    }

    /// From the ground towards the moon: across the sky from the sun, a little off.
    pub fn moon_position(&self) -> Vec3 {
        let (sin, cos) = ((self.hour - 6.0) / HOURS * std::f32::consts::TAU).sin_cos();
        (-self.east * cos - self.midnight * sin + self.midnight.cross(self.east) * 0.18).normalize_or(Vec3::Y)
    }

    /// How much the sun is lighting the world, from 0 at night to 1 once it is up.
    pub fn daylight(&self) -> f32 {
        light_fade(self.sun_position().y)
    }

    /// Whether the sun has set, for whatever comes out at night.
    pub fn is_night(&self) -> bool {
        self.daylight() <= 0.0
    }

    /// How things look now: the hour, on its way from the last weather if it is changing.
    pub fn key(&self) -> DayKey {
        let key = self.hour_key();
        let Some(change) = &self.change else { return key };
        let t = (change.passed / change.seconds).clamp(0.0, 1.0);
        change.from.lerp(&key, t * t * (3.0 - 2.0 * t))
    }

    /// The hour's look, between the two keys around it.
    fn hour_key(&self) -> DayKey {
        let keys = &self.keys;
        let Some(first) = keys.first() else { return DayKey::default() };
        let after = keys.iter().position(|key| key.hour > self.hour);
        let (a, b, span, into) = match after {
            Some(0) | None => {
                // Around midnight: from the last key of the day to the first of the next.
                let last = &keys[keys.len() - 1];
                let span = first.hour + HOURS - last.hour;
                let into = if self.hour >= last.hour { self.hour - last.hour } else { self.hour + HOURS - last.hour };
                (last, first, span, into)
            }
            Some(i) => (&keys[i - 1], &keys[i], keys[i].hour - keys[i - 1].hour, self.hour - keys[i - 1].hour),
        };
        let t = if span > 0.0 { (into / span).clamp(0.0, 1.0) } else { 0.0 };
        a.lerp(b, t * t * (3.0 - 2.0 * t))
    }

    /// Writes this hour into `env`. The directional light is the sun by day and
    /// the moon by night, both fading out near the horizon, where they swap.
    pub fn apply(&self, env: &mut Environment) {
        let key = self.key();
        let (sun, moon) = (self.sun_position(), self.moon_position());
        let sun_light = light_fade(sun.y);
        let (light, color, intensity) = if sun_light > 0.0 {
            (sun, key.sun_color, key.sun_intensity * sun_light)
        } else {
            (moon, key.moon_color, key.moon_intensity * light_fade(-sun.y) * light_fade(moon.y))
        };
        env.sun.direction = Vec3::new(light.x, light.y.max(MIN_LIGHT_ELEVATION), light.z).normalize_or(Vec3::Y);
        env.sun.color = color;
        env.sun.intensity = intensity;
        env.ambient.sky = key.ambient_sky;
        env.ambient.ground = key.ambient_ground;
        env.ambient.intensity = key.ambient_intensity;
        env.fog.color = key.fog_color;
        env.fog.density = key.fog_density * self.haze;
        env.sky.zenith = key.zenith;
        env.sky.horizon = key.horizon;
        env.sky.ground = key.ground;
        env.sky.sun_glow = key.glow;
        env.sky.stars = key.stars;
        env.sky.clouds.amount = key.cloud_amount;
        env.sky.clouds.dark = key.cloud_dark;
        env.sky.clouds.light = key.cloud_light;
        env.sky.clouds.nebula = key.nebula;
        env.sky.clouds.puff = key.cloud_puff;
        env.fog.mist.strength = key.mist;
        env.surface.stain_color = key.stain_color;
        env.sky.sun_position = Some(sun);
        env.sky.moon_position = Some(moon);
        env.sky.light_position = Some(light);
        env.post.exposure = key.exposure;
        env.wind.scale = key.wind;
        env.precipitation = Precipitation {
            rain: key.rain,
            snow: key.snow,
            wet: self.wet,
            snow_cover: self.snow_cover,
            flash: self.flash(),
        };
    }
}

/// Moves `value` towards 1 if `rising` and towards 0 if not, over those many seconds.
fn lag(value: f32, rising: bool, dt: f32, rise: f32, fall: f32) -> f32 {
    if rising { (value + dt / rise).min(1.0) } else { (value - dt / fall).max(0.0) }
}

fn light_fade(elevation: f32) -> f32 {
    let t = ((elevation - LIGHT_HORIZON) / LIGHT_FADE).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day_at(hour: f32) -> DayCycle {
        DayCycle::new(&DayCycleDesc { time: hour, ..Default::default() })
    }

    #[test]
    fn the_sun_rises_peaks_and_sets_on_time() {
        assert!(day_at(6.0).sun_position().y.abs() < 1e-5);
        assert!(day_at(18.0).sun_position().y.abs() < 1e-5);
        assert!(day_at(12.0).sun_position().y > 0.7);
        assert!(day_at(0.0).sun_position().y < -0.7);
        assert!(day_at(9.0).sun_position().y > 0.0 && day_at(21.0).sun_position().y < 0.0);
        assert!(day_at(0.0).moon_position().y > 0.6);
        assert!(day_at(12.0).moon_position().y < -0.6);
        // Rises at the east of the description, sets at the opposite side.
        let east = day_at(6.0).sun_position();
        assert!(east.dot(day_at(18.0).sun_position()) < -0.99);
    }

    #[test]
    fn the_clock_runs_and_wraps() {
        let mut day = DayCycle::new(&DayCycleDesc { time: 23.0, day_length: 240.0, ..Default::default() });
        day.advance(10.0);
        assert!(day.hour().abs() < 1e-3 || (day.hour() - 24.0).abs() < 1e-3);
        day.advance(30.0);
        assert!((day.hour() - 3.0).abs() < 1e-3);
        day.day_length = 0.0;
        day.advance(1000.0);
        assert!((day.hour() - 3.0).abs() < 1e-3);
        day.set_hour(-1.0);
        assert!((day.hour() - 23.0).abs() < 1e-3);
    }

    #[test]
    fn keys_interpolate_and_wrap_around_midnight() {
        let at = |hour, stars| DayKey { hour, stars, ..Default::default() };
        let desc = DayCycleDesc { keys: vec![at(18.0, 1.0), at(6.0, 0.0)], time: 6.0, ..Default::default() };
        let mut day = DayCycle::new(&desc);
        assert_eq!(day.key().stars, 0.0);
        day.set_hour(12.0);
        assert!((day.key().stars - 0.5).abs() < 1e-5);
        day.set_hour(18.0);
        assert_eq!(day.key().stars, 1.0);
        day.set_hour(0.0);
        assert!((day.key().stars - 0.5).abs() < 1e-5, "{}", day.key().stars);
        day.set_hour(3.0);
        assert!(day.key().stars < 0.5 && day.key().stars > 0.0);
    }

    #[test]
    fn night_and_day_differ_and_light_does_not_jump() {
        let mut env = Environment::default();
        day_at(12.0).apply(&mut env);
        let noon = env.clone();
        day_at(0.0).apply(&mut env);
        // The moon is strong on purpose, since the night is when the land is at its most vivid.
        assert!(noon.sun.intensity > env.sun.intensity);
        assert!(noon.ambient.sky.y > env.ambient.sky.y);
        assert!(env.sky.stars > 0.9 && noon.sky.stars < 0.01);
        assert!(day_at(0.0).is_night() && !day_at(12.0).is_night());
        assert!(env.sun.direction.y > 0.1, "the moon lights from above");

        // Sweeping the whole day, the light never jumps: when the source swaps
        // between sun and moon, it is dark.
        let mut before: Option<Vec3> = None;
        for step in 0..=2400 {
            day_at(step as f32 / 100.0).apply(&mut env);
            assert!(env.sun.direction.y >= 0.13);
            let radiance = env.sun.color * env.sun.intensity;
            if let Some(previous) = before {
                let change = (radiance - previous).length();
                assert!(change < 0.15, "jump of {change} at {}", step as f32 / 100.0);
            }
            before = Some(radiance);
        }
    }

    #[test]
    fn every_weather_has_its_own_day_and_the_same_night() {
        let at = |weather, hour| {
            let mut day = DayCycle::new(&DayCycleDesc { weather, time: hour, ..Default::default() });
            day.set_hour(hour);
            let mut env = Environment::default();
            day.apply(&mut env);
            env
        };
        let (clear, overcast, storm) =
            (at(Weather::Clear, 12.0), at(Weather::Overcast, 12.0), at(Weather::Storm, 12.0));
        assert!(clear.sun.intensity > storm.sun.intensity && storm.sun.intensity > overcast.sun.intensity);
        assert!(clear.sky.clouds.amount < storm.sky.clouds.amount);
        assert!(overcast.sky.clouds.amount > clear.sky.clouds.amount);
        assert!(clear.fog.density < overcast.fog.density && overcast.fog.density < storm.fog.density);
        assert_eq!(at(Weather::Clear, 0.0), at(Weather::Overcast, 0.0));
        assert_eq!(at(Weather::Clear, 23.0), at(Weather::Overcast, 23.0));

        for weather in Weather::ALL {
            let mut before: Option<Vec3> = None;
            let mut env = Environment::default();
            for step in 0..=2400 {
                let mut day = DayCycle::new(&DayCycleDesc { weather, ..Default::default() });
                day.set_hour(step as f32 / 100.0);
                day.apply(&mut env);
                let radiance = env.sun.color * env.sun.intensity;
                if let Some(previous) = before {
                    assert!((radiance - previous).length() < 0.15, "{weather:?} jumps at {}", step as f32 / 100.0);
                }
                before = Some(radiance);
            }
        }

        let mut day = DayCycle::new(&DayCycleDesc { weather: Weather::Storm, time: 12.0, ..Default::default() });
        day.set_weather(Weather::Clear);
        let mut env = Environment::default();
        day.apply(&mut env);
        assert_eq!(env, clear);
    }

    #[test]
    fn what_falls_comes_with_the_weather() {
        let falling = |weather| {
            let mut env = Environment::default();
            DayCycle::new(&DayCycleDesc { weather, time: 12.0, ..Default::default() }).apply(&mut env);
            env.precipitation
        };
        assert_eq!(falling(Weather::Clear), Precipitation::default());
        assert_eq!(falling(Weather::Overcast), Precipitation::default());
        assert!(falling(Weather::Rain).rain > 0.0 && falling(Weather::Rain).wet == 1.0);
        assert!(falling(Weather::Storm).rain > 0.0 && falling(Weather::Storm).snow == 0.0);
        assert!(falling(Weather::Snow).snow > 0.0 && falling(Weather::Snow).snow_cover == 1.0);
    }

    #[test]
    fn a_change_of_weather_never_jumps() {
        let fog = |day: &DayCycle| {
            let mut env = Environment::default();
            day.apply(&mut env);
            env.fog.density
        };
        let desc = DayCycleDesc { time: 12.0, day_length: 0.0, ..Default::default() };
        let mut day = DayCycle::new(&desc);
        let clear = fog(&day);
        day.change_weather(Weather::Rain, 10.0);
        assert_eq!(day.weather(), Weather::Rain);
        assert_eq!(fog(&day), clear, "nothing has changed yet");
        let mut before = clear;
        for _ in 0..50 {
            day.advance(0.1);
            assert!((fog(&day) - before).abs() < 0.001);
            before = fog(&day);
        }

        // Halfway, it turns to something else, from where it had got to.
        let halfway = fog(&day);
        assert!(halfway > clear);
        day.change_weather(Weather::Clear, 10.0);
        assert_eq!(fog(&day), halfway);
        for _ in 0..101 {
            day.advance(0.1);
        }
        assert_eq!(fog(&day), clear);
    }

    #[test]
    fn the_ground_follows_the_weather_at_its_own_pace() {
        let mut day = DayCycle::new(&DayCycleDesc { time: 12.0, day_length: 0.0, ..Default::default() });
        let ground = |day: &DayCycle| {
            let mut env = Environment::default();
            day.apply(&mut env);
            (env.precipitation.wet, env.precipitation.snow_cover)
        };
        day.change_weather(Weather::Rain, 2.0);
        day.advance(1.0);
        day.advance(1.0);
        let (wet, _) = ground(&day);
        assert!(wet > 0.0 && wet < 0.5, "wetting: {wet}");
        for _ in 0..30 {
            day.advance(1.0);
        }
        assert_eq!(ground(&day).0, 1.0);

        day.change_weather(Weather::Snow, 2.0);
        for _ in 0..10 {
            day.advance(1.0);
        }
        let (wet, snow) = ground(&day);
        assert!(wet > 0.8 && snow > 0.0 && snow < 0.5, "still drying and only starting to settle: {wet} {snow}");

        day.set_weather(Weather::Clear);
        assert_eq!(ground(&day), (0.0, 0.0), "at once, it leaves the ground as it would end up");
    }

    #[test]
    fn lightning_flashes_and_then_thunders_once_only_in_a_storm() {
        let desc = DayCycleDesc { time: 12.0, day_length: 0.0, weather: Weather::Storm, ..Default::default() };
        let mut day = DayCycle::new(&desc);
        let (mut flashes, mut thunders, mut flash_first) = (0, 0, true);
        let mut brightest: f32 = 0.0;
        let mut was_bright = false;
        for _ in 0..(120 * 60) {
            day.advance(1.0 / 60.0);
            let flash = day.flash();
            brightest = brightest.max(flash);
            let bright = flash > 0.5;
            if bright && !was_bright {
                flashes += 1;
            }
            was_bright = bright;
            if day.take_thunder() {
                thunders += 1;
                flash_first &= thunders <= flashes;
            }
        }
        assert!(flashes > 2 && flashes <= 12, "{flashes} strikes in two minutes");
        assert!(thunders >= flashes - 1 && thunders <= flashes, "{thunders} thunders for {flashes} flashes");
        assert!(flash_first && brightest > 1.0);

        day.set_weather(Weather::Clear);
        for _ in 0..(120 * 60) {
            day.advance(1.0 / 60.0);
            assert_eq!(day.flash(), 0.0);
            assert!(!day.take_thunder());
        }
    }

    #[test]
    fn the_moon_can_keep_a_path_of_its_own() {
        let desc = DayCycleDesc { tilt: 40.0, moon_tilt: Some(76.0), time: 0.0, ..Default::default() };
        let day = DayCycle::new(&desc);
        let same = DayCycle::new(&DayCycleDesc { moon_tilt: None, ..desc.clone() });
        assert!(day.moon_position().y < 0.5 && same.moon_position().y > 0.7);
        assert_eq!(day.sun_position(), same.sun_position());

        // Without one, it stays across the sky from the sun, a little off.
        let mut day = same;
        for hour in [0.0, 5.0, 11.0, 17.0, 22.0] {
            day.set_hour(hour);
            assert!(day.moon_position().dot(day.sun_position()) < -0.95);
        }
    }

    #[test]
    fn apply_leaves_the_rest_of_the_environment_alone() {
        let mut env = Environment::default();
        env.shadows.distance = 77.0;
        env.post.vignette = 0.9;
        day_at(15.0).apply(&mut env);
        assert_eq!((env.shadows.distance, env.post.vignette), (77.0, 0.9));
    }
}
