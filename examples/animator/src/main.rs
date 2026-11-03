//! Grimora's animator: drag the character's hands, feet and blade, and the rest follows by IK.
//! Clips are the text files in the model's `_clips` folder, the same ones the game loads.

mod edit;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use edit::{Change, Doc, snap};
use grimora::core::animation::JointTransform;
use grimora::core::{Control, Controls, POSE_FPS, PoseSampler, Rig, Skeleton, SolvedPose};
use grimora::prelude::*;
use grimora::scene::{DEFAULT_MAX_TEXTURE_SIZE, load_model, load_pose_clips, pose_clips_dir};

const MODEL: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/models/chartest/chartest.gltf");
const SWORD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/models/weapons/espada_reta.glb");
const SCENE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/scenes/animator.ron");
const BLADE_TRAIL: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/effects/blade_trail.ron");
const HIT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/effects/hit.ron");
/// The sword's scale, as in the game.
const SWORD_SCALE: f32 = 0.263;
/// Where along the weapon joint the blade runs, as the combat data says.
const BLADE: (f32, f32) = (0.11, 0.79);
/// The target's capsule: its height and radius.
const TARGET: (f32, f32) = (1.7, 0.3);
const HIT_STOP: f32 = 0.075;

const LEFT_WIDTH: f32 = 196.0;
const RIGHT_WIDTH: f32 = 310.0;
const BOTTOM_HEIGHT: f32 = 158.0;
const ROW: f32 = 26.0;
const GRAB_RADIUS: f32 = 13.0;
/// Degrees per pixel, turning the body with the mouse.
const DEGREES_PER_PIXEL: f32 = 0.4;

const SPEEDS: [f32; 4] = [1.0, 0.5, 0.25, 0.1];
/// (name in the clip, on the choice, on the timeline, color)
const EVENTS: [(&str, &str, &str, [f32; 3]); 7] = [
    ("hit_on", "começa a ferir", "fere", [1.0, 0.35, 0.3]),
    ("hit_off", "para de ferir", "parou", [0.85, 0.55, 0.35]),
    ("combo", "próximo golpe", "combo", [0.45, 0.9, 0.5]),
    ("cancel", "pode rolar", "rolar", [0.45, 0.7, 1.0]),
    ("swing", "som do golpe", "som", [0.8, 0.8, 0.8]),
    ("fall", "som de queda", "queda", [0.7, 0.7, 0.7]),
    ("step", "passo", "passo", [0.7, 0.7, 0.7]),
];

const TEXT: Vec4 = Vec4::new(0.92, 0.9, 0.85, 1.0);
const DIM: Vec4 = Vec4::new(0.62, 0.62, 0.62, 1.0);
const ACCENT: Vec4 = Vec4::new(1.0, 0.78, 0.42, 1.0);
const BAD: Vec4 = Vec4::new(1.0, 0.42, 0.35, 1.0);
const GOOD: Vec4 = Vec4::new(0.5, 0.95, 0.55, 1.0);
const PANEL: Vec4 = Vec4::new(0.07, 0.07, 0.09, 0.93);
const LINE: Vec4 = Vec4::new(0.32, 0.32, 0.36, 1.0);

/// What can be grabbed on the character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Root,
    Hips,
    Torso,
    Head,
    HandR,
    HandL,
    Tip,
    ElbowR,
    ElbowL,
    KneeL,
    KneeR,
    FootL,
    FootR,
}

const PARTS: [Part; 13] = [
    Part::Root,
    Part::Hips,
    Part::Torso,
    Part::Head,
    Part::HandR,
    Part::HandL,
    Part::Tip,
    Part::ElbowR,
    Part::ElbowL,
    Part::KneeL,
    Part::KneeR,
    Part::FootL,
    Part::FootR,
];

impl Part {
    /// The control a plain drag keys, and the one with Shift.
    fn controls(self) -> (Control, Control) {
        match self {
            Part::Root => (Control::Root, Control::Root),
            Part::Hips => (Control::Hips, Control::HipsRot),
            Part::Torso => (Control::Torso, Control::Torso),
            Part::Head => (Control::Head, Control::Head),
            Part::HandR => (Control::HandR, Control::HandR),
            Part::HandL => (Control::HandL, Control::HandL),
            Part::Tip => (Control::Blade, Control::Edge),
            Part::ElbowR => (Control::ElbowR, Control::ElbowR),
            Part::ElbowL => (Control::ElbowL, Control::ElbowL),
            Part::KneeL => (Control::KneeL, Control::KneeL),
            Part::KneeR => (Control::KneeR, Control::KneeR),
            Part::FootL => (Control::FootL, Control::FootLRot),
            Part::FootR => (Control::FootR, Control::FootRRot),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Part::Root => "posição no chão",
            Part::Hips => "quadril (Shift: gira)",
            Part::Torso => "tronco (Shift: tomba)",
            Part::Head => "cabeça (Shift: tomba)",
            Part::HandR => "mão da espada",
            Part::HandL => "mão livre",
            Part::Tip => "ponta da espada (Shift: gira o fio)",
            Part::ElbowR => "cotovelo da espada",
            Part::ElbowL => "cotovelo livre",
            Part::KneeL => "joelho esquerdo",
            Part::KneeR => "joelho direito",
            Part::FootL => "pé esquerdo (Shift: gira)",
            Part::FootR => "pé direito (Shift: gira)",
        }
    }

    fn color(self) -> Vec4 {
        match self {
            Part::HandR | Part::HandL | Part::FootL | Part::FootR => Vec4::new(0.35, 0.8, 1.0, 1.0),
            Part::Tip => ACCENT,
            Part::Hips | Part::Torso | Part::Head => Vec4::new(0.95, 0.5, 0.85, 1.0),
            Part::Root => Vec4::new(0.5, 0.95, 0.55, 1.0),
            _ => Vec4::new(0.8, 0.8, 0.8, 1.0),
        }
    }
}

fn control_name(control: Control) -> &'static str {
    match control {
        Control::Root => "Posição no chão",
        Control::Hips => "Quadril",
        Control::HipsRot => "Quadril, giro",
        Control::Torso => "Tronco",
        Control::Head => "Cabeça",
        Control::FootL => "Pé esquerdo",
        Control::FootR => "Pé direito",
        Control::FootLRot => "Pé esq., giro",
        Control::FootRRot => "Pé dir., giro",
        Control::ToeL => "Dedos esq.",
        Control::ToeR => "Dedos dir.",
        Control::KneeL => "Joelho esq.",
        Control::KneeR => "Joelho dir.",
        Control::HandR => "Mão da espada",
        Control::Blade => "Lâmina",
        Control::Edge => "Fio",
        Control::ElbowR => "Cotovelo espada",
        Control::HandL => "Mão livre",
        Control::ElbowL => "Cotovelo livre",
        Control::Tuck => "Encolher (rolar)",
    }
}

/// The sliders of a control: label and range of each number it uses, and the step.
fn control_sliders(control: Control) -> (&'static [(usize, &'static str)], f32, f32) {
    const POSITION: &[(usize, &str)] = &[(0, "X (direita)"), (1, "Y (cima)"), (2, "Z (trás)")];
    const ANGLES: &[(usize, &str)] = &[(0, "Inclina"), (1, "Vira"), (2, "Tomba")];
    const FLOOR: &[(usize, &str)] = &[(0, "X (direita)"), (2, "Z (trás)")];
    const ONE: &[(usize, &str)] = &[(0, "Valor")];
    match control {
        Control::Root => (FLOOR, 6.0, 0.005),
        Control::Hips | Control::FootL | Control::FootR | Control::HandR | Control::HandL => (POSITION, 3.0, 0.005),
        Control::HipsRot | Control::Torso | Control::Head | Control::FootLRot | Control::FootRRot => {
            (ANGLES, 400.0, 1.0)
        }
        Control::ToeL | Control::ToeR => (ONE, 60.0, 1.0),
        Control::Tuck => (ONE, 1.0, 0.01),
        _ => (POSITION, 1.0, 0.01),
    }
}

/// Keys of these move together on the timeline.
fn with_partner(control: Control) -> Vec<Control> {
    match control {
        Control::Blade | Control::Edge => vec![Control::Blade, Control::Edge],
        other => vec![other],
    }
}

/// What the mouse is holding.
enum Drag {
    /// A handle on the character: `pivot` is what it turns around, if it does.
    Part {
        part: Part,
        shift: bool,
        before: grimora::core::PoseClip,
        at: f32,
        press: Vec2,
        anchor: Vec3,
        pivot: Vec3,
    },
    Scrub,
    /// Keys on the timeline; nothing happens until the mouse leaves the frame they're on.
    Keys {
        controls: Vec<Control>,
        before: grimora::core::PoseClip,
        from: f32,
        moved: bool,
    },
    Event {
        index: usize,
        moved: bool,
    },
    Orbit,
    Pan,
}

