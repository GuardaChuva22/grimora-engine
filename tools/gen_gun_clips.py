"""Writes the flintlocks' clips: `pistol_*` and `musket_*`, in assets/models/chartest/chartest_clips.

Poses say what the gun does, in the body's space, and this works out `hand_r`, `blade` and `edge`:
the barrel's direction, which way its top faces, and which point of the gun is where. The free
hand is told where its palm goes and which way the fingers and the palm face, because it has things
to reach on the gun. Overwrites those files, so edits made in the animator go away: run on request.

    python -I tools/gen_gun_clips.py

It also prints what `items.ron` wants for each gun (`at`, `turn`, `rod_grip`).
"""

import copy
import json
import math
import os

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
MODEL = os.path.join(HERE, "..", "assets", "models", "chartest")
OUT = os.path.join(MODEL, "chartest_clips")


def unit(v):
    v = np.array(v, dtype=float)
    return v / np.linalg.norm(v)


def rot_y(degrees):
    c, s = math.cos(math.radians(degrees)), math.sin(math.radians(degrees))
    return np.array([[c, 0, s], [0, 1, 0], [-s, 0, c]])


def rot_z(degrees):
    c, s = math.cos(math.radians(degrees)), math.sin(math.radians(degrees))
    return np.array([[c, -s, 0], [s, c, 0], [0, 0, 1]])


def rot_x(degrees):
    c, s = math.cos(math.radians(degrees)), math.sin(math.radians(degrees))
    return np.array([[1, 0, 0], [0, c, -s], [0, s, c]])


# ---- the free hand, measured on the model at rest

