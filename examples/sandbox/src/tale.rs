//! The story's side of the game: what has happened so far (`Tale`), which of its screens is up
//! (`Panel`) and what has to be kept up each frame. A screen stops the character taking orders the
//! same way the inventory does. `--menu dialogue|shop|rest|death|area|journal:<tab>` captures them.

use std::collections::HashMap;
use std::sync::Arc;

use grimora::portrait::{PortraitShot, Sitter, camera_facing, yaw_of};
use grimora::prelude::*;
use grimora::render::ImageId;
use grimora::scene::story::StoryBook;
use grimora::story::{QuestNews, Story};

use crate::Sandbox;
use crate::dialogue::Talk;
use crate::journal::JournalTab;
use crate::rest::Resting;
use crate::shop::Stall;

const BOOK: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/story/story.ron");
pub const OPEN_JOURNAL: &str = "journal";
pub const CONTROL_NAME: (&str, &str) = (OPEN_JOURNAL, "Diário");
/// How often the animals around are looked at, for the bestiary, and how near counts as met.
const SIGHT_EVERY: u64 = 60;
const SIGHT_RADIUS: f32 = 40.0;
/// What the face in the corner of the HUD takes up, in pixels, and how it's framed.
const FACE_SIZE: (u32, u32) = (72, 90);
const FACE_ABOVE_HEAD_JOINT: f32 = 0.09;
const FACE_DISTANCE: f32 = 1.3;
const FACE_FOV: f32 = 14.0;

/// A screen of the story.
pub enum Panel {
    Talk(Talk),
    Journal(JournalTab),
    Shop(Stall),
    Rest(Resting),
}

/// What the game knows about the story, and the pictures to draw it with.
#[derive(Default)]
pub struct Tale {
    pub story: Story,
    pub book: Arc<StoryBook>,
    pub panel: Option<Panel>,
    /// A capture's screen and the frame to open it on.
    pub scheduled: Option<(String, u64)>,
    /// Where the character wakes up after falling: where it last rested.
    pub respawn: Option<(WorldPos, f32)>,
    /// `1..0`, the dark lifting after the character wakes up or rests.
    pub waking: f32,
    pub pictures: HashMap<&'static str, Option<ImageId>>,
    /// Each species' model, once it's been asked for; they go when the world goes.
    pub beasts: crate::journal::Beasts,
}

impl Tale {
    /// Starts over with what a new game has.
    pub fn restart(&mut self) {
        self.story = Story::default();
        self.panel = None;
        self.respawn = None;
        self.waking = 0.0;
    }
}

/// Which rows of a list to draw, starting from `first`, so that the one next to the focus is always
/// there for the arrows to reach. Returns the new `first`.
pub fn scrolled(first: usize, rows: usize, visible: usize, focused: Option<usize>) -> usize {
    let mut first = first.min(rows.saturating_sub(visible));
    if let Some(at) = focused {
        if at <= first {
            first = at.saturating_sub(1);
        } else if at + 2 > first + visible {
            first = (at + 2).saturating_sub(visible).min(rows.saturating_sub(visible));
        }
    }
    first
}

/// Breaks text into lines by hand, so that typing it out letter by letter never moves a word to
/// the next line halfway through.
pub fn wrapped(ui: &Ui, text: &str, style: &TextStyle, width: f32) -> String {
    let mut out = String::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let longer = if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
        if !line.is_empty() && ui.measure(&longer, style).x > width {
            out.push_str(&line);
            out.push('\n');
            line = word.to_owned();
        } else {
            line = longer;
        }
    }
    out.push_str(&line);
    out
}

impl Sandbox {
    pub(crate) fn load_book(&mut self) {
        match StoryBook::load(BOOK) {
            Ok(book) => self.tale.book = Arc::new(book),
            Err(err) => eprintln!("[sandbox] {err:#}"),
        }
    }

    /// Room for a picture that's drawn each frame, once.
    pub(crate) fn picture(&mut self, ctx: &mut Context, name: &'static str, size: (u32, u32)) -> Option<ImageId> {
        *self.tale.pictures.entry(name).or_insert_with(|| ctx.ui.reserve_image(size.0, size.1))
    }

