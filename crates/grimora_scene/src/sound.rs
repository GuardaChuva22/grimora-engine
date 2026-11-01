//! Looping sounds placed in a scene: ambience heard everywhere, or emitters
//! that get quieter with distance and pan with the listener.

use std::path::{Path, PathBuf};

use grimora_core::{Transform, Vec3};
use serde::{Deserialize, Serialize};

/// A looping sound in the scene file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundDesc {
    /// WAV or OGG file, relative to the scene file.
    pub path: String,
    /// Where the sound comes from; with a terrain, `y` is measured from the ground.
    /// `None` is ambience: same volume everywhere, no panning. On an object this is
    /// an offset rotated with it, and the sound is always positional.
    pub position: Option<Vec3>,
    /// Linear gain; 1 plays the file as recorded.
    pub volume: f32,
    /// Full volume up to this distance, in meters.
    pub min_distance: f32,
    /// Silent beyond this distance.
    pub max_distance: f32,
}

impl Default for SoundDesc {
    fn default() -> Self {
        Self { path: String::new(), position: None, volume: 1.0, min_distance: 1.0, max_distance: 25.0 }
    }
}

/// A resolved scene sound: path relative to the working directory, world-space position.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneSound {
    pub path: PathBuf,
    /// `None` for ambience.
    pub position: Option<Vec3>,
    pub volume: f32,
    pub min_distance: f32,
    pub max_distance: f32,
}

impl SoundDesc {
    /// Free-standing sound; `ground` gives the terrain height at `(x, z)`.
    pub(crate) fn resolve(&self, dir: &Path, ground: impl Fn(f32, f32) -> f32) -> SceneSound {
        let position = self.position.map(|p| p + Vec3::Y * ground(p.x, p.z));
        self.resolved(dir, position)
    }

    /// Sound attached to an object: rotated with it, but not scaled (like lights).
    pub(crate) fn attached(&self, dir: &Path, transform: &Transform) -> SceneSound {
        let offset = self.position.unwrap_or(Vec3::ZERO);
        self.resolved(dir, Some(transform.position + transform.rotation * offset))
    }

    fn resolved(&self, dir: &Path, position: Option<Vec3>) -> SceneSound {
        SceneSound {
            path: dir.join(&self.path),
            position,
            volume: self.volume.max(0.0),
            min_distance: self.min_distance.max(0.01),
            max_distance: self.max_distance.max(self.min_distance.max(0.01) + 0.01),
        }
    }
}
