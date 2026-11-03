//! A clip being edited, with no window in sight: keys, markers, undo and the file.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context as _, Result};
use grimora::core::{Control, POSE_FPS, PoseClip, PoseKey, PoseSampler};
use grimora::prelude::{Quat, Vec3};

/// Keys this close, in seconds, are the same key.
const SAME_KEY: f32 = 0.5 / POSE_FPS;
const MAX_UNDO: usize = 200;

/// The frame nearest `t`.
pub fn snap(t: f32) -> f32 {
    (t * POSE_FPS).round() / POSE_FPS
}

/// How a drag changes the keys it reaches.
#[derive(Debug, Clone, Copy)]
pub enum Change {
    Offset(Vec3),
    Rotate(Quat),
}

impl Change {
    fn on(self, value: Vec3, weight: f32) -> Vec3 {
        match self {
            Change::Offset(offset) => value + offset * weight,
            Change::Rotate(turn) => Quat::IDENTITY.slerp(turn, weight) * value,
        }
    }
}

/// One clip and its history.
pub struct Doc {
    pub name: String,
    pub clip: PoseClip,
    undo: Vec<PoseClip>,
    redo: Vec<PoseClip>,
    /// Goes up with every change, so views know to rebuild.
    pub revision: u64,
    pub dirty: bool,
}

impl Doc {
    pub fn new(name: impl Into<String>, clip: PoseClip) -> Self {
        Self { name: name.into(), clip, undo: Vec::new(), redo: Vec::new(), revision: 0, dirty: false }
    }

    /// Call before a change: undo comes back here. A drag calls it once, at its start.
    pub fn checkpoint(&mut self) {
        self.undo.push(self.clip.clone());
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
    }

    pub fn undo(&mut self) -> bool {
        let Some(clip) = self.undo.pop() else { return false };
        self.redo.push(std::mem::replace(&mut self.clip, clip));
        self.touched();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(clip) = self.redo.pop() else { return false };
        self.undo.push(std::mem::replace(&mut self.clip, clip));
        self.touched();
        true
    }

    pub fn touched(&mut self) {
        self.revision += 1;
        self.dirty = true;
    }

    /// Whether `control` has a key at `t`.
    pub fn has_key(&self, control: Control, t: f32) -> bool {
        self.clip.tracks.get(&control).is_some_and(|keys| keys.iter().any(|k| (k.0 - t).abs() < SAME_KEY))
    }

    /// Replaces the key at `t`, or adds one on that frame.
    pub fn set_key(&mut self, control: Control, t: f32, value: Vec3) {
        set_key(self.clip.tracks.entry(control).or_default(), t, value);
        self.touched();
    }

    pub fn delete_key(&mut self, control: Control, t: f32) {
        if let Some(keys) = self.clip.tracks.get_mut(&control) {
            keys.retain(|k| (k.0 - t).abs() >= SAME_KEY);
            if keys.is_empty() {
                self.clip.tracks.remove(&control);
            }
            self.touched();
        }
    }

    /// `before` with its keys at `from` on `controls` moved to the frame nearest `to`. A key
    /// already there is replaced. Starting from a copy lets a drag pass over keys and leave
    /// them standing.
    pub fn retime(&mut self, before: &PoseClip, controls: &[Control], from: f32, to: f32) -> f32 {
        let to = snap(to.clamp(0.0, before.duration));
        self.clip = before.clone();
        for control in controls {
            let Some(keys) = self.clip.tracks.get_mut(control) else { continue };
            let Some(at) = keys.iter().position(|k| (k.0 - from).abs() < SAME_KEY) else { continue };
            let (_, value) = keys.remove(at);
            set_key(keys, to, value);
        }
        self.touched();
        to
    }

    /// `before` with `changes` applied at `at`: a key there takes the whole change, added if
    /// missing, and the ones less than `reach` seconds away a share that fades with
    /// distance, so moving one key of a dense arc bends the arc instead of denting it.
    pub fn deform(&mut self, before: &PoseClip, at: f32, changes: &[(Control, Change)], reach: f32) {
        let held = PoseSampler::with_springs(before, false).sample(at);
        self.clip = before.clone();
        let end = before.duration;
        for &(control, change) in changes {
            let keys = self.clip.tracks.entry(control).or_default();
            for key in keys.iter_mut() {
                let away = (key.0 - at).abs();
                if away >= SAME_KEY && away < reach {
                    let x = 1.0 - away / reach;
                    key.1 = change.on(key.1, x * x * (3.0 - 2.0 * x));
                }
            }
            let value = change.on(held[control], 1.0);
            set_key(keys, at, value);
            // The two ends of a loop are one pose.
            if before.looping && at < SAME_KEY {
                set_key(keys, end, value);
            } else if before.looping && (end - at).abs() < SAME_KEY {
                set_key(keys, 0.0, value);
            }
        }
        self.touched();
    }

