//! What the scene file says about fighting. The behavior is in `grimora::combat`.

use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

/// `combat:` on an object: it can be hurt, and with a weapon it can hurt back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CombatDesc {
    pub health: f32,
    /// Blows only land on other teams. The player is usually team 0.
    pub team: u32,
    /// How wide it is, in meters: a blow reaches this much further to hit it.
    pub radius: f32,
    pub weapon: Option<Weapon>,
}

impl Default for CombatDesc {
    fn default() -> Self {
        Self { health: 30.0, team: 1, radius: 0.4, weapon: None }
    }
}

/// A firearm: what leaves the barrel, and how it feels to fire it. It loads one shot at a time,
/// and how the character does that is in its clips. Positions are in the model's own space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GunDesc {
    pub damage: f32,
    /// Meters a shot goes.
    pub range: f32,
    /// Meters of full damage; from here on it fades, down to half at `range`.
    pub full_damage_to: f32,
    /// The item it loads, one for each shot.
    pub ammo: String,
    pub muzzle: Vec3,
    /// Where the priming flashes.
    pub pan: Vec3,
    /// How far a shot strays, in degrees, from the hip and with the gun up.
    pub spread: f32,
    pub aimed_spread: f32,
    /// Seconds from the flint falling to the shot: a flintlock doesn't fire at once.
    pub lock_time: f32,
    /// Seconds to bring it up, when fired without aiming first.
    pub raise: f32,
    /// How far the view kicks, in degrees: up, and to either side at most.
    pub recoil: Vec2,
    pub shake: f32,
    /// Meters per second it shoves the character back.
    pub shove: f32,
    /// What the field of view is multiplied by, when aiming.
    pub zoom: f32,
    /// How big the flash and the smoke are; 1 for a pistol.
    pub power: f32,
    /// Start of the clips' names: `<clip>_hold`, `_aim`, `_fire` and `_reload`.
    pub clip: String,
    pub reload: ReloadDesc,
}

impl Default for GunDesc {
    fn default() -> Self {
        Self {
            damage: 30.0,
            range: 60.0,
            full_damage_to: 25.0,
            ammo: String::new(),
            muzzle: Vec3::X,
            pan: Vec3::ZERO,
            spread: 3.0,
            aimed_spread: 0.5,
            lock_time: 0.06,
            raise: 0.15,
            recoil: Vec2::new(4.0, 1.0),
            shake: 0.05,
            shove: 0.0,
            zoom: 1.0,
            power: 1.0,
            clip: String::new(),
            reload: ReloadDesc::default(),
        }
    }
}

/// When things happen in the reload clip, in seconds. Overlays don't fire events, so these say.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReloadDesc {
    /// The ball is in the free hand: from taking it to dropping it down the barrel.
    pub ball: (f32, f32),
    /// The rod is out of the gun and in the free hand.
    pub rod: (f32, f32),
    /// Its length in meters, and which point of it is in the fist over time, as (seconds, meters
    /// from the end that goes down the barrel): the hand slides up and down a long one.
    pub rod_length: f32,
    pub rod_grip: Vec<(f32, f32)>,
    /// Each time the rod hits bottom.
    pub strokes: Vec<f32>,
    /// The thumb pulls the hammer back.
    pub cock: f32,
    /// A gun too long to load from where the hand holds it changes grip for it.
    pub grounded: Option<GunGrip>,
}

/// Holding a gun some other way for part of the reload.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GunGrip {
    /// From the joint to the model, like the item's own, and how it turns there in degrees.
    pub at: Vec3,
    pub turn: Vec3,
    /// When the hand slides there, and when it slides back, each from one second to another.
    pub slide_in: (f32, f32),
    pub slide_out: (f32, f32),
}

/// A blow: how hard, how far, and which clip delivers it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Weapon {
    pub damage: f32,
    /// Meters from the attacker, on the ground plane.
    pub reach: f32,
    /// Degrees in front of the attacker, in total.
    pub arc: f32,
    /// Seconds between the starts of two blows.
    pub cooldown: f32,
    /// Clip to play. Without it, the blow lands right away.
    pub clip: String,
    /// Event of that clip at which the blow lands; without it, at the clip's end.
    /// A weapon with a `blade` ignores it, and hurts between `hit_on` and `hit_off`.
    pub event: String,
    /// The clips of the blows that can follow, in order. Each starts when the one
    /// before fires `combo`, if the attack was asked for in time.
    pub combo: Vec<String>,
    /// Hurts what it touches, rather than everything in `reach` and `arc`, which
    /// then only tell characters when to swing.
    pub blade: Option<Blade>,
    /// What each blow takes from the attacker's `Stamina`.
    pub stamina: f32,
    /// Seconds both sides hold still when a blow lands.
    pub hit_stop: f32,
}

/// The cutting part of a weapon, along the local Y of a joint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Blade {
    pub joint: String,
    /// Meters from the joint.
    pub base: f32,
    pub tip: f32,
    pub radius: f32,
}

impl Default for Blade {
    fn default() -> Self {
        Self { joint: "weapon.R".into(), base: 0.11, tip: 0.79, radius: 0.05 }
    }
}

impl Default for Weapon {
    fn default() -> Self {
        Self {
            damage: 8.0,
            reach: 1.6,
            arc: 120.0,
            cooldown: 1.5,
            clip: "attack".into(),
            event: "hit".into(),
            combo: Vec::new(),
            blade: None,
            stamina: 0.0,
            hit_stop: 0.0,
        }
    }
}
