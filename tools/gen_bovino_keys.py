"""The cattle's clips, written as poses for `assets/models/animals/Bovino_keys.ron`.

The rig has straight legs and no shoulder blade, so a leg can't reach the ground anywhere but
straight under its hip. The clips solve every leg here, ahead of time: each leg is a two-bone
chain that only bends fore and aft (the knee forward on a foreleg, the hock back on a hind one),
the hoof is planted on a line and swung on an arc, and the body (the `Root`) sinks a little while
it walks so the stride can be long enough. Nothing is solved in the game: it only plays these.

Model space: +X is the animal's left, +Y up, +Z ahead. Clips are keyed every frame at 30 Hz.

Run: python -I tools/gen_bovino_keys.py   (prints the reach and the stride speeds)
"""

import json
import math
import pathlib
import struct

import numpy as np
from scipy.spatial.transform import Rotation as R

ROOT = pathlib.Path(__file__).resolve().parent.parent
MODEL = ROOT / "assets/models/animals/Bovino.glb"
OUT = ROOT / "assets/models/animals/Bovino_keys.ron"
FPS = 30

LEGS = ["ForeUpper.L", "ForeUpper.R", "HindUpper.L", "HindUpper.R"]
CHAIN = {u: (u, u.replace("Upper", "Lower"), u.replace("Upper", "Hoof")) for u in LEGS}

# Lying: how far the body sinks.
LIE_DROP = 0.48


def load_rig():
    data = MODEL.read_bytes()
    n = struct.unpack("<I", data[12:16])[0]
    gltf = json.loads(data[20 : 20 + n])
    nodes = gltf["nodes"]
    parent = {}
    for i, node in enumerate(nodes):
        for c in node.get("children", []):
            parent[c] = i
    joints = {}
    for i, node in enumerate(nodes):
        if "mesh" in node:
            continue
        joints[node["name"]] = i
    local = {}
    for name, i in joints.items():
        node = nodes[i]
        local[name] = (
            np.array(node.get("translation", [0, 0, 0]), float),
            R.from_quat(node.get("rotation", [0, 0, 0, 1])),
        )
    parents = {name: (nodes[parent[i]]["name"] if i in parent else None) for name, i in joints.items()}
    return local, parents


LOCAL, PARENTS = load_rig()
ORDER = []


def _order(name):
    if name in ORDER:
        return
    if PARENTS[name] is not None:
        _order(PARENTS[name])
    ORDER.append(name)


for _name in LOCAL:
    _order(_name)


def fk(rotations=None, shift=(0, 0, 0)):
    """Model-space positions and rotations. `rotations` sets some joints' global rotation."""
    rotations = rotations or {}
    pos, rot = {}, {}
    for name in ORDER:
        t, r = LOCAL[name]
        p = PARENTS[name]
        if p is None:
            pos[name], rot[name] = t.copy(), r
            continue
        if name == "Root":
            t = t + np.array(shift)
        pos[name] = pos[p] + rot[p].apply(t)
        rot[name] = rotations.get(name, rot[p] * r)
    return pos, rot


REST_POS, REST_ROT = fk()


def bone(name):
    return REST_ROT[name].apply([0, 1, 0])


def arc(a, b):
    """The shortest rotation taking direction a to b."""
    a = np.asarray(a, float) / np.linalg.norm(a)
    b = np.asarray(b, float) / np.linalg.norm(b)
    axis = np.cross(a, b)
    s = np.linalg.norm(axis)
    c = float(np.dot(a, b))
    if s < 1e-9:
        return R.identity()
    return R.from_rotvec(axis / s * math.atan2(s, c))


def aimed(name, aim):
    """The global rotation a key with `aim` gives (as keys.rs does)."""
    return arc(bone(name), aim) * REST_ROT[name]


def rot_x(deg):
    return R.from_euler("x", deg, degrees=True)


def turned(name, x=0.0, y=0.0, z=0.0):
    """Global rotation of `turn: (x, y, z)` in keys.rs (YXZ euler, in the model's space)."""
    return R.from_euler("YXZ", [y, x, z], degrees=True) * REST_ROT[name]


def length(a, b):
    return float(np.linalg.norm(REST_POS[a] - REST_POS[b]))


