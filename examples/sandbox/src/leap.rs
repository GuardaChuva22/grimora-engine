//! The leap ("Arremetida"): a charged jump straight up, a moment hanging at the top, and a
//! dash that throws the character far ahead, gliding down on what is left of it. Separate
//! from the usual jump, and tuned in `assets/abilities/leap.ron`, read again at every leap.

use grimora::prelude::*;
use serde::Deserialize;

use crate::{Mode, Sandbox};

pub const LEAP: &str = "leap";
/// The name of that control for the menu.
pub const CONTROL_NAME: (&str, &str) = (LEAP, "Arremetida");
const LEAP_FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/abilities/leap.ron");

pub fn add_controls(controls: &mut ActionMap) {
    controls.add(LEAP, [Binding::Key(KeyCode::KeyZ), Binding::Pad(PadButton::LeftStick)]);
}

/// Everything about the leap that can be tuned. Seconds are real ones where the world is
/// slowed down (the charge, the top, the burst), world ones elsewhere.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LeapDesc {
    /// The charge: a tap leaps as if held for `charge_least`, and holding past `charge_most`
    /// leaps by itself.
    pub charge_least: f32,
    pub charge_most: f32,
    /// How fast the world goes while it crouches.
    pub charge_slow: f32,
    pub charge_fov: f32,
    pub charge_pull: f32,
    pub charge_rumble: f32,
    /// Up, in meters per second, from a tap to a full charge.
    pub rise_least: f32,
    pub rise_most: f32,
    /// Ahead, towards the movement keys, and the share of the run it keeps.
    pub rise_ahead: f32,
    pub rise_keep: f32,
    /// Under this upward speed it hangs, with `hang_gravity` of the usual gravity, for up to
    /// `hang_most` seconds; then it falls, if it didn't dash.
    pub hang_from: f32,
    pub hang_gravity: f32,
    pub hang_most: f32,
    pub rise_fov: f32,
    pub rise_pull: f32,
    pub rise_lag: f32,
    pub launch_shake: f32,
    /// Radians the view is thrown up as it leaves the ground.
    pub launch_kick: f32,
    /// Seconds after the launch before the dash can be asked for.
    pub dash_after: f32,
    /// The pause at the top: how long (real seconds) and how slow the world goes.
    pub apex_seconds: f32,
    pub apex_slow: f32,
    pub apex_fov: f32,
    pub apex_pull: f32,
    /// The burst: an instant of near stillness as it breaks through.
    pub freeze_seconds: f32,
    pub freeze_slow: f32,
    pub burst_shake: f32,
    /// The dash: where the camera looks, lifted a little and kept between these pitches.
    pub dash_speed: f32,
    pub dash_lift: f32,
    pub dash_pitch_least: f32,
    pub dash_pitch_most: f32,
    /// It loses the speed above `dash_cruise` by `dash_drag` per second, and gravity comes
    /// back over `gravity_in` seconds, up to `dash_gravity` of the usual.
    pub dash_cruise: f32,
    pub dash_drag: f32,
    pub dash_gravity: f32,
    pub gravity_in: f32,
    pub dash_steer: f32,
    pub dash_lean: f32,
    /// How far the flying pose can point its nose down or up along the way it goes.
    pub glide_pitch: f32,
    pub dash_cost: f32,
    /// The camera at full speed: it eases down to nothing at cruise speed.
    pub dash_fov: f32,
    pub dash_pull: f32,
    pub dash_lag: f32,
    pub dash_rumble: f32,
    /// Below this height over the ground, it rises from lying in the air to upright, so it
    /// touches down on its feet.
    pub flare_height: f32,
    /// Pressing again while dashing (after `fly_after` real seconds) flies: the keys along
    /// the camera at `fly_speed`, or `fly_fast` running, the forward key tipping with the
    /// camera (`fly_lift` radians up from it), jump and dive straight up and down. It gets
    /// there by `fly_accel` per second, and stops by `fly_stop`; pressing again lets go.
    pub fly_after: f32,
    pub fly_speed: f32,
    pub fly_fast: f32,
    pub fly_climb: f32,
    pub fly_lift: f32,
    pub fly_accel: f32,
    pub fly_stop: f32,
    /// Under `hover_from` it levitates, over `fly_from` it flies, crossing in `fly_fade`
    /// seconds; between the two it keeps what it was doing.
    pub hover_from: f32,
    pub fly_from: f32,
    pub fly_fade: f32,
    /// How far the flying pose pitches, the levitating one leans, and either rolls in a turn.
    pub fly_pitch: f32,
    pub hover_lean: f32,
    pub fly_bank: f32,
    /// The camera at `fly_fast`, easing to nothing at `fly_speed`.
    pub fly_fov: f32,
    pub fly_pull: f32,
    pub fly_lag: f32,
    pub fly_rumble: f32,
    /// Flying into a wall faster than this bounces it back at `bonk_back`.
    pub bonk_from: f32,
    pub bonk_back: f32,
    /// Landing: it skids for `land_seconds`, losing speed by `land_drag` per second, and rolls
    /// if it comes in faster than `roll_from` along the ground.
    pub land_seconds: f32,
    pub land_drag: f32,
    pub roll_from: f32,
    pub land_shake: f32,
}