/// What happens in the clip being looked at when it plays against the target.
#[derive(Default)]
struct Strike {
    /// The moment the blade touched, or `None`.
    hit_at: Option<f32>,
    /// How near the blade came, in meters.
    nearest: Option<f32>,
    /// The blade's base and tip, a moment ago.
    blade: Option<(Vec3, Vec3)>,
    stop: f32,
}

/// What's rebuilt only when the clip changes.
struct Views {
    key: (usize, u64, Part),
    /// With and without the springs.
    sprung: PoseSampler,
    raw: PoseSampler,
    tip_path: Vec<Vec3>,
    part_path: Vec<Vec3>,
}

/// The joints the handles stand on.
#[derive(Clone, Copy)]
struct Joints {
    hips: usize,
    chest: usize,
    head: usize,
    weapon: usize,
    hand_l: usize,
    forearm_r: usize,
    forearm_l: usize,
    upper_r: usize,
    upper_l: usize,
    shin_l: usize,
    shin_r: usize,
    thigh_l: usize,
    thigh_r: usize,
    foot_l: usize,
    foot_r: usize,
}

struct Tool {
    skeleton: Option<Arc<Skeleton>>,
    rig: Option<Rig>,
    joints: Option<Joints>,
    folder: PathBuf,
    docs: Vec<Doc>,
    /// Clips only in the model's file, which can be watched but aren't poses to edit.
    view_only: Vec<String>,
    /// An index into `docs`, then into `view_only`.
    current: usize,
    views: Option<Views>,
    avatar: Option<Entity>,
    target: Option<Entity>,
    trails: Option<[Entity; 2]>,
    hit_effect: Option<ParticleEffect>,
    time: f32,
    playing: bool,
    /// How much longer to hold the last frame before looping.
    hold: f32,
    repeat: bool,
    speed: usize,
    follow: bool,
    /// Show the springs while stopped, not only the keys.
    springs_when_stopped: bool,
    target_distance: f32,
    orbit_target: Vec3,
    orbit_yaw: f32,
    orbit_pitch: f32,
    orbit_distance: f32,
    selected: Control,
    selected_part: Part,
    selected_event: Option<usize>,
    new_event: usize,
    /// How many seconds around it a drag reaches.
    reach: f32,
    drag: Option<Drag>,
    /// A slider has taken its undo point and is still being held.
    slider_open: bool,
    last_cursor: Vec2,
    strike: Strike,
    status: String,
    pose: Vec<JointTransform>,
    /// The pose on screen.
    controls: Controls,
    solved: SolvedPose,
    /// `--menu clip@seconds`, for captures.
    open_at: Option<(String, f32)>,
    /// `--menu clip@seconds/joint`: the joint a capture looks at.
    focus: Option<String>,
}

impl Tool {
    fn new() -> Self {
        Self {
            skeleton: None,
            rig: None,
            joints: None,
            folder: pose_clips_dir(MODEL.as_ref()),
            docs: Vec::new(),
            view_only: Vec::new(),
            current: 0,
            views: None,
            avatar: None,
            target: None,
            trails: None,
            hit_effect: None,
            time: 0.0,
            playing: false,
            hold: 0.0,
            repeat: true,
            speed: 0,
            follow: false,
            springs_when_stopped: false,
            target_distance: 1.8,
            orbit_target: Vec3::new(0.0, 1.0, -0.4),
            orbit_yaw: 75f32.to_radians(),
            orbit_pitch: 8f32.to_radians(),
            orbit_distance: 3.6,
            selected: Control::HandR,
            selected_part: Part::HandR,
            selected_event: None,
            new_event: 0,
            reach: 0.06,
            drag: None,
            slider_open: false,
            last_cursor: Vec2::ZERO,
            strike: Strike::default(),
            status: String::new(),
            pose: Vec::new(),
            controls: Controls::default(),
            solved: SolvedPose::default(),
            open_at: None,
            focus: None,
        }
    }

    fn doc(&self) -> Option<&Doc> {
        self.docs.get(self.current)
    }

    fn doc_mut(&mut self) -> Option<&mut Doc> {
        self.docs.get_mut(self.current)
    }

    fn duration(&self) -> f32 {
        if let Some(doc) = self.doc() {
            return doc.clip.duration;
        }
        let name = self.view_only.get(self.current.wrapping_sub(self.docs.len()));
        let clip = self.skeleton.as_ref().zip(name).and_then(|(s, name)| s.clip(name).map(|k| s.clips[k].duration));
        clip.unwrap_or(1.0)
    }

    fn open(&mut self, index: usize) {
        self.current = index;
        self.time = 0.0;
        self.hold = 0.0;
        self.strike = Strike::default();
        self.views = None;
        self.selected_event = None;
        self.drag = None;
    }

    fn stop(&mut self) {
        self.playing = false;
        self.time = snap(self.time);
    }

    fn select(&mut self, control: Control) {
        self.selected = control;
        if let Some(part) = PARTS.iter().find(|p| {
            let (plain, shift) = p.controls();
            plain == control || shift == control
        }) {
            self.selected_part = *part;
        }
    }

    /// World position of a part, in the pose on screen.
    fn part_at(&self, part: Part, solved: &SolvedPose, controls: &Controls) -> Vec3 {
        let Some(j) = self.joints else { return Vec3::ZERO };
        let root = root_of(controls);
        let at = |joint: usize| root + solved.positions[joint];
        match part {
            Part::Root => root,
            Part::Hips => at(j.hips),
            Part::Torso => at(j.chest),
            Part::Head => at(j.head) + solved.turns[j.head] * (Vec3::Y * 0.12),
            Part::HandR => at(j.weapon),
            Part::HandL => at(j.hand_l),
            Part::Tip => blade_of(j, solved, root).1,
            Part::ElbowR => at(j.forearm_r),
            Part::ElbowL => at(j.forearm_l),
            Part::KneeL => at(j.shin_l),
            Part::KneeR => at(j.shin_r),
            Part::FootL => at(j.foot_l),
            Part::FootR => at(j.foot_r),
        }
    }

    /// Rebuilds what depends on the clip, if it changed.
    fn refresh_views(&mut self) {
        let Some(doc) = self.doc() else { return };
        let key = (self.current, doc.revision, self.selected_part);
        if self.views.as_ref().is_some_and(|views| views.key == key) {
            return;
        }
        let Some(rig) = &self.rig else { return };
        let sprung = PoseSampler::new(&doc.clip);
        let raw = PoseSampler::with_springs(&doc.clip, false);
        let frames = (doc.clip.duration * POSE_FPS).round() as usize;
        let (mut tip_path, mut part_path) = (Vec::with_capacity(frames + 1), Vec::with_capacity(frames + 1));
        for k in 0..=frames {
            let controls = sprung.sample(k as f32 / POSE_FPS);
            let solved = rig.solve(&controls);
            tip_path.push(self.part_at(Part::Tip, &solved, &controls));
            part_path.push(self.part_at(self.selected_part, &solved, &controls));
        }
        self.views = Some(Views { key, sprung, raw, tip_path, part_path });
    }

    /// Works out the pose for this moment and puts it on the character.
    fn show_pose(&mut self, ctx: &mut Context) {
        let Some(avatar) = self.avatar else { return };
        self.refresh_views();
        let mut root = Vec3::ZERO;
        if let (Some(rig), Some(views), true) = (&self.rig, &self.views, self.current < self.docs.len()) {
            let dragging = matches!(self.drag, Some(Drag::Part { .. }));
            let exact = dragging || (!self.playing && !self.springs_when_stopped);
            let controls = if exact { views.raw.sample(self.time) } else { views.sprung.sample(self.time) };
            let solved = rig.solve(&controls);
            rig.local_pose(&solved, &controls, Vec3::ZERO, &mut self.pose);
            root = root_of(&controls);
            self.controls = controls;
            self.solved = solved;
        } else if let Some(skeleton) = &self.skeleton
            && let Some(name) = self.view_only.get(self.current.wrapping_sub(self.docs.len()))
            && let Some(index) = skeleton.clip(name)
        {
            self.pose = skeleton.rest_pose();
            skeleton.clips[index].sample(self.time, &mut self.pose);
        }
        if let Some(animator) = ctx.world.get_mut::<Animator>(avatar) {
            animator.set_pose(&self.pose);
        }
        if let Some(transform) = ctx.world.get_mut::<Transform>(avatar) {
            transform.position = root;
        }
        if let Some(target) = self.target
            && let Some(transform) = ctx.world.get_mut::<Transform>(target)
        {
            transform.position.z = -self.target_distance;
        }
        if self.follow {
            self.orbit_target.x = root.x;
            self.orbit_target.z = root.z - 0.4;
        }
    }

    /// Moves the clock, and strikes the target with the blade on the way.
    fn advance(&mut self, ctx: &mut Context, dt: f32) {
        if !self.playing {
            return;
        }
        if self.strike.stop > 0.0 {
            self.strike.stop -= dt;
            return;
        }
        if self.hold > 0.0 {
            self.hold -= dt;
            if self.hold <= 0.0 {
                self.time = 0.0;
                self.strike = Strike::default();
            }
            return;
        }
        let duration = self.duration();
        let before = self.time;
        self.time += dt * SPEEDS[self.speed];
        self.check_strike(ctx, before);
        if self.time >= duration {
            let looping = self.doc().is_some_and(|doc| doc.clip.looping);
            if looping {
                self.time %= duration.max(1e-3);
            } else if self.repeat {
                self.time = duration;
                self.hold = 0.6;
            } else {
                self.time = duration;
                self.stop();
            }
        }
    }

