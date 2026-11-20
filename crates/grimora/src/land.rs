//! The ground of a generated world (`--world`): tiles of [`grimora_world`] around the camera,
//! finer near it, built on all cores when they come into view, and colliders only for the
//! tiles around the player. Tiles are kept by their world key, never by local position, so an
//! origin shift only moves their entities and colliders, as it does everything else.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use glam::{DVec2, DVec3, Vec2, Vec3};
use grimora_core::{
    Material, MeshData, RiverPoint, TextureData, Transform, WaterBody, WaterLook, WaterShape, WorldPos, WorldTransform,
};
use grimora_physics::{ColliderHandle, Physics};
use grimora_scene::{ColliderShape, Ground, RiverDesc, load_texture};
use grimora_world::{
    FloraKey, FreshCourses, FreshWaterLook, LifeBook, MacroMap, PlantSpot, Relief, TileKey, TileLook, build_tile,
    flora_around, map_to_world, place_plants, tile_collider, tiles_around,
};
use hecs::Entity;

use crate::generated::Generated;
use crate::world::{MeshId, Name, Renderable, World};

/// Level 0 tiles on each side of the player's that get a collider: 5 × 5 tiles, 320 m.
const COLLIDER_REACH: i64 = 2;

/// A generated world's ground, kept in [`World::land`].
pub struct Land {
    pub(crate) relief: Arc<Relief>,
    look: TileLook,
    view: f64,
    shown: HashMap<TileKey, Shown>,
    /// Meshes of tiles that went away, reused by the next ones.
    spare: Vec<MeshId>,
    colliders: HashMap<TileKey, ColliderHandle>,
    fresh_water: FreshWater,
    /// Where the sea's depth map is centered (world), and the one being made.
    bed_at: Option<DVec2>,
    bed_job: Option<std::thread::JoinHandle<(DVec2, Vec<f32>)>>,
    life: Arc<LifeBook>,
    /// Where the life file is, for its plants' pictures.
    life_dir: std::path::PathBuf,
    /// The village's house, built by `set_land` and spawned by the first `update_land`, which
    /// has the physics: the house, where its floor's middle is, its turn and its name.
    #[allow(clippy::type_complexity)]
    village: Option<(Vec<(grimora_scene::House, DVec3, f32, String)>, Vec<(std::path::PathBuf, DVec3, f64, bool)>)>,
    flora: Flora,
    pub(crate) folk: crate::folk::Folk,
    /// The streams' courses, for the water each tile draws over itself.
    fresh_courses: Arc<FreshCourses>,
    /// The streams' and ponds' material, given to the water of every tile.
    fresh_material: Material,
    /// Tiles built ahead and not shown yet, by key: shown all at once when every tile the view
    /// wants is here, so the ground never has a hole. Kept a while after the view moves on.
    ready: HashMap<TileKey, Built>,
    /// Builds tiles, colliders and plants on other threads; `None` until the first update.
    workers: Option<Workers>,
    /// Builds everything in [`World::update_land`] itself, as tests want.
    blocking: bool,
}

/// A tile's ground, and the fresh water over it on the same grid, if any.
type Built = (MeshData, Option<MeshData>);

/// A tile on screen: its ground and its water, each an entity and the mesh it draws.
struct Shown {
    ground: (Entity, MeshId),
    water: Option<(Entity, MeshId)>,
}

/// Tiles kept built but unseen at most: past this, those the view doesn't want go.
const MOST_READY: usize = 384;

/// Work for [`Workers`]. Each result depends only on its key, so the order they come back in
/// doesn't change the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Job {
    Collider(TileKey),
    Tile(TileKey),
    Plants(FloraKey),
}

enum Done {
    Collider(TileKey, Vec<Vec3>, Vec<[u32; 3]>),
    Tile(TileKey, Built),
    Plants(FloraKey, Vec<PlantSpot>),
}

/// What the workers build from.
struct Makers {
    pub(crate) relief: Arc<Relief>,
    look: TileLook,
    life: Arc<LifeBook>,
    courses: Arc<FreshCourses>,
    sea: Option<WaterLook>,
}

impl Makers {
    fn run(&self, job: Job) -> Done {
        match job {
            Job::Collider(key) => {
                let (vertices, indices) = tile_collider(&self.relief, key);
                Done::Collider(key, vertices, indices)
            }
            Job::Tile(key) => {
                Done::Tile(key, build_one(&self.relief, &self.look, &self.courses, self.sea.as_ref(), key))
            }
            Job::Plants(key) => Done::Plants(key, place_plants(&self.relief, &self.life, key)),
        }
    }
}

#[derive(Default)]
struct Queue {
    /// Waiting, most urgent first; the main thread replaces it each frame.
    jobs: std::collections::VecDeque<Job>,
    /// Taken by a worker and not sent back yet.
    running: HashSet<Job>,
    closed: bool,
}

/// A few threads that take [`Job`]s from a shared queue, so building never holds up a frame.
struct Workers {
    queue: Arc<(std::sync::Mutex<Queue>, std::sync::Condvar)>,
    done: std::sync::mpsc::Receiver<Done>,
}

impl Workers {
    fn new(makers: Arc<Makers>) -> Self {
        let queue: Arc<(std::sync::Mutex<Queue>, std::sync::Condvar)> = Arc::default();
        let (send, done) = std::sync::mpsc::channel();
        // One core stays for the frame.
        let threads = std::thread::available_parallelism().map_or(3, |n| n.get()).saturating_sub(1).max(1);
        for i in 0..threads {
            let queue = queue.clone();
            let send = send.clone();
            let makers = makers.clone();
            let spawned = std::thread::Builder::new().name(format!("grimora-land-{i}")).spawn(move || {
                loop {
                    let job = {
                        let mut q = queue.0.lock().unwrap_or_else(|e| e.into_inner());
                        loop {
                            if q.closed {
                                return;
                            }
                            if let Some(job) = q.jobs.pop_front() {
                                q.running.insert(job);
                                break job;
                            }
                            q = queue.1.wait(q).unwrap_or_else(|e| e.into_inner());
                        }
                    };
                    let done = makers.run(job);
                    if send.send(done).is_err() {
                        return;
                    }
                    queue.0.lock().unwrap_or_else(|e| e.into_inner()).running.remove(&job);
                }
            });
            if let Err(e) = spawned {
                log::error!("Land worker: {e}");
            }
        }
        Self { queue, done }
    }

    /// Replaces what waits with `jobs`, skipping those being built.
    fn ask(&self, jobs: Vec<Job>) {
        let mut q = self.queue.0.lock().unwrap_or_else(|e| e.into_inner());
        q.jobs.clear();
        let running = std::mem::take(&mut q.running);
        q.jobs.extend(jobs.into_iter().filter(|job| !running.contains(job)));
        q.running = running;
        drop(q);
        self.queue.1.notify_all();
    }

    fn take(&self) -> Vec<Done> {
        self.done.try_iter().collect()
    }
}

impl Drop for Workers {
    fn drop(&mut self) {
        self.queue.0.lock().unwrap_or_else(|e| e.into_inner()).closed = true;
        self.queue.1.notify_all();
    }
}

/// The plants shown around the camera, by cell.
#[derive(Default)]
struct Flora {
    /// Each plant's variants, made when the land is set.
    meshes: Vec<Vec<MeshId>>,
    /// What each variant bumps into, if anything, beside [`Flora::meshes`].
    shapes: Vec<Vec<Option<Arc<ColliderShape>>>>,
    /// What each plant is underfoot (a trunk is wood, a boulder stone), for the steps.
    grounds: Vec<Ground>,
    shown: HashMap<FloraKey, Vec<Entity>>,
    /// The cells the eye wants, shown or on their way.
    wanted: HashSet<FloraKey>,
    /// Where the cells were last picked from: they are picked again only after a few meters.
    eye: Option<DVec2>,
    /// The solid plants shown.
    solids: HashMap<Entity, Solid>,
    /// The ones near the player, with their colliders.
    colliders: HashMap<Entity, ColliderHandle>,
    /// Where those were last picked around; `None` picks them again.
    focus: Option<DVec2>,
}