    /// Runs the screen that's up, if any. Returns whether one was, which keeps the character
    /// from taking orders.
    pub(crate) fn run_panel(&mut self, ctx: &mut Context) -> bool {
        self.capture_panel(ctx);
        self.keep_up(ctx);
        if self.dead_for.is_some() {
            self.tale.panel = None;
        }
        let journal_key = ctx.input.action_just_pressed(OPEN_JOURNAL);
        if journal_key && matches!(self.tale.panel, Some(Panel::Journal(_))) {
            self.tale.panel = None;
            return true;
        }
        if self.tale.panel.is_none()
            && journal_key
            && self.mode == crate::Mode::Play
            && self.dead_for.is_none()
            && !self.bag.open
        {
            self.tale.panel = Some(Panel::Journal(JournalTab::default()));
            ctx.ui.reset_focus();
        }
        let Some(panel) = self.tale.panel.take() else { return false };
        let text_size = ctx.ui.theme.text_size;
        let next = match panel {
            Panel::Talk(talk) => self.run_talk(ctx, talk),
            Panel::Journal(tab) => self.run_journal(ctx, tab),
            Panel::Shop(stall) => self.run_shop(ctx, stall),
            Panel::Rest(resting) => self.run_rest(ctx, resting),
        };
        // A choice in a dialogue may have opened something else.
        self.tale.panel = self.tale.panel.take().or(next);
        ctx.ui.theme.text_size = text_size;
        ctx.set_cursor_grab(false);
        self.hud.draw_with_hint(ctx, "");
        true
    }

    /// What goes on whatever is on the screen.
    fn keep_up(&mut self, ctx: &mut Context) {
        let news = self.tale.story.advance(&self.tale.book, &ctx.world.items.clone(), &mut self.bag.inventory);
        self.tell(ctx, news);
        let defense = self.bag.defense(&ctx.world.items.clone());
        if let Some(health) = ctx.world.get_mut::<Health>(self.avatar) {
            health.armor = defense;
        }
        if ctx.time.frame().is_multiple_of(SIGHT_EVERY) {
            for species in ctx.world.fauna.species_near(ctx.camera.position, SIGHT_RADIUS) {
                self.tale.story.saw(&species);
            }
        }
    }

    pub(crate) fn tell(&mut self, ctx: &Context, news: Vec<QuestNews>) {
        for news in news {
            let (id, words) = match &news {
                QuestNews::Started(id) => (id, "Nova missão"),
                QuestNews::Moved(id) => (id, "Diário atualizado"),
                QuestNews::Done(id) => (id, "Missão concluída"),
            };
            let name = self.tale.book.quest(id).map_or("", |quest| quest.name.as_str());
            self.hud.notify(ctx, format!("{words}: {name}"), true);
        }
    }

    /// Starts what this object has to say, if it has anything. Returns whether it did.
    pub(crate) fn start_talk(&mut self, ctx: &mut Context, object: &str) -> bool {
        let book = self.tale.book.clone();
        let Some(dialogue) = book.dialogue_of(object) else { return false };
        let Some(node) = self.tale.story.opening(&book, &self.bag.inventory, dialogue) else { return false };
        self.tale.panel = Some(Panel::Talk(Talk::new(dialogue, node)));
        ctx.ui.reset_focus();
        true
    }

    /// The face in the corner of the HUD.
    pub(crate) fn ask_for_face(&mut self, ctx: &mut Context) {
        let image = self.picture(ctx, "face", FACE_SIZE);
        self.hud.face = image;
        let Some(image) = image else { return };
        let Some(head) = ctx.world.joint_transform(self.avatar, "head") else { return };
        let Some(body) = ctx.world.transform(self.avatar) else { return };
        let point = head.position + Vec3::Y * FACE_ABOVE_HEAD_JOINT;
        let camera = camera_facing(point, yaw_of(&body) + 0.35, FACE_DISTANCE, FACE_FOV);
        ctx.portraits.push(PortraitShot::new(image, Sitter::Entity(self.avatar), camera));
    }

