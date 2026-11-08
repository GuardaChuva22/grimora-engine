"""Writes the great swords' moveset: `heavy_*`, in assets/models/chartest/chartest_clips.

Written as arcs, not as hand positions. A key says where each arm points as seen from the chest and
how far out it is, and where the blade points: azimuth (0 in front, +90 to the right, past 180 behind)
and elevation, in degrees. The arm and the blade go round on those angles, so they sweep instead of
cutting straight across, and the body's turn comes on top of that. The edge comes from the forearm
and the elbow from the blade, so the wrist stays straight unless a key asks otherwise.

The blade stays on one side of the forearm all through a cut, trailing it: passing through the
line of the arm flips the hand over. To change sides it goes over the top, around the forearm (see the
start of `write_blow_2`). Both hands stay on the handle: the arms' amplitude comes from the hands going from one shoulder to the
other hip while the chest turns under them.
Overwrites those files, so edits made in the animator go away.

    python -I tools/gen_heavy_clips.py
"""

import math
import os

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "..", "assets", "models", "chartest", "chartest_clips")
STEP = 1.0 / 60.0

# Measured on the model at rest.
HIPS_REST = np.array([0.0, 0.89, 0.026])
SPINE_REST = np.array([0.0, 1.06, 0.03])
CHEST_REST = np.array([0.0, 1.302, 0.056])
SHOULDER_R = np.array([0.19, 1.453, 0.114]) - CHEST_REST
SHOULDER_L = np.array([-0.19, 1.453, 0.114]) - CHEST_REST
UPPER, FORE = 0.26, 0.25
# Palm from the wrist: the weapon joint sits in the fist.
PALM = 0.117
# Reach 1.0 is the arm nearly straight, in meters from the shoulder to the wrist.
FULL_REACH = 0.48

# Seconds after the hips: the head looks first, then the hips, the chest, the arm, and the blade last.
DELAY = {"head": -0.03, "hips": 0.0, "rot": 0.0, "torso": 0.012, "arm": 0.024, "blade": 0.04}


def rotation(angles):
    """As `rotation_of`: Ry(y) Rx(-x) Rz(-z), from (pitch, yaw, roll) in degrees."""
    x, y, z = [math.radians(v) for v in angles]
    cx, sx = math.cos(-x), math.sin(-x)
    cy, sy = math.cos(y), math.sin(y)
    cz, sz = math.cos(-z), math.sin(-z)
    rx = np.array([[1, 0, 0], [0, cx, -sx], [0, sx, cx]])
    ry = np.array([[cy, 0, sy], [0, 1, 0], [-sy, 0, cy]])
    rz = np.array([[cz, -sz, 0], [sz, cz, 0], [0, 0, 1]])
    return ry @ rx @ rz


def direction(az, el):
    a, e = math.radians(az), math.radians(el)
    return np.array([math.sin(a) * math.cos(e), math.sin(e), -math.cos(a) * math.cos(e)])


def unit(v):
    return v / max(np.linalg.norm(v), 1e-6)


def chest_of(hips, rot, torso):
    """Where the chest is and how it's turned, as `Rig::solve` works it out."""
    turn_hips = rotation(rot)
    turn_chest = turn_hips @ rotation(torso)
    half = rotation([v * 0.45 for v in torso])
    spine = HIPS_REST + np.array(hips, dtype=float) + turn_hips @ (SPINE_REST - HIPS_REST)
    return spine + turn_hips @ half @ (CHEST_REST - SPINE_REST), turn_chest


def elbow_of(shoulder, wrist, pole):
    """The elbow of a two-bone arm, bending towards `pole`."""
    to = wrist - shoulder
    d = min(np.linalg.norm(to), UPPER + FORE - 1e-4)
    along = to / max(np.linalg.norm(to), 1e-6)
    a = (UPPER * UPPER - FORE * FORE + d * d) / (2.0 * d)
    h = math.sqrt(max(UPPER * UPPER - a * a, 0.0))
    side = unit(pole - along * pole.dot(along))
    return shoulder + along * a + side * h


