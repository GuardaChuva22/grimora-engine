//! Saved games: named slots on disk ([`SaveStore`]), each a RON [`SaveFile`]
//! holding the game's own state next to what the engine puts back by itself
//! ([`WorldState`]: the camera, the scene objects that moved, were hidden or
//! were despawned, and the state of the triggers).
//!
//! Most games only need [`crate::Context::save_game`] and
//! [`crate::Context::load_game`].

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result, bail};
use grimora_core::{Camera, Transform, Vec3, Weather, WorldPos, WorldTransform};
use grimora_physics::Physics;
use grimora_scene::Interactable;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::World;
use crate::combat::Health;
use crate::fauna::FaunaCount;
use crate::generated::{GenState, Generated};
use crate::interaction::TriggerState;
use crate::items::ItemState;
use crate::world::{Body, Renderable};

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
    /// Scene objects that differ from the scene file: moved, hidden, still
    /// tumbling, used up or despawned. Entities the game spawned and streamed scatter
    /// are not saved.
    #[serde(default)]
    pub objects: Vec<ObjectState>,
    #[serde(default)]
    pub triggers: TriggerState,
    /// The hour, in a scene with a day cycle.
    #[serde(default)]
    pub time_of_day: Option<f32>,
    /// The weather, in a scene with a day cycle.
    #[serde(default)]
    pub weather: Option<Weather>,
    /// How the wind was blowing.
    #[serde(default)]
    pub wind: Option<grimora_core::WindState>,
    /// What was taken from the world, what's in the chests and what was dropped.
    #[serde(default)]
    pub items: ItemState,
    /// How many animals are left in each group.
    #[serde(default)]
    pub fauna: Vec<FaunaCount>,
    /// What differs from the generated world (`--world`), by [`crate::GenId`]: only what was
    /// removed, moved or used, so a long walk still saves in a few KB.
    #[serde(default, skip_serializing_if = "GenState::is_empty")]
    pub generated: GenState,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct CameraState {
    /// In the world. Saves from before the origin moved had it local to zero, which is the same.
    pub position: WorldPos,
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
    /// In the world, like [`CameraState::position`].
    pub transform: WorldTransform,
    #[serde(default = "visible_default", skip_serializing_if = "is_visible")]
    pub visible: bool,
    /// Dynamic bodies still in motion: linear and angular velocity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub velocity: Option<(Vec3, Vec3)>,
    /// Despawned since the scene loaded (picked up, destroyed...).
    #[serde(default, skip_serializing_if = "is_false")]
    pub removed: bool,
    /// The game took away its `interact` (a chest already opened...).
    #[serde(default, skip_serializing_if = "is_false")]
    pub used: bool,
    /// Health left, for what `combat:` made hurtable and got hurt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<f32>,
}

fn visible_default() -> bool {
    true
}