impl Default for LeapDesc {
    fn default() -> Self {
        Self {
            charge_least: 0.12,
            charge_most: 0.55,
            charge_slow: 0.8,
            charge_fov: -0.08,
            charge_pull: -0.8,
            charge_rumble: 0.012,
            rise_least: 11.0,
            rise_most: 17.0,
            rise_ahead: 2.5,
            rise_keep: 0.5,
            hang_from: 4.0,
            hang_gravity: 0.3,
            hang_most: 0.7,
            rise_fov: 0.12,
            rise_pull: 1.5,
            rise_lag: 0.12,
            launch_shake: 0.18,
            launch_kick: 0.12,
            dash_after: 0.15,
            apex_seconds: 0.2,
            apex_slow: 0.2,
            apex_fov: -0.12,
            apex_pull: 0.4,
            freeze_seconds: 0.06,
            freeze_slow: 0.03,
            burst_shake: 0.35,
            dash_speed: 42.0,
            dash_lift: 0.1,
            dash_pitch_least: -1.1,
            dash_pitch_most: 0.9,
            dash_cruise: 13.0,
            dash_drag: 1.1,
            dash_gravity: 0.55,
            gravity_in: 0.9,
            dash_steer: 1.4,
            dash_lean: 0.6,
            glide_pitch: 1.3,
            dash_cost: 25.0,
            dash_fov: 0.5,
            dash_pull: 2.5,
            dash_lag: 0.15,
            dash_rumble: 0.05,
            flare_height: 2.5,
            fly_after: 0.2,
            fly_speed: 18.0,
            fly_fast: 40.0,
            fly_climb: 10.0,
            fly_lift: 0.2,
            fly_accel: 3.0,
            fly_stop: 3.5,
            hover_from: 6.0,
            fly_from: 9.0,
            fly_fade: 0.3,
            fly_pitch: 1.0,
            hover_lean: 0.3,
            fly_bank: 0.55,
            fly_fov: 0.3,
            fly_pull: 1.5,
            fly_lag: 0.1,
            fly_rumble: 0.02,
            bonk_from: 12.0,
            bonk_back: 4.0,
            land_seconds: 0.6,
            land_drag: 3.5,
            roll_from: 7.0,
            land_shake: 0.25,
        }
    }
}

impl LeapDesc {
    fn load() -> Self {
        let read = std::fs::read_to_string(LEAP_FILE).map_err(|err| err.to_string());
        match read.and_then(|text| ron::from_str(&text).map_err(|err| err.to_string())) {
            Ok(desc) => desc,
            Err(err) => {
                eprintln!("[sandbox] {LEAP_FILE}: {err}");
                Self::default()
            }
        }
    }
}

/// How much faster the glide beats right after a burst, and for how long (real seconds).
const BURST_RATE: f32 = 3.0;
const BURST_SECONDS: f32 = 0.45;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
enum Phase {
    #[default]
    Idle,
    /// Crouching, for this many real seconds, with this speed along the ground to carry.
    Charge { held: f32, carried: Vec3 },
    /// Going up, for this many seconds, and hanging for that many.
    Rise { t: f32, hung: f32 },
    /// Stopped at the top, for this many real seconds, turning to the way it will go.
    Apex { t: f32 },
    /// Thrown that way, for this many seconds (world), real ones since the burst, and how
    /// long (real) the world stays still for it.
    Dash { t: f32, since: f32, way: Vec3, freeze: f32 },
    /// Falling without the leap's rules: no dash, or a bounce off a wall.
    Fall,
    /// Flying free, for this many real seconds, at speed (the flying pose) or not (levitating).
    Fly { t: f32, fast: bool },
    /// Skidding on the ground, for this many seconds.
    Land { t: f32 },
}

