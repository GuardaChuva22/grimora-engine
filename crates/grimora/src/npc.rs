//! Characters that move on their own. An [`Agent`] walks where it is told,
//! around whatever is in the way; a [`Brain`] tells it where, from the scene
//! file's `npc:`. Games can drive an agent themselves by removing its brain.

use grimora_core::{Animator, Quat, Rng, Transform, Vec3};
use grimora_physics::{CharacterBody, ColliderHandle, NavShape, Physics};
use grimora_scene::{Behavior, Notice, NpcDesc, Reaction};
use hecs::Entity;

use crate::World;
use crate::combat::{Fighter, Frozen, Health};
use crate::third_person::lerp_angle;

/// New paths searched per update at most, so a crowd never stalls a frame.
const PATHS_PER_UPDATE: u32 = 2;
/// Farther than this from the player, characters wait where they are.
const ACTIVE_DISTANCE: f32 = 120.0;
/// How wide the player counts as when characters keep clear of them.
const PLAYER_RADIUS: f32 = 0.35;
/// Characters start bending their way this far before touching someone.
const PERSONAL_SPACE: f32 = 0.8;
/// Seconds between path searches of one agent.
const REPLAN_DELAY: f32 = 0.4;
/// Closer than this, a waypoint counts as reached.
const WAYPOINT_RADIUS: f32 = 0.35;
/// Seconds an agent pushes against something before looking for another way.
const STUCK_TIME: f32 = 0.7;
const MAX_ATTEMPTS: u32 = 3;
/// Seconds between looks at the player.
const SIGHT_INTERVAL: f32 = 0.2;
/// Seconds out of sight before a character loses interest.
const FORGET_TIME: f32 = 2.0;
/// How far a fleeing character aims each time.
const FLEE_STRIDE: f32 = 8.0;
/// How squarely it must face the player to strike, as a cosine.
const FACING_TO_STRIKE: f32 = 0.85;

/// What [`World::update_npcs`] noticed this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NpcEvent {
    /// This character just noticed the player.
    Noticed(Entity),
    /// It lost interest.
    Lost(Entity),
}

/// How an [`Agent::go_to`] is going.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AgentStatus {
    /// Nothing asked yet, or [`Agent::stop`].
    #[default]
    Idle,
    Moving,
    Arrived,
    /// There is no way, or it got stuck. It stopped as close as it could.
    Failed,
}

/// Walks a [`CharacterBody`] along paths from [`Physics::find_path`]. Its capsule
/// blocks the player and casts into ordinary raycasts, but not into path
/// searches or the camera's.
#[derive(Debug)]
pub struct Agent {
    pub body: CharacterBody,
    /// Direction it faces, in radians (0 looks towards -Z).
    pub facing: f32,
    pub walk_speed: f32,
    pub run_speed: f32,
    pub running: bool,
    /// How close to the target counts as there.
    pub arrive_distance: f32,
    /// Picks `idle`, `walk` or `run` on the entity's [`Animator`] by speed.
    /// Turn it off while the game plays something else.
    pub animate: bool,
    /// The speeds the `walk` and `run` clips were made for.
    pub clip_speeds: (f32, f32),
    /// Comes before the names of its clips.
    pub clip_prefix: String,
    /// A whole clip name played in place of `<prefix>idle` while standing still (a routine action).
    pub rest_clip: Option<String>,
    collider: ColliderHandle,
    /// Where its clip moves it this frame, on top of walking.
    root_motion: Vec3,
    velocity: Vec3,
    target: Option<Vec3>,
    look_at: Option<Vec3>,
    path: Vec<Vec3>,
    complete: bool,
    /// How far (flat) the target was when the path was found: a path that stops short of it
    /// but got this much nearer is followed by another from where it ends.
    planned_gap: f32,
    status: AgentStatus,
    wants_path: bool,
    replan_in: f32,
    stuck: f32,
    attempts: u32,
}

impl Agent {
    /// Moves it and where it's going by `offset`, as the world's origin moves the other way.
    pub(crate) fn shift(&mut self, offset: Vec3) {
        self.body.position += offset;
        if let Some(target) = &mut self.target {
            *target += offset;
        }
        if let Some(look) = &mut self.look_at {
            *look += offset;
        }
        for point in &mut self.path {
            *point += offset;
        }
    }
}