class Curve:
    """A monotone cubic, as the engine's: it passes through a key at speed if the keys around it
    go the same way, and stops only where the value turns back."""

    def __init__(self, keys):
        self.t = np.array([k[0] for k in keys], dtype=float)
        self.v = np.array([k[1] for k in keys], dtype=float)
        n = len(keys)
        self.m = np.zeros_like(self.v)
        h = np.diff(self.t)
        d = np.diff(self.v, axis=0) / h[:, None]
        for c in range(self.v.shape[1]):
            for i in range(1, n - 1):
                a, b = d[i - 1, c], d[i, c]
                if a * b > 0:
                    w1, w2 = 2 * h[i] + h[i - 1], h[i] + 2 * h[i - 1]
                    self.m[i, c] = (w1 + w2) / (w1 / a + w2 / b)

    def at(self, t):
        if t <= self.t[0]:
            return self.v[0].copy()
        if t >= self.t[-1]:
            return self.v[-1].copy()
        i = int(np.searchsorted(self.t, t)) - 1
        h = self.t[i + 1] - self.t[i]
        u = (t - self.t[i]) / h
        h00 = (1 + 2 * u) * (1 - u) ** 2
        h10 = u * (1 - u) ** 2
        h01 = u * u * (3 - 2 * u)
        h11 = u * u * (u - 1)
        return h00 * self.v[i] + h10 * h * self.m[i] + h01 * self.v[i + 1] + h11 * h * self.m[i + 1]


def arm_controls(place, turn, arm, blade_angles, roll=0.0):
    """Hand, blade, edge and elbows, relative to the root, from a chest and the arms' angles."""
    blade_local = direction(blade_angles[0], blade_angles[1])
    wrist_r = SHOULDER_R + direction(arm[0], arm[1]) * arm[2] * FULL_REACH
    # Elbows down and a little in, away from the blade: chosen by a search over every clip.
    k, *d = [float(x) for x in os.environ.get("POLE_R", "0.3,0.3,-0.9,0").split(",")]
    pole = unit(-blade_local * k + np.array(d))
    elbow = elbow_of(SHOULDER_R, wrist_r, pole)
    forearm = unit(wrist_r - elbow)
    edge_local = -(forearm - blade_local * forearm.dot(blade_local))
    if roll:
        k = np.array([[0, -blade_local[2], blade_local[1]], [blade_local[2], 0, -blade_local[0]], [-blade_local[1], blade_local[0], 0]])
        a = math.radians(roll)
        edge_local = (np.eye(3) + math.sin(a) * k + (1 - math.cos(a)) * k @ k) @ edge_local
    palm = wrist_r - unit(edge_local) * PALM
    # The left hand goes wherever the handle is: `grip_l` is always 1, this is only what it starts from.
    wrist_l = palm - blade_local * 0.105
    k, *d = [float(x) for x in os.environ.get("POLE_L", "0,-0.9,-0.6,0").split(",")]
    pole_l = unit(-blade_local * k + np.array(d))
    return {
        "hand_r": place + turn @ palm,
        "blade": turn @ blade_local,
        "edge": turn @ edge_local,
        "elbow_r": turn @ pole,
        "hand_l": place + turn @ wrist_l,
        "elbow_l": turn @ pole_l,
    }


def write_clip(name, duration, looping, tracks, events=()):
    lines = ["(", f"    duration: {duration:.4f},", f"    looping: {str(looping).lower()},", "    springs: true,", "    tracks: {"]
    for control, track in tracks.items():
        lines.append(f"        {control}: [")
        for t, v in track:
            lines.append(f"            ({t:.4f}, ({v[0]:.4f}, {v[1]:.4f}, {v[2]:.4f})),")
        lines.append("        ],")
    lines.append("    },")
    if events:
        lines.append("    events: [")
        lines.extend(f'        ({t:.4f}, "{n}"),' for t, n in events)
        lines.append("    ],")
    lines.extend([")", ""])
    with open(os.path.join(OUT, f"{name}.ron"), "w", encoding="utf8", newline="\n") as out:
        out.write("\n".join(lines))
    print("wrote", name, f"{duration:.2f} s")