#[derive(Default)]
pub struct Leap {
    phase: Phase,
    desc: LeapDesc,
    /// The world's speed before the leap slowed it down.
    scale_before: Option<f32>,
    /// Its speed the last time it was in the air.
    last_velocity: Vec3,
    /// Real seconds a press of the leap is kept for, while a burst cannot come yet.
    wanted: f32,
    /// Bursts since it left the ground: the break clips and effects take turns by it.
    bursts: u32,
}

impl Leap {
    /// Whether it keeps the movement keys to itself: while it crouches or skids.
    pub fn holds(&self) -> bool {
        matches!(self.phase, Phase::Charge { .. } | Phase::Land { .. })
    }
}

fn slow(leap: &mut Leap, time: &mut Time, scale: f32) {
    leap.scale_before.get_or_insert(time.scale());
    time.set_scale(scale);
}

fn unslow(leap: &mut Leap, time: &mut Time) {
    if let Some(scale) = leap.scale_before.take() {
        time.set_scale(scale);
    }
}

impl Sandbox {
    /// Runs the leap. Returns whether it owns the character now: no blows, rolls or other
    /// clips while it does.
    pub(crate) fn update_leap(&mut self, ctx: &mut Context, dt: f32) -> bool {
        let real = ctx.time.delta();
        let Some(player) = self.player.as_mut() else { return false };
        let leap = &mut self.leap;
        let can = self.mode == Mode::Play && self.dead_for.is_none();
        if !can || player.swimming() {
            if leap.phase != Phase::Idle {
                end(leap, player, &mut ctx.time);
            }
            return false;
        }
        let pressed = ctx.input.action_just_pressed(LEAP);
        leap.wanted = if pressed { 0.2 } else { (leap.wanted - real).max(0.0) };
        let holding = ctx.input.action(LEAP);
        if !player.body.grounded {
            leap.last_velocity = player.velocity();
        }
        let stamina = ctx.world.get::<Stamina>(self.avatar).map_or(0.0, |s| s.current);
        let mut parts = ctx.world.ecs.query_one_mut::<(&mut Animator, &mut Health)>(self.avatar).ok();

        match leap.phase {
            Phase::Idle => {
                if pressed && player.body.grounded && !player.locked && stamina > 0.0 {
                    leap.desc = LeapDesc::load();
                    let d = leap.desc.clone();
                    let velocity = player.velocity();
                    leap.phase =
                        Phase::Charge { held: 0.0, carried: Vec3::new(velocity.x, 0.0, velocity.z) * d.rise_keep };
                    slow(leap, &mut ctx.time, d.charge_slow);
                    player.fov_boost = d.charge_fov;
                    player.camera_pull = d.charge_pull;
                    if let Some((animator, _)) = &mut parts {
                        animator.replay_once("jump_land", 0.1);
                        animator.speed = 1.0;
                    }
                }
            }
            Phase::Charge { held, carried } => {
                let d = leap.desc.clone();
                let held = held + real;
                let level = ((held - d.charge_least) / (d.charge_most - d.charge_least).max(1e-3)).clamp(0.0, 1.0);
                player.rumble = d.charge_rumble * (0.3 + level);
                if let Some((animator, _)) = &mut parts {
                    // Holds the deepest crouch of the landing.
                    if animator.current() == Some("jump_land") && animator.time() > 0.12 {
                        animator.speed = 0.0;
                    }
                }
                let go = held >= d.charge_least && (!holding || held >= d.charge_most);
                if !player.body.grounded {
                    end(leap, player, &mut ctx.time);
                } else if go {
                    let up = d.rise_least + (d.rise_most - d.rise_least) * level;
                    let ahead = player.move_direction(&ctx.input) * d.rise_ahead;
                    player.set_velocity(carried + ahead + Vec3::Y * up);
                    player.flight = Some(Flight {
                        gravity: 1.0,
                        drag: 0.0,
                        cruise: 0.0,
                        steer: 0.8,
                        lean: 0.0,
                        lean_rate: 6.0,
                        flat: false,
                        bank: 0.0,
                        face: None,
                    });
                    player.shake(d.launch_shake);
                    player.recoil(d.launch_kick, 0.0);
                    player.fov_boost = d.rise_fov;
                    player.camera_pull = d.rise_pull;
                    player.camera_lag = d.rise_lag;
                    player.rumble = 0.0;
                    unslow(leap, &mut ctx.time);
                    if let Some((animator, _)) = &mut parts {
                        animator.replay_once("jump", 0.05);
                        animator.speed = 1.0;
                    }
                    leap.phase = Phase::Rise { t: 0.0, hung: 0.0 };
                } else {
                    leap.phase = Phase::Charge { held, carried };
                }
            }
            Phase::Rise { t, hung } => {
                let d = leap.desc.clone();
                let t = t + dt;
                let vy = player.velocity().y;
                let hanging = vy < d.hang_from;
                let hung = if hanging { hung + dt } else { hung };
                if let Some(flight) = &mut player.flight {
                    flight.gravity = if hanging { d.hang_gravity } else { 1.0 };
                }
                if pressed && t >= d.dash_after && stamina >= d.dash_cost * 0.5 {
                    // Stops at the top and turns to where the camera looks.
                    player.set_velocity(player.velocity() * 0.1);
                    if let Some(flight) = &mut player.flight {
                        flight.gravity = 0.0;
                        flight.steer = 0.0;
                    }
                    player.face(ctx.camera.forward());
                    player.fov_boost = d.apex_fov;
                    player.camera_pull = d.apex_pull;
                    player.camera_lag = 0.0;
                    player.rumble = d.charge_rumble;
                    slow(leap, &mut ctx.time, d.apex_slow);
                    if let Some((animator, _)) = &mut parts {
                        animator.play_once("jump_fall", 0.08);
                    }
                    leap.phase = Phase::Apex { t: 0.0 };
                } else if player.body.grounded && t > 0.1 {
                    end(leap, player, &mut ctx.time);
                } else if hung > d.hang_most {
                    calm(player);
                    player.flight = None;
                    leap.phase = Phase::Fall;
                } else {
                    leap.phase = Phase::Rise { t, hung };
                }
            }
            Phase::Apex { t } => {
                let d = leap.desc.clone();
                let t = t + real;
                // Keeps turning with the camera while the world waits.
                let look = ctx.camera.forward();
                player.face(look);
                if t >= d.apex_seconds {
                    let way = dash_way(&d, look);
                    if let Some(stamina) = ctx.world.get_mut::<Stamina>(self.avatar) {
                        stamina.current = (stamina.current - d.dash_cost).max(0.0);
                    }
                    leap.bursts = 0;
                    leap.wanted = 0.0;
                    burst(leap, player, &mut ctx.time, way, d.dash_speed, d.freeze_seconds, d.burst_shake);
                    if let Some(animator) = ctx.world.get_mut::<Animator>(self.avatar) {
                        animator.play("leap_glide", 0.12);
                        animator.speed = BURST_RATE;
                    }
                } else {
                    leap.phase = Phase::Apex { t };
                }
            }
            Phase::Dash { t, since, way, freeze } => {
                let d = leap.desc.clone();
                let (t, since) = (t + dt, since + real);
                if since >= freeze {
                    unslow(leap, &mut ctx.time);
                }
                // Rises to its feet as the ground comes up.
                let height = ctx
                    .physics
                    .raycast_scenery(player.body.position + Vec3::Y * 0.2, Vec3::NEG_Y, d.flare_height + 0.5)
                    .map_or(d.flare_height, |hit| (hit.distance - 0.2).max(0.0));
                let flare = (height / d.flare_height.max(0.1)).clamp(0.0, 1.0);
                // The glide lies flat by itself. Each burst beats it faster for a moment, and it
                // stands up into the fall as the ground comes near.
                let mut gliding = false;
                if let Some((animator, _)) = &mut parts {
                    if animator.current() == Some("leap_glide") {
                        let left = (1.0 - since / BURST_SECONDS).max(0.0);
                        animator.speed = 1.0 + (BURST_RATE - 1.0) * left * left;
                        if flare <= 0.6 {
                            animator.play_once("jump_fall", 0.2);
                            animator.speed = 1.0;
                        }
                    }
                    gliding = animator.current() == Some("leap_glide");
                }
                let speed = player.velocity().length();
                let rush = ((speed - d.dash_cruise) / (d.dash_speed - d.dash_cruise).max(1.0)).clamp(0.0, 1.0);
                player.fov_boost = d.dash_fov * rush;
                player.camera_pull = d.dash_pull * rush;
                player.rumble = d.dash_rumble * rush;
                if let Some(flight) = &mut player.flight {
                    let back = (t / d.gravity_in.max(1e-3)).clamp(0.0, 1.0);
                    flight.gravity = d.dash_gravity * back * back * (3.0 - 2.0 * back);
                    flight.flat = gliding;
                    flight.lean =
                        if gliding { d.glide_pitch } else { d.dash_lean * flare * flare * (3.0 - 2.0 * flare) };
                }
                let fly = leap.wanted > 0.0 && since >= d.fly_after && !player.body.grounded;
                if player.impact() > d.bonk_from {
                    // Into a wall: back off it, up a little, and down from there.
                    let flat = Vec3::new(way.x, 0.0, way.z).normalize_or_zero();
                    player.set_velocity(-flat * d.bonk_back + Vec3::Y * 3.0);
                    player.shake(d.burst_shake);
                    calm(player);
                    player.flight = None;
                    unslow(leap, &mut ctx.time);
                    if let Some((animator, _)) = &mut parts {
                        animator.replay_once("hurt", 0.05);
                    }
                    leap.phase = Phase::Fall;
                } else if fly {
                    // Takes the dash's speed into flight.
                    leap.wanted = 0.0;
                    unslow(leap, &mut ctx.time);
                    let fast = gliding;
                    leap.phase = Phase::Fly { t: 0.0, fast };
                } else if player.body.grounded && t > 0.05 {
                    let parts = ctx.world.ecs.query_one_mut::<(&mut Animator, &mut Health)>(self.avatar).ok();
                    land(leap, player, parts);
                    unslow(leap, &mut ctx.time);
                } else {
                    leap.phase = Phase::Dash { t, since, way, freeze };
                }
            }
            Phase::Fall => {
                if player.body.grounded {
                    end(leap, player, &mut ctx.time);
                } else if pressed {
                    // Takes off again from a fall.
                    leap.desc = LeapDesc::load();
                    leap.phase = Phase::Fly { t: 0.0, fast: false };
                    return true;
                }
                return false;
            }
            Phase::Fly { t, fast } => {
                let d = leap.desc.clone();
                let t = t + real;
                let velocity = player.velocity();
                let speed = velocity.length();
                if pressed && t > 0.3 {
                    // Lets go, and falls the usual way.
                    calm(player);
                    player.flight = None;
                    if let Some((animator, _)) = &mut parts {
                        animator.speed = 1.0;
                        animator.play_once("jump_fall", 0.4);
                    }
                    leap.phase = Phase::Fall;
                } else if player.body.grounded && t > 0.2 && velocity.y <= 0.5 {
                    if speed > d.roll_from {
                        land(leap, player, parts);
                    } else {
                        if let Some((animator, _)) = &mut parts {
                            animator.speed = 1.0;
                            animator.replay_once("jump_land", 0.25);
                        }
                        end(leap, player, &mut ctx.time);
                    }
                } else {
                    // Where it wants to go: the keys along the camera, the forward key
                    // tipping with it, jump and dive straight up and down.
                    let wish = player.move_direction(&ctx.input);
                    let (sin, cos) = player.camera_yaw.sin_cos();
                    let forward = Vec3::new(-sin, 0.0, -cos);
                    let ahead = wish.dot(forward);
                    let pitch = player.camera_pitch + d.fly_lift;
                    let running = ctx.input.action(grimora::core::actions::RUN);
                    let top = if running { d.fly_fast } else { d.fly_speed };
                    let mut target = wish * top + forward * ahead * top * (pitch.cos() - 1.0);
                    let climb = ctx.input.action_axis(grimora::core::actions::DIVE, grimora::core::actions::JUMP);
                    target.y = ahead * top * pitch.sin() + climb * d.fly_climb;
                    let rate = if target.length() > 0.1 { d.fly_accel } else { d.fly_stop };
                    player.set_velocity(velocity + (target - velocity) * (1.0 - (-rate * dt).exp()));

                    // Levitates when slow and flies when fast, with room between the two so
                    // it never flickers.
                    let fast = if fast { speed > d.hover_from } else { speed > d.fly_from };
                    if let Some((animator, _)) = &mut parts {
                        let clip = if fast { "leap_glide" } else { "leap_hover" };
                        if animator.current() != Some(clip) {
                            animator.play(clip, d.fly_fade);
                        }
                        let beat = if fast { (speed / d.fly_speed).clamp(0.7, 1.6) } else { 1.0 };
                        animator.speed += (beat - animator.speed) * (1.0 - (-4.0 * dt).exp());
                    }
                    player.flight = Some(Flight {
                        gravity: 0.0,
                        drag: 0.0,
                        cruise: 0.0,
                        steer: 0.0,
                        lean: if fast { d.fly_pitch } else { d.hover_lean },
                        lean_rate: 5.0,
                        flat: fast,
                        bank: d.fly_bank,
                        face: (!fast).then_some(player.camera_yaw),
                    });
                    let rush = ((speed - d.fly_speed) / (d.fly_fast - d.fly_speed).max(1.0)).clamp(0.0, 1.0);
                    player.fov_boost = d.fly_fov * rush;
                    player.camera_pull = d.fly_pull * rush;
                    player.camera_lag = d.fly_lag;
                    player.rumble = d.fly_rumble * rush;
                    leap.phase = Phase::Fly { t, fast };
                }
            }
            Phase::Land { t } => {
                let t = t + dt;
                if t >= leap.desc.land_seconds {
                    end(leap, player, &mut ctx.time);
                } else {
                    leap.phase = Phase::Land { t };
                }
            }
        }
        leap.phase != Phase::Idle && leap.phase != Phase::Fall
    }
}