/// Someone to keep clear of.
#[derive(Debug, Clone, Copy)]
struct Neighbor {
    entity: Option<Entity>,
    position: Vec3,
    radius: f32,
}

impl Agent {
    const DEFAULT_ARRIVE: f32 = 0.4;
    const ACCELERATION: f32 = 8.0;
    const GRAVITY: f32 = 22.0;

    /// A walker standing at `position` (feet), with its capsule added to `physics`.
    pub fn new(physics: &mut Physics, position: Vec3, facing: f32, radius: f32, height: f32) -> Self {
        let collider = physics.add_agent(position, radius, height);
        let mut body = CharacterBody::new(position, radius, height);
        body.ignore = Some(collider);
        Self {
            body,
            facing,
            walk_speed: 2.0,
            run_speed: 4.5,
            running: false,
            arrive_distance: Self::DEFAULT_ARRIVE,
            animate: true,
            clip_speeds: (2.0, 4.5),
            clip_prefix: String::new(),
            rest_clip: None,
            collider,
            root_motion: Vec3::ZERO,
            velocity: Vec3::ZERO,
            target: None,
            look_at: None,
            path: Vec::new(),
            complete: false,
            planned_gap: 0.0,
            status: AgentStatus::Idle,
            wants_path: false,
            replan_in: 0.0,
            stuck: 0.0,
            attempts: 0,
        }
    }

    /// Starts walking to `target`. Calling it every frame with a moving target
    /// is fine: paths are searched a few times per second at most.
    pub fn go_to(&mut self, target: Vec3) {
        self.look_at = None;
        if self.status == AgentStatus::Moving && self.target.is_some_and(|old| old.distance(target) < 0.5) {
            return;
        }
        self.target = Some(target);
        self.status = AgentStatus::Moving;
        self.wants_path = true;
        self.stuck = 0.0;
        self.attempts = 0;
    }

    /// Stops walking.
    pub fn stop(&mut self) {
        self.finish(AgentStatus::Idle);
    }

    /// Turns to face `point` while not walking.
    pub fn look_at(&mut self, point: Vec3) {
        self.look_at = Some(point);
    }

    pub fn status(&self) -> AgentStatus {
        self.status
    }

    pub fn target(&self) -> Option<Vec3> {
        self.target
    }

    /// Places still to walk through, the next one first.
    pub fn path(&self) -> &[Vec3] {
        &self.path
    }

    pub fn velocity(&self) -> Vec3 {
        self.velocity
    }

    /// Its capsule, to recognize it in a `RayHit`.
    pub fn collider(&self) -> ColliderHandle {
        self.collider
    }

    /// Moves it at once, forgetting where it was going (as when loading a saved game).
    pub fn teleport(&mut self, physics: &mut Physics, position: Vec3, facing: f32) {
        self.body.position = position;
        self.facing = facing;
        self.velocity = Vec3::ZERO;
        self.look_at = None;
        self.stop();
        physics.move_agent(self.collider, self.body.center());
    }

    /// Adds to its speed; it slows back down by itself.
    pub(crate) fn push(&mut self, velocity: Vec3) {
        self.velocity += velocity;
    }

    fn finish(&mut self, status: AgentStatus) {
        self.status = status;
        self.target = None;
        self.path.clear();
        self.wants_path = false;
        self.stuck = 0.0;
    }

    fn plan(&mut self, physics: &Physics) {
        let Some(target) = self.target else { return };
        let shape = NavShape::new(self.body.radius, self.body.height);
        let path = physics.find_path(self.body.position, target, &shape);
        self.path = path.points;
        self.complete = path.complete;
        self.planned_gap = flat(target - self.body.position).length();
        self.wants_path = false;
        self.replan_in = REPLAN_DELAY;
        if self.path.is_empty() {
            self.finish(AgentStatus::Failed);
        }
    }

