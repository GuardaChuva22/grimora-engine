"""Writes where things are carried on the body and how they come out and go back: `draw_<place>` and
`stow_<place>`, plus `item_offer` and `item_eat`, in assets/models/chartest/chartest_clips, and the list
of places, assets/models/chartest/chartest_stow.ron.

A place is only where the hand is when it takes the thing. The game reads that moment back from the
baked clip and leaves the thing there, so nothing jumps between the body and the hand. Overwrites
those files, so edits made in the animator go away: run on request.

    python -I tools/gen_stow_clips.py [place...]
"""

import ast
import math
import os
import re
import sys

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
MODEL = os.path.join(HERE, "..", "assets", "models", "chartest")
OUT = os.path.join(MODEL, "chartest_clips")
STEP = 1.0 / 30.0

# Printed by `STOW_NUMBERS=1 cargo test -p grimora_scene --test stow`.
WEAPON_HEAD = (0.553015, 1.0480533, 0.013107374)
FINGERS_HEAD = (0.5959689, 1.0560137, -0.015695263)
HAND_REST_FRAME = [[-0.116068825, -0.0436694, -0.9922807], [-0.7521452, 0.65634257, 0.059094686], [0.6486954, 0.7531982, -0.10902664]]

SPINE = np.array([0.0, 1.07, 0.026])
CHEST = np.array([0.0, 1.302, 0.056])


def unit(v):
    v = np.array(v, dtype=float)
    return v / np.linalg.norm(v)


def rot_x(d):
    c, s = math.cos(math.radians(d)), math.sin(math.radians(d))
    return np.array([[1, 0, 0], [0, c, -s], [0, s, c]])


def rot_y(d):
    c, s = math.cos(math.radians(d)), math.sin(math.radians(d))
    return np.array([[c, 0, s], [0, 1, 0], [-s, 0, c]])


def rot_z(d):
    c, s = math.cos(math.radians(d)), math.sin(math.radians(d))
    return np.array([[c, -s, 0], [s, c, 0], [0, 0, 1]])


def rotation_of(a):
    """As `rotation_of` in pose.rs."""
    return rot_y(a[1]) @ rot_x(-a[0]) @ rot_z(-a[2])


def frame(blade, edge):
    b = unit(blade)
    e = np.array(edge, dtype=float)
    e = unit(e - b * e.dot(b))
    return np.column_stack([b, e, np.cross(b, e)])


def quat_of(m):
    t = np.trace(m)
    if t > 0:
        s = math.sqrt(t + 1.0) * 2
        return np.array([(m[2, 1] - m[1, 2]) / s, (m[0, 2] - m[2, 0]) / s, (m[1, 0] - m[0, 1]) / s, 0.25 * s])
    i = int(np.argmax(np.diag(m)))
    j, k = (i + 1) % 3, (i + 2) % 3
    s = math.sqrt(1.0 + m[i, i] - m[j, j] - m[k, k]) * 2
    q = np.zeros(4)
    q[i] = 0.25 * s
    q[j] = (m[j, i] + m[i, j]) / s
    q[k] = (m[k, i] + m[i, k]) / s
    q[3] = (m[k, j] - m[j, k]) / s
    return q