/// A plant that bumps into the player when near.
struct Solid {
    /// Which of the life file's plants.
    plant: usize,
    at: WorldPos,
    rotation: glam::Quat,
    scale: f32,
    shape: Arc<ColliderShape>,
}

/// Meters the camera moves before the plants' cells are picked again.
const FLORA_MOVE: f64 = 4.0;
/// Where a plant starts to dither away, as a share of its reach. It is gone
/// [`FLORA_MOVE`] × 1.5 before the reach, where its cell may not be there yet.
const FLORA_FADE_FROM: f64 = 0.6;

/// Lets a plant dither away before the edge of its reach, so its cells come and go unseen.
fn fade_by_reach(mesh: &mut grimora_core::MeshData, reach: f64) {
    let gone = (reach - FLORA_MOVE * 1.5).max(reach * 0.5);
    let fade = glam::Vec2::new((reach * FLORA_FADE_FROM).min(gone * 0.9) as f32, gone as f32);
    for material in mesh.materials.iter_mut().chain(mesh.lods.iter_mut().flat_map(|lod| lod.mesh.materials.iter_mut()))
    {
        material.fade = fade;
    }
}
/// The map of the sea's depth the waves shoal and break on: samples a side, meters between
/// them, and how far the camera goes before it is made again around it (on another thread).
const BED_SAMPLES: usize = 640;
const BED_SPACING: f64 = 5.0;
const BED_MOVE: f64 = 500.0;
/// Solid plants within this many meters of the player get a collider.
const PLANT_COLLIDER_REACH: f64 = 60.0;
/// Meters the player moves before those are picked again.
const PLANT_COLLIDER_MOVE: f64 = 4.0;
/// Meters of fall between two emitters of spray down its sheet.
const FALL_VEIL_EVERY: f32 = 7.0;
/// Meters above the ground over which the land's ambience fades out.
const AMBIENCE_ALOFT: f32 = 120.0;

/// The streams' and ponds' look, with its pictures loaded.
struct FreshWater {
    look: FreshWaterLook,
    texture: Option<Arc<TextureData>>,
    foam: Option<Arc<TextureData>>,
    fall_effect: Option<grimora_core::ParticleEffect>,
    fall_veil: Option<grimora_core::ParticleEffect>,
    fall_sound: Option<std::path::PathBuf>,
}

/// What [`World::update_land`] did, for logs and tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LandStats {
    pub tiles: usize,
    pub built: usize,
    pub colliders: usize,
    /// Cells of plants shown, and the plants in them.
    pub flora_cells: usize,
    pub plants: usize,
    /// Trees, rocks and the like with a collider around the player.
    pub plant_colliders: usize,
}

impl Land {
    /// Loads a `WorldDesc` `.ron`, its map and the ground's pictures.
    pub fn load(path: &Path) -> Result<Self> {
        let map = MacroMap::load(path)?;
        let ground = map.desc.ground.clone();
        let picture = |file: &Path| {
            let path = map.dir.join(file);
            match load_texture(&path) {
                Ok(texture) => Material { base_color_texture: Some(texture), pixelated: true, ..Default::default() },
                Err(e) => {
                    log::error!("Ground picture {}: {e:#}", path.display());
                    Material::default()
                }
            }
        };
        let look = TileLook {
            grass: picture(&ground.grass),
            sand: picture(&ground.sand),
            rock: picture(&ground.rock),
            bed: picture(ground.bed.as_ref().unwrap_or(&ground.sand)),
            mud: picture(ground.mud.as_ref().or(ground.bed.as_ref()).unwrap_or(&ground.sand)),
            tile: ground.tile,
        };
        let view = map.desc.view;
        let fresh = map.desc.fresh_water.clone();
        let water_picture = |file: &Option<std::path::PathBuf>| {
            let path = map.dir.join(file.as_ref()?);
            load_texture(&path).map_err(|e| log::error!("Water picture {}: {e:#}", path.display())).ok()
        };
        let effect_file = |path: &Path| {
            grimora_scene::load_effect(path).map_err(|e| log::error!("Fall effect {}: {e:#}", path.display())).ok()
        };
        let fresh_water = FreshWater {
            texture: water_picture(&fresh.texture),
            foam: water_picture(&fresh.foam_texture),
            fall_effect: fresh.fall_effect.as_ref().and_then(|file| effect_file(&map.dir.join(file))),
            fall_veil: fresh.fall_veil.as_ref().and_then(|file| effect_file(&map.dir.join(file))),
            fall_sound: fresh.fall_sound.as_ref().map(|file| map.dir.join(file)),
            look: fresh,
        };
        let mut fresh_material = grimora_scene::water_material(
            &fresh_water.look.stream,
            fresh_water.texture.clone(),
            fresh_water.foam.clone(),
            fresh_water.look.tile,
        );
        // A flow map: each vertex says which way the water goes.
        fresh_material.water_river = 2.0;
        let (life, life_dir) = match &map.desc.life {
            Some(file) => {
                let path = map.dir.join(file);
                let book = LifeBook::load(&path).unwrap_or_else(|e| {
                    log::error!("{e:#}");
                    LifeBook::default()
                });
                (book, path.parent().map(Path::to_path_buf).unwrap_or_default())
            }
            None => (LifeBook::default(), map.dir.clone()),
        };
        let relief = Relief::new(map);
        Ok(Self {
            life: Arc::new(life),
            life_dir,
            village: None,
            folk: Default::default(),
            flora: Flora::default(),
            fresh_courses: Arc::new(grimora_world::fresh_courses(&relief)),
            fresh_material,
            relief: Arc::new(relief),
            look,
            view,
            shown: HashMap::new(),
            spare: Vec::new(),
            colliders: HashMap::new(),
            fresh_water,
            bed_at: None,
            bed_job: None,
            ready: HashMap::new(),
            workers: None,
            blocking: true,
        })
    }

    /// Whether [`World::update_land`] builds everything before it returns (the default, for
    /// tests and captures) or on other threads, showing what is ready: the runner's way.
    pub fn set_blocking(&mut self, blocking: bool) {
        self.blocking = blocking;
    }

    pub fn relief(&self) -> &Relief {
        &self.relief
    }

    /// How far the land is drawn, in meters.
    pub fn view(&self) -> f64 {
        self.view
    }

    /// The ground's height under a world position.
    pub fn height_at(&self, at: WorldPos) -> f64 {
        self.relief.height_at(at.x, at.z)
    }

    /// The point on the ground at a map point (x east, y south), or at the world's start.
    pub fn start(&self, at: Option<DVec2>) -> WorldPos {
        let mut p = map_to_world(at.unwrap_or(self.relief.map().desc.start));
        p.y = self.height_at(p);
        p
    }

    /// What a step sounds like at a world position, from the same height and slope that
    /// pick the tile's picture.
    pub fn ground_at(&self, at: WorldPos) -> Ground {
        grimora_world::tile_ground(&self.relief, at.x, at.z)
    }

    /// What lives here: the world's life file.
    /// What the solid plant whose collider is `handle` is underfoot, if it is one.
    pub(crate) fn plant_ground(&self, handle: ColliderHandle) -> Option<Ground> {
        let (entity, _) = self.flora.colliders.iter().find(|(_, h)| **h == handle)?;
        let solid = self.flora.solids.get(entity)?;
        self.flora.grounds.get(solid.plant).copied()
    }

    /// Lets go of a plant the world is despawning: its collider, and its place in its cell.
    pub(crate) fn forget(&mut self, entity: Entity, physics: &mut Physics) {
        let flora = &mut self.flora;
        flora.solids.remove(&entity);
        if let Some(handle) = flora.colliders.remove(&entity) {
            physics.remove(handle);
        }
        for entities in flora.shown.values_mut() {
            entities.retain(|&e| e != entity);
        }
    }