    pub fn add_event(&mut self, t: f32, name: &str) {
        self.clip.events.push((t, name.to_string()));
        self.touched();
    }

    pub fn delete_event(&mut self, index: usize) {
        if index < self.clip.events.len() {
            self.clip.events.remove(index);
            self.touched();
        }
    }

    /// When an event of this name happens.
    pub fn event_time(&self, name: &str) -> Option<f32> {
        self.clip.events.iter().find(|(_, event)| event == name).map(|(t, _)| *t)
    }

    /// Writes `<folder>/<name>.ron`, all at once or not at all.
    pub fn save(&mut self, folder: &Path) -> Result<()> {
        std::fs::create_dir_all(folder).with_context(|| format!("creating {}", folder.display()))?;
        self.clip.events.sort_by(|a, b| a.0.total_cmp(&b.0));
        let path = folder.join(format!("{}.ron", self.name));
        let temporary = path.with_extension("ron.tmp");
        std::fs::write(&temporary, clip_text(&self.clip))
            .with_context(|| format!("writing {}", temporary.display()))?;
        std::fs::rename(&temporary, &path).with_context(|| format!("writing {}", path.display()))?;
        self.dirty = false;
        Ok(())
    }
}

fn set_key(keys: &mut Vec<PoseKey>, t: f32, value: Vec3) {
    if let Some(key) = keys.iter_mut().find(|k| (k.0 - t).abs() < SAME_KEY) {
        key.1 = value;
        return;
    }
    let t = snap(t);
    let at = keys.partition_point(|k| k.0 < t);
    keys.insert(at, (t, value));
}

/// A new clip from a single pose, held for a second.
pub fn clip_from_pose(controls: &grimora::core::Controls) -> PoseClip {
    let tracks =
        Control::ALL.iter().map(|&control| (control, vec![(0.0, controls[control])])).collect::<BTreeMap<_, _>>();
    PoseClip { duration: 1.0, looping: false, springs: true, tracks, events: Vec::new() }
}

