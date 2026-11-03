//! Health, blows and who can hit whom.
//!
//! A weapon without a [`Blade`] lands when the attacker's clip fires its event, on
//! everything in an arc. One with a blade hurts what the blade touches between the
//! clip's `hit_on` and `hit_off` events, so a blow can be walked away from. Scene
//! objects get these components from `combat:`; the game adds them to its player
//! and calls [`World::update_combat`] once per frame.
//!
//! Besides those, clips can fire `combo` (the next blow of a chain may start now)
//! and `cancel` (the rest is recovery, which a roll or a step may cut short).

use grimora_core::{Animator, Mat4, Transform, Vec3};
use grimora_physics::Physics;
use grimora_scene::{CombatDesc, Weapon};
use hecs::Entity;

use crate::World;
use crate::npc::{Agent, Brain};
use crate::world::despawn;

/// Seconds a flinch keeps a character from acting.
const STAGGER: f32 = 0.35;
/// Speed in meters per second a blow gives the character it hits.
const KNOCKBACK: f32 = 2.5;
/// Blows don't reach things further above or below than this, in meters.
const MAX_HEIGHT_GAP: f32 = 2.0;
/// Height of the line of sight a blow needs, up from the feet.
const CHEST: f32 = 1.1;
/// How tall what a blade can touch is, up from the feet.
const TARGET_HEIGHT: f32 = 1.8;
const FADE_IN: f32 = 0.12;
/// A chain's blows blend a little longer: each starts where the last one settled.
const CHAIN_FADE: f32 = 0.1;
/// Seconds an attack asked for mid-blow is remembered.
const BUFFER: f32 = 0.45;
/// How long the attacker's clip all but stops when a blow rings on a guard.
const BLOCK_STOP: f32 = 0.16;
/// Seconds after walking out of a blow in which an attack still follows on from it.
const CHAIN_GRACE: f32 = 0.4;
/// How fast clips run while a hit lands, so it feels like it met something.
const HIT_STOP_SPEED: f32 = 0.05;
/// How many positions along a frame's swing are tested, so that a fast blade
/// doesn't go through someone between two frames.
const SWEEP_STEPS: usize = 5;

const EVENT_HIT_ON: &str = "hit_on";
const EVENT_HIT_OFF: &str = "hit_off";
const EVENT_COMBO: &str = "combo";
const EVENT_CANCEL: &str = "cancel";

/// What can be hurt. At zero it plays its `die` clip and stays there; without
/// that clip, it is despawned (a barrel that breaks).
#[derive(Debug, Clone)]
pub struct Health {
    pub current: f32,
    pub max: f32,
    /// Blows only land on other teams.
    pub team: u32,
    /// How wide it is, in meters: a blow reaches this much further to hit it.
    pub radius: f32,
    stagger: f32,
    untouchable: f32,
}

impl Health {
    pub fn new(max: f32, team: u32) -> Self {
        Self { current: max, max, team, radius: 0.4, stagger: 0.0, untouchable: 0.0 }
    }

    pub fn is_dead(&self) -> bool {
        self.current <= 0.0
    }

    /// How much is left, from 0 to 1.
    pub fn fraction(&self) -> f32 {
        (self.current / self.max.max(1e-6)).clamp(0.0, 1.0)
    }

    /// Still flinching from a blow, and unable to act.
    pub fn staggered(&self) -> bool {
        self.stagger > 0.0
    }

    /// Nothing can hurt it for this many seconds: the heart of a roll.
    pub fn make_untouchable(&mut self, seconds: f32) {
        self.untouchable = seconds;
    }

    pub fn untouchable(&self) -> bool {
        self.untouchable > 0.0
    }
}

/// What blows and rolls cost. It refills by itself soon after the last time it was spent.
#[derive(Debug, Clone)]
pub struct Stamina {
    pub current: f32,
    pub max: f32,
    /// Per second.
    pub regen: f32,
    /// Seconds after spending before it starts to refill.
    pub delay: f32,
    wait: f32,
}