CHANNELS = {"hips": "hips", "rot": "hips_rot", "torso": "torso", "head": "head"}


def key(t, base=None, **changes):
    values = dict(base or {})
    values.update(changes)
    values["t"] = t
    return values


def compile_blow(name, duration, keys, feet, events, root=None):
    """Samples the arcs every frame, so the curve between two keys is the arc and not a straight line."""
    curve = {part: Curve([(k["t"] + DELAY[part], np.array(k[part], dtype=float)) for k in keys]) for part in ("hips", "rot", "torso", "head", "arm", "blade")}
    roll = Curve([(k["t"] + DELAY["blade"], np.array([k.get("roll", 0.0)])) for k in keys])
    tracks = {}
    if root:
        tracks["root"] = [(t, (0.0, 0.0, z)) for t, z in root]
    for part, control in CHANNELS.items():
        tracks[control] = [(max(k["t"] + DELAY[part], 0.0), curve[part].at(max(k["t"] + DELAY[part], 0.0))) for k in keys[:-1]]
        tracks[control][0] = (0.0, curve[part].at(0.0))
        tracks[control].append((duration, curve[part].at(duration + DELAY[part])))
    tracks.update(feet)
    arms = {control: [] for control in ("hand_r", "blade", "edge", "elbow_r", "hand_l", "elbow_l")}
    for i in range(int(round(duration / STEP)) + 1):
        t = i * STEP
        place, turn = chest_of(curve["hips"].at(t), curve["rot"].at(t), curve["torso"].at(t))
        for control, value in arm_controls(place, turn, curve["arm"].at(t), curve["blade"].at(t), roll.at(t)[0]).items():
            arms[control].append((t, value))
    tracks.update(arms)
    tracks["grip_l"] = [(0.0, (1.0, 0.0, 0.0)), (duration, (1.0, 0.0, 0.0))]
    write_clip(name, duration, False, tracks, events)


def step(start, t0, t1, to, lift=0.08):
    """A foot from where it is to `to`, off the ground between `t0` and `t1`. It rises and lands
    nearly straight, because a foot low and still moving looks like it slides."""
    start, to = np.array(start, dtype=float), np.array(to, dtype=float)
    mid = (start + to) / 2.0 + np.array([0.0, lift, 0.0])
    near_a = start + (to - start) * 0.02 + np.array([0.0, lift * 0.5, 0.0])
    near_b = to + (start - to) * 0.02 + np.array([0.0, lift * 0.5, 0.0])
    h = t1 - t0
    return [(t0, start), (t0 + h * 0.2, near_a), (t0 + h * 0.5, mid), (t0 + h * 0.8, near_b), (t1, to)]


def v(*values):
    return np.array(values, dtype=float)


# ---- the stance: both hands in front of the belly, the blade pointing ahead and up, the chest turned a
# little to the right

IDLE = dict(
    hips=(0.015, -0.14, 0.03),
    rot=(4.0, -14.0, 2.0),
    torso=(8.0, -8.0, -2.0),
    head=(2.0, 20.0, 0.0),
    arm=(-22.0, -28.0, 0.97),
    blade=(-14.0, 54.0),
)
FOOT_L = v(-0.17, 0.09, -0.28)
FOOT_R = v(0.21, 0.09, 0.24)
FOOT_L_ROT = v(0, -6, 0)
FOOT_R_ROT = v(0, -46, 0)

