"""Village routine clips for the chartest: sit, sleep, eat, hammer, fish, sweep, talk, listen, tend.

Writes assets/models/chartest/chartest_clips/<name>.ron (and overwrites them: run it only on request).
    python -I tools/gen_routine_clips.py [clip ...]

Frame: root space, -Z in front, Y up. `hips` is an offset from the standing hips (0.89 m high),
feet are absolute, hands are in the root's space and so are written from the hips.
Held things (hammer, rod, broom): `blade` is the way the object points from the hand, and the
wrist is kept straight by construction (`grip`): the object axis is made square to the forearm.
"""
import math
import sys
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "assets/models/chartest/chartest_clips"
TAU = math.tau


def add(a, b): return tuple(x + y for x, y in zip(a, b))
def sub(a, b): return tuple(x - y for x, y in zip(a, b))
def mul(a, k): return tuple(x * k for x in a)
def dot(a, b): return sum(x * y for x, y in zip(a, b))
def norm(a):
    n = math.sqrt(dot(a, a)) or 1.0
    return mul(a, 1 / n)
def lerp(a, b, k): return tuple(x + (y - x) * k for x, y in zip(a, b))
def ease(x):
    x = min(max(x, 0.0), 1.0)
    return x * x * (3 - 2 * x)
def wave(t, period, phase=0.0): return math.sin(TAU * (t / period + phase))


def shoulder(hips, pitch, yaw, right):
    """Where a shoulder is, roughly, for a torso leaning `pitch` degrees forward."""
    x, y, z = (0.19 if right else -0.19), 0.56, 0.14
    a = math.radians(pitch * 0.8)
    y2, z2 = y * math.cos(a) + z * math.sin(a), z * math.cos(a) - y * math.sin(a)
    b = math.radians(yaw * 0.8)
    x3, z3 = x * math.cos(b) + z2 * math.sin(b), -x * math.sin(b) + z2 * math.cos(b)
    return add((0.0, 0.89, -0.026), add(hips, (x3, y2, z3)))


def grip(hand, blade, from_shoulder):
    """(blade, edge) with the object square to the forearm, the edge running back to the shoulder."""
    e = norm(sub(from_shoulder, hand))
    b = norm(sub(blade, mul(e, dot(e, blade))))
    return b, e


def at(hips, local):
    """A point given from the hips joint: the standing hips are at (0, 0.89, -0.026)."""
    return add(add(hips, (0.0, 0.89, -0.026)), local)


def knees_up():
    return (0.0, 1.0, 0.0)


def ron(name, duration, looping, springs, tracks):
    lines = ["(", f"    duration: {duration:.4f},", f"    looping: {str(looping).lower()},",
             f"    springs: {str(springs).lower()},", "    tracks: {"]
    for control, keys in tracks.items():
        lines.append(f"        {control}: [")
        for t, v in keys:
            lines.append(f"            ({t:.4f}, ({v[0]:.4f}, {v[1]:.4f}, {v[2]:.4f})),")
        lines.append("        ],")
    lines += ["    },", ")", ""]
    (OUT / f"{name}.ron").write_text("\n".join(lines))
    print(f"wrote {name}.ron ({duration:.1f}s, {len(tracks)} controls)")


def sampled(duration, step, fn):
    """fn(t) -> {control: vec}; a closed loop sampled every `step` seconds."""
    n = round(duration / step)
    tracks = {}
    for i in range(n + 1):
        t = duration * (0 if i == n else i) / n
        for control, v in fn(t).items():
            tracks.setdefault(control, []).append((duration * i / n, v))
    return tracks


def keyed(poses):
    """poses: [(t, {control: vec})]: each control keeps its last value until it is keyed again."""
    tracks, last = {}, {}
    controls = {c for _, p in poses for c in p}
    for t, p in poses:
        last.update(p)
        for c in controls:
            if c in last:
                tracks.setdefault(c, []).append((t, last[c]))
    return tracks


# --- feet -----------------------------------------------------------------------------------

