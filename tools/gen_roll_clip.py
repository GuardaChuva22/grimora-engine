"""Writes `sword_roll.ron`: a combat roll that really rolls, over the left shoulder, across the back
and off the right hip onto the feet, with the arms and the legs open. Running it overwrites the file.

The height and the travel aren't written by hand. Every pass measures the skinned mesh against the
floor (`tests/roll.rs`), lifts the body until the lowest point of the trunk rests on it and moves it
so that point doesn't slide: what rolls has to stand still where it touches. Run from the repository:

    python -I tools/gen_roll_clip.py
    ROLL=sword_roll cargo test --release -p grimora_scene --test roll -- --nocapture
"""

import math
import os
import subprocess

import numpy as np

CLIP = "assets/models/chartest/chartest_clips/sword_roll.ron"
FPS = 60
DURATION = 0.92
PASSES = 10
N = int(round(DURATION * FPS)) + 1
TIMES = np.arange(N) / FPS

# Rest joints, from `RIG=1 cargo test ... --test roll`.
HIPS = np.array([0.0, 0.8905, 0.0256])
SPINE = np.array([0.0, 1.0696, 0.0256])
CHEST = np.array([0.0, 1.3021, 0.0562])

# Where the clip starts and ends: the old roll's, the sword low at the side.
START = dict(
    hips=(0.0, -0.06, 0.01),
    hand_r=(0.34, 0.86, -0.12),
    hand_l=(-0.33, 0.97, -0.05),
    blade=(0.26, -0.46, -0.85),
    edge=(0.08, 0.88, -0.46),
    elbow_r=(0.62, -0.1, 0.78),
    elbow_l=(-0.6, -0.1, 0.75),
    foot_l=(-0.1, 0.1, -0.04),
    foot_r=(0.12, 0.1, 0.1),
)

# The spin, the curve of the back and the lean that puts the left shoulder first (x y z of `hips_rot`).
SPIN = [(0, 3), (0.06, 10), (0.14, 46), (0.22, 105), (0.3, 165), (0.38, 225), (0.46, 280), (0.54, 322), (0.62, 348), (0.72, 360), (0.92, 363)]
YAW = [(0, -10), (0.1, -14), (0.3, -16), (0.6, -10), (0.92, -10)]
LEAN = [(0, 1), (0.1, 10), (0.22, 24), (0.4, 26), (0.54, 14), (0.66, 4), (0.92, 1)]
BACK = [(0, 5), (0.08, 26), (0.2, 46), (0.34, 52), (0.46, 46), (0.56, 34), (0.66, 24), (0.78, 12), (0.92, 5)]
BACK_TWIST = [(0, 12), (0.2, -10), (0.4, -12), (0.6, 0), (0.92, 12)]
HEAD = [(0, 2), (0.1, 22), (0.22, 38), (0.4, 42), (0.54, 26), (0.66, 8), (0.92, 2)]
HEAD_TURN = [(0, -2), (0.2, -25), (0.45, -25), (0.65, -5), (0.92, -2)]

# How low the hips want to go; the floor stops them. Then the crouch they land in, and up.
LOW = [(0, -0.06), (0.08, -0.14), (0.16, -0.34), (0.24, -0.8), (0.48, -0.8), (0.58, -0.47), (0.66, -0.4), (0.76, -0.2), (0.92, -0.06)]
# The dive's travel, up to where the back takes over. Then what's left dies away.
DIVE = [(0, 0), (0.06, -0.1), (0.14, -0.36), (0.22, -0.72)]
DIES_FROM, DIES_TO = 0.54, 0.78

# Hands from the chest, in its frame, always away from it: reaching for the floor, then flung out.
HAND_L = [(0.1, (-0.36, -0.1, -0.32)), (0.18, (-0.38, -0.12, -0.36)), (0.28, (-0.42, -0.05, -0.16)), (0.4, (-0.46, -0.16, 0.04)), (0.52, (-0.4, -0.2, -0.12)), (0.62, (-0.34, -0.24, -0.2))]
# The sword lies out to the side, close to the line the body spins round, so it doesn't sweep the floor.
# These came from a search scored on the wrist, the twist and on joints turning too far in a frame: an
# elbow's pole crossing the line from the shoulder to the wrist flips the whole arm in one frame.
HAND_R = [(0.1, (0.36, -0.2, -0.24)), (0.3, (0.38, -0.19, -0.17)), (0.46, (0.36, -0.18, -0.24)), (0.62, (0.27, -0.24, -0.24))]
BLADE = [(0.1, (0.85, -0.05, -0.35)), (0.34, (0.85, -0.05, -0.35)), (0.46, (1.0, -0.05, -0.62)), (0.6, (0.55, 0.15, -0.6))]
# Round the blade from where the start pose's edge would be, in the roll and in the crouch.
EDGE_TURNS = (145.0, 130.0)
POLES_R = ((0.1, -1.3, 0.2), (0.0, -0.8, 0.0))
POLE_L = (-0.9, -0.2, 0.3)
STAGES = [(0.34, 0), (0.46, 1)]
ARMS_OUT = (0.0, 0.14)  # from the start pose to the hold
ARMS_BACK = (0.6, 0.86)  # and back to it

