//! What the scene file says about fighting. The behavior is in `grimora::combat`.

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