    /// Where it wants to go this frame, flat and at full speed.
    fn desired_velocity(&mut self, neighbors: &[Neighbor], me: Entity) -> Vec3 {
        let here = self.body.position;
        let Some(target) = self.target else { return Vec3::ZERO };
        let remaining = flat(target - here).length();
        if remaining <= self.arrive_distance {
            self.finish(AgentStatus::Arrived);
            return Vec3::ZERO;
        }
        while self.path.first().is_some_and(|&p| flat(p - here).length() < WAYPOINT_RADIUS) {
            self.path.remove(0);
            self.attempts = 0;
        }
        let Some(&next) = self.path.first() else {
            // Still waiting for its first path, or the path is over.
            if !self.wants_path {
                if !self.complete && remaining < self.planned_gap - 1.0 {
                    // A long way, found a piece at a time.
                    self.wants_path = true;
                } else {
                    self.finish(if self.complete { AgentStatus::Arrived } else { AgentStatus::Failed });
                }
            }
            return Vec3::ZERO;
        };

        let speed = if self.running { self.run_speed } else { self.walk_speed };
        // Eases in on the very end rather than overshooting and turning back.
        let easing = ((remaining - self.arrive_distance) / 0.6).clamp(0.3, 1.0);
        let direction = flat(next - here).normalize_or_zero();
        let mut desired = direction * speed * easing;

        // Bends around those in the way, each keeping to its own right, and
        // stops short of walking into them.
        let right = Vec3::new(-direction.z, 0.0, direction.x);
        for other in neighbors.iter().filter(|n| n.entity != Some(me)) {
            let away = flat(here - other.position);
            let distance = away.length();
            let gap = distance - self.body.radius - other.radius;
            if gap >= PERSONAL_SPACE || distance < 1e-4 {
                continue;
            }
            let out = away / distance;
            let closing = -desired.dot(out);
            if closing <= 0.0 {
                continue;
            }
            let urgency = (1.0 - gap / PERSONAL_SPACE).clamp(0.0, 1.0);
            desired += out * closing * urgency + right * speed * urgency;
        }
        desired.clamp_length_max(speed)
    }

    fn step(&mut self, physics: &mut Physics, neighbors: &[Neighbor], me: Entity, dt: f32) {
        self.replan_in -= dt;
        let desired = self.desired_velocity(neighbors, me);

        let blend = 1.0 - (-Self::ACCELERATION * dt).exp();
        let horizontal = flat(self.velocity).lerp(desired, blend);
        let mut vertical = self.velocity.y - Self::GRAVITY * dt;
        if self.body.grounded {
            vertical = vertical.max(-1.0);
        }
        self.velocity = horizontal + Vec3::Y * vertical;

        let motion = self.velocity * dt + std::mem::take(&mut self.root_motion);
        let applied = self.body.move_and_slide(physics, motion, dt);
        if self.body.grounded {
            self.velocity.y = 0.0;
        }
        physics.move_agent(self.collider, self.body.center());

        // Pushing against something it didn't plan for: look for another way, a few times.
        let wanted = desired.length();
        if wanted > 0.5 && flat(applied).length() < wanted * dt * 0.3 {
            self.stuck += dt;
            if self.stuck > STUCK_TIME {
                self.stuck = 0.0;
                self.attempts += 1;
                if self.attempts > MAX_ATTEMPTS {
                    self.finish(AgentStatus::Failed);
                } else {
                    self.wants_path = true;
                }
            }
        } else {
            self.stuck = 0.0;
        }

        let moving = flat(applied) / dt.max(1e-5);
        let look = if moving.length() > 0.3 {
            Some(moving)
        } else {
            self.look_at.map(|point| flat(point - self.body.position))
        };
        if let Some(look) = look.filter(|look| look.length() > 1e-3) {
            self.facing = lerp_angle(self.facing, (-look.x).atan2(-look.z), 1.0 - (-8.0 * dt).exp());
        }
    }