# The feet from the hips, in their frame, in the air: knees bent, the legs apart. Not against the chest.
FLY_L = [(0.2, (-0.17, -0.56, -0.4)), (0.4, (-0.2, -0.48, -0.44)), (0.56, (-0.2, -0.58, -0.36))]
FLY_R = [(0.2, (0.2, -0.46, -0.28)), (0.4, (0.22, -0.4, -0.32)), (0.5, (0.2, -0.54, -0.3))]
ANKLE = 0.1
STEP_L = (0.0, 0.1, (-0.12, 0.1, -0.44))  # the left foot steps out before the dive
LIFT_R, LIFT_L = 0.14, 0.22
LIFT_SPAN, SETTLE = 0.08, 0.08
# The ball of the foot from the ankle: a raised heel has to lift the ankle by this much.
TOE = 0.145
LAND_WIDE = (0.2, -0.22)
SHUFFLE_R, SHUFFLE_L = (0.72, 0.84), (0.76, 0.88)
EVENTS = [(0.25, "fall"), (0.52, "fall"), (0.66, "cancel")]


def smooth(a, b, t):
    x = np.clip((t - a) / (b - a), 0.0, 1.0)
    return x * x * (3.0 - 2.0 * x)


def monotone(keys, times=TIMES):
    """The engine's curve: a cubic that doesn't overshoot its keys (Fritsch-Carlson)."""
    xs = np.array([k[0] for k in keys], float)
    ys = np.array([k[1] for k in keys], float)
    out = np.zeros((len(times),) + ys.shape[1:])
    h = np.diff(xs)
    d = np.diff(ys, axis=0) / h.reshape((-1,) + (1,) * (ys.ndim - 1))
    m = np.zeros_like(ys)
    m[0], m[-1] = d[0], d[-1]
    for i in range(1, len(xs) - 1):
        m[i] = np.where(d[i - 1] * d[i] > 0, 2 * d[i - 1] * d[i] / (d[i - 1] + d[i] + 1e-12), 0.0)
    for k, t in enumerate(times):
        t = min(max(t, xs[0]), xs[-1])
        i = min(np.searchsorted(xs, t, side="right") - 1, len(xs) - 2)
        s = (t - xs[i]) / h[i]
        out[k] = (
            (2 * s**3 - 3 * s**2 + 1) * ys[i]
            + (s**3 - 2 * s**2 + s) * h[i] * m[i]
            + (-2 * s**3 + 3 * s**2) * ys[i + 1]
            + (s**3 - s**2) * h[i] * m[i + 1]
        )
    return out


def turn(angles):
    """`rotation_of` in pose.rs: Ry(y) Rx(-x) Rz(-z)."""
    x, y, z = np.radians(angles)
    cx, sx, cy, sy, cz, sz = math.cos(-x), math.sin(-x), math.cos(y), math.sin(y), math.cos(-z), math.sin(-z)
    rx = np.array([[1, 0, 0], [0, cx, -sx], [0, sx, cx]])
    ry = np.array([[cy, 0, sy], [0, 1, 0], [-sy, 0, cy]])
    rz = np.array([[cz, -sz, 0], [sz, cz, 0], [0, 0, 1]])
    return ry @ rx @ rz


def slerp_turn(a, b, s):
    """Between two rotation matrices, the short way."""
    r = a.T @ b
    angle = math.acos(np.clip((np.trace(r) - 1) / 2, -1, 1))
    if angle < 1e-5:
        return a
    axis = np.array([r[2, 1] - r[1, 2], r[0, 2] - r[2, 0], r[1, 0] - r[0, 1]]) / (2 * math.sin(angle))
    k = np.array([[0, -axis[2], axis[1]], [axis[2], 0, -axis[0]], [-axis[1], axis[0], 0]])
    a2 = angle * s
    return a @ (np.eye(3) + math.sin(a2) * k + (1 - math.cos(a2)) * k @ k)


def frame(blade, edge):
    b = np.asarray(blade, float) / np.linalg.norm(blade)
    e = np.asarray(edge, float) - b * np.dot(edge, b)
    e /= np.linalg.norm(e)
    return np.column_stack([b, e, np.cross(b, e)])


def roll_x(degrees):
    c, s = math.cos(math.radians(degrees)), math.sin(math.radians(degrees))
    return np.array([[1, 0, 0], [0, c, -s], [0, s, c]])