STAND_FEET = {
    "foot_l": (-0.15, 0.09, -0.06), "foot_r": (0.15, 0.09, -0.06),
    "foot_l_rot": (0.0, -4.0, 0.0), "foot_r_rot": (0.0, -8.0, 0.0),
}
SEAT_HIPS = (0.0, -0.34, 0.25)


def seated(hips_extra=(0, 0, 0), pitch=8.0):
    return {"hips": add(SEAT_HIPS, hips_extra), "torso": (pitch, 0.0, 0.0)}


# --- sitting --------------------------------------------------------------------------------

def sit_idle_pose(t):
    breath = wave(t, 3.0)
    look = wave(t, 6.0, 0.1)
    pitch = 7.0 + 0.8 * breath
    hips = add(SEAT_HIPS, (0.01 * wave(t, 6.0), 0.0, 0.004 * breath))
    sh_r, sh_l = shoulder(hips, pitch, 0, True), shoulder(hips, pitch, 0, False)
    hand_r = at(hips, (0.20, 0.13 + 0.003 * breath, -0.19 + 0.01 * wave(t, 6.0, 0.3)))
    hand_l = at(hips, (-0.20, 0.13 + 0.003 * breath, -0.19 - 0.01 * wave(t, 6.0, 0.3)))
    blade, edge = grip(hand_r, (0.1, 0.0, -1.0), sh_r)
    return {
        "hips": hips, "hips_rot": (0.0, 0.5 * look, 0.0), "torso": (pitch, 1.5 * look, 0.4 * wave(t, 6.0)),
        "head": (6.0 + 3.0 * wave(t, 6.0, 0.35) - 0.5 * breath, 6.0 * look, 1.0 * wave(t, 6.0, 0.2)),
        "hand_r": hand_r, "blade": blade, "edge": edge, "elbow_r": (0.8, -0.6, 0.1),
        "hand_l": hand_l, "elbow_l": (-1.0, -0.4, 0.5),
        **SEATED_FEET,
    }


SEATED_FEET = {
    "foot_l": (-0.16, 0.09, -0.16), "foot_r": (0.16, 0.09, -0.16),
    "foot_l_rot": (0.0, -4.0, 0.0), "foot_r_rot": (0.0, -8.0, 0.0),
    "knee_l": (-0.15, 0.0, -1.0), "knee_r": (0.15, 0.0, -1.0),
}


def standing_arms(hips, pitch, t=0.0, tag=0.0):
    sh_r = shoulder(hips, pitch, 0, True)
    hand_r = at(hips, (0.25, 0.10, -0.06 + 0.006 * wave(t, 4.0, tag)))
    blade, edge = grip(hand_r, (0.0, -0.6, -0.6), sh_r)
    return {
        "hand_r": hand_r, "blade": blade, "edge": edge, "elbow_r": (0.9, -0.5, 0.4),
        "hand_l": at(hips, (-0.25, 0.10, -0.06 - 0.006 * wave(t, 4.0, tag))), "elbow_l": (-0.9, -0.5, 0.4),
    }