    fn check_strike(&mut self, ctx: &mut Context, before: f32) {
        let (Some(rig), Some(joints), Some(views), Some(doc)) = (&self.rig, self.joints, &self.views, self.doc())
        else {
            return;
        };
        let (Some(on), Some(off)) = (doc.event_time("hit_on"), doc.event_time("hit_off")) else { return };
        if self.time < on || before > off || self.strike.hit_at.is_some() {
            return;
        }
        let axis = (Vec3::new(0.0, 0.0, -self.target_distance), Vec3::new(0.0, TARGET.0, -self.target_distance));
        let (mut nearest, mut hit) = (self.strike.nearest.unwrap_or(f32::MAX), None);
        let mut last = self.strike.blade;
        // Fast blades go a long way in a frame: look in between.
        let (from, to) = (before.max(on), self.time.min(off));
        for k in 1..=6 {
            let t = from + (to - from) * k as f32 / 6.0;
            let controls = views.sprung.sample(t);
            let blade = blade_of(joints, &rig.solve(&controls), root_of(&controls));
            let previous = last.unwrap_or(blade);
            for (a, b) in [(blade.0, blade.1), (previous.1, blade.1)] {
                let gap = segment_distance(a, b, axis.0, axis.1) - TARGET.1;
                nearest = nearest.min(gap);
                if gap <= 0.0 && hit.is_none() {
                    hit = Some((t, b));
                }
            }
            last = Some(blade);
        }
        self.strike.blade = last;
        self.strike.nearest = Some(nearest);
        if let Some((t, at)) = hit {
            self.strike.hit_at = Some(t);
            self.strike.stop = HIT_STOP;
            if let Some(effect) = &self.hit_effect {
                ctx.world.play_effect(effect, at);
            }
        }
    }

    fn shortcuts(&mut self, ctx: &mut Context) {
        let input = &ctx.input;
        let ctrl = input.pressed(KeyCode::ControlLeft) || input.pressed(KeyCode::ControlRight);
        let shift = input.pressed(KeyCode::ShiftLeft) || input.pressed(KeyCode::ShiftRight);
        let undo = ctrl && input.just_pressed(KeyCode::KeyZ) && !shift;
        let redo = ctrl && (input.just_pressed(KeyCode::KeyY) || (input.just_pressed(KeyCode::KeyZ) && shift));
        let save = ctrl && input.just_pressed(KeyCode::KeyS);
        let toggle = input.just_pressed(KeyCode::Space);
        let back = input.just_pressed(KeyCode::ArrowLeft) || input.just_repeated(KeyCode::ArrowLeft);
        let forward = input.just_pressed(KeyCode::ArrowRight) || input.just_repeated(KeyCode::ArrowRight);
        let delete = input.just_pressed(KeyCode::Delete);
        let key_here = input.just_pressed(KeyCode::KeyK);
        if undo && let Some(doc) = self.doc_mut() {
            doc.undo();
        }
        if redo && let Some(doc) = self.doc_mut() {
            doc.redo();
        }
        if save {
            self.save_all();
        }
        if toggle {
            self.toggle_play();
        }
        if back || forward {
            self.stop();
            let step = if back { -1.0 } else { 1.0 } / POSE_FPS;
            self.time = snap((self.time + step).clamp(0.0, self.duration()));
        }
        if delete {
            self.delete_selected();
        }
        if key_here {
            self.key_here();
        }
    }

    fn toggle_play(&mut self) {
        if self.playing {
            self.stop();
        } else {
            if self.time >= self.duration() - 1e-3 {
                self.time = 0.0;
            }
            self.playing = true;
            self.hold = 0.0;
            self.strike = Strike::default();
        }
    }

    fn delete_selected(&mut self) {
        let (time, selected, event) = (self.time, self.selected, self.selected_event.take());
        let Some(doc) = self.doc_mut() else { return };
        doc.checkpoint();
        match event {
            Some(index) => doc.delete_event(index),
            None => with_partner(selected).into_iter().for_each(|control| doc.delete_key(control, time)),
        }
    }

    /// A key on the selected control for what it's doing right now.
    fn key_here(&mut self) {
        self.stop();
        let Some(views) = &self.views else { return };
        let (time, selected) = (self.time, self.selected);
        let values: Vec<_> = with_partner(selected).into_iter().map(|c| (c, views.raw.sample(time)[c])).collect();
        let Some(doc) = self.doc_mut() else { return };
        doc.checkpoint();
        for (control, value) in values {
            doc.set_key(control, time, value);
        }
    }

    fn save_all(&mut self) {
        let mut saved = 0;
        for doc in self.docs.iter_mut().filter(|doc| doc.dirty) {
            match doc.save(&self.folder) {
                Ok(()) => saved += 1,
                Err(err) => {
                    self.status = format!("Erro ao salvar: {err:#}");
                    return;
                }
            }
        }
        self.status = match saved {
            0 => "Nada novo para salvar.".to_string(),
            1 => "Salvo. O jogo já lê este clipe ao abrir.".to_string(),
            n => format!("{n} clipes salvos."),
        };
    }

    /// A new clip's name that isn't taken.
    fn free_name(&self, base: &str) -> String {
        (2..)
            .map(|n| format!("{base}_{n}"))
            .find(|name| !self.docs.iter().any(|doc| doc.name == *name))
            .unwrap_or_default()
    }

    fn add_doc(&mut self, mut doc: Doc) {
        doc.dirty = true;
        self.docs.push(doc);
        self.docs.sort_by(|a, b| a.name.cmp(&b.name));
    }

    /// The mouse in the 3D view: grabbing handles, and the camera.
    fn viewport(&mut self, ctx: &mut Context, cursor: Vec2) {
        let mouse_moved = cursor - self.last_cursor;
        let free = !self.over_panels(ctx.ui.size(), cursor) && self.drag.is_none();
        if free && ctx.input.mouse_just_pressed(MouseButton::Right) {
            self.drag = Some(Drag::Orbit);
        }
        if free && ctx.input.mouse_just_pressed(MouseButton::Middle) {
            self.drag = Some(Drag::Pan);
        }
        if free && ctx.input.scroll() != 0.0 {
            self.orbit_distance = (self.orbit_distance * 0.9f32.powf(ctx.input.scroll())).clamp(0.6, 14.0);
        }
        if free && self.current < self.docs.len() && ctx.input.mouse_just_pressed(MouseButton::Left) {
            self.grab_part(ctx, cursor);
        }
        match &self.drag {
            Some(Drag::Orbit) => {
                self.orbit_yaw -= mouse_moved.x * 0.006;
                self.orbit_pitch = (self.orbit_pitch + mouse_moved.y * 0.006).clamp(-0.4, 1.5);
                if !ctx.input.mouse_pressed(MouseButton::Right) {
                    self.drag = None;
                }
            }
            Some(Drag::Pan) => {
                let scale = self.orbit_distance * 0.0016;
                let right = ctx.camera.right();
                let up = right.cross(ctx.camera.forward());
                self.orbit_target += up * (mouse_moved.y * scale) - right * (mouse_moved.x * scale);
                if !ctx.input.mouse_pressed(MouseButton::Middle) {
                    self.drag = None;
                }
            }
            Some(Drag::Part { .. }) => self.drag_part(ctx, cursor),
            _ => {}
        }
        let direction = Vec3::new(
            self.orbit_yaw.sin() * self.orbit_pitch.cos(),
            self.orbit_pitch.sin(),
            self.orbit_yaw.cos() * self.orbit_pitch.cos(),
        );
        ctx.camera.position = self.orbit_target + direction * self.orbit_distance;
        ctx.camera.look_at(self.orbit_target);
        // A capture close to a joint: `--camera` is then how far from it to look.
        if let (Some(avatar), Some(joint)) = (self.avatar, &self.focus)
            && let Some(at) = ctx.world.joint_transform(avatar, joint)
        {
            let away = ctx.config().camera_position.unwrap_or(Vec3::new(0.3, 0.1, 0.3));
            ctx.camera.position = at.position + away;
            ctx.camera.look_at(at.position);
        }
    }

    fn over_panels(&self, size: Vec2, cursor: Vec2) -> bool {
        cursor.x < LEFT_WIDTH || cursor.x > size.x - RIGHT_WIDTH || cursor.y > size.y - BOTTOM_HEIGHT
    }

    /// The part under the cursor.
    fn part_under(&self, ctx: &Context, cursor: Vec2) -> Option<Part> {
        let mut best = (GRAB_RADIUS, None);
        for part in PARTS {
            let at = self.part_at(part, &self.solved, &self.controls);
            if let Some(on_screen) = project(ctx, at) {
                let off = on_screen.distance(cursor);
                if off < best.0 {
                    best = (off, Some(part));
                }
            }
        }
        best.1
    }