def blur(a, frames=2):
    kernel = np.exp(-0.5 * (np.arange(-3 * frames, 3 * frames + 1) / frames) ** 2)
    kernel /= kernel.sum()
    return np.convolve(np.pad(a, 3 * frames, mode="edge"), kernel, mode="valid")


def build(height, step):
    """Every track, given the hips' height over the rest pose and their travel per frame."""
    spin, yaw, lean = monotone(SPIN), monotone(YAW), monotone(LEAN)
    back, twist = monotone(BACK), monotone(BACK_TWIST)
    head, head_turn = monotone(HEAD), monotone(HEAD_TURN)
    hips_world = 0.01 + np.cumsum(step)
    target = hips_world - 0.01
    root_z = blur(target, 7)
    root_z += (target[-1] - root_z[-1]) * (TIMES / DURATION) ** 2 - root_z[0] * (1 - TIMES / DURATION) ** 2
    root_z = np.minimum.accumulate(root_z)
    hips = np.column_stack([np.zeros(N), height, hips_world - root_z])
    rots = np.column_stack([spin, yaw, lean])
    torso = np.column_stack([back, twist, -0.08 * lean])
    heads = np.column_stack([head, head_turn, np.zeros(N)])

    t = {name: [] for name in "hand_r hand_l blade edge elbow_r elbow_l".split()}
    start_frame = frame(START["blade"], START["edge"])
    # Which way the edge faces in the hold: back along the forearm, turned by EDGE_TURN round the blade.
    stages = STAGES
    turns_at = monotone([(at, (EDGE_TURNS[i],)) for at, i in stages])[:, 0]
    poles_r = monotone([(at, POLES_R[i]) for at, i in stages])
    blades = monotone(BLADE)
    hand_r_local, hand_l_local = monotone(HAND_R), monotone(HAND_L)
    hip_at = np.zeros((N, 3))
    for i, now in enumerate(TIMES):
        h = turn(rots[i])
        chest_turn = h @ turn(torso[i])
        part = slerp_turn(np.eye(3), turn(torso[i]), 0.45)
        at = HIPS + hips[i]
        chest = at + h @ (SPINE - HIPS) + h @ part @ (CHEST - SPINE)
        out = smooth(*ARMS_OUT, now) * (1 - smooth(*ARMS_BACK, now))
        t["hand_r"].append(np.array(START["hand_r"]) * (1 - out) + (chest + chest_turn @ hand_r_local[i]) * out)
        left = np.array(START["hand_l"]) * (1 - out) + (chest + chest_turn @ hand_l_local[i]) * out
        left[1] = max(left[1], 0.12)
        t["hand_l"].append(left)
        # Not a blend of two frames: the short way there can wring the forearm. The blade moves, the
        # edge follows it, and the turn round the blade grows on its own.
        unrolled = slerp_turn(start_frame, chest_turn @ frame(blades[i], (0.0, 0.17, -0.98)), out)
        turned = unrolled @ roll_x(turns_at[i] * out)
        t["blade"].append(turned[:, 0])
        t["edge"].append(turned[:, 1])
        t["elbow_r"].append(np.array(START["elbow_r"]) * (1 - out) + chest_turn @ poles_r[i] * out)
        t["elbow_l"].append(np.array(START["elbow_l"]) * (1 - out) + chest_turn @ np.array(POLE_L) * out)
        hip_at[i] = at + [0, 0, root_z[i]]

    # The feet: planted, off, in the air with the hips, down where they come down, then a last shuffle.
    tracks = {}
    stance_l = np.array(START["foot_l"]) + [0, 0, root_z[-1]]
    stance_r = np.array(START["foot_r"]) + [0, 0, root_z[-1]]
    fly_l, fly_r = monotone(FLY_L), monotone(FLY_R)
    side = {
        "l": (LIFT_L, fly_l, np.array(START["foot_l"]), LAND_WIDE[1], SHUFFLE_L, stance_l, -1),
        "r": (LIFT_R, fly_r, np.array(START["foot_r"]), LAND_WIDE[0], SHUFFLE_R, stance_r, 1),
    }
    for name, (lift, fly, first, wide, shuffle, stance, sign) in side.items():
        air = np.array([hip_at[i] + turn(rots[i]) @ fly[i] for i in range(N)])
        # It comes down where it touches, and stays: nothing slides.
        i_high = int(np.argmax(air[:, 1]))
        i_land = i_high + int(np.argmax(air[i_high:, 1] <= ANKLE + TOE * 0.3))
        land = TIMES[i_land]
        planted = air[i_land].copy()
        feet, angles, knees = [], [], []
        for i, now in enumerate(TIMES):
            h = turn(rots[i])
            base = first.copy()
            if name == "l":
                stepped = smooth(STEP_L[0], STEP_L[1], now)
                base = first * (1 - stepped) + np.array(STEP_L[2]) * stepped
                base[1] += 0.07 * math.sin(math.pi * stepped)
            off = smooth(lift, lift + LIFT_SPAN, now)
            settle = smooth(land, land + SETTLE, now)
            if now < land:
                p = base * (1 - off) + air[i] * off
            else:
                p = planted.copy()
                p[0] = planted[0] * (1 - settle) + wide * settle
                p[1] = ANKLE * settle + planted[1] * (1 - settle)
            shuf = smooth(shuffle[0], shuffle[1], now)
            p = p * (1 - shuf) + stance * shuf
            p[1] += 0.05 * math.sin(math.pi * shuf)
            air_part = smooth(lift - 0.04, lift + LIFT_SPAN, now) * (1 - settle)
            ankle = 30.0 if name == "l" else 42.0
            flat = 0.0 if now < 0.3 else 360.0
            # The heel comes up before the foot leaves the floor.
            flat += 34.0 * smooth(lift - 0.08, lift, now) * (1 - off)
            angle = np.array([flat, 0.0, 0.0]) * (1 - air_part) + (rots[i] + [ankle, 0, 0]) * air_part
            angles.append(angle)
            p[1] = max(p[1], ANKLE + TOE * max(math.sin(math.radians(angle[0])), 0.0))
            feet.append(p)
            knee = np.array([0.12 * sign, 0.0, -1.0])
            flying = smooth(lift - 0.1, lift, now) * (1 - smooth(land, land + 0.2, now))
            knees.append(knee * (1 - flying) + h @ np.array([0.2 * sign, 0.35, -1.0]) * flying)
        tracks[f"foot_{name}"] = np.array(feet)
        tracks[f"foot_{name}_rot"] = np.array(angles)
        tracks[f"knee_{name}"] = np.array(knees)
    tracks["root"] = np.column_stack([np.zeros(N), np.zeros(N), root_z])
    tracks["hips"] = hips
    tracks["hips_rot"] = rots
    tracks["torso"] = torso
    tracks["head"] = heads
    for name, keys in t.items():
        tracks[name] = np.array(keys)
    return tracks


