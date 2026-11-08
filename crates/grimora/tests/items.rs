//! Carrying items: stacks, the hotbar, taking them from the world, chests, and how all of it
//! lasts through streaming and saved games. Headless, like `character.rs`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use grimora::prelude::*;
use grimora::save::{SaveFile, WorldState};
use grimora::scene::Scene;
use grimora::{Container, ItemState};

const CATALOG: &str = r#"[
    (id: "maca", name: "Maçã", stack: 5, kind: Consumable(health: 15.0, stamina: 5.0)),
    (id: "pedra", name: "Pedra", stack: 99),
    (id: "espada", name: "Espada", size: (1, 3), scale: 0.25, kind: Weapon(weapon: (damage: 12.0))),
    (id: "adaga", name: "Adaga", scale: 0.25, kind: Weapon(weapon: (damage: 5.0))),
]"#;

fn catalog() -> ItemCatalog {
    ItemCatalog::parse(CATALOG, Path::new("")).unwrap()
}

/// A new folder with the catalog in it, for scene text that reads `items: "items.ron"`.
fn folder() -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let name = format!("grimora_items_{}_{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
    let dir = std::env::temp_dir().join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("items.ron"), CATALOG).unwrap();
    dir
}

fn build(dir: &Path, source: &str) -> (Physics, World) {
    let path = dir.join("scene.ron");
    std::fs::write(&path, source).unwrap();
    let mut physics = Physics::new();
    let mut world = World::from_scene(Scene::load(&path).unwrap(), &mut physics);
    physics.refresh();
    world.load_around(Vec3::ZERO, &mut physics);
    (physics, world)
}

fn find_pickup(world: &World, item: &str) -> Option<Entity> {
    let mut query = world.ecs.query::<(Entity, &Pickup)>();
    query.iter().find(|(_, pickup)| pickup.item == item).map(|(entity, _)| entity)
}

#[test]
fn stacks_fill_up_and_overflow() {
    let catalog = catalog();
    let mut bag = Inventory::new(3, 1);
    assert_eq!(bag.add(&catalog, "maca", 3), 0);
    assert_eq!(bag.add(&catalog, "maca", 4), 0, "on top of the first stack, then a new one");
    let id = |bag: &Inventory, x| bag.at(x, 0).unwrap().id;
    assert_eq!(bag.at(0, 0).unwrap().stack.count, 5);
    assert_eq!(bag.at(1, 0).unwrap().stack.count, 2);
    assert_eq!(bag.count_of("maca"), 7);
    assert_eq!(bag.add(&catalog, "adaga", 1), 0);
    assert_eq!(bag.add(&catalog, "adaga", 1), 1, "a dagger doesn't stack, and there's no room left");
    assert_eq!(bag.add(&catalog, "maca", 5), 2, "only what fits on the stack with room");
    assert_eq!(bag.count_of("maca"), 10);

    let second = id(&bag, 1);
    assert_eq!(bag.take(second, 2), Some(ItemStack { item: "maca".into(), count: 2 }));
    assert_eq!(bag.piece(second).unwrap().stack.count, 3);
    assert_eq!(bag.take(second, 9).unwrap().count, 3, "no more than there is");
    assert!(bag.piece(second).is_none());
    assert_eq!(bag.take(second, 1), None);
    assert_eq!(bag.take_item("maca", 3), 3);
    assert_eq!(bag.count_of("maca"), 2);
}

#[test]
fn a_piece_takes_the_cells_its_item_says() {
    let catalog = catalog();
    let mut bag = Inventory::new(3, 3);
    assert_eq!(bag.add(&catalog, "espada", 1), 0, "three cells, standing");
    let sword = bag.first_of("espada").unwrap().clone();
    assert_eq!((sword.x, sword.y, sword.size, sword.turned), (0, 0, (1, 3), false));
    assert!((0..3).all(|y| bag.at(0, y).is_some()));
    assert_eq!(bag.cells_taken(), 3);

    // Only lying down is there room left for it.
    let mut low = Inventory::new(3, 2);
    low.add(&catalog, "espada", 1);
    let lying = low.first_of("espada").unwrap();
    assert_eq!((lying.size, lying.turned), ((3, 1), true));
    assert_eq!(low.add(&catalog, "espada", 1), 0, "the row under it");
    assert_eq!(low.add(&catalog, "espada", 1), 1, "and there's no third row");
    assert!(Inventory::new(2, 2).add(&catalog, "espada", 1) == 1, "too long either way");
}