    fn grab_part(&mut self, ctx: &Context, cursor: Vec2) {
        let (Some(part), Some(joints)) = (self.part_under(ctx, cursor), self.joints) else { return };
        let Some(doc) = self.doc() else { return };
        let before = doc.clip.clone();
        let shift = ctx.input.pressed(KeyCode::ShiftLeft) || ctx.input.pressed(KeyCode::ShiftRight);
        self.stop();
        let root = root_of(&self.controls);
        let pivot = match part {
            Part::Tip => root + self.solved.positions[joints.weapon],
            Part::ElbowR => root + self.solved.positions[joints.upper_r],
            Part::ElbowL => root + self.solved.positions[joints.upper_l],
            Part::KneeL => root + self.solved.positions[joints.thigh_l],
            Part::KneeR => root + self.solved.positions[joints.thigh_r],
            _ => Vec3::ZERO,
        };
        let anchor = self.part_at(part, &self.solved, &self.controls);
        let at = self.time;
        self.selected_part = part;
        let (plain, with_shift) = part.controls();
        self.selected = if shift { with_shift } else { plain };
        self.selected_event = None;
        if let Some(doc) = self.doc_mut() {
            doc.checkpoint();
        }
        self.drag = Some(Drag::Part { part, shift, before, at, press: cursor, anchor, pivot });
        self.drag_part(ctx, cursor);
    }

    fn drag_part(&mut self, ctx: &Context, cursor: Vec2) {
        let Some(Drag::Part { part, shift, before, at, press, anchor, pivot }) = self.drag.take() else { return };
        let moved = cursor - press;
        let forward = ctx.camera.forward();
        let point = plane_point(ctx, cursor, anchor, forward).unwrap_or(anchor);
        let angles = Vec3::new(moved.y, -moved.x, 0.0) * DEGREES_PER_PIXEL;
        let turning = Vec3::new(0.0, 0.0, moved.x) * DEGREES_PER_PIXEL;
        let arc = |from: Vec3, to: Vec3| Quat::from_rotation_arc(from.normalize_or_zero(), to.normalize_or_zero());
        let (plain, with_shift) = part.controls();
        let changes: Vec<(Control, Change)> = match part {
            Part::Root => {
                let on_floor = plane_point(ctx, cursor, anchor, Vec3::Y).unwrap_or(anchor);
                vec![(plain, Change::Offset((on_floor - anchor) * Vec3::new(1.0, 0.0, 1.0)))]
            }
            Part::Torso | Part::Head => vec![(plain, Change::Offset(if shift { turning } else { angles }))],
            Part::Hips | Part::FootL | Part::FootR if shift => vec![(with_shift, Change::Offset(angles))],
            Part::Tip if shift => {
                let axis = (anchor - pivot).normalize_or_zero();
                vec![(Control::Edge, Change::Rotate(Quat::from_axis_angle(axis, moved.x.to_radians() * 0.6)))]
            }
            Part::Tip => {
                let turn = arc(anchor - pivot, point - pivot);
                vec![(Control::Blade, Change::Rotate(turn)), (Control::Edge, Change::Rotate(turn))]
            }
            Part::ElbowR | Part::ElbowL | Part::KneeL | Part::KneeR => {
                vec![(plain, Change::Rotate(arc(anchor - pivot, point - pivot)))]
            }
            _ => vec![(plain, Change::Offset(point - anchor))],
        };
        let reach = self.reach;
        if let Some(doc) = self.doc_mut() {
            doc.deform(&before, at, &changes, reach);
        }
        if ctx.input.mouse_pressed(MouseButton::Left) {
            self.drag = Some(Drag::Part { part, shift, before, at, press, anchor, pivot });
        }
    }

    /// The bottom panel: the buttons, and the timeline's rows.
    fn timeline(&mut self, ctx: &mut Context, cursor: Vec2) {
        let size = ctx.ui.size();
        let panel = Rect::new(Vec2::new(0.0, size.y - BOTTOM_HEIGHT), Vec2::new(size.x - RIGHT_WIDTH, BOTTOM_HEIGHT));
        ctx.ui.panel(panel, PANEL, LINE);
        let mut x = panel.min.x + 8.0;
        let top = panel.min.y + 6.0;
        let label = if self.playing { "Pausar" } else { "Tocar" };
        if ctx.ui.button(Rect::new(Vec2::new(x, top), Vec2::new(70.0, ROW)), label) {
            self.toggle_play();
        }
        x += 76.0;
        if ctx.ui.button(Rect::new(Vec2::new(x, top), Vec2::new(28.0, ROW)), "<") {
            self.stop();
            self.time = snap((self.time - 1.0 / POSE_FPS).max(0.0));
        }
        x += 32.0;
        if ctx.ui.button(Rect::new(Vec2::new(x, top), Vec2::new(28.0, ROW)), ">") {
            self.stop();
            self.time = snap((self.time + 1.0 / POSE_FPS).min(self.duration()));
        }
        x += 36.0;
        ctx.ui.choice(
            Rect::new(Vec2::new(x, top), Vec2::new(226.0, ROW)),
            "Velocidade",
            &["100%", "50%", "25%", "10%"],
            &mut self.speed,
        );
        x += 232.0;
        ctx.ui.toggle(Rect::new(Vec2::new(x, top), Vec2::new(124.0, ROW)), "Repetir", &mut self.repeat);
        x += 132.0;
        let frame = (self.time * POSE_FPS).round() as i32;
        let clock = format!("{:.2} s  quadro {frame}", self.time);
        ctx.ui.text(Vec2::new(x, top + 5.0), &clock, &TextStyle::new(14.0).color(TEXT));

        // Rows.
        let area = Rect::new(
            Vec2::new(panel.min.x + 62.0, top + ROW + 6.0),
            Vec2::new(panel.size.x - 74.0, BOTTOM_HEIGHT - ROW - 22.0),
        );
        let duration = self.duration().max(1e-3);
        let x_of = |t: f32| area.min.x + t / duration * area.size.x;
        let row = |k: usize| {
            Rect::new(Vec2::new(area.min.x, area.min.y + 18.0 + k as f32 * 26.0), Vec2::new(area.size.x, 24.0))
        };
        let small = TextStyle::new(11.0).color(DIM);
        ctx.ui.rect(area, Vec4::new(0.11, 0.11, 0.13, 1.0));
        let mut tenth = 0;
        while tenth as f32 * 0.1 <= duration + 1e-3 {
            let at = x_of(tenth as f32 * 0.1);
            let long = tenth % 5 == 0;
            ctx.ui
                .rect(Rect::new(Vec2::new(at, area.min.y), Vec2::new(1.0, if long { area.size.y } else { 7.0 })), LINE);
            if long {
                ctx.ui.text(Vec2::new(at + 3.0, area.min.y + 1.0), &format!("{:.1}", tenth as f32 * 0.1), &small);
            }
            tenth += 1;
        }
        let names = [("Marcas", 0), ("Tudo", 1)];
        for (name, k) in names {
            ctx.ui.text(Vec2::new(panel.min.x + 6.0, row(k).min.y + 5.0), name, &TextStyle::new(12.0).color(DIM));
        }
        ctx.ui.text(
            Vec2::new(panel.min.x + 6.0, row(2).min.y + 5.0),
            short_name(self.selected),
            &TextStyle::new(12.0).color(ACCENT),
        );

        if let Some(doc) = self.docs.get(self.current) {
            // Where the blade hurts, and where the next blow can come in.
            if let (Some(on), Some(off)) = (doc.event_time("hit_on"), doc.event_time("hit_off")) {
                let band = Rect::new(Vec2::new(x_of(on), area.min.y), Vec2::new(x_of(off) - x_of(on), area.size.y));
                ctx.ui.rect(band, Vec4::new(0.8, 0.15, 0.1, 0.22));
            }
            for (index, (t, name)) in doc.clip.events.iter().enumerate() {
                let (_, _, short, color) =
                    EVENTS.iter().find(|e| e.0 == name).copied().unwrap_or(("", "", name, [0.8, 0.8, 0.8]));
                let color = Vec4::new(color[0], color[1], color[2], 1.0);
                let at = x_of(*t);
                let selected = self.selected_event == Some(index);
                ctx.ui.rect(Rect::new(Vec2::new(at - 1.0, row(0).min.y), Vec2::new(2.0, 24.0)), color);
                diamond(&mut ctx.ui, Vec2::new(at, row(0).min.y + 6.0), if selected { 6.0 } else { 4.0 }, color);
                // Marks are often close together: alternate between two heights.
                let y = row(0).min.y + if index % 2 == 0 { 0.0 } else { 12.0 };
                ctx.ui.text(Vec2::new(at + 5.0, y), short, &TextStyle::new(11.0).color(color));
            }
            // Every key of the clip, then those of what's selected.
            let mut times: Vec<f32> = doc.clip.tracks.values().flatten().map(|k| k.0).collect();
            times.sort_by(f32::total_cmp);
            times.dedup_by(|a, b| (*a - *b).abs() < 0.5 / POSE_FPS);
            for t in &times {
                diamond(&mut ctx.ui, Vec2::new(x_of(*t), row(1).min.y + 12.0), 3.5, DIM);
            }
            for key in doc.clip.tracks.get(&self.selected).into_iter().flatten() {
                let here = (key.0 - self.time).abs() < 0.5 / POSE_FPS;
                diamond(
                    &mut ctx.ui,
                    Vec2::new(x_of(key.0), row(2).min.y + 12.0),
                    if here { 6.0 } else { 4.5 },
                    if here { TEXT } else { ACCENT },
                );
            }
        }
        ctx.ui.rect(Rect::new(Vec2::new(x_of(self.time) - 1.0, area.min.y), Vec2::new(2.0, area.size.y)), TEXT);

        // Mouse.
        if self.drag.is_none() && area.contains(cursor) && ctx.input.mouse_just_pressed(MouseButton::Left) {
            self.grab_timeline(cursor, &area, duration, [row(0), row(1), row(2)]);
        }
        let held = ctx.input.mouse_pressed(MouseButton::Left);
        let t = ((cursor.x - area.min.x) / area.size.x * duration).clamp(0.0, duration);
        match self.drag.take() {
            Some(Drag::Scrub) if held => {
                self.time = snap(t).min(duration);
                self.drag = Some(Drag::Scrub);
            }
            Some(Drag::Keys { controls, before, from, mut moved }) if held => {
                moved |= (snap(t) - from).abs() >= 0.5 / POSE_FPS;
                let mut at = from;
                if moved {
                    if let Some(doc) = self.doc_mut() {
                        at = doc.retime(&before, &controls, from, t);
                    }
                    self.time = at;
                }
                self.drag = Some(Drag::Keys { controls, before, from, moved });
            }
            Some(Drag::Event { index, mut moved }) if held => {
                moved |= self
                    .docs
                    .get(self.current)
                    .and_then(|doc| doc.clip.events.get(index))
                    .is_some_and(|e| (e.0 - snap(t)).abs() >= 0.5 / POSE_FPS);
                if moved && let Some(doc) = self.doc_mut() {
                    if let Some(event) = doc.clip.events.get_mut(index) {
                        event.0 = snap(t);
                    }
                    doc.touched();
                }
                self.drag = Some(Drag::Event { index, moved });
            }
            Some(Drag::Scrub | Drag::Keys { .. } | Drag::Event { .. }) => {}
            other => self.drag = other,
        }
    }

