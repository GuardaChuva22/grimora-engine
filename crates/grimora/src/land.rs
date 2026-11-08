//! The ground of a generated world (`--world`): tiles of [`grimora_world`] around the camera,
//! finer near it, built on all cores when they come into view, and colliders only for the
//! tiles around the player. Tiles are kept by their world key, never by local position, so an
//! origin shift only moves their entities and colliders, as it does everything else.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use glam::{DVec2, DVec3, Vec2, Vec3};
use grimora_core::{Material, MeshData, RiverPoint, TextureData, Transform, WaterBody, WaterShape, WorldPos,
    WorldTransform,
};
use grimora_physics::{ColliderHandle, Physics};
use grimora_scene::{ColliderShape, Ground, RiverDesc, load_texture};
use grimora_world::{
    FloraKey, FreshWaterLook, LifeBook, MacroMap, PlantSpot, Relief, TileKey, TileLook, build_tile, flora_around,
    map_to_world, place_plants, tile_collider, tiles_around,
};
use hecs::Entity;

use crate::generated::Generated;
use crate::world::{MeshId, Name, Renderable, World};

/// Level 0 tiles on each side of the player's that get a collider: 5 × 5 tiles, 640 m.
const COLLIDER_REACH: i64 = 2;
/// Mirrors `SAND_BELOW` and `ROCK_SLOPE` in `grimora_world`'s tiles, for footsteps.
const SAND_BELOW: f64 = 3.5;
const ROCK_SLOPE: f64 = 0.74;

/// A generated world's ground, kept in [`World::land`].
pub struct Land {
    relief: Arc<Relief>,
    look: TileLook,
    view: f64,
    shown: HashMap<TileKey, (Entity, MeshId)>,
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
    flora: Flora,
    /// The fresh water's drawn chunks (center, entity): only those within [`FRESH_REACH`] of
    /// the eye are drawn, checked again each [`FLORA_MOVE`] meters.
    fresh_drawn: Vec<(DVec2, Entity)>,
    fresh_eye: Option<DVec2>,
    /// Tiles built ahead and not shown yet, by key: shown all at once when every tile the view
    /// wants is here, so the ground never has a hole. Kept a while after the view moves on.
    ready: HashMap<TileKey, MeshData>,
    /// Builds tiles, colliders and plants on other threads; `None` until the first update.
    workers: Option<Workers>,
    /// Builds everything in [`World::update_land`] itself, as tests want.
    blocking: bool,
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
    Tile(TileKey, MeshData),
    Plants(FloraKey, Vec<PlantSpot>),
}

/// What the workers build from.
struct Makers {
    relief: Arc<Relief>,
    look: TileLook,
    life: Arc<LifeBook>,
}