impl Stamina {
    pub fn new(max: f32) -> Self {
        Self { current: max, max, regen: 34.0, delay: 0.7, wait: 0.0 }
    }

    /// Spends `amount` if there is any left at all, as in the games this copies:
    /// an empty bar stops the next action, not the one that empties it.
    pub fn spend(&mut self, amount: f32) -> bool {
        if self.current <= 0.0 {
            return false;
        }
        self.current = (self.current - amount).max(0.0);
        self.wait = self.delay;
        true
    }

    /// How much is left, from 0 to 1.
    pub fn fraction(&self) -> f32 {
        (self.current / self.max.max(1e-6)).clamp(0.0, 1.0)
    }

    fn refill(&mut self, dt: f32) {
        if self.wait > 0.0 {
            self.wait -= dt;
        } else {
            self.current = (self.current + self.regen * dt).min(self.max);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Ready,
    Wanted,
    Playing { struck: bool },
}

/// What can deal blows. Ask with [`Fighter::attack`]; [`World::update_combat`]
/// plays the clip and lands the blow.
#[derive(Debug, Clone)]
pub struct Fighter {
    pub weapon: Weapon,
    cooldown: f32,
    phase: Phase,
    /// Which blow of the chain: 0 is `weapon.clip`, the rest `weapon.combo`.
    step: usize,
    /// Seconds left of an attack asked for mid-blow.
    buffered: f32,
    /// Seconds left to follow on from a blow that was walked out of.
    chain_left: f32,
    chain_open: bool,
    can_cancel: bool,
    /// The blade hurts right now.
    striking: bool,
    /// Who this swing already hit.
    hit: Vec<Entity>,
    /// Where the blade was last frame.
    last_blade: Option<(Vec3, Vec3)>,
}

impl Fighter {
    pub fn new(weapon: Weapon) -> Self {
        Self {
            weapon,
            cooldown: 0.0,
            phase: Phase::Ready,
            step: 0,
            buffered: 0.0,
            chain_left: 0.0,
            chain_open: false,
            can_cancel: false,
            striking: false,
            hit: Vec::new(),
            last_blade: None,
        }
    }

    /// Starts a blow, unless it is in the middle of one or still recovering.
    /// Returns whether it did. With a chain to follow, one asked for too soon is
    /// remembered for a moment and comes when the clip fires `combo`.
    pub fn attack(&mut self) -> bool {
        let ready = self.ready();
        if ready {
            self.phase = Phase::Wanted;
            self.step = if self.chain_left > 0.0 { (self.step + 1) % (self.weapon.combo.len() + 1) } else { 0 };
            self.chain_left = 0.0;
        } else if self.busy() && !self.weapon.combo.is_empty() {
            self.buffered = BUFFER;
        }
        ready
    }

    /// Whether a call to [`Fighter::attack`] would start a blow.
    pub fn ready(&self) -> bool {
        self.phase == Phase::Ready && self.cooldown <= 0.0
    }

    /// In the middle of a blow, with its clip playing. The game leaves its
    /// character's animation alone meanwhile.
    pub fn busy(&self) -> bool {
        self.phase != Phase::Ready
    }

    /// The blade hurts right now.
    pub fn striking(&self) -> bool {
        self.striking
    }

    /// The blow is past the point where it can be cut short: see [`Fighter::interrupt`].
    pub fn can_cancel(&self) -> bool {
        self.can_cancel
    }

    /// Drops the blow where it is. The game plays what comes next.
    pub fn interrupt(&mut self) {
        self.phase = Phase::Ready;
        self.step = 0;
        self.buffered = 0.0;
        self.chain_left = 0.0;
        self.chain_open = false;
        self.can_cancel = false;
        self.striking = false;
    }

    /// Drops a blow's recovery to walk away, with the next blow of the chain still on
    /// offer for a moment: stepping between blows doesn't start the chain over.
    pub fn walk_off(&mut self) {
        let step = self.step;
        self.interrupt();
        self.step = step;
        self.chain_left = CHAIN_GRACE;
    }

    fn clip(&self) -> &str {
        match self.step {
            0 => &self.weapon.clip,
            step => self.weapon.combo.get(step - 1).unwrap_or(&self.weapon.clip),
        }
    }

    /// Starts the current blow's clip, if it can pay for it. Returns whether the
    /// blow lands now, which is what happens with no clip to play.
    fn start(
        &mut self,
        animator: Option<&mut Animator>,
        stamina: Option<&mut Stamina>,
        fade: f32,
        me: Entity,
        events: &mut Vec<CombatEvent>,
    ) -> bool {
        if stamina.is_some_and(|stamina| !stamina.spend(self.weapon.stamina)) {
            self.interrupt();
            return false;
        }
        events.push(CombatEvent::Swing { attacker: me });
        if self.step == 0 {
            self.cooldown = self.weapon.cooldown;
        }
        self.buffered = 0.0;
        self.chain_open = false;
        self.can_cancel = false;
        self.striking = false;
        let clip = self.clip().to_string();
        let started = animator.is_some_and(|animator| {
            let started = animator.replay_once(&clip, fade);
            if started {
                animator.speed = 1.0;
            }
            started
        });
        self.phase = if started { Phase::Playing { struck: false } } else { Phase::Ready };
        !started
    }

    /// Moves the blow along. Returns whether it lands now.
    fn advance(
        &mut self,
        animator: Option<&mut Animator>,
        stamina: Option<&mut Stamina>,
        me: Entity,
        dt: f32,
        events: &mut Vec<CombatEvent>,
    ) -> bool {
        self.buffered = (self.buffered - dt).max(0.0);
        self.chain_left = (self.chain_left - dt).max(0.0);
        match self.phase {
            Phase::Ready => false,
            Phase::Wanted => self.start(animator, stamina, FADE_IN, me, events),
            Phase::Playing { struck } => {
                // Something else took over the animation: a flinch, usually.
                let clip = self.clip().to_string();
                let Some(animator) = animator.filter(|a| a.current() == Some(clip.as_str())) else {
                    self.interrupt();
                    return false;
                };
                self.chain_open |= animator.fired(EVENT_COMBO);
                self.can_cancel |= animator.fired(EVENT_CANCEL);
                if animator.fired(EVENT_HIT_ON) {
                    self.striking = true;
                    self.hit.clear();
                    self.last_blade = None;
                }
                if animator.fired(EVENT_HIT_OFF) {
                    self.striking = false;
                }
                if self.chain_open && self.buffered > 0.0 && !self.weapon.combo.is_empty() {
                    self.step = (self.step + 1) % (self.weapon.combo.len() + 1);
                    return self.start(Some(animator), stamina, CHAIN_FADE, me, events);
                }
                let finished = animator.finished();
                let lands = self.weapon.blade.is_none() && !struck && (animator.fired(&self.weapon.event) || finished);
                if finished {
                    self.interrupt();
                } else {
                    self.phase = Phase::Playing { struck: struck || lands };
                }
                lands
            }
        }
    }

    fn cancel(&mut self) {
        self.interrupt();
        self.cooldown = 0.0;
    }
}

/// A weapon held up to meet blows: while it's `raised`, what comes from in front rings on
/// it and hurts nobody. The game raises and lowers it, and plays what that looks like.
#[derive(Debug, Clone, Copy)]
pub struct Guard {
    pub raised: bool,
    /// How wide what it covers is, in degrees, centered on where the character faces.
    pub arc: f32,
    /// Stamina each blow on it takes. An empty bar lets the blow through.
    pub cost: f32,
}

impl Default for Guard {
    fn default() -> Self {
        Self { raised: false, arc: 180.0, cost: 0.0 }
    }
}

/// What happened in a fight this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CombatEvent {
    /// A blow started.
    Swing { attacker: Entity },
    /// A blow rang on `target`'s [`Guard`], at `position` on its weapon.
    Blocked { attacker: Entity, target: Entity, position: Vec3, direction: Vec3 },
    /// A blow landed. With `killed` and no `die` clip, `target` is already gone.
    Hit {
        attacker: Entity,
        target: Entity,
        damage: f32,
        /// On the target, on the side facing the attacker.
        position: Vec3,
        /// Flat, from the attacker towards the target.
        direction: Vec3,
        killed: bool,
    },
}

/// A blow about to land, with what it needs from its attacker.
struct Strike {
    attacker: Entity,
    team: Option<u32>,
    origin: Vec3,
    forward: Vec3,
    damage: f32,
    reach: f32,
    /// Cosine of half the arc.
    spread: f32,
    hit_stop: f32,
}

impl Strike {
    fn reaches(&self, target: Vec3, radius: f32) -> bool {
        let to = target - self.origin;
        let flat = Vec3::new(to.x, 0.0, to.z);
        let distance = flat.length();
        to.y.abs() <= MAX_HEIGHT_GAP
            && distance - radius <= self.reach
            && (distance <= radius || self.forward.dot(flat / distance) >= self.spread)
    }
}

/// A blade's swing over a frame, from one base and tip to the next.
struct Sweep {
    attacker: Entity,
    team: Option<u32>,
    origin: Vec3,
    from: (Vec3, Vec3),
    to: (Vec3, Vec3),
    radius: f32,
    damage: f32,
    hit_stop: f32,
    already: Vec<Entity>,
}

impl Sweep {
    /// The point of the blade that touches a standing capsule at `feet`, if any.
    fn touches(&self, feet: Vec3, radius: f32) -> Option<Vec3> {
        let bottom = feet + Vec3::Y * radius.min(TARGET_HEIGHT * 0.5);
        let top = feet + Vec3::Y * (TARGET_HEIGHT - radius).max(radius);
        (0..=SWEEP_STEPS).find_map(|k| {
            let t = k as f32 / SWEEP_STEPS as f32;
            let (base, tip) = (self.from.0.lerp(self.to.0, t), self.from.1.lerp(self.to.1, t));
            let (on_blade, on_body) = closest_points(base, tip, bottom, top);
            (on_blade.distance(on_body) <= radius + self.radius).then_some(on_blade)
        })
    }
}

/// The closest points of two segments.
fn closest_points(a0: Vec3, a1: Vec3, b0: Vec3, b1: Vec3) -> (Vec3, Vec3) {
    let (d1, d2, r) = (a1 - a0, b1 - b0, a0 - b0);
    let (aa, ee, f) = (d1.length_squared().max(1e-8), d2.length_squared().max(1e-8), d2.dot(r));
    let (c, b) = (d1.dot(r), d1.dot(d2));
    let denominator = aa * ee - b * b;
    let mut s = if denominator > 1e-8 { ((b * f - c * ee) / denominator).clamp(0.0, 1.0) } else { 0.0 };
    let mut t = (b * s + f) / ee;
    if t < 0.0 {
        t = 0.0;
        s = (-c / aa).clamp(0.0, 1.0);
    } else if t > 1.0 {
        t = 1.0;
        s = ((b - c) / aa).clamp(0.0, 1.0);
    }
    (a0 + d1 * s, b0 + d2 * t)
}

/// Clips run slowly for this many more seconds, while a hit lands.
struct HitStop(f32);

pub(crate) fn components(desc: &CombatDesc) -> (Health, Option<Fighter>) {
    let health = Health { radius: desc.radius, ..Health::new(desc.health, desc.team) };
    (health, desc.weapon.clone().map(Fighter::new))
}

/// Full health and ready to act, with its capsule back. Its animation is up to the caller.
pub(crate) fn revive(ecs: &mut hecs::World, entity: Entity, physics: &mut Physics) {
    let query = ecs.query_one_mut::<(&mut Health, Option<&mut Fighter>, Option<&Agent>)>(entity);
    let Ok((health, fighter, agent)) = query else { return };
    health.current = health.max;
    health.stagger = 0.0;
    health.untouchable = 0.0;
    if let Some(fighter) = fighter {
        fighter.cancel();
    }
    if let Some(agent) = agent {
        physics.set_enabled(agent.collider(), true);
    }
}

impl World {
    /// Starts the blows asked for, lands those whose moment came, and returns
    /// what happened. Call it once per frame from `Game::update`, after
    /// [`World::update_npcs`].
    pub fn update_combat(&mut self, physics: &mut Physics, dt: f32) -> Vec<CombatEvent> {
        let mut events = Vec::new();
        let mut strikes = Vec::new();
        let mut sweeps = Vec::new();
        for (entity, transform, fighter, mut animator, health, stamina) in self.ecs.query_mut::<(
            Entity,
            &Transform,
            &mut Fighter,
            Option<&mut Animator>,
            Option<&Health>,
            Option<&mut Stamina>,
        )>() {
            fighter.cooldown = (fighter.cooldown - dt).max(0.0);
            if health.is_some_and(Health::is_dead) {
                fighter.interrupt();
            } else if fighter.advance(animator.as_deref_mut(), stamina, entity, dt, &mut events) {
                let forward = transform.rotation * Vec3::NEG_Z;
                strikes.push(Strike {
                    attacker: entity,
                    team: health.map(|health| health.team),
                    origin: transform.position,
                    forward: Vec3::new(forward.x, 0.0, forward.z).normalize_or(Vec3::NEG_Z),
                    damage: fighter.weapon.damage,
                    reach: fighter.weapon.reach,
                    spread: (fighter.weapon.arc.to_radians() * 0.5).cos(),
                    hit_stop: fighter.weapon.hit_stop,
                });
            }
            if !fighter.striking {
                continue;
            }
            let Some(blade) = &fighter.weapon.blade else { continue };
            let Some(joint) = animator.as_deref().and_then(|animator| animator.joint_transform(&blade.joint)) else {
                continue;
            };
            let world: Mat4 = transform.matrix() * joint;
            let now = (world.transform_point3(Vec3::Y * blade.base), world.transform_point3(Vec3::Y * blade.tip));
            sweeps.push(Sweep {
                attacker: entity,
                team: health.map(|health| health.team),
                origin: transform.position,
                from: fighter.last_blade.unwrap_or(now),
                to: now,
                radius: blade.radius,
                damage: fighter.weapon.damage,
                hit_stop: fighter.weapon.hit_stop,
                already: fighter.hit.clone(),
            });
            fighter.last_blade = Some(now);
        }
        for health in self.ecs.query_mut::<&mut Health>() {
            health.stagger = (health.stagger - dt).max(0.0);
            health.untouchable = (health.untouchable - dt).max(0.0);
        }
        for stamina in self.ecs.query_mut::<&mut Stamina>() {
            stamina.refill(dt);
        }
        for strike in &strikes {
            self.land(strike, physics, &mut events);
        }
        for sweep in &sweeps {
            self.land_blade(sweep, physics, &mut events);
        }
        self.run_hit_stops(dt);
        events
    }