# Where a blow can hand over to the next one.
END_1 = dict(
    hips=(-0.03, -0.3, 0.05),
    rot=(14.0, 38.0, 3.0),
    torso=(20.0, 32.0, 8.0),
    head=(-6.0, -40.0, 0.0),
    arm=(-32.0, -28.0, 0.90),
    blade=(25.0, 37.0), roll=-56.0,
)
END_2 = dict(
    hips=(0.05, -0.27, 0.02),
    rot=(8.0, -60.0, -3.0),
    torso=(10.0, -38.0, -8.0),
    head=(0.0, 66.0, 0.0),
    arm=(-4.0, -16.0, 0.80),
    blade=(-26.0, 54.0), roll=20.0,
)
ROOT_1, ROOT_2, ROOT_3 = 0.36, 0.45, 0.72


def write_blow_1():
    """From behind the right shoulder to behind the left hip, the left hand out in front to aim, then
    thrown back as the sword comes through. Chest and arm together make the whole circle."""
    coil = dict(hips=(0.04, -0.2, 0.09), rot=(4.0, -56.0, -5.0), torso=(-10.0, -36.0, -10.0), head=(-2.0, 74.0, 0.0), arm=(16.0, 30.0, 0.62), blade=(117.0, 40.0), roll=-60.0)
    keys = [
        key(0.0, IDLE),
        key(0.1, hips=(0.03, -0.17, 0.05), rot=(4.0, -38.0, -2.0), torso=(-2.0, -20.0, -7.0), head=(0.0, 50.0, 0.0), arm=(8.0, -6.0, 0.68), blade=(110.0, 20.0), roll=-48.0),
        key(0.22, coil),
        key(0.3, coil, hips=(0.045, -0.2, 0.1), rot=(4.5, -59.0, -5.5), torso=(-11.5, -38.0, -11.0), arm=(8.0, 34.0, 0.63), blade=(104.0, 44.0), roll=-48.0),
        key(0.37, hips=(0.0, -0.24, 0.02), rot=(8.0, -28.0, -2.0), torso=(4.0, -20.0, -4.0), head=(0.0, 24.0, 0.0), arm=(-6.0, 40.0, 0.74), blade=(94.0, 45.0)),
        key(0.43, hips=(-0.02, -0.28, 0.03), rot=(12.0, 4.0, 1.0), torso=(14.0, 4.0, 3.0), head=(-4.0, -6.0, 0.0), arm=(-14.0, 6.0, 0.90), blade=(56.0, 24.0), roll=-60.0),
        key(0.5, hips=(-0.03, -0.29, 0.05), rot=(14.0, 26.0, 2.5), torso=(18.0, 22.0, 6.0), head=(-6.0, -28.0, 0.0), arm=(-22.0, -2.0, 0.92), blade=(58.0, 36.0), roll=-53.0),
        key(0.62, END_1),
        key(0.88, hips=(-0.02, -0.26, 0.05), rot=(11.0, 24.0, 2.0), torso=(17.0, 20.0, 5.0), head=(-4.0, -24.0, 0.0), arm=(-30.0, -36.0, 0.86), blade=(40.0, 12.0), roll=-60.0),
        key(1.06, hips=(0.0, -0.18, 0.0), rot=(6.0, -8.0, 1.0), torso=(12.0, 0.0, 0.0), head=(0.0, 6.0, 0.0), arm=(-4.0, -44.0, 0.80), blade=(60.0, 18.0), roll=-70.0),
        key(1.3, IDLE),
    ]
    foot_l_to = v(-0.24, 0.09, -0.74)
    foot_r_up = v(0.21, 0.116, 0.24)
    feet = {
        "foot_l": [(0.0, FOOT_L)] + step(FOOT_L, 0.26, 0.42, foot_l_to, lift=0.1) + step(foot_l_to, 1.02, 1.2, FOOT_L + v(0, 0, -ROOT_1), lift=0.03),
        "foot_l_rot": [(0.0, FOOT_L_ROT), (0.42, v(0, 14, 0)), (1.02, v(0, 14, 0)), (1.2, FOOT_L_ROT)],
        "foot_r": [(0.0, FOOT_R), (0.34, FOOT_R), (0.44, foot_r_up), (0.74, foot_r_up + v(0, -0.002, 0)), (0.92, FOOT_R)] + step(FOOT_R, 0.96, 1.18, FOOT_R + v(0, 0, -ROOT_1), lift=0.06),
        "foot_r_rot": [(0.0, FOOT_R_ROT), (0.34, FOOT_R_ROT), (0.44, v(24, -46, 0)), (0.74, v(23, -46, 0)), (0.92, FOOT_R_ROT)],
        "toe_r": [(0.0, v(0, 0, 0)), (0.34, v(0, 0, 0)), (0.44, v(26, 0, 0)), (0.74, v(25, 0, 0)), (0.92, v(0, 0, 0))],
    }
    root = [(0.0, 0.0), (0.24, 0.0), (0.3, -0.04), (0.42, -ROOT_1 * 0.88), (0.5, -ROOT_1)]
    compile_blow("heavy_1", 1.3, keys, feet, [(0.32, "swing"), (0.38, "hit_on"), (0.54, "hit_off"), (0.66, "combo"), (0.76, "cancel")], root=root)