def sit_down():
    rest = add((0.0, 0.0, 0.0), (0.0, -0.02, 0.0))
    seq = []
    # (time, hips, torso pitch, head pitch)
    steps = [(0.00, (0.0, -0.02, 0.0), 2.0, 4.0), (0.30, (0.0, -0.05, 0.04), 16.0, 14.0),
             (0.62, (0.0, -0.18, 0.18), 24.0, 12.0), (0.95, (0.0, -0.31, 0.24), 14.0, 8.0),
             (1.30, SEAT_HIPS, 8.0, 6.0)]
    for t, hips, pitch, head in steps:
        k = t / 1.3
        arms = standing_arms(hips, pitch)
        seated_hand = at(hips, (0.20, 0.13, -0.19))
        blade, edge = grip(seated_hand, (0.1, 0.0, -1.0), shoulder(hips, pitch, 0, True))
        hand_r = lerp(arms["hand_r"], seated_hand, ease((k - 0.15) / 0.5))
        hand_l = lerp(arms["hand_l"], at(hips, (-0.20, 0.13, -0.19)), ease((k - 0.15) / 0.5))
        seq.append((t, {
            "hips": hips, "hips_rot": (0.0, 0.0, 0.0), "torso": (pitch, 0.0, 0.0), "head": (head, 0.0, 0.0),
            "hand_r": hand_r, "blade": blade, "edge": edge, "elbow_r": (0.85, -0.55, 0.3),
            "hand_l": hand_l, "elbow_l": (-0.85, -0.55, 0.3),
            "foot_l": (-0.15, 0.09, -0.12 - 0.04 * k), "foot_r": (0.15, 0.09, -0.12 - 0.04 * k),
            "foot_l_rot": (0.0, -4.0, 0.0), "foot_r_rot": (0.0, -8.0, 0.0),
            "knee_l": (-0.15, 0.0, -1.0), "knee_r": (0.15, 0.0, -1.0),
        }))
    return keyed(seq), 1.3


def sit_up():
    tracks, duration = sit_down()
    out = {}
    for control, keys in tracks.items():
        # Mirror in time, and make the way up quicker at the start (the push) and slower at the end.
        out[control] = [(round(duration - t, 4), v) for t, v in reversed(keys)]
    return out, duration


# --- eating ---------------------------------------------------------------------------------

def eat_pose(t):
    d = 5.0
    u = (t % d) / d
    dip = ease((u - 0.00) / 0.15) * (1 - ease((u - 0.15) / 0.2))    # toward the bowl
    mouth = ease((u - 0.15) / 0.2) * (1 - ease((u - 0.52) / 0.2))   # up to the mouth
    chew = ease((u - 0.35) / 0.1) * (1 - ease((u - 0.72) / 0.1))
    pitch = 9.0 + 7.0 * dip - 3.0 * mouth
    hips = add(SEAT_HIPS, (0.0, 0.0, -0.01 * dip))
    sh_r = shoulder(hips, pitch, 0, True)
    bowl = at(hips, (0.10, 0.25, -0.20))
    lips = at(hips, (0.04, 0.58, -0.17))
    rest = at(hips, (0.20, 0.13, -0.19))
    hand_r = lerp(lerp(rest, bowl, dip), lips, mouth)
    hand_r = add(hand_r, (0.0, 0.0, 0.0))
    aim = lerp((0.0, -0.2, -1.0), (0.2, 0.5, -0.8), mouth)
    blade, edge = grip(hand_r, aim, sh_r)
    nod = 2.2 * math.sin(TAU * t * 2.4) * chew
    return {
        "hips": hips, "hips_rot": (0.0, 0.0, 0.0),
        "torso": (pitch, 0.0, 0.0),
        "head": (6.0 + 8.0 * dip - 6.0 * mouth + nod, 2.0 * wave(t, d, 0.2), 0.0),
        "hand_r": hand_r, "blade": blade, "edge": edge, "elbow_r": (0.8, -0.7, 0.2),
        "hand_l": at(hips, (-0.18, 0.20 + 0.01 * dip, -0.15)), "elbow_l": (-0.8, -0.6, 0.1),
        **SEATED_FEET,
    }


# --- sleeping -------------------------------------------------------------------------------

def sleep_pose(t):
    breath = wave(t, 5.0)
    lie = (0.0, -0.265 + 0.003 * breath, 0.0)
    hand_r = (0.14, 0.14 + 0.012 * breath, 0.26)
    hand_l = (-0.18, 0.14 + 0.012 * breath, 0.24)
    shoulder_pos = at(lie, (0.19, -0.14, 0.56))
    lying_blade, lying_edge = grip(at(lie, hand_r), (-1.0, 0.2, 0.0), shoulder_pos)
    return {
        "hips": lie, "hips_rot": (-90.0, 0.0, 0.0), "torso": (-1.5 + 0.8 * breath, 0.0, 0.0),
        "head": (-4.0 + 0.5 * breath, 8.0, 0.0),
        "hand_r": at(lie, hand_r), "blade": lying_blade, "edge": lying_edge,
        "elbow_r": (1.0, 0.2, 0.0),
        "hand_l": at(lie, hand_l), "elbow_l": (-1.0, 0.2, 0.0),
        "foot_l": (-0.12, 0.62, -0.74), "foot_r": (0.12, 0.61, -0.72),
        "foot_l_rot": (-62.0, 8.0, 0.0), "foot_r_rot": (-58.0, -8.0, 0.0),
        "knee_l": knees_up(), "knee_r": knees_up(),
    }