LEG = {}
for upper, (u, lo, h) in CHAIN.items():
    LEG[upper] = dict(
        l1=length(u, lo),
        l2=length(lo, h),
        foot=REST_POS[h] * np.array([1, 0, 1]),  # on the ground under the hoof joint
        tip=np.array([0.0, -REST_POS[h][1], 0.0]),  # hoof joint to the ground, at rest
        pole=np.array([0, 0, 1.0]) if upper.startswith("Fore") else np.array([0, 0, -1.0]),
    )


class Pose:
    """A pose: some joints aimed or turned, the root shifted, and legs solved to their feet."""

    def __init__(self):
        self.keys = {}  # joint -> dict for the RON
        self.global_rot = {}
        self.shift = np.zeros(3)

    def turn(self, name, x=0.0, y=0.0, z=0.0):
        self.keys[name] = {"turn": (x, y, z)}
        self.global_rot[name] = turned(name, x, y, z)
        return self

    def aim(self, name, aim):
        aim = np.asarray(aim, float)
        aim = aim / np.linalg.norm(aim)
        self.keys[name] = {"aim": tuple(aim)}
        self.global_rot[name] = aimed(name, aim)
        return self

    def sink(self, y, z=0.0):
        self.shift = np.array([0.0, y, z])
        return self

    def positions(self):
        return fk(self.global_rot, self.shift)

    def leg(self, upper, foot, curl=0.0, bend_up=None):
        """Puts a leg's hoof on `foot` (where it touches the ground, model space), the hoof
        turned back by `curl` degrees. Returns how far short of the target the leg stays."""
        u, lo, h = CHAIN[upper]
        leg = LEG[upper]
        pos, _ = self.positions()
        hip = pos[u]
        hoof_dir = rot_x(curl).apply(bone(h))
        ankle = np.asarray(foot, float) - rot_x(curl).apply(leg["tip"])
        reach = ankle - hip
        d = float(np.linalg.norm(reach))
        longest = (leg["l1"] + leg["l2"]) * 0.999
        along = reach / max(d, 1e-6)
        short = max(d - longest, 0.0)
        d = min(max(d, 0.05), longest)
        pole = leg["pole"] if bend_up is None else np.asarray(bend_up, float)
        pole = pole - along * np.dot(pole, along)
        pole = pole / max(np.linalg.norm(pole), 1e-6)
        a = (leg["l1"] ** 2 - leg["l2"] ** 2 + d * d) / (2 * d)
        hh = math.sqrt(max(leg["l1"] ** 2 - a * a, 0.0))
        knee = hip + along * a + pole * hh
        end = hip + along * d
        self.aim(u, knee - hip)
        self.aim(lo, end - knee)
        self.aim(h, hoof_dir)
        return short

    def joint_at(self, name):
        return self.positions()[0][name]

    def globals_of(self):
        """Every keyed joint's global rotation."""
        return dict(self.global_rot)

    @staticmethod
    def from_globals(globals_, shift):
        """A pose that gives these joints these global rotations, written as turns."""
        p = Pose()
        for name, g in globals_.items():
            y, x, z = (g * REST_ROT[name].inv()).as_euler("YXZ", degrees=True)
            p.keys[name] = {"turn": (x, y, z)}
            p.global_rot[name] = g
        p.shift = np.asarray(shift, float)
        return p

    def ron(self):
        fields = {}
        for name, key in self.keys.items():
            if "aim" in key:
                a = key["aim"]
                fields[name] = [f"aim: ({a[0]:.4f}, {a[1]:.4f}, {a[2]:.4f})"]
            else:
                t = key["turn"]
                fields[name] = [f"turn: ({t[0]:.2f}, {t[1]:.2f}, {t[2]:.2f})"]
        if np.any(np.abs(self.shift) > 1e-5):
            s = self.shift
            fields.setdefault("Root", []).append(f"shift: ({s[0]:.4f}, {s[1]:.4f}, {s[2]:.4f})")
        return "{ " + ", ".join(f'"{name}": ({", ".join(f)})' for name, f in fields.items()) + " }"


def smooth(t):
    t = min(max(t, 0.0), 1.0)
    return t * t * (3 - 2 * t)


def lerp(a, b, t):
    return a + (b - a) * t


def head(p, neck_pitch=0.0, head_pitch=0.0, yaw=0.0, jaw=0.0):
    """Neck and head pitched down by these degrees from rest (negative is up), turned by yaw."""
    p.turn("Neck", neck_pitch, yaw * 0.6, 0.0)
    p.turn("Head", neck_pitch + head_pitch, yaw, 0.0)
    p.turn("Jaw", neck_pitch + head_pitch + jaw, yaw, 0.0)
    return p