def write_blow_2():
    """Backhand, flat, the whole arm out: it starts wrapped round the left side and flies open."""
    foot_l_from = v(-0.24, 0.09, -0.74 + ROOT_1)
    foot_r_from = v(0.21, 0.116, 0.24 + ROOT_1)
    coil = dict(hips=(-0.05, -0.32, -0.02), rot=(13.0, 56.0, 4.0), torso=(16.0, 36.0, 10.0), head=(-2.0, -70.0, 0.0), arm=(-22.0, 18.0, 0.72), blade=(-108.0, 24.0), roll=-55.0)
    keys = [
        key(0.0, END_1),
        key(0.14, END_1, hips=(-0.04, -0.31, 0.03), rot=(14.0, 47.0, 3.5), torso=(18.0, 35.0, 9.0), head=(-4.0, -56.0, 0.0), arm=(-46.0, -2.0, 0.78), blade=(4.0, 96.0), roll=-46.0),
        key(0.24, coil),
        key(0.29, coil, rot=(13.0, 59.0, 4.0), torso=(15.0, 38.0, 10.0), arm=(-31.0, 28.0, 0.72), blade=(-93.0, -8.0), roll=-12.0),
        key(0.35, hips=(-0.02, -0.28, -0.02), rot=(12.0, 26.0, 2.0), torso=(13.0, 16.0, 4.0), head=(0.0, -26.0, 0.0), arm=(-20.0, 10.0, 0.80), blade=(-58.0, 34.0), roll=8.0),
        key(0.4, hips=(0.02, -0.26, 0.0), rot=(10.0, -10.0, -1.0), torso=(10.0, -6.0, -3.0), head=(0.0, 10.0, 0.0), arm=(-16.0, 8.0, 0.86), blade=(-38.0, 20.0), roll=48.0),
        key(0.46, hips=(0.04, -0.27, 0.02), rot=(8.0, -42.0, -2.5), torso=(10.0, -26.0, -7.0), head=(0.0, 44.0, 0.0), arm=(-18.0, 12.0, 0.84), blade=(-61.0, 30.0), roll=48.0),
        key(0.58, END_2),
        key(0.86, hips=(0.04, -0.23, 0.02), rot=(7.0, -52.0, -2.0), torso=(10.0, -30.0, -6.0), head=(0.0, 58.0, 0.0), arm=(0.0, -4.0, 0.76), blade=(13.0, 72.0), roll=-40.0),
        key(1.06, hips=(0.02, -0.17, 0.02), rot=(4.0, -36.0, 1.0), torso=(8.0, -12.0, -3.0), head=(1.0, 34.0, 0.0), arm=(-8.0, -46.0, 0.78), blade=(80.0, 30.0), roll=-60.0),
        key(1.25, IDLE),
    ]
    foot_r_to = v(0.34, 0.09, -0.28)
    feet = {
        "foot_l": [(0.0, foot_l_from), (0.96, foot_l_from)] + step(foot_l_from, 0.98, 1.16, FOOT_L + v(0, 0, -ROOT_2), lift=0.04),
        "foot_l_rot": [(0.0, v(0, 14, 0)), (0.2, v(0, 34, 0)), (0.98, v(0, 34, 0)), (1.16, FOOT_L_ROT)],
        "foot_r": step(foot_r_from, 0.06, 0.38, foot_r_to, lift=0.09) + step(foot_r_to, 0.94, 1.14, FOOT_R + v(0, 0, -ROOT_2), lift=0.05),
        "foot_r_rot": [(0.0, v(24, -46, 0)), (0.16, v(0, -46, 0)), (0.38, v(0, -30, 0)), (0.94, v(0, -30, 0)), (1.14, FOOT_R_ROT)],
        "toe_r": [(0.0, v(26, 0, 0)), (0.14, v(0, 0, 0)), (1.25, v(0, 0, 0))],
    }
    root = [(0.0, 0.0), (0.12, 0.0), (0.28, -0.14), (0.38, -ROOT_2 * 0.86), (0.46, -ROOT_2)]
    compile_blow("heavy_2", 1.25, keys, feet, [(0.32, "swing"), (0.36, "hit_on"), (0.52, "hit_off"), (0.62, "combo"), (0.74, "cancel")], root=root)


