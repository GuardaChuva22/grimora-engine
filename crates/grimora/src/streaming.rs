//! Loads and unloads parts of the world around a point (see
//! [`grimora_scene::SceneStreaming`]): grid cells of scattered objects, built
//! when the scene loads and switched on and off here, and region files, read
//! and built on a background thread.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use grimora_core::{MeshData, PointLight, Transform, Vec3, WaterBody};
use grimora_physics::Physics;
use grimora_scene::{CellCoord, Scene, SceneObject, SceneRegion, SceneSound, SceneStreaming, Terrain};
use hecs::Entity;

use crate::world::{MeshId, despawn, object_builder};

/// Cells switched on per frame at most, so walking into new ground never stalls a frame.
const CELLS_PER_FRAME: usize = 8;

/// What a loaded cell or region adds to the world.
#[derive(Debug, Default)]
pub(crate) struct Chunk {
    pub entities: Vec<Entity>,
    /// A region's free-standing lights (lights on objects are components).
    pub lights: Vec<PointLight>,
    pub sounds: Vec<SceneSound>,
    /// A region's lakes and rivers, which [`World::waters`](crate::World) answers for while
    /// it is loaded.
    pub waters: Vec<WaterBody>,
}

impl Chunk {
    fn new(
        objects: &[SceneObject],
        mesh: impl Fn(usize) -> MeshId,
        meshes: &[MeshData],
        ecs: &mut hecs::World,
        physics: &mut Physics,
        offset: Vec3,
    ) -> Self {
        let entities = objects
            .iter()
            .map(|o| ecs.spawn(object_builder(o, mesh(o.mesh), meshes, physics, offset).build()))
            .collect();
        Chunk { entities, ..Default::default() }
    }

    /// Despawns the entities (the game may have despawned some already).
    fn unload(self, ecs: &mut hecs::World, physics: &mut Physics) {
        for entity in self.entities {
            despawn(ecs, entity, physics);
        }
    }
}

#[derive(Debug)]
enum RegionState {
    Unloaded,
    /// Being built on the worker thread.
    Loading,
    Loaded(Chunk),
    /// Failed to load; not retried until the scene reloads.
    Failed,
}

#[derive(Debug)]
struct Region {
    desc: SceneRegion,
    state: RegionState,
    /// World meshes from an earlier load, reused when the region comes back.
    meshes: Vec<MeshId>,
}

type RegionResult = (usize, anyhow::Result<Scene>);

/// Builds region files off the main thread.
#[derive(Debug)]
struct Worker {
    jobs: Sender<(usize, PathBuf)>,
    results: Receiver<RegionResult>,
}

impl Worker {
    /// The thread ends on its own once the streamer (and so `jobs`) is dropped.
    fn spawn(terrain: Option<Terrain>) -> Self {
        let (jobs, inbox) = mpsc::channel::<(usize, PathBuf)>();
        let (outbox, results) = mpsc::channel();
        std::thread::Builder::new()
            .name("grimora-regions".into())
            .spawn(move || {
                for (index, path) in inbox {
                    let started = std::time::Instant::now();
                    let scene = Scene::load_region(&path, terrain.as_ref());
                    if scene.is_ok() {
                        log::info!(
                            "Built region {} in {:.0} ms",
                            path.display(),
                            started.elapsed().as_secs_f64() * 1000.0
                        );
                    }
                    if outbox.send((index, scene)).is_err() {
                        break;
                    }
                }
            })
            .expect("spawning the region loader thread");
        Self { jobs, results }
    }
}

/// How much of the world is loaded (see [`crate::World::streaming_stats`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StreamingStats {
    pub loaded_cells: usize,
    pub cells: usize,
    pub loaded_regions: usize,
    pub regions: usize,
    /// Entities spawned by loaded cells and regions.
    pub objects: usize,
}

#[derive(Debug)]
pub(crate) struct Streamer {
    scene: SceneStreaming,
    terrain: Option<Terrain>,
    pub loaded: HashMap<CellCoord, Chunk>,
    regions: Vec<Region>,
    worker: Option<Worker>,
    /// How far the world's origin has moved what the scene placed: added to everything that
    /// loads, and taken from the focus to find what is near.
    shifted: Vec3,
}