    pub fn life(&self) -> &LifeBook {
        &self.life
    }

    pub fn stats(&self) -> LandStats {
        LandStats {
            tiles: self.shown.len(),
            built: 0,
            colliders: self.colliders.len(),
            flora_cells: self.flora.shown.len(),
            plants: self.flora.shown.values().map(Vec::len).sum(),
            plant_colliders: self.flora.colliders.len(),
        }
    }
}

impl World {
    pub fn ground(&self) -> Option<&Land> {
        self.land.as_ref()
    }

    /// Makes `land` the ground; [`World::update_land`] builds it. Its streams and ponds are
    /// all made now, drawn and in [`World::waters`], around the world's origin as it is: set
    /// that first.
    pub fn set_land(&mut self, mut land: Land) {
        self.wind.set_seed(grimora_world::path_seed(&[land.relief.seed(), 0x77696e64]));
        self.add_fresh_water(&land);
        self.add_plant_meshes(&mut land);
        self.add_land_fauna(&land);
        self.add_village(&mut land);
        self.land = Some(land);
    }

    /// The village: a small standing stone on the largo's edge, its houses on their lots, the well,
    /// the fenced field and the boat on the stream's bank.
    /// Why it is there goes to the log.
    fn add_village(&mut self, land: &mut Land) {
        let Some(village) = land.relief.village() else {
            log::warn!("Village: no place fits");
            return;
        };
        log::info!(
            "Village {:016x} at x {:.0}, z {:.0} (--at {:.2},{:.2}): {}",
            village.id.0,
            village.center.x,
            village.center.y,
            village.center.x / grimora_world::METERS_PER_UNIT,
            village.center.y / grimora_world::METERS_PER_UNIT,
            village.reason
        );
        let origin = self.origin();
        let stone = land.life_dir.join("../textures/parede_de_pedra_umida.png");
        // A small standing stone on the largo's edge, a landmark.
        let mut mesh = grimora_scene::MeshDesc::Pillar { seed: village.id.0, broken: false }.build_in(&land.life_dir);
        grimora_scene::wrap_in_texture(&mut mesh, &stone, 1.3);
        let id = self.add_mesh(mesh);
        let mut transform = Transform::from_position(origin.to_local(DVec3::new(
            village.layout.marker.x,
            village.level - 0.2,
            village.layout.marker.y,
        )));
        transform.scale = Vec3::new(0.9, 2.4, 0.9);
        self.ecs.spawn((
            transform,
            Renderable { color: Vec3::splat(2.0), ..Renderable::new(id) },
            Name("village".into()),
        ));
        // The houses on their lots, each its trade's plan; the rest of the village with them.
        let models = land.life_dir.join("../models/objects");
        let textures = land.life_dir.join("../textures");
        let layout = &village.layout;
        log::info!(
            "Largo at --at {:.4},{:.4}; {} houses placed, field {}",
            layout.largo.center.x / grimora_world::METERS_PER_UNIT,
            layout.largo.center.y / grimora_world::METERS_PER_UNIT,
            layout.lots.len(),
            if layout.field.is_some() { "placed" } else { "missing" }
        );
        let mut houses = Vec::new();
        for lot in &layout.lots {
            let house = grimora_scene::build_house(&lot.desc, &textures, &models);
            log::info!(
                "House {} {:016x}: {} rooms, {} beds, door at --at {:.4},{:.4}",
                lot.trade.name(),
                lot.id.0,
                lot.desc.rooms.len(),
                house.beds.len(),
                lot.door.x / grimora_world::METERS_PER_UNIT,
                lot.door.y / grimora_world::METERS_PER_UNIT
            );
            let at = DVec3::new(lot.center.x, village.level, lot.center.y);
            houses.push((house, at, lot.yaw as f32, format!("house:{:016x}", lot.id.0)));
        }
        let built: Vec<crate::folk::Built> = layout
            .lots
            .iter()
            .zip(&houses)
            .map(|(lot, (house, at, _, _))| crate::folk::Built { lot, house, at: *at })
            .collect();
        let graves = layout
            .extras
            .iter()
            .find(|e| e.kind == grimora_world::ExtraKind::Cemetery)
            .map(|e| graves_of(e, village.id.0))
            .unwrap_or_default();
        let pen = layout.extras.iter().find(|e| e.kind == grimora_world::ExtraKind::Pen).map(|e| e.center);
        land.folk.add_village(
            &built,
            layout.largo.center,
            layout.well,
            layout.field.map(|f| f.center),
            layout.boat.map(|(at, _)| at),
            &graves,
            pen,
            &land.relief,
            &land.life_dir,
        );
        drop(built);
        let ground = |at: glam::DVec2| DVec3::new(at.x, land.relief.height_at(at.x, at.y), at.y);
        let mut things = vec![(models.join("Poco-de-Pedra.glb"), ground(layout.well), 0.3, true)];
        let mut k = village.id.0 as usize;
        // Fences round a fenced place, the five kinds in turn, a gap on its -Z side (or +Z).
        let mut fence_round = |things: &mut Vec<(std::path::PathBuf, DVec3, f64, bool)>,
                               center: glam::DVec2,
                               h: glam::DVec2,
                               yaw: f64,
                               gate_back: bool| {
            let fences = [
                "Cerca-Falha-Central",
                "Cerca-Miolo-Baixo",
                "Cerca-Quebrada",
                "Cerca-Tabuas-Inclinadas",
                "Cerca-Travessa-Partida",
            ];
            let sides = [
                (glam::DVec2::new(-h.x, -h.y), glam::DVec2::new(h.x, -h.y), !gate_back),
                (glam::DVec2::new(-h.x, h.y), glam::DVec2::new(h.x, h.y), gate_back),
                (glam::DVec2::new(-h.x, -h.y), glam::DVec2::new(-h.x, h.y), false),
                (glam::DVec2::new(h.x, -h.y), glam::DVec2::new(h.x, h.y), false),
            ];
            for (a, b, gate) in sides {
                let n = (a.distance(b) / FENCE_LENGTH).ceil().max(1.0) as usize;
                let along = (b - a) / n as f64;
                let side_yaw = yaw + (-along.y).atan2(along.x);
                for i in 0..n {
                    if gate && i == n / 2 {
                        continue;
                    }
                    let local = a + along * (i as f64 + 0.5);
                    let at = center + grimora_world::turn(local, yaw);
                    k = k.wrapping_add(1);
                    things.push((models.join(format!("{}.glb", fences[k % fences.len()])), ground(at), side_yaw, true));
                }
            }
        };
        if let Some(field) = layout.field {
            fence_round(&mut things, field.center, field.half, field.yaw, false);
            self.add_crops(&land.life_dir, field.center, field.half, field.yaw, village.id.0, &ground, &origin);
        }
        for extra in &layout.extras {
            match extra.kind {
                grimora_world::ExtraKind::Pen => {
                    // The gate on the far side from the house, out to the pasture.
                    fence_round(&mut things, extra.center, extra.half, extra.yaw, false);
                    let n = (2.0 * extra.half.x / FENCE_LENGTH).ceil().max(1.0);
                    let gate_x = -extra.half.x + 2.0 * extra.half.x / n * ((n as usize / 2) as f64 + 0.5);
                    let pen = crate::fauna::PenPlace {
                        center: origin.to_local(ground(extra.center)),
                        half: glam::Vec2::new(extra.half.x as f32, extra.half.y as f32),
                        yaw: extra.yaw as f32,
                        gate_x: gate_x as f32,
                    };
                    self.add_herd(&land.life_dir.join("../fauna/bovino.ron"), pen, village.id.0 ^ 0xCA77_1E);
                    // A trough of a hollow log and a barrel of water by it.
                    let trough =
                        extra.center + grimora_world::turn(glam::DVec2::new(-extra.half.x + 1.2, 0.0), extra.yaw);
                    things.push((
                        models.join("Tronco-Oco.glb"),
                        ground(trough),
                        extra.yaw + std::f64::consts::FRAC_PI_2,
                        true,
                    ));
                    let barrel = extra.center
                        + grimora_world::turn(glam::DVec2::new(extra.half.x - 0.8, extra.half.y - 0.8), extra.yaw);
                    things.push((models.join("Barril.glb"), ground(barrel), extra.yaw, true));
                }
                grimora_world::ExtraKind::Cemetery => {
                    fence_round(&mut things, extra.center, extra.half, extra.yaw, false);
                    // A dead tree in a back corner, over the graves.
                    let corner = extra.center
                        + grimora_world::turn(glam::DVec2::new(extra.half.x - 1.0, extra.half.y - 0.9), extra.yaw);
                    let tree = land.life_dir.join("../models/savana/Arvore-Morta-01-Galhada.glb");
                    things.push((tree, ground(corner), extra.yaw * 3.1, true));
                    let cross =
                        extra.center + grimora_world::turn(glam::DVec2::new(0.0, extra.half.y - 1.0), extra.yaw);
                    self.add_graves(&land.life_dir, &graves, (cross, extra.yaw), village.id.0, &ground, &origin);
                }
            }
            log::info!(
                "{} at --at {:.4},{:.4}",
                extra.name(),
                extra.center.x / grimora_world::METERS_PER_UNIT,
                extra.center.y / grimora_world::METERS_PER_UNIT
            );
        }
        if let Some((at, yaw)) = layout.boat {
            things.push((land.life_dir.join("../models/boats/Bote.glb"), ground(at), yaw, true));
        }
        land.village = Some((houses, things));
    }

