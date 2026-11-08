//! How the story stands: flags, quests and which animals have been met. It goes in the saved
//! game. What can happen is in [`grimora_scene::story`].

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use grimora_scene::story::{Condition, Deed, DialogueDesc, QuestDesc, RecipeDesc, StoryBook};
use grimora_scene::{ItemCatalog, ItemDesc};
use serde::{Deserialize, Serialize};

use crate::items::Inventory;

/// What's known about a kind of animal.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sighting {
    pub seen: bool,
    pub killed: u32,
}

/// Something that changed in a quest, for the game to tell the player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuestNews {
    Started(String),
    /// Moved on to its next stage.
    Moved(String),
    Done(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Story {
    pub flags: BTreeSet<String>,
    /// The stage each started quest is at. Past the last one, it's done.
    pub quests: BTreeMap<String, u32>,
    /// By species name.
    pub bestiary: BTreeMap<String, Sighting>,
}

impl Story {
    pub fn set_flag(&mut self, flag: impl Into<String>) {
        self.flags.insert(flag.into());
    }

    pub fn is_done(&self, quest: &QuestDesc) -> bool {
        self.quests.get(&quest.id).is_some_and(|&stage| stage as usize >= quest.stages.len())
    }

    pub fn holds(&self, book: &StoryBook, bag: &Inventory, condition: &Condition) -> bool {
        match condition {
            Condition::Flag(flag) => self.flags.contains(flag),
            Condition::NoFlag(flag) => !self.flags.contains(flag),
            Condition::QuestNew(id) => !self.quests.contains_key(id),
            Condition::QuestActive(id) => {
                self.quests.contains_key(id) && !book.quest(id).is_some_and(|quest| self.is_done(quest))
            }
            Condition::QuestAt(id, stage) => self.quests.get(id).is_some_and(|at| at >= stage),
            Condition::QuestDone(id) => book.quest(id).is_some_and(|quest| self.is_done(quest)),
            Condition::Has(item, count) => bag.count_of(item) >= *count,
        }
    }

    pub fn all_hold(&self, book: &StoryBook, bag: &Inventory, conditions: &[Condition]) -> bool {
        conditions.iter().all(|condition| self.holds(book, bag, condition))
    }

    /// Which node a conversation starts on.
    pub fn opening<'a>(&self, book: &StoryBook, bag: &Inventory, dialogue: &'a DialogueDesc) -> Option<&'a str> {
        let chosen = dialogue.openings.iter().find(|(when, _)| self.all_hold(book, bag, when));
        chosen.map(|(_, node)| node.as_str()).or(dialogue.nodes.first().map(|node| node.id.as_str()))
    }

    /// Does what a choice says. Returns the shop to open, if any, and what the player should hear.
    pub fn apply(
        &mut self,
        book: &StoryBook,
        catalog: &ItemCatalog,
        bag: &mut Inventory,
        deeds: &[Deed],
    ) -> (Option<String>, Vec<QuestNews>) {
        let mut shop = None;
        let mut news = Vec::new();
        for deed in deeds {
            match deed {
                Deed::Flag(flag) => self.set_flag(flag.clone()),
                Deed::StartQuest(id) => {
                    if book.quest(id).is_some() && !self.quests.contains_key(id) {
                        self.quests.insert(id.clone(), 0);
                        news.push(QuestNews::Started(id.clone()));
                    }
                }
                Deed::Give(item, count) => {
                    bag.add(catalog, item, *count);
                }
                Deed::Take(item, count) => {
                    bag.take_item(item, *count);
                }
                Deed::Shop(id) => shop = Some(id.clone()),
            }
        }
        news.extend(self.advance(book, catalog, bag));
        (shop, news)
    }

    /// Moves every quest on as far as it can go, paying what the finished ones pay.
    pub fn advance(&mut self, book: &StoryBook, catalog: &ItemCatalog, bag: &mut Inventory) -> Vec<QuestNews> {
        let mut news = Vec::new();
        for quest in &book.quests {
            let Some(&first) = self.quests.get(&quest.id) else { continue };
            let mut stage = first as usize;
            while let Some(now) = quest.stages.get(stage) {
                if !self.all_hold(book, bag, &now.until) {
                    break;
                }
                stage += 1;
                if stage == quest.stages.len() {
                    for (item, count) in &quest.reward {
                        bag.add(catalog, item, *count);
                    }
                    news.push(QuestNews::Done(quest.id.clone()));
                } else {
                    news.push(QuestNews::Moved(quest.id.clone()));
                }
                self.quests.insert(quest.id.clone(), stage as u32);
            }
        }
        news
    }

    pub fn saw(&mut self, species: &str) {
        self.bestiary.entry(species.to_owned()).or_default().seen = true;
    }

    pub fn killed(&mut self, species: &str) {
        let sighting = self.bestiary.entry(species.to_owned()).or_default();
        sighting.seen = true;
        sighting.killed += 1;
    }
}

/// How many times over the bag has what the recipe needs.
pub fn can_make(bag: &Inventory, recipe: &RecipeDesc) -> bool {
    recipe.needs.iter().all(|(item, count)| bag.count_of(item) >= *count)
}

/// Makes it, if everything's there and the result fits. Returns whether it did.
pub fn make(catalog: &ItemCatalog, bag: &mut Inventory, recipe: &RecipeDesc) -> bool {
    if !can_make(bag, recipe) {
        return false;
    }
    let before = bag.clone();
    for (item, count) in &recipe.needs {
        bag.take_item(item, *count);
    }
    if bag.add(catalog, &recipe.makes.0, recipe.makes.1) > 0 {
        *bag = before;
        return false;
    }
    true
}

/// What a shop pays for an item.
pub fn sale_price(item: &ItemDesc, buys_at: f32) -> u32 {
    ((item.price as f32 * buys_at).floor() as u32).max(u32::from(item.price > 0))
}