    /// Hurts everything the blow reaches, unless something solid is in the way.
    fn land(&mut self, strike: &Strike, physics: &mut Physics, events: &mut Vec<CombatEvent>) {
        let targets: Vec<(Entity, Vec3, f32)> = self
            .ecs
            .query::<(Entity, &Transform, &Health)>()
            .iter()
            .filter(|&(entity, transform, health)| {
                entity != strike.attacker
                    && !health.is_dead()
                    && Some(health.team) != strike.team
                    && strike.reaches(transform.position, health.radius)
            })
            .map(|(entity, transform, health)| (entity, transform.position, health.radius))
            .collect();
        for (target, position, radius) in targets {
            let eye = strike.origin + Vec3::Y * CHEST;
            let to = position + Vec3::Y * CHEST - eye;
            let distance = to.length();
            let clear = distance - radius - 0.05;
            if clear > 0.0 && physics.raycast_scenery(eye, to, clear).is_some() {
                continue;
            }
            let direction = Vec3::new(to.x, 0.0, to.z).normalize_or(strike.forward);
            if let Some(contact) = self.guard_contact(target, strike.origin) {
                self.ring(strike.attacker, target, contact, direction, events);
                continue;
            }
            let Some(killed) = self.damage(target, strike.damage, physics) else { continue };
            if !killed && let Some(agent) = self.get_mut::<Agent>(target) {
                agent.push(direction * KNOCKBACK);
            }
            self.stop_for_hit(strike.attacker, target, killed, strike.hit_stop);
            events.push(CombatEvent::Hit {
                attacker: strike.attacker,
                target,
                damage: strike.damage,
                position: position + Vec3::Y * CHEST - direction * radius,
                direction,
                killed,
            });
        }
    }