fn is_visible(visible: &bool) -> bool {
    *visible
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl WorldState {
    /// Records the camera, the triggers and every scene object that differs
    /// from the scene file: moved, hidden, still tumbling, used up or despawned.
    pub fn capture(world: &World, physics: &Physics, camera: &Camera) -> Self {
        let live = world.scene_entities();
        let origin = world.origin();
        let mut objects = Vec::new();
        for (index, desc) in world.scene_objects.iter().enumerate() {
            let Some(&entity) = live.get(&index) else {
                objects.push(ObjectState {
                    index,
                    name: desc.name.clone(),
                    transform: origin.transform_to_world(&desc.transform),
                    visible: true,
                    velocity: None,
                    removed: true,
                    used: false,
                    health: None,
                });
                continue;
            };
            let mut query = world.ecs.query_one::<(
                &Transform,
                Option<&Renderable>,
                Option<&Body>,
                Option<&Interactable>,
                Option<&Health>,
            )>(entity);
            let Ok((transform, renderable, body, interact, health)) = query.get() else { continue };
            let visible = renderable.is_none_or(|r| r.visible);
            let velocity = body
                .and_then(|Body(body)| physics.body_velocity(*body))
                .filter(|(linear, angular)| linear.length_squared() + angular.length_squared() > 1e-8);
            let used = desc.interact.is_some() && interact.is_none();
            let health = health.filter(|health| health.current != health.max).map(|health| health.current);
            if *transform != desc.transform || !visible || velocity.is_some() || used || health.is_some() {
                objects.push(ObjectState {
                    index,
                    name: desc.name.clone(),
                    transform: origin.transform_to_world(transform),
                    visible,
                    velocity,
                    removed: false,
                    used,
                    health,
                });
            }
        }
        let mut generated = world.generated.clone();
        for (transform, made) in world.ecs.query::<(&Transform, &Generated)>().without::<&crate::Villager>().iter() {
            generated.note_place(made.id, &made.made, origin.transform_to_world(transform));
        }
        let camera = CameraState { position: origin.to_world(camera.position), yaw: camera.yaw, pitch: camera.pitch };
        Self {
            camera,
            objects,
            triggers: world.trigger_state.clone(),
            time_of_day: world.time_of_day(),
            weather: world.weather(),
            wind: Some(world.wind.state),
            items: world.item_state.clone(),
            fauna: world.fauna.census(),
            generated,
        }
    }

    /// Puts the scene objects back as the scene file placed them (respawning
    /// despawned ones), applies the saved changes on top (dynamic bodies
    /// included), moves the camera, puts the triggers back as they were, and
    /// streams the world in afresh around the camera.
    /// Entities the game spawned are left to the game; scene entities that
    /// were never despawned keep their ids. Returns how many saved objects no
    /// longer match the scene.
    pub fn restore(&self, world: &mut World, physics: &mut Physics, camera: &mut Camera) -> usize {
        let live = world.reset_scene_objects(physics);
        let origin = world.origin();
        let mut unmatched = 0;
        for saved in &self.objects {
            let in_scene = world.scene_objects.get(saved.index).is_some_and(|o| o.name == saved.name);
            let Some(&entity) = live.get(&saved.index).filter(|_| in_scene) else {
                log::warn!("saved object #{} ({:?}) is no longer in the scene; skipped", saved.index, saved.name);
                unmatched += 1;
                continue;
            };
            if saved.removed {
                world.despawn(entity, physics);
                continue;
            }
            let query = world.ecs.query_one_mut::<(&mut Transform, Option<&mut Renderable>, Option<&Body>)>(entity);
            let Ok((transform, renderable, body)) = query else { continue };
            *transform = origin.transform_to_local(&saved.transform);
            let saved_transform = *transform;
            if let Some(renderable) = renderable {
                renderable.visible = saved.visible;
            }
            if let Some(Body(body)) = body {
                let (linear, angular) = saved.velocity.unwrap_or_default();
                physics.set_body_state(*body, saved_transform.position, saved_transform.rotation, linear, angular);
            }
            if saved.used {
                let _ = world.ecs.remove_one::<Interactable>(entity);
            }
            if let Some(health) = saved.health {
                world.set_health(entity, health, physics);
            }
        }

        if let Some(hour) = self.time_of_day {
            world.set_time_of_day(hour);
        }
        if let Some(weather) = self.weather
            && world.weather() != Some(weather)
        {
            world.set_weather(weather);
        }
        if let Some(wind) = self.wind {
            world.wind.state = wind;
        }
        camera.position = origin.to_local(self.camera.position);
        camera.yaw = self.camera.yaw;
        camera.pitch = self.camera.pitch;
        world.trigger_state = self.triggers.clone();
        world.place_agents(physics);
        world.restore_items(&self.items, physics);
        world.generated = self.generated.clone();
        world.reset_flora(physics);
        world.reload_streaming(camera.position, physics);
        world.restore_fauna(&self.fauna);
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
