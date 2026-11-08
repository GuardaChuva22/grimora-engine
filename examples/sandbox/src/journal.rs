//! The journal: the character, what they wear and carry; the quests; what can be made; and the
//! animals met. J opens it. Each tab is a list on the left, and what's chosen fills the right.

use std::collections::HashMap;

use grimora::portrait::{PortraitShot, Sitter, camera_facing};
use grimora::prelude::*;
use grimora::scene::{ArmorSlot, load_model, load_species};
use grimora::story::{can_make, make};

use crate::Sandbox;
use crate::menu::left_margin;
use crate::tale::Panel;

const DARK: Vec4 = Vec4::new(0.006, 0.008, 0.03, 0.95);
const LINE: Vec4 = Vec4::new(0.62, 0.46, 0.58, 1.0);
const PALE: Vec4 = Vec4::new(0.98, 0.95, 0.84, 1.0);
const TEXT: Vec4 = Vec4::new(0.9, 0.87, 0.76, 1.0);
const DIM: Vec4 = Vec4::new(0.7, 0.68, 0.6, 0.9);
const GOLD: Vec4 = Vec4::new(0.95, 0.8, 0.5, 1.0);
const RED: Vec4 = Vec4::new(0.95, 0.32, 0.28, 1.0);
const GREEN: Vec4 = Vec4::new(0.42, 0.88, 0.5, 1.0);
const BLUE: Vec4 = Vec4::new(0.45, 0.68, 1.0, 1.0);

const TOP: f32 = 44.0;
const TAB: Vec2 = Vec2::new(150.0, 30.0);
const CONTENT_TOP: f32 = 104.0;
const LIST_WIDTH: f32 = 440.0;
const ROW: f32 = 30.0;
const GAP: f32 = 64.0;
const MARK: f32 = 24.0;
const STAT_ROW: f32 = 34.0;
const WORD_WIDTH: f32 = 120.0;
const BODY_SIZE: (u32, u32) = (140, 260);
const BODY_SCALE: f32 = 2.0;
const BODY_FOV: f32 = 26.0;
const BODY_DISTANCE: f32 = 5.0;
const BODY_MIDDLE: f32 = 0.95;
const BODY_TURNS: f32 = 0.5;
const BEAST_SIZE: (u32, u32) = (160, 160);
const BEAST_SCALE: f32 = 2.0;
const BEAST_FOV: f32 = 26.0;
const BEAST_TURNS: f32 = 0.6;
const RECIPE_BOTTOM: f32 = 480.0;
const KEYS: &str = "ENTER  ESCOLHER    Q/E  ABAS    J  FECHAR";
const KEYS_PAD: &str = "A  ESCOLHER    LB/RB  ABAS    B  FECHAR";
const NAMES: [&str; 4] = ["Ficha", "Missões", "Criar", "Bestiário"];
const SLOTS: [(ArmorSlot, &str); 4] =
    [(ArmorSlot::Head, "Cabeça"), (ArmorSlot::Body, "Peito"), (ArmorSlot::Legs, "Pernas"), (ArmorSlot::Feet, "Pés")];
/// Every animal there is, by the name in its file, with the name to read.
const SPECIES: [(&str, &str); 5] = [
    ("sardinha", "Sardinha"),
    ("gaivota", "Gaivota"),
    ("caranguejo", "Caranguejo"),
    ("arraia", "Arraia"),
    ("agua_viva", "Água-viva"),
];
const FAUNA_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fauna");

/// Which tab is up.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JournalTab(usize);

impl JournalTab {
    /// For `--menu journal:<name>`.
    pub fn named(name: &str) -> Self {
        Self(["ficha", "missoes", "criar", "bestiario"].iter().position(|&tab| tab == name).unwrap_or(0))
    }
}

/// One animal's model, and how to look at it.
#[derive(Clone)]
pub struct Beast {
    mesh: MeshId,
    center: Vec3,
    radius: f32,
    description: String,
}

/// Each species' model, once it's been asked for. They go when the world goes.
pub type Beasts = HashMap<&'static str, Option<Beast>>;

/// A line in the right column, and how far down the next goes.
fn paragraph(ui: &mut Ui, at: Vec2, width: f32, text: &str, color: Vec4) -> f32 {
    let style = TextStyle::new(16.0).color(color).wrap(width);
    ui.text(at, text, &style).max().y
}