    /// Hurts what the blade touched this frame, once a swing.
    fn land_blade(&mut self, sweep: &Sweep, physics: &mut Physics, events: &mut Vec<CombatEvent>) {
        let targets: Vec<(Entity, Vec3, Vec3)> = self
            .ecs
            .query::<(Entity, &Transform, &Health)>()
            .iter()
            .filter(|&(entity, _, health)| {
                entity != sweep.attacker
                    && !health.is_dead()
                    && Some(health.team) != sweep.team
                    && !sweep.already.contains(&entity)
            })
            .filter_map(|(entity, transform, health)| {
                let contact = sweep.touches(transform.position, health.radius)?;
                Some((entity, transform.position, contact))
            })
            .collect();
        for (target, position, contact) in targets {
            // A wall between them stops it, as a blade can reach through thin air only.
            let eye = sweep.origin + Vec3::Y * CHEST;
            let to = contact - eye;
            if physics.raycast_scenery(eye, to, to.length()).is_some() {
                continue;
            }
            let flat = position - sweep.origin;
            let direction = Vec3::new(flat.x, 0.0, flat.z).normalize_or(Vec3::NEG_Z);
            if let Some(contact) = self.guard_contact(target, sweep.origin) {
                if let Some(fighter) = self.get_mut::<Fighter>(sweep.attacker) {
                    fighter.hit.push(target);
                }
                self.ring(sweep.attacker, target, contact, direction, events);
                continue;
            }
            let Some(killed) = self.damage(target, sweep.damage, physics) else { continue };
            if !killed && let Some(agent) = self.get_mut::<Agent>(target) {
                agent.push(direction * KNOCKBACK);
            }
            if let Some(fighter) = self.get_mut::<Fighter>(sweep.attacker) {
                fighter.hit.push(target);
            }
            self.stop_for_hit(sweep.attacker, target, killed, sweep.hit_stop);
            events.push(CombatEvent::Hit {
                attacker: sweep.attacker,
                target,
                damage: sweep.damage,
                position: contact,
                direction,
                killed,
            });
        }
    }