#[test]
fn pieces_move_merge_and_swap() {
    let catalog = catalog();
    let mut bag = Inventory::new(4, 3);
    bag.add(&catalog, "espada", 1);
    bag.add(&catalog, "adaga", 1);
    let sword = bag.first_of("espada").unwrap().id;
    let dagger = bag.first_of("adaga").unwrap().id;

    assert!(bag.move_piece(&catalog, sword, (2, 0), false));
    assert_eq!(bag.piece(sword).unwrap().x, 2);
    assert!(!bag.move_piece(&catalog, sword, (3, 1), false), "it would stick out of the grid");
    assert!(bag.move_piece(&catalog, sword, (1, 2), true), "turned, it lies along the last row");
    assert_eq!(bag.piece(sword).unwrap().size, (3, 1));
    assert!(!bag.move_piece(&catalog, sword, (2, 0), true), "lying, it no longer fits there");

    // Landing on one piece sends it to where this one was.
    assert_eq!(bag.piece(dagger).unwrap().x, 1);
    let dagger_at = (bag.piece(dagger).unwrap().x, bag.piece(dagger).unwrap().y);
    assert!(bag.move_piece(&catalog, sword, (0, 2), true));
    let before = (bag.piece(sword).unwrap().x, bag.piece(sword).unwrap().y);
    assert!(bag.move_piece(&catalog, dagger, before, false));
    assert_eq!((bag.piece(dagger).unwrap().x, bag.piece(dagger).unwrap().y), before);
    assert_ne!((bag.piece(sword).unwrap().x, bag.piece(sword).unwrap().y), before, "the sword got out of the way");
    assert_ne!(dagger_at, before);

    // On two pieces at once, nothing happens.
    let mut crowded = Inventory::new(3, 3);
    crowded.add(&catalog, "adaga", 1);
    crowded.add(&catalog, "adaga", 1);
    crowded.add(&catalog, "espada", 1);
    let long = crowded.first_of("espada").unwrap().clone();
    let landing = crowded.landing(&catalog, "espada", Some(long.id), (0, 0), (1, 3));
    assert_eq!(landing, Landing::Blocked);

    // The same item goes on top, and what doesn't fit stays.
    let mut apples = Inventory::new(3, 1);
    apples.add(&catalog, "maca", 7);
    let (full, part) = (apples.at(0, 0).unwrap().id, apples.at(1, 0).unwrap().id);
    apples.take(full, 1);
    assert!(apples.move_piece(&catalog, part, (0, 0), false));
    assert_eq!(apples.piece(full).unwrap().stack.count, 5);
    assert_eq!(apples.piece(part).unwrap().stack.count, 1);
    apples.take(full, 2);
    assert!(apples.move_piece(&catalog, part, (0, 0), false));
    assert!(apples.piece(part).is_none(), "all of it went on top");
    assert_eq!(apples.count_of("maca"), 4);
}

#[test]
fn shortcuts_and_the_hand_follow_the_piece() {
    let catalog = catalog();
    let mut bag = Inventory::new(5, 3);
    bag.add(&catalog, "adaga", 1);
    bag.add(&catalog, "pedra", 1);
    bag.add(&catalog, "espada", 1);
    let dagger = bag.first_of("adaga").unwrap().id;
    let sword = bag.first_of("espada").unwrap().id;
    assert_eq!(bag.shortcut_of(dagger), None, "nothing goes on the hotbar by itself");
    let taken = bag.cells_taken();
    assert!(bag.set_shortcut(0, Some(dagger)));
    assert!(bag.set_shortcut(1, Some(sword)));
    assert!(bag.cells_taken() < taken, "the hotbar is off the grid");
    assert!(bag.at(bag.piece(sword).unwrap().x, bag.piece(sword).unwrap().y).is_none());

    bag.set_hand(Some(sword));
    assert!(bag.move_piece(&catalog, sword, (4, 0), false));
    assert_eq!(bag.held().unwrap().item, "espada", "moving it doesn't take it out of the hand");
    assert_eq!(bag.shortcut(1).unwrap().id, sword);

    bag.set_shortcut(3, Some(sword));
    assert_eq!((bag.shortcut_of(sword), bag.shortcut(1).is_none()), (Some(3), true), "one key for each");

    // Another piece takes the slot, and what was there is back on the grid.
    let stone = bag.first_of("pedra").unwrap().id;
    assert!(bag.set_shortcut(0, Some(stone)));
    assert!(bag.on_grid(dagger) && !bag.on_grid(stone));
    bag.swap_shortcuts(0, 2);
    assert_eq!(bag.shortcut(2).unwrap().id, stone);

    bag.arrange(&catalog);
    assert_eq!((bag.held().unwrap().item.as_str(), bag.shortcut(3).unwrap().id), ("espada", sword));

    let mut chest = Inventory::new(3, 3);
    assert!(bag.move_piece_to(&catalog, sword, &mut chest, (0, 0), false));
    assert_eq!((bag.hand(), bag.shortcut(3).is_none()), (None, true), "it went into the chest");
    assert_eq!(chest.count_of("espada"), 1);

    // Eating the last of a stack moves its shortcut to the next one.
    let mut food = Inventory::new(2, 1);
    food.add(&catalog, "maca", 6);
    let first = food.first_of("maca").unwrap().id;
    food.set_shortcut(0, Some(first));
    food.take(first, 5);
    assert_eq!(food.shortcut(0).unwrap().stack.count, 1);
}

