//! Houses in the world: spawning a [`grimora_scene::House`] and its doors, which open and
//! close when used.

use glam::{Quat, Vec3};
use grimora_core::Transform;
use grimora_physics::Physics;
use grimora_scene::{ColliderShape, ContainerDesc, Ground, House, Interactable, UsePoint};

use crate::items::Container;
use crate::world::{Collider, Light, Name, Renderable, TriangleGrounds, World};

/// How far a door turns open, radians (negative: into the house).
pub const DOOR_OPENS: f32 = -1.75;
/// The node of `Porta.glb` the leaf, its boards and its iron turn with: the hinge.
const DOOR_PIVOT: &str = "Pivô da porta";
/// How fast it turns, radians a second.
const DOOR_SPEED: f32 = 2.6;
/// How far ahead along their path (meters) a villager going through a door opens it: the
/// leaf takes about 0.7 s to turn.
const DOOR_AHEAD: f32 = 2.5;
/// How much wider than the leaf the doorway counts, each side, for a path to go through it.
const DOOR_SLACK: f32 = 0.2;
/// Someone this much past the leaf's length from the hinge is out of its way: it may shut.
const DOOR_CLEAR: f32 = 0.35;
/// How close one must be to use a door or a chest.
const USE_RADIUS: f32 = 2.2;

/// Who a house is for, on the house's entity: its trade (`pescador`, `ferreiro`...) and where
/// its beds are, in the house's space (its entity's `Transform` places them).
#[derive(Debug, Clone)]
pub struct Dwelling {
    pub trade: String,
    pub beds: Vec<Vec3>,
}

/// The box under a house's roof, on the house's entity: rain and snow stop there.
#[derive(Debug, Clone, Copy)]
pub struct Roof {
    pub half: glam::Vec2,
    pub top: f32,
}

impl Roof {
    pub fn covers(&self, at: &Transform, point: Vec3) -> bool {
        let p = at.rotation.inverse() * (point - at.position);
        p.x.abs() < self.half.x && p.z.abs() < self.half.y && p.y < self.top && p.y > -1.0
    }
}

/// Where people sit, lie or stand to use this entity (a chair, a bed, the counter, the anvil,
/// the stove, the well...), in its own space: its `Transform` places them in the world.
#[derive(Debug, Clone)]
pub struct Uses(pub Vec<UsePoint>);

/// A fire's particles (a flame, a chimney's smoke): burning only at night when `night_only`.
#[derive(Debug, Clone, Copy)]
pub struct Lit {
    pub night_only: bool,
}

/// A house's rooms, on the house's entity, in its space: under `wall_height` inside one of them
/// is indoors, where the sky's light is dim and only the room's own lights reach.
#[derive(Debug, Clone)]
pub struct Interior {
    pub rooms: Vec<(glam::Vec2, glam::Vec2)>,
    pub wall_height: f32,
}

/// A villager who may go through a door: their feet and what's left of their path.
pub(crate) struct Walker {
    pub at: Vec3,
    pub path: Vec<Vec3>,
}

/// Whether the flat segments `a0`-`a1` and `b0`-`b1` cross.
fn segments_cross(a0: glam::Vec2, a1: glam::Vec2, b0: glam::Vec2, b1: glam::Vec2) -> bool {
    let side = |p: glam::Vec2, q: glam::Vec2, r: glam::Vec2| (q - p).perp_dot(r - p);
    let (d1, d2) = (side(b0, b1, a0), side(b0, b1, a1));
    let (d3, d4) = (side(a0, a1, b0), side(a0, a1, b1));
    d1 * d2 <= 0.0 && d3 * d4 <= 0.0 && (d1 != d2 || d3 != d4)
}

/// A door leaf turning on its hinge: the entity's origin.
#[derive(Debug, Clone)]
pub struct Door {
    /// The leaf's turn when shut.
    pub closed: Quat,
    pub open: bool,
    /// How far it is open now, radians.
    pub angle: f32,
    /// The middle of its collider in the leaf's space.
    center: Vec3,
    /// Opened by someone walking through, not by the player: it shuts behind them.
    held: bool,
    /// Started to open (`true`) or to shut (`false`) since the last [`World::take_door_sounds`].
    heard: Option<bool>,
    /// Which way it is turning now, `None` at rest.
    moving: Option<bool>,
}