def write_blow_3():
    """Down into a crouch, up off the ground arched like a bow with both hands over the head and the
    sword down the back, and everything falls at once into the ground. Angles are seen from a chest
    that's bent: forward pitch takes away from elevation, so to point ahead here the arm says up."""
    foot_l_from = v(-0.24, 0.09, -0.74 + ROOT_1 + ROOT_2)
    foot_r_from = v(0.34, 0.09, -0.28 + ROOT_2)
    crouch = dict(hips=(0.03, -0.36, 0.02), rot=(16.0, -44.0, 0.0), torso=(26.0, -22.0, -3.0), head=(-18.0, 50.0, 0.0), arm=(-4.0, 2.0, 0.74), blade=(76.0, 87.0), roll=-42.0)
    top = dict(hips=(0.0, 0.12, 0.02), rot=(-10.0, -18.0, 0.0), torso=(-20.0, -6.0, 0.0), head=(-18.0, 14.0, 0.0), arm=(-24.0, 60.0, 0.76), blade=(-28.0, 186.0), roll=-8.0)
    hit = dict(hips=(0.01, -0.4, 0.0), rot=(24.0, -10.0, 0.0), torso=(36.0, 2.0, 0.0), head=(-30.0, 6.0, 0.0), arm=(-24.0, -6.0, 0.88), blade=(4.0, 54.0), roll=-32.0)
    keys = [
        key(0.0, END_2),
        key(0.12, hips=(0.05, -0.31, 0.02), rot=(11.0, -54.0, -2.0), torso=(16.0, -32.0, -6.0), head=(-8.0, 60.0, 0.0), arm=(2.0, -6.0, 0.76), blade=(24.0, 57.0), roll=-58.0),
        key(0.24, crouch),
        key(0.36, hips=(0.01, -0.08, 0.02), rot=(-2.0, -30.0, 0.0), torso=(-2.0, -14.0, 0.0), head=(-12.0, 28.0, 0.0), arm=(-6.0, 28.0, 0.78), blade=(-20.0, 128.0), roll=-27.0),
        key(0.48, top),
        key(0.56, top, hips=(0.0, 0.08, 0.0), rot=(-11.0, -16.0, 0.0), torso=(-22.0, -5.0, 0.0), arm=(-24.0, 47.0, 0.77), blade=(-28.0, 190.0)),
        key(0.62, hips=(0.0, -0.1, -0.01), rot=(4.0, -13.0, 0.0), torso=(2.0, -2.0, 0.0), head=(-20.0, 10.0, 0.0), arm=(-24.0, 36.0, 0.74), blade=(-20.0, 136.0), roll=-40.0),
        key(0.67, hips=(0.01, -0.32, -0.01), rot=(18.0, -11.0, 0.0), torso=(26.0, 0.0, 0.0), head=(-26.0, 8.0, 0.0), arm=(-24.0, 0.0, 0.74), blade=(-8.0, 86.0)),
        key(0.72, hit),
        key(0.81, hit, hips=(0.01, -0.42, 0.0), rot=(25.0, -9.0, 0.0), torso=(38.0, 3.0, 0.0), arm=(-24.0, -3.0, 0.86), blade=(4.0, 64.0)),
        key(1.0, hit, hips=(0.01, -0.41, 0.0), rot=(23.0, -10.0, 0.0), torso=(35.0, 2.0, 0.0), head=(-24.0, 10.0, 0.0), arm=(-23.0, -7.0, 0.87), blade=(5.0, 52.0)),
        key(1.26, hips=(0.01, -0.24, 0.0), rot=(11.0, -16.0, 1.0), torso=(20.0, -4.0, -1.0), head=(-8.0, 18.0, 0.0), arm=(-12.0, -22.0, 0.80), blade=(107.0, 27.0), roll=-28.0),
        key(1.5, IDLE),
    ]
    land_l = v(-0.26, 0.09, -0.3 - ROOT_3)
    land_r = v(0.25, 0.09, 0.34 - ROOT_3)
    air_l = (foot_l_from + land_l) / 2.0 + v(0, 0.3, 0.05)
    air_r = (foot_r_from + land_r) / 2.0 + v(0, 0.24, 0.1)
    feet = {
        "foot_l": [(0.0, foot_l_from), (0.3, foot_l_from), (0.36, foot_l_from + v(0, 0.08, -0.03)), (0.5, air_l), (0.64, land_l + v(0, 0.08, 0.02)), (0.7, land_l), (1.14, land_l)] + step(land_l, 1.16, 1.36, FOOT_L + v(0, 0, -ROOT_3), lift=0.04),
        "foot_l_rot": [(0.0, v(0, 34, 0)), (0.3, v(0, 34, 0)), (0.5, v(30, 16, 0)), (0.7, v(0, 8, 0)), (1.16, v(0, 8, 0)), (1.36, FOOT_L_ROT)],
        "foot_r": [(0.0, foot_r_from), (0.32, foot_r_from), (0.38, foot_r_from + v(0, 0.08, -0.03)), (0.5, air_r), (0.64, land_r + v(0, 0.08, 0.02)), (0.71, land_r), (1.1, land_r)] + step(land_r, 1.12, 1.34, FOOT_R + v(0, 0, -ROOT_3), lift=0.05),
        "foot_r_rot": [(0.0, v(0, -30, 0)), (0.32, v(0, -30, 0)), (0.5, v(38, -36, 0)), (0.71, v(0, -44, 0)), (1.12, v(0, -44, 0)), (1.34, FOOT_R_ROT)],
    }
    root = [(0.0, 0.0), (0.3, 0.0), (0.38, -0.12), (0.58, -0.56), (0.7, -ROOT_3)]
    compile_blow("heavy_3", 1.5, keys, feet, [(0.6, "swing"), (0.63, "hit_on"), (0.74, "impact"), (0.74, "hit_off"), (1.06, "cancel")], root=root)


