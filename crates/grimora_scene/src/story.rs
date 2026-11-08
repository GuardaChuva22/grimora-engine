//! What can be said and done in the story, as plain data: dialogues, quests, shops and recipes,
//! all read from one file. What has happened so far is in `grimora::story`.

use std::path::Path;

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

/// Something true or not about how the story stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Condition {
    Flag(String),
    NoFlag(String),
    /// The quest hasn't been started.
    QuestNew(String),
    /// The quest is under way, at any stage.
    QuestActive(String),
    /// The quest has reached this stage of its list, or gone past it.
    QuestAt(String, u32),
    QuestDone(String),
    /// The player has this many of the item.
    Has(String, u32),
}

/// What a choice in a dialogue does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Deed {
    Flag(String),
    StartQuest(String),
    Give(String, u32),
    Take(String, u32),
    /// Opens this shop when the dialogue ends.
    Shop(String),
}

/// One thing the player can answer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChoiceDesc {
    pub text: String,
    /// Which node comes next; without one, the dialogue ends.
    pub next: Option<String>,
    /// It's only offered if all of these hold.
    pub when: Vec<Condition>,
    pub does: Vec<Deed>,
}

/// One thing someone says, with the answers to it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NodeDesc {
    pub id: String,
    pub text: String,
    pub choices: Vec<ChoiceDesc>,
}

/// A conversation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DialogueDesc {
    /// The name of the scene object who says it.
    pub object: String,
    /// The name the player reads.
    pub speaker: String,
    /// The first of these whose conditions hold says where to start. Without any, the first node.
    pub openings: Vec<(Vec<Condition>, String)>,
    pub nodes: Vec<NodeDesc>,
}

impl DialogueDesc {
    pub fn node(&self, id: &str) -> Option<&NodeDesc> {
        self.nodes.iter().find(|node| node.id == id)
    }
}

/// A stage of a quest.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StageDesc {
    /// What the player reads in the journal.
    pub text: String,
    /// It ends once all of these hold.
    pub until: Vec<Condition>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct QuestDesc {
    pub id: String,
    pub name: String,
    pub stages: Vec<StageDesc>,
    /// What the player gets at the end.
    pub reward: Vec<(String, u32)>,
}

/// One thing a shop has.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WaresDesc {
    pub item: String,
    /// If not given, the item's own `price`.
    pub price: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShopDesc {
    pub id: String,
    pub name: String,
    pub wares: Vec<WaresDesc>,
    /// The share of an item's price it pays for it.
    pub buys_at: f32,
}

impl Default for ShopDesc {
    fn default() -> Self {
        Self { id: String::new(), name: String::new(), wares: Vec::new(), buys_at: 0.5 }
    }
}

/// How to make something out of other things.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecipeDesc {
    pub makes: (String, u32),
    pub needs: Vec<(String, u32)>,
}

/// The whole file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StoryBook {
    pub dialogues: Vec<DialogueDesc>,
    pub quests: Vec<QuestDesc>,
    pub shops: Vec<ShopDesc>,
    pub recipes: Vec<RecipeDesc>,
}

impl StoryBook {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Self> {
        let options = ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
        Ok(options.from_str(text)?)
    }

    /// What the object of this name has to say.
    pub fn dialogue_of(&self, object: &str) -> Option<&DialogueDesc> {
        self.dialogues.iter().find(|dialogue| dialogue.object == object)
    }

    pub fn quest(&self, id: &str) -> Option<&QuestDesc> {
        self.quests.iter().find(|quest| quest.id == id)
    }

    pub fn shop(&self, id: &str) -> Option<&ShopDesc> {
        self.shops.iter().find(|shop| shop.id == id)
    }
}
