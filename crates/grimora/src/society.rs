//! The village's people with each other and with the player: what they say, whom they seek
//! and avoid, how they greet, scold, cry thief and fight back, and what the village thinks of
//! the player (`GenState::reputation`, saved).

use glam::Vec3;
use grimora_physics::Physics;
use grimora_scene::{Behavior, CombatDesc, Notice, Reaction, Trade, Weapon};
use grimora_world::{Kin, Resident, path_seed};

use crate::Health;
use crate::folk::Villager;
use crate::npc::{Agent, Brain};
use crate::world::{Entity, World};

/// How long a line stays over a head, seconds.
pub(crate) const SAYING: f32 = 4.5;
/// Seconds a scared villager runs away; seconds a strong one fights back.
pub(crate) const FEAR: f32 = 25.0;
pub(crate) const ANGER: f32 = 45.0;
/// How long the village stays alarmed about the player after a blow, and after a death,
/// seconds (a game hour is 20).
const ALARM: f32 = 180.0;
pub(crate) const MURDER_ALARM: f32 = 600.0;
/// How far eyes see by day and by night, and how close someone is felt without being seen.
const SIGHT: f32 = 18.0;
const SIGHT_NIGHT: f32 = 8.0;
const FELT: f32 = 2.5;
/// A blow this close wakes whoever sleeps.
const WAKES: f32 = 10.0;

/// Whether eyes at `eyes`, turned to `facing`, see `target`: near enough for the light there
/// is, in front (or close enough to be felt), and with nothing in between.
pub(crate) fn sees(physics: &Physics, eyes: Vec3, facing: f32, target: Vec3, night: bool) -> bool {
    let to = Vec3::new(target.x - eyes.x, 0.0, target.z - eyes.z);
    let d = to.length();
    if d > if night { SIGHT_NIGHT } else { SIGHT } {
        return false;
    }
    let ahead = Vec3::new(-facing.sin(), 0.0, -facing.cos());
    (d < FELT || to.dot(ahead) > -0.34 * d) && !physics.line_blocked(eyes, target)
}
/// Who sees a blow or a theft, meters.
const WITNESS: f32 = 25.0;
const SEES_THEFT: f32 = 10.0;

/// What one person is to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Relation {
    Family,
    Friend,
    Foe,
    Neighbour,
}

/// A line over someone's head.
#[derive(Debug, Clone)]
pub(crate) struct Saying {
    pub who: usize,
    pub text: String,
    pub left: f32,
}

fn pick(lines: &[&'static str], roll: u64) -> &'static str {
    lines[(roll % lines.len() as u64) as usize]
}

/// Something `speaker` says to a neighbour they are standing with.
pub(crate) fn chat_line(speaker: &Resident, relation: Relation, roll: u64) -> &'static str {
    let by_relation: &[&str] = match relation {
        Relation::Family => &[
            "Já pôs a água no fogo?",
            "Volte antes de escurecer.",
            "Comeu alguma coisa hoje?",
            "Amanhã você me ajuda.",
        ],
        Relation::Friend => &[
            "Bebe uma comigo mais tarde?",
            "Ha! Essa é boa.",
            "Ouviu o que o vento trouxe do mar?",
            "Guardei um lugar pra você.",
        ],
        Relation::Foe => {
            &["Hmpf.", "Não tenho nada pra falar com você.", "Ainda me deve aquilo.", "Saia da minha frente."]
        }
        Relation::Neighbour => &[
            "Bom tempo pra secar o peixe.",
            "A chuva anda demorando.",
            "Dizem que viram luzes na falésia.",
            "O poço baixou de novo.",
        ],
    };
    let by_trade: &[&str] = match speaker.trade {
        Trade::Smith => &["O ferro não espera.", "Preciso de mais carvão.", "Essa lâmina ainda dá trabalho."],
        Trade::Fisher => &["O riacho anda cheio de peixe.", "A rede rasgou de novo.", "Os peixes sobem com a lua."],
        Trade::Tavern => &["Hoje a taverna enche.", "O barril novo está bom.", "Alguém não pagou ontem."],
        Trade::Healer => &["Essa tosse não me agrada.", "Falta erva amarga.", "Beba mais água, ouviu?"],
        Trade::Chief => &["Precisamos reforçar a cerca.", "Os forasteiros andam muitos.", "A vila vai bem, por ora."],
        Trade::Home => &["A roça pede chuva.", "O varal secou cedo.", "As galinhas sumiram de novo."],
        Trade::Herder => {
            &["A cabra preta fugiu de novo.", "O leite rendeu hoje.", "Tem bicho rondando o curral à noite."]
        }
        Trade::Sexton => &["Cavei mais uma ontem.", "Os mortos não reclamam.", "A terra do cemitério anda mole."],
        Trade::Carpenter => {
            &["Essa viga não aguenta outro inverno.", "Me falta madeira boa.", "Fiz o caixão do velho em dois dias."]
        }
    };
    if relation == Relation::Foe || roll % 2 == 0 { pick(by_relation, roll / 2) } else { pick(by_trade, roll / 2) }
}

/// A greeting for the player passing by, by the hour and the village's opinion.
pub(crate) fn greeting(reputation: i32, hour: f32, roll: u64) -> &'static str {
    match reputation {
        r if r >= 30 => pick(&["Que bom te ver, amigo!", "Seja bem-vindo de volta!", "Olá, amigo da vila!"], roll),
        r if r >= -15 => {
            if hour < 12.0 {
                pick(&["Bom dia.", "Bom dia, forasteiro.", "Dia."], roll)
            } else {
                pick(&["Boa tarde.", "Boa tarde, forasteiro.", "Tarde."], roll)
            }
        }
        r if r >= -50 => pick(&["Hmpf. Você de novo.", "...", "Olho em você."], roll),
        _ => pick(&["Fique longe de mim.", "Não chegue perto!", "Vá embora daqui."], roll),
    }
}