    fn grab_timeline(&mut self, cursor: Vec2, area: &Rect, duration: f32, rows: [Rect; 3]) {
        let x_of = |t: f32| area.min.x + t / duration * area.size.x;
        self.stop();
        let Some(doc) = self.docs.get_mut(self.current) else {
            self.drag = Some(Drag::Scrub);
            return;
        };
        let near = |t: f32| (x_of(t) - cursor.x).abs() < 6.0;
        if rows[0].contains(cursor)
            && let Some(index) = doc.clip.events.iter().position(|e| near(e.0))
        {
            doc.checkpoint();
            self.selected_event = Some(index);
            self.time = snap(doc.clip.events[index].0);
            self.drag = Some(Drag::Event { index, moved: false });
            return;
        }
        let keys_of = |controls: &[Control]| -> Option<f32> {
            let times = controls.iter().filter_map(|c| doc.clip.tracks.get(c)).flatten().map(|k| k.0);
            times
                .filter(|t| near(*t))
                .min_by(|a, b| (x_of(*a) - cursor.x).abs().total_cmp(&(x_of(*b) - cursor.x).abs()))
        };
        let grabbed = if rows[2].contains(cursor) {
            let controls = with_partner(self.selected);
            keys_of(&controls).map(|t| (controls, t))
        } else if rows[1].contains(cursor) {
            keys_of(&Control::ALL).map(|t| (Control::ALL.to_vec(), t))
        } else {
            None
        };
        self.selected_event = None;
        if let Some((controls, from)) = grabbed {
            let before = doc.clip.clone();
            doc.checkpoint();
            self.time = from;
            self.drag = Some(Drag::Keys { controls, before, from, moved: false });
        } else {
            self.drag = Some(Drag::Scrub);
        }
    }

    /// The panel with the clips.
    fn clip_list(&mut self, ctx: &mut Context) {
        let size = ctx.ui.size();
        let panel = Rect::new(Vec2::ZERO, Vec2::new(LEFT_WIDTH, size.y - BOTTOM_HEIGHT));
        ctx.ui.panel(panel, PANEL, LINE);
        ctx.ui.text(Vec2::new(10.0, 8.0), "Clipes", &TextStyle::new(15.0).color(ACCENT));
        let mut y = 30.0;
        let mut open = None;
        let names: Vec<(String, bool)> = self.docs.iter().map(|doc| (doc.name.clone(), doc.dirty)).collect();
        for (index, (name, dirty)) in names.iter().enumerate() {
            let label = if *dirty { format!("{name} *") } else { name.clone() };
            let rect = Rect::new(Vec2::new(8.0, y), Vec2::new(LEFT_WIDTH - 16.0, 22.0));
            if ctx.ui.button(rect, &label) {
                open = Some(index);
            }
            if index == self.current {
                ctx.ui.rect(Rect::new(Vec2::new(4.0, y), Vec2::new(3.0, 22.0)), ACCENT);
            }
            y += 24.0;
        }
        let bottom = size.y - BOTTOM_HEIGHT - 66.0;
        y += 6.0;
        if y < bottom {
            ctx.ui.text(Vec2::new(10.0, y), "Só ver", &TextStyle::new(12.0).color(DIM));
            y += 18.0;
        }
        let first = self.docs.len();
        for (index, name) in self.view_only.clone().iter().enumerate() {
            if y >= bottom {
                break;
            }
            let rect = Rect::new(Vec2::new(8.0, y), Vec2::new(LEFT_WIDTH - 16.0, 20.0));
            if ctx.ui.button(rect, name) {
                open = Some(first + index);
            }
            if first + index == self.current {
                ctx.ui.rect(Rect::new(Vec2::new(4.0, y), Vec2::new(3.0, 20.0)), ACCENT);
            }
            y += 22.0;
        }
        let duplicate = Rect::new(Vec2::new(8.0, size.y - BOTTOM_HEIGHT - 60.0), Vec2::new(LEFT_WIDTH - 16.0, ROW));
        let new = Rect::new(Vec2::new(8.0, size.y - BOTTOM_HEIGHT - 32.0), Vec2::new(LEFT_WIDTH - 16.0, ROW));
        let editable = self.current < self.docs.len();
        if ctx.ui.button_if(editable, duplicate, "Duplicar")
            && let Some(doc) = self.doc()
        {
            let copy = Doc::new(self.free_name(&doc.name.clone()), doc.clip.clone());
            let name = copy.name.clone();
            self.add_doc(copy);
            open = self.docs.iter().position(|doc| doc.name == name);
        }
        if ctx.ui.button_if(editable, new, "Novo desta pose") {
            let copy = Doc::new(self.free_name("novo"), edit::clip_from_pose(&self.controls));
            let name = copy.name.clone();
            self.add_doc(copy);
            open = self.docs.iter().position(|doc| doc.name == name);
        }
        if let Some(index) = open {
            self.open(index);
        }
    }

