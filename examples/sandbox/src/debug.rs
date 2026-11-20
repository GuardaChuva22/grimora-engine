//! The debug menu (F1): the world's hour, clock and weather; flying, the body and its health;
//! anything to spawn (items, models, effects); and the village's insides (what each villager
//! thinks, what each house has to eat, reputation). Q/E change the tab.

use std::path::PathBuf;

use grimora::core::Weather;
use grimora::prelude::*;

use crate::menu::left_margin;
use crate::tale::Panel;
use crate::{Mode, Sandbox};

const TABS: [&str; 4] = ["Mundo", "Jogador", "Criar", "Vila"];
const TAB: Vec2 = Vec2::new(110.0, 24.0);
const TOP: f32 = 24.0;
const ROW: Vec2 = Vec2::new(420.0, 26.0);
const GAP: f32 = 4.0;
const DARK: Vec4 = Vec4::new(0.02, 0.02, 0.04, 0.72);
const PALE: Vec4 = Vec4::new(0.86, 0.84, 0.76, 1.0);
const DIM: Vec4 = Vec4::new(0.7, 0.68, 0.6, 0.9);
const WEATHERS: [(Weather, &str); 5] = [
    (Weather::Clear, "sol"),
    (Weather::Overcast, "nublado"),
    (Weather::Rain, "chuva"),
    (Weather::Storm, "tempestade"),
    (Weather::Snow, "neve"),
];
/// Real seconds a day lasts, for each choice of clock.
const CLOCKS: [(f32, &str); 6] = [
    (0.0, "parado"),
    (5760.0, "normal (96 min)"),
    (480.0, "rápido (8 min)"),
    (120.0, "muito rápido (2 min)"),
    (30.0, "voando (30 s)"),
    (8.0, "relâmpago (8 s)"),
];
const SPEEDS: [f32; 6] = [1.0, 0.5, 0.25, 0.1, 2.0, 4.0];
/// How far the thoughts over villagers' heads are drawn, meters.
const THOUGHTS_REACH: f32 = 25.0;

/// What the debug menu remembers while it is shut.
#[derive(Default)]
pub struct Debug {
    pub tab: usize,
    pub thoughts: bool,
    pub god: bool,
    weather: usize,
    clock: usize,
    speed: usize,
    item: usize,
    model: usize,
    effect: usize,
    place: usize,
    villager: usize,
    models: Vec<PathBuf>,
    effects: Vec<PathBuf>,
    /// The aura files, and which one V lights.
    auras: Vec<PathBuf>,
    pub aura: usize,
    /// `--menu aura:<name>`: the aura to light, and frames to wait first.
    pub scheduled_aura: Option<(String, u32)>,
    /// What the menu spawned, to take away again.
    spawned: Vec<Entity>,
}

/// The files with `ext` in `dir` and the folders under it, sorted.
fn files_in(dir: &std::path::Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = read.filter_map(|e| Some(e.ok()?.path())).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            files_in(&path, ext, out);
        } else if path.extension().is_some_and(|e| e == ext) {
            out.push(path);
        }
    }
}