    fn play_clip(&self, animator: &mut Animator) {
        let speed = flat(self.velocity).length();
        let (walk, run) = self.clip_speeds;
        // Sitting down and getting up play once, in between the seated clips and the rest.
        const SEATED: [&str; 3] = ["sit_idle", "sit_talk", "eat"];
        let wants_seat = speed < 0.3 && self.rest_clip.as_deref().is_some_and(|c| SEATED.contains(&c));
        let current = animator.current().map(str::to_owned);
        let seated = current.as_deref().is_some_and(|c| SEATED.contains(&c));
        if wants_seat && current.as_deref().is_some_and(|c| !SEATED.contains(&c) && c != "sit_down") {
            animator.play_once("sit_down", 0.25);
            return;
        }
        if current.as_deref() == Some("sit_down") && wants_seat && !animator.finished() {
            return;
        }
        if !wants_seat && (seated || current.as_deref() == Some("sit_up")) {
            if seated {
                animator.play_once("sit_up", 0.25);
            }
            if !animator.finished() {
                return;
            }
        }
        if speed < 0.3
            && let Some(rest) = &self.rest_clip
        {
            if animator.play(rest, 0.3) {
                animator.speed = 1.0;
            }
            return;
        }
        let (clip, rate) = if speed < 0.3 {
            ("idle", 1.0)
        } else if speed < (self.walk_speed + self.run_speed) * 0.5 {
            ("walk", (speed / walk.max(0.01)).max(0.4))
        } else {
            ("run", (speed / run.max(0.01)).max(0.4))
        };
        if animator.play(&format!("{}{clip}", self.clip_prefix), 0.25) {
            animator.speed = rate;
        }
    }
}

/// Decides where an [`Agent`] goes: its routine, and what it does on noticing the player.
#[derive(Debug, Clone)]
pub struct Brain {
    pub behavior: Behavior,
    pub notice: Option<Notice>,
    /// Where it started: the middle of a wander, and where a stander stands.
    pub home: Vec3,
    home_facing: f32,
    rng: Rng,
    pause: f32,
    next_point: usize,
    noticed: bool,
    next_look: f32,
    unseen: f32,
    next_flee: f32,
}

impl Brain {
    /// `seed` picks where it wanders.
    pub fn new(behavior: Behavior, notice: Option<Notice>, home: Vec3, facing: f32, seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        // Starts apart from its neighbors: not everybody sets off at once or
        // takes a look on the same frame.
        let pause = rng.range(0.0, 1.0);
        let next_look = rng.range(0.0, SIGHT_INTERVAL);
        Self {
            behavior,
            notice,
            home,
            home_facing: facing,
            rng,
            pause,
            next_point: 0,
            noticed: false,
            next_look,
            unseen: 0.0,
            next_flee: 0.0,
        }
    }

    /// Whether it has noticed the player right now.
    pub fn noticed(&self) -> bool {
        self.noticed
    }

    /// Something hurt it: if `alive`, it looks for the player, seen or not, if it
    /// ever looks for anyone. The dead stop caring.
    pub(crate) fn alert(&mut self, alive: bool) {
        self.noticed = alive && self.notice.is_some();
        self.unseen = 0.0;
    }

    #[allow(clippy::too_many_arguments)]
    fn think(
        &mut self,
        agent: &mut Agent,
        fighter: Option<&mut Fighter>,
        physics: &Physics,
        player: Option<Vec3>,
        dt: f32,
        me: Entity,
        events: &mut Vec<NpcEvent>,
    ) {
        let Some(player) = player else {
            if self.noticed {
                self.noticed = false;
                events.push(NpcEvent::Lost(me));
                self.settle(agent);
            }
            self.routine(agent, dt);
            return;
        };
        self.look_around(agent, physics, player, dt, me, events);
        if self.noticed {
            self.react(agent, fighter, player, dt);
        } else {
            self.routine(agent, dt);
        }
    }

    fn look_around(
        &mut self,
        agent: &mut Agent,
        physics: &Physics,
        player: Vec3,
        dt: f32,
        me: Entity,
        events: &mut Vec<NpcEvent>,
    ) {
        let Some(notice) = &self.notice else { return };
        self.next_look -= dt;
        if self.next_look > 0.0 {
            return;
        }
        self.next_look += SIGHT_INTERVAL;

        let eyes = agent.body.position + Vec3::Y * agent.body.height * 0.9;
        let distance = eyes.distance(player);
        let limit = if self.noticed { notice.radius * 1.5 } else { notice.radius };
        let sees = distance <= limit && physics.raycast_scenery(eyes, player - eyes, distance).is_none();
        if sees {
            self.unseen = 0.0;
            if !self.noticed {
                self.noticed = true;
                events.push(NpcEvent::Noticed(me));
            }
        } else if self.noticed {
            self.unseen += SIGHT_INTERVAL;
            if self.unseen >= FORGET_TIME {
                self.noticed = false;
                events.push(NpcEvent::Lost(me));
                self.settle(agent);
            }
        }
    }