    /// Where a blow from `from` meets `target`'s guard, if it's up, facing that way and there's
    /// the stamina to take it, which this spends: on the blade, towards the tip, or just in
    /// front of the chest with no blade to show.
    fn guard_contact(&mut self, target: Entity, from: Vec3) -> Option<Vec3> {
        let guard = *self.get::<Guard>(target)?;
        if !guard.raised {
            return None;
        }
        let transform = *self.get::<Transform>(target)?;
        let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z).normalize_or_zero();
        let towards = flat(from - transform.position);
        if flat(transform.rotation * Vec3::NEG_Z).dot(towards) < (guard.arc.to_radians() * 0.5).cos() {
            return None;
        }
        let on_blade = || {
            let fighter = self.get::<Fighter>(target)?;
            let blade = fighter.weapon.blade.as_ref()?;
            let joint = self.get::<Animator>(target)?.joint_transform(&blade.joint)?;
            let along = blade.base + (blade.tip - blade.base) * 0.6;
            Some((transform.matrix() * joint).transform_point3(Vec3::Y * along))
        };
        let contact = on_blade().unwrap_or(transform.position + Vec3::Y * CHEST + towards * 0.4);
        if let Some(stamina) = self.get_mut::<Stamina>(target)
            && !stamina.spend(guard.cost)
        {
            return None;
        }
        Some(contact)
    }