impl Sandbox {
    pub(crate) fn run_journal(&mut self, ctx: &mut Context, mut tab: JournalTab) -> Option<Panel> {
        if ctx.input.pad_just_pressed(PadButton::East) {
            return None;
        }
        let step =
            i32::from(ctx.input.just_pressed(KeyCode::KeyE) || ctx.input.pad_just_pressed(PadButton::RightBumper))
                - i32::from(ctx.input.just_pressed(KeyCode::KeyQ) || ctx.input.pad_just_pressed(PadButton::LeftBumper));
        if step != 0 {
            tab.0 = (tab.0 as i32 + step).rem_euclid(NAMES.len() as i32) as usize;
            ctx.ui.reset_focus();
        }
        if self.bag.screen.marks.is_empty() {
            self.bag.screen.marks = crate::bag_screen::draw_marks(&mut ctx.ui);
        }
        let screen = ctx.ui.screen();
        ctx.ui.rect(screen, DARK);
        ctx.ui.theme.text_size = 16.0;
        let left = left_margin(&ctx.ui);

        // The rows of a tab take the focus first; the names of the tabs come after them in the list.
        let right = left + LIST_WIDTH + GAP;
        match tab.0 {
            0 => self.draw_sheet(ctx, left, right),
            1 => self.draw_quests(ctx, left, right),
            2 => self.draw_recipes(ctx, left, right),
            _ => self.draw_bestiary(ctx, left, right),
        }

        for (at, name) in NAMES.iter().enumerate() {
            let rect = Rect::new(Vec2::new(left + at as f32 * (TAB.x + 8.0), TOP), TAB);
            if ctx.ui.button(rect, name) && tab.0 != at {
                tab.0 = at;
                ctx.ui.reset_focus();
            }
            if tab.0 == at {
                let under = Rect::new(Vec2::new(rect.min.x, rect.max().y + 2.0), Vec2::new(TAB.x, ctx.ui.line()));
                ctx.ui.dither(under, LINE, 0.0, false);
            }
        }

        let keys = if ctx.input.gamepad_last() { KEYS_PAD } else { KEYS };
        let style = TextStyle::new(8.0).color(DIM).anchor(Anchor::Bottom);
        ctx.ui.text(ctx.ui.at(Anchor::Bottom, Vec2::new(0.0, -14.0)), keys, &style);
        Some(Panel::Journal(tab))
    }

    /// What they wear, what they can take, and the whole body turning slowly.
    fn draw_sheet(&mut self, ctx: &mut Context, left: f32, right: f32) {
        let catalog = ctx.world.items.clone();
        let mut y = CONTENT_TOP;
        let name_of = |id: u32, bag: &crate::bag::Bag| {
            bag.inventory.piece(id).and_then(|piece| catalog.get(&piece.stack.item)).map(|item| item.name.clone())
        };
        for (slot, word) in SLOTS {
            let worn = self.bag.worn_in(slot).and_then(|piece| catalog.get(&piece.stack.item));
            let value = worn.map_or_else(|| "-".to_owned(), |item| item.name.clone());
            let row = Rect::new(Vec2::new(left, y), Vec2::new(LIST_WIDTH, ROW - 2.0));
            if ctx.ui.button_row(row, word, &value) && !self.bag.next_worn(&catalog, slot) {
                self.hud.notify(ctx, crate::bag::NO_ROOM, false);
            }
            y += ROW;
        }
        let hand = self.bag.inventory.hand().and_then(|id| name_of(id, &self.bag));
        let row = Rect::new(Vec2::new(left, y), Vec2::new(LIST_WIDTH, ROW - 2.0));
        ctx.ui.button_row_if(false, row, "Na mão", hand.as_deref().unwrap_or("-"));
        y += ROW + 22.0;

        let (life, most) = ctx.world.get::<Health>(self.avatar).map_or((0.0, 0.0), |h| (h.current, h.max));
        let breath = ctx.world.get::<Stamina>(self.avatar).map_or(0.0, |s| s.max);
        let blow = ctx.world.get::<Fighter>(self.avatar).map_or(0.0, |f| f.weapon.damage);
        let defense = self.bag.defense(&catalog);
        let coins = self.bag.inventory.count_of("moeda");
        let stats = [
            (3, RED, format!("{life:.0} / {most:.0}"), "VIDA"),
            (1, GREEN, format!("{breath:.0}"), "FÔLEGO"),
            (0, RED, format!("{blow:.0}"), "DANO"),
            (crate::bag_screen::MARK_SHIELD, BLUE, format!("{defense:.0}"), "DEFESA"),
            (crate::bag_screen::MARK_COIN, GOLD, coins.to_string(), "MOEDAS"),
        ];
        for (mark, color, value, word) in stats {
            if let Some(&image) = self.bag.screen.marks.get(mark) {
                ctx.ui.image(Rect::new(Vec2::new(left, y), Vec2::splat(MARK)), image, color);
            }
            let small = TextStyle::new(8.0).color(DIM).anchor(Anchor::Left);
            ctx.ui.text(Vec2::new(left + MARK + 10.0, y + MARK * 0.5), word, &small);
            let number = TextStyle::new(16.0).color(color).anchor(Anchor::Left);
            ctx.ui.text(Vec2::new(left + MARK + 10.0 + WORD_WIDTH, y + MARK * 0.5), &value, &number);
            y += STAT_ROW;
        }

        let Some(image) = self.picture(ctx, "body", BODY_SIZE) else { return };
        let Some(body) = ctx.world.transform(self.avatar) else { return };
        let turn = ctx.time.elapsed() * BODY_TURNS;
        let middle = body.position + Vec3::Y * BODY_MIDDLE;
        let camera = camera_facing(middle, turn, BODY_DISTANCE, BODY_FOV);
        ctx.portraits.push(PortraitShot::new(image, Sitter::Entity(self.avatar), camera));
        let size = Vec2::new(BODY_SIZE.0 as f32, BODY_SIZE.1 as f32) * BODY_SCALE;
        let at = Vec2::new(right + 40.0, (ctx.ui.size().y - size.y) * 0.5 + 34.0).floor();
        ctx.ui.image(Rect::new(at, size), image, Vec4::ONE);
    }

