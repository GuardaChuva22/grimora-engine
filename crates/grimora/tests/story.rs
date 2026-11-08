//! Dialogues, quests and recipes, on data written here.

use std::path::Path;

use grimora::scene::ItemCatalog;
use grimora::scene::story::{Condition, Deed, RecipeDesc, StoryBook};
use grimora::story::{QuestNews, Story, make, sale_price};
use grimora::{Inventory, prelude::*};

const ITEMS: &str = r#"[
    (id: "moeda", name: "Moeda", stack: 99, price: 1),
    (id: "erva", name: "Erva", stack: 5, price: 3),
    (id: "tonico", name: "Tônico", size: (1, 2), price: 10),
]"#;

const STORY: &str = r#"(
    dialogues: [(object: "vela", speaker: "Vela", openings: [([QuestDone("pedra")], "fim")], nodes: [
        (id: "oi", text: "Vá.", choices: [(text: "Sim.", does: [StartQuest("pedra")])]),
        (id: "fim", text: "Voltou."),
    ])],
    quests: [(id: "pedra", name: "A pedra", reward: [("moeda", 5)], stages: [
        (text: "Vá até a pedra.", until: [Flag("area:pedra")]),
        (text: "Leve duas ervas.", until: [Has("erva", 2)]),
    ])],
    shops: [(id: "loja", name: "Loja", wares: [(item: "erva")])],
    recipes: [(makes: ("tonico", 1), needs: [("erva", 2)])],
)"#;

fn catalog() -> ItemCatalog {
    ItemCatalog::parse(ITEMS, Path::new("")).unwrap()
}

#[test]
fn a_quest_starts_moves_on_pays_and_changes_what_is_said() {
    let (book, catalog) = (StoryBook::parse(STORY).unwrap(), catalog());
    let mut bag = Inventory::new(4, 4);
    let mut story = Story::default();
    let dialogue = book.dialogue_of("vela").unwrap();
    assert_eq!(story.opening(&book, &bag, dialogue), Some("oi"));

    let deeds = [Deed::StartQuest("pedra".into())];
    let (_, news) = story.apply(&book, &catalog, &mut bag, &deeds);
    assert_eq!(news, [QuestNews::Started("pedra".into())]);
    assert!(story.holds(&book, &bag, &Condition::QuestActive("pedra".into())));
    assert!(story.advance(&book, &catalog, &mut bag).is_empty(), "nothing's happened yet");

    story.set_flag("area:pedra");
    assert_eq!(story.advance(&book, &catalog, &mut bag), [QuestNews::Moved("pedra".into())]);
    bag.add(&catalog, "erva", 2);
    assert_eq!(story.advance(&book, &catalog, &mut bag), [QuestNews::Done("pedra".into())]);
    assert_eq!(bag.count_of("moeda"), 5);
    assert!(story.advance(&book, &catalog, &mut bag).is_empty(), "it only pays once");
    assert_eq!(story.opening(&book, &bag, dialogue), Some("fim"));
}

#[test]
fn a_story_lasts_through_a_save_and_a_shop_opens_from_a_choice() {
    let (book, catalog) = (StoryBook::parse(STORY).unwrap(), catalog());
    let mut bag = Inventory::new(4, 4);
    let mut story = Story::default();
    let deeds = [Deed::Shop("loja".into()), Deed::Give("moeda".into(), 2), Deed::Flag("falou".into())];
    assert_eq!(story.apply(&book, &catalog, &mut bag, &deeds).0.as_deref(), Some("loja"));
    assert_eq!(bag.count_of("moeda"), 2);
    story.killed("Sardinha");
    story.saw("Gaivota");
    let text = ron::to_string(&story).unwrap();
    let read: Story = ron::from_str(&text).unwrap();
    assert_eq!(read, story);
    assert_eq!((read.bestiary["Sardinha"].killed, read.bestiary["Gaivota"].killed), (1, 0));
}

#[test]
fn a_recipe_uses_up_what_it_needs_or_changes_nothing() {
    let catalog = catalog();
    let recipe = RecipeDesc { makes: ("tonico".into(), 1), needs: vec![("erva".into(), 2)] };
    let mut bag = Inventory::new(4, 4);
    bag.add(&catalog, "erva", 1);
    assert!(!make(&catalog, &mut bag, &recipe), "not enough");
    bag.add(&catalog, "erva", 2);
    assert!(make(&catalog, &mut bag, &recipe));
    assert_eq!((bag.count_of("erva"), bag.count_of("tonico")), (1, 1));

    // With nowhere to put the result, the materials stay.
    let mut full = Inventory::new(1, 1);
    full.add(&catalog, "erva", 2);
    assert!(!make(&catalog, &mut full, &recipe));
    assert_eq!(full.count_of("erva"), 2);
}

#[test]
fn a_shop_pays_a_share_of_the_price_and_at_least_a_coin() {
    let catalog = catalog();
    assert_eq!(sale_price(catalog.get("tonico").unwrap(), 0.5), 5);
    assert_eq!(sale_price(catalog.get("moeda").unwrap(), 0.5), 1);
    let _ = Vec3::ZERO;
}