impl World {
    /// Spawns `house` placed by `at`: its walls, roof and floors as one static triangle
    /// collider, its furniture, its windows and its doors. `name` keys its chest.
    pub fn add_house(
        &mut self,
        house: &House,
        at: &Transform,
        name: &str,
        effects: &std::path::Path,
        physics: &mut Physics,
    ) {
        let place = |local: Vec3| at.position + at.rotation * local;
        for (i, mesh) in house.meshes.iter().enumerate() {
            let id = self.add_mesh(mesh.clone());
            let entity = self.ecs.spawn((*at, Renderable::new(id), Name(name.to_owned())));
            if i == 0 {
                let dwelling = Dwelling { trade: house.trade.name().to_owned(), beds: house.beds.clone() };
                let interior = Interior { rooms: house.rooms.clone(), wall_height: house.wall_height };
                self.ecs.insert(entity, (Roof { half: house.roof_half, top: house.ridge }, dwelling, interior)).ok();
                let (vertices, indices) = house.collision.clone();
                if let Some(handle) = physics.add_static(&ColliderShape::TriMesh { vertices, indices }, at) {
                    let grounds = TriangleGrounds(std::sync::Arc::new(house.collision_grounds.clone()));
                    self.ecs.insert(entity, (Collider(handle), grounds)).ok();
                }
            }
        }
        for (n, prop) in house.props.iter().enumerate() {
            // A thing one can take: once taken it stays taken, through saved games.
            let item = house_item(&prop.what).filter(|_| prop.flame.is_none() && prop.light.is_none());
            let key = format!("{name}:{}:{n}", prop.what);
            if item.is_some() && self.item_state.picked.get(&key).is_some_and(|&left| left == 0) {
                continue;
            }
            let Ok(mesh) = grimora_scene::load_model(&prop.path, 0) else {
                log::warn!("House: no model {}", prop.path.display());
                continue;
            };
            let points: Vec<Vec3> = mesh.vertices.iter().map(|v| Vec3::from(v.position)).collect();
            let transform =
                Transform { position: place(prop.position), rotation: at.rotation * prop.rotation, scale: Vec3::ONE };
            let ground = Ground::of_mesh(&mesh).or_else(|| Ground::from_name(&prop.what)).unwrap_or(Ground::Wood);
            let id = self.add_mesh(mesh);
            let entity =
                self.ecs.spawn((transform, Renderable::new(id), Name(format!("{name}:{}", prop.what)), ground));
            // Seats and beds let villagers through (they sit and lie on them); not the player.
            let passable = ["Cadeira", "Banco", "Banqueta", "Cama", "Beliche", "Enxergao"]
                .iter()
                .any(|w| prop.what.starts_with(w));
            let shape = ColliderShape::ConvexHull { points };
            if prop.solid
                && let Some(handle) =
                    if passable { physics.add_door(&shape, &transform) } else { physics.add_static(&shape, &transform) }
            {
                self.ecs.insert_one(entity, Collider(handle)).ok();
            }
            if !prop.uses.is_empty() {
                self.ecs.insert_one(entity, Uses(prop.uses.clone())).ok();
            }
            if let Some(item) = item {
                self.ecs
                    .insert(
                        entity,
                        (
                            crate::items::Pickup::new(key, item.to_owned(), 1),
                            Interactable { prompt: "pegar".into(), radius: USE_RADIUS },
                        ),
                    )
                    .ok();
            }
            if let Some(light) = &prop.light {
                self.ecs.insert_one(entity, Light(light.clone())).ok();
            }
            if let Some(flame) = &prop.flame {
                let at = Transform::from_position(transform.position + transform.rotation * flame.at);
                self.add_fire(&effects.join(flame.effect), at, flame.night_only);
            }
            if prop.container {
                let container = Container {
                    key: format!("{name}:bau"),
                    desc: ContainerDesc { size: (6, 4), ..Default::default() },
                };
                self.ecs.insert(entity, (container, Interactable { prompt: "abrir".into(), radius: USE_RADIUS })).ok();
            }
        }
        for &(top, night_only) in &house.smokes {
            self.add_fire(&effects.join("chimney_smoke.ron"), Transform::from_position(place(top)), night_only);
        }
        for door in &house.doors {
            let Ok((frame, leaf, hinge)) = grimora_scene::load_model_split(&door.path, DOOR_PIVOT) else {
                log::warn!("House: no door {}", door.path.display());
                continue;
            };
            let rotation = at.rotation * door.rotation;
            let frame_at = Transform { position: place(door.position), rotation, scale: Vec3::ONE };
            let id = self.add_mesh(frame);
            self.ecs.spawn((frame_at, Renderable::new(id), Name(format!("{name}:porta"))));
            let (min, max) = leaf.bounds().unwrap_or((Vec3::splat(-0.1), Vec3::splat(0.1)));
            let center = (min + max) * 0.5;
            let leaf_at = Transform { position: frame_at.position + rotation * hinge, rotation, scale: Vec3::ONE };
            let id = self.add_mesh(leaf);
            let entity = self.ecs.spawn((
                leaf_at,
                Renderable::new(id),
                Name(format!("{name}:folha")),
                Interactable { prompt: "abrir".into(), radius: USE_RADIUS },
                Door { closed: rotation, open: false, angle: 0.0, center, held: false, heard: None, moving: None },
            ));
            let shape = ColliderShape::Cuboid { center, half_extents: (max - min) * 0.5 };
            if let Some(handle) = physics.add_door(&shape, &leaf_at) {
                self.ecs.insert(entity, (Collider(handle), Ground::Wood)).ok();
            }
        }
    }