# ---- standing, walking and guarding


def compile_loop(name, length, pose, breathes=1.0, feet=None, count=24):
    """A pose that's alive: every part drifts on its own slow wave, and they all close at the end."""
    tracks = {control: [] for control in ("hips", "hips_rot", "torso", "head", "hand_r", "blade", "edge", "elbow_r", "hand_l", "elbow_l")}
    tau = 2.0 * math.pi / length
    for i in range(count + 1):
        t = length * i / count
        a, b, c = math.sin(tau * t), math.sin(tau * t + 1.1), math.sin(2 * tau * t + 2.1)
        late, later = math.sin(tau * (t - 0.22)), math.sin(tau * (t - 0.4) + 1.1)
        hips = v(*pose["hips"]) + v(0.012 * b, 0.009 * a, 0.0) * breathes
        rot = v(*pose["rot"]) + v(0.6 * a, 2.0 * b, 1.4 * b) * breathes
        torso = v(*pose["torso"]) + v(2.2 * a, 2.0 * b, 1.2 * c) * breathes
        head = v(*pose["head"]) + v(1.5 * b, 5.0 * c, 0.8 * a) * breathes
        arm = v(*pose["arm"]) + v(4.0 * later, 3.0 * late, 0.008 * late) * breathes
        blade = v(*pose["blade"]) + v(6.0 * later, 4.0 * late) * breathes
        place, turn = chest_of(hips, rot, torso)
        for control, value in (("hips", hips), ("hips_rot", rot), ("torso", torso), ("head", head)):
            tracks[control].append((t, value))
        for control, value in arm_controls(place, turn, arm, blade).items():
            tracks[control].append((t, value))
    tracks.update(feet or {})
    tracks["grip_l"] = [(0.0, (1.0, 0.0, 0.0)), (length, (1.0, 0.0, 0.0))]
    write_clip(name, length, True, tracks)