    /// Back to its routine, after a moment.
    fn settle(&mut self, agent: &mut Agent) {
        agent.stop();
        agent.running = false;
        agent.arrive_distance = Agent::DEFAULT_ARRIVE;
        agent.look_at = None;
        self.pause = 0.5;
        if let Behavior::Patrol { points, .. } = &self.behavior
            && !points.is_empty()
        {
            self.next_point = (self.next_point + points.len() - 1) % points.len();
        }
    }

    fn react(&mut self, agent: &mut Agent, fighter: Option<&mut Fighter>, player: Vec3, dt: f32) {
        let Some(notice) = &self.notice else { return };
        let distance = flat(player - agent.body.position).length();
        match notice.reaction {
            Reaction::Watch => {
                if agent.status == AgentStatus::Moving {
                    agent.stop();
                }
                agent.look_at(player);
            }
            Reaction::Follow { distance: keep } => {
                agent.arrive_distance = keep;
                agent.running = distance > keep * 3.0;
                // Some slack, so it doesn't shuffle at every step the player takes.
                if distance > keep * 1.3 {
                    agent.go_to(player);
                } else if agent.status != AgentStatus::Moving {
                    agent.look_at(player);
                }
            }
            Reaction::Attack => {
                let Some(fighter) = fighter else {
                    agent.look_at(player);
                    return;
                };
                let reach = fighter.weapon.reach;
                agent.arrive_distance = reach * 0.7;
                agent.running = distance > reach * 2.0;
                if distance > reach * 0.9 {
                    agent.go_to(player);
                    return;
                }
                if agent.status == AgentStatus::Moving {
                    agent.stop();
                }
                agent.look_at(player);
                let (sin, cos) = agent.facing.sin_cos();
                let forward = Vec3::new(-sin, 0.0, -cos);
                if forward.dot(flat(player - agent.body.position).normalize_or_zero()) > FACING_TO_STRIKE {
                    fighter.attack();
                }
            }
            Reaction::Flee => {
                agent.running = true;
                self.next_flee -= dt;
                if self.next_flee <= 0.0 || agent.status != AgentStatus::Moving {
                    self.next_flee = 1.0;
                    let away = flat(agent.body.position - player).normalize_or(Vec3::Z);
                    agent.go_to(agent.body.position + away * FLEE_STRIDE);
                }
            }
        }
    }

    fn routine(&mut self, agent: &mut Agent, dt: f32) {
        if agent.status == AgentStatus::Moving {
            return;
        }
        self.pause -= dt;
        match &self.behavior {
            Behavior::Stand => {
                if flat(agent.body.position - self.home).length() > 1.0 {
                    if self.pause <= 0.0 {
                        self.pause = 3.0;
                        agent.go_to(self.home);
                    }
                } else {
                    let (sin, cos) = self.home_facing.sin_cos();
                    agent.look_at(agent.body.position + Vec3::new(-sin, 0.0, -cos));
                }
            }
            Behavior::Wander { radius, pause } => {
                if self.pause <= 0.0 {
                    // Square root: evenly over the disc, not bunched at the middle.
                    let angle = self.rng.range(0.0, std::f32::consts::TAU);
                    let reach = radius * self.rng.f32().sqrt();
                    agent.go_to(self.home + Vec3::new(angle.cos(), 0.0, angle.sin()) * reach);
                    self.pause = self.rng.range(pause.0, pause.1);
                }
            }
            Behavior::Patrol { points, pause } => {
                if self.pause <= 0.0 && !points.is_empty() {
                    agent.go_to(points[self.next_point % points.len()]);
                    self.next_point = (self.next_point + 1) % points.len();
                    self.pause = *pause;
                }
            }
        }
    }
}