    /// A blow rings on a guard: nobody is hurt, and the attacker's arm stops dead for a moment.
    fn ring(
        &mut self,
        attacker: Entity,
        target: Entity,
        position: Vec3,
        direction: Vec3,
        events: &mut Vec<CombatEvent>,
    ) {
        let _ = self.ecs.insert_one(attacker, HitStop(BLOCK_STOP));
        events.push(CombatEvent::Blocked { attacker, target, position, direction });
    }

    /// Both sides of a blow hold still for an instant.
    fn stop_for_hit(&mut self, attacker: Entity, target: Entity, killed: bool, seconds: f32) {
        if seconds <= 0.0 {
            return;
        }
        for entity in [attacker, target] {
            if self.ecs.contains(entity) && !(killed && entity == target) {
                let _ = self.ecs.insert_one(entity, HitStop(seconds));
            }
        }
    }

    fn run_hit_stops(&mut self, dt: f32) {
        let mut over = Vec::new();
        for (entity, stop, animator) in self.ecs.query_mut::<(Entity, &mut HitStop, Option<&mut Animator>)>() {
            stop.0 -= dt;
            let done = stop.0 <= 0.0;
            if let Some(animator) = animator {
                animator.speed = if done { 1.0 } else { HIT_STOP_SPEED };
            }
            if done {
                over.push(entity);
            }
        }
        for entity in over {
            let _ = self.ecs.remove_one::<HitStop>(entity);
        }
    }