/// Whether `resident` fights back when someone is attacked: grown men of the forge and the
/// chief's house.
pub(crate) fn strong(resident: &Resident) -> bool {
    matches!(resident.trade, Trade::Smith | Trade::Chief)
        && matches!(resident.kin, Kin::Head | Kin::Sibling | Kin::Spouse)
        && !resident.woman
        && (18..=60).contains(&resident.age)
}

/// Health for every villager, and a fighter with a blow for the strong ones.
pub(crate) fn combat_of(resident: &Resident) -> CombatDesc {
    let weapon = strong(resident).then(|| Weapon {
        damage: 8.0,
        reach: 1.7,
        arc: 90.0,
        cooldown: 2.0,
        clip: "sword_1".into(),
        // `sword_1` has no `hit`: without this the blow landed at the clip's end.
        event: "hit_on".into(),
        ..Default::default()
    });
    CombatDesc { health: if strong(resident) { 45.0 } else { 25.0 }, team: 2, radius: 0.4, weapon }
}

/// A rumor about someone else of the village.
pub(crate) fn rumor(about: &Resident, foe: Option<&Resident>, roll: u64) -> String {
    let name = &about.name;
    if let Some(foe) = foe {
        return format!("{name} e {} não se falam desde a última festa. Ninguém sabe bem por quê.", foe.name);
    }
    let lines = [
        format!("{name} jura que viu luzes na falésia, de noite."),
        format!("Dizem que {name} anda guardando moedas debaixo da cama."),
        format!("{name} anda de olho na roça dos outros, eu acho."),
        format!("Ouvi que {name} quer ir embora para a costa."),
        format!("{name} canta sozinho quando pensa que ninguém escuta."),
    ];
    lines[(roll % lines.len() as u64) as usize].clone()
}

/// What talking to a villager needs.
pub struct VillagerTalk {
    pub name: String,
    pub role: String,
    /// The trade's word: the dialogue `oficio:<trade>` in the story book says the rest.
    pub trade: &'static str,
    pub greeting: String,
    pub rumor: String,
    /// The village thinks so little of the player that they won't talk.
    pub refuses: bool,
}

impl World {
    /// What the village thinks of the player, -100..100.
    pub fn village_reputation(&self) -> i32 {
        self.generated.reputation
    }

    /// Changes it by `by`, kept in -100..100.
    pub fn change_reputation(&mut self, by: i32) {
        self.generated.reputation = (self.generated.reputation + by).clamp(-100, 100);
    }

    /// The game says each frame whether the player has a weapon in the hand: villagers who
    /// see it grow wary, warn, step back, and children run.
    pub fn player_holds_weapon(&mut self, armed: bool) {
        if let Some(land) = self.land.as_mut() {
            land.folk.player_armed = armed;
        }
    }

    /// Whether the village is alarmed about the player now.
    pub fn village_alarmed(&self) -> bool {
        self.ground().is_some_and(|land| land.folk.alarm > 0.0)
    }

    /// The lines over the villagers' heads now: where (local) and what.
    pub fn villager_lines(&self) -> Vec<(Vec3, String)> {
        let Some(land) = self.ground() else { return Vec::new() };
        land.folk
            .says
            .iter()
            .filter_map(|saying| {
                let entity = land.folk.entity_of(saying.who)?;
                let transform = self.ecs.get::<&grimora_core::Transform>(entity).ok()?;
                Some((transform.position + Vec3::Y * (1.95 * transform.scale.y + 0.25), saying.text.clone()))
            })
            .collect()
    }