    fn draw_quests(&mut self, ctx: &mut Context, left: f32, right: f32) {
        let book = self.tale.book.clone();
        let story = &self.tale.story;
        let mut quests: Vec<_> = book.quests.iter().filter(|quest| story.quests.contains_key(&quest.id)).collect();
        quests.sort_by_key(|quest| story.is_done(quest));
        if quests.is_empty() {
            paragraph(&mut ctx.ui, Vec2::new(left, CONTENT_TOP), LIST_WIDTH, "Nada por fazer. Ainda.", DIM);
            return;
        }
        let width = ctx.ui.size().x - right - left;
        let mut y = CONTENT_TOP;
        let mut shown = quests[0];
        for quest in &quests {
            let row = Rect::new(Vec2::new(left, y), Vec2::new(LIST_WIDTH, ROW - 2.0));
            ctx.ui.button_row(row, &quest.name, if story.is_done(quest) { "Concluída" } else { "" });
            if ctx.ui.is_focused(&quest.name) {
                shown = quest;
            }
            y += ROW;
        }

        let at = story.quests.get(&shown.id).copied().unwrap_or(0) as usize;
        let title = TextStyle::new(24.0).color(PALE).wrap(width);
        let mut y = ctx.ui.text(Vec2::new(right, CONTENT_TOP), &shown.name.to_uppercase(), &title).max().y + 20.0;
        for (stage, step) in shown.stages.iter().enumerate().take(at + 1) {
            let color = if stage < at { DIM } else { TEXT };
            y = paragraph(&mut ctx.ui, Vec2::new(right, y), width, &step.text, color) + 14.0;
        }
        if !shown.reward.is_empty() {
            y += 12.0;
            let small = TextStyle::new(8.0).color(DIM);
            ctx.ui.text(Vec2::new(right, y), "RECOMPENSA", &small);
            y += 20.0;
            let catalog = ctx.world.items.clone();
            for (item, count) in &shown.reward {
                let name = catalog.get(item).map_or(item.as_str(), |item| item.name.as_str());
                let text = if *count > 1 { format!("{name} x{count}") } else { name.to_owned() };
                y = paragraph(&mut ctx.ui, Vec2::new(right, y), width, &text, GOLD) + 4.0;
            }
        }
    }

    fn draw_recipes(&mut self, ctx: &mut Context, left: f32, right: f32) {
        let book = self.tale.book.clone();
        let catalog = ctx.world.items.clone();
        let mut y = CONTENT_TOP;
        let mut shown = book.recipes.first();
        let mut made = None;
        for recipe in &book.recipes {
            let name = catalog.get(&recipe.makes.0).map_or(recipe.makes.0.as_str(), |item| item.name.as_str());
            let value = if can_make(&self.bag.inventory, recipe) { "Pronto" } else { "" };
            let label = if recipe.makes.1 > 1 { format!("{name} x{}", recipe.makes.1) } else { name.to_owned() };
            let row = Rect::new(Vec2::new(left, y), Vec2::new(LIST_WIDTH, ROW - 2.0));
            if ctx.ui.button_row(row, &label, value) {
                made = Some(recipe);
            }
            if ctx.ui.is_focused(&label) {
                shown = Some(recipe);
            }
            y += ROW;
        }
        if let Some(recipe) = made {
            let name = catalog.get(&recipe.makes.0).map_or(String::new(), |item| item.name.clone());
            if make(&catalog, &mut self.bag.inventory, recipe) {
                self.hud.notify(ctx, format!("Feito: {name}"), true);
            } else if can_make(&self.bag.inventory, recipe) {
                self.hud.notify(ctx, "Não cabe mais nada", false);
            } else {
                self.hud.notify(ctx, "Falta material", false);
            }
        }
        let Some(recipe) = shown else { return };

        y += 22.0;
        ctx.ui.text(Vec2::new(left, y), "PRECISA DE", &TextStyle::new(8.0).color(DIM));
        y += 20.0;
        for (item, count) in &recipe.needs {
            let name = catalog.get(item).map_or(item.as_str(), |item| item.name.as_str());
            let has = self.bag.inventory.count_of(item);
            let color = if has >= *count { TEXT } else { RED };
            ctx.ui.text(Vec2::new(left, y), name, &TextStyle::new(16.0).color(color));
            let numbers = TextStyle::new(16.0).color(color).anchor(Anchor::TopRight);
            ctx.ui.text(Vec2::new(left + LIST_WIDTH, y), &format!("{has} / {count}"), &numbers);
            y += 26.0;
        }
        self.draw_item_details(ctx, right, CONTENT_TOP, RECIPE_BOTTOM, &recipe.makes.0);
    }