CARRY = dict(IDLE, hips=(0.0, -0.03, 0.0), rot=(0.0, 0.0, 0.0), torso=(6.0, -22.0, -3.0), head=(0.0, 18.0, 0.0))
# The blade lies across the front, point to the right and a little up, both hands on the handle in front
# of the chest: what a blow coming from ahead meets.
GUARD = dict(hips=(0.0, -0.16, 0.02), rot=(5.0, -10.0, 0.0), torso=(6.0, -14.0, 0.0), head=(-2.0, 24.0, 0.0), arm=(-36.0, 18.0, 0.72), blade=(86.0, 2.0))
IDLE_FEET = {
    "foot_l": [(0.0, FOOT_L), (3.6, FOOT_L)],
    "foot_l_rot": [(0.0, FOOT_L_ROT), (3.6, FOOT_L_ROT)],
    "foot_r": [(0.0, FOOT_R), (3.6, FOOT_R)],
    "foot_r_rot": [(0.0, FOOT_R_ROT), (3.6, FOOT_R_ROT)],
}


def write_block():
    """The blow lands on the guard and shoves it: sword back and aside, chest thrown round, then it
    comes back slower than it went."""
    shoved = dict(GUARD, hips=(0.02, -0.2, 0.05), rot=(1.0, -16.0, -3.0), torso=(-6.0, -20.0, -5.0), head=(4.0, 30.0, 0.0), arm=(-16.0, 14.0, 0.62), blade=(94.0, 28.0), roll=-56.0)
    keys = [
        key(0.0, GUARD),
        key(0.07, shoved),
        key(0.2, shoved, hips=(0.02, -0.21, 0.04), rot=(2.0, -15.0, -2.5), torso=(-4.0, -18.0, -4.0), arm=(-17.0, -3.0, 0.63), blade=(95.0, 26.0)),
        key(0.55, GUARD),
    ]
    compile_blow("heavy_block", 0.55, keys, {}, ())


def main(only=()):
    """Writes every clip, or only those named on the command line."""
    clips = {
        "heavy_1": write_blow_1,
        "heavy_2": write_blow_2,
        "heavy_3": write_blow_3,
        "heavy_idle": lambda: compile_loop("heavy_idle", 3.6, IDLE, feet=IDLE_FEET),
        "heavy_carry": lambda: compile_loop("heavy_carry", 2.0, CARRY, breathes=1.3),
        "heavy_guard": lambda: compile_loop("heavy_guard", 2.4, GUARD),
        "heavy_block": write_block,
    }
    for name, write in clips.items():
        if not only or name in only:
            write()


if __name__ == "__main__":
    import sys

    main(sys.argv[1:])
