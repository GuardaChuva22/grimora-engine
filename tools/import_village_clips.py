"""Retarget the villagers' everyday clips (sitting, sleeping, talking, praying, digging,
fishing) onto chartest, with the same retarget as tools/import_quaternius_clips.py.

    python -I tools/import_village_clips.py [--analyze]

Sources live in assets/animations/village/: mannequin (UE) takes as they came, and Mixamo
takes converted from FBX by tools/blender/fbx_to_glb.py. Nothing moves the root: each take
is placed where the game expects the body (the seat behind the root, the bed along it),
and loops get a short seam blend. The clips go into chartest.gltf with their own buffer,
chartest_village.bin; rerunning removes what the previous run added. Clips of the same
name in chartest_clips/ are moved to authoring_archive/, or they would win.
"""
import argparse
import json
from pathlib import Path
import shutil
import sys

import numpy as np
from scipy.spatial.transform import Rotation as R, Slerp

sys.path.insert(0, str(Path(__file__).resolve().parent))
from import_quaternius_clips import FPS, MODEL, ROOT, Target, clip, retarget, write  # noqa: E402

SOURCES = ROOT/'assets/animations/village'
# The takes on the UE mannequin skeleton; the rest came from Mixamo.
MANNEQUIN = {'sitting.glb', 'talking.glb', 'listening.glb', 'fishing_catch.glb', 'swimming.glb'}
BUFFER = 'chartest_village.bin'
CLIPS = MODEL.parent/'chartest_clips'


class Take:
    """One clip to make: file and take, seconds kept, whether it loops, where it goes."""

    def __init__(self, name, file, animation=None, span=None, loop=True, turn=0.0, place=None, seam=0.4,
                 upright=False):
        self.name, self.file, self.animation, self.span = name, file, animation, span
        self.upright = upright
        self.loop, self.turn, self.place, self.seam = loop, turn, place, seam


# Seated: the old clips sat the hips 0.25 m behind the root and 0.34 m down, and the
# seats in life.rs are placed for that. `place` says where the pelvis goes, in metres
# from its bind, at the frame named (`seated`): the whole take moves by the same amount,
# so sitting down and up stay continuous with the seated loops.
SEATED = ('seated', np.array([0.0, -0.34, 0.25]))
# Lying in bed: hips over the root, 0.265 m down, head toward +Z (the headboard).
IN_BED = ('lying', np.array([0.0, -0.265, 0.0]))
ON_FLOOR = ('lying', None)

TAKES = [
    Take('sit_down', 'sitting.glb', 'Sitting_Enter', loop=False, place=SEATED),
    Take('sit_up', 'sitting.glb', 'Sitting_Exit', loop=False, place=SEATED),
    Take('sit_idle', 'sitting.glb', 'Sitting_Idle', place=SEATED),
    Take('sit_talk', 'sitting.glb', 'Sitting_Talking', place=SEATED),
    Take('talk', 'talking.glb'),
    Take('listen', 'listening.glb'),
    Take('sleep', 'laying_sleeping.glb', place=IN_BED),
    Take('sleep_floor', 'sleeping_idle.glb', place=ON_FLOOR),
    Take('dig', 'digging.glb'),
    Take('kneel_pray', 'praying.glb'),
    Take('pray', 'praying_1.glb'),
    # The swimmer's lean is the controller's (`ThirdPersonController::tilt`), so the stroke
    # is stood up: `upright` leans the take back until its trunk is vertical on average.
    Take('swim', 'swimming.glb', 'Swim_Fwd', upright=True),
    Take('swim_idle', 'swimming.glb', 'Swim_Idle'),
    Take('fish', 'fishing_idle.glb', loop=False),
    Take('fish_cast', 'fishing_cast_and_reel.glb', loop=False),
    Take('fish_catch', 'fishing_catch.glb', loop=False),
]
# Clips made of others, one after another, each joined over JOIN seconds: waiting with
# the line out, casting and reeling in, waiting, a catch, and around again.
CHAINS = {'fish': ['fish', 'fish', 'fish_cast', 'fish', 'fish_catch']}
JOIN = 0.35


def shrugs(file):
    """Mannequin takes hold the shoulders high (see `retarget`); Mixamo ones do not."""
    return file in MANNEQUIN


def lying_turn(target, take_spec):
    """The turn that puts the head toward +Z, measured on a first pass."""
    take = retarget(target, SOURCES/take_spec.file, take_spec.animation)
    head = take['head'].mean(axis=0)
    # A turn of θ around up moves a direction's angle from +Z (toward +X) by θ.
    return -np.degrees(np.arctan2(head[0], head[2]))