    /// The panel with what's selected, the clip's settings and the file.
    fn side_panel(&mut self, ctx: &mut Context) {
        let size = ctx.ui.size();
        let panel = Rect::new(Vec2::new(size.x - RIGHT_WIDTH, 0.0), Vec2::new(RIGHT_WIDTH, size.y));
        ctx.ui.panel(panel, PANEL, LINE);
        let x = panel.min.x + 8.0;
        let width = RIGHT_WIDTH - 16.0;
        let mut y = 8.0;
        let line = |y: &mut f32| {
            let rect = Rect::new(Vec2::new(x, *y), Vec2::new(width, ROW));
            *y += ROW + 3.0;
            rect
        };
        let halves = |y: &mut f32| {
            let half = (width - 4.0) / 2.0;
            let rects = (
                Rect::new(Vec2::new(x, *y), Vec2::new(half, ROW)),
                Rect::new(Vec2::new(x + half + 4.0, *y), Vec2::new(half, ROW)),
            );
            *y += ROW + 3.0;
            rects
        };
        if !ctx.input.mouse_pressed(MouseButton::Left) {
            self.slider_open = false;
        }
        let Some(views) = &self.views else {
            ctx.ui.text(Vec2::new(x, y), "Clipe só para ver:", &TextStyle::new(14.0).color(DIM));
            ctx.ui.text(
                Vec2::new(x, y + 20.0),
                "vem do arquivo do modelo e não tem poses para editar.",
                &TextStyle::new(12.0).color(DIM).wrap(width),
            );
            return;
        };
        if self.current >= self.docs.len() {
            ctx.ui.text(Vec2::new(x, y), "Clipe só para ver:", &TextStyle::new(14.0).color(DIM));
            ctx.ui.text(
                Vec2::new(x, y + 20.0),
                "vem do arquivo do modelo e não tem poses para editar.",
                &TextStyle::new(12.0).color(DIM).wrap(width),
            );
            return;
        }
        let raw = views.raw.sample(self.time);
        let name = self.docs[self.current].name.clone();
        let dirty = if self.docs[self.current].dirty { " (não salvo)" } else { "" };
        ctx.ui.text(Vec2::new(x, y), &format!("{name}{dirty}"), &TextStyle::new(15.0).color(ACCENT));
        y += 24.0;

        // The selected control.
        let names: Vec<&str> = Control::ALL.iter().map(|&c| control_name(c)).collect();
        let mut index = Control::ALL.iter().position(|&c| c == self.selected).unwrap_or(0);
        if ctx.ui.choice(line(&mut y), "Controle", &names, &mut index) {
            self.select(Control::ALL[index]);
        }
        let selected = self.selected;
        let (sliders, range, step) = control_sliders(selected);
        let mut value = raw[selected];
        let mut changed = false;
        for &(axis, label) in sliders {
            let decimals = if step < 0.5 { 3 } else { 0 };
            changed |= ctx.ui.number_slider(line(&mut y), label, &mut value[axis], -range..=range, step, decimals);
        }
        for _ in sliders.len()..3 {
            y += ROW + 3.0;
        }
        if changed {
            self.stop();
            let time = self.time;
            let open = std::mem::replace(&mut self.slider_open, true);
            let doc = &mut self.docs[self.current];
            if !open {
                doc.checkpoint();
            }
            doc.set_key(selected, time, value);
        }
        let (key_button, delete_button) = halves(&mut y);
        if ctx.ui.button(key_button, "Chave (K)") {
            self.key_here();
        }
        let has_key = self.docs[self.current].has_key(selected, self.time);
        if ctx.ui.button_if(has_key, delete_button, "Tirar chave") {
            self.selected_event = None;
            self.delete_selected();
        }
        ctx.ui.number_slider(line(&mut y), "Alcance (s)", &mut self.reach, 0.0..=0.4, 0.01, 2);
        y += 6.0;

        // The clip.
        let mut duration = self.docs[self.current].clip.duration;
        if ctx.ui.number_slider(line(&mut y), "Duração (s)", &mut duration, 0.1..=6.0, 1.0 / POSE_FPS, 2) {
            let open = std::mem::replace(&mut self.slider_open, true);
            let doc = &mut self.docs[self.current];
            if !open {
                doc.checkpoint();
            }
            doc.clip.duration = duration;
            doc.touched();
            self.time = self.time.min(duration);
        }
        let mut springs = self.docs[self.current].clip.springs;
        if ctx.ui.toggle(line(&mut y), "Molas no clipe", &mut springs) {
            let doc = &mut self.docs[self.current];
            doc.checkpoint();
            doc.clip.springs = springs;
            doc.touched();
        }
        ctx.ui.toggle(line(&mut y), "Ver molas parado", &mut self.springs_when_stopped);
        ctx.ui.toggle(line(&mut y), "Câmera segue", &mut self.follow);
        ctx.ui.number_slider(line(&mut y), "Alvo (m)", &mut self.target_distance, 0.5..=4.0, 0.05, 2);
        y += 6.0;

        // Markers.
        let options: Vec<&str> = EVENTS.iter().map(|e| e.1).collect();
        ctx.ui.choice(line(&mut y), "Marca", &options, &mut self.new_event);
        let (add, remove) = halves(&mut y);
        if ctx.ui.button(add, "Marcar") {
            let (time, event) = (snap(self.time), EVENTS[self.new_event].0);
            let doc = &mut self.docs[self.current];
            doc.checkpoint();
            doc.add_event(time, event);
        }
        if ctx.ui.button_if(self.selected_event.is_some(), remove, "Tirar marca") {
            self.delete_selected();
        }
        y += 6.0;

        // How the pose is doing.
        let style = |bad: bool| TextStyle::new(13.0).color(if bad { BAD } else { TEXT });
        let solved = &self.solved;
        ctx.ui.text(Vec2::new(x, y), &format!("Pulso {:.0}°", solved.wrist), &style(solved.wrist > 65.0));
        ctx.ui.text(Vec2::new(x + 98.0, y), &format!("Giro {:.0}°", solved.twist), &style(solved.twist > 100.0));
        let short = solved.short_hand.max(solved.short_foot) * 100.0;
        ctx.ui.text(Vec2::new(x + 192.0, y), &format!("Falta {short:.0} cm"), &style(short > 2.0));
        y += 22.0;

        let (undo, redo) = halves(&mut y);
        if ctx.ui.button(undo, "Desfazer") {
            self.docs[self.current].undo();
        }
        if ctx.ui.button(redo, "Refazer") {
            self.docs[self.current].redo();
        }
        if ctx.ui.button(line(&mut y), "Salvar (Ctrl+S)") {
            self.save_all();
        }
        ctx.ui.text(Vec2::new(x, y), &self.status, &TextStyle::new(12.0).color(DIM).wrap(width));
    }

    /// What's drawn over the 3D view: handles, paths and the blow's result.
    fn overlay(&mut self, ctx: &mut Context, cursor: Vec2) {
        let size = ctx.ui.size();
        let help = "Arraste as alças. Shift: gira. Botão direito: orbita. Meio: move. Roda: zoom. Espaço: toca. Setas: quadro a quadro. Ctrl+Z: desfaz.";
        ctx.ui.text(
            Vec2::new(LEFT_WIDTH + 10.0, 8.0),
            help,
            &TextStyle::new(12.0).color(DIM).wrap(size.x - LEFT_WIDTH - RIGHT_WIDTH - 20.0),
        );
        if let (Some(views), Some(doc)) = (&self.views, self.doc()) {
            let window = doc.event_time("hit_on").zip(doc.event_time("hit_off"));
            for (k, at) in views.tip_path.iter().enumerate() {
                let t = k as f32 / POSE_FPS;
                let hurting = window.is_some_and(|(on, off)| t >= on && t <= off);
                if let Some(p) = project(ctx, *at) {
                    let side = if hurting { 4.0 } else { 3.0 };
                    let color = if hurting { BAD } else { Vec4::new(1.0, 0.78, 0.42, 0.55) };
                    ctx.ui.rect(Rect::new(p - Vec2::splat(side / 2.0), Vec2::splat(side)), color);
                }
            }
            if self.selected_part != Part::Tip {
                for at in &views.part_path {
                    if let Some(p) = project(ctx, *at) {
                        ctx.ui.rect(Rect::new(p - Vec2::splat(1.5), Vec2::splat(3.0)), Vec4::new(0.35, 0.8, 1.0, 0.6));
                    }
                }
            }
        }
        if self.current < self.docs.len() && !self.playing {
            let hovered = if self.over_panels(size, cursor) { None } else { self.part_under(ctx, cursor) };
            for part in PARTS {
                let Some(p) = project(ctx, self.part_at(part, &self.solved, &self.controls)) else { continue };
                let chosen = part == self.selected_part;
                let side = if chosen || hovered == Some(part) { 13.0 } else { 9.0 };
                let color = part.color();
                ctx.ui.rect(
                    Rect::new(p - Vec2::splat(side / 2.0 + 1.0), Vec2::splat(side + 2.0)),
                    Vec4::new(0.0, 0.0, 0.0, 0.85),
                );
                ctx.ui
                    .rect(Rect::new(p - Vec2::splat(side / 2.0), Vec2::splat(side)), if chosen { TEXT } else { color });
            }
            if let Some(part) = hovered {
                ctx.ui.text(cursor + Vec2::new(14.0, -6.0), part.name(), &TextStyle::new(13.0).color(TEXT));
            }
        }
        let at = Vec2::new(size.x / 2.0 - RIGHT_WIDTH / 2.0 + LEFT_WIDTH / 2.0, 38.0);
        let centered = |color| TextStyle::new(20.0).color(color).anchor(Anchor::Top);
        if let Some(t) = self.strike.hit_at {
            ctx.ui.text(at, &format!("ACERTOU em {t:.2} s"), &centered(GOOD));
        } else if let Some(nearest) = self.strike.nearest
            && self.doc().and_then(|doc| doc.event_time("hit_off")).is_some_and(|off| self.time > off)
        {
            ctx.ui.text(at, &format!("Errou por {:.0} cm", nearest.max(0.0) * 100.0), &centered(BAD));
        }
    }

    /// The blade's trail while it hurts.
    fn update_trails(&self, ctx: &mut Context) {
        let (Some(trails), Some(joints)) = (self.trails, self.joints) else { return };
        let hurting = self.playing
            && self.doc().is_some_and(|doc| {
                doc.event_time("hit_on")
                    .zip(doc.event_time("hit_off"))
                    .is_some_and(|(on, off)| self.time >= on && self.time <= off)
            });
        if self.solved.positions.is_empty() {
            return;
        }
        let (base, tip) = blade_of(joints, &self.solved, root_of(&self.controls));
        for (trail, at) in trails.into_iter().zip([tip, base.lerp(tip, 0.5)]) {
            if let Some(transform) = ctx.world.get_mut::<Transform>(trail) {
                transform.position = at;
            }
            if let Some(system) = ctx.world.get_mut::<ParticleSystem>(trail) {
                system.emitting = hurting;
            }
        }
    }
}

