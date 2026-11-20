//! The diary tab: the quests, and the animals met. Each page is a list on the left, and what's
//! chosen fills the right. J opens it.

use std::collections::HashMap;

use grimora::portrait::{PortraitShot, Sitter, camera_facing};
use grimora::prelude::*;
use grimora::scene::{load_model, load_species};

use crate::Sandbox;
use crate::bag_screen::PANEL_PAD;

const LINE: Vec4 = Vec4::new(0.62, 0.46, 0.58, 1.0);
const PALE: Vec4 = Vec4::new(0.98, 0.95, 0.84, 1.0);
const TEXT: Vec4 = Vec4::new(0.9, 0.87, 0.76, 1.0);
const DIM: Vec4 = Vec4::new(0.7, 0.68, 0.6, 0.9);
const GOLD: Vec4 = Vec4::new(0.95, 0.8, 0.5, 1.0);

const PAGE: Vec2 = Vec2::new(150.0, 28.0);
/// From the top of the panel to the lists.
const PAGES_HEIGHT: f32 = 52.0;
const LIST_WIDTH: f32 = 400.0;
const ROW: f32 = 30.0;
const GAP: f32 = 64.0;
const BEAST_SIZE: (u32, u32) = (160, 160);
const BEAST_SCALE: f32 = 2.0;
const BEAST_FOV: f32 = 26.0;
const BEAST_TURNS: f32 = 0.6;
const PAGES: [&str; 2] = ["Missões", "Bestiário"];
/// Every animal there is, by the name in its file, with the name to read.
const SPECIES: [(&str, &str); 5] = [
    ("sardinha", "Sardinha"),
    ("gaivota", "Gaivota"),
    ("caranguejo", "Caranguejo"),
    ("arraia", "Arraia"),
    ("agua_viva", "Água-viva"),
];
const FAUNA_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fauna");

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
    pub(crate) fn draw_diary(&mut self, ctx: &mut Context, panel: Rect) {
        let inner = panel.expand(-PANEL_PAD);
        let (left, top) = (inner.min.x, inner.min.y + PAGES_HEIGHT);
        let right = left + LIST_WIDTH + GAP;
        let far = inner.max().x;
        // The rows of a page take the focus first; the names of the pages come after them.
        match self.bag.screen.diary_page {
            0 => self.draw_quests(ctx, left, right, far, top),
            _ => self.draw_bestiary(ctx, left, right, far, top),
        }
        for (at, name) in PAGES.iter().enumerate() {
            let rect = Rect::new(Vec2::new(left + at as f32 * (PAGE.x + 8.0), inner.min.y), PAGE);
            if ctx.ui.button(rect, name) && self.bag.screen.diary_page != at {
                self.bag.screen.diary_page = at;
                ctx.ui.reset_focus();
            }
            if self.bag.screen.diary_page == at {
                let under = Rect::new(Vec2::new(rect.min.x, rect.max().y + 2.0), Vec2::new(PAGE.x, ctx.ui.line()));
                ctx.ui.dither(under, LINE, 0.0, false);
            }
        }
    }

    fn draw_quests(&mut self, ctx: &mut Context, left: f32, right: f32, far: f32, top: f32) {
        let book = self.tale.book.clone();
        let story = &self.tale.story;
        let mut quests: Vec<_> = book.quests.iter().filter(|quest| story.quests.contains_key(&quest.id)).collect();
        quests.sort_by_key(|quest| story.is_done(quest));
        if quests.is_empty() {
            paragraph(&mut ctx.ui, Vec2::new(left, top), LIST_WIDTH, "Nada por fazer. Ainda.", DIM);
            return;
        }
        let width = far - right;
        let mut y = top;
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
        let mut y = ctx.ui.text(Vec2::new(right, top), &shown.name.to_uppercase(), &title).max().y + 20.0;
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

    fn draw_bestiary(&mut self, ctx: &mut Context, left: f32, right: f32, far: f32, top: f32) {
        let mut y = top;
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
            paragraph(&mut ctx.ui, Vec2::new(right, top), 400.0, "Ainda não encontrado.", DIM);
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
            ctx.ui.image(Rect::new(Vec2::new(right, top), size), image, Vec4::ONE);
        }
        let width = (far - right).max(200.0);
        let mut y = top + size.y + 10.0;
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