def tail(p, swing=0.0, lift=0.0):
    p.turn("Tail.01", -lift, 0.0, swing)
    p.turn("Tail.02", -lift * 0.4, 0.0, swing * 1.6)
    p.turn("Tail.03", -lift * 0.15, 0.0, swing * 2.2)
    return p


def ears(p, back=0.0, flick=0.0):
    p.turn("Ear.L", 0.0, back, flick)
    p.turn("Ear.R", 0.0, -back, -flick)
    return p


def muzzle(p):
    pos, rot = p.positions()
    # The bottom of the mouth at rest, in the head's frame.
    rest = np.array([0.0, 1.0, 1.48])
    offset = REST_ROT["Head"].inv().apply(rest - REST_POS["Head"])
    return pos["Head"] + rot["Head"].apply(offset)


# ---------------------------------------------------------------------------------------- gaits


def gait(name, seconds, duty, stride, lift, order, sink, bob, curl, pitch_rock=0.0, head_fn=None, tail_fn=None):
    """A looping gait: each leg plants for `duty` of the cycle, sweeping `stride` metres back."""
    frames = round(seconds * FPS)
    poses, worst = [], 0.0
    for f in range(frames):
        t = f / frames
        p = Pose()
        p.sink(-(sink + bob * (0.5 + 0.5 * math.cos(4 * math.pi * t))))
        if pitch_rock:
            p.turn("Body", pitch_rock * math.sin(2 * math.pi * t), 0.0, 0.0)
        (head_fn or (lambda p, t: head(p, 3.0 * math.sin(4 * math.pi * t + 1.0))))(p, t)
        (tail_fn or (lambda p, t: tail(p, 8.0 * math.sin(2 * math.pi * t), 0.0)))(p, t)
        for upper, offset in zip(LEGS, order):
            ph = (t + offset) % 1.0
            foot = LEG[upper]["foot"].copy()
            if ph < duty:
                u = ph / duty
                foot[2] += stride * (0.5 - u)
                # The heel lifts as the leg trails, rolling onto the toe.
                c = curl * 0.35 * smooth((u - 0.75) / 0.25)
            else:
                u = (ph - duty) / (1 - duty)
                foot[2] += stride * (smooth(u) - 0.5)
                foot[1] += lift * math.sin(math.pi * u) ** 1.2
                # Curled back while it swings, flat again to land.
                c = curl * (0.35 + 0.65 * math.sin(math.pi * min(u * 1.3, 1.0))) * (1 - smooth((u - 0.7) / 0.3))
            worst = max(worst, p.leg(upper, foot, c))
        poses.append(p)
    print(f"{name}: {stride / (duty * seconds):.2f} m/s, legs short by {worst * 100:.1f} cm at most")
    return poses


def standing(p, sink=0.0, fore=0.0, hind=0.0, spread=0.0):
    """Every hoof on its spot (fore and hind moved ahead by these), the body sunk by `sink`."""
    p.sink(-sink)
    short = 0.0
    for upper in LEGS:
        foot = LEG[upper]["foot"].copy()
        foot[2] += fore if upper.startswith("Fore") else hind
        foot[0] *= 1 + spread
        short = max(short, p.leg(upper, foot))
    return short


# --------------------------------------------------------------------------------------- clips

CLIPS = {}


def clip(name, poses, looping, events=()):
    CLIPS[name] = (poses, looping, list(events))


