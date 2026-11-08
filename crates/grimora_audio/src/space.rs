//! What the space does to a sound on its way to the ears: how loud, how dull, how
//! late and how far to one side. Only numbers, so all of it can be tested without mixing.

use grimora_core::{Quat, Vec3};

/// In meters per second, for how late a far sound arrives.
pub const SPEED_OF_SOUND: f32 = 343.0;

/// Where a filter lets everything through, in hertz.
pub(crate) const OPEN_CUTOFF: f32 = 20000.0;
/// Air takes nothing off up to this distance, in meters...
const AIR_CLEAR: f32 = 5.0;
/// ...and then the highs go at this power of the distance: 2 kHz at 40 m, 900 Hz at 80 m.
const AIR_ROLLOFF: f32 = 1.1;
/// However far or walled in, a sound keeps this much, or it stops being a sound.
const LOWEST_CUTOFF: f32 = 350.0;
/// What the back of the head leaves of the highs, for a sound right behind.
const BEHIND_CUTOFF: f32 = 0.3;

/// Volume and highs left behind a full wall. Sound still gets around it, just less.
const OCCLUDED_GAIN: f32 = 0.3;
const OCCLUDED_CUTOFF: f32 = 0.04;
/// Closer than this, nothing is taken as standing in between.
const ALWAYS_CLEAR: f32 = 1.5;
/// A ray stops this short of the emitter, because a blow on a wall comes from that wall.
const RAY_CLEARANCE: f32 = 0.6;
/// The rays go to these places around the emitter, so a post takes only part of the
/// sound and a low wall lets it through over the top. Up from the point, then to its sides.
const RAY_LIFT: f32 = 0.4;
const RAY_OVER: f32 = 1.8;
const RAY_SIDE: f32 = 0.9;

/// How hard kira pans: a sound on one side comes about 15 dB lower in the other ear.
const PAN_STRENGTH: f32 = 0.85;
/// Right on top of the listener a direction means nothing and swings about with the camera,
/// so the pan opens between these distances.
const PAN_NEAR: f32 = 0.4;
const PAN_FULL: f32 = 2.5;
/// What kira's ears, turned a little to the front, hear of a sound straight ahead at full pan.
const FRONT_LEVEL: f32 = 0.6913;

/// Where a positional sound plays and how it fades with distance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Emitter {
    pub position: Vec3,
    /// Full volume up to this distance, in meters.
    pub min_distance: f32,
    /// Silent beyond this distance.
    pub max_distance: f32,
}

impl Emitter {
    /// A sound of ordinary size at `position`, such as a blow (2 to 40 m).
    pub fn at(position: Vec3) -> Self {
        Self { position, ..Default::default() }
    }

    /// A small sound, such as a step, that doesn't carry (1 to 20 m).
    pub fn quiet(position: Vec3) -> Self {
        Self { position, min_distance: 1.0, max_distance: 20.0 }
    }

    /// A big sound, such as something breaking, heard across a valley (6 to 90 m).
    pub fn loud(position: Vec3) -> Self {
        Self { position, min_distance: 6.0, max_distance: 90.0 }
    }

    /// Gain heard from `listener`: 1 within `min_distance`, then the natural
    /// inverse-distance falloff (-6 dB per doubling), eased to silence at `max_distance`.
    pub fn gain_at(&self, listener: Vec3) -> f32 {
        let (min, max) = self.distances();
        let d = self.position.distance(listener);
        let t = ((d - min) / (max - min)).clamp(0.0, 1.0);
        min / d.max(min) * (1.0 - t * t)
    }

    fn distances(&self) -> (f32, f32) {
        let min = self.min_distance.max(0.01);
        (min, self.max_distance.max(min + 0.01))
    }
}

impl Default for Emitter {
    fn default() -> Self {
        Self { position: Vec3::ZERO, min_distance: 2.0, max_distance: 40.0 }
    }
}

/// How a sound reaches the listener.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Heard {
    /// From the distance and from what stands in between, without the sound's own volume.
    pub gain: f32,
    /// Hertz of the low-pass filter: lower when farther, behind the head or walled in.
    pub cutoff: f32,
    /// Seconds the sound takes to get here.
    pub delay: f32,
    /// How strongly it comes from one side, from 0 (both ears alike) to [`PAN_STRENGTH`].
    pub pan: f32,
}

impl Heard {
    /// The track's gain. Kira's panning makes a sound straight ahead lower than a flat
    /// one, and this gives that back, so a blow in front sounds as loud as the player's own.
    pub(crate) fn track_gain(&self) -> f32 {
        self.gain / (1.0 - self.pan * (1.0 - FRONT_LEVEL))
    }
}

/// Where the listener is, which may be two places.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Ears {
    /// The direction of a sound is judged from here (the camera), matching the picture.
    pub position: Vec3,
    pub orientation: Quat,
    /// How far a sound is, how late and what is in its way are judged from here. With
    /// a camera behind a character, this is the character: a blow on its shield isn't
    /// far away because the camera is.
    pub hearing_point: Option<Vec3>,
}

impl Default for Ears {
    fn default() -> Self {
        Self { position: Vec3::ZERO, orientation: Quat::IDENTITY, hearing_point: None }
    }
}

impl Ears {
    pub fn hearing_from(&self) -> Vec3 {
        self.hearing_point.unwrap_or(self.position)
    }

    /// What reaches the ears from `emitter`, with `occlusion` (0..1) of it blocked.
    pub fn hear(&self, emitter: &Emitter, occlusion: f32) -> Heard {
        let from = self.hearing_from();
        let distance = emitter.position.distance(from);
        let occlusion = occlusion.clamp(0.0, 1.0);

        let to_sound = emitter.position - self.position;
        let open = smoothstep(PAN_NEAR, PAN_FULL, to_sound.length());
        let ahead = (self.orientation * Vec3::NEG_Z).dot(to_sound.normalize_or_zero());
        let behind = (-ahead).max(0.0) * open;

        let air = (AIR_CLEAR / distance.max(AIR_CLEAR)).powf(AIR_ROLLOFF);
        let cutoff = OPEN_CUTOFF * air * BEHIND_CUTOFF.powf(behind) * OCCLUDED_CUTOFF.powf(occlusion);
        Heard {
            gain: emitter.gain_at(from) * (1.0 + (OCCLUDED_GAIN - 1.0) * occlusion),
            cutoff: cutoff.max(LOWEST_CUTOFF),
            delay: distance / SPEED_OF_SOUND,
            pan: PAN_STRENGTH * open,
        }
    }
}

/// How much of the way from `from` to `to` is blocked, from 0 to 1, by the part of a few
/// rays around `to` for which `blocked` answers true.
pub(crate) fn occlusion_between(from: Vec3, to: Vec3, blocked: &mut impl FnMut(Vec3, Vec3) -> bool) -> f32 {
    if from.distance(to) < ALWAYS_CLEAR {
        return 0.0;
    }
    let side = (to - from).cross(Vec3::Y).normalize_or(Vec3::X);
    let lifted = Vec3::Y * RAY_LIFT;
    let around = [lifted, Vec3::Y * RAY_OVER, lifted + side * RAY_SIDE, lifted - side * RAY_SIDE];
    let hits = around
        .into_iter()
        .filter(|offset| {
            let end = to + *offset;
            blocked(from, end - (end - from).normalize_or_zero() * RAY_CLEARANCE)
        })
        .count();
    hits as f32 / around.len() as f32
}

fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