    /// Every room of every house, for the renderer: indoors.
    pub fn room_boxes(&self) -> Vec<grimora_core::RoomBox> {
        let mut boxes = Vec::new();
        for (at, interior) in self.ecs.query::<(&Transform, &Interior)>().iter() {
            let x = at.rotation * Vec3::X;
            let axis = glam::Vec2::new(x.x, x.z).normalize_or(glam::Vec2::X);
            for &(min, max) in &interior.rooms {
                let middle = (min + max) * 0.5;
                boxes.push(grimora_core::RoomBox {
                    center: at.position + at.rotation * Vec3::new(middle.x, 0.0, middle.y),
                    axis,
                    half: (max - min) * 0.5,
                    walls: at.position.y + interior.wall_height,
                });
            }
        }
        boxes
    }

    /// A looping effect burning at `at`, only at night when `night_only`.
    pub fn add_fire(&mut self, effect: &std::path::Path, at: Transform, night_only: bool) -> Option<hecs::Entity> {
        match grimora_scene::load_effect(effect) {
            Ok(effect) => {
                let seed = u64::from(at.position.x.to_bits()) << 32 ^ u64::from(at.position.z.to_bits());
                let system = grimora_core::ParticleSystem::new(effect, Vec3::ZERO, 1.0, seed);
                Some(self.ecs.spawn((at, system, Lit { night_only })))
            }
            Err(err) => {
                log::warn!("House: {err:#}");
                None
            }
        }
    }

    /// A model from `path` standing at `at`, bumped into (its hull) when `solid`.
    pub fn add_model(
        &mut self,
        path: &std::path::Path,
        at: &Transform,
        solid: bool,
        physics: &mut Physics,
    ) -> Option<hecs::Entity> {
        let Ok(mesh) = grimora_scene::load_model(path, 0) else {
            log::warn!("No model {}", path.display());
            return None;
        };
        let points: Vec<Vec3> = mesh.vertices.iter().map(|v| Vec3::from(v.position)).collect();
        let what = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let uses = mesh.bounds().map(|(min, max)| grimora_scene::use_points(&what, min, max)).unwrap_or_default();
        let ground = Ground::of_mesh(&mesh).or_else(|| Ground::from_name(&what)).unwrap_or(Ground::Stone);
        let id = self.add_mesh(mesh);
        let entity = self.ecs.spawn((*at, Renderable::new(id), Name(what), ground));
        if !uses.is_empty() {
            self.ecs.insert_one(entity, Uses(uses)).ok();
        }
        if solid && let Some(handle) = physics.add_static(&ColliderShape::ConvexHull { points }, at) {
            self.ecs.insert_one(entity, Collider(handle)).ok();
        }
        Some(entity)
    }

    /// Opens a shut door or shuts an open one; `None` if `entity` is no door.
    pub fn toggle_door(&mut self, entity: hecs::Entity) -> Option<bool> {
        let mut door = self.ecs.get::<&mut Door>(entity).ok()?;
        door.open = !door.open;
        door.held = false;
        let open = door.open;
        drop(door);
        if let Ok(mut interactable) = self.ecs.get::<&mut Interactable>(entity) {
            interactable.prompt = if open { "fechar" } else { "abrir" }.into();
        }
        Some(open)
    }