/// Where a dash goes for a camera looking `look`: lifted a little, kept between the pitches.
fn dash_way(d: &LeapDesc, look: Vec3) -> Vec3 {
    let pitch = (look.y.clamp(-1.0, 1.0).asin() + d.dash_lift).clamp(d.dash_pitch_least, d.dash_pitch_most);
    let flat = Vec3::new(look.x, 0.0, look.z).normalize_or(Vec3::NEG_Z);
    flat * pitch.cos() + Vec3::Y * pitch.sin()
}

/// Breaks through: thrown `way` at `speed`, the world still for `freeze` real seconds.
/// The break clip and its effect come in here, one per burst (`Leap::bursts`).
fn burst(
    leap: &mut Leap,
    player: &mut ThirdPersonController,
    time: &mut Time,
    way: Vec3,
    speed: f32,
    freeze: f32,
    shake: f32,
) {
    let d = &leap.desc;
    player.set_velocity(way * speed);
    player.flight = Some(Flight {
        gravity: 0.0,
        drag: d.dash_drag,
        cruise: d.dash_cruise,
        steer: d.dash_steer,
        lean: d.dash_lean,
        lean_rate: 8.0,
        flat: false,
        bank: 0.4,
        face: None,
    });
    player.shake(shake);
    player.camera_lag = d.dash_lag;
    leap.bursts += 1;
    let still = d.freeze_slow;
    slow(leap, time, still);
    leap.phase = Phase::Dash { t: 0.0, since: 0.0, way, freeze };
}