# --- smith ----------------------------------------------------------------------------------

def hammer_pose(t):
    d = 1.3
    u = (t % d) / d
    up = ease(u / 0.5)                        # lift, slow
    hit = ease((u - 0.52) / 0.07)             # strike, fast
    back = ease((u - 0.62) / 0.16)            # rebound and rest
    amount = up * (1 - hit) + 0.0
    low = hit * (1 - back)
    stride = 1.0 - 0.0
    pitch = 8.0 + 18.0 * low - 4.0 * up * (1 - hit)
    hips = (0.0, -0.04 - 0.05 * low + 0.02 * up * (1 - hit), -0.06 * low + 0.03 * up * (1 - hit))
    sh_r = shoulder(hips, pitch, 0, True)
    ready = at(hips, (0.27, 0.58, -0.28))
    raised = at(hips, (0.27, 0.70, -0.14))
    struck = at(hips, (0.22, 0.22, -0.38))
    hand_r = lerp(ready, raised, up * (1 - hit))
    hand_r = lerp(hand_r, struck, low)
    aim = lerp(lerp((0.0, 0.4, -1.0), (0.0, 0.85, 0.5), up * (1 - hit)), (0.0, -0.45, -0.9), low)
    blade, edge = grip(hand_r, aim, sh_r)
    hand_l = at(hips, (-0.14, 0.25 + 0.01 * up, -0.25))
    return {
        "hips": hips, "hips_rot": (0.0, -4.0 * low, 0.0),
        "torso": (pitch, 6.0 * low - 4.0 * up * (1 - hit), 0.0),
        "head": (18.0 + 4.0 * low - 6.0 * up * (1 - hit), 0.0, 0.0),
        "hand_r": hand_r, "blade": blade, "edge": edge, "elbow_r": (0.9, -0.5, 0.3),
        "hand_l": hand_l, "elbow_l": (-0.9, -0.6, 0.1),
        "foot_l": (-0.18, 0.09, -0.04), "foot_r": (0.18, 0.09, -0.16),
        "foot_l_rot": (0.0, 6.0, 0.0), "foot_r_rot": (0.0, -26.0, 0.0),
    }


# --- fishing --------------------------------------------------------------------------------

def fish_pose(t):
    d = 8.0
    pitch = 6.0 + 1.0 * wave(t, d)
    hips = (0.01 * wave(t, d), -0.03, 0.01 * wave(t, d, 0.25))
    # now and then the tip dips as if something nibbled, at about 5 s
    nibble = math.exp(-((t - 5.0) / 0.25) ** 2) * math.sin(TAU * t * 3.0)
    sh_r = shoulder(hips, pitch, 0, True)
    hand_r = at(hips, (0.22, 0.15 + 0.004 * nibble, -0.13))
    aim = (-0.1, 0.55 + 0.05 * nibble + 0.02 * wave(t, d), -0.82)
    blade, edge = grip(hand_r, aim, sh_r)
    hand_l = at(hips, (-0.12, 0.10 + 0.01 * wave(t, 4.0), -0.12))
    return {
        "hips": hips, "hips_rot": (0.0, -6.0, 0.0), "torso": (pitch, 4.0 * wave(t, d, 0.1), 0.0),
        "head": (15.0 + 2.0 * wave(t, d, 0.4) + 3.0 * nibble, -3.0, 0.0),
        "hand_r": hand_r, "blade": blade, "edge": edge, "elbow_r": (0.9, -0.6, 0.2),
        "hand_l": hand_l, "elbow_l": (-0.9, -0.5, 0.3),
        "foot_l": (-0.17, 0.09, -0.02), "foot_r": (0.17, 0.09, -0.18),
        "foot_l_rot": (0.0, 8.0, 0.0), "foot_r_rot": (0.0, -20.0, 0.0),
    }