    /// Opens each door that one of `walkers` is about to go through (their path crosses the
    /// doorway within [`DOOR_AHEAD`]), and shuts the ones they opened once nobody is going
    /// through or standing where the leaf turns. Doors the player opened stay as they are.
    pub(crate) fn doors_for(&mut self, walkers: &[Walker]) {
        let flat = |v: Vec3| glam::Vec2::new(v.x, v.z);
        for (door, transform, interactable) in
            self.ecs.query_mut::<(&mut Door, &Transform, Option<&mut Interactable>)>()
        {
            // The doorway, flat: from the hinge across the shut leaf, a little wider.
            let hinge = flat(transform.position);
            let span = flat(door.closed * door.center) * 2.0;
            let length = span.length();
            if length < 0.01 {
                continue;
            }
            let along = span / length;
            let (a, b) = (hinge - along * DOOR_SLACK, hinge + span + along * DOOR_SLACK);
            let level = transform.position.y + door.center.y;
            let by = |w: &Walker| (w.at.y - level).abs() < 2.0;
            let through = walkers.iter().filter(|w| by(w)).any(|w| {
                let mut from = flat(w.at);
                let mut left = DOOR_AHEAD;
                for &next in &w.path {
                    let to = flat(next);
                    let step = from.distance(to);
                    let to = if step > left { from + (to - from) * (left / step) } else { to };
                    if segments_cross(from, to, a, b) {
                        return true;
                    }
                    left -= step;
                    if left <= 0.0 {
                        break;
                    }
                    from = to;
                }
                false
            });
            let in_the_way = walkers.iter().filter(|w| by(w)).any(|w| flat(w.at).distance(hinge) < length + DOOR_CLEAR);
            if through && !door.open {
                (door.open, door.held) = (true, true);
            } else if !through && !in_the_way && door.held {
                (door.open, door.held) = (false, false);
            } else {
                continue;
            }
            if let Some(interactable) = interactable {
                interactable.prompt = if door.open { "fechar" } else { "abrir" }.into();
            }
        }
    }

    /// Where a door started to open (`true`) or to shut (`false`) since the last call,
    /// for the game to play.
    pub fn take_door_sounds(&mut self) -> Vec<(Vec3, bool)> {
        let mut sounds = Vec::new();
        for (door, transform) in self.ecs.query_mut::<(&mut Door, &Transform)>() {
            if let Some(opening) = door.heard.take() {
                sounds.push((transform.position + transform.rotation * door.center, opening));
            }
        }
        sounds
    }

    /// Turns the doors toward open or shut, their colliders with them.
    pub fn update_doors(&mut self, physics: &mut Physics, dt: f32) {
        for (door, transform, collider) in self.ecs.query_mut::<(&mut Door, &mut Transform, Option<&Collider>)>() {
            let target = if door.open { DOOR_OPENS } else { 0.0 };
            if door.angle == target {
                door.moving = None;
                continue;
            }
            // It starts to move, from rest or turning back: one sound.
            let opening = door.open;
            if door.moving != Some(opening) {
                door.moving = Some(opening);
                door.heard = Some(opening);
            }
            let step = DOOR_SPEED * dt;
            door.angle = if (target - door.angle).abs() <= step {
                target
            } else {
                door.angle + step * (target - door.angle).signum()
            };
            transform.rotation = door.closed * Quat::from_rotation_y(door.angle);
            if let Some(collider) = collider {
                physics.set_collider_pose(
                    collider.0,
                    transform.position + transform.rotation * door.center,
                    transform.rotation,
                );
            }
        }
    }
}

/// The item a house prop is, by its name, if the player (or anyone) can take it.
pub fn house_item(what: &str) -> Option<&'static str> {
    Some(match what {
        "Vassoura-de-Palha" => "vassoura",
        "Balde-de-Madeira" => "balde",
        "Caneca" => "caneca",
        "Concha-de-Madeira" => "concha",
        "Colher-de-Madeira" => "colher_de_pau",
        "Garfo-de-Madeira" => "garfo_de_pau",
        "Faca-de-Cozinha" => "faca_de_cozinha",
        "Cutelo-de-Acougueiro" => "cutelo",
        "Tabua-de-Cortar" => "tabua_de_cortar",
        "Tigela-de-Barro" => "tigela_de_barro",
        "Prato" => "prato",
        "Jarra-de-Barro" => "jarra_de_barro",
        "Pote-de-Barro" => "pote_de_barro",
        "Garrafa-de-Vidro-Vazia" => "garrafa_vazia",
        "Garrafa-de-Vinho-Oliva" => "garrafa_de_vinho",
        "Garrafa-Barriguda-Ambar" => "garrafa_ambar",
        "Frasco-Quadrado-Azul" => "frasco_azul",
        "Garrafa-Pocao-Estriada" => "pocao_estriada",
        "Cantil-de-Vidro-Fume" => "cantil",
        "Martelo-de-Ferreiro" => "martelo",
        "Panela-de-Ferro" => "panela_de_ferro",
        "Penico-de-Ceramica" => "penico",
        "Bacia-de-Lavar" => "bacia",
        "Lenha-Avulsa" => "lenha",
        "Pao" => "pao",
        "Queijo" => "queijo",
        "Maca" => "maca",
        _ => return None,
    })
}