def matrix_of(q):
    x, y, z, w = q
    return np.array(
        [
            [1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
            [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
            [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)],
        ]
    )


def slerp(a, b, t):
    qa, qb = quat_of(a), quat_of(b)
    if qa.dot(qb) < 0:
        qb = -qb
    d = min(1.0, qa.dot(qb))
    if d > 0.9995:
        q = qa + (qb - qa) * t
    else:
        angle = math.acos(d)
        q = (math.sin((1 - t) * angle) * qa + math.sin(t * angle) * qb) / math.sin(angle)
    return matrix_of(q / np.linalg.norm(q))


def smooth(t):
    return t * t * (3 - 2 * t)


def chest_moved(point, torso):
    """Where a point fixed to the chest at rest goes when the torso turns."""
    r = rotation_of(torso)
    half = slerp(np.eye(3), r, 0.45)
    chest = SPINE + half @ (CHEST - SPINE)
    return chest + r @ (np.array(point, dtype=float) - CHEST)


def chest_turned(f, torso):
    return rotation_of(torso) @ f


# ---- the arm, as `Rig::solve` does it

UPPER, FOREARM, HAND = np.array([0.19, 1.453, 0.114]), np.array([0.361, 1.258, 0.128]), np.array([0.532, 1.117, 0.022])
CLAV = np.array([0.0, 1.529, 0.091])
WEAPON = np.array(WEAPON_HEAD)
FINGERS = np.array(FINGERS_HEAD)
UPPER_LEN, FORE_LEN = np.linalg.norm(FOREARM - UPPER), np.linalg.norm(HAND - FOREARM)
HAND_REST = np.array(HAND_REST_FRAME).T


def rig_frame(direction, pole):
    d = unit(direction)
    p = np.array(pole, dtype=float)
    p = p - d * p.dot(d)
    if np.linalg.norm(p) < 1e-5:
        p = np.array([0.0, 0.0, 1.0]) - d * d[2]
    p = unit(p)
    return np.column_stack([d, p, np.cross(d, p)])


def aim(rest_dir, rest_pole, direction, pole):
    return rig_frame(direction, pole) @ rig_frame(rest_dir, rest_pole).T


def hinge_poles(upper, lower, pole):
    axis = np.cross(lower, upper)
    if np.linalg.norm(axis) < 0.008:
        return pole, pole
    axis = unit(axis)
    return np.cross(axis, unit(upper)), np.cross(axis, unit(lower))


REST_POLE_FORE = hinge_poles(FOREARM - UPPER, HAND - FOREARM, np.array([0.0, 0.0, 1.0]))[1]


def arc_turn(a, b):
    a, b = unit(a), unit(b)
    v = np.cross(a, b)
    c = a.dot(b)
    if c < -0.9999:
        return -np.eye(3)
    k = np.array([[0, -v[2], v[1]], [v[2], 0, -v[0]], [-v[1], v[0], 0]])
    return np.eye(3) + k + k @ k / (1 + c)


def solve_arm(point, held, torso, pole):
    """What the forearm does with the hand here: its turn, the axis, how short the arm is, and the elbow."""
    turn = held @ HAND_REST.T
    wrist = np.array(point) - turn @ (WEAPON - HAND)
    r = rotation_of(torso)
    half = slerp(np.eye(3), r, 0.45)
    chest = SPINE + half @ (CHEST - SPINE)
    clav = chest + r @ (CLAV - CHEST)
    shoulder = clav + r @ (UPPER - CLAV)
    pull = arc_turn(r @ unit(FOREARM - UPPER), wrist - shoulder)
    upper = clav + slerp(np.eye(3), pull, 0.18) @ r @ (UPPER - CLAV)
    to = wrist - upper
    reach = (UPPER_LEN + FORE_LEN) * 0.999
    short = max(0.0, np.linalg.norm(to) - reach)
    distance = max(min(np.linalg.norm(to), reach), abs(UPPER_LEN - FORE_LEN) + 1e-3)
    d = unit(to)
    along = (UPPER_LEN**2 - FORE_LEN**2 + distance**2) / (2 * distance)
    height = math.sqrt(max(0.0, UPPER_LEN**2 - along**2))
    side = np.array(pole, dtype=float) - d * d.dot(pole)
    if np.linalg.norm(side) < 1e-5:
        side = np.array([0.0, 0.0, 1.0])
    elbow = upper + d * along + unit(side) * height
    at_wrist = elbow + unit(wrist - elbow) * FORE_LEN
    _, pole_b = hinge_poles(elbow - upper, at_wrist - elbow, np.array(pole, dtype=float))
    fore = aim(unit(HAND - FOREARM), REST_POLE_FORE, at_wrist - elbow, pole_b)
    return fore, unit(at_wrist - elbow), short, elbow


def strain(held, fore, axis):
    """Degrees of the wrist's bend and of its twist, as `Rig::solve` measures them."""
    turn = held @ HAND_REST.T
    wrist = math.degrees(math.acos(max(-1, min(1, (turn @ unit(FINGERS - HAND)).dot(axis)))))
    rel = quat_of(fore.T @ turn)
    if rel[3] < 0:
        rel = -rel
    local = fore.T @ axis
    along = local * rel[:3].dot(local)
    q = np.array([along[0], along[1], along[2], rel[3]])
    q = q / np.linalg.norm(q)
    return wrist, 2 * math.degrees(math.acos(min(1.0, abs(q[3]))))


def poles_around(hint, point, torso, degrees):
    r = rotation_of(torso)
    axis = unit(np.array(point) - (CHEST + r @ (UPPER - CHEST)))
    k = np.array([[0, -axis[2], axis[1]], [axis[2], 0, -axis[0]], [-axis[1], axis[0], 0]])
    for d in degrees:
        a = math.radians(d)
        yield d, (np.eye(3) + math.sin(a) * k + (1 - math.cos(a)) * k @ k) @ np.array(hint, dtype=float)


def best_hand(point, blade, torso, pole_hint, hold=1.0):
    """The elbow and the roll of the hand that bend the wrist least, with the blade near `blade`."""
    best = None
    for d, pole in poles_around(pole_hint, point, torso, range(-90, 91, 15)):
        for tilt_x in (-30, -15, 0, 15, 30):
            for tilt_y in (-30, -15, 0, 15, 30):
                b = rot_x(tilt_x) @ rot_y(tilt_y) @ unit(blade)
                seen = abs(tilt_x) + abs(tilt_y)
                for roll in range(0, 360, 20):
                    start = unit(np.cross(b, (0.31, 0.77, 0.55)))
                    a = math.radians(roll)
                    e = start * math.cos(a) + np.cross(b, start) * math.sin(a)
                    held = rig_frame(b, e)
                    fore, axis, short, elbow = solve_arm(point, held, torso, pole)
                    wrist, twist = strain(held, fore, axis)
                    # Inside the body is no place for an elbow.
                    inside = max(0.0, 0.2 - math.hypot(elbow[0], elbow[2] - 0.05)) * 200
                    cost = wrist + 1.2 * max(0.0, twist - 35.0) + 0.1 * abs(d) + hold * 0.8 * seen + 900 * short + inside
                    if best is None or cost < best[0]:
                        best = (cost, held, pole, wrist, twist, short)
    return best


def read_first_key(clip):
    """The first key of each control in a clip file, to end on what comes next."""
    with open(os.path.join(OUT, clip + ".ron"), encoding="utf8") as source:
        text = source.read()
    out = {}
    for name, first in re.findall(r"(\w+): \[\s*\([\d.]+, (\([^)]*\))\)", text):
        out[name] = np.array(ast.literal_eval(first), dtype=float)
    return out


# ---- how things sit in the hand

# As in tools/gen_gun_clips.py: from a gun's file to the `weapon.R` joint.
PISTOL_HELD = rot_y(90) @ rot_z(30)


def gun_frame(barrel, top):
    x = unit(barrel)
    y = np.array(top, dtype=float)
    y = unit(y - x * y.dot(x))
    z = np.cross(x, y)
    held = np.column_stack([x, y, z]) @ PISTOL_HELD.T
    return np.column_stack([held[:, 1], held[:, 2], np.cross(held[:, 1], held[:, 2])])


MUSKET_HELD = rot_y(90) @ rot_z(25)


def musket_frame(barrel, top):
    """As `gun_frame`, for the musket's turn in the hand."""
    x = unit(barrel)
    y = np.array(top, dtype=float)
    y = unit(y - x * y.dot(x))
    held = np.column_stack([x, y, np.cross(x, y)]) @ MUSKET_HELD.T
    return np.column_stack([held[:, 1], held[:, 2], np.cross(held[:, 1], held[:, 2])])


# A long gun slung across the back, muzzle up by the left shoulder and the stock by the right hip:
# the right hand takes the stock and swings it to the front. `SLING_AT` is where the grip is.
SLING_AT = (0.16, 1.05, 0.21)
SLING_LONG = (-0.551, 0.832, 0.06)
SLING_TOP = (0.834, 0.552, 0.0)


# ---- poses the hand comes from and goes back to

LOOSE_HAND = {"p": np.array([0.285, 0.985, -0.03]), "f": frame((0.3, -0.34, -0.89), (0.05, 0.93, -0.36)), "elbow": np.array([0.35, -0.15, 1.0])}
TORSO_REST = (2.0, 0.0, 0.0)
HEAD_REST = (0.0, 0.0, 0.0)


def end_from_clip(clip, fallback=LOOSE_HAND):
    keys = read_first_key(clip)
    if "hand_r" not in keys:
        return fallback
    return {
        "p": keys["hand_r"],
        "f": frame(keys["blade"], keys["edge"]),
        "elbow": keys.get("elbow_r", fallback["elbow"]),
        "torso": keys.get("torso", np.array(TORSO_REST)),
        "head": keys.get("head", np.array(HEAD_REST)),
    }


# ---- a clip as poses in time

class Key:
    def __init__(self, t, p, f, elbow, torso, head):
        self.t, self.p, self.f = t, np.array(p, dtype=float), f
        self.elbow, self.torso, self.head = np.array(elbow, dtype=float), np.array(torso, dtype=float), np.array(head, dtype=float)


def catmull(keys, i, u):
    """On the curve through the keys' hands, with their real times, so the arcs stay arcs."""
    p1, p2 = keys[i].p, keys[i + 1].p
    t1, t2 = keys[i].t, keys[i + 1].t
    dt = t2 - t1
    p0, t0 = (keys[i - 1].p, keys[i - 1].t) if i > 0 else (p1, t1 - dt)
    p3, t3 = (keys[i + 2].p, keys[i + 2].t) if i + 2 < len(keys) else (p2, t2 + dt)
    m1 = (p2 - p0) / (t2 - t0) * dt
    m2 = (p3 - p1) / (t3 - t1) * dt
    # Ends stop.
    if i == 0:
        m1 = m1 * 0.0
    if i + 2 >= len(keys):
        m2 = m2 * 0.0
    h = (2 * u**3 - 3 * u**2 + 1, u**3 - 2 * u**2 + u, -2 * u**3 + 3 * u**2, u**3 - u**2)
    return h[0] * p1 + h[1] * m1 + h[2] * p2 + h[3] * m2


def track_times(keys):
    duration = keys[-1].t
    times = set(round(k.t, 4) for k in keys)
    t = 0.0
    while t < duration:
        times.add(round(t, 4))
        t += STEP
    return sorted(times)


def within_reach(key, most=0.445):
    """Brings the hand back in if the arm wouldn't get there, keeping the way it goes from the shoulder."""
    if key.f is not None:
        return
    r = rotation_of(key.torso)
    half = slerp(np.eye(3), r, 0.45)
    shoulder = SPINE + half @ (CHEST - SPINE) + r @ (UPPER - CHEST)
    # The wrist is behind the hand by about this much.
    to = key.p - shoulder
    length = np.linalg.norm(to)
    if length > most + 0.09:
        key.p = shoulder + to * (most + 0.09) / length


def write_clip(name, keys, looping=False, extra=None):
    """Writes keys for each control. The hand turns relative to the forearm between the poses that
    say how it turns, so the wrist doesn't twist on the way, wherever the arm goes."""
    anchors = []
    for key in keys:
        within_reach(key)
        if key.f is not None:
            fore, _, _, _ = solve_arm(key.p, key.f, key.torso, key.elbow)
            anchors.append((key.t, fore.T @ (key.f @ HAND_REST.T)))
    tracks = {c: [] for c in ("hand_r", "blade", "edge", "elbow_r", "torso", "head")}
    held = keys[0].f
    worst = [0.0, 0.0, 0.0]
    for t in track_times(keys):
        i = min(max(j for j, k in enumerate(keys) if k.t <= t + 1e-6), len(keys) - 2)
        a, b = keys[i], keys[i + 1]
        u = min(1.0, (t - a.t) / (b.t - a.t))
        s = smooth(u)
        p = catmull(keys, i, u)
        pole = a.elbow + (b.elbow - a.elbow) * s
        torso = a.torso + (b.torso - a.torso) * s
        n = min(max(j for j, x in enumerate(anchors) if x[0] <= t + 1e-6), len(anchors) - 2)
        v = min(1.0, (t - anchors[n][0]) / (anchors[n + 1][0] - anchors[n][0]))
        rel = slerp(anchors[n][1], anchors[n + 1][1], smooth(v))
        for _ in range(3):
            fore, axis, short, _ = solve_arm(p, held, torso, pole)
            held = fore @ rel @ HAND_REST
        wrist, twist = strain(held, fore, axis)
        worst = [max(worst[0], wrist), max(worst[1], twist), max(worst[2], short * 1000)]
        values = {"hand_r": p, "blade": held[:, 0], "edge": held[:, 1], "elbow_r": pole, "torso": torso, "head": a.head + (b.head - a.head) * s}
        for control, value in values.items():
            tracks[control].append((t, value))
    if extra:
        tracks.update(extra)
    save(name, keys[-1].t, looping, tracks)
    print(f"  {name:20s} wrist {worst[0]:3.0f} twist {worst[1]:3.0f} short {worst[2]:3.0f}")
    return tracks


def save(name, duration, looping, tracks):
    lines = ["(", f"    duration: {duration:.4f},", f"    looping: {str(looping).lower()},", "    springs: false,", "    tracks: {"]
    for control, track in tracks.items():
        lines.append(f"        {control}: [")
        lines.extend(f"            ({t:.4f}, ({v[0]:.4f}, {v[1]:.4f}, {v[2]:.4f}))," for t, v in track)
        lines.append("        ],")
    lines.extend(["    },", ")", ""])
    with open(os.path.join(OUT, f"{name}.ron"), "w", encoding="utf8", newline="\n") as out:
        out.write("\n".join(lines))


# ---- the places

class Place:
    def __init__(self, id, family, bone, point, blade=None, edge=None, f=None, elbow=(0.35, -0.15, 1.0), torso=(0, 0, 0), head=(0, 0, 0), span=(0.0, 0.0), out=(0, 0, 0), style="hip", hand="weapon.R", lay=None):
        self.id, self.family, self.bone, self.hand = id, family, bone, hand
        self.point = np.array(point, dtype=float)
        self.f = f if f is not None else frame(blade, edge)
        self.elbow, self.torso, self.head = np.array(elbow, dtype=float), np.array(torso, dtype=float), np.array(head, dtype=float)
        self.span = span
        self.out = np.array(out, dtype=float)
        self.style = style
        # How the thing lies there, when it doesn't lie the way the hand takes it.
        self.lay = lay or (point, self.f[:, 0] if f is None else None, self.f[:, 1] if f is None else None)
        self.top = (blade, edge)

    def segment(self):
        """What it takes up on the body, nominally: from one end of the thing to the other."""
        along = self.f[:, 0] if self.family != "Pistol" else unit(0.5 * self.f[:, 0] - 0.87 * self.f[:, 1])
        return self.point + along * self.span[0], self.point + along * self.span[1]


SWORD = (-0.14, 0.95)
GREAT = (-0.26, 1.3)
GUN_SPAN = (-0.06, 0.4)
SMALL = (-0.05, 0.05)
LONG_GUN = (-1.12, 0.28)

# Each place says how the thing lies on the body, in the model's space, touching it: `point` is where
# the handle goes (and where the hand goes for it), `blade` where its tip or muzzle points and `edge`
# where its edge or top points. What's taken by another part than the handle (`lay`) gives the middle
# of it instead. The body's back comes to z 0.205 at the shoulder blades, 0.13 at the waist and 0.16
# at the seat, its side to x 0.18 at the belt, and its front to z -0.07.
ACROSS_BACK = (1.0, 0.0, 0.0)
PLACES = [
    # On the left of the belt, across the body: the classic draw, tip down and back, flat on the hip.
    Place("belt_l0", "Sword", "hips", (-0.19, 1.04, -0.12), (-0.1, -0.62, 0.78), (0.0, 0.78, 0.62), elbow=(0.8, -0.15, -0.55), torso=(18, 30, 4), head=(14, 18, 0), span=SWORD, out=(-0.05, 0.02, -0.06)),
    Place("belt_l1", "Sword", "hips", (-0.225, 1.09, -0.06), (-0.12, -0.45, 0.88), (0.0, 0.88, 0.45), elbow=(0.8, -0.1, -0.5), torso=(16, 34, 5), head=(12, 22, 0), span=SWORD, out=(-0.06, 0.03, -0.06)),
    Place("belt_l2", "Sword", "hips", (-0.21, 0.97, -0.115), (-0.08, -0.8, 0.6), (0.0, 0.6, 0.8), elbow=(0.8, -0.2, -0.55), torso=(22, 34, 5), head=(16, 20, 0), span=SWORD, out=(-0.05, 0.0, -0.06)),
    # On the right, drawn straight up: short blades go here first.
    Place("belt_r0", "Sword", "hips", (0.195, 1.04, -0.11), (0.1, -0.62, 0.78), (0.0, 0.78, 0.62), elbow=(0.5, -0.2, 0.9), torso=(5, -12, -3), head=(10, -14, 0), span=SWORD, out=(0.05, 0.03, -0.06)),
    Place("belt_r1", "Sword", "hips", (0.225, 1.0, -0.05), (0.08, -0.8, 0.6), (0.0, 0.6, 0.8), elbow=(0.5, -0.2, 0.9), torso=(4, -16, -4), head=(10, -18, 0), span=SWORD, out=(0.05, 0.03, -0.05)),
    # Over the shoulders, flat on the back, in layers so that they cross without touching.
    Place("back_r0", "Back", "chest", (0.2, 1.6, 0.225), (-0.5, -0.86, -0.03), ACROSS_BACK, elbow=(0.95, 0.1, -0.3), torso=(4, -8, -3), head=(-4, 14, 0), span=GREAT, style="back"),
    Place("back_l0", "Back", "chest", (-0.2, 1.6, 0.25), (0.5, -0.86, -0.03), ACROSS_BACK, elbow=(0.55, 0.5, -0.9), torso=(4, -4, 0), head=(2, -24, 0), span=GREAT, style="back_cross"),
    Place("back_m0", "Back", "chest", (0.035, 1.7, 0.275), (0.0, -1.0, -0.03), ACROSS_BACK, elbow=(0.8, 0.5, -0.7), torso=(2, -6, -2), head=(10, -6, 0), span=GREAT, style="back"),
    Place("back_r1", "Back", "chest", (0.27, 1.53, 0.25), (-0.2, -0.97, -0.02), ACROSS_BACK, elbow=(0.9, 0.3, -0.5), torso=(3, -10, -3), head=(-4, 16, 0), span=GREAT, style="back"),
    Place("back_l1", "Back", "chest", (-0.12, 1.67, 0.3), (0.24, -0.97, -0.02), ACROSS_BACK, elbow=(0.6, 0.5, -0.8), torso=(3, -4, 0), head=(6, -14, 0), span=GREAT, style="back_cross"),
    # A long gun slung with the muzzle up, flat on the back, resting on the shoulder blades and the
    # seat, and held by the barrel, near its end.
    Place("sling_r", "Slung", "chest", SLING_AT, f=musket_frame(SLING_LONG, SLING_TOP), elbow=(0.95, 0.1, -0.3), torso=(4, -8, -3), head=(-4, 14, 0), span=LONG_GUN, style="sling", lay=(SLING_AT, SLING_LONG, SLING_TOP)),
    # Pistols in the pouches of accessories/Coldre_Couro.glb (fitted to them): muzzle down and the
    # butt back on the right hip, and butt forward on the left, for the right hand across the body.
    Place("holster_r0", "Pistol", "hips", (0.176, 1.195, 0.103), f=gun_frame((0.1855, -0.9812, 0.0535), (0.0099, -0.0526, -0.9986)), elbow=(-0.3, 0.4, 0.9), torso=(10, 20, 5), head=(10, -14, 0), span=GUN_SPAN, out=(0.05, 0.05, 0.02)),
    Place("holster_l0", "Pistol", "hips", (-0.177, 1.185, -0.089), f=gun_frame((-0.1855, -0.9812, 0.0535), (0.0099, 0.0526, 0.9986)), elbow=(0.5, 1.0, 0.0), torso=(8, 12, -3), head=(14, 16, 0), span=GUN_SPAN, out=(0.0, 0.05, -0.06)),
    # Flasks hang upright from the belt, in front and behind, out of the way of the blades.
    Place("pouch_f0", "Small", "hips", (0.09, 0.99, -0.15), (0.15, -0.3, 0.94), (0.15, 0.9, 0.35), elbow=(0.8, -0.25, 0.6), torso=(10, 8, 0), head=(18, -6, 0), span=SMALL, out=(0.04, 0.04, -0.05), style="pouch", lay=((0.09, 0.99, -0.1), (0.0, 1.0, 0.0), (1.0, 0.0, 0.0))),
    Place("pouch_f1", "Small", "hips", (-0.05, 0.99, -0.155), (0.1, -0.3, 0.95), (0.3, 0.9, 0.25), elbow=(0.8, -0.25, 0.5), torso=(12, 16, 2), head=(18, 8, 0), span=SMALL, out=(0.04, 0.04, -0.05), style="pouch", lay=((-0.05, 0.99, -0.1), (0.0, 1.0, 0.0), (1.0, 0.0, 0.0))),
    Place("pouch_b0", "Small", "hips", (0.15, 1.0, 0.215), (0.45, -0.5, -0.74), (0.3, 0.78, -0.55), elbow=(0.85, 0.15, 0.55), torso=(6, -22, -4), head=(4, -24, 0), span=SMALL, out=(0.06, 0.03, 0.0), style="pouch", lay=((0.13, 1.0, 0.175), (0.0, 1.0, 0.0), (1.0, 0.0, 0.0))),
    Place("pouch_b1", "Small", "hips", (0.24, 1.0, 0.09), (0.45, -0.5, -0.74), (0.3, 0.78, -0.55), elbow=(0.85, 0.15, 0.55), torso=(6, -14, -4), head=(4, -20, 0), span=SMALL, out=(0.06, 0.03, 0.0), style="pouch", lay=((0.2, 1.0, 0.07), (0.0, 1.0, 0.0), (1.0, 0.0, 0.0))),
    Place("pouch_b2", "Small", "hips", (0.245, 0.99, -0.06), (0.15, -0.3, 0.94), (0.15, 0.9, 0.35), elbow=(0.8, -0.25, 0.6), torso=(10, 2, 0), head=(18, -12, 0), span=SMALL, out=(0.06, 0.03, 0.0), style="pouch", lay=((0.2, 0.99, -0.05), (0.0, 1.0, 0.0), (1.0, 0.0, 0.0))),
    # Whatever else comes out of the pocket, and doesn't show until then.
    Place("pocket", "Pocket", "hips", (0.235, 0.95, -0.02), (0.15, -0.3, 0.94), (0.15, 0.9, 0.35), elbow=(0.85, 0.1, 0.5), torso=(3, -6, -2), head=(6, -8, 0), span=SMALL, out=(0.06, 0.03, 0.0), style="pouch"),
]

# For the free hand: where the hand goes, how it's turned, and how the book lies (a point of the body
# it touches, its width, and the way out of the body).
BOOK_PLACES = [
    ("book_0", (-0.24, 1.02, 0.07), (-5, 45, 0), ((-0.175, 1.0, 0.03), (-0.25, 0.0, -0.97), (-0.97, 0.0, 0.25))),
    ("book_1", (-0.15, 1.0, 0.235), (-5, 40, 0), ((-0.07, 1.0, 0.155), (-1.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
]


# The pistols again, as the barrel and the top of `gun_frame`.
GUN_LAYS = {
    "holster_r0": ((0.1855, -0.9812, 0.0535), (0.0099, -0.0526, -0.9986)),
    "holster_l0": ((-0.1855, -0.9812, 0.0535), (0.0099, 0.0526, 0.9986)),
}


def draw_keys(place, end):
    """The poses of taking it: to it, off it, and round to where the hand carries it. Only the ones
    with a turn of the hand say how it should turn: the others leave it to the forearm."""
    start = LOOSE_HAND
    site, torso, head = place.point, place.torso, place.head
    blade = place.f[:, 0]
    if place.bone == "chest":
        site, blade = chest_moved(site, torso), rotation_of(torso) @ blade
    _, f, elbow, wrist, twist, short = best_hand(site, blade, torso, place.elbow, 1.0)
    if place.family == "Pocket":
        # Into the pocket the way it hangs: nothing to turn the hand for.
        # The hand keeps its turn on the forearm from where it hangs, so the wrist doesn't move.
        fore0, *_ = solve_arm(start["p"], start["f"], TORSO_REST, start["elbow"])
        rel = fore0.T @ (start["f"] @ HAND_REST.T)
        elbow = np.array(start["elbow"], dtype=float)
        f = start["f"]
        for _ in range(4):
            fore, axis, short, _ = solve_arm(site, f, torso, elbow)
            f = fore @ rel @ HAND_REST
        wrist, twist = strain(f, fore, axis)
    if place.family in ("Pistol", "Slung"):
        # The hand goes to the butt as it is, so that the gun needn't turn in it.
        f = rotation_of(torso) @ place.f if place.bone == "chest" else place.f
        tries = []
        for _, pole in poles_around(place.elbow, site, torso, range(-90, 91, 15)):
            fore, axis, short, _ = solve_arm(site, f, torso, pole)
            bend, turn = strain(f, fore, axis)
            tries.append((bend + 0.5 * turn + 900 * short, tuple(pole), bend, turn, short))
        _, elbow, wrist, twist, short = min(tries, key=lambda row: row[0])
        elbow = np.array(elbow)
    end_torso = end.get("torso", np.array(TORSO_REST))
    end_head = end.get("head", np.array(HEAD_REST))
    t_ = np.array(torso, dtype=float)
    h_ = np.array(head, dtype=float)
    keys = [Key(0.0, start["p"], start["f"], start["elbow"], TORSO_REST, HEAD_REST)]
    if place.style in ("hip", "pouch"):
        ahead = site + place.out * 1.2 + np.array([0.0, 0.06, 0.0])
        keys += [
            Key(0.13, (start["p"] * 0.45 + ahead * 0.55) + (0, 0.04, -0.05), None, elbow * 0.5 + start["elbow"] * 0.5, t_ * 0.65, h_ * 0.8),
            Key(0.25, ahead, None, elbow, t_ * 0.95, head),
            Key(0.33, site, f, elbow, torso, head),
        ]
        if place.style == "hip":
            # A pistol comes straight up out of its holster, the way the barrel went in.
            along = f[:, 0] if place.family != "Pistol" else unit(GUN_LAYS[place.id][0])
            # Straight out along itself for a moment, then round the outside of the hip.
            pulled = site - along * 0.2 + place.out * 0.4
            sweep = site - along * 0.28 + place.out * 2.0 + np.array([0.08, 0.1, -0.14])
            swung = np.array([0.3, 1.2, -0.3])
            keys += [
                Key(0.42, pulled, None, elbow, t_ * 0.8, h_ * 0.6),
                Key(0.52, sweep, None, elbow * 0.45 + end["elbow"] * 0.55, t_ * 0.2, h_ * 0.1),
                Key(0.63, swung, None, end["elbow"], (1.5, -7.0, -1.0), (0, -3, 0)),
                Key(0.88, end["p"], end["f"], end["elbow"], end_torso, end_head),
            ]
        else:
            lift = (site + end["p"]) * 0.5 + np.array([0.04, 0.06, -0.06])
            keys += [
                Key(0.45, lift, None, elbow * 0.5 + end["elbow"] * 0.5, t_ * 0.45, h_ * 0.4),
                Key(0.66, end["p"], end["f"], end["elbow"], end_torso, end_head),
            ]
    else:
        # Up and over the shoulder, with the elbow high, and down in front.
        cross = place.style == "back_cross"
        up = (site + start["p"]) * 0.5 + np.array([0.22, 0.2, -0.28])
        over = np.array([0.02 if cross else 0.3, 1.86, 0.1])
        high = np.array([0.32, 1.72, -0.15])
        front = np.array([0.33, 1.32, -0.33])
        _, high_f, high_elbow, *_ = best_hand(high, (0.12, 0.93, 0.33), (0, 6, 0), (0.9, 0.5, -0.3), 1.0)
        _, front_f, front_elbow, *_ = best_hand(front, (0.25, 0.35, -0.9), (5, 8, 2), (0.6, -0.4, 0.5), 1.0)
        keys += [
            Key(0.15, up, None, elbow * 0.6 + start["elbow"] * 0.4, t_ * 0.6, h_ * 0.5),
            Key(0.28, site + np.array([0.02, 0.06, -0.07]), None, elbow, t_ * 0.95, head),
            Key(0.36, site, f, elbow, torso, head),
            Key(0.47, over, None, elbow, (2, -4, -1), h_ * 0.5),
            Key(0.59, high, high_f, high_elbow, (0, 6, 0), (0, -2, 0)),
            Key(0.73, front, front_f, front_elbow, (5, 8, 2), (2, 0, 0)),
            Key(1.0, end["p"], end["f"], end["elbow"], end_torso, end_head),
        ]
    print(f"{place.id:12s} at grab: wrist {wrist:3.0f} twist {twist:3.0f} short {short*1000:3.0f} blade off {math.degrees(math.acos(max(-1,min(1,f[:,0].dot(unit(blade)))))):3.0f}")
    return keys


# ---- pistols out of the holster

# The pistol sits high and long in its holster, so it comes out in three parts: the hand reaches the
# butt with the turn it will hold it with, slides the barrel straight up out of the leather (only
# at the very end does it tip out), and swings it round to the front, turning relative to the
# forearm so the wrist never twists on the way. Found by a search (`holster_plan` and every frame
# measured: both strains, how short the arm is, and elbow and hand kept out of the trunk).
# `times` are the grab, out of the holster, swung round, and done.
HOLSTER_PLANS = {
    "holster_r0": {"lift": 0.24, "lift_off": (0.12, 0.0, 0.05), "ahead": (0.111, 0.0, 0.05), "swing": (0.263, 1.044, -0.115), "bulge": (-0.077, 0.003, 0.044),
                   "times": (0.3, 0.48, 0.66, 0.86), "tip": 45.0, "tip_toward": (0.0, 0.0, -1.0), "turn_by_swing": 1.0,
                   "torso_grab": (15.0, 25.0, -10.0), "torso_lift": (10.943, 3.308, -10.892), "torso_swing": (-0.304, 1.37, -2.406),
                   "head_grab": (10.0, -14.0, 0.0), "head_lift": (6.0, -8.0, 0.0)},
    "holster_l0": {"lift": 0.24, "lift_off": (0.04, 0.0, -0.068), "ahead": (0.101, 0.021, -0.1), "swing": (0.348, 1.072, -0.198), "bulge": (-0.014, 0.069, -0.08),
                   "times": (0.3, 0.48, 0.66, 0.86), "tip": 26.947, "tip_toward": (0.0, 0.0, -1.0), "turn_by_swing": 0.843,
                   "torso_grab": (-7.235, -18.43, -15.0), "torso_lift": (-11.284, -17.848, -18.0), "torso_swing": (-2.33, 1.295, 0.994),
                   "head_grab": (10.0, 14.0, 0.0), "head_lift": (6.0, 8.0, 0.0)},
}


def holster_tip(place, plan, u):
    barrel = unit(GUN_LAYS[place.id][0])
    return axis_turn(np.cross(barrel, np.array(plan["tip_toward"], dtype=float)), plan["tip"] * u ** 3)


def forearm_rel(p, f, torso, pole):
    fore, *_ = solve_arm(p, f, torso, pole)
    return fore.T @ (f @ HAND_REST.T)


def hand_from_rel(p, rel, torso, pole, guess):
    f = guess
    for _ in range(4):
        fore, *_ = solve_arm(p, f, torso, pole)
        f = fore @ rel @ HAND_REST
    return f


def out_of_trunk(elbow, p):
    cost = 0.0
    for q, room in ((elbow, 0.0), (p, 0.03)):
        if 0.9 < q[1] < 1.55:
            e = math.hypot(q[0] / 0.17, (q[2] - 0.04) / 0.13)
            cost += max(0.0, 1.0 + room / 0.15 - e) * 300
    return cost


def keyed(keys, t):
    for (a, va), (b, vb) in zip(keys, keys[1:]):
        if t <= b + 1e-9:
            return va + (vb - va) * smooth(min(1.0, max(0.0, (t - a) / (b - a))))
    return keys[-1][1]


def holster_frames(place, plan, end):
    """Every frame of the draw: time, hand, its turn, the elbow's pole, torso, head, and the strains."""
    barrel = unit(GUN_LAYS[place.id][0])
    grab_f, site, start = place.f, place.point, LOOSE_HAND
    tg, tl, ts, te = plan["times"]
    lift_top = site - barrel * plan["lift"] + np.array(plan["lift_off"])
    ahead = site + np.array(plan["ahead"])
    swing = np.array(plan["swing"])
    end_t, end_h = np.array(end["torso"]), np.array(end["head"])
    torso_keys = [(0, np.array(TORSO_REST)), (tg, np.array(plan["torso_grab"])), (tl, np.array(plan["torso_lift"])), (ts, np.array(plan["torso_swing"])), (te, end_t)]
    head_keys = [(0, np.array(HEAD_REST)), (tg, np.array(plan["head_grab"])), (tl, np.array(plan["head_lift"])), (ts, end_h * 0.6), (te, end_h)]
    rel_loose = forearm_rel(start["p"], start["f"], TORSO_REST, start["elbow"])
    rel_end = forearm_rel(end["p"], end["f"], end_t, end["elbow"])
    prev, f_prev, rel_top = np.array(start["elbow"], dtype=float), start["f"], None
    rows = []
    for i in range(int(round(te * 30)) + 1):
        t = i / 30
        torso, head = keyed(torso_keys, t), keyed(head_keys, t)
        if t <= tg + 1e-9:
            u = smooth(t / tg)
            p, mode = bezier(start["p"], ahead, site, u), ("reach", u)
        elif t <= tl + 1e-9:
            u = smooth((t - tg) / (tl - tg))
            p, mode = site + (lift_top - site) * u, ("lift", holster_tip(place, plan, u) @ grab_f)
        elif t <= ts + 1e-9:
            u = smooth((t - tl) / (ts - tl))
            p = bezier(lift_top, (lift_top + swing) * 0.5 + np.array(plan["bulge"]), swing, u)
            mode = ("swing", u * plan["turn_by_swing"])
        else:
            u = smooth((t - ts) / (te - ts))
            p = swing + (end["p"] - swing) * u
            mode = ("swing", plan["turn_by_swing"] + (1 - plan["turn_by_swing"]) * u)
        best = None
        for d, pole in poles_around(prev, p, torso, range(-90, 91, 6)):
            if mode[0] == "lift":
                f = mode[1]
            elif mode[0] == "reach":
                rel = slerp(rel_loose, forearm_rel(site, grab_f, plan["torso_grab"], pole), mode[1])
                f = hand_from_rel(p, rel, torso, pole, f_prev)
            else:
                f = hand_from_rel(p, slerp(rel_top, rel_end, mode[1]), torso, pole, f_prev)
            fore, axis, short, elbow = solve_arm(p, f, torso, pole)
            bend, twist = strain(f, fore, axis)
            cost = max(0, bend - 35) * 1.5 + max(0.0, twist - 35) + 900 * short + 0.3 * abs(d) + out_of_trunk(elbow, p)
            if best is None or cost < best[0]:
                best = (cost, pole, f, bend, twist, short, elbow)
        cost, pole, f, bend, twist, short, elbow = best
        if mode[0] == "lift":
            rel_top = forearm_rel(p, f, torso, pole)
        prev, f_prev = pole, f
        rows.append((t, p, f, pole, torso, head, cost, bend, twist, short))
    return rows


def holster_tracks(place, end):
    """The draw as tracks, one key a frame. The elbow's pole is written square to the arm, so the
    game's solver can't flip it."""
    plan = HOLSTER_PLANS[place.id]
    rows = holster_frames(place, plan, end)
    tracks = {c: [] for c in ("hand_r", "blade", "edge", "elbow_r", "torso", "head")}
    worst = [0.0, 0.0, 0.0]
    for t, p, f, pole, torso, head, _, bend, twist, short in rows:
        _, _, _, elbow = solve_arm(p, f, torso, pole)
        r = rotation_of(torso)
        shoulder = CHEST + r @ (UPPER - CHEST)
        d = unit(p - shoulder)
        side = unit((elbow - shoulder) - d * d.dot(elbow - shoulder))
        for control, value in (("hand_r", p), ("blade", f[:, 0]), ("edge", f[:, 1]), ("elbow_r", side), ("torso", torso), ("head", head)):
            tracks[control].append((t, value))
        worst = [max(worst[0], bend), max(worst[1], twist), max(worst[2], short * 1000)]
    save(f"draw_{place.id}", rows[-1][0], False, tracks)
    print(f"  draw_{place.id:14s} wrist {worst[0]:3.0f} twist {worst[1]:3.0f} short {worst[2]:3.0f}")
    return tracks, rows[-1][0], plan["times"][0]


# The clip's order, and when the hand has it, in seconds.
def grab_time(place):
    return 0.36 if place.bone == "chest" else 0.33


def stow_tracks(tracks, length, grab_at):
    """The way it came out, frame by frame and backwards, without the flourish: it lets go a fifth of
    the way later. Working it out again from the end finds another way of bending the arm."""
    duration = length * 1.02
    release = duration * 0.62

    def back(t):
        if t >= grab_at:
            return (1.0 - (t - grab_at) / (length - grab_at)) * release
        return release + (1.0 - t / grab_at) * (duration - release)

    return {c: sorted(((back(t), v) for t, v in track), key=lambda row: row[0]) for c, track in tracks.items()}, duration, release


def ron_float(v):
    return f"{v:.4f}"


def write_place(place):
    if place.style == "sling":
        return write_sling(place)
    if place.family == "Slung":
        end = end_from_clip("musket_hold")
    elif place.family == "Pistol":
        end = end_from_clip("pistol_hold")
    elif place.family == "Back":
        end = end_from_clip("heavy_carry")
    else:
        end = {"p": LOOSE_HAND["p"], "f": LOOSE_HAND["f"], "elbow": LOOSE_HAND["elbow"]}
    if place.id in HOLSTER_PLANS:
        tracks, length, grab = holster_tracks(place, end)
    else:
        keys = draw_keys(place, end)
        grab, length = grab_time(place), keys[-1].t
        tracks = write_clip(f"draw_{place.id}", keys)
    back, duration, release = stow_tracks(tracks, length, grab)
    save(f"stow_{place.id}", duration, False, back)
    return {"grab": grab, "release": release}


# ---- the long gun, from one hand to the other

def gun_clips():
    import importlib.util
    spec = importlib.util.spec_from_file_location("gen_gun_clips", os.path.join(HERE, "gen_gun_clips.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


SLING_GRAB = 0.35
SLING_CATCH = 0.75
SLING_PORT = 1.05
SLING_LENGTH = 1.1
# How long the left hand takes to reach the barrel.
SLING_REACH_LEFT = 0.3
# How the draw goes, found by a search that measures every frame of it: both wrists, the arm
# against the trunk, neck and head, and the gun against the body.
SLING_PLAN = {
    "c": 0.04,
    "alpha": 363.362,
    "gamma": 62.585,
    "sigma": 1.0,
    "yaw_g": -11.835,
    "pole_g": (0.638, 0.753, 0.159),
    "reach_mid": (0.198, 1.015, 0.458),
    "pole_mid": (0.279, -0.935, 0.217),
    "carry_mid": (0.371, 1.17, -0.441),
    "pole_carry": (0.201, -0.894, -0.4),
    "w_c": (0.181, 1.354, -0.22),
    "pole_c": (0.317, -0.377, -0.87),
    "flex_c": (-2.745, -20.221, 38.449),
    "yaw_c": -26.112,
    "pole_l": (-0.021, -0.881, -0.473),
}
MIRROR = np.diag([-1.0, 1.0, 1.0])
HANG_POLE = (-0.4, -0.3, 0.7)


def axis_turn(axis, degrees):
    a = unit(axis)
    k = np.array([[0, -a[2], a[1]], [a[2], 0, -a[0]], [-a[1], a[0], 0]])
    r = math.radians(degrees)
    return np.eye(3) + math.sin(r) * k + (1 - math.cos(r)) * k @ k


def mirrored_torso(torso):
    return np.array([torso[0], -torso[1], -torso[2]], dtype=float)


def left_forearm(wrist, torso, pole):
    """The left forearm's turn from rest with the wrist here, by the right arm's solver in a mirror,
    with how short the arm is and the elbow."""
    point = MIRROR @ np.array(wrist, dtype=float) + (WEAPON - HAND)
    fore, axis, short, elbow = solve_arm(point, HAND_REST, mirrored_torso(torso), MIRROR @ np.array(pole, dtype=float))
    return MIRROR @ fore @ MIRROR, short, MIRROR @ elbow


def left_strain(wrist, turn, torso, pole):
    turn_r = MIRROR @ turn @ MIRROR
    held = turn_r @ HAND_REST
    point = MIRROR @ np.array(wrist, dtype=float) + turn_r @ (WEAPON - HAND)
    fore, axis, short, elbow = solve_arm(point, held, mirrored_torso(torso), MIRROR @ np.array(pole, dtype=float))
    bend, twist = strain(held, fore, axis)
    return bend, twist, short, MIRROR @ elbow


def right_strain(tracks, torso, pole):
    held = frame(tracks["blade"], tracks["edge"])
    fore, axis, short, elbow = solve_arm(tracks["hand_r"], held, np.array(torso, dtype=float), np.array(pole, dtype=float))
    bend, twist = strain(held, fore, axis)
    return bend, twist, short, elbow


def bezier(a, b, c, u):
    return a * (1 - u) ** 2 + b * 2 * u * (1 - u) + c * u * u


def right_forearm(wrist, torso, pole):
    """The right forearm's turn from rest with the wrist here, how short the arm is and the elbow."""
    fore, _, short, elbow = solve_arm(np.array(wrist, dtype=float) + (WEAPON - HAND), HAND_REST, np.array(torso, dtype=float), np.array(pole, dtype=float))
    return fore, short, elbow


def right_controls(wrist, turn):
    """`hand_r`, `blade` and `edge` for the hand with its wrist here and turned this way."""
    held = turn @ HAND_REST
    return {"hand_r": np.array(wrist, dtype=float) + turn @ (WEAPON - HAND), "blade": held[:, 0], "edge": held[:, 1]}


def gun_in_hand(gg, controls):
    """The gun as the game holds it in the right hand: what `send_to_hand` puts there."""
    held = frame(controls["blade"], controls["edge"])
    joint = held[:, [2, 0, 1]]
    to_body = joint @ gg.MUSKET.held
    return gg.Pose(gg.MUSKET, to_body[:, 0], up=to_body[:, 1], grip_at=controls["hand_r"])


def sling_timeline(plan, gg):
    """Every frame of the draw: the controls, the gun and the left hand's turn. The right hand
    reaches behind its hip for the stock and swings the gun forward over the left shoulder without
    it moving in the fist, as the game holds it as it is there; the left hand catches the barrel in
    front, and the right one settles on the wrist of the stock as the game slides the gun into its
    proper hold. Each hand's turn is kept relative to its forearm, so a wrist bends only as far as
    it does at the ends of each part."""
    p = {k: np.array(v, dtype=float) if np.ndim(v) else float(v) for k, v in plan.items()}
    end = read_first_key("musket_hold")
    port = gg.musket_hold()
    torso_g = np.array((4.0, p["yaw_g"], -3.0))
    torso_c = np.array((4.0, p["yaw_c"], 0.0))
    torso_keys = [(0.0, TORSO_REST), (SLING_GRAB, tuple(torso_g)), (SLING_CATCH, tuple(torso_c)), (SLING_PORT, tuple(end["torso"])), (SLING_LENGTH, tuple(end["torso"]))]
    head_keys = [(0.0, HEAD_REST), (SLING_GRAB, (6.0, -18.0, 0.0)), (SLING_CATCH, (4.0, 10.0, 0.0)), (SLING_PORT, tuple(end["head"])), (SLING_LENGTH, tuple(end["head"]))]

    # The gun on the back as the torso has it at the grab, and the fist round its stock.
    rest = gg.Pose(gg.MUSKET, SLING_LONG, up=SLING_TOP, grip_at=SLING_AT)
    back_turn = chest_turned(rest.file_to_body, torso_g)
    back = gg.Pose(gg.MUSKET, back_turn[:, 0], up=back_turn[:, 1], grip_at=chest_moved(SLING_AT, torso_g))
    stock = back_turn[:, 0] * p["sigma"]
    blade = axis_turn(back_turn[:, 2], p["gamma"]) @ stock
    edge = axis_turn(blade, p["alpha"]) @ back_turn[:, 1]
    held_g = frame(blade, edge)
    turn_g = held_g @ HAND_REST.T
    point_g = back.point(p["c"], -0.02, 0.0)
    w_g = point_g - turn_g @ (WEAPON - HAND)

    def flex_r(wrist, turn, torso, pole):
        return right_forearm(wrist, torso, pole)[0].T @ turn

    def flex_l(wrist, turn, torso, pole):
        return left_forearm(wrist, torso, pole)[0].T @ turn

    loose_turn = LOOSE_HAND["f"] @ HAND_REST.T
    loose_w = LOOSE_HAND["p"] - loose_turn @ (WEAPON - HAND)
    flex_loose = flex_r(loose_w, loose_turn, TORSO_REST, LOOSE_HAND["elbow"])
    flex_g = flex_r(w_g, turn_g, torso_g, p["pole_g"])
    a, b, c = p["flex_c"]
    flex_c = axis_turn(HAND_REST[:, 0], a) @ axis_turn(HAND_REST[:, 1], b) @ axis_turn(HAND_REST[:, 2], c)
    w_c = p["w_c"]
    turn_c = right_forearm(w_c, torso_c, p["pole_c"])[0] @ flex_c
    end_turn_r = frame(end["blade"], end["edge"]) @ HAND_REST.T
    end_w = end["hand_r"] - end_turn_r @ (WEAPON - HAND)
    flex_end = flex_r(end_w, end_turn_r, end["torso"], end["elbow_r"])

    hang = gg.hanging()
    hang_turn = gg.matrix_of(hang["hand_l_rot"])
    flex_hang = flex_l(hang["hand_l"], hang_turn, TORSO_REST, HANG_POLE)
    end_turn_l = gg.matrix_of(end["hand_l_rot"])
    on_port = (port.file_to_body.T @ (end["hand_l"] - port.hand), port.file_to_body.T @ end_turn_l)

    frames = []
    times = sorted(set([round(i * STEP, 4) for i in range(int(round(SLING_LENGTH / STEP)) + 1)] + [SLING_GRAB, SLING_CATCH, SLING_PORT]))
    for t in times:
        torso = keyed_value(t, torso_keys)
        head = keyed_value(t, head_keys)
        if t <= SLING_GRAB:
            u = smooth(t / SLING_GRAB)
            wrist = bezier(loose_w, p["reach_mid"], w_g, u)
            pole_r = keyed_value(t, [(0.0, tuple(LOOSE_HAND["elbow"])), (SLING_GRAB * 0.5, tuple(p["pole_mid"])), (SLING_GRAB, tuple(p["pole_g"]))])
            turn = right_forearm(wrist, torso, pole_r)[0] @ slerp(flex_loose, flex_g, u)
            gun = back
        elif t <= SLING_CATCH:
            u = smooth((t - SLING_GRAB) / (SLING_CATCH - SLING_GRAB))
            wrist = bezier(w_g, p["carry_mid"], w_c, u)
            pole_r = keyed_value(t, [(SLING_GRAB, tuple(p["pole_g"])), ((SLING_GRAB + SLING_CATCH) * 0.5, tuple(p["pole_carry"])), (SLING_CATCH, tuple(p["pole_c"]))])
            turn = right_forearm(wrist, torso, pole_r)[0] @ slerp(flex_g, flex_c, u)
            moved = turn @ turn_g.T
            gt = moved @ back.file_to_body
            gun = gg.Pose(gg.MUSKET, gt[:, 0], up=gt[:, 1], grip_at=moved @ (back.hand - w_g) + wrist)
        else:
            u = smooth(min(1.0, (t - SLING_CATCH) / (SLING_PORT - SLING_CATCH)))
            wrist = w_c + (end_w - w_c) * u
            pole_r = keyed_value(t, [(SLING_CATCH, tuple(p["pole_c"])), (SLING_PORT, tuple(end["elbow_r"])), (SLING_LENGTH, tuple(end["elbow_r"]))])
            turn = right_forearm(wrist, torso, pole_r)[0] @ slerp(flex_c, flex_end, u)
            gun = None
        right = right_controls(wrist, turn)
        if gun is None:
            gun = gun_in_hand(gg, right)
        if t >= SLING_PORT:
            right = {"hand_r": end["hand_r"], "blade": end["blade"], "edge": end["edge"]}
        right["elbow_r"] = np.array(pole_r, dtype=float)
        # The left hand hangs, then reaches for the barrel where it will hold it, on the gun as it comes.
        reach = smooth(min(1.0, max(0.0, (t - (SLING_CATCH - SLING_REACH_LEFT)) / SLING_REACH_LEFT)))
        on_w = gun.hand + gun.file_to_body @ on_port[0]
        on_turn = gun.file_to_body @ on_port[1]
        pole_l = keyed_value(t, [(0.0, HANG_POLE), (SLING_CATCH - SLING_REACH_LEFT, HANG_POLE), (SLING_CATCH, tuple(p["pole_l"])), (SLING_PORT, tuple(end["elbow_l"])), (SLING_LENGTH, tuple(end["elbow_l"]))])
        wrist_l = np.array(hang["hand_l"]) + (on_w - np.array(hang["hand_l"])) * reach
        fore_l = left_forearm(wrist_l, torso, pole_l)[0]
        turn_l = fore_l @ slerp(flex_hang, flex_l(on_w, on_turn, torso, pole_l), reach)
        curl = keyed_value(t, [(0.0, 0.25), (SLING_CATCH - 0.08, 0.3), (SLING_CATCH, 0.85), (SLING_LENGTH, float(end["curl_l"][0]))])
        values = {**right, "hand_l": wrist_l, "hand_l_rot": gg.euler_of(turn_l), "curl_l": curl, "elbow_l": np.array(pole_l, dtype=float), "torso": torso, "head": head}
        frames.append((t, values, gun, turn_l))
    # How far the gun slides in the fist when the game puts it in its proper hold.
    proper = gun_in_hand(gg, right_controls(w_c, turn_c))
    caught = [g for t, _, g, _ in frames if abs(t - SLING_CATCH) < 1e-6][0]
    slide = max(np.linalg.norm(proper.point(x) - caught.point(x)) for x in (-0.4, 1.0))
    return frames, slide


def write_sling(place):
    gg = gun_clips()
    frames, _ = sling_timeline(SLING_PLAN, gg)
    keys = [(t, values) for t, values, _, _ in frames]
    gg.write_clip(f"draw_{place.id}", SLING_LENGTH, False, False, keys)
    # Putting it back is the same, backwards.
    gg.write_clip(f"stow_{place.id}", SLING_LENGTH, False, False, sorted(((SLING_LENGTH - t, v) for t, v in keys), key=lambda row: row[0]))
    return {"grab": SLING_GRAB, "release": SLING_LENGTH - SLING_GRAB, "pass": ("weapon.R", SLING_CATCH, SLING_LENGTH - SLING_CATCH)}


def elbow_between(shoulder, wrist, pole):
    to = wrist - shoulder
    distance = min(max(np.linalg.norm(to), abs(UPPER_LEN - FORE_LEN) + 1e-3), (UPPER_LEN + FORE_LEN) * 0.999)
    d = unit(to)
    along = (UPPER_LEN**2 - FORE_LEN**2 + distance**2) / (2 * distance)
    height = math.sqrt(max(0.0, UPPER_LEN**2 - along**2))
    side = np.array(pole, dtype=float) - d * d.dot(pole)
    return shoulder + d * along + unit(side) * height


def keyed_value(t, keys):
    for (a, x), (b, y) in zip(keys, keys[1:]):
        if t <= b + 1e-9:
            u = smooth(min(1.0, max(0.0, (t - a) / (b - a))))
            return np.array(x, dtype=float) + (np.array(y, dtype=float) - np.array(x, dtype=float)) * u
    return np.array(keys[-1][1], dtype=float)


# ---- the book, in the left hand, which has its own controls

def write_book_place(id, point, turn):
    """Reaches behind with the free hand, and the book comes round to the front to be read."""
    hold = read_first_key("book_hold")
    hanging = np.array([-0.31, 0.93, -0.03])
    rest_turn = np.array([0.0, 0.0, 0.0])
    elbow = np.array([-0.7, 0.1, 0.7])
    times = [0.0, 0.14, 0.27, 0.36, 0.5, 0.72]
    p = [hanging, (hanging + point) * 0.5 + (-0.08, 0.05, 0.02), np.array(point) + (-0.03, 0.05, 0.03), np.array(point), np.array([-0.4, 1.2, 0.02]), hold["hand_l"]]
    r = [rest_turn, np.array(turn) * 0.5, np.array(turn), np.array(turn), (np.array(turn) + hold["hand_l_rot"]) * 0.5, hold["hand_l_rot"]]
    e = [elbow, elbow, elbow, elbow, (elbow + hold["elbow_l"]) * 0.5, hold["elbow_l"]]
    torso = [TORSO_REST, (4, 10, 0), (6, 18, 2), (6, 20, 2), (3, 8, 1), hold.get("torso", np.array(TORSO_REST))]
    head = [HEAD_REST, (0, 8, 0), (4, 16, 0), (4, 18, 0), (6, 4, 0), hold.get("head", np.array(HEAD_REST))]
    keep_right = {"hand_r": hold["hand_r"], "blade": hold["blade"], "edge": hold["edge"], "elbow_r": hold["elbow_r"]}

    def write(name, tt, order, grip):
        tracks = {c: [] for c in ("hand_l", "hand_l_rot", "elbow_l", "curl_l", "torso", "head")}
        tracks.update({c: [] for c in keep_right})
        duration = tt[-1]
        samples = sorted(set([round(x, 4) for x in tt] + [round(x * STEP, 4) for x in range(int(duration / STEP) + 1)]))
        for t in samples:
            i = min(max(j for j, x in enumerate(tt) if x <= t + 1e-6), len(tt) - 2)
            u = smooth(min(1.0, (t - tt[i]) / (tt[i + 1] - tt[i])))
            a, b = order[i], order[i + 1]
            tracks["hand_l"].append((t, p[a] + (p[b] - p[a]) * u))
            tracks["hand_l_rot"].append((t, r[a] + (r[b] - r[a]) * u))
            tracks["elbow_l"].append((t, e[a] + (e[b] - e[a]) * u))
            tracks["curl_l"].append((t, np.array([grip(t), 0.0, 0.0])))
            tracks["torso"].append((t, np.array(torso[a], dtype=float) + (np.array(torso[b], dtype=float) - np.array(torso[a], dtype=float)) * u))
            tracks["head"].append((t, np.array(head[a], dtype=float) + (np.array(head[b], dtype=float) - np.array(head[a], dtype=float)) * u))
            # The other arm stays out of it: the game lets the clips underneath have it.
            for control, value in keep_right.items():
                tracks[control].append((t, value))
        save(name, duration, False, tracks)

    write(f"draw_{id}", times, list(range(6)), lambda t: smooth(min(1.0, max(0.0, (t - 0.26) / 0.1))))
    back = [0.0, 0.2, 0.42, 0.48, 0.6, 0.76]
    write(f"stow_{id}", back, list(reversed(range(6))), lambda t: 1.0 - smooth(min(1.0, max(0.0, (t - 0.41) / 0.1))))
    return {"grab": 0.36, "release": back[2]}


# ---- holding something out and eating it

def palm_up(direction):
    """The hand's frame with the fingers this way and the palm to the sky. The joint's Z runs back
    along the hand and its X out of the back of the hand."""
    d = unit(direction)
    # Up and a little inwards, as the forearm turns no further.
    back = np.array([0.5, -0.86, 0.0])
    back = back - d * back.dot(d)
    back = unit(back)
    edge = -d
    blade = np.cross(edge, back)
    return np.column_stack([blade, edge, np.cross(blade, edge)])


def write_offer():
    """Handing something over: the upper arm down and a little forward, the forearm out level at the
    height of the ribs, the palm up with the thing on it, and a slow sway."""
    length = 3.0
    torso = (3, -4, 0)
    point = np.array([0.215, 1.2, -0.3])
    elbow = (0.4, -1.0, 0.45)
    f = palm_up((-0.08, 0.08, -1.0))
    for _ in range(3):
        fore, axis, short, _ = solve_arm(point, f, torso, elbow)
        f = palm_up(axis + np.array([0.0, 0.08, 0.0]))
    wrist, twist = strain(f, fore, axis)
    print(f"offer wrist {wrist:.0f} twist {twist:.0f} short {short*1000:.0f}")
    keys = []
    for i in range(7):
        t = length * i / 6
        wave = math.sin(2 * math.pi * t / length)
        slow = math.cos(2 * math.pi * t / length)
        p = point + (0.004 * slow, 0.008 * wave, -0.006 * wave)
        keys.append(Key(t, p, f, elbow, (3 + 0.8 * wave, -4 + 0.7 * slow, 0), (8 + 1.0 * slow, -4, 0)))
    write_clip("item_offer", keys, looping=True)


EAT_BITE = 0.62


def write_eat():
    """To the mouth, the head coming to meet it, a bite, and back down while chewing."""
    start = LOOSE_HAND
    # The thing sits past the fist, along the forearm, and that's what reaches the mouth.
    mouth = np.array([0.055, 1.54, -0.085])
    torso = (7, -6, 0)
    tries = [best_hand(mouth, blade, torso, (0.8, -0.5, 0.2), 0.2) for blade in ((-0.45, 0.2, -0.87), (0.5, 0.4, -0.75), (0.3, 0.3, 0.9), (0.6, 0.6, 0.5))]
    _, f, elbow, wrist, twist, short = min(tries, key=lambda t: t[0])
    print(f"eat wrist {wrist:.0f} twist {twist:.0f} short {short*1000:.0f}")
    half = elbow * 0.6 + start["elbow"] * 0.4
    keys = [
        Key(0.0, start["p"], start["f"], start["elbow"], TORSO_REST, HEAD_REST),
        Key(0.22, (0.31, 1.24, -0.2), None, half, (3, -3, 0), (2, 0, 0)),
        Key(0.46, mouth + (0.01, -0.02, -0.03), None, elbow, (5, -5, 0), (5, 3, 0)),
        Key(EAT_BITE, mouth, f, elbow, torso, (9, 5, 0)),
        Key(0.76, mouth + (0.02, -0.05, -0.04), f, elbow, (4, -5, 0), (-2, 3, 0)),
        Key(0.95, (0.31, 1.22, -0.2), None, half, (3, -3, 0), (3, 1, 0)),
        Key(1.2, start["p"], start["f"], start["elbow"], TORSO_REST, (1, 0, 0)),
    ]
    write_clip("item_eat", keys)


# ---- the list

def place_lay(place):
    if place.family == "Pocket":
        return None
    if place.family == "Pistol":
        return place.point, GUN_LAYS[place.id][0], GUN_LAYS[place.id][1]
    if place.lay[1] is not None:
        return place.lay
    return place.point, place.top[0], place.top[1]


def lies_row(family, lay):
    if lay is None:
        return ""
    at, long, across = (tuple(round(float(x), 4) for x in v) for v in lay)
    return f", lies: Some((at: {at}, long: {long}, across: {across}))"



def main():
    only = set(sys.argv[1:])
    entries = []
    for place in PLACES:
        if only and place.id not in only:
            continue
        entries.append((place, write_place(place)))
    print("wrote", len(entries), "places")
    books = []
    for id, point, turn, lay in BOOK_PLACES:
        books.append((id, write_book_place(id, point, turn), lay))
    write_offer()
    write_eat()
    if only:
        return
    rows = ["// Written by tools/gen_stow_clips.py. Where things are carried, in the order they're taken.", "(", f"    eat_bite: {EAT_BITE},", "    places: ["]
    for place, times in entries:
        passing = f', pass: Some((hand: "{times["pass"][0]}", draw: {times["pass"][1]:.3f}, stow: {times["pass"][2]:.3f}))' if "pass" in times else ""
        rows.append(f'        (id: "{place.id}", family: {place.family}, bone: "{place.bone}", hand: "{place.hand}", grab: {times["grab"]:.3f}, release: {times["release"]:.3f}{lies_row(place.family, place_lay(place))}{passing}),')
    for id, times, lay in books:
        rows.append(f'        (id: "{id}", family: Book, bone: "hips", hand: "hand.L", grab: {times["grab"]:.3f}, release: {times["release"]:.3f}{lies_row("Book", lay)}),')
    rows += ["    ],", ")", ""]
    with open(os.path.join(MODEL, "chartest_stow.ron"), "w", encoding="utf8", newline="\n") as out:
        out.write("\n".join(rows))
    for row in rows[3:-2]:
        print(row.strip()[:150])


if __name__ == "__main__":
    main()