    /// Rows of crops in the field: maize and cassava in turn, a path down the middle.
    #[allow(clippy::too_many_arguments)]
    fn add_crops(
        &mut self,
        life_dir: &std::path::Path,
        center: glam::DVec2,
        half: glam::DVec2,
        yaw: f64,
        seed: u64,
        ground: &dyn Fn(glam::DVec2) -> DVec3,
        origin: &grimora_core::Origin,
    ) {
        let kinds = [
            ("../textures/capim_espigas.png", 1.5, Vec3::new(0.95, 1.1, 0.55)),
            ("../textures/mato2.png", 0.9, Vec3::new(0.7, 1.05, 0.6)),
        ];
        let meshes: Vec<_> = kinds
            .iter()
            .filter_map(|(texture, size, _)| {
                let desc: grimora_scene::MeshDesc =
                    ron::from_str(&format!("Sprite(texture: \"{texture}\", size: {size})")).ok()?;
                Some(self.add_mesh(desc.build_in(life_dir)))
            })
            .collect();
        if meshes.len() < kinds.len() {
            return;
        }
        let mut rows = 0;
        let mut z = -half.y + 1.0;
        while z < half.y - 0.8 {
            let (mesh, (_, _, tint)) = (meshes[rows % 2], kinds[rows % 2]);
            let mut x = -half.x + 0.8;
            let mut n = 0u64;
            while x < half.x - 0.8 {
                if x.abs() > 0.7 {
                    let u = |k: u64| {
                        (grimora_world::path_seed(&[seed, 0xc209, rows as u64, n, k]) >> 11) as f64
                            / (1u64 << 53) as f64
                    };
                    let local = glam::DVec2::new(x + (u(0) - 0.5) * 0.2, z + (u(1) - 0.5) * 0.15);
                    let at = ground(center + grimora_world::turn(local, yaw));
                    let mut transform = Transform::from_position(origin.to_local(at));
                    transform.rotation = glam::Quat::from_rotation_y((u(2) * std::f64::consts::TAU) as f32);
                    transform.scale = Vec3::splat(0.75 + u(3) as f32 * 0.5);
                    self.ecs.spawn((
                        transform,
                        Renderable { color: tint * (0.85 + u(4) as f32 * 0.3), ..Renderable::new(mesh) },
                        Name("village:roca".into()),
                    ));
                }
                x += 0.55;
                n += 1;
            }
            z += 1.1;
            rows += 1;
        }
    }

