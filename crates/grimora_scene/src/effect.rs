//! Particle effects in their own `.ron` files, shared by every scene that
//! names them. Loaded effects are cached by file path and modification time.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use anyhow::{Context as _, Result};
use grimora_core::{ParticleDesc, ParticleEffect, ParticleSystem, Transform, Vec3};
use serde::{Deserialize, Serialize};

/// A particle effect placed in a scene. On an object, `position` is an offset
/// rotated with it (not scaled), like `light`; on its own, `y` is measured from
/// the ground, like `sounds`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EffectDesc {
    /// Effect file, relative to the scene file.
    pub path: String,
    pub position: Vec3,
    /// Multiplies the effect's sizes, speeds and distances.
    pub scale: f32,
}

impl Default for EffectDesc {
    fn default() -> Self {
        Self { path: String::new(), position: Vec3::ZERO, scale: 1.0 }
    }
}

/// A loaded effect ready to start.
#[derive(Debug, Clone)]
pub struct SceneEffect {
    pub effect: ParticleEffect,
    pub offset: Vec3,
    pub scale: f32,
}

impl SceneEffect {
    /// The effect running on an entity at `transform`. Its seed comes from where
    /// it is, so equal effects look different but the same place never changes.
    pub fn system(&self, transform: &Transform) -> ParticleSystem {
        let [x, y, z] = transform.position.to_array().map(|v| u64::from(v.to_bits()));
        let seed = x ^ y.rotate_left(21) ^ z.rotate_left(42);
        ParticleSystem::new(self.effect.clone(), self.offset, self.scale, seed)
    }
}

type Cache = HashMap<PathBuf, (Option<SystemTime>, ParticleEffect)>;

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Loads an effect file: a list of emitters. Reuses the cached copy while the
/// file is unchanged, so games can call this whenever they play one.
pub fn load_effect(path: impl AsRef<Path>) -> Result<ParticleEffect> {
    let path = path.as_ref();
    let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    if let Some((cached_mtime, effect)) = cache().lock().unwrap().get(path)
        && *cached_mtime == mtime
    {
        return Ok(effect.clone());
    }
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let effect = parse_effect(&text).with_context(|| format!("parsing {}", path.display()))?;
    cache().lock().unwrap().insert(path.to_path_buf(), (mtime, effect.clone()));
    Ok(effect)
}

/// Parses the text of an effect file.
pub fn parse_effect(text: &str) -> Result<ParticleEffect> {
    let options = ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
    let emitters: Vec<ParticleDesc> = options.from_str(text)?;
    Ok(emitters.into())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn effect_files_are_lists_of_emitters() {
        let effect = parse_effect(
            r#"[
                (rate: 30.0, lifetime: (0.5, 1.0), sizes: [0.1, 0.3], colors: [(1.0, 0.8, 0.3, 1.0), (1.0, 0.2, 0.1, 0.0)],
                 additive: 1.0, emissive: 4.0, duration: 2.0),
                (shape: Diamond, stretch: 0.05),
            ]"#,
        )
        .unwrap();
        assert_eq!(effect.len(), 2);
        assert_eq!(effect[0].rate, 30.0);
        assert_eq!(effect[0].duration, Some(2.0));
        assert_eq!(effect[1].shape, grimora_core::ParticleShape::Diamond);
        assert_eq!(effect[1].rate, ParticleDesc::default().rate, "unlisted fields use their defaults");
        assert!(parse_effect("[(nonsense: 1.0)]").is_err());
    }

    #[test]
    fn the_cache_notices_a_changed_file() {
        let path = std::env::temp_dir().join(format!("grimora_effect_{}.ron", std::process::id()));
        std::fs::write(&path, "[(rate: 1.0)]").unwrap();
        let first = load_effect(&path).unwrap();
        assert!(Arc::ptr_eq(&first, &load_effect(&path).unwrap()), "unchanged: same effect");

        // Rewrite until the file system's clock shows a different time.
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();
        while std::fs::metadata(&path).unwrap().modified().unwrap() == before {
            std::thread::sleep(std::time::Duration::from_millis(5));
            std::fs::write(&path, "[(rate: 2.0)]").unwrap();
        }
        assert_eq!(load_effect(&path).unwrap()[0].rate, 2.0);
        std::fs::remove_file(&path).unwrap();
        assert!(load_effect(&path).is_err());
    }
}