/// The camera goes back to normal.
fn calm(player: &mut ThirdPersonController) {
    player.fov_boost = 0.0;
    player.camera_pull = 0.0;
    player.camera_lag = 0.0;
    player.rumble = 0.0;
}

fn end(leap: &mut Leap, player: &mut ThirdPersonController, time: &mut Time) {
    calm(player);
    player.flight = None;
    unslow(leap, time);
    leap.phase = Phase::Idle;
}

/// Touches the ground from the dash: rolls out of it if it came in fast along the ground,
/// lands hard otherwise, and skids.
fn land(leap: &mut Leap, player: &mut ThirdPersonController, parts: Option<(&mut Animator, &mut Health)>) {
    let d = &leap.desc;
    let came = leap.last_velocity;
    let along = Vec3::new(came.x, 0.0, came.z);
    player.set_velocity(along);
    calm(player);
    player.flight = Some(Flight {
        gravity: 1.0,
        drag: d.land_drag,
        cruise: 0.0,
        steer: 0.0,
        lean: 0.0,
        lean_rate: 6.0,
        flat: false,
        bank: 0.0,
        face: None,
    });
    let rolls = along.length() > d.roll_from;
    player.shake(d.land_shake * (came.length() / d.dash_speed).clamp(0.3, 1.0));
    if let Some((animator, health)) = parts {
        if rolls {
            health.make_untouchable(d.land_seconds);
        }
        animator.replay_once(if rolls { "sword_roll" } else { "leap_land" }, 0.12);
        animator.speed = 1.0;
    }
    leap.phase = Phase::Land { t: 0.0 };
}