    /// The graves: a mound of earth each, with a wooden cross or a mossy stone at its head.
    #[allow(clippy::too_many_arguments)]
    fn add_graves(
        &mut self,
        life_dir: &std::path::Path,
        graves: &[(glam::DVec2, f64)],
        monument: (glam::DVec2, f64),
        seed: u64,
        ground: &dyn Fn(glam::DVec2) -> DVec3,
        origin: &grimora_core::Origin,
    ) {
        let textures = life_dir.join("../textures");
        let dressed = |mut mesh: grimora_core::MeshData, file: &str, tile: f32| {
            grimora_scene::wrap_in_texture(&mut mesh, &textures.join(file), tile);
            mesh
        };
        let mound = self.add_mesh(dressed(
            grimora_scene::cube(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.8, 0.3, 1.9), Vec3::ONE),
            "parede_de_barro.png",
            1.0,
        ));
        let mut cross = grimora_scene::cube(Vec3::new(0.0, 0.5, 0.0), Vec3::new(0.08, 1.0, 0.07), Vec3::ONE);
        cross.append(
            &grimora_scene::cube(Vec3::new(0.0, 0.74, 0.0), Vec3::new(0.48, 0.08, 0.07), Vec3::ONE),
            Vec3::ZERO,
            Vec3::ONE,
        );
        let cross = self.add_mesh(dressed(cross, "madeira_apodrecida.png", 0.6));
        let stone = self.add_mesh(dressed(
            grimora_scene::cube(Vec3::new(0.0, 0.32, 0.0), Vec3::new(0.5, 0.64, 0.13), Vec3::ONE),
            "pedra_coberta_de_musgo.png",
            0.7,
        ));
        let n_graves = graves.len();
        for (n, &(at, yaw)) in graves.iter().enumerate() {
            let u =
                |k: u64| (grimora_world::path_seed(&[seed, 0x9a4e, n as u64, k]) >> 11) as f32 / (1u64 << 53) as f32;
            let turn = glam::Quat::from_rotation_y(yaw as f32);
            let toward = glam::DVec2::new(yaw.sin(), yaw.cos());
            // The first in the list are the front row: two of them are fresh, high, bare earth
            // with a new cross; the old ones sink, go green and get a stone.
            let fresh = n < 2 && n_graves > 6;
            let old = !fresh && u(6) < 0.55;
            let mut t = Transform::from_position(origin.to_local(ground(at)));
            t.rotation = turn * glam::Quat::from_rotation_y((u(0) - 0.5) * 0.04);
            t.scale =
                if fresh { Vec3::new(1.0, 1.5, 1.0) } else { Vec3::new(1.0, 0.5 + u(1) * 0.4, 0.92 + u(2) * 0.1) };
            let color = if fresh {
                Vec3::new(0.52, 0.42, 0.36)
            } else if old {
                Vec3::new(0.6, 0.68, 0.52)
            } else {
                Vec3::splat(0.75)
            };
            self.ecs.spawn((t, Renderable { color, ..Renderable::new(mound) }, Name("village:cova".into())));
            let head = ground(at + toward * 1.05);
            let mut t = Transform::from_position(origin.to_local(head) - Vec3::Y * 0.05);
            let tilt = if old {
                0.3
            } else if fresh {
                0.04
            } else {
                0.14
            };
            let lean =
                glam::Quat::from_rotation_x((u(3) - 0.5) * tilt) * glam::Quat::from_rotation_z((u(4) - 0.5) * tilt);
            t.rotation = turn * lean;
            let mesh = if old && u(5) < 0.8 { stone } else { cross };
            self.ecs.spawn((t, Renderable::new(mesh), Name("village:lapide".into())));
        }
        // The great cross at the end of the path, on a stone plinth.
        let (at, yaw) = monument;
        let plinth = self.add_mesh(dressed(
            grimora_scene::cube(Vec3::new(0.0, 0.2, 0.0), Vec3::new(1.1, 0.4, 1.1), Vec3::ONE),
            "pedra_coberta_de_musgo.png",
            0.8,
        ));
        let mut big = grimora_scene::cube(Vec3::new(0.0, 1.5, 0.0), Vec3::new(0.18, 2.4, 0.16), Vec3::ONE);
        big.append(
            &grimora_scene::cube(Vec3::new(0.0, 2.05, 0.0), Vec3::new(1.15, 0.16, 0.16), Vec3::ONE),
            Vec3::ZERO,
            Vec3::ONE,
        );
        let big = self.add_mesh(dressed(big, "madeira_apodrecida.png", 0.8));
        let mut t = Transform::from_position(origin.to_local(ground(at)) - Vec3::Y * 0.05);
        t.rotation = glam::Quat::from_rotation_y(yaw as f32);
        self.ecs.spawn((t, Renderable::new(plinth), Name("village:cruzeiro".into())));
        self.ecs.spawn((t, Renderable::new(big), Name("village:cruzeiro".into())));
    }

    /// Makes each plant's variants once; every copy shares them.
    fn add_plant_meshes(&mut self, land: &mut Land) {
        let life = land.life.clone();
        let mut shapes = Vec::new();
        let mut grounds = Vec::new();
        land.flora.meshes = life
            .plants
            .iter()
            .enumerate()
            .map(|(i, plant)| {
                let mut plant_shapes = Vec::new();
                let meshes = (0..plant.variants.max(1))
                    .map(|v| {
                        let base = match plant.meshes.len() {
                            0 => &plant.mesh,
                            n => &plant.meshes[v as usize % n],
                        };
                        let desc = base.with_seed(grimora_world::path_seed(&[land.relief.seed(), i as u64, v as u64]));
                        let mut mesh = desc.build_in(&land.life_dir);
                        if let Some(texture) = &plant.texture {
                            grimora_scene::wrap_in_texture(&mut mesh, &land.life_dir.join(&texture.path), texture.tile);
                        }
                        // Down there a shadow is hardly seen, and each one is drawn again for
                        // every cascade it falls in.
                        if plant.depth.is_some() {
                            mesh.shadowless = true;
                            // Whatever wind its model asks for is the air's: none down there.
                            mesh.wind = 0.0;
                        }
                        if plant.sway > 0.0 {
                            // `CURRENT_SWAY` in scene.wgsl marks what the sea moves.
                            mesh.wind = plant.sway + if plant.depth.is_some() { 50.0 } else { 0.0 };
                        }
                        if v == 0 {
                            let guess = Ground::of_mesh(&mesh).or_else(|| Ground::from_name(&plant.name));
                            grounds.push(guess.unwrap_or(Ground::Wood));
                        }
                        plant_shapes.push(plant_shape(&mesh, plant.solid));
                        fade_by_reach(&mut mesh, plant.reach);
                        self.add_mesh(mesh)
                    })
                    .collect();
                shapes.push(plant_shapes);
                meshes
            })
            .collect();
        land.flora.shapes = shapes;
        land.flora.grounds = grounds;
    }

    /// The streams' falls and the bodies in [`World::waters`]; their surface is drawn by the
    /// tiles, each over itself (`grimora_world::fresh_tile`).
    fn add_fresh_water(&mut self, land: &Land) {
        let origin = self.origin();
        let water = &land.fresh_water;
        let look = &water.look;
        let mut bodies = Vec::new();
        for (s, stream) in land.relief.water().streams.iter().enumerate() {
            let drawn = &stream.points[land.relief.drawn_course(s)];
            if drawn.len() < 2 {
                continue;
            }
            let desc = RiverDesc { speed: stream.speed as f32, ..Default::default() };
            let points: Vec<RiverPoint> = drawn
                .iter()
                .map(|p| RiverPoint { position: origin.to_local(p.at), half_width: p.half_width as f32 })
                .collect();
            let level = points[0].position.y;
            let shape = WaterShape::river(points, desc.speed);
            bodies.push(WaterBody { shape, level, ripples: look.waves, look: look.stream });
        }
        self.add_falls(land);
        for pond in &land.relief.water().ponds {
            let reach = pond.reach() as f32;
            let center = origin.to_local(DVec3::new(pond.center.x, pond.level, pond.center.y));
            // A square sheet: past its shore, the ground stands over the water.
            let shape = WaterShape::Sheet { center: Vec2::new(center.x, center.z), half: Vec2::splat(reach) };
            bodies.push(WaterBody { shape, level: center.y, ripples: look.waves, look: look.stream });
        }
        let at = self.streamed_waters.min(self.waters.bodies.len());
        self.waters.bodies.splice(at..at, bodies);
        let streams = &land.relief.water().streams;
        let points = streams.iter().flat_map(|s| &s.points);
        let (mut widths, mut depths): (Vec<f64>, Vec<f64>) = points.map(|p| (p.half_width * 2.0, p.depth)).unzip();
        widths.sort_by(f64::total_cmp);
        depths.sort_by(f64::total_cmp);
        let spread = |v: &[f64]| match v.len() {
            0 => String::from("-"),
            n => format!("{:.1} / {:.1} / {:.1} / {:.1}", v[0], v[n / 10], v[n / 2], v[n - 1 - n / 50]),
        };
        log::info!(
            "Streams: {}; width m (least / 10% / middle / 98%): {}; depth m: {}",
            streams.len(),
            spread(&widths),
            spread(&depths)
        );
    }

    /// Every fall's curtain, and the mist, spray and roar where it lands.
    fn add_falls(&mut self, land: &Land) {
        let origin = self.origin();
        let water = &land.fresh_water;
        for (s, curtain) in land.fresh_courses.curtains() {
            let lip = origin.to_local(curtain.lip);
            let landing = origin.to_local(curtain.landing);
            let map = grimora_world::world_to_map(curtain.landing);
            log::info!(
                "Fall of {:.1} m on stream {s}, {:.1} m wide, {} rows, lip {:.1},{:.1},{:.1} lands {:.1},{:.1},{:.1} (--at {:.2},{:.2})",
                curtain.height,
                curtain.half_width * 2.0,
                curtain.rows(),
                lip.x,
                lip.y,
                lip.z,
                landing.x,
                landing.y,
                landing.z,
                map.x,
                map.y
            );
            let mut mesh = curtain.mesh(curtain.lip);
            mesh.materials = vec![land.fresh_material.clone()];
            let id = self.add_mesh(mesh);
            self.ecs.spawn((Transform::from_position(lip), Renderable::new(id), Name("waterfall".into())));
            let scale = grimora_scene::fall_effect_scale(curtain.half_width as f32);
            let mut play = |effect: &grimora_core::ParticleEffect, at: Vec3| {
                let scene = grimora_scene::SceneEffect { effect: effect.clone(), offset: Vec3::ZERO, scale };
                let transform = Transform::from_position(at);
                self.ecs.spawn((transform, scene.system(&transform)));
            };
            if let Some(effect) = &water.fall_effect {
                play(effect, landing);
            }
            // Spray off the sheet, one emitter every few meters down it, not on the lip.
            if let Some(effect) = &water.fall_veil {
                let count = (curtain.height as f32 / FALL_VEIL_EVERY).round().max(1.0) as usize;
                for k in 0..count {
                    let down = (k as f64 + 0.6) / (count as f64 + 0.4);
                    play(effect, origin.to_local(curtain.face_at(down)));
                }
            }
            if let Some(path) = &water.fall_sound {
                let (wide, high) = (curtain.half_width as f32, curtain.height as f32);
                self.sounds.push(grimora_scene::SceneSound {
                    path: path.clone(),
                    position: Some(landing),
                    volume: 1.0,
                    min_distance: wide + high,
                    max_distance: 50.0 + high * 4.0,
                });
            }
        }
    }

    /// The life file's ambience loops and how loud each should be at `camera` (local): by the
    /// biome under it, the hour, how far inland it is and how high above the ground.
    pub(crate) fn ambience(&self, camera: Vec3) -> Vec<(&Path, f32)> {
        let Some(land) = &self.land else { return Vec::new() };
        let at = self.origin().to_world(camera);
        let water = land.relief.water();
        let coast = water.coast_distance(at.x, at.z);
        let ground = land.height_at(at).max(0.0);
        let aloft = 1.0 - ((at.y - ground) as f32 / AMBIENCE_ALOFT).clamp(0.0, 1.0);
        let biome = land.relief.map().sample(at).biome.map(|b| b.id.clone()).unwrap_or_default();
        let daylight = self.day_cycle.as_ref().map_or(1.0, |day| day.daylight());
        land.life
            .ambience
            .iter()
            .map(|desc| {
                let here = if desc.biomes.is_empty() { coast > 0.0 } else { desc.biomes.contains(&biome) };
                let hour = match desc.when {
                    grimora_world::When::Always => 1.0,
                    grimora_world::When::Day => daylight,
                    grimora_world::When::Night => 1.0 - daylight,
                };
                let inland = if desc.inland > 0.0 { (coast / desc.inland).clamp(0.0, 1.0) as f32 } else { 1.0 };
                let volume = if here { desc.volume * hour * inland * aloft } else { 0.0 };
                (desc.sound.as_path(), volume)
            })
            .collect()
    }

    /// Puts the life file's animals where they fit: the sea's along the coast, fish in the deep
    /// ponds. Their models join the world's meshes.
    fn add_land_fauna(&mut self, land: &Land) {
        let origin = self.origin();
        let places = grimora_world::fauna_places(&land.relief, &land.life);
        let descs: Vec<grimora_scene::FaunaDesc> = places
            .iter()
            .map(|place| grimora_scene::FaunaDesc {
                species: place.species.to_string_lossy().into_owned(),
                position: origin.to_local(place.at),
                radius: place.radius,
                count: place.count,
                seed: place.seed,
            })
            .collect();
        let mut meshes = Vec::new();
        let mut groups = grimora_scene::load_fauna(&descs, Path::new(""), &mut meshes);
        let ids: Vec<MeshId> = meshes.into_iter().map(|mesh| self.add_mesh(mesh)).collect();
        for group in &mut groups {
            group.mesh = ids[group.mesh].0;
        }
        log::info!("Land fauna: {} groups, {} animals", groups.len(), groups.iter().map(|g| g.count).sum::<u32>());
        self.fauna.add(groups, &self.meshes);
    }

    /// Shows the tiles `eye` (local) should see and puts colliders around `focus` (local),
    /// building what is missing on every core. Call each frame; the runner does.
    pub fn update_land(&mut self, eye: Vec3, focus: Vec3, physics: &mut Physics) -> LandStats {
        let Some(mut land) = self.land.take() else {
            return LandStats::default();
        };
        let origin = self.origin();
        if let Some((houses, things)) = land.village.take() {
            let effects = land.life_dir.join("../effects");
            for (house, at, yaw, name) in houses {
                let at = Transform {
                    position: origin.to_local(at),
                    rotation: glam::Quat::from_rotation_y(yaw),
                    scale: Vec3::ONE,
                };
                self.add_house(&house, &at, &name, &effects, physics);
            }
            for (path, at, yaw, solid) in things {
                let at = Transform {
                    position: origin.to_local(at),
                    rotation: glam::Quat::from_rotation_y(yaw as f32),
                    scale: Vec3::ONE,
                };
                self.add_model(&path, &at, solid, physics);
            }
        }
        let eye_world = origin.to_world(eye);
        let above = eye_world.y - land.height_at(eye_world).max(0.0);
        self.update_sea_bed(&mut land, DVec2::new(eye_world.x, eye_world.z));
        let wanted = tiles_around(eye_world, above, land.view);
        let focus = origin.to_world(focus);
        let under = TileKey::at(DVec2::new(focus.x, focus.z));
        let ring = |reach: i64| {
            let mut keys = Vec::new();
            for dz in -reach..=reach {
                for dx in -reach..=reach {
                    keys.push(TileKey { level: 0, x: under.x + dx, z: under.z + dz });
                }
            }
            keys
        };
        let solid = ring(COLLIDER_REACH);
        // The player's own tiles can't wait; the rest come from the workers.
        let underfoot: HashSet<TileKey> = ring(1).into_iter().collect();

        // What came back from the workers.
        let mut added = false;
        let done = land.workers.as_ref().map(Workers::take).unwrap_or_default();
        let mut plants = Vec::new();
        for done in done {
            match done {
                Done::Tile(key, built) => {
                    land.ready.insert(key, built);
                }
                Done::Collider(key, vertices, indices) => {
                    let near =
                        (key.x - under.x).abs() <= COLLIDER_REACH + 1 && (key.z - under.z).abs() <= COLLIDER_REACH + 1;
                    if near && !land.colliders.contains_key(&key) {
                        added |= add_tile_collider(&mut land, physics, origin, key, vertices, indices);
                    }
                }
                Done::Plants(key, spots) => plants.push((key, spots)),
            }
        }

        // Tiles: all the view wants are shown at once, so coarse and fine never leave a gap.
        let mut missing: Vec<TileKey> = wanted.iter().copied().filter(|k| !land.shown.contains_key(k)).collect();
        let jumped = !wanted.iter().any(|k| land.shown.contains_key(k));
        let unbuilt: Vec<TileKey> = missing.iter().copied().filter(|k| !land.ready.contains_key(k)).collect();
        if land.blocking || jumped {
            let sea = land.fresh_water.look.sea;
            let built = build_all(&land.relief, &land.look, &land.fresh_courses, sea.as_ref(), &unbuilt);
            for (key, built) in unbuilt.iter().zip(built) {
                land.ready.insert(*key, built);
            }
        }
        let built = missing.len();
        if missing.iter().all(|k| land.ready.contains_key(k)) {
            let keep: HashSet<TileKey> = wanted.iter().copied().collect();
            land.shown.retain(|key, shown| {
                if keep.contains(key) {
                    return true;
                }
                for (entity, mesh) in std::iter::once(shown.ground).chain(shown.water) {
                    let _ = self.ecs.despawn(entity);
                    land.spare.push(mesh);
                }
                false
            });
            for key in missing.drain(..) {
                let Some((ground, water)) = land.ready.remove(&key) else { continue };
                let center = key.center();
                let position = origin.to_local(DVec3::new(center.x, 0.0, center.y));
                let ground = self.spawn_tile_mesh(&mut land.spare, ground, position, "terrain");
                let water = water.map(|mut mesh| {
                    mesh.materials = vec![land.fresh_material.clone()];
                    mesh.wind = land.fresh_water.look.waves.max(0.0);
                    self.spawn_tile_mesh(&mut land.spare, mesh, position, "water")
                });
                land.shown.insert(key, Shown { ground, water });
            }
        }
        if land.ready.len() > MOST_READY {
            let keep: HashSet<TileKey> = wanted.iter().copied().collect();
            land.ready.retain(|key, _| keep.contains(key));
        }

        // Colliders: 5 × 5 tiles, kept one tile further before they go, so walking along an
        // edge doesn't make and drop them over and over.
        land.colliders.retain(|key, handle| {
            let stays = (key.x - under.x).abs() <= COLLIDER_REACH + 1 && (key.z - under.z).abs() <= COLLIDER_REACH + 1;
            if !stays {
                physics.remove(*handle);
            }
            stays
        });
        let mut later = Vec::new();
        for key in solid {
            if land.colliders.contains_key(&key) {
                continue;
            }
            if land.blocking || underfoot.contains(&key) {
                let (vertices, indices) = tile_collider(&land.relief, key);
                added |= add_tile_collider(&mut land, physics, origin, key, vertices, indices);
            } else {
                later.push(Job::Collider(key));
            }
        }
        if added {
            physics.refresh();
        }
        self.update_flora(&mut land, eye_world, plants, physics);
        update_plant_colliders(&mut land.flora, origin, DVec2::new(focus.x, focus.z), physics);

        // What is still missing goes to the workers, nearest first.
        if !land.blocking {
            let at = DVec2::new(eye_world.x, eye_world.z);
            let mut tiles: Vec<TileKey> = missing.into_iter().filter(|k| !land.ready.contains_key(k)).collect();
            tiles.sort_by(|a, b| a.level.cmp(&b.level).then(a.distance(at).total_cmp(&b.distance(at))));
            let mut jobs = later;
            jobs.extend(tiles.into_iter().map(Job::Tile));
            let mut cells: Vec<FloraKey> =
                land.flora.wanted.iter().copied().filter(|k| !land.flora.shown.contains_key(k)).collect();
            cells.sort();
            jobs.extend(cells.into_iter().map(Job::Plants));
            let workers = land.workers.get_or_insert_with(|| {
                Workers::new(Arc::new(Makers {
                    relief: land.relief.clone(),
                    look: land.look.clone(),
                    life: land.life.clone(),
                    courses: land.fresh_courses.clone(),
                    sea: land.fresh_water.look.sea,
                }))
            });
            workers.ask(jobs);
        }
        // After the ground under the player has its colliders: the villagers stand on it.
        self.update_folk(&mut land, focus, physics, 1.0 / 60.0);
        self.update_cattle_bodies(physics, 1.0 / 60.0);
        let stats = LandStats { built, ..land.stats() };
        self.land = Some(land);
        stats
    }

    /// Keeps the map of the sea's depth around `eye` (world), which the waves shoal and break
    /// on: made again on another thread once the eye has gone [`BED_MOVE`], the old one kept
    /// until then. The first is waited for.
    fn update_sea_bed(&mut self, land: &mut Land, eye: DVec2) {
        let done = land.bed_job.as_ref().is_some_and(|job| job.is_finished() || land.bed_at.is_none());
        if done && let Some(Ok((center, depths))) = land.bed_job.take().map(|job| job.join()) {
            land.bed_at = Some(center);
            let half = BED_SPACING * (BED_SAMPLES - 1) as f64 * 0.5;
            let origin = self.origin();
            let lo = origin.to_local(DVec3::new(center.x - half, 0.0, center.y - half));
            let hi = origin.to_local(DVec3::new(center.x + half, 0.0, center.y + half));
            self.waters.bed = Some(Arc::new(grimora_core::WaterBed {
                min: Vec2::new(lo.x, lo.z),
                max: Vec2::new(hi.x, hi.z),
                width: BED_SAMPLES as u32,
                height: BED_SAMPLES as u32,
                depths,
            }));
        }
        if land.bed_job.is_none() && land.bed_at.is_none_or(|at| at.distance(eye) > BED_MOVE) {
            let center = (eye / BED_SPACING).round() * BED_SPACING;
            let relief = land.relief.clone();
            land.bed_job = Some(std::thread::spawn(move || (center, sea_depths(&relief, center))));
            if land.bed_at.is_none() {
                self.update_sea_bed(land, eye);
            }
        }
    }

    /// Draws a tile's mesh, in a spare mesh slot if there is one.
    fn spawn_tile_mesh(
        &mut self,
        spare: &mut Vec<MeshId>,
        mesh: MeshData,
        position: Vec3,
        name: &str,
    ) -> (Entity, MeshId) {
        let id = match spare.pop() {
            Some(id) => {
                self.replace_mesh(id, mesh);
                id
            }
            None => self.add_mesh(mesh),
        };
        let entity = self.ecs.spawn((Transform::from_position(position), Renderable::new(id), Name(name.into())));
        (entity, id)
    }

    /// Shows the plants' cells within their reach of `eye` and drops the others. `arrived` are
    /// cells the workers placed; the ones still wanted are shown.
    fn update_flora(
        &mut self,
        land: &mut Land,
        eye: WorldPos,
        mut arrived: Vec<(FloraKey, Vec<PlantSpot>)>,
        physics: &mut Physics,
    ) {
        let eye = DVec2::new(eye.x, eye.z);
        let moved = land.flora.eye.is_none_or(|last| last.distance(eye) >= FLORA_MOVE);
        if !land.life.plants.is_empty() && moved {
            land.flora.eye = Some(eye);
            land.flora.wanted = flora_around(&land.life, eye).into_iter().collect();
            let origin = self.origin();
            let keep = &land.flora.wanted;
            land.flora.shown.retain(|key, entities| {
                if keep.contains(key) {
                    return true;
                }
                for entity in entities.drain(..) {
                    if let Ok((transform, generated)) = self.ecs.query_one_mut::<(&Transform, &Generated)>(entity) {
                        let now = origin.transform_to_world(transform);
                        self.generated.note_place(generated.id, &generated.made, now);
                    }
                    let _ = self.ecs.despawn(entity);
                    land.flora.solids.remove(&entity);
                    if let Some(handle) = land.flora.colliders.remove(&entity) {
                        physics.remove(handle);
                    }
                }
                false
            });
            // The first cells, and all of them when blocking, are placed now.
            if land.blocking || land.flora.shown.is_empty() {
                let mut missing: Vec<FloraKey> =
                    land.flora.wanted.iter().copied().filter(|k| !land.flora.shown.contains_key(k)).collect();
                missing.sort();
                let placed = place_all(&land.relief, &land.life, &missing);
                arrived.extend(missing.into_iter().zip(placed));
            }
        }
        arrived.retain(|(key, _)| land.flora.wanted.contains(key) && !land.flora.shown.contains_key(key));
        if arrived.is_empty() {
            return;
        }
        let origin = self.origin();
        let flora = &mut land.flora;
        for (key, spots) in arrived {
            if flora.shown.contains_key(&key) {
                continue;
            }
            let meshes = &flora.meshes[key.plant as usize];
            let shapes = &flora.shapes[key.plant as usize];
            let generated = &self.generated;
            let entities = spots
                .iter()
                .filter(|spot| !generated.is_removed(spot.id))
                .map(|spot: &PlantSpot| {
                    let made =
                        WorldTransform { position: spot.at, rotation: spot.rotation, scale: Vec3::splat(spot.scale) };
                    let placed = generated.moved.get(&spot.id).copied().unwrap_or(made);
                    let transform = origin.transform_to_local(&placed);
                    let variant = spot.variant as usize % meshes.len();
                    let renderable = Renderable { color: spot.color, ..Renderable::new(meshes[variant]) };
                    let entity = self.ecs.spawn((transform, renderable, Generated { id: spot.id, made }));
                    if let Some(shape) = &shapes[variant] {
                        let solid = Solid {
                            plant: key.plant as usize,
                            at: placed.position,
                            rotation: placed.rotation,
                            scale: placed.scale.x,
                            shape: shape.clone(),
                        };
                        flora.solids.insert(entity, solid);
                    }
                    entity
                })
                .collect();
            flora.shown.insert(key, entities);
        }
        flora.focus = None;
    }

    /// Takes every plant away and picks the cells again on the next [`World::update_land`],
    /// so the differences in [`World::generated`] are put on them afresh (a saved game loaded).
    pub fn reset_flora(&mut self, physics: &mut Physics) {
        let Some(land) = &mut self.land else { return };
        let flora = &mut land.flora;
        for (_, entities) in flora.shown.drain() {
            for entity in entities {
                let _ = self.ecs.despawn(entity);
            }
        }
        for (_, handle) in flora.colliders.drain() {
            physics.remove(handle);
        }
        flora.solids.clear();
        flora.wanted.clear();
        flora.eye = None;
        flora.focus = None;
    }

    /// The plants with a collider, by name, and where each stands in local space: for tests
    /// and debugging.
    pub fn plant_colliders(&self) -> Vec<(&str, Vec3)> {
        let Some(land) = &self.land else { return Vec::new() };
        let origin = self.origin();
        let flora = &land.flora;
        flora
            .colliders
            .keys()
            .filter_map(|e| flora.solids.get(e))
            .map(|solid| (land.life.plants[solid.plant].name.as_str(), origin.to_local(solid.at)))
            .collect()
    }
}