impl Streamer {
    pub fn new(scene: SceneStreaming, terrain: Option<&Terrain>) -> Self {
        let regions: Vec<Region> = scene
            .regions
            .iter()
            .map(|desc| Region { desc: desc.clone(), state: RegionState::Unloaded, meshes: Vec::new() })
            .collect();
        let worker = (!regions.is_empty()).then(|| Worker::spawn(terrain.cloned()));
        Self { scene, terrain: terrain.cloned(), loaded: HashMap::new(), regions, worker, shifted: Vec3::ZERO }
    }

    /// Moves what is loaded and what will load by `offset`, as the world's origin moves the
    /// other way. The entities move with the rest of the world.
    pub fn shift(&mut self, offset: Vec3) {
        self.shifted += offset;
        let chunks = self.loaded.values_mut().chain(self.regions.iter_mut().filter_map(|r| match &mut r.state {
            RegionState::Loaded(chunk) => Some(chunk),
            _ => None,
        }));
        for chunk in chunks {
            for light in &mut chunk.lights {
                light.position += offset;
            }
            for sound in &mut chunk.sounds {
                if let Some(position) = &mut sound.position {
                    *position += offset;
                }
            }
            for water in &mut chunk.waters {
                water.shift(offset);
            }
        }
    }

    /// Loads what is within reach of `focus` and unloads what was left behind.
    /// With `wait`, everything in reach is loaded before returning; otherwise
    /// cells come in a few per frame and regions in the background. Returns
    /// whether anything changed.
    pub fn update(
        &mut self,
        focus: Vec3,
        ecs: &mut hecs::World,
        meshes: &mut Vec<MeshData>,
        physics: &mut Physics,
        wait: bool,
    ) -> bool {
        let focus = focus - self.shifted;
        let load = self.scene.load_distance;
        // Things unload a little further out than they load, so walking along
        // the edge doesn't flip them on and off.
        let unload = load + (self.scene.cell_size * 0.5).max(10.0);
        let mut changed = false;

        let scene = &self.scene;
        self.loaded.retain(|&cell, chunk| {
            let keep = scene.cell_distance(cell, focus) <= unload;
            if !keep {
                std::mem::take(chunk).unload(ecs, physics);
                changed = true;
            }
            keep
        });
        let reach = (load / scene.cell_size).ceil() as i32;
        let (cx, cz) = scene.cell_of(focus);
        let mut wanted: Vec<(f32, CellCoord)> = (cx - reach..=cx + reach)
            .flat_map(|x| (cz - reach..=cz + reach).map(move |z| (x, z)))
            .filter(|cell| scene.cells.contains_key(cell) && !self.loaded.contains_key(cell))
            .map(|cell| (scene.cell_distance(cell, focus), cell))
            .filter(|&(distance, _)| distance < load)
            .collect();
        wanted.sort_by(|a, b| a.0.total_cmp(&b.0));
        let budget = if wait { wanted.len() } else { CELLS_PER_FRAME };
        for (_, cell) in wanted.into_iter().take(budget) {
            let chunk = Chunk::new(&scene.cells[&cell].objects, MeshId, meshes, ecs, physics, self.shifted);
            self.loaded.insert(cell, chunk);
            changed = true;
        }

        changed |= self.update_regions(focus, (load, unload), ecs, meshes, physics, wait);
        changed
    }