def write(tracks):
    lines = ["(", f"    duration: {DURATION:.4f},", "    springs: false,", "    tracks: {"]
    for name in sorted(tracks):
        lines.append(f"        {name}: [")
        for i, v in enumerate(tracks[name]):
            lines.append(f"            ({TIMES[i]:.4f}, ({v[0]:.4f}, {v[1]:.4f}, {v[2]:.4f})),")
        lines.append("        ],")
    lines += ["    },", "    events: ["]
    lines += [f'        ({at:.4f}, "{name}"),' for at, name in EVENTS]
    lines += ["    ],", ")", ""]
    with open(CLIP, "w", encoding="utf8", newline="\n") as f:
        f.write("\n".join(lines))


def measure():
    env = dict(os.environ, ROLL="sword_roll", ROLL_DATA="1")
    out = subprocess.run(
        ["cargo", "test", "--release", "-q", "-p", "grimora_scene", "--test", "roll", "a_roll_stays", "--", "--nocapture"],
        env=env,
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    rows = [line.split() for line in out.splitlines() if line.startswith("F ")]
    return np.array([float(r[2]) for r in rows]), np.array([float(r[3]) for r in rows])


def main():
    wanted = monotone(LOW)
    height = wanted.copy()
    dive = monotone(DIVE)
    step = np.diff(dive, prepend=dive[0])
    i_dive, i_from = int(round(DIVE[-1][0] * FPS)), int(round(DIES_FROM * FPS))
    # Until the floor says otherwise, the rolling part keeps the dive's speed.
    step[i_dive:] = step[i_dive - 1]
    fade = 1 - smooth(DIES_FROM, DIES_TO, TIMES)
    for number in range(PASSES):
        step[i_from:] = step[i_from - 1] * fade[i_from:]
        write(build(height, step))
        trunk, slip = measure()
        # Up until the trunk rests on the floor; only what the floor pushes back counts.
        height = np.maximum(wanted, blur(np.maximum(height - trunk, wanted), 1))
        touching = (trunk < 0.03) & (TIMES >= DIVE[-1][0]) & (TIMES <= DIES_FROM)
        step = step - 0.7 * slip * touching
        slide = np.abs(slip[touching]).mean() * FPS if touching.any() else 0
        print(f"pass {number}: lowest {trunk.min():.3f}, slide {slide:.2f} m/s")
    step[i_from:] = step[i_from - 1] * fade[i_from:]
    tracks = build(height, step)
    write(tracks)
    print(f"root travels {tracks['root'][-1][2]:.2f} m")
    return height, step


if __name__ == "__main__":
    main()