# --- sweeping -------------------------------------------------------------------------------

def sweep_pose(t):
    d = 2.4
    s = wave(t, d, -0.25)       # -1 left .. 1 right, smooth
    lean = 0.5 + 0.5 * math.cos(TAU * t / (d / 2))   # pushes on each stroke end
    pitch = 14.0 + 3.0 * wave(t, d / 2, 0.1)
    hips = (0.03 * s, -0.05, -0.02 + 0.03 * wave(t, d / 2, 0.3))
    sh_r = shoulder(hips, pitch, 12 * s, True)
    hand_r = at(hips, (0.12 + 0.12 * s, 0.19, -0.24))
    aim = (-0.2 * s - 0.2, 0.75, 0.55)
    blade, edge = grip(hand_r, aim, sh_r)
    hand_l = add(hand_r, mul(blade, 0.24))
    return {
        "hips": hips, "hips_rot": (0.0, -10.0 * s, 0.0), "torso": (pitch, -4.0 * s, 0.0),
        "head": (20.0 - 2.0 * lean, 8.0 * s, 0.0),
        "hand_r": hand_r, "blade": blade, "edge": edge, "elbow_r": (0.9, -0.5, 0.3),
        "hand_l": hand_l, "elbow_l": (-0.9, -0.5, 0.3),
        "foot_l": (-0.18, 0.09, -0.04), "foot_r": (0.16, 0.09, -0.12),
        "foot_l_rot": (0.0, 6.0, 0.0), "foot_r_rot": (0.0, -12.0, 0.0),
    }


# --- talk and listen ------------------------------------------------------------------------

def talk_pose(t):
    d = 7.0
    hips = (0.012 * wave(t, d, 0.1), -0.02, 0.0)
    pitch = 3.0
    # Sentences: right hand rises on stress, left follows later, both drop between them.
    r = ease((wave(t, d / 2, 0.0) + 0.3) / 1.3) * (0.6 + 0.4 * wave(t, d / 3.5))
    l = ease((wave(t, d / 2, 0.25) - 0.1) / 1.0) * 0.8
    sh_r = shoulder(hips, pitch, 0, True)
    hand_r = at(hips, (0.26 + 0.04 * r, 0.08 + 0.15 * r + 0.015 * wave(t, d / 7), -0.03 - 0.14 * r))
    blade, edge = grip(hand_r, (0.3, 0.2, -0.9), sh_r)
    hand_l = at(hips, (-0.26 - 0.03 * l, 0.08 + 0.16 * l, -0.03 - 0.15 * l))
    return {
        "hips": hips, "hips_rot": (0.0, 3.0 * wave(t, d, 0.2), 0.0),
        "torso": (pitch + 1.5 * r, 4.0 * wave(t, d / 2, 0.15), 0.8 * wave(t, d)),
        "head": (2.0 + 3.0 * wave(t, d / 3.5, 0.1) * 1.0, 6.0 * wave(t, d / 2, 0.3), 1.5 * wave(t, d)),
        "hand_r": hand_r, "blade": blade, "edge": edge, "elbow_r": (0.9, -0.5, 0.4),
        "hand_l": hand_l, "elbow_l": (-0.9, -0.5, 0.4),
        "foot_l": (-0.16, 0.09, -0.04), "foot_r": (0.16, 0.09, -0.08),
        "foot_l_rot": (0.0, 4.0, 0.0), "foot_r_rot": (0.0, -10.0, 0.0),
    }


