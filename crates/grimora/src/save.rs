//! Saved games: named slots on disk ([`SaveStore`]), each a RON [`SaveFile`]
//! holding the game's own state next to what the engine puts back by itself
//! ([`WorldState`]: the camera and the scene objects that moved or were hidden).
//!
//! Most games only need [`crate::Context::save_game`] and
//! [`crate::Context::load_game`].

use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result, bail};
use grimora_core::{Camera, Transform, Vec3};
use grimora_physics::Physics;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::World;

/// Version of the [`SaveFile`] layout. Files written by a newer engine are refused.
pub const SAVE_FORMAT: u32 = 1;

/// One saved game.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SaveFile<T> {
    pub format: u32,
    /// Seconds since the Unix epoch.
    pub saved_at: u64,
    /// File name of the scene the game was saved in (e.g. `"sandbox.ron"`).
    #[serde(default)]
    pub scene: Option<String>,
    pub world: WorldState,
    /// Whatever the game keeps: player, inventory, quests... Give new fields
    /// `#[serde(default)]` so older saves still load.
    pub game: T,
}

impl<T> SaveFile<T> {
    pub fn new(scene: Option<&Path>, world: WorldState, game: T) -> Self {
        let saved_at = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let scene = scene.and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned());
        Self { format: SAVE_FORMAT, saved_at, scene, world, game }
    }
}

/// What the engine saves of the world by itself.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WorldState {
    pub camera: CameraState,
    /// Scene objects that differ from the scene file: moved, hidden or still
    /// tumbling. Objects the game spawned and streamed scatter are not saved.
    #[serde(default)]
    pub objects: Vec<ObjectState>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct CameraState {
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObjectState {
    /// Index in the scene's `objects`.
    pub index: usize,
    /// Must still match on load, so a save made before the scene file changed
    /// doesn't move the wrong object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub transform: Transform,
    #[serde(default = "visible_default", skip_serializing_if = "is_visible")]
    pub visible: bool,
    /// Dynamic bodies still in motion: linear and angular velocity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub velocity: Option<(Vec3, Vec3)>,
}

fn visible_default() -> bool {
    true
}

fn is_visible(visible: &bool) -> bool {
    *visible
}

impl WorldState {
    /// Records the camera and every scene object that differs from the scene file.
    pub fn capture(world: &World, physics: &Physics, camera: &Camera) -> Self {
        let bodies: HashMap<usize, _> = physics.scene_bodies().iter().map(|b| (b.object, b.body)).collect();
        let objects = world
            .objects
            .iter()
            .zip(&world.baseline)
            .enumerate()
            .filter_map(|(index, (object, rest))| {
                let velocity = bodies
                    .get(&index)
                    .and_then(|&body| physics.body_velocity(body))
                    .filter(|(linear, angular)| linear.length_squared() + angular.length_squared() > 1e-8);
                let changed = object.transform != *rest || !object.visible || velocity.is_some();
                changed.then(|| ObjectState {
                    index,
                    name: object.name.clone(),
                    transform: object.transform,
                    visible: object.visible,
                    velocity,
                })
            })
            .collect();
        let camera = CameraState { position: camera.position, yaw: camera.yaw, pitch: camera.pitch };
        Self { camera, objects }
    }

    /// Puts the scene objects back as the scene file placed them, applies the
    /// saved changes on top (dynamic bodies included), moves the camera, and
    /// streams the world in afresh around it. Objects the game spawned are left
    /// to the game. Returns how many saved objects no longer match the scene.
    pub fn restore(&self, world: &mut World, physics: &mut Physics, camera: &mut Camera) -> usize {
        for (object, rest) in world.objects.iter_mut().zip(&world.baseline) {
            object.transform = *rest;
            object.visible = true;
        }
        let bodies: HashMap<usize, _> = physics.scene_bodies().iter().map(|b| (b.object, b.body)).collect();
        for (&index, &body) in &bodies {
            let rest = world.baseline[index];
            physics.set_body_state(body, rest.position, rest.rotation, Vec3::ZERO, Vec3::ZERO);
        }

        let mut unmatched = 0;
        for saved in &self.objects {
            let object = world
                .objects
                .get_mut(saved.index)
                .filter(|o| saved.index < world.baseline.len() && o.name == saved.name);
            let Some(object) = object else {
                log::warn!("saved object #{} ({:?}) is no longer in the scene; skipped", saved.index, saved.name);
                unmatched += 1;
                continue;
            };
            object.transform = saved.transform;
            object.visible = saved.visible;
            if let Some(&body) = bodies.get(&saved.index) {
                let (linear, angular) = saved.velocity.unwrap_or_default();
                physics.set_body_state(body, saved.transform.position, saved.transform.rotation, linear, angular);
            }
        }

        camera.position = self.camera.position;
        camera.yaw = self.camera.yaw;
        camera.pitch = self.camera.pitch;
        world.reload_streaming(camera.position, physics);
        unmatched
    }
}

/// A folder of saved games, one `<slot>.ron` file per slot. Slot names are
/// letters, digits, `-` and `_`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveStore {
    dir: PathBuf,
}