def rest_of_left_hand():
    document = json.load(open(os.path.join(MODEL, "chartest.gltf"), encoding="utf8"))
    nodes = document["nodes"]
    parent = {c: i for i, n in enumerate(nodes) for c in n.get("children", [])}

    def quat(q):
        x, y, z, w = q
        return np.array(
            [
                [1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
                [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
                [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)],
            ]
        )

    def world(i):
        node = nodes[i]
        r = quat(node.get("rotation", [0, 0, 0, 1])) @ np.diag(node.get("scale", [1, 1, 1]))
        t = np.array(node.get("translation", [0, 0, 0]))
        if i in parent:
            pr, pt = world(parent[i])
            return pr @ r, pr @ t + pt
        return r, t

    index = {n.get("name"): i for i, n in enumerate(nodes)}
    hand_r, hand_p = world(index["hand.L"])
    _, knuckle = world(index["fingers.L"])
    _, row = world(index["index_01_l_015"])
    fingers = unit(knuckle - hand_p)
    towards = row - knuckle
    towards = unit(towards - fingers * towards.dot(fingers))
    return hand_r, fingers, towards, np.cross(fingers, towards), np.linalg.norm(knuckle - hand_p)


JOINT, F0, T0, N0, KNUCKLE = rest_of_left_hand()
# The hand's frames, as columns: fingers, thumb side, palm.
REST = np.column_stack([F0, T0, N0])
# Where things sit in the palm: from the wrist, along the fingers and out of the palm.
PALM = (KNUCKLE * 0.8, 0.03)
FINGERTIPS = (KNUCKLE + 0.045, 0.025)


def euler_of(r):
    """What `rotation_of` wants to make `r`: Ry(y) Rx(-x) Rz(-z), in degrees."""
    x = math.asin(max(-1.0, min(1.0, -r[1, 2])))
    y = math.atan2(r[0, 2], r[2, 2])
    z = math.atan2(r[1, 0], r[1, 1])
    return np.degrees([-x, y, -z])


def matrix_of(angles):
    """The other way from `euler_of`."""
    return rot_y(angles[1]) @ rot_x(-angles[0]) @ rot_z(-angles[2])


def turned_between(a, b, share):
    """The angles a share of the way from `a` to `b`, by the shortest turn of the hand."""
    delta = matrix_of(b) @ matrix_of(a).T
    angle = math.acos(max(-1.0, min(1.0, (np.trace(delta) - 1.0) / 2.0)))
    if angle < 1e-4:
        return np.array(a, dtype=float)
    axis = unit([delta[2, 1] - delta[1, 2], delta[0, 2] - delta[2, 0], delta[1, 0] - delta[0, 1]])
    k = np.array([[0, -axis[2], axis[1]], [axis[2], 0, -axis[0]], [-axis[1], axis[0], 0]])
    part = np.eye(3) + math.sin(angle * share) * k + (1 - math.cos(angle * share)) * k @ k
    return euler_of(part @ matrix_of(a))


def palm_on(point, fingers, palm, at=PALM, curl=0.0, elbow=(-0.4, -0.3, 0.7)):
    """The free hand with its palm at `point`, fingers and palm facing those ways."""
    f = unit(fingers)
    n = np.array(palm, dtype=float)
    n = unit(n - f * n.dot(f))
    t = np.cross(n, f)
    frame = np.column_stack([f, t, n])
    wrist = np.array(point, dtype=float) - f * at[0] - n * at[1]
    return {"hand_l": wrist, "hand_l_rot": euler_of(frame @ REST.T), "curl_l": curl, "elbow_l": np.array(elbow, dtype=float)}


def fist_on(point, rod_up, towards=(0.55, 0.0, -0.85), curl=1.0, elbow=(-0.5, -0.4, 0.6)):
    """The free hand closed around a rod going up, with the thumb on top."""
    t = unit(rod_up)
    f = np.array(towards, dtype=float)
    f = unit(f - t * f.dot(t))
    return palm_on(point, f, np.cross(f, t), curl=curl, elbow=elbow)


def hanging(curl=0.25, sway=0.0, breathing=0.0):
    """Slightly bent at the elbow, so that it isn't a locked straight arm, and drifting with the body."""
    point = (-0.31 + 0.004 * sway, 0.93 + 0.004 * breathing, -0.03 - 0.004 * sway)
    return palm_on(point, (0.02, -0.97, -0.2), (0.9, -0.1, 0.15), curl=curl)


# The fingers and palm of a hand dropping a ball into a muzzle, and the fist on a pistol's short rod.
OVER_FINGERS = (0.8, -0.3, -0.5)
OVER_PALM = (0.2, -0.8, 0.55)
P_ROD_TOWARDS = (0.8, 0.0, -0.6)
P_ROD_ELBOW = (-0.4, -1.0, 0.1)


# ---- the guns

class Gun:
    def __init__(self, name, grip, slope, muzzle, rod_y, rod_from, rod_to, barrel_y):
        self.name, self.grip, self.muzzle = name, np.array(grip), np.array(muzzle)
        self.rod_y, self.rod_from, self.rod_to, self.barrel_y = rod_y, rod_from, rod_to, barrel_y
        # From the model's space to the joint's: the barrel leaves the fist away from the wrist,
        # and the handle leans forward by `slope`.
        self.slope = slope
        self.held = rot_y(90) @ rot_z(slope)
        self.at = -self.held @ self.grip
        # Held like a staff instead, by the barrel: the barrel goes up out of the fist.
        self.staff = rot_y(-90) @ rot_z(90)


class Pose:
    def __init__(self, gun, barrel, up=None, lock=None, grip_at=None, anchor=None, anchor_at=None, staff=None):
        """`barrel` is where it points. `up` is where its top faces, or `lock` where its lock's
        side faces. Either `grip_at` says where the hand is, or `anchor` (a point of the model)
        where `anchor_at` says. With `staff`, the hand is on the gun at that point instead."""
        x = unit(barrel)
        if lock is not None:
            z = np.array(lock, dtype=float)
            z = unit(z - x * z.dot(x))
            y = np.cross(z, x)
        else:
            y = np.array(up, dtype=float)
            y = unit(y - x * y.dot(x))
            z = np.cross(x, y)
        self.file_to_body = np.column_stack([x, y, z])
        self.gun = gun
        self.staff = staff
        grip = np.array(staff) if staff is not None else gun.grip
        if grip_at is not None:
            self.hand = np.array(grip_at, dtype=float)
        else:
            self.hand = np.array(anchor_at, dtype=float) + self.file_to_body @ (grip - np.array(anchor))
        self.grip = grip

    def point(self, x, y=0.0, z=0.0):
        return self.hand + self.file_to_body @ (np.array([x, y, z], dtype=float) - self.grip)

    def along(self):
        return self.file_to_body[:, 0]

    def tracks(self):
        to_joint = self.gun.staff if self.staff is not None else self.gun.held
        frame = self.file_to_body @ to_joint.T
        return {"hand_r": self.hand, "blade": frame[:, 1], "edge": frame[:, 2]}


def key(pose, torso, head, elbow_r, left):
    out = dict(pose.tracks())
    out.update({"torso": np.array(torso, dtype=float), "head": np.array(head, dtype=float)})
    out["elbow_r"] = np.array(elbow_r, dtype=float)
    out.update(left)
    return out


def write_clip(name, duration, looping, springs, keys):
    tracks = {}
    for time, values in keys:
        for control, value in values.items():
            value = np.array(value, dtype=float) if np.ndim(value) else np.array([value, 0.0, 0.0])
            tracks.setdefault(control, []).append((time, value))
    # Angles keep going the same way round, so that nothing spins between two keys.
    # Between two far apart turns of the free hand, the shortest way, or the angles alone would
    # bend the wrist much further on the way than at either end.
    for control in ("hand_l_rot",):
        track = tracks.get(control, [])
        fine = track[:1]
        for (t0, a), (t1, b) in zip(track, track[1:]):
            for share in (0.25, 0.5, 0.75):
                fine.append((t0 + (t1 - t0) * share, turned_between(a, b, share)))
            fine.append((t1, b))
        for i in range(1, len(fine)):
            step = (fine[i][1] - fine[i - 1][1] + 180.0) % 360.0 - 180.0
            fine[i] = (fine[i][0], fine[i - 1][1] + step)
        if track:
            tracks[control] = fine
    lines = ["(", f"    duration: {duration:.4f},", f"    looping: {str(looping).lower()},", f"    springs: {str(springs).lower()},", "    tracks: {"]
    for control, track in tracks.items():
        lines.append(f"        {control}: [")
        lines.extend(f"            ({t:.4f}, ({v[0]:.4f}, {v[1]:.4f}, {v[2]:.4f}))," for t, v in track)
        lines.append("        ],")
    lines.extend(["    },", ")", ""])
    with open(os.path.join(OUT, f"{name}.ron"), "w", encoding="utf8", newline="\n") as out:
        out.write("\n".join(lines))
    print("wrote", name, f"{duration:.2f} s")


def moved(pose, offset=(0.0, 0.0, 0.0), turn=None, pivot=None):
    """The same gun shifted, and turned around `pivot` (the hand, unless told)."""
    out = copy.copy(pose)
    turn = np.eye(3) if turn is None else turn
    pivot = pose.hand if pivot is None else np.array(pivot, dtype=float)
    out.file_to_body = turn @ pose.file_to_body
    out.hand = pivot + turn @ (pose.hand - pivot) + np.array(offset, dtype=float)
    return out


def breath(t, length):
    """(in, sway, nod), -1..1, each closing the loop at `length`. They peak at different moments,
    so that no two parts of the body arrive together."""
    phase = 2.0 * math.pi * t / length
    return math.sin(phase), math.sin(phase + 0.9), math.sin(2.0 * phase + 2.1)


def idle_times(length, count=8):
    return [length * i / count for i in range(count + 1)]


# ---------------------------------------------------------------- pistol

PISTOL = Gun("pistola", grip=(-0.002, -0.012, 0.0), slope=30.0, muzzle=(0.411, 0.101, 0.0), rod_y=0.055, rod_from=0.178, rod_to=0.396, barrel_y=0.101)

# Side on to the target, the right shoulder thrown forward and the arm straight out: the chest turns
# a lot and the head turns back to look down the arm.
P_AIM_TORSO, P_AIM_HEAD = (3, 40, 0), (0, -38, 0)
P_AIM_GRIP = (0.18, 1.413, -0.61)
P_ELBOW_AIM = (0.6, -0.75, 0.2)
P_BEHIND = palm_on((-0.1, 1.03, 0.28), (0.8, -0.25, 0.3), (0.05, 0.1, 1.0), curl=0.55, elbow=(-0.95, -0.2, 0.5))
P_HOLD = 3.2


def pistol_aim(up=0.0, back=0.0, climb=0.0):
    pitch = rot_x(up)
    return Pose(PISTOL, pitch @ np.array([0, 0, -1.0]), up=pitch @ np.array([0, 1.0, 0]), grip_at=(P_AIM_GRIP[0], P_AIM_GRIP[1] + climb, P_AIM_GRIP[2] + back))


def pistol_hold():
    """Hanging by the thigh, the elbow a little bent and back, the muzzle at the ground ahead."""
    return Pose(PISTOL, (0.06, -0.94, -0.34), lock=(1.0, 0.1, 0.0), grip_at=(0.27, 0.96, -0.04))


def write_stand():
    """The legs under any gun that's held still. An idle made for a bare stance would rock the
    hips under the gun, and a musket's butt would skate on the ground."""
    length = 4.0

    def stand(t):
        breathing, sway, nod = breath(t, length)
        # The chest, the head and a free left arm, for when the gun's clip lets that arm go
        # (`Animator::overlay_leave`): it hangs and drifts here, and swings with the walk.
        return hanging(sway=sway, breathing=breathing) | {
            "torso": (2.0 + 1.2 * breathing, 4.0 + 1.0 * sway, 0.6 * sway),
            "head": (0.8 * sway, -4.0 - 2.5 * nod, 0.0),
            "hips": (0.012 + 0.006 * sway, -0.035 + 0.003 * breathing, 0.0),
            "hips_rot": (2.0, -4.0 + 0.6 * sway, 1.5 + 0.6 * sway),
            "foot_l": (-0.14, 0.09, -0.08),
            "foot_r": (0.17, 0.09, 0.12),
            "foot_l_rot": (0.0, 8.0, 0.0),
            "foot_r_rot": (0.0, -12.0, 0.0),
        }

    write_clip("gun_stand", length, True, False, [(t, stand(t)) for t in idle_times(length)])


def write_pistol():
    def hold(t=0.0):
        breathing, sway, nod = breath(t, P_HOLD)
        gun = moved(pistol_hold(), (0.003 * sway, 0.006 * breathing, -0.004 * breathing), rot_x(1.8 * sway))
        left = hanging(sway=sway, breathing=breathing)
        return key(gun, (2 + 1.2 * breathing, 4 + 1.0 * sway, 0.6 * sway), (0.8 * sway, -4 - 2.5 * nod, 0), (0.35, -0.15, 1.0), left)

    def aim(up=0.0, back=0.0, climb=0.0, torso=P_AIM_TORSO, head=P_AIM_HEAD):
        return key(pistol_aim(up, back, climb), torso, head, P_ELBOW_AIM, P_BEHIND)

    write_clip("pistol_hold", P_HOLD, True, False, [(t, hold(t)) for t in idle_times(P_HOLD)])
    write_clip(
        "pistol_aim",
        3.0,
        True,
        False,
        [
            (0.0, aim()),
            (1.5, aim(up=0.6, climb=0.003, torso=(3.8, 40.4, 0), head=(-0.4, -38.3, 0))),
            (3.0, aim()),
        ],
    )
    write_clip(
        "pistol_fire",
        0.62,
        False,
        True,
        [
            (0.0, aim()),
            (0.03, aim(up=30.0, back=0.04, climb=0.045, torso=(-5, 44, 0), head=(-3, -40, 0))),
            (0.11, aim(up=24.0, back=0.03, climb=0.05, torso=(-6, 43, 0), head=(-4, -40, 0))),
            (0.3, aim(up=4.0, back=0.008, climb=0.012, torso=(0, 41, 0))),
            (0.42, aim(up=-1.5)),
            (0.62, aim()),
        ],
    )

    # Loading: the barrel leans away, with the forearm under it and the wrist nearly straight. The
    # free hand works over the muzzle, so the gun comes to the middle of the chest.
    base = Pose(PISTOL, (-0.15, 0.75, -0.65), up=(-0.1, 0.65, 0.75), grip_at=(0.07, 1.2, -0.22))
    muzzle_x = PISTOL.muzzle[0]
    along = 0.172  # where the fist has the rod, from its end

    def at_work(lift=0.0, forward=0.0, tilt=0.0):
        return moved(base, (0.0, lift, -forward), rot_x(tilt))

    def ready(gun, left, torso, head, elbow=(0.35, -1.0, 0.2)):
        return key(gun, torso, head, elbow, left)

    def on_rod(gun, end, y):
        return fist_on(gun.point(end + along, y), gun.along(), towards=P_ROD_TOWARDS, elbow=P_ROD_ELBOW)

    def rod_key(end, y, lift, torso, head):
        gun = at_work(lift=lift)
        return ready(gun, on_rod(gun, end, y), torso, head)

    rest_end = PISTOL.rod_from
    pulled = muzzle_x + 0.03
    seat = muzzle_x - 0.13
    pouch = lambda curl: palm_on((-0.25, 0.97, -0.13), (0.25, -0.95, -0.1), (0.9, 0.2, 0.3), curl=curl)
    rising = palm_on((-0.08, 1.22, -0.27), (0.45, 0.35, -0.8), (0.55, 0.65, 0.5), curl=0.9)

    def over(gun, curl, above=0.05):
        # Comes in from the side with the ball pinched, the fingers down and away from the chest.
        return palm_on(gun.point(muzzle_x + above, PISTOL.barrel_y), OVER_FINGERS, OVER_PALM, at=FINGERTIPS, curl=curl)

    lowered, raised, ball = at_work(lift=-0.02, tilt=3.0), at_work(lift=0.012, tilt=-2.0), at_work(lift=-0.006)
    write_clip(
        "pistol_reload",
        3.5,
        False,
        False,
        [
            (0.0, hold()),
            # Down to the pouch, with the shoulder and the eyes: the gun waits low.
            (0.28, ready(lowered, pouch(0.2), (20, 2, -7), (20, 16, 0))),
            (0.5, ready(moved(lowered, (0, -0.006, 0)), pouch(0.9), (22, 3, -8), (22, 18, 0))),
            # Back up, leading with the eyes.
            (0.76, ready(ball, rising, (15, -7, -1), (8, 5, 0))),
            (1.0, ready(raised, over(raised, 0.75), (11, -13, 2), (6, 7, 0))),
            (1.12, ready(moved(raised, (0, -0.006, 0)), over(raised, 0.35, 0.06), (12, -13, 2), (8, 7, 0))),
            # The rod: every pull lifts the chest and every push drops it onto the hands.
            (1.34, rod_key(rest_end, PISTOL.rod_y, -0.012, (15, -11, 0), (14, 6, 0))),
            (1.6, rod_key(pulled, PISTOL.rod_y, -0.09, (11, -29, 3), (-2, 14, 0))),
            (1.74, rod_key(pulled, PISTOL.barrel_y, -0.09, (11, -29, 3), (-4, 14, 0))),
            (1.95, rod_key(seat, PISTOL.barrel_y, 0.008, (14, -14, -1), (14, 6, 0))),
            (2.1, rod_key(seat + 0.05, PISTOL.barrel_y, -0.006, (11, -15, 0), (10, 7, 0))),
            (2.25, rod_key(seat, PISTOL.barrel_y, 0.012, (15, -14, -1), (15, 5, 0))),
            (2.54, rod_key(pulled, PISTOL.barrel_y, -0.09, (11, -29, 3), (-3, 14, 0))),
            (2.68, rod_key(pulled, PISTOL.rod_y, -0.09, (11, -29, 3), (0, 14, 0))),
            (2.92, rod_key(rest_end, PISTOL.rod_y, -0.01, (14, -11, 0), (13, 6, 0))),
            (3.12, ready(at_work(lift=-0.05), palm_on((-0.25, 1.04, -0.14), (0.5, -0.72, -0.48), (0.7, 0.5, 0.5), curl=0.3), (8, -3, 0), (6, 1, 0))),
            (3.5, hold()),
        ],
    )
    print(f"pistola: at {PISTOL.at.round(4).tolist()}, turn (0, 90, {PISTOL.slope}), rod length {PISTOL.rod_to + 0.0 - PISTOL.rod_from:.3f}, along {along}")


# ---------------------------------------------------------------- musket

MUSKET = Gun("mosquete", grip=(-0.06, -0.018, 0.0), slope=25.0, muzzle=(1.007, 0.044, 0.0), rod_y=-0.005, rod_from=0.197, rod_to=0.994, barrel_y=0.044)
BUTT = (-0.4, -0.07, 0.0)
SHOULDER = np.array([0.14, 1.41, 0.17])
M_AIM_TORSO, M_AIM_HEAD = (4, -30, 0), (9, 26, 10)
M_ELBOW_AIM = (1.0, -0.3, 0.15)
M_HOLD = 3.6


def musket_aim(up=0.0, back=0.0, rise=0.0):
    pitch = rot_x(up)
    return Pose(
        MUSKET,
        pitch @ np.array([0, 0, -1.0]),
        up=pitch @ np.array([0, 1.0, 0]),
        anchor=BUTT,
        anchor_at=SHOULDER + np.array([0, rise, back]),
    )


def musket_support(gun, curl=0.85, at=0.22, elbow=(-0.5, -1.0, 0.1)):
    return palm_on(
        gun.point(at, -0.022),
        gun.file_to_body @ np.array([0.45, 0.0, 0.9]),
        gun.file_to_body @ np.array([0, 1.0, 0]),
        curl=curl,
        elbow=elbow,
    )


def musket_hold():
    # Held like a rifle at port arms: across the chest, the muzzle up by the left shoulder and the
    # right hand on the wrist of the stock at the belly. Found by a search on the measured wrists.
    return Pose(MUSKET, (-0.4194, 0.699, -0.5792), up=(0.1707, 0.6874, 0.706), grip_at=(0.1, 1.15, -0.17))


def musket_port_support(gun, curl=0.85):
    """The left hand on the side of the fore-end, palm against it."""
    return palm_on(
        gun.point(0.3, -0.022),
        gun.file_to_body @ np.array([0.45, 0.0, 0.9]),
        gun.file_to_body @ np.array([0.0, 0.0, 1.0]),
        curl=curl,
        elbow=(-0.7, -0.8, 0.1),
    )


def write_musket():
    def port(t=0.0, torso=(5, -24, 0), head=(0, 20, 0)):
        breathing, sway, nod = breath(t, M_HOLD)
        gun = moved(musket_hold(), (0.003 * sway, 0.006 * breathing, -0.004 * breathing), rot_y(1.2 * sway))
        torso = (torso[0] + 1.2 * breathing, torso[1] + 1.0 * sway, torso[2] + 0.6 * sway)
        head = (head[0] + 0.8 * sway, head[1] - 2.5 * nod, head[2])
        return key(gun, torso, head, (0.7, -0.7, -0.1), musket_port_support(gun))

    def aim(up=0.0, back=0.0, rise=0.0, torso=M_AIM_TORSO, head=M_AIM_HEAD):
        gun = musket_aim(up, back, rise)
        return key(gun, torso, head, M_ELBOW_AIM, musket_support(gun))

    write_clip("musket_hold", M_HOLD, True, False, [(t, port(t)) for t in idle_times(M_HOLD)])
    write_clip(
        "musket_aim",
        3.0,
        True,
        False,
        [
            (0.0, aim()),
            (1.5, aim(up=0.3, rise=0.004, torso=(4.7, -30.3, 0), head=(8.7, 26.2, 10))),
            (3.0, aim()),
        ],
    )
    # No springs here: the left hand's would slide it along the stock with the kick.
    write_clip(
        "musket_fire",
        0.9,
        False,
        False,
        [
            (0.0, aim()),
            (0.03, aim(up=5.5, back=0.075, rise=0.02, torso=(-7, -35, -2), head=(1, 30, 11))),
            (0.13, aim(up=5.5, back=0.06, rise=0.02, torso=(-8, -35, -2), head=(0, 30, 11))),
            (0.42, aim(up=1.5, back=0.012, torso=(1, -31, 0))),
            (0.6, aim(up=-0.5, torso=(4.5, -29.5, 0))),
            (0.9, aim()),
        ],
    )

    # On the ground, outside the right foot, with the hand on the barrel at the height of the elbow,
    # so that the forearm crosses the barrel and the wrist stays straight. It leans in, for the
    # free hand to reach over the muzzle. Only the butt stays still: the rest gives with each push.
    staff_at = (0.72, 0.03, 0.0)
    base = Pose(MUSKET, (-0.3, 0.92, -0.25), up=(0.0, 0.25, -0.97), grip_at=(0.14, 1.09, -0.3), staff=staff_at)
    butt_at = base.point(*BUTT)

    def rocked(forward=0.0, side=0.0):
        return moved(base, turn=rot_x(-forward) @ rot_z(side), pivot=butt_at)

    def ready(gun, left, torso, head, elbow=(0.7, -0.35, 0.7)):
        return key(gun, torso, head, elbow, left)

    muzzle_x = MUSKET.muzzle[0]
    rest = MUSKET.rod_from
    rod_y, bore_y = MUSKET.rod_y, MUSKET.barrel_y
    # (seconds, end of the rod along the gun, where the fist is on it, across the gun).
    rod = [
        (1.9, rest, 0.773, rod_y),
        (2.2, 0.55, 0.773, rod_y),
        (2.4, 0.55, 0.3, rod_y),
        (2.7, 1.02, 0.3, rod_y),
        (2.82, 1.02, 0.3, bore_y),
        (3.05, 0.7, 0.3, bore_y),
        (3.22, 0.7, 0.55, bore_y),
        (3.5, 0.4, 0.55, bore_y),
        (3.64, 0.52, 0.55, bore_y),
        (3.78, 0.4, 0.55, bore_y),
        (4.1, 0.8, 0.55, bore_y),
        (4.25, 0.8, 0.3, bore_y),
        (4.5, 1.02, 0.3, bore_y),
        (4.62, 1.02, 0.3, rod_y),
        (4.95, rest, 0.773, rod_y),
    ]
    pouch = lambda curl: palm_on((-0.23, 0.98, -0.12), (0.25, -0.95, -0.1), (0.9, 0.2, 0.3), curl=curl)

    def over(gun, curl, above=0.06):
        return palm_on(gun.point(muzzle_x + above, bore_y), OVER_FINGERS, OVER_PALM, at=FINGERTIPS, curl=curl)

    # The body bends down to the fist low on the rod, and straightens to the fist up high.
    low, high = min(e + g for _, e, g, _ in rod), max(e + g for _, e, g, _ in rod)

    def on_rod(end, grip, across):
        height = (end + grip - low) / (high - low)
        # A push rocks the barrel away, a pull brings it back.
        gun = rocked(forward=1.5 - 3.0 * height, side=1.0 * height)
        torso = (20 - 12 * height, -22 + 5 * height, 3 * height - 1)
        head = (12 - 22 * height, 18 + 3 * height, 0)
        return ready(gun, fist_on(gun.point(end + grip, across), gun.along(), towards=P_ROD_TOWARDS, elbow=P_ROD_ELBOW), torso, head)

    lean_in = rocked(forward=-1.0)
    keys = [
        (0.0, port()),
        (0.45, ready(rocked(), hanging(), (9, -14, -2), (6, 10, 0))),
        (0.75, ready(rocked(forward=0.5), pouch(0.2), (18, -8, -6), (20, 14, 0))),
        (0.95, ready(rocked(forward=0.8), pouch(0.9), (20, -7, -7), (22, 15, 0))),
        (1.2, ready(rocked(forward=-0.5), palm_on((-0.08, 1.3, -0.27), (0.45, 0.35, -0.8), (0.55, 0.65, 0.5), curl=0.9), (12, -18, 0), (2, 16, 0))),
        (1.45, ready(lean_in, over(lean_in, 0.75), (8, -22, 2), (-10, 20, 0))),
        (1.58, ready(rocked(forward=-1.3), over(lean_in, 0.35, 0.07), (9, -22, 2), (-8, 20, 0))),
        (1.85, ready(rocked(), fist_on(base.point(rest + 0.773, rod_y), base.along(), towards=P_ROD_TOWARDS, elbow=P_ROD_ELBOW), (14, -21, 1), (2, 18, 0))),
    ]
    for time, end, grip, across in rod:
        keys.append((time, on_rod(end, grip, across)))
    leaving = rocked(forward=0.5)
    keys.extend(
        [
            (5.1, ready(leaving, palm_on(leaving.point(0.9, 0.0), (0.45, -0.1, -0.85), (0.8, 0.5, 0.3), curl=0.7), (10, -15, 0), (4, 12, 0))),
            (5.5, port(torso=(6, -23, 0))),
            (5.9, port()),
        ]
    )
    write_clip("musket_reload", 5.9, False, False, keys)
    staff_at_joint = -base.gun.staff @ np.array(staff_at)
    grips = ", ".join(f"({t}, {g})" for t, _, g, _ in rod)
    print(f"mosquete: at {MUSKET.at.round(4).tolist()}, turn (0, 90, {MUSKET.slope})")
    print(f"  staff at {staff_at_joint.round(4).tolist()}, turn (0, -90, 90)")
    print(f"  rod_grip: [{grips}]")


if __name__ == "__main__":
    write_stand()
    write_pistol()
    write_musket()
    left = np.linalg.inv(JOINT)
    palm = left @ (F0 * PALM[0] + N0 * PALM[1])
    tips = left @ (F0 * FINGERTIPS[0] + N0 * FINGERTIPS[1])
    print(f"hand.L: palm {palm.round(4).tolist()}, fingertips {tips.round(4).tolist()}, rod axis {(left @ T0).round(4).tolist()}")