    /// Takes `amount` of health from `target`, which flinches or dies (see
    /// [`Health`]) and, if it has a [`Brain`], notices who did it. Traps and
    /// spells can call this directly. Returns whether it died, or `None` if it
    /// can't be hurt, was already dead or is in the middle of a roll.
    pub fn damage(&mut self, target: Entity, amount: f32, physics: &mut Physics) -> Option<bool> {
        let (health, animator, agent, fighter, brain) = self
            .ecs
            .query_one_mut::<(
                &mut Health,
                Option<&mut Animator>,
                Option<&mut Agent>,
                Option<&mut Fighter>,
                Option<&mut Brain>,
            )>(target)
            .ok()?;
        if health.is_dead() || health.untouchable() {
            return None;
        }
        health.current = (health.current - amount).max(0.0);
        if let Some(brain) = brain {
            brain.alert(!health.is_dead());
        }
        let clip = if health.is_dead() { "die" } else { "hurt" };
        let played = animator.is_some_and(|animator| {
            let played = animator.replay_once(clip, FADE_IN);
            if played {
                animator.speed = 1.0;
            }
            played
        });
        if !health.is_dead() {
            if played {
                health.stagger = STAGGER;
            }
            return Some(false);
        }
        if let Some(fighter) = fighter {
            fighter.interrupt();
        }
        if let Some(agent) = agent {
            agent.stop();
            physics.set_enabled(agent.collider(), false);
        }
        if !played {
            despawn(&mut self.ecs, target, physics);
        }
        Some(true)
    }

    /// Back on its feet at full health, for a game that respawns its player.
    /// Its animation is up to the game.
    pub fn revive(&mut self, entity: Entity, physics: &mut Physics) {
        revive(&mut self.ecs, entity, physics);
    }

    /// Sets the health a saved game recorded. A character saved dead lies where it fell.
    pub(crate) fn set_health(&mut self, entity: Entity, current: f32, physics: &mut Physics) {
        let query = self.ecs.query_one_mut::<(&mut Health, Option<&mut Animator>, Option<&Agent>)>(entity);
        let Ok((health, animator, agent)) = query else { return };
        health.current = current;
        if !health.is_dead() {
            return;
        }
        if let Some(animator) = animator
            && animator.replay_once("die", 0.0)
        {
            animator.skip_to_end();
        }
        if let Some(agent) = agent {
            physics.set_enabled(agent.collider(), false);
        }
    }
}