    fn draw_bestiary(&mut self, ctx: &mut Context, left: f32, right: f32) {
        let mut y = CONTENT_TOP;
        let mut shown = None;
        for (at, (id, name)) in SPECIES.iter().enumerate() {
            let sighting = self.tale.story.bestiary.get(*id).copied().unwrap_or_default();
            // Unknown ones are told apart by their spaces, which nobody can see.
            let label = if sighting.seen { (*name).to_owned() } else { format!("???{}", " ".repeat(at)) };
            let value = if sighting.killed > 0 { format!("x{}", sighting.killed) } else { String::new() };
            let row = Rect::new(Vec2::new(left, y), Vec2::new(LIST_WIDTH, ROW - 2.0));
            ctx.ui.button_row(row, &label, &value);
            if ctx.ui.is_focused(&label) {
                shown = Some((*id, *name, sighting));
            }
            y += ROW;
        }
        let Some((id, name, sighting)) = shown else { return };
        if !sighting.seen {
            paragraph(&mut ctx.ui, Vec2::new(right, CONTENT_TOP), 400.0, "Ainda não encontrado.", DIM);
            return;
        }
        let Some(beast) = self.beast(ctx, id) else { return };

        let size = Vec2::new(BEAST_SIZE.0 as f32, BEAST_SIZE.1 as f32) * BEAST_SCALE;
        if let Some(image) = self.picture(ctx, "beast", BEAST_SIZE) {
            let turn = Quat::from_rotation_y(ctx.time.elapsed() * BEAST_TURNS);
            let transform = Transform { position: -(turn * beast.center), rotation: turn, scale: Vec3::ONE };
            let distance = beast.radius / (BEAST_FOV.to_radians() * 0.5).tan() * 1.1;
            let mut camera = camera_facing(Vec3::ZERO, 0.0, distance, BEAST_FOV);
            camera.position.y += distance * 0.35;
            camera.look_at(Vec3::ZERO);
            camera.near = distance * 0.1;
            camera.far = distance * 4.0;
            ctx.portraits.push(PortraitShot::new(image, Sitter::Mesh(beast.mesh, transform), camera));
            ctx.ui.image(Rect::new(Vec2::new(right, CONTENT_TOP), size), image, Vec4::ONE);
        }
        let width = (ctx.ui.size().x - right - left).max(200.0);
        let mut y = CONTENT_TOP + size.y + 10.0;
        let title = TextStyle::new(24.0).color(PALE);
        ctx.ui.text(Vec2::new(right, y), &name.to_uppercase(), &title);
        y += 36.0;
        y = paragraph(&mut ctx.ui, Vec2::new(right, y), width, &beast.description, TEXT) + 10.0;
        if sighting.killed > 0 {
            let text = format!("ABATIDOS  {}", sighting.killed);
            ctx.ui.text(Vec2::new(right, y), &text, &TextStyle::new(8.0).color(DIM));
        }
    }

    fn beast(&mut self, ctx: &mut Context, id: &'static str) -> Option<Beast> {
        if let Some(known) = self.tale.beasts.get(id) {
            return known.clone();
        }
        let dir = std::path::Path::new(FAUNA_DIR);
        let loaded = load_species(dir.join(format!("{id}.ron"))).and_then(|species| {
            let mesh = load_model(&dir.join(&species.model), grimora::scene::DEFAULT_MAX_TEXTURE_SIZE)?;
            let sphere = mesh.bounding_sphere();
            let mesh = ctx.world.add_mesh(mesh);
            Ok(Beast { mesh, center: sphere.center, radius: sphere.radius.max(0.01), description: species.description })
        });
        let beast = loaded.map_err(|err| eprintln!("[sandbox] {err:#}")).ok();
        self.tale.beasts.insert(id, beast.clone());
        beast
    }
}