#[test]
fn arranging_joins_and_packs_or_changes_nothing() {
    let catalog = catalog();
    let mut bag = Inventory::new(4, 3);
    bag.add(&catalog, "maca", 7);
    bag.add(&catalog, "espada", 1);
    let first = bag.first_of("maca").unwrap().id;
    bag.take(first, 3);
    assert!(bag.arrange(&catalog));
    assert_eq!(bag.count_of("maca"), 4);
    assert_eq!(bag.pieces().iter().filter(|p| p.stack.item == "maca").count(), 1, "the stacks are one now");
    let sword = bag.first_of("espada").unwrap();
    assert_eq!((sword.x, sword.y), (0, 0), "the longest first");

    // Three swords lying in a 3x3 grid leave no way to stand them all and also fit this.
    let mut tight = Inventory::new(3, 2);
    tight.add(&catalog, "espada", 2);
    let before = tight.clone();
    assert!(!tight.arrange(&catalog), "standing, they don't fit");
    assert_eq!(tight, before);
}

#[test]
fn a_save_from_before_the_grid_still_loads() {
    let catalog = catalog();
    let text = r#"(slots: [Some((item: "espada", count: 1)), None, Some((item: "maca", count: 3))], hand: Some(2))"#;
    let mut bag: Inventory = ron::from_str(text).unwrap();
    bag.settle(&catalog, 4, 3);
    assert_eq!((bag.width(), bag.height()), (4, 3));
    assert_eq!(bag.count_of("espada"), 1);
    assert_eq!(bag.shortcut(0).unwrap().stack.item, "espada");
    assert_eq!(bag.shortcut(2).unwrap().stack.item, "maca");
    assert_eq!(bag.held().unwrap().item, "maca");
    let settled = bag.clone();
    bag.settle(&catalog, 9, 9);
    assert_eq!(bag, settled, "only once");

    // More than fits gets more rows, since none of it may be lost.
    let text = r#"(slots: [Some((item: "espada", count: 1)), Some((item: "espada", count: 1))])"#;
    let mut full: Inventory = ron::from_str(text).unwrap();
    full.settle(&catalog, 1, 3);
    assert_eq!(full.count_of("espada"), 2);
}

#[test]
fn an_inventory_lasts_through_a_save_file() {
    let catalog = catalog();
    let mut bag = Inventory::new(HOTBAR as u32, 1);
    bag.add(&catalog, "maca", 3);
    bag.set_hand(bag.first_of("maca").map(|piece| piece.id));
    let dir = folder();
    let store = SaveStore::new(&dir);
    store.write("bag", &SaveFile::new(None, WorldState::default(), bag.clone())).unwrap();
    let read: SaveFile<Inventory> = store.read("bag").unwrap();
    assert_eq!(read.game, bag);
    std::fs::remove_dir_all(dir).unwrap();
}