/// The agent and brain of a scene object with `npc:`, seeded by where it stands.
/// `transform` is where the scene placed it, which also picks its seed, and `offset` how far
/// the world's origin has moved that since.
pub(crate) fn from_desc(desc: &NpcDesc, transform: &Transform, offset: Vec3, physics: &mut Physics) -> (Agent, Brain) {
    let position = transform.position + offset;
    let facing = yaw_of(transform.rotation);
    let mut agent = Agent::new(physics, position, facing, desc.radius, desc.height);
    agent.walk_speed = desc.walk_speed;
    agent.run_speed = desc.run_speed;
    agent.clip_speeds = desc.clip_speeds.unwrap_or((desc.walk_speed, desc.run_speed));
    agent.clip_prefix = desc.clip_prefix.clone();
    let [x, y, z] = transform.position.to_array().map(|v| u64::from(v.to_bits()));
    let seed = x ^ y.rotate_left(21) ^ z.rotate_left(42);
    (agent, Brain::new(desc.behavior.clone(), desc.notice.clone(), position, facing, seed))
}

fn yaw_of(rotation: Quat) -> f32 {
    rotation.to_euler(grimora_core::EulerRot::YXZ).0
}

fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

impl World {
    /// Moves every [`Agent`] and runs every [`Brain`], with `player` (usually the
    /// center of the player's body) as who they see and keep clear of. Returns
    /// who noticed the player or lost interest. Call it once per frame from
    /// `Game::update`, after moving the player.
    pub fn update_npcs(&mut self, physics: &mut Physics, player: Vec3, dt: f32) -> Vec<NpcEvent> {
        let mut neighbors = vec![Neighbor { entity: None, position: player, radius: PLAYER_RADIUS }];
        for (entity, agent, health) in self.ecs.query::<(Entity, &Agent, Option<&Health>)>().iter() {
            if health.is_none_or(|health| !health.is_dead()) {
                neighbors.push(Neighbor {
                    entity: Some(entity),
                    position: agent.body.position,
                    radius: agent.body.radius,
                });
            }
        }
        let seen = (!self.npcs_ignore_player).then_some(player);

        let mut events = Vec::new();
        let mut budget = PATHS_PER_UPDATE;
        for (entity, agent, transform, brain, animator, mut fighter, health, frozen) in self.ecs.query_mut::<(
            Entity,
            &mut Agent,
            &mut Transform,
            Option<&mut Brain>,
            Option<&mut Animator>,
            Option<&mut Fighter>,
            Option<&Health>,
            Option<&Frozen>,
        )>() {
            if flat(agent.body.position - player).length() > ACTIVE_DISTANCE
                || health.is_some_and(|health| health.is_dead())
            {
                continue;
            }
            let staggered = health.is_some_and(|health| health.staggered()) || frozen.is_some();
            if staggered || fighter.as_ref().is_some_and(|fighter| fighter.busy()) {
                // It keeps its feet, and its last idea of where to look, until it can act.
                agent.stop();
            } else if let Some(brain) = brain {
                brain.think(agent, fighter.as_deref_mut(), physics, seen, dt, entity, &mut events);
            }
            if agent.wants_path && agent.replan_in <= 0.0 && budget > 0 {
                budget -= 1;
                agent.plan(physics);
            }
            if let Some(animator) = animator.as_deref() {
                agent.root_motion = Quat::from_rotation_y(agent.facing) * animator.root_delta();
            }
            agent.step(physics, &neighbors, entity, dt);
            transform.position = agent.body.position;
            transform.rotation = Quat::from_rotation_y(agent.facing);
            if agent.animate
                && !staggered
                && !fighter.is_some_and(|fighter| fighter.busy())
                && let Some(animator) = animator
            {
                agent.play_clip(animator);
            }
        }
        events
    }

    /// Puts each agent where its transform says and starts its brain over, after
    /// a saved game moved them.
    pub(crate) fn place_agents(&mut self, physics: &mut Physics) {
        for (agent, transform, brain) in self.ecs.query_mut::<(&mut Agent, &Transform, Option<&mut Brain>)>() {
            agent.teleport(physics, transform.position, yaw_of(transform.rotation));
            if let Some(brain) = brain {
                brain.noticed = false;
                brain.settle(agent);
            }
        }
    }
}