def build():
    # Walk: lateral sequence, hind left, fore left, hind right, fore right.
    walk_order = [0.25, 0.75, 0.0, 0.5]
    clip("walk", gait("walk", 1.3, 0.66, 0.7, 0.11, walk_order, 0.08, 0.012, 55.0), True)

    # Run: a gallop, hinds landing close together, then the fores.
    run_order = [0.45, 0.55, 0.0, 0.1]

    def run_head(p, t):
        head(p, -4.0 + 6.0 * math.sin(2 * math.pi * t), 4.0)

    def run_tail(p, t):
        tail(p, 10.0 * math.sin(2 * math.pi * t), 35.0)

    clip("run", gait("run", 0.62, 0.42, 1.0, 0.24, run_order, 0.12, 0.05, 95.0, 5.0, run_head, run_tail), True)

    def charge_head(p, t):
        head(p, 32.0 + 3.0 * math.sin(2 * math.pi * t), -22.0)
        ears(p, 35.0)

    clip("charge", gait("charge", 0.62, 0.42, 1.0, 0.24, run_order, 0.14, 0.05, 95.0, 5.0, charge_head, run_tail), True)

    # Stand: breathing, a tail that sways, an ear now and then, the head looking about.
    frames = 6 * FPS
    stand = []
    for f in range(frames):
        t = f / frames
        p = Pose()
        head(p, 2.0 * math.sin(2 * math.pi * t), 1.5 * math.sin(4 * math.pi * t), 7.0 * math.sin(2 * math.pi * t + 0.7))
        tail(p, 7.0 * math.sin(6 * math.pi * t), 0.0)
        flick = 25.0 * math.exp(-(((t - 0.62) * 30) ** 2))
        ears(p, 0.0, flick)
        standing(p, sink=0.006 * (0.5 + 0.5 * math.sin(2 * math.pi * 3 * t)))
        stand.append(p)
    clip("stand", stand, True)

    # Graze: the head down in the grass, tearing a bite now and then and swinging to the next.
    frames = 4 * FPS
    graze, worst_muzzle = [], []
    for f in range(frames):
        t = f / frames
        p = Pose()
        # The neck is short: the chest goes down between the forelegs (found by a search for
        # the mouth on the grass, ahead of the hooves).
        p.turn("Body", 6.0, 0.0, 0.0)
        p.turn("Chest", 32.0, 0.0, 0.0)
        bite = math.exp(-(((t % 0.5) - 0.3) * 14) ** 2)
        head(p, 50.0, 10.0 + 6.0 * bite, 14.0 * math.sin(2 * math.pi * t), 9.0 * bite)
        tail(p, 9.0 * math.sin(4 * math.pi * t), 0.0)
        ears(p, 10.0, 0.0)
        standing(p, sink=0.08, fore=0.1, hind=-0.05)
        graze.append(p)
        worst_muzzle.append(muzzle(p)[1])
    print(f"graze: the mouth from {min(worst_muzzle):.2f} to {max(worst_muzzle):.2f} m above the ground")
    clip("graze", graze, True)

    # Drink: the mouth at the trough's water.
    frames = 3 * FPS
    drink = []
    for f in range(frames):
        t = f / frames
        p = Pose()
        sip = 0.5 + 0.5 * math.sin(6 * math.pi * t)
        head(p, 42.0, 14.0, 0.0, 4.0 * sip)
        tail(p, 6.0 * math.sin(2 * math.pi * t), 0.0)
        standing(p, sink=0.02, fore=0.04)
        drink.append(p)
    print(f"drink: the mouth at {muzzle(drink[0])[1]:.2f} m")
    clip("drink", drink, True)

    # Low: the neck stretched out, the head up, the mouth open.
    seconds = 2.2
    frames = round(seconds * FPS)
    low = []
    for f in range(frames + 1):
        s = f / FPS
        o = smooth(s / 0.35) * (1 - smooth((s - 1.75) / 0.4))
        p = Pose()
        head(p, -10.0 * o, -28.0 * o, 0.0, 22.0 * o * (0.85 + 0.15 * math.sin(s * 9)))
        tail(p, 0.0, 10.0 * o)
        ears(p, -15.0 * o)
        standing(p, sink=0.0)
        low.append(p)
    clip("low", low, False)

    # Nurse (a calf): the head up under its mother, butting now and then, the tail going.
    frames = 2 * FPS
    nurse = []
    for f in range(frames):
        t = f / frames
        p = Pose()
        butt = math.exp(-(((t % 0.5) - 0.2) * 12) ** 2)
        head(p, -25.0 + 6.0 * butt, -25.0, 0.0, 6.0)
        tail(p, 30.0 * math.sin(10 * math.pi * t), 25.0)
        standing(p, sink=0.04, fore=0.05)
        nurse.append(p)
    clip("nurse", nurse, True)

    # Paw (the bull): the right forehoof scraping back through the dirt, the head low.
    frames = round(0.9 * FPS)
    paw = []
    for f in range(frames):
        t = f / frames
        p = Pose()
        head(p, 28.0, -14.0, 0.0, 0.0)
        ears(p, 30.0)
        tail(p, 14.0 * math.sin(2 * math.pi * t), 20.0)
        standing(p, sink=0.03)
        foot = LEG["ForeUpper.R"]["foot"].copy()
        if t < 0.45:  # lifted and brought forward
            u = t / 0.45
            foot[2] += lerp(-0.25, 0.22, smooth(u))
            foot[1] += 0.16 * math.sin(math.pi * u)
            c = 40.0 * math.sin(math.pi * u)
        else:  # dragged back along the ground
            u = (t - 0.45) / 0.55
            foot[2] += lerp(0.22, -0.25, u)
            foot[1] += 0.01
            c = 10.0 * u
        p.leg("ForeUpper.R", foot, c)
        paw.append(p)
    clip("paw", paw, True, [(0.5, "scrape")])

    # Lying down, the way cattle do it: front first onto the knees, then the back end, then the
    # hind legs tucked to one side and the body rolled onto them.
    def lie_pose(p, front, back, roll=0.0, settle=0.0, jaw=0.0, look=0.0):
        """`front` and `back`: 0 standing, 1 down."""
        chest = LIE_DROP * front
        rump = LIE_DROP * back
        # The body pitches between its two ends: chest and rump sink by these.
        span = REST_POS["ForeUpper.L"][2] - REST_POS["HindUpper.L"][2]
        pitch = math.degrees(math.atan2(chest - rump, span))
        p.sink(-(chest + rump) / 2)
        p.turn("Body", pitch, 0.0, roll)
        # Fore: knee on the ground, the shin folded back under the chest.
        for upper in ["ForeUpper.L", "ForeUpper.R"]:
            leg = LEG[upper]
            foot = leg["foot"].copy()
            if front < 0.02:
                p.leg(upper, foot)
                continue
            u, lo, h = CHAIN[upper]
            hip = p.joint_at(u)
            # Where the knee goes as the leg folds: ahead and down to the ground.
            k = smooth(front * 1.25)
            knee_y = max(lerp(REST_POS[lo][1], 0.07, k), 0.07)
            dy = hip[1] - knee_y
            dz = math.sqrt(max(leg["l1"] ** 2 - dy * dy, 0.0))
            knee = np.array([hip[0], knee_y, hip[2] + dz])
            if dy > leg["l1"]:
                knee = hip + np.array([0, -leg["l1"], 0])
            fold = smooth((front - 0.15) / 0.6)
            shin = np.array([0.0, lerp(-1.0, -0.06, fold), lerp(0.0, -1.0, fold)])
            p.aim(u, knee - hip)
            p.aim(lo, shin)
            p.aim(h, rot_x(lerp(0.0, 105.0, fold)).apply(bone(h)))
        # Hind: the hock back, the hooves folded forward under the belly, both to the left.
        for upper in ["HindUpper.L", "HindUpper.R"]:
            leg = LEG[upper]
            foot = leg["foot"].copy()
            if back < 0.02:
                p.leg(upper, foot)
                continue
            side = 1.0 if upper.endswith(".L") else -1.0
            hip = p.joint_at(CHAIN[upper][0])
            fold = smooth(back)
            out = 0.22 * fold * (1.0 if side > 0 else 0.35)
            target = np.array(
                [hip[0] + side * out, lerp(foot[1], 0.06, fold), lerp(foot[2], hip[2] + 0.42, fold * fold)]
            )
            p.leg(upper, target, lerp(0.0, 95.0, fold), bend_up=np.array([side * 0.3 * fold, 0.0, -1.0]))
        head(p, lerp(0.0, 12.0, settle) + 20.0 * front * (1 - back), 0.0, look, jaw)
        tail(p, 0.0, 0.0)

    global LIE_POSE
    LIE_POSE = lie_pose
    seconds = 2.4
    frames = round(seconds * FPS)
    lie_down = []
    for f in range(frames + 1):
        s = f / FPS
        front = smooth((s - 0.25) / 0.9)
        back = smooth((s - 1.0) / 0.9)
        settle = smooth((s - 1.7) / 0.7)
        p = Pose()
        lie_pose(p, front, back, roll=6.0 * settle, settle=settle)
        lie_down.append(p)
    clip("lie_down", lie_down, False)

    frames = 8 * FPS
    lie = []
    for f in range(frames):
        t = f / frames
        p = Pose()
        chew = 0.5 + 0.5 * math.sin(2 * math.pi * 12 * t)
        lie_pose(p, 1.0, 1.0, roll=6.0, settle=1.0, jaw=5.0 * chew, look=10.0 * math.sin(2 * math.pi * t))
        p.shift[1] += 0.006 * math.sin(2 * math.pi * 4 * t)
        tail(p, 4.0 * math.sin(2 * math.pi * 2 * t), 0.0)
        ears(p, 0.0, 20.0 * math.exp(-(((t - 0.4) * 40) ** 2)))
        lie.append(p)
    clip("lie", lie, True)

    # Getting up: the back end first (the rump up, still on its knees in front), then the front.
    seconds = 2.4
    frames = round(seconds * FPS)
    get_up = []
    for f in range(frames + 1):
        s = f / FPS
        settle = 1 - smooth(s / 0.4)
        back = 1 - smooth((s - 0.3) / 0.9)
        front = 1 - smooth((s - 1.15) / 0.95)
        p = Pose()
        lie_pose(p, front, back, roll=6.0 * settle, settle=settle)
        get_up.append(p)
    clip("get_up", get_up, False)