/// Puts a level 0 tile's collider in the physics; true if it went in.
fn add_tile_collider(
    land: &mut Land,
    physics: &mut Physics,
    origin: grimora_core::Origin,
    key: TileKey,
    vertices: Vec<Vec3>,
    indices: Vec<[u32; 3]>,
) -> bool {
    let center = key.center();
    let at = Transform::from_position(origin.to_local(DVec3::new(center.x, 0.0, center.y)));
    match physics.add_static(&ColliderShape::TriMesh { vertices, indices }, &at) {
        Some(handle) => {
            land.colliders.insert(key, handle);
            true
        }
        None => false,
    }
}

/// What a plant's variant bumps into: its model's collision nodes, or its hull if `solid`.
fn plant_shape(mesh: &MeshData, solid: bool) -> Option<Arc<ColliderShape>> {
    match &mesh.collision {
        Some(shape) => {
            Some(Arc::new(ColliderShape::TriMesh { vertices: shape.vertices.clone(), indices: shape.indices.clone() }))
        }
        None if solid && !mesh.vertices.is_empty() => Some(Arc::new(ColliderShape::hull(mesh))),
        None => None,
    }
}

/// Gives the solid plants within [`PLANT_COLLIDER_REACH`] of `focus` a static collider and takes
/// it from the ones that went further.
fn update_plant_colliders(flora: &mut Flora, origin: grimora_core::Origin, focus: DVec2, physics: &mut Physics) {
    if flora.focus.is_some_and(|last| last.distance(focus) < PLANT_COLLIDER_MOVE) {
        return;
    }
    flora.focus = Some(focus);
    let near = |solid: &Solid| DVec2::new(solid.at.x, solid.at.z).distance(focus) <= PLANT_COLLIDER_REACH;
    let solids = &flora.solids;
    flora.colliders.retain(|entity, handle| {
        let stays = solids.get(entity).is_some_and(near);
        if !stays {
            physics.remove(*handle);
        }
        stays
    });
    let mut added = false;
    for (entity, solid) in &flora.solids {
        if flora.colliders.contains_key(entity) || !near(solid) {
            continue;
        }
        let transform = Transform {
            position: origin.to_local(solid.at),
            rotation: solid.rotation,
            scale: Vec3::splat(solid.scale),
        };
        if let Some(handle) = physics.add_static(&solid.shape, &transform) {
            flora.colliders.insert(*entity, handle);
            added = true;
        }
    }
    if added {
        physics.refresh();
    }
}