/// A slot found by [`SaveStore::slots`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotInfo {
    pub name: String,
    pub modified: SystemTime,
}

impl SaveStore {
    /// Slots in `dir`, created on the first write.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The usual place for a game's saves: `<user data>/<game>/saves`, where user
    /// data is `%APPDATA%` on Windows, `~/Library/Application Support` on macOS
    /// and `$XDG_DATA_HOME` (or `~/.local/share`) elsewhere; `./saves` if none is set.
    pub fn for_game(game: &str) -> Self {
        let env = |name| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
        let data = if cfg!(windows) {
            env("APPDATA")
        } else if cfg!(target_os = "macos") {
            env("HOME").map(|home| home.join("Library/Application Support"))
        } else {
            env("XDG_DATA_HOME").or_else(|| env("HOME").map(|home| home.join(".local/share")))
        };
        let folder: String =
            game.chars().map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '_' } else { c }).collect();
        match data {
            Some(data) => Self::new(data.join(folder.trim()).join("saves")),
            None => Self::new("saves"),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// File of `slot`, or an error if the name isn't a valid slot name.
    pub fn path(&self, slot: &str) -> Result<PathBuf> {
        let valid = !slot.is_empty()
            && slot.len() <= 64
            && slot.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !valid {
            bail!("invalid save slot name {slot:?} (use letters, digits, '-' and '_')");
        }
        Ok(self.dir.join(format!("{slot}.ron")))
    }

    pub fn exists(&self, slot: &str) -> bool {
        self.path(slot).is_ok_and(|p| p.is_file())
    }

    /// Writes `value` to `slot` as RON. The old file is replaced only once the
    /// new one is fully on disk, so a crash mid-save never loses the slot.
    pub fn write<T: Serialize>(&self, slot: &str, value: &T) -> Result<PathBuf> {
        let path = self.path(slot)?;
        std::fs::create_dir_all(&self.dir).with_context(|| format!("creating {}", self.dir.display()))?;
        let text = ron::ser::to_string_pretty(value, ron::ser::PrettyConfig::default())
            .with_context(|| format!("serializing save slot {slot:?}"))?;
        let temp = path.with_extension("ron.tmp");
        let mut file = std::fs::File::create(&temp).with_context(|| format!("creating {}", temp.display()))?;
        file.write_all(text.as_bytes())
            .and_then(|()| file.sync_all())
            .with_context(|| format!("writing {}", temp.display()))?;
        drop(file);
        std::fs::rename(&temp, &path).with_context(|| format!("replacing {}", path.display()))?;
        Ok(path)
    }

    pub fn read<T: DeserializeOwned>(&self, slot: &str) -> Result<T> {
        let path = self.path(slot)?;
        let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        ron::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn delete(&self, slot: &str) -> Result<()> {
        let path = self.path(slot)?;
        std::fs::remove_file(&path).with_context(|| format!("deleting {}", path.display()))
    }

    /// Every slot, most recently saved first. An empty list if the folder doesn't exist yet.
    pub fn slots(&self) -> Result<Vec<SlotInfo>> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e).with_context(|| format!("listing {}", self.dir.display())),
        };
        let mut slots: Vec<SlotInfo> = entries
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                let name = path.file_name()?.to_str()?.strip_suffix(".ron")?.to_owned();
                self.path(&name).ok()?;
                let modified = path.metadata().and_then(|m| m.modified()).ok()?;
                Some(SlotInfo { name, modified })
            })
            .collect();
        slots.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.name.cmp(&b.name)));
        Ok(slots)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_names_stay_inside_the_folder() {
        let store = SaveStore::new("saves");
        assert!(store.path("quick_1").is_ok());
        for bad in ["", "../x", "a/b", "a.b", "C:", "con fig"] {
            assert!(store.path(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn game_folder_names_are_sanitized() {
        let store = SaveStore::for_game("My: Game?");
        let dir = store.dir().to_string_lossy();
        assert!(dir.contains("My_ Game_"), "{dir}");
        assert!(store.dir().ends_with("saves"));
    }
}