impl Game for Tool {
    fn scene_loaded(&mut self, ctx: &mut Context) {
        self.avatar = ctx.world.find("avatar");
        self.target = ctx.world.find("target");
        let mesh = match load_model(MODEL.as_ref(), DEFAULT_MAX_TEXTURE_SIZE) {
            Ok(mesh) => mesh,
            Err(err) => {
                eprintln!("[animator] {err:#}");
                return;
            }
        };
        let Some(skeleton) = mesh.skeleton.clone() else { return };
        self.rig = Rig::new(&skeleton);
        self.joints = joints_of(&skeleton);
        if self.docs.is_empty() {
            self.docs = load_pose_clips(MODEL.as_ref()).into_iter().map(|(name, clip)| Doc::new(name, clip)).collect();
        }
        self.view_only = skeleton
            .clips
            .iter()
            .map(|c| c.name.clone())
            .filter(|name| !name.starts_with("__"))
            .filter(|name| !self.docs.iter().any(|doc| doc.name == *name))
            .collect();
        self.skeleton = Some(skeleton);
        match load_model(SWORD.as_ref(), DEFAULT_MAX_TEXTURE_SIZE) {
            Ok(sword) => {
                let sword = ctx.world.add_mesh(sword);
                let hold = Transform {
                    position: Vec3::ZERO,
                    rotation: Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2),
                    scale: Vec3::splat(SWORD_SCALE),
                };
                for entity in [self.avatar, self.target].into_iter().flatten() {
                    ctx.world.attach(entity, "weapon.R", sword, hold);
                }
            }
            Err(err) => eprintln!("[animator] {err:#}"),
        }
        self.hit_effect = load_effect(HIT).map_err(|err| eprintln!("[animator] {err:#}")).ok();
        self.trails = load_effect(BLADE_TRAIL).map_err(|err| eprintln!("[animator] {err:#}")).ok().map(|effect| {
            [0, 1].map(|seed| {
                let system = ParticleSystem::new(effect.clone(), Vec3::ZERO, 1.0, seed);
                ctx.world.ecs.spawn((Transform::IDENTITY, system))
            })
        });
        // `sword_1@0.24`: opens on that clip at that moment. `/hand.R` after it looks at that joint.
        let start = ctx.config().menu.clone().unwrap_or_else(|| "sword_1".to_string());
        let (start, focus) =
            start.split_once('/').map_or((start.as_str(), None), |(start, joint)| (start, Some(joint)));
        self.focus = focus.map(str::to_string);
        let (name, at) = start.split_once('@').unwrap_or((start, "0"));
        // `clip@seconds+`: as it plays, with springs, and not the exact keys.
        self.springs_when_stopped |= at.ends_with('+');
        self.open_at = Some((name.to_string(), at.trim_end_matches('+').parse().unwrap_or(0.0)));
    }

    fn update(&mut self, ctx: &mut Context) {
        // A capture shows what its arguments ask for, whatever the real keys and mouse do.
        if ctx.config().screenshot.is_some() {
            ctx.input.clear();
        }
        // The keyboard belongs to the shortcuts here, not to moving a menu's focus.
        ctx.ui.theme.text_size = 13.0;
        let input = UiInput {
            up: false,
            down: false,
            left: false,
            right: false,
            activate: false,
            ..UiInput::from_input(&ctx.input, ctx.ui.scale())
        };
        ctx.ui.set_input(input);
        let cursor = input.cursor.unwrap_or(Vec2::ZERO);
        if let Some((name, at)) = self.open_at.take() {
            let all = self.docs.iter().map(|doc| doc.name.as_str()).chain(self.view_only.iter().map(String::as_str));
            if let Some(index) = all.collect::<Vec<_>>().iter().position(|n| *n == name) {
                self.open(index);
            }
            self.time = at;
        }
        self.shortcuts(ctx);
        self.refresh_views();
        self.advance(ctx, ctx.time.delta());
        self.show_pose(ctx);
        self.viewport(ctx, cursor);
        self.show_pose(ctx);
        self.clip_list(ctx);
        self.timeline(ctx, cursor);
        self.refresh_views();
        self.side_panel(ctx);
        self.overlay(ctx, cursor);
        self.update_trails(ctx);
        self.last_cursor = cursor;
    }
}

fn root_of(controls: &Controls) -> Vec3 {
    let root = controls[Control::Root];
    Vec3::new(root.x, 0.0, root.z)
}

fn blade_of(j: Joints, solved: &SolvedPose, root: Vec3) -> (Vec3, Vec3) {
    let grip = root + solved.positions[j.weapon];
    let along = solved.turns[j.weapon] * Vec3::NEG_Z;
    (grip + along * BLADE.0, grip + along * BLADE.1)
}

fn joints_of(skeleton: &Skeleton) -> Option<Joints> {
    let joint = |name: &str| skeleton.joint(name);
    Some(Joints {
        hips: joint("hips")?,
        chest: joint("chest")?,
        head: joint("head")?,
        weapon: joint("weapon.R")?,
        hand_l: joint("hand.L")?,
        forearm_r: joint("forearm.R")?,
        forearm_l: joint("forearm.L")?,
        upper_r: joint("upper_arm.R")?,
        upper_l: joint("upper_arm.L")?,
        shin_l: joint("shin.L")?,
        shin_r: joint("shin.R")?,
        thigh_l: joint("thigh.L")?,
        thigh_r: joint("thigh.R")?,
        foot_l: joint("foot.L")?,
        foot_r: joint("foot.R")?,
    })
}

fn short_name(control: Control) -> &'static str {
    match control {
        Control::Root => "Chão",
        Control::Hips | Control::HipsRot => "Quadril",
        Control::Torso => "Tronco",
        Control::Head => "Cabeça",
        Control::FootL | Control::FootLRot => "Pé esq.",
        Control::FootR | Control::FootRRot => "Pé dir.",
        Control::ToeL | Control::ToeR => "Dedos",
        Control::KneeL | Control::KneeR => "Joelho",
        Control::HandR | Control::HandL => "Mão",
        Control::Blade | Control::Edge => "Lâmina",
        Control::ElbowR | Control::ElbowL => "Cotov.",
        Control::Tuck => "Encolhe",
    }
}

/// Where a point in the world falls on screen, in logical pixels.
fn project(ctx: &Context, point: Vec3) -> Option<Vec2> {
    let clip = ctx.camera.view_projection(ctx.aspect()) * point.extend(1.0);
    if clip.w <= 0.0 {
        return None;
    }
    let size = ctx.ui.size();
    Some(Vec2::new((clip.x / clip.w + 1.0) * 0.5 * size.x, (1.0 - clip.y / clip.w) * 0.5 * size.y))
}

/// Where the ray through a point on screen crosses a plane.
fn plane_point(ctx: &Context, cursor: Vec2, through: Vec3, normal: Vec3) -> Option<Vec3> {
    let size = ctx.ui.size();
    let inverse = ctx.camera.view_projection(ctx.aspect()).inverse();
    let (x, y) = (cursor.x / size.x * 2.0 - 1.0, 1.0 - cursor.y / size.y * 2.0);
    let near = inverse.project_point3(Vec3::new(x, y, 0.0));
    let far = inverse.project_point3(Vec3::new(x, y, 1.0));
    let along = far - near;
    let facing = along.dot(normal);
    if facing.abs() < 1e-6 {
        return None;
    }
    let u = (through - near).dot(normal) / facing;
    (u > 0.0).then(|| near + along * u)
}

fn diamond(ui: &mut Ui, at: Vec2, radius: f32, color: Vec4) {
    // Rows of pixels, wider towards the middle: the HUD only has rectangles.
    let radius = radius.round();
    let mut dy = -radius;
    while dy <= radius {
        let half = radius - dy.abs();
        ui.rect(Rect::new(Vec2::new(at.x - half - 0.5, at.y + dy), Vec2::new(half * 2.0 + 1.0, 1.0)), color);
        dy += 1.0;
    }
}

/// Shortest distance between two segments.
fn segment_distance(a0: Vec3, a1: Vec3, b0: Vec3, b1: Vec3) -> f32 {
    let (d1, d2, r) = (a1 - a0, b1 - b0, a0 - b0);
    let (aa, ee, f, c, b) = (d1.dot(d1), d2.dot(d2), d2.dot(r), d1.dot(r), d1.dot(d2));
    let denominator = aa * ee - b * b;
    let mut s = if denominator > 1e-9 { ((b * f - c * ee) / denominator).clamp(0.0, 1.0) } else { 0.0 };
    let mut t = (b * s + f) / ee.max(1e-9);
    if t < 0.0 {
        t = 0.0;
        s = (-c / aa.max(1e-9)).clamp(0.0, 1.0);
    } else if t > 1.0 {
        t = 1.0;
        s = ((b - c) / aa.max(1e-9)).clamp(0.0, 1.0);
    }
    (a0 + d1 * s).distance(b0 + d2 * t)
}