/// The clip as a file: a key per line, so that it reads and diffs well.
pub fn clip_text(clip: &PoseClip) -> String {
    let mut out = format!("(\n    duration: {:.4},\n", clip.duration);
    if clip.looping {
        out += "    looping: true,\n";
    }
    if !clip.springs {
        out += "    springs: false,\n";
    }
    out += "    tracks: {\n";
    for (control, keys) in &clip.tracks {
        let name = ron::to_string(control).unwrap_or_default();
        out += &format!("        {name}: [\n");
        for (t, v) in keys {
            out += &format!("            ({t:.4}, ({:.4}, {:.4}, {:.4})),\n", v.x, v.y, v.z);
        }
        out += "        ],\n";
    }
    out += "    },\n";
    if !clip.events.is_empty() {
        out += "    events: [\n";
        for (t, name) in &clip.events {
            out += &format!("        ({t:.4}, {name:?}),\n");
        }
        out += "    ],\n";
    }
    out + ")\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> Doc {
        let tracks = BTreeMap::from([(
            Control::HandR,
            vec![(0.0, Vec3::ZERO), (0.1, Vec3::X), (0.2, Vec3::X * 2.0), (1.0, Vec3::X * 3.0)],
        )]);
        let clip =
            PoseClip { duration: 1.0, looping: false, springs: true, tracks, events: vec![(0.5, "hit_on".into())] };
        Doc::new("test", clip)
    }

    #[test]
    fn a_key_on_a_frame_replaces_one_off_it_by_little_and_adds_otherwise() {
        let mut doc = doc();
        doc.set_key(Control::HandR, 0.104, Vec3::Y);
        assert_eq!(doc.clip.tracks[&Control::HandR][1], (0.1, Vec3::Y));
        doc.set_key(Control::HandR, 0.5, Vec3::Z);
        let keys = &doc.clip.tracks[&Control::HandR];
        assert_eq!(keys.len(), 5);
        assert!(keys.windows(2).all(|pair| pair[0].0 < pair[1].0));
    }

    #[test]
    fn undo_and_redo_walk_the_history() {
        let mut doc = doc();
        let original = doc.clip.clone();
        doc.checkpoint();
        doc.delete_key(Control::HandR, 0.1);
        let changed = doc.clip.clone();
        assert_ne!(original, changed);
        assert!(doc.undo());
        assert_eq!(doc.clip, original);
        assert!(doc.redo());
        assert_eq!(doc.clip, changed);
        assert!(!doc.redo());
    }

    #[test]
    fn dragging_a_key_past_another_leaves_it_standing() {
        let mut doc = doc();
        let before = doc.clip.clone();
        doc.retime(&before, &[Control::HandR], 0.1, 0.2);
        assert_eq!(doc.clip.tracks[&Control::HandR].len(), 3);
        let at = doc.retime(&before, &[Control::HandR], 0.1, 0.301);
        assert_eq!(at, 18.0 / 60.0);
        assert_eq!(doc.clip.tracks[&Control::HandR].len(), 4);
        assert_eq!(doc.clip.tracks[&Control::HandR][1].1, Vec3::X * 2.0);
    }

    #[test]
    fn a_drag_fades_out_over_its_reach() {
        let mut doc = doc();
        let before = doc.clip.clone();
        doc.deform(&before, 0.1, &[(Control::HandR, Change::Offset(Vec3::Y))], 0.15);
        let keys = &doc.clip.tracks[&Control::HandR];
        assert_eq!(keys[1].1, Vec3::new(1.0, 1.0, 0.0));
        assert!(keys[0].1.y > 0.0 && keys[0].1.y < 1.0);
        assert!(keys[2].1.y > 0.0 && keys[2].1.y < 1.0);
        assert_eq!(keys[3].1, Vec3::X * 3.0);
        // Starting over from the copy: nothing piles up from frame to frame.
        doc.deform(&before, 0.1, &[(Control::HandR, Change::Offset(Vec3::Y * 2.0))], 0.0);
        assert_eq!(doc.clip.tracks[&Control::HandR][1].1, Vec3::new(1.0, 2.0, 0.0));
        assert_eq!(doc.clip.tracks[&Control::HandR][0].1, Vec3::ZERO);
    }

    #[test]
    fn a_pose_put_between_keys_starts_from_where_the_curve_was() {
        let mut doc = doc();
        let before = doc.clip.clone();
        let was = PoseSampler::with_springs(&before, false).sample(0.5)[Control::HandR];
        doc.deform(&before, 0.5, &[(Control::HandR, Change::Offset(Vec3::Z))], 0.0);
        assert_eq!(doc.clip.tracks[&Control::HandR].len(), 5);
        assert!(PoseSampler::with_springs(&doc.clip, false).sample(0.5)[Control::HandR].distance(was + Vec3::Z) < 1e-5);
    }

    #[test]
    fn the_two_ends_of_a_loop_stay_together() {
        let mut doc = doc();
        doc.clip.looping = true;
        let before = doc.clip.clone();
        doc.deform(&before, 0.0, &[(Control::HandR, Change::Offset(Vec3::Y))], 0.0);
        let keys = &doc.clip.tracks[&Control::HandR];
        assert_eq!(keys[0].1, keys[keys.len() - 1].1);
    }

    #[test]
    fn a_saved_clip_reads_back_the_same() {
        let folder = std::env::temp_dir().join(format!("grimora_animator_{}", std::process::id()));
        let mut doc = doc();
        doc.clip.springs = false;
        doc.clip.looping = true;
        doc.add_event(0.25, "combo");
        doc.save(&folder).unwrap();
        assert!(!doc.dirty);
        let text = std::fs::read_to_string(folder.join("test.ron")).unwrap();
        let back: PoseClip = ron::from_str(&text).unwrap();
        assert_eq!(back.tracks, doc.clip.tracks);
        assert_eq!(back.events[0], (0.25, "combo".to_string()));
        assert!(!back.springs && back.looping);
        std::fs::remove_dir_all(folder).unwrap();
    }
}