fn stem(path: &std::path::Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Where a thing put "in front" goes: a few meters ahead of the camera, on whatever is below.
fn in_front(ctx: &Context) -> Vec3 {
    let ahead = ctx.camera.position + ctx.camera.forward() * 3.5;
    ctx.physics.raycast_scenery(ahead + Vec3::Y * 4.0, Vec3::NEG_Y, 40.0).map_or(ahead, |hit| hit.point)
}

impl Sandbox {
    /// F1 opens and shuts the menu; the god mode and the thoughts go on while it's shut.
    pub(crate) fn debug_frame(&mut self, ctx: &mut Context) {
        if ctx.input.just_pressed(KeyCode::F1) {
            if matches!(self.tale.panel, Some(Panel::Debug)) {
                self.tale.panel = None;
            } else if self.tale.panel.is_none() && !self.bag.open {
                self.tale.panel = Some(Panel::Debug);
                ctx.ui.reset_focus();
            }
        }
        if self.debug.god
            && let Some(health) = ctx.world.get_mut::<Health>(self.avatar)
        {
            health.current = health.max;
            health.make_untouchable(0.25);
        }
        if self.debug.thoughts {
            let thoughts = ctx.world.villager_thoughts();
            let size = ctx.ui.size();
            let view = ctx.camera.view_projection(size.x / size.y.max(1.0));
            for (point, text) in thoughts {
                if ctx.camera.position.distance(point) > THOUGHTS_REACH {
                    continue;
                }
                let clip = view * point.extend(1.0);
                if clip.w <= 0.0 {
                    continue;
                }
                let at = Vec2::new((clip.x / clip.w + 1.0) * 0.5 * size.x, (1.0 - clip.y / clip.w) * 0.5 * size.y);
                let lines: Vec<&str> = text.lines().collect();
                let style = TextStyle::new(8.0).color(PALE).anchor(Anchor::Bottom);
                for (k, line) in lines.iter().rev().enumerate() {
                    ctx.ui.text(at - Vec2::new(0.0, k as f32 * 11.0), line, &style);
                }
            }
        }
    }

    pub(crate) fn run_debug(&mut self, ctx: &mut Context) -> Option<Panel> {
        if ctx.input.pad_just_pressed(PadButton::East) {
            return None;
        }
        let step = i32::from(ctx.input.just_pressed(KeyCode::KeyE)) - i32::from(ctx.input.just_pressed(KeyCode::KeyQ));
        if step != 0 {
            self.debug.tab = (self.debug.tab as i32 + step).rem_euclid(TABS.len() as i32) as usize;
            ctx.ui.reset_focus();
        }
        let screen = ctx.ui.screen();
        let side = Rect::new(screen.min, Vec2::new(left_margin(&ctx.ui) * 2.0 + ROW.x, screen.size.y));
        ctx.ui.rect(side, DARK);
        ctx.ui.theme.text_size = 12.0;
        let left = left_margin(&ctx.ui);
        let mut y = TOP + TAB.y + 14.0;
        let mut row = || {
            let rect = Rect::new(Vec2::new(left, y), ROW);
            y += ROW.y + GAP;
            rect
        };
        match self.debug.tab {
            0 => self.debug_world(ctx, &mut row),
            1 => self.debug_player(ctx, &mut row),
            2 => self.debug_spawn(ctx, &mut row),
            _ => self.debug_village(ctx, &mut row),
        }
        for (at, name) in TABS.iter().enumerate() {
            let rect = Rect::new(Vec2::new(left + at as f32 * (TAB.x + 6.0), TOP), TAB);
            if ctx.ui.button(rect, name) && self.debug.tab != at {
                self.debug.tab = at;
                ctx.ui.reset_focus();
            }
            if self.debug.tab == at {
                let under = Rect::new(Vec2::new(rect.min.x, rect.max().y + 2.0), Vec2::new(TAB.x, ctx.ui.line()));
                ctx.ui.dither(under, PALE, 0.0, false);
            }
        }
        let style = TextStyle::new(8.0).color(DIM).anchor(Anchor::Bottom);
        ctx.ui.text(ctx.ui.at(Anchor::Bottom, Vec2::new(0.0, -14.0)), "F1  FECHAR    Q/E  ABA    ENTER  USAR", &style);
        Some(Panel::Debug)
    }

    fn debug_world(&mut self, ctx: &mut Context, row: &mut dyn FnMut() -> Rect) {
        if let Some(mut hour) = ctx.world.time_of_day() {
            let style = TextStyle::new(12.0).color(DIM);
            let now = format!("Agora: {:02}:{:02}", hour as u32, (hour.fract() * 60.0) as u32);
            ctx.ui.text(row().min + Vec2::new(0.0, 6.0), &now, &style);
            // The label is the widget's id: it must not change with the hour, or the slider
            // loses the focus and the drag every minute of game time.
            if ctx.ui.number_slider(row(), "Hora", &mut hour, 0.0..=23.75, 0.25, 2) {
                ctx.world.set_time_of_day(hour);
            }
            for (label, hours) in [("Avançar 1 hora", 1.0), ("Avançar 6 horas", 6.0)] {
                if ctx.ui.button(row(), label) {
                    ctx.world.set_time_of_day((hour + hours).rem_euclid(24.0));
                }
            }
            for (label, at) in [("Ir para 6:00 (manhã)", 6.0), ("Ir para 12:00", 12.0), ("Ir para 20:00 (noite)", 20.0)]
            {
                if ctx.ui.button(row(), label) {
                    ctx.world.set_time_of_day(at);
                }
            }
        } else {
            let style = TextStyle::new(12.0).color(DIM);
            ctx.ui.text(row().min, "Esta cena não tem dia e noite.", &style);
        }
        let clocks: Vec<&str> = CLOCKS.iter().map(|c| c.1).collect();
        if let Some(day) = ctx.world.day_cycle.as_ref() {
            self.debug.clock =
                CLOCKS.iter().position(|c| (c.0 - day.day_length).abs() < 0.5).unwrap_or(self.debug.clock);
        }
        if ctx.ui.choice(row(), "Relógio", &clocks, &mut self.debug.clock)
            && let Some(day) = ctx.world.day_cycle.as_mut()
        {
            day.day_length = CLOCKS[self.debug.clock].0;
        }
        let names: Vec<&str> = WEATHERS.iter().map(|w| w.1).collect();
        if let Some(now) = ctx.world.weather() {
            let style = TextStyle::new(12.0).color(DIM);
            let name = WEATHERS.iter().find(|w| w.0 == now).map_or("?", |w| w.1);
            ctx.ui.text(row().min + Vec2::new(0.0, 6.0), &format!("Clima agora: {name}"), &style);
        }
        ctx.ui.choice(row(), "Clima", &names, &mut self.debug.weather);
        if ctx.ui.button(row(), "Trocar o clima já") {
            ctx.world.set_weather(WEATHERS[self.debug.weather].0);
        }
        if ctx.ui.button(row(), "Trocar o clima devagar (20 s)") {
            ctx.world.change_weather(WEATHERS[self.debug.weather].0, 20.0);
        }
        let mut auto = ctx.world.auto_weather.is_some();
        if ctx.ui.toggle(row(), "Clima automático", &mut auto) {
            ctx.world.auto_weather = auto.then(|| grimora::AutoWeather::warm(ctx.time.frame()));
        }
        // The wind: what it does now, and holding it by hand.
        let air = ctx.world.environment.wind.clone();
        let here = ctx.world.wind_at(ctx.camera.position).length();
        // Where it comes from, on the compass: -Z is north and +X east.
        let from = (-air.blowing.x).atan2(air.blowing.y).to_degrees().rem_euclid(360.0);
        let point = ["N", "NE", "L", "SE", "S", "SO", "O", "NO"][((from + 22.5) / 45.0) as usize % 8];
        let style = TextStyle::new(12.0).color(DIM);
        let now = format!("Vento: {:.1} m/s (aqui {:.1}), vindo de {point}", air.speed_ms, here);
        ctx.ui.text(row().min + Vec2::new(0.0, 6.0), &now, &style);
        let mut held = ctx.world.wind.held.is_some();
        if ctx.ui.toggle(row(), "Segurar o vento", &mut held) {
            let degrees = air.blowing.y.atan2(air.blowing.x).to_degrees();
            ctx.world.wind.held = held.then_some((air.speed_ms, degrees));
        }
        if let Some((mut speed, mut degrees)) = ctx.world.wind.held {
            let a = ctx.ui.number_slider(row(), "Força do vento (m/s)", &mut speed, 0.0..=30.0, 0.5, 1);
            let b = ctx.ui.number_slider(row(), "Rumo do vento (graus)", &mut degrees, -180.0..=180.0, 5.0, 0);
            if a || b {
                ctx.world.wind.held = Some((speed, degrees));
            }
        }
        let speeds: Vec<String> = SPEEDS.iter().map(|s| format!("{:.0}%", s * 100.0)).collect();
        let speeds: Vec<&str> = speeds.iter().map(String::as_str).collect();
        self.debug.speed = SPEEDS.iter().position(|&s| s == ctx.time.scale()).unwrap_or(self.debug.speed);
        if ctx.ui.choice(row(), "Velocidade do mundo", &speeds, &mut self.debug.speed) {
            ctx.time.set_scale(SPEEDS[self.debug.speed]);
        }
        if ctx.ui.button(row(), "Passar um dia (fauna)") {
            ctx.world.pass_fauna_days(1.0, ctx.camera.position);
        }
    }

    fn debug_player(&mut self, ctx: &mut Context, row: &mut dyn FnMut() -> Rect) {
        let mut flying = self.mode == Mode::Fly;
        if ctx.ui.toggle(row(), "Voar (Tab)", &mut flying) {
            self.mode = if flying { Mode::Fly } else { Mode::Play };
        }
        if ctx.ui.button(row(), "Trazer o personagem até a câmera") {
            let at = in_front(ctx);
            if let Some(player) = &mut self.player {
                player.body.position = at + Vec3::Y * 0.2;
            }
            self.mode = Mode::Play;
        }
        ctx.ui.toggle(row(), "Invencível", &mut self.debug.god);
        self.load_auras();
        let auras: Vec<String> = self.debug.auras.iter().map(|p| stem(p)).collect();
        let auras: Vec<&str> = auras.iter().map(String::as_str).collect();
        if !auras.is_empty() {
            self.debug.aura = self.debug.aura.min(auras.len() - 1);
            if ctx.ui.choice(row(), "Aura (V liga, B troca)", &auras, &mut self.debug.aura)
                && ctx.world.has_aura(self.avatar)
            {
                self.light_aura(ctx);
            }
            let lit = ctx.world.has_aura(self.avatar);
            if ctx.ui.button(row(), if lit { "Apagar a aura" } else { "Acender a aura" }) {
                if lit { ctx.world.set_aura(self.avatar, None) } else { self.light_aura(ctx) }
            }
        }
        if ctx.ui.button(row(), "Curar") {
            if let Some(health) = ctx.world.get_mut::<Health>(self.avatar) {
                health.current = health.max;
            }
            if let Some(stamina) = ctx.world.get_mut::<Stamina>(self.avatar) {
                stamina.current = stamina.max;
            }
        }
        if ctx.ui.button(row(), "Dinheiro +100") {
            let catalog = ctx.world.items.clone();
            self.bag.inventory.add(&catalog, "moeda", 100);
        }
        let places = ctx.world.village_places();
        if places.is_empty() {
            let style = TextStyle::new(12.0).color(DIM);
            ctx.ui.text(row().min + Vec2::new(0.0, 6.0), "Nenhuma vila neste mundo.", &style);
            return;
        }
        let names: Vec<&str> = places.iter().map(|p| p.0.as_str()).collect();
        self.debug.place = self.debug.place.min(names.len() - 1);
        ctx.ui.choice(row(), "Lugar", &names, &mut self.debug.place);
        if ctx.ui.button(row(), "Ir até lá") {
            let at = places[self.debug.place].1;
            self.teleport(ctx, at);
        }
    }

    fn load_auras(&mut self) {
        if self.debug.auras.is_empty() {
            files_in(std::path::Path::new("assets/auras"), "ron", &mut self.debug.auras);
        }
    }

    /// Lights the chosen aura on the character, reading the file again so it can be tuned
    /// while playing.
    fn light_aura(&mut self, ctx: &mut Context) {
        let Some(path) = self.debug.auras.get(self.debug.aura) else { return };
        match grimora::scene::load_aura(path) {
            Ok(aura) => {
                ctx.world.set_aura(self.avatar, Some(&aura));
                self.hud.notify(ctx, format!("Aura: {}", aura.name), true);
            }
            Err(error) => self.hud.notify(ctx, format!("Aura quebrada: {error:#}"), false),
        }
    }

    /// V lights or puts out the aura, B lights the next one.
    pub(crate) fn aura_keys(&mut self, ctx: &mut Context) {
        if let Some((name, wait)) = &mut self.debug.scheduled_aura {
            if *wait > 0 {
                *wait -= 1;
            } else {
                let name = name.clone();
                self.debug.scheduled_aura = None;
                self.load_auras();
                if let Some(at) = self.debug.auras.iter().position(|p| stem(p) == name) {
                    self.debug.aura = at;
                    self.light_aura(ctx);
                }
            }
        }
        if ctx.input.just_pressed(KeyCode::KeyV) {
            self.load_auras();
            if ctx.world.has_aura(self.avatar) {
                ctx.world.set_aura(self.avatar, None);
            } else {
                self.light_aura(ctx);
            }
        }
        if ctx.input.just_pressed(KeyCode::KeyB) {
            self.load_auras();
            if !self.debug.auras.is_empty() {
                self.debug.aura = (self.debug.aura + 1) % self.debug.auras.len();
                self.light_aura(ctx);
            }
        }
    }

    /// Puts the character (and the camera with it) at `at`, local.
    fn teleport(&mut self, ctx: &mut Context, at: Vec3) {
        if let Some(player) = &mut self.player {
            player.body.position = at + Vec3::Y * 0.5;
        }
        ctx.camera.position = at + Vec3::new(0.0, 4.0, 6.0);
        self.mode = Mode::Play;
    }

    fn debug_spawn(&mut self, ctx: &mut Context, row: &mut dyn FnMut() -> Rect) {
        let catalog = ctx.world.items.clone();
        let ids: Vec<&str> = catalog.iter().map(|i| i.id.as_str()).collect();
        let names: Vec<&str> = catalog.iter().map(|i| i.name.as_str()).collect();
        if !ids.is_empty() {
            self.debug.item = self.debug.item.min(ids.len() - 1);
            ctx.ui.choice(row(), "Item", &names, &mut self.debug.item);
            if ctx.ui.button(row(), "Pôr 1 na mochila") {
                self.bag.inventory.add(&catalog, ids[self.debug.item], 1);
            }
            if ctx.ui.button(row(), "Pôr 5 na mochila") {
                self.bag.inventory.add(&catalog, ids[self.debug.item], 5);
            }
        }
        if self.debug.models.is_empty() {
            files_in(std::path::Path::new("assets/models/objects"), "glb", &mut self.debug.models);
            files_in(std::path::Path::new("assets/models/savana"), "glb", &mut self.debug.models);
            files_in(std::path::Path::new("assets/effects"), "ron", &mut self.debug.effects);
        }
        let models: Vec<String> = self.debug.models.iter().map(|p| stem(p)).collect();
        let models: Vec<&str> = models.iter().map(String::as_str).collect();
        if !models.is_empty() {
            self.debug.model = self.debug.model.min(models.len() - 1);
            ctx.ui.choice(row(), "Modelo", &models, &mut self.debug.model);
            if ctx.ui.button(row(), "Pôr o modelo na frente") {
                let at = in_front(ctx);
                let yaw = ctx.camera.forward().x.atan2(ctx.camera.forward().z) + std::f32::consts::PI;
                let transform = Transform { position: at, rotation: Quat::from_rotation_y(yaw), scale: Vec3::ONE };
                if let Some(e) = ctx.world.add_model(
                    &self.debug.models[self.debug.model].clone(),
                    &transform,
                    true,
                    &mut ctx.physics,
                ) {
                    self.debug.spawned.push(e);
                }
            }
        }
        let effects: Vec<String> = self.debug.effects.iter().map(|p| stem(p)).collect();
        let effects: Vec<&str> = effects.iter().map(String::as_str).collect();
        if !effects.is_empty() {
            self.debug.effect = self.debug.effect.min(effects.len() - 1);
            ctx.ui.choice(row(), "Efeito", &effects, &mut self.debug.effect);
            if ctx.ui.button(row(), "Acender o efeito na frente") {
                let at = in_front(ctx);
                if let Some(e) = ctx.world.add_fire(
                    &self.debug.effects[self.debug.effect].clone(),
                    Transform::from_position(at),
                    false,
                ) {
                    self.debug.spawned.push(e);
                }
            }
        }
        if ctx.ui.button(row(), &format!("Apagar o que criei ({})", self.debug.spawned.len())) {
            for e in std::mem::take(&mut self.debug.spawned) {
                ctx.world.despawn(e, &mut ctx.physics);
            }
        }
    }

    fn debug_village(&mut self, ctx: &mut Context, row: &mut dyn FnMut() -> Rect) {
        ctx.ui.toggle(row(), "Pensamentos sobre a cabeça", &mut self.debug.thoughts);
        let reputation = ctx.world.village_reputation();
        if ctx.ui.button(row(), &format!("Reputação {reputation}: +20")) {
            ctx.world.change_reputation(20);
        }
        if ctx.ui.button(row(), "Reputação -20") {
            ctx.world.change_reputation(-20);
        }
        if ctx.ui.button(row(), "Encher a despensa de todos") {
            ctx.world.fill_pantries();
        }
        let villagers = ctx.world.villagers();
        if !villagers.is_empty() {
            let names: Vec<String> = villagers.iter().map(|v| format!("{} ({})", v.0, v.1)).collect();
            let names: Vec<&str> = names.iter().map(String::as_str).collect();
            self.debug.villager = self.debug.villager.min(names.len() - 1);
            ctx.ui.choice(row(), "Morador", &names, &mut self.debug.villager);
            if ctx.ui.button(row(), "Ir até ele") {
                let at = villagers[self.debug.villager].2;
                self.teleport(ctx, at + Vec3::new(1.5, 0.0, 1.5));
            }
        }
        let style = TextStyle::new(8.0).color(PALE);
        let header = row();
        ctx.ui.text(header.min + Vec2::new(0.0, 8.0), "DESPENSA       CRU  PRONTO  ÁGUA", &style);
        for (trade, raw, meals, water) in ctx.world.village_pantries() {
            let rect = Rect::new(row().min, Vec2::new(ROW.x, 12.0));
            ctx.ui.text(rect.min, &format!("{trade:<14} {raw:>4.1}  {meals:>5.1}  {water:>4.1}"), &style);
        }
    }
}