impl Makers {
    fn run(&self, job: Job) -> Done {
        match job {
            Job::Collider(key) => {
                let (vertices, indices) = tile_collider(&self.relief, key);
                Done::Collider(key, vertices, indices)
            }
            Job::Tile(key) => Done::Tile(key, build_tile(&self.relief, key, &self.look)),
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

/// Meters from the eye past which the streams and ponds aren't drawn: at that distance they
/// are a pixel or two wide.
const FRESH_REACH: f64 = 3000.0;
/// Meters the camera moves before the plants' cells are picked again.
const FLORA_MOVE: f64 = 4.0;
/// The map of the sea's depth the waves shoal and break on: samples a side, meters between
/// them, and how far the camera goes before it is made again around it (on another thread).
const BED_SAMPLES: usize = 640;
const BED_SPACING: f64 = 5.0;
const BED_MOVE: f64 = 500.0;
/// Solid plants within this many meters of the player get a collider.
const PLANT_COLLIDER_REACH: f64 = 60.0;
/// Meters the player moves before those are picked again.
const PLANT_COLLIDER_MOVE: f64 = 4.0;
/// Meters above the ground over which the land's ambience fades out.
const AMBIENCE_ALOFT: f32 = 120.0;

/// The streams' and ponds' look, with its pictures loaded.
struct FreshWater {
    look: FreshWaterLook,
    texture: Option<Arc<TextureData>>,
    foam: Option<Arc<TextureData>>,
    fall_effect: Option<grimora_core::ParticleEffect>,
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
        let fresh_water = FreshWater {
            texture: water_picture(&fresh.texture),
            foam: water_picture(&fresh.foam_texture),
            fall_effect: fresh.fall_effect.as_ref().and_then(|file| {
                let path = map.dir.join(file);
                grimora_scene::load_effect(&path).map_err(|e| log::error!("Fall effect {}: {e:#}", path.display())).ok()
            }),
            fall_sound: fresh.fall_sound.as_ref().map(|file| map.dir.join(file)),
            look: fresh,
        };
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
        Ok(Self {
            life: Arc::new(life),
            life_dir,
            flora: Flora::default(),
            fresh_drawn: Vec::new(),
            fresh_eye: None,
            relief: Arc::new(Relief::new(map)),
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
        const STEP: f64 = 2.0;
        let h = |x: f64, z: f64| self.relief.height_at(x, z);
        let center = h(at.x, at.z);
        let dx = h(at.x + STEP, at.z) - center;
        let dz = h(at.x, at.z + STEP) - center;
        let up = STEP / (dx * dx + dz * dz + STEP * STEP).sqrt();
        if up < ROCK_SLOPE {
            Ground::Stone
        } else if center < SAND_BELOW {
            Ground::Sand
        } else {
            Ground::Grass
        }
    }

    /// What lives here: the world's life file.
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
        land.fresh_drawn = self.add_fresh_water(&land);
        self.add_plant_meshes(&mut land);
        self.add_land_fauna(&land);
        self.add_village(&land);
        self.land = Some(land);
    }

    /// Marks where the village will stand: a ring of stone pillars on its flattened ground.
    /// Why it is there goes to the log.
    fn add_village(&mut self, land: &Land) {
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
        // A tall stone in the middle, seen from far over the savanna.
        let mut mesh = grimora_scene::MeshDesc::Pillar { seed: village.id.0, broken: false }.build_in(&land.life_dir);
        grimora_scene::wrap_in_texture(&mut mesh, &stone, 1.3);
        let id = self.add_mesh(mesh);
        let mut transform = Transform::from_position(origin.to_local(DVec3::new(village.center.x, village.level - 0.3, village.center.y)));
        transform.scale = Vec3::new(3.0, 11.0, 3.0);
        self.ecs.spawn((transform, Renderable { color: Vec3::splat(2.0), ..Renderable::new(id) }, Name("village".into())));
        const PILLARS: u64 = 10;
        for k in 0..PILLARS {
            let angle = k as f64 * std::f64::consts::TAU / PILLARS as f64;
            let at = village.center + glam::DVec2::new(angle.cos(), angle.sin()) * 16.0;
            let desc = grimora_scene::MeshDesc::Pillar { seed: village.id.0.wrapping_add(k), broken: k % 3 == 1 };
            let mut mesh = desc.build_in(&land.life_dir);
            grimora_scene::wrap_in_texture(&mut mesh, &stone, 1.3);
            let id = self.add_mesh(mesh);
            let mut transform = Transform::from_position(origin.to_local(DVec3::new(at.x, village.level - 0.2, at.y)));
            transform.rotation = glam::Quat::from_rotation_y(-angle as f32);
            transform.scale = Vec3::new(2.2, 3.0, 2.2);
            self.ecs.spawn((transform, Renderable { color: Vec3::splat(2.0), ..Renderable::new(id) }, Name("village".into())));
        }
    }

    /// Makes each plant's variants once; every copy shares them.
    fn add_plant_meshes(&mut self, land: &mut Land) {
        let life = land.life.clone();
        let mut shapes = Vec::new();
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
                        plant_shapes.push(plant_shape(&mesh, plant.solid));
                        self.add_mesh(mesh)
                    })
                    .collect();
                shapes.push(plant_shapes);
                meshes
            })
            .collect();
        land.flora.shapes = shapes;
    }

    fn add_fresh_water(&mut self, land: &Land) -> Vec<(DVec2, Entity)> {
        let origin = self.origin();
        let water = &land.fresh_water;
        let look = &water.look;
        // Drawn as one surface, every stream and pond on the same grid: see `grimora_world`'s
        // surface.rs. The bodies below only answer where the water is.
        let started = std::time::Instant::now();
        let chunks = grimora_world::fresh_chunks(&land.relief, look.sea);
        let mut material = grimora_scene::water_material(&look.stream, water.texture.clone(), water.foam.clone(), look.tile);
        // A flow map: each vertex says which way the water goes.
        material.water_river = 2.0;
        let count = chunks.len();
        let mut triangles = 0;
        let mut drawn = Vec::new();
        for chunk in chunks {
            let mut mesh = chunk.mesh;
            mesh.materials = vec![material.clone()];
            mesh.wind = look.waves.max(0.0);
            for lod in &mut mesh.lods {
                lod.mesh.materials = vec![material.clone()];
                lod.mesh.wind = mesh.wind;
            }
            triangles += mesh.indices.len() / 3;
            let entity = self.spawn_water(mesh, origin.to_local(DVec3::new(chunk.center.x, 0.0, chunk.center.y)));
            drawn.push((chunk.center, entity));
        }
        log::info!("Fresh water: {count} chunks, {triangles} triangles up close, in {:?}", started.elapsed());
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
            // Mist and roar where it falls.
            for fall in grimora_scene::river_falls(&points) {
                if let Some(effect) = &water.fall_effect {
                    let scene = grimora_scene::SceneEffect {
                        effect: effect.clone(),
                        offset: Vec3::ZERO,
                        scale: (fall.half_width / grimora_scene::FALL_EFFECT_WIDTH).max(0.3),
                    };
                    let transform = Transform::from_position(fall.foot);
                    self.ecs.spawn((transform, scene.system(&transform)));
                }
                if let Some(path) = &water.fall_sound {
                    self.sounds.push(grimora_scene::SceneSound {
                        path: path.clone(),
                        position: Some(fall.foot),
                        volume: 1.0,
                        min_distance: fall.half_width + fall.height,
                        max_distance: 50.0 + fall.height * 4.0,
                    });
                }
            }
            let level = points[0].position.y;
            let shape = WaterShape::river(points, desc.speed);
            bodies.push(WaterBody { shape, level, ripples: look.waves, look: look.stream });
        }
        for pond in &land.relief.water().ponds {
            let reach = pond.reach() as f32;
            let center = origin.to_local(DVec3::new(pond.center.x, pond.level, pond.center.y));
            // A square sheet: past its shore, the ground stands over the water.
            let shape = WaterShape::Sheet { center: Vec2::new(center.x, center.z), half: Vec2::splat(reach) };
            bodies.push(WaterBody { shape, level: center.y, ripples: look.waves, look: look.stream });
        }
        let at = self.streamed_waters.min(self.waters.bodies.len());
        self.waters.bodies.splice(at..at, bodies);
        drawn
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

    fn spawn_water(&mut self, mesh: MeshData, position: Vec3) -> Entity {
        let id = self.add_mesh(mesh);
        self.ecs.spawn((Transform::from_position(position), Renderable::new(id), Name("water".into())))
    }

    /// Shows the tiles `eye` (local) should see and puts colliders around `focus` (local),
    /// building what is missing on every core. Call each frame; the runner does.
    pub fn update_land(&mut self, eye: Vec3, focus: Vec3, physics: &mut Physics) -> LandStats {
        let Some(mut land) = self.land.take() else {
            return LandStats::default();
        };
        let origin = self.origin();
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
                Done::Tile(key, mesh) => {
                    land.ready.insert(key, mesh);
                }
                Done::Collider(key, vertices, indices) => {
                    let near = (key.x - under.x).abs() <= COLLIDER_REACH + 1 && (key.z - under.z).abs() <= COLLIDER_REACH + 1;
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
            for (key, mesh) in unbuilt.iter().zip(build_all(&land.relief, &land.look, &unbuilt)) {
                land.ready.insert(*key, mesh);
            }
        }
        let built = missing.len();
        if missing.iter().all(|k| land.ready.contains_key(k)) {
            let keep: HashSet<TileKey> = wanted.iter().copied().collect();
            land.shown.retain(|key, (entity, mesh)| {
                if keep.contains(key) {
                    return true;
                }
                let _ = self.ecs.despawn(*entity);
                land.spare.push(*mesh);
                false
            });
            for key in missing.drain(..) {
                let Some(mesh) = land.ready.remove(&key) else { continue };
                let id = match land.spare.pop() {
                    Some(id) => {
                        self.replace_mesh(id, mesh);
                        id
                    }
                    None => self.add_mesh(mesh),
                };
                let center = key.center();
                let position = origin.to_local(DVec3::new(center.x, 0.0, center.y));
                let entity =
                    self.ecs.spawn((Transform::from_position(position), Renderable::new(id), Name("terrain".into())));
                land.shown.insert(key, (entity, id));
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
        self.update_fresh_drawn(&mut land, DVec2::new(eye_world.x, eye_world.z));
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
                }))
            });
            workers.ask(jobs);
        }
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

    /// Draws only the fresh water's chunks near `eye` (world), so walking the whole island
    /// doesn't draw every stream on it.
    fn update_fresh_drawn(&mut self, land: &mut Land, eye: DVec2) {
        if land.fresh_eye.is_some_and(|last| last.distance(eye) < FLORA_MOVE) {
            return;
        }
        land.fresh_eye = Some(eye);
        let reach = FRESH_REACH.min(land.view) + grimora_world::FRESH_CHUNK;
        for (center, entity) in &land.fresh_drawn {
            if let Ok(mut renderable) = self.ecs.get::<&mut Renderable>(*entity) {
                renderable.visible = center.distance(eye) <= reach;
            }
        }
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
                    let made = WorldTransform {
                        position: spot.at,
                        rotation: spot.rotation,
                        scale: Vec3::splat(spot.scale),
                    };
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

/// Builds tiles on every core. Each tile depends only on its key, so the split doesn't matter.
fn build_all(relief: &Relief, look: &TileLook, keys: &[TileKey]) -> Vec<MeshData> {
    if keys.len() < 2 {
        return keys.iter().map(|&k| build_tile(relief, k, look)).collect();
    }
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(keys.len());
    let per = keys.len().div_ceil(threads);
    std::thread::scope(|s| {
        let parts: Vec<_> = keys
            .chunks(per)
            .map(|part| s.spawn(move || part.iter().map(|&k| build_tile(relief, k, look)).collect::<Vec<_>>()))
            .collect();
        parts.into_iter().flat_map(|h| h.join().expect("tile builder panicked")).collect()
    })
}