const SCENE: &str = r#"(
    items: "items.ron",
    objects: [
        (name: "marker", mesh: Cube, position: (9.0, 0.0, 0.0), collider: None),
        (name: "apples", pickup: (item: "maca", count: 7), position: (1.0, 0.0, 0.0)),
        (pickup: (item: "espada"), position: (-1.0, 0.0, 0.0), rotation: (0.0, 30.0, 0.0)),
        (pickup: (item: "nada"), position: (-2.0, 0.0, 0.0)),
        (name: "chest", mesh: Cube, position: (0.0, 0.0, -3.0), collider: None,
         interact: (prompt: "abrir"), container: (slots: 4, items: [("pedra", 120), ("adaga", 1)])),
    ],
)"#;

#[test]
fn what_lies_in_the_world_can_be_taken() {
    let dir = folder();
    let (mut physics, mut world) = build(&dir, SCENE);
    assert!(find_pickup(&world, "nada").is_none(), "an item the catalog lacks isn't placed");

    let sword = find_pickup(&world, "espada").unwrap();
    let lying = world.transform(sword).unwrap();
    assert_eq!(lying.scale, Vec3::splat(0.25), "the catalog's scale");
    let target = world.interaction_target(Vec3::new(-1.0, 1.0, 1.0), Vec3::NEG_Z, &physics).unwrap();
    assert_eq!((target.entity, target.prompt.as_str()), (sword, "pegar Espada"));

    let mut bag = Inventory::new(4, 1);
    assert_eq!(world.pick_up(sword, &mut bag, &mut physics), 1);
    assert!(world.get::<Pickup>(sword).is_none(), "gone from the world");
    assert_eq!(bag.count_of("espada"), 1);

    let marker = world.find("marker").unwrap();
    assert_eq!(world.pick_up(marker, &mut bag, &mut physics), 0, "only pickups can be picked up");

    // A slot left for five of the seven.
    let apples = find_pickup(&world, "maca").unwrap();
    assert_eq!(world.pick_up(apples, &mut bag, &mut physics), 5);
    assert_eq!(world.get::<Pickup>(apples).unwrap().count, 2, "what didn't fit stays");
    assert_eq!(world.pick_up(apples, &mut bag, &mut physics), 0, "still no room");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn what_was_taken_stays_taken_after_loading() {
    let dir = folder();
    let (mut physics, mut world) = build(&dir, SCENE);
    let mut camera = Camera::default();
    let mut bag = Inventory::new(4, 1);
    world.pick_up(find_pickup(&world, "espada").unwrap(), &mut bag, &mut physics);
    let apples = find_pickup(&world, "maca").unwrap();
    world.pick_up(apples, &mut bag, &mut physics);

    let dropped =
        world.drop_item(ItemStack { item: "pedra".into(), count: 3 }, Vec3::new(4.0, 2.0, 0.0), 10.0, &physics);
    assert!(dropped.is_some());
    assert!(world.drop_item(ItemStack { item: "nada".into(), count: 1 }, Vec3::ZERO, 0.0, &physics).is_none());

    let saved = WorldState::capture(&world, &physics, &camera);
    let text = ron::to_string(&saved).unwrap();

    // Everything changes, and the save puts it back.
    world.item_state = ItemState::default();
    world.pick_up(dropped.unwrap(), &mut Inventory::new(1, 1), &mut physics);
    assert!(find_pickup(&world, "pedra").is_none());

    let read: WorldState = ron::from_str(&text).unwrap();
    read.restore(&mut world, &mut physics, &mut camera);
    assert!(find_pickup(&world, "espada").is_none(), "the sword stays taken");
    assert_eq!(world.get::<Pickup>(find_pickup(&world, "maca").unwrap()).unwrap().count, 2, "and part of the apples");
    let stone = find_pickup(&world, "pedra").unwrap();
    assert_eq!(world.get::<Pickup>(stone).unwrap().count, 3, "what was dropped is back");

    // Taking a dropped thing is remembered too, and a new drop doesn't reuse its key.
    world.pick_up(stone, &mut Inventory::new(1, 1), &mut physics);
    let saved = WorldState::capture(&world, &physics, &camera);
    assert!(saved.items.dropped.is_empty());
    saved.restore(&mut world, &mut physics, &mut camera);
    assert!(find_pickup(&world, "pedra").is_none());

    // An older save, without any of it, brings everything back.
    let old = WorldState::default();
    old.restore(&mut world, &mut physics, &mut camera);
    assert!(find_pickup(&world, "espada").is_some() && find_pickup(&world, "maca").is_some());
    assert_eq!(world.get::<Pickup>(find_pickup(&world, "maca").unwrap()).unwrap().count, 7);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn chests_keep_what_is_put_in_them() {
    let dir = folder();
    let (mut physics, mut world) = build(&dir, SCENE);
    let mut camera = Camera::default();
    let catalog = world.items.clone();
    let chest = world.find("chest").unwrap();
    let key = world.container_key(chest).unwrap();
    assert_eq!(key, "chest");
    assert!(world.get::<Container>(chest).is_some());
    assert!(world.container("nobody").is_none());
    assert!(world.item_state.containers.is_empty(), "nothing to save until it's opened");

    let mut bag = Inventory::new(4, 1);
    let contents = world.container(&key).unwrap();
    assert_eq!((contents.count_of("pedra"), contents.count_of("adaga")), (120, 1));
    assert_eq!(contents.first_of("pedra").unwrap().stack.count, 99, "its stacks follow the catalog");
    let dagger = contents.first_of("adaga").unwrap().id;
    assert!(contents.move_piece_to(&catalog, dagger, &mut bag, (0, 0), false));
    assert_eq!(bag.count_of("adaga"), 1);
    assert_eq!(world.container(&key).unwrap().count_of("adaga"), 0);

    let saved = WorldState::capture(&world, &physics, &camera);
    world.container(&key).unwrap().add(&catalog, "maca", 1);
    saved.restore(&mut world, &mut physics, &mut camera);
    let contents = world.container(&key).unwrap();
    assert_eq!((contents.count_of("adaga"), contents.count_of("maca")), (0, 0), "as it was when saved");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn eating_gives_back_what_the_item_says() {
    let dir = folder();
    let (_physics, mut world) = build(&dir, SCENE);
    let eater = world.ecs.spawn((Health::new(20.0, 0), Stamina::new(50.0)));
    world.get_mut::<Health>(eater).unwrap().current = 10.0;
    world.get_mut::<Stamina>(eater).unwrap().current = 0.0;
    assert!(world.consume(eater, "maca"));
    assert_eq!(world.get::<Health>(eater).unwrap().current, 20.0, "no more than the bar holds");
    assert_eq!(world.get::<Stamina>(eater).unwrap().current, 5.0);
    assert!(!world.consume(eater, "espada"), "a sword isn't eaten");
    assert!(!world.consume(eater, "nada"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn what_was_taken_stays_taken_through_streaming() {
    let dir = folder();
    let region = r#"(
        items: "items.ron",
        objects: [(name: "region_apples", pickup: (item: "maca", count: 3), position: (0.0, 0.0, 0.0))],
    )"#;
    std::fs::write(dir.join("region.ron"), region).unwrap();
    let scene = r#"(
        items: "items.ron",
        regions: [(path: "region.ron", center: (0.0, 0.0, 0.0), radius: 5.0)],
        streaming: (cell_size: 32.0, load_distance: 30.0),
    )"#;
    let (mut physics, mut world) = build(&dir, scene);
    let apples = find_pickup(&world, "maca").expect("the region is around the camera");
    assert_eq!(world.get::<Pickup>(apples).unwrap().key, "region_apples");

    // A bag with room for only some of them, then the region comes and goes.
    let mut bag = Inventory::new(1, 1);
    bag.add(&world.items.clone(), "maca", 4);
    assert_eq!(world.pick_up(apples, &mut bag, &mut physics), 1);

    let far = Vec3::new(500.0, 0.0, 0.0);
    world.load_around(far, &mut physics);
    assert!(find_pickup(&world, "maca").is_none(), "unloaded");
    world.load_around(Vec3::ZERO, &mut physics);
    let again = find_pickup(&world, "maca").expect("the rest is there again");
    assert_eq!(world.get::<Pickup>(again).unwrap().count, 2, "and not the three of the file");

    // All of it taken, it doesn't come back at all, not even after saving and loading.
    world.pick_up(again, &mut Inventory::new(1, 1), &mut physics);
    world.load_around(far, &mut physics);
    world.load_around(Vec3::ZERO, &mut physics);
    assert!(find_pickup(&world, "maca").is_none());

    let mut camera = Camera::default();
    let saved = WorldState::capture(&world, &physics, &camera);
    saved.restore(&mut world, &mut physics, &mut camera);
    assert!(find_pickup(&world, "maca").is_none());
    std::fs::remove_dir_all(dir).unwrap();
}