    /// A capture of one of the screens, with something in it to show.
    fn capture_panel(&mut self, ctx: &mut Context) {
        let Some((name, frame)) = &self.tale.scheduled else { return };
        if ctx.time.frame() < *frame {
            return;
        }
        let name = name.clone();
        self.tale.scheduled = None;
        let catalog = ctx.world.items.clone();
        for (item, count) in [("moeda", 83), ("maca", 2), ("erva_amarga", 3), ("sucata_de_ferro", 2), ("gibao", 1)] {
            self.bag.inventory.add(&catalog, item, count);
        }
        ctx.ui.reset_focus();
        match name.as_str() {
            "dialogue" => {
                self.start_talk(ctx, "watcher");
                if let Some(Panel::Talk(talk)) = &mut self.tale.panel {
                    talk.finish();
                }
            }
            "shop" => self.tale.panel = Some(Panel::Shop(Stall::new("vela"))),
            "rest" => self.tale.panel = Some(Panel::Rest(Resting::default())),
            "area" => self.hud.show_area(ctx, "O Círculo de Pedras"),
            "death" => {
                ctx.world.damage(self.avatar, 100_000.0, &mut ctx.physics);
                self.dead_for = Some(2.6);
            }
            journal => {
                let book = self.tale.book.clone();
                let (bag, story) = (&mut self.bag.inventory, &mut self.tale.story);
                story.apply(&book, &catalog, bag, &[grimora::scene::story::Deed::StartQuest("errante".into())]);
                story.apply(&book, &catalog, bag, &[grimora::scene::story::Deed::StartQuest("circulo".into())]);
                story.set_flag("area:circle");
                story.killed("sardinha");
                story.killed("sardinha");
                story.saw("gaivota");
                story.saw("caranguejo");
                let tab = JournalTab::named(journal.strip_prefix("journal:").unwrap_or(""));
                self.tale.panel = Some(Panel::Journal(tab));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::DEFAULT_SCENE;

    const DT: f32 = 1.0 / 60.0;

    /// A save folder, deleted at the end of the test.
    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("grimora_tale_{name}_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            Self(dir)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn start(dir: &Folder) -> (Sandbox, Context) {
        let config = EngineConfig {
            scene: Some(DEFAULT_SCENE.into()),
            save_dir: Some(dir.0.clone()),
            load: Some("none".into()),
            escape_quits: false,
            ..Default::default()
        };
        let mut game = Sandbox::new();
        let mut ctx = Context::headless(config, &mut game).unwrap();
        frames(&mut game, &mut ctx, 30);
        ctx.world.npcs_ignore_player = true;
        (game, ctx)
    }

    fn frames(game: &mut Sandbox, ctx: &mut Context, count: u32) {
        for _ in 0..count {
            ctx.headless_frame(game, DT).unwrap();
        }
    }

    fn press(game: &mut Sandbox, ctx: &mut Context, key: KeyCode) {
        ctx.input.key_down(key);
        frames(game, ctx, 1);
        ctx.input.key_up(key);
        frames(game, ctx, 1);
    }

    /// Lets the words finish and takes the answer with this text.
    fn answer(game: &mut Sandbox, ctx: &mut Context, text: &str) {
        press(game, ctx, KeyCode::Enter);
        frames(game, ctx, 1);
        for _ in 0..8 {
            if ctx.ui.is_focused(text) {
                press(game, ctx, KeyCode::Enter);
                return;
            }
            press(game, ctx, KeyCode::ArrowDown);
        }
        panic!("nobody offered {text:?}");
    }

    #[test]
    fn a_quest_from_the_first_word_to_the_reward_and_through_a_save() {
        let dir = Folder::new("quest");
        let (mut game, mut ctx) = start(&dir);
        let coins = game.bag.inventory.count_of("moeda");

        assert!(game.start_talk(&mut ctx, "watcher"));
        frames(&mut game, &mut ctx, 1);
        answer(&mut game, &mut ctx, "Que pedras?");
        answer(&mut game, &mut ctx, "Eu vou.");
        assert!(game.tale.story.quests.contains_key("circulo"));
        answer(&mut game, &mut ctx, "Obrigado.");
        assert!(game.tale.panel.is_none(), "the conversation is over");
        assert_eq!(game.bag.inventory.count_of("erva_amarga"), 2);

        // Walking into the circle moves it on, whether or not anyone says anything.
        game.tale.story.set_flag("area:circle");
        frames(&mut game, &mut ctx, 2);
        assert_eq!(game.tale.story.quests["circulo"], 1);
        ctx.save_game("story", &game.save_state(&ctx)).unwrap();

        game.tale.story = Story::default();
        assert!(game.load(&mut ctx, "story"));
        assert_eq!(game.tale.story.quests["circulo"], 1, "the save keeps it");

        assert!(game.start_talk(&mut ctx, "watcher"));
        frames(&mut game, &mut ctx, 1);
        answer(&mut game, &mut ctx, "Canta.");
        answer(&mut game, &mut ctx, "Até logo.");
        assert_eq!(game.bag.inventory.count_of("moeda"), coins + 40);
        assert_eq!(game.bag.inventory.count_of("capuz"), 1);
    }

    #[test]
    fn what_is_worn_takes_some_of_each_blow_but_never_all_of_it() {
        let dir = Folder::new("armor");
        let (mut game, mut ctx) = start(&dir);
        let catalog = ctx.world.items.clone();
        game.bag.inventory.add(&catalog, "gibao", 1);
        let piece = game.bag.inventory.first_of("gibao").unwrap().id;
        let cells = game.bag.inventory.cells_taken();

        game.use_piece(&mut ctx, piece);
        assert!(game.bag.inventory.first_of("gibao").is_none(), "it leaves the bag");
        assert!(game.bag.inventory.cells_taken() < cells);
        frames(&mut game, &mut ctx, 1);
        assert_eq!(ctx.world.get::<Health>(game.avatar).unwrap().armor, 5.0);
        let life = ctx.world.get::<Health>(game.avatar).unwrap().current;
        ctx.world.damage(game.avatar, 9.0, &mut ctx.physics);
        assert_eq!(ctx.world.get::<Health>(game.avatar).unwrap().current, life - 4.0);
        ctx.world.get_mut::<Health>(game.avatar).unwrap().current = life;
        ctx.world.damage(game.avatar, 5.0, &mut ctx.physics);
        assert_eq!(ctx.world.get::<Health>(game.avatar).unwrap().current, life - 1.0);

        assert!(game.bag.take_off(&catalog, grimora::scene::ArmorSlot::Body));
        frames(&mut game, &mut ctx, 1);
        assert_eq!(ctx.world.get::<Health>(game.avatar).unwrap().armor, 0.0, "it comes off");
        assert!(game.bag.inventory.first_of("gibao").is_some(), "back in the bag");
    }

    #[test]
    fn a_shop_takes_coins_and_gives_them_back_and_keeps_what_is_in_use() {
        let dir = Folder::new("shop");
        let (mut game, mut ctx) = start(&dir);
        let coins = game.bag.inventory.count_of("moeda");
        game.buy(&mut ctx, "maca", 4);
        assert_eq!(game.bag.inventory.count_of("maca"), 1);
        assert_eq!(game.bag.inventory.count_of("moeda"), coins - 4);
        game.buy(&mut ctx, "gibao", 100_000);
        assert_eq!(game.bag.inventory.count_of("gibao"), 0, "not enough coins");

        game.sell(&mut ctx, "maca", 2);
        assert_eq!(game.bag.inventory.count_of("maca"), 0);
        assert_eq!(game.bag.inventory.count_of("moeda"), coins - 2);

        let held = crate::FIRST_WEAPON;
        game.sell(&mut ctx, held, 5);
        assert_eq!(game.bag.inventory.count_of(held), 1, "it's in the hand");
    }

    #[test]
    fn resting_heals_and_is_where_the_character_wakes_up() {
        let dir = Folder::new("rest");
        let (mut game, mut ctx) = start(&dir);
        ctx.world.get_mut::<Health>(game.avatar).unwrap().current = 1.0;
        let place = game.player.as_ref().unwrap().body.position;

        game.tale.panel = Some(Panel::Rest(Resting::default()));
        ctx.ui.reset_focus();
        frames(&mut game, &mut ctx, 1);
        press(&mut game, &mut ctx, KeyCode::ArrowDown);
        press(&mut game, &mut ctx, KeyCode::Enter);
        frames(&mut game, &mut ctx, 70);
        assert!(game.tale.panel.is_none());
        assert_eq!(ctx.world.get::<Health>(game.avatar).unwrap().fraction(), 1.0);
        assert!(ctx.world.day_cycle.as_ref().is_none_or(|day| (day.hour() - 12.0).abs() < 0.5));
        assert!(game.tale.waking > 0.0, "the dark is lifting");

        // Fall somewhere else, then get up where the fire was.
        let player = game.player.as_mut().unwrap();
        player.body.position += Vec3::new(4.0, 0.0, 0.0);
        ctx.world.damage(game.avatar, 100_000.0, &mut ctx.physics);
        game.dead_for = Some(3.99);
        frames(&mut game, &mut ctx, 3);
        assert!(game.dead_for.is_none());
        let back = game.player.as_ref().unwrap().body.position;
        assert!(back.distance(place) < 1.0, "woke up at {back}, rested at {place}");
    }

    #[test]
    fn the_journal_opens_with_its_key_and_esc_closes_it_without_pausing() {
        let dir = Folder::new("journal");
        let (mut game, mut ctx) = start(&dir);
        press(&mut game, &mut ctx, KeyCode::KeyJ);
        assert!(matches!(game.tale.panel, Some(Panel::Journal(_))));
        press(&mut game, &mut ctx, KeyCode::KeyE);
        assert!(matches!(game.tale.panel, Some(Panel::Journal(_))), "E turns the page and uses nothing");
        press(&mut game, &mut ctx, KeyCode::Escape);
        assert!(game.tale.panel.is_none());
        assert!(!ctx.is_paused());
        press(&mut game, &mut ctx, KeyCode::KeyJ);
        press(&mut game, &mut ctx, KeyCode::KeyJ);
        assert!(game.tale.panel.is_none());
    }

    #[test]
    fn a_list_keeps_the_row_after_the_focus_in_reach() {
        assert_eq!(scrolled(0, 30, 10, Some(0)), 0);
        assert_eq!(scrolled(0, 30, 10, Some(8)), 0);
        assert_eq!(scrolled(0, 30, 10, Some(9)), 1);
        assert_eq!(scrolled(20, 30, 10, Some(29)), 20, "the last row has nothing after it");
        assert_eq!(scrolled(10, 30, 10, Some(10)), 9);
        assert_eq!(scrolled(25, 3, 10, None), 0);
    }
}