    fn update_regions(
        &mut self,
        focus: Vec3,
        (load, unload): (f32, f32),
        ecs: &mut hecs::World,
        meshes: &mut Vec<MeshData>,
        physics: &mut Physics,
        wait: bool,
    ) -> bool {
        let mut changed = false;
        for index in 0..self.regions.len() {
            let region = &mut self.regions[index];
            let distance = region.desc.distance(focus);
            match region.state {
                RegionState::Loaded(_) if distance > unload => {
                    if let RegionState::Loaded(chunk) = std::mem::replace(&mut region.state, RegionState::Unloaded) {
                        chunk.unload(ecs, physics);
                    }
                    log::info!("Unloaded region {}", region.desc.path.display());
                    changed = true;
                }
                RegionState::Unloaded if distance < load => {
                    if wait {
                        let scene = Scene::load_region(&region.desc.path, self.terrain.as_ref());
                        changed |= finish(region, scene, ecs, meshes, physics, self.shifted);
                    } else if let Some(worker) = &self.worker {
                        region.state = RegionState::Loading;
                        let _ = worker.jobs.send((index, region.desc.path.clone()));
                    }
                }
                _ => {}
            }
        }

        let Some(worker) = &self.worker else { return changed };
        loop {
            let pending = self.regions.iter().any(|r| matches!(r.state, RegionState::Loading));
            let result = if wait && pending { worker.results.recv().ok() } else { worker.results.try_recv().ok() };
            let Some((index, scene)) = result else { break };
            let region = &mut self.regions[index];
            if !matches!(region.state, RegionState::Loading) {
                continue;
            }
            if region.desc.distance(focus) > unload {
                // The camera moved on while it was being built.
                region.state = RegionState::Unloaded;
                continue;
            }
            changed |= finish(region, scene, ecs, meshes, physics, self.shifted);
        }
        changed
    }

    /// Unloads every cell and region; the next update loads them again as the
    /// scene describes them (regions that failed stay failed).
    pub fn unload_all(&mut self, ecs: &mut hecs::World, physics: &mut Physics) {
        for (_, chunk) in self.loaded.drain() {
            chunk.unload(ecs, physics);
        }
        for region in &mut self.regions {
            match std::mem::replace(&mut region.state, RegionState::Unloaded) {
                RegionState::Loaded(chunk) => chunk.unload(ecs, physics),
                // A build still in flight is dropped when it arrives.
                RegionState::Loading | RegionState::Unloaded => {}
                RegionState::Failed => region.state = RegionState::Failed,
            }
        }
    }

    pub fn chunks(&self) -> impl Iterator<Item = &Chunk> {
        self.loaded.values().chain(self.regions.iter().filter_map(|r| match &r.state {
            RegionState::Loaded(chunk) => Some(chunk),
            _ => None,
        }))
    }

    pub fn stats(&self) -> StreamingStats {
        StreamingStats {
            loaded_cells: self.loaded.len(),
            cells: self.scene.cells.len(),
            loaded_regions: self.regions.iter().filter(|r| matches!(r.state, RegionState::Loaded(_))).count(),
            regions: self.regions.len(),
            objects: self.chunks().map(|c| c.entities.len()).sum(),
        }
    }
}

/// Adds a built region to the world, or records why it couldn't be built.
fn finish(
    region: &mut Region,
    scene: anyhow::Result<Scene>,
    ecs: &mut hecs::World,
    meshes: &mut Vec<MeshData>,
    physics: &mut Physics,
    offset: Vec3,
) -> bool {
    let scene = match scene {
        Ok(scene) => scene,
        Err(err) => {
            log::error!("Region {}: {err:#}", region.desc.path.display());
            region.state = RegionState::Failed;
            return false;
        }
    };
    if region.meshes.len() != scene.meshes.len() {
        region.meshes = scene
            .meshes
            .into_iter()
            .map(|mesh| {
                meshes.push(mesh);
                MeshId(meshes.len() - 1)
            })
            .collect();
    }
    let ids = &region.meshes;
    let mut chunk = Chunk::new(&scene.objects, |i| ids[i], meshes, ecs, physics, offset);
    let moved = |t: &Transform| Transform { position: t.position + offset, ..*t };
    chunk.entities.extend(scene.triggers.into_iter().map(|t| ecs.spawn((moved(&t.transform), t.trigger))));
    chunk.entities.extend(scene.effects.iter().map(|(t, e)| ecs.spawn((moved(t), e.system(t)))));
    chunk.lights = scene.lights;
    chunk.sounds = scene.sounds;
    chunk.waters = scene.waters.bodies;
    for water in &mut chunk.waters {
        water.shift(offset);
    }
    for light in &mut chunk.lights {
        light.position += offset;
    }
    for sound in &mut chunk.sounds {
        if let Some(position) = &mut sound.position {
            *position += offset;
        }
    }
    log::info!("Loaded region {} ({} objects)", region.desc.path.display(), chunk.entities.len());
    region.state = RegionState::Loaded(chunk);
    true
}