    /// What `entity`, a villager, says when spoken to.
    pub fn villager_talk(&self, entity: Entity) -> Option<VillagerTalk> {
        let villager = self.ecs.get::<&Villager>(entity).ok()?;
        let r = &villager.resident;
        let land = self.ground()?;
        // The dead say nothing, and neither does someone asleep.
        if self.ecs.get::<&Health>(entity).is_ok_and(|h| h.is_dead())
            || land.folk.people.iter().position(|p| p.resident.id == r.id).is_some_and(|i| land.folk.asleep(i))
        {
            return None;
        }
        let reputation = self.generated.reputation;
        let hour = self.time_of_day().unwrap_or(9.0);
        let day = (hour / 24.0) as u64;
        let roll = path_seed(&[r.id.0, day, reputation as u64]);
        let (about, foe) = land.folk.rumor_about(r.id, roll);
        Some(VillagerTalk {
            name: r.name.clone(),
            role: r.role.clone(),
            trade: r.trade.name(),
            greeting: greeting(reputation, hour, roll).to_owned(),
            rumor: about.map_or_else(|| "Por aqui nada acontece.".to_owned(), |about| rumor(about, foe, roll >> 8)),
            refuses: reputation < -40,
        })
    }

    /// The player opened the chest called `key` at `player` (local): if it is a house's and a
    /// villager is close enough to see, they cry thief and the village thinks less of the
    /// player. Returns whether someone saw.
    pub fn villagers_see_theft(&mut self, key: &str, player: Vec3, physics: &Physics) -> bool {
        if !key.starts_with("house:") {
            return false;
        }
        let Some(land) = self.land.as_mut() else { return false };
        let mut seen = false;
        for i in 0..land.folk.people.len() {
            let Some(entity) = land.folk.entity_of(i) else { continue };
            // Only eyes that are open see it.
            if land.folk.asleep(i) || self.ecs.get::<&Health>(entity).is_ok_and(|h| h.is_dead()) {
                continue;
            }
            let Ok(transform) = self.ecs.get::<&grimora_core::Transform>(entity) else { continue };
            let eyes = transform.position + Vec3::Y * 1.6 * transform.scale.y;
            if eyes.distance(player) > SEES_THEFT || physics.line_blocked(eyes, player + Vec3::Y * 1.2) {
                continue;
            }
            drop(transform);
            let line = ["Ladrão! Isso não é seu!", "Ei! Largue isso!", "Vou contar pra todo mundo!"][i % 3];
            land.folk.say(i, line);
            seen = true;
        }
        if seen {
            self.change_reputation(-15);
        }
        seen
    }

    /// The player struck `target`, a villager: the village thinks much less of them, the
    /// strong ones near fight back ([`Reaction::Attack`]) and the rest run away.
    pub fn villager_attacked(&mut self, target: Entity, player: Vec3, physics: &mut Physics) {
        let Some(victim) = self.ecs.get::<&grimora_core::Transform>(target).ok().map(|t| t.position) else { return };
        self.change_reputation(-30);
        let Some(land) = self.land.as_mut() else { return };
        land.folk.alarm = land.folk.alarm.max(ALARM);
        let mut brains = Vec::new();
        for i in 0..land.folk.people.len() {
            let Some(entity) = land.folk.entity_of(i) else { continue };
            let Ok((agent, transform, health)) =
                self.ecs.query_one_mut::<(&mut Agent, &grimora_core::Transform, Option<&Health>)>(entity)
            else {
                continue;
            };
            if health.is_some_and(|h| h.is_dead()) || transform.position.distance(victim) > WITNESS {
                continue;
            }
            let here = transform.position;
            // Asleep and far from it, they hear nothing.
            if land.folk.asleep(i) && here.distance(victim) > WAKES {
                continue;
            }
            // Whatever they were doing stops: up from the chair, awake from the bed.
            land.folk.forget(i);
            agent.rest_clip = None;
            let person = &mut land.folk.people[i];
            if strong(&person.resident) {
                person.angry = ANGER;
                let notice = Notice { radius: 30.0, reaction: Reaction::Attack };
                let brain = Brain::new(Behavior::Stand, Some(notice), here, agent.facing, person.resident.id.0);
                agent.running = true;
                brains.push((entity, brain));
                land.folk.say(i, if entity == target { "Agora você vai ver!" } else { "Vai pagar por isso!" });
            } else {
                person.fear = FEAR;
                let away = Vec3::new(here.x - player.x, 0.0, here.z - player.z).normalize_or_zero();
                agent.running = true;
                agent.go_to(here + away * 18.0);
                land.folk.say(
                    i,
                    if entity == target { "Socorro!" } else { ["Corram!", "Socorro!", "Ele está louco!"][i % 3] },
                );
            }
        }
        for (entity, brain) in brains {
            let _ = self.ecs.insert_one(entity, brain);
        }
        let _ = physics;
    }
}