/// A generated world's ground for the animals: its relief, and where the origin is in it.
pub(crate) fn floor(land: &Option<Land>, origin: grimora_core::Origin) -> Option<(&Relief, grimora_core::Origin)> {
    land.as_ref().map(|land| (land.relief(), origin))
}

/// Places plants on every core, like [`build_all`].
fn place_all(relief: &Relief, life: &LifeBook, keys: &[FloraKey]) -> Vec<Vec<PlantSpot>> {
    if keys.len() < 8 {
        return keys.iter().map(|&k| place_plants(relief, life, k)).collect();
    }
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(keys.len());
    let per = keys.len().div_ceil(threads);
    std::thread::scope(|s| {
        let parts: Vec<_> = keys
            .chunks(per)
            .map(|part| s.spawn(move || part.iter().map(|&k| place_plants(relief, life, k)).collect::<Vec<_>>()))
            .collect();
        parts.into_iter().flat_map(|h| h.join().expect("plant placer panicked")).collect()
    })
}

/// How deep the sea is over the ground, before the streams dig it, on the bed's grid around
/// `center` (world): what the waves shoal on. Below 0 on land.
fn sea_depths(relief: &Relief, center: DVec2) -> Vec<f32> {
    let half = BED_SPACING * (BED_SAMPLES - 1) as f64 * 0.5;
    let row = |j: usize| {
        let z = center.y - half + j as f64 * BED_SPACING;
        (0..BED_SAMPLES).map(move |i| -relief.raw_height_at(center.x - half + i as f64 * BED_SPACING, z) as f32)
    };
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).saturating_sub(1).max(1);
    let per = BED_SAMPLES.div_ceil(threads);
    std::thread::scope(|s| {
        let jobs: Vec<_> = (0..BED_SAMPLES)
            .step_by(per)
            .map(|first| s.spawn(move || (first..(first + per).min(BED_SAMPLES)).flat_map(row).collect::<Vec<f32>>()))
            .collect();
        jobs.into_iter().flat_map(|job| job.join().expect("a row of the sea's depth")).collect()
    })
}