fn main() -> Result<()> {
    let config = EngineConfig {
        title: "Grimora Animator".into(),
        scene: Some(SCENE.into()),
        escape_quits: false,
        ..Default::default()
    }
    .with_cli()?;
    grimora::run(config, Tool::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start() -> (Tool, Context) {
        let mut tool = Tool::new();
        let config = EngineConfig { scene: Some(SCENE.into()), audio: false, ..Default::default() };
        let mut ctx = Context::headless(config, &mut tool).unwrap();
        tool.folder = std::env::temp_dir().join(format!("grimora_animator_tool_{}", std::process::id()));
        ctx.headless_frame(&mut tool, 1.0 / 60.0).unwrap();
        (tool, ctx)
    }

    fn frame(tool: &mut Tool, ctx: &mut Context) {
        ctx.headless_frame(tool, 1.0 / 60.0).unwrap();
    }

    fn physical(ctx: &Context, logical: Vec2) -> Vec2 {
        logical * ctx.ui.scale()
    }

    #[test]
    fn it_opens_on_the_first_blow_with_the_saved_clips() {
        let (tool, _ctx) = start();
        let current = tool.doc().unwrap();
        assert_eq!(current.name, "sword_1");
        assert!(tool.docs.len() >= 8);
        // Native locomotion must not be overridden by an old pose document.
        for name in [
            "sword_idle",
            "sword_walk",
            "sword_run",
            "jump",
            "jump_fall",
            "jump_land",
            "jump_land_walk",
            "jump_land_run",
        ] {
            assert!(!tool.docs.iter().any(|doc| doc.name == name));
            assert!(tool.view_only.iter().any(|clip| clip == name));
        }
        assert!(tool.view_only.iter().all(|name| !name.starts_with("__")));
        assert!(tool.view_only.iter().any(|name| name == "idle"));
    }

    #[test]
    fn dragging_the_hand_moves_it_keys_it_and_undo_brings_it_back() {
        let (mut tool, mut ctx) = start();
        // Edit the reachable recovery, not the old rig's extended follow-through.
        // A clamped IK target cannot stay under the pointer in a translation test.
        const EDIT_AT: f32 = 37.0 / POSE_FPS;
        tool.time = EDIT_AT;
        frame(&mut tool, &mut ctx);
        assert!(tool.solved.short_hand < 1e-4, "the drag fixture must be reachable");
        let was = tool.part_at(Part::HandR, &tool.solved, &tool.controls);
        let keys_before = tool.doc().unwrap().clip.tracks[&Control::HandR].len();
        assert!(!tool.doc().unwrap().has_key(Control::HandR, EDIT_AT), "the fixture must insert a new key");
        let on_screen = project(&ctx, was).unwrap();
        let press = physical(&ctx, on_screen);
        ctx.input.set_cursor(press);
        frame(&mut tool, &mut ctx);
        ctx.input.button_down(MouseButton::Left);
        frame(&mut tool, &mut ctx);
        assert!(matches!(tool.drag, Some(Drag::Part { part: Part::HandR, .. })));
        let to = physical(&ctx, on_screen + Vec2::new(0.0, -30.0));
        ctx.input.set_cursor(to);
        frame(&mut tool, &mut ctx);
        ctx.input.button_up(MouseButton::Left);
        frame(&mut tool, &mut ctx);
        assert!(tool.drag.is_none());
        let now = tool.part_at(Part::HandR, &tool.solved, &tool.controls);
        assert!(tool.solved.short_hand < 1e-4, "the dragged target must remain reachable");
        assert!(now.y > was.y + 0.03, "{was} {now}");
        // On screen the hand stayed under the mouse.
        assert!(project(&ctx, now).unwrap().distance(on_screen + Vec2::new(0.0, -30.0)) < 2.0);
        let doc = tool.doc().unwrap();
        assert!(doc.dirty);
        assert!(doc.has_key(Control::HandR, EDIT_AT));
        assert_eq!(doc.clip.tracks[&Control::HandR].len(), keys_before + 1);
        ctx.input.key_down(KeyCode::ControlLeft);
        ctx.input.key_down(KeyCode::KeyZ);
        frame(&mut tool, &mut ctx);
        ctx.input.key_up(KeyCode::KeyZ);
        ctx.input.key_up(KeyCode::ControlLeft);
        frame(&mut tool, &mut ctx);
        assert_eq!(tool.doc().unwrap().clip.tracks[&Control::HandR].len(), keys_before);
        let back = tool.part_at(Part::HandR, &tool.solved, &tool.controls);
        assert!(back.distance(was) < 1e-3);
    }

    #[test]
    fn dragging_the_tip_turns_the_blade_and_its_edge_stays_across_it() {
        let (mut tool, mut ctx) = start();
        tool.time = 0.5;
        frame(&mut tool, &mut ctx);
        let was = tool.part_at(Part::Tip, &tool.solved, &tool.controls);
        let on_screen = project(&ctx, was).unwrap();
        ctx.input.set_cursor(physical(&ctx, on_screen));
        frame(&mut tool, &mut ctx);
        ctx.input.button_down(MouseButton::Left);
        frame(&mut tool, &mut ctx);
        ctx.input.set_cursor(physical(&ctx, on_screen + Vec2::new(0.0, -60.0)));
        frame(&mut tool, &mut ctx);
        ctx.input.button_up(MouseButton::Left);
        frame(&mut tool, &mut ctx);
        let doc = tool.doc().unwrap();
        assert!(doc.has_key(Control::Blade, 0.5) && doc.has_key(Control::Edge, 0.5));
        assert_ne!(tool.part_at(Part::Tip, &tool.solved, &tool.controls), was);
        assert!(
            project(&ctx, tool.part_at(Part::Tip, &tool.solved, &tool.controls))
                .unwrap()
                .distance(on_screen + Vec2::new(0.0, -60.0))
                < 4.0
        );
    }

    #[test]
    fn the_first_blow_reaches_a_target_in_range_and_not_one_far_away() {
        let (mut tool, mut ctx) = start();
        tool.target_distance = 1.3;
        tool.toggle_play();
        for _ in 0..40 {
            frame(&mut tool, &mut ctx);
        }
        assert!(tool.strike.hit_at.is_some());
        tool.stop();
        tool.time = 0.0;
        tool.target_distance = 4.0;
        tool.toggle_play();
        for _ in 0..40 {
            frame(&mut tool, &mut ctx);
        }
        assert!(tool.strike.hit_at.is_none());
        assert!(tool.strike.nearest.unwrap() > 1.0);
    }

    #[test]
    fn dragging_on_the_timeline_scrubs_and_retimes_keys() {
        let (mut tool, mut ctx) = start();
        let size = ctx.ui.size();
        let (left, width) = (62.0, size.x - RIGHT_WIDTH - 74.0);
        let duration = tool.duration();
        let x_of = |t: f32| left + t / duration * width;
        let rows_top = size.y - BOTTOM_HEIGHT + 6.0 + ROW + 6.0 + 18.0;
        // Scrub, on the empty row.
        ctx.input.set_cursor(physical(&ctx, Vec2::new(x_of(0.40), rows_top + 26.0 * 2.0 + 12.0)));
        frame(&mut tool, &mut ctx);
        ctx.input.button_down(MouseButton::Left);
        frame(&mut tool, &mut ctx);
        ctx.input.button_up(MouseButton::Left);
        frame(&mut tool, &mut ctx);
        assert!((tool.time - 0.4).abs() < 0.02, "{}", tool.time);
        // A marker.
        let on = tool.doc().unwrap().event_time("hit_on").unwrap();
        ctx.input.set_cursor(physical(&ctx, Vec2::new(x_of(on), rows_top + 12.0)));
        frame(&mut tool, &mut ctx);
        ctx.input.button_down(MouseButton::Left);
        frame(&mut tool, &mut ctx);
        assert!(matches!(tool.drag, Some(Drag::Event { .. })));
        ctx.input.set_cursor(physical(&ctx, Vec2::new(x_of(0.5), rows_top + 12.0)));
        frame(&mut tool, &mut ctx);
        ctx.input.button_up(MouseButton::Left);
        frame(&mut tool, &mut ctx);
        assert!((tool.doc().unwrap().event_time("hit_on").unwrap() - 0.5).abs() < 0.02);
    }

    #[test]
    fn saving_writes_the_file_in_the_folder() {
        let (mut tool, mut ctx) = start();
        tool.key_here();
        frame(&mut tool, &mut ctx);
        tool.save_all();
        let saved = tool.folder.join("sword_1.ron");
        let back: grimora::core::PoseClip = ron::from_str(&std::fs::read_to_string(&saved).unwrap()).unwrap();
        assert_eq!(back.duration, tool.doc().unwrap().clip.duration);
        assert!(!tool.doc().unwrap().dirty);
        std::fs::remove_dir_all(&tool.folder).unwrap();
    }
}