def seam(c, seconds):
    """Blend the last frames into the first, so the loop closes."""
    n = len(next(iter(c['rotations'].values())))
    k = min(int(seconds*FPS), n//4)
    if k < 2:
        return
    for node, q in c['rotations'].items():
        for f in range(n-k, n):
            u = (f-(n-k))/(k-1)
            w = u*u*(3-2*u)
            a, b = R.from_quat(q[f]), R.from_quat(q[0])
            q[f] = Slerp([0, 1], R.concatenate([a, b]))(w).as_quat()
    for node, p in c['translations'].items():
        for f in range(n-k, n):
            u = (f-(n-k))/(k-1)
            w = u*u*(3-2*u)
            p[f] = p[f]*(1-w) + p[0]*w


def chain(name, parts):
    """One clip of `parts` played in turn; each starts from where the one before ended."""
    k = int(JOIN*FPS)
    rotations = {n: [q.copy()] for n, q in parts[0]['rotations'].items()}
    translations = {n: [p.copy()] for n, p in parts[0]['translations'].items()}
    for part in parts[1:]:
        for n in rotations:
            last, q = R.from_quat(rotations[n][-1][-1]), part['rotations'][n].copy()
            for f in range(min(k, len(q))):
                u = (f+1)/k
                w = u*u*(3-2*u)
                q[f] = Slerp([0, 1], R.concatenate([last, R.from_quat(q[f])]))(w).as_quat()
            rotations[n].append(q[1:])
        for n in translations:
            last, p = translations[n][-1][-1], part['translations'][n].copy()
            for f in range(min(k, len(p))):
                u = (f+1)/k
                w = u*u*(3-2*u)
                p[f] = last*(1-w) + p[f]*w
            translations[n].append(p[1:])
    rotations = {n: np.concatenate(v) for n, v in rotations.items()}
    translations = {n: np.concatenate(v) for n, v in translations.items()}
    frames = len(next(iter(rotations.values())))
    return dict(name=name, duration=(frames-1)/FPS, rotations=rotations, translations=translations, events=[])


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--analyze', action='store_true', help='print where each take is, write nothing')
    args = parser.parse_args()
    document = json.loads(MODEL.read_text(encoding='utf-8'))
    target = Target(document)
    clips, offsets = [], {}
    for t in TAKES:
        turn = t.turn
        if t.place and t.place[0] == 'lying':
            turn = lying_turn(target, t)
        pitch = 0.0
        if t.upright:
            head = retarget(target, SOURCES/t.file, t.animation)['head'].mean(axis=0)
            pitch = np.degrees(np.arctan2(-head[2], head[1]))
            print(f'[village] {t.name} leans {pitch:.1f} degrees: stood up')
        take = retarget(target, SOURCES/t.file, t.animation, relax=shrugs(t.file), turn=turn, pitch=pitch)
        pelvis = take['pelvis']
        n = len(pelvis)
        start, end = (0, n-1) if not t.span else (int(t.span[0]*FPS), min(n-1, int(t.span[1]*FPS)))
        shift = np.zeros(3)
        if t.place:
            kind, goal = t.place
            if kind == 'seated':
                # One shift for the whole sitting file, measured on the seated loop.
                if t.file not in offsets:
                    idle = retarget(target, SOURCES/t.file, 'Sitting_Idle', relax=True)
                    offsets[t.file] = goal - idle['pelvis'].mean(axis=0)
                shift = offsets[t.file]
            else:
                mean = pelvis.mean(axis=0)
                shift = -mean
                shift[1] = 0 if goal is None else goal[1]-mean[1]
        pelvis += shift
        head = take['head'].mean(axis=0)
        print(f'[village] {t.name:11} {t.file}:{t.animation or ""} {n/FPS:.2f}s turn {turn:6.1f} '
              f'pelvis start {np.round(pelvis[0], 2)} mid {np.round(pelvis[n//2], 2)} end {np.round(pelvis[-1], 2)} '
              f'feet {take["feet"].min():.2f} head {np.round(head, 2)} shift {np.round(shift, 2)}')
        c = clip(target, take, start, end, t.name, ground=False)
        if t.loop:
            seam(c, t.seam)
        clips.append(c)
    made = {c['name']: c for c in clips}
    for name, parts in CHAINS.items():
        made[name] = chain(name, [made[p] for p in parts])
        seam(made[name], JOIN)
        for p in parts:
            if p != name:
                made.pop(p, None)
        print(f'[village] {name}: {made[name]["duration"]:.2f}s from {parts}')
    clips = list(made.values())
    if args.analyze:
        return
    replaced = write(document, clips, BUFFER, 'chartest_village',
                     'Mixamo / mannequin takes, retargeted by tools/import_village_clips.py')
    archive = MODEL.parent/'authoring_archive'
    for c in clips:
        old = CLIPS/f'{c["name"]}.ron'
        if old.exists():
            kept = archive/f'{c["name"]}.legacy.ron'
            if not kept.exists():
                shutil.copy2(old, kept)
            old.unlink()
    print(f'[village] wrote {len(clips)} clips to {BUFFER}; replaced {replaced}')


if __name__ == '__main__':
    main()