def listen_pose(t):
    d = 8.0
    hips = (0.015 * wave(t, d), -0.02, 0.0)
    pitch = 4.0
    sh_r = shoulder(hips, pitch, 0, True)
    hand_r = at(hips, (0.10, 0.19 + 0.004 * wave(t, 4.0), -0.14))
    blade, edge = grip(hand_r, (-1.0, 0.1, -0.2), sh_r)
    nod = max(0.0, wave(t, d / 2, 0.1)) ** 3
    return {
        "hips": hips, "hips_rot": (0.0, 0.0, 0.0),
        "torso": (pitch + 0.6 * wave(t, 4.0), 0.0, 0.5 * wave(t, d, 0.25)),
        "head": (3.0 + 4.0 * nod, -4.0 + 2.0 * wave(t, d, 0.3), 2.0 * wave(t, d, 0.1)),
        "hand_r": hand_r, "blade": blade, "edge": edge, "elbow_r": (0.9, -0.6, 0.3),
        "hand_l": at(hips, (-0.09, 0.23, -0.13 + 0.003 * wave(t, 4.0, 0.2))), "elbow_l": (-0.9, -0.5, 0.3),
        "foot_l": (-0.16, 0.09, -0.02), "foot_r": (0.16, 0.09, -0.10),
        "foot_l_rot": (0.0, 3.0, 0.0), "foot_r_rot": (0.0, -12.0, 0.0),
    }


# --- crouching to work the ground -----------------------------------------------------------

def tend_pose(t):
    d = 3.0
    dig = wave(t, d, 0.0)
    pitch = 58.0 + 4.0 * dig
    hips = (0.0, -0.55, 0.08 + 0.01 * dig)
    sh_r = shoulder(hips, pitch, 0, True)
    hand_r = at(hips, (0.22, 0.05 + 0.04 * max(0.0, dig), -0.27 + 0.06 * dig))
    blade, edge = grip(hand_r, (0.2, -0.5, -0.8), sh_r)
    hand_l = at(hips, (-0.22, 0.05 + 0.02 * wave(t, d, 0.5), -0.26 - 0.04 * dig))
    return {
        "hips": hips, "hips_rot": (0.0, 3.0 * dig, 0.0), "torso": (pitch, 5.0 * dig, 0.0),
        "head": (-24.0, 0.0, 0.0),
        "hand_r": hand_r, "blade": blade, "edge": edge, "elbow_r": (1.0, -0.2, 0.3),
        "hand_l": hand_l, "elbow_l": (-1.0, -0.2, 0.3),
        "foot_l": (-0.18, 0.09, -0.02), "foot_r": (0.18, 0.09, -0.02),
        "foot_l_rot": (0.0, 6.0, 0.0), "foot_r_rot": (0.0, -6.0, 0.0),
        "knee_l": (-0.3, 0.0, -1.0), "knee_r": (0.3, 0.0, -1.0),
    }


CLIPS = {
    "sit_down": lambda: (*sit_down(), False, True),
    "sit_up": lambda: (*sit_up(), False, True),
    "sit_idle": lambda: (sampled(6.0, 0.25, sit_idle_pose), 6.0, True, True),
    "eat": lambda: (sampled(5.0, 0.1, eat_pose), 5.0, True, True),
    "sleep": lambda: (sampled(5.0, 0.25, sleep_pose), 5.0, True, True),
    "hammer": lambda: (sampled(1.3, 0.0325, hammer_pose), 1.3, True, False),
    "fish": lambda: (sampled(8.0, 0.1, fish_pose), 8.0, True, False),
    "sweep": lambda: (sampled(2.4, 0.06, sweep_pose), 2.4, True, False),
    "talk": lambda: (sampled(7.0, 0.1, talk_pose), 7.0, True, True),
    "listen": lambda: (sampled(8.0, 0.2, listen_pose), 8.0, True, True),
    "tend": lambda: (sampled(3.0, 0.1, tend_pose), 3.0, True, True),
}

if __name__ == "__main__":
    wanted = sys.argv[1:] or list(CLIPS)
    for name in wanted:
        tracks, duration, looping, springs = CLIPS[name]()
        ron(name, duration, looping, springs, tracks)