/// A tile's ground and the fresh water over it.
fn build_one(relief: &Relief, look: &TileLook, courses: &FreshCourses, sea: Option<&WaterLook>, key: TileKey) -> Built {
    (build_tile(relief, key, look), grimora_world::fresh_tile(relief, courses, sea, key))
}

/// Builds tiles on every core. Each tile depends only on its key, so the split doesn't matter.
fn build_all(
    relief: &Relief,
    look: &TileLook,
    courses: &FreshCourses,
    sea: Option<&WaterLook>,
    keys: &[TileKey],
) -> Vec<Built> {
    let one = |&k: &TileKey| build_one(relief, look, courses, sea, k);
    if keys.len() < 2 {
        return keys.iter().map(one).collect();
    }
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(keys.len());
    let per = keys.len().div_ceil(threads);
    std::thread::scope(|s| {
        let parts: Vec<_> =
            keys.chunks(per).map(|part| s.spawn(move || part.iter().map(one).collect::<Vec<_>>())).collect();
        parts.into_iter().flat_map(|h| h.join().expect("tile builder panicked")).collect()
    })
}

/// Length of one fence piece, meters.
const FENCE_LENGTH: f64 = 2.2;

/// Where each grave of the graveyard `e` lies (world x, z of its middle) and its turn: in rows
/// across it, the head toward the gate, a few places left empty.
/// The graves of a cemetery, as (middle of the mound, turn): rows on both sides of the path
/// up its middle, the heads toward the back (its +Z, where the cross stands), feet to the path
/// side's gate, with the ground by the gate left free and some plots still empty.
pub(crate) fn graves_of(e: &grimora_world::Extra, seed: u64) -> Vec<(glam::DVec2, f64)> {
    let mut graves = Vec::new();
    let mut n = 0u64;
    let back = e.half.y;
    for z in [back - 7.7, back - 4.8, back - 1.9] {
        for x in [-5.3, -4.0, -2.7, -1.4, 1.4, 2.7, 4.0, 5.3] {
            n += 1;
            // The dead tree's corner.
            if x > 4.5 && z > back - 2.5 {
                continue;
            }
            let u = |k: u64| (grimora_world::path_seed(&[seed, 0x6a7e, n, k]) >> 11) as f64 / (1u64 << 53) as f64;
            // The back rows (the oldest, by the cross) are fuller than the front.
            let full = if z > back - 3.0 {
                0.8
            } else if z > back - 6.0 {
                0.65
            } else {
                0.45
            };
            if u(0) < full {
                let local = glam::DVec2::new(x + (u(1) - 0.5) * 0.12, z + (u(2) - 0.5) * 0.15);
                graves.push((e.center + grimora_world::turn(local, e.yaw), e.yaw + (u(3) - 0.5) * 0.05));
            }
        }
    }
    graves
}