def slerp(a, b, t):
    from scipy.spatial.transform import Slerp

    return Slerp([0.0, 1.0], R.concatenate([a, b]))(t)


def build_die():
    """Dying: a jolt, the knees go (front first, then the back), and it tips over onto its right
    side, the legs going stiff and out. The game turns it into a body at the end of this."""
    seconds = 1.9
    frames = round(seconds * FPS)
    # Lying on its side, the legs out straight, the head on the ground.
    flat = Pose()
    for upper in LEGS:
        u, lo, h = CHAIN[upper]
        flat.aim(u, bone(u) + np.array([0.0, 0.0, 0.25 if upper.startswith("Fore") else -0.25]))
        flat.aim(lo, bone(lo))
        flat.aim(h, bone(h))
    head(flat, 25.0, 15.0, 0.0, 12.0)
    tail(flat, 0.0, -10.0)
    poses = []
    for f in range(frames + 1):
        sec = f / FPS
        jolt = math.sin(math.pi * min(sec / 0.3, 1.0))
        front = smooth((sec - 0.15) / 0.5)
        back = smooth((sec - 0.35) / 0.55)
        roll = smooth((sec - 0.75) / 0.75)
        p = Pose()
        LIE_POSE(p, front * 0.85, back * 0.8)
        head(p, -18.0 * jolt + 25.0 * front, -10.0 * jolt, 0.0, 14.0 * jolt)
        # Blend towards lying flat, and turn the whole thing over by the roll.
        g_kneel, g_flat = p.globals_of(), flat.globals_of()
        names = set(g_kneel) | set(g_flat)
        tip = R.from_euler("z", 84.0 * roll, degrees=True)
        out = {}
        for name in names:
            a = g_kneel.get(name, REST_ROT[name])
            b = g_flat.get(name, REST_ROT[name])
            out[name] = tip * slerp(a, b, roll)
        out["Root"] = tip * REST_ROT["Root"]
        # The body's middle stays about a body's width off the ground as it goes over.
        sunk = p.shift[1]
        shift = np.array([0.55 * roll, lerp(sunk, 0.2, roll), 0.0])
        poses.append(Pose.from_globals(out, shift))
    clip("die", poses, False)


def write():
    lines = [
        "// Written by tools/gen_bovino_keys.py: don't edit by hand, change the script and run it.",
        "// Every leg is solved there: the game only plays these clips.",
        "(",
        "    poses: {",
    ]
    clip_lines = ["    clips: {"]
    for name, (poses, looping, events) in CLIPS.items():
        keys = []
        for i, p in enumerate(poses):
            pose = f"{name}_{i:03d}"
            lines.append(f'        "{pose}": {p.ron()},')
            keys.append(f'({i / FPS:.4f}, ["{pose}"])')
        count = len(poses)
        duration = count / FPS if looping else (count - 1) / FPS
        ev = ", ".join(f'({t * duration:.3f}, "{e}")' for t, e in events)
        clip_lines.append(
            f'        "{name}": (duration: {duration:.4f}, looping: {str(looping).lower()}, '
            f"events: [{ev}], keys: [{', '.join(keys)}]),"
        )
    lines.append("    },")
    lines += clip_lines
    lines += ["    },", ")", ""]
    OUT.write_text("\n".join(lines), encoding="utf-8")
    print("written", OUT, f"({OUT.stat().st_size // 1024} KB)")


if __name__ == "__main__":
    build()
    build_die()
    write()
