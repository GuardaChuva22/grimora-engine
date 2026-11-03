"""Authoring animation in text: poses described by what they do, solved by IK.

A clip is a list of keys, each naming only the controls that have a key at that
moment; a control between its keys follows a monotone cubic, so it eases into stops.
Keys for different controls at different times are how overlap is written, and on
top of that `SPRINGS` makes the body follow the keys instead of being on them: it
trails a little, overshoots where a move stops and settles. That is most of the
difference between poses being interpolated and a body with weight. Feet and the
root aren't filtered, or they would slide. `sweep` writes a blow's arc.

All positions are in engine terms, in meters: x right, y up, the character looks
towards -z. Angles are degrees, as (pitch, yaw, roll): pitch leans forward (for a
foot: toes down), yaw turns left, roll leans to the character's right.

Controls:
    root      (x, z) where the character's root is. The engine moves the capsule by
              it, so the clip travels with no sliding; see `Animator::root_delta`.
    hips      offset from the standing hips; hips_rot its rotation
    torso     rotation over the hips, shared by spine and chest
    head      rotation over the chest, shared with the neck
    foot_l/r  ankle, in the clip's space: unlike the rest, it ignores `root`, so a
              foot with no new key stays planted on the ground
    foot_l_rot, foot_r_rot, toe_l, toe_r, knee_l, knee_r (a direction)
    hand_r    the middle of the grip; blade says where the blade points, and edge where
              its back edge does: the one towards the wrist, opposite the knuckles. With
              a straight wrist that's along the forearm, so a blade at right angles to
              it and an edge pointing back up it is the easy way to hold a sword.
              elbow_r is where the elbow points
    hand_l    the palm; elbow_l
    tuck      0..1 from those feet to tuck_feet in the hips' space, and from those
              hands to tuck_hand_* in the chest's space: for rolling

`report` measures what eyes would otherwise have to catch: read it after any change.
"""
import math

from mathutils import Matrix, Quaternion, Vector

FPS = 60
# How each control follows its keys, as (hertz, damping): the lower the hertz the later
# it arrives, and under 1 of damping it overshoots. The hips lead and the free arm floats.
SPRINGS = {"hips": (8.0, 0.6), "hips_rot": (10.0, 0.62), "torso": (7.0, 0.52), "head": (5.5, 0.62),
           "hand_r": (17.0, 0.6), "blade": (15.0, 0.55), "edge": (15.0, 0.55), "elbow_r": (9.0, 0.75),
           "hand_l": (4.2, 0.45), "elbow_l": (5.0, 0.65)}
SPRING_RATE = 240  # steps per second
THRUSTS = ("sword_3",)  # blows that go point first, not edge first
# Degrees from the rest pose that (fingers, fingertips, thumb, thumb across the palm) bend,
# closing around a grip or relaxed.
FIST = (26.0, 64.0, 46.0, 34.0)
OPEN = (8.0, 10.0, 6.0, 0.0)
# Where the blade runs along the weapon joint, in meters: see the engine's `Blade`.
BLADE = (0.11, 0.79)
STANDING = {"hips": (0, 0, 0), "hips_rot": (0, 0, 0), "torso": (0, 0, 0), "head": (0, 0, 0), "root": (0, 0),
            "knee_l": (-0.12, 0, -1), "knee_r": (0.12, 0, -1), "foot_l_rot": (0, 0, 0), "foot_r_rot": (0, 0, 0),
            "toe_l": 0.0, "toe_r": 0.0, "tuck": 0.0, "elbow_r": (0.35, -0.2, 0.9), "elbow_l": (-0.4, -0.2, 0.8),
            "tuck_feet": (0.1, -0.28, -0.32), "tuck_hand_r": (0.2, -0.16, -0.26), "tuck_hand_l": (-0.2, -0.16, -0.26)}


def rot(pitch=0.0, yaw=0.0, roll=0.0):
    """Rotation of an (pitch, yaw, roll) in degrees, in engine axes."""
    p, y, r = math.radians(pitch), math.radians(yaw), math.radians(roll)
    return Quaternion((0, 1, 0), y) @ Quaternion((1, 0, 0), -p) @ Quaternion((0, 0, 1), -r)


# Rest directions, in the hand: the blade across the palm, its back edge towards the wrist
# and a little to the palm's side. At rest the fingers point down the arm, 35 degrees out.
ARM = Vector((math.sin(math.radians(35)), -math.cos(math.radians(35)), 0.0))
PALM = Vector((-math.cos(math.radians(35)), -math.sin(math.radians(35)), 0.0))


def rest_frame(edge):
    return frame(Vector((0, 0, -1)), edge)


def to_blender_quat(q):
    return Quaternion((q.w, q.x, -q.z, q.y))


def frame(direction, pole):
    """A rotation matrix whose first column is `direction`, the second as near `pole` as can be."""
    d = direction.normalized()
    p = pole - d * pole.dot(d)
    if p.length < 1e-5:
        p = Vector((0, 0, 1)) - d * d.z
        if p.length < 1e-5:
            p = Vector((1, 0, 0)) - d * d.x
    p.normalize()
    return Matrix((d, p, d.cross(p))).transposed()


def aim(rest_dir, rest_pole, direction, pole):
    """The rotation that takes a limb at rest to `direction`, with its hinge towards `pole`."""
    return (frame(direction, pole) @ frame(rest_dir, rest_pole).transposed()).to_quaternion()


HAND_REST = rest_frame(-ARM + PALM * 0.3)
WEAPON_REST = rest_frame(Vector((0, 1, 0)))  # how the joint sits at rest: edges up and down
# The weapon joint's rotation on top of the hand's: it stays the same however the hand moves.
WEAPON_IN_HAND = (HAND_REST @ WEAPON_REST.transposed()).to_quaternion()


def finger_rotations(x, hand, curl):
    """The rotations of a hand's fingers from rest, given the hand's."""
    closing = -1 if x == "R" else 1
    fingers, tips, thumb = (Quaternion((0, 0, 1), math.radians(closing * angle)) for angle in curl[:3])
    palm = PALM if x == "R" else Vector((-PALM.x, PALM.y, 0.0))
    across = Quaternion(palm, math.radians(-closing * curl[3]))
    fingers = hand @ fingers
    return {f"fingers.{x}": fingers, f"fingertips.{x}": fingers @ tips, f"thumb.{x}": hand @ thumb @ across}


def hinge_poles(upper, lower, pole):
    """Poles for the two bones of a bent limb, from the axis it really bends around:
    the wanted `pole` can point right along a bone, and the bone would spin there."""
    axis = lower.cross(upper)
    if axis.length < 0.008:  # nearly straight, with no axis to speak of
        return pole, pole
    axis.normalize()
    return axis.cross(upper.normalized()), axis.cross(lower.normalized())


def two_bones(start, target, a, b, pole):
    """Where the middle joint goes. Returns it and how far short of the target the limb stops."""
    to = target - start
    distance = to.length
    reach = (a + b) * 0.999
    short = max(0.0, distance - reach)
    distance = max(min(distance, reach), abs(a - b) + 1e-3, 1e-3)
    d = to.normalized()
    along = (a * a - b * b + distance * distance) / (2 * distance)
    height = math.sqrt(max(0.0, a * a - along * along))
    side = pole - d * pole.dot(d)
    if side.length < 1e-5:
        side = Vector((0, 0, 1))
    return start + d * along + side.normalized() * height, short


class Track:
    """One control's keys: a monotone cubic through them, flat at both ends."""

    def __init__(self, keys):
        # Two keys at one moment: the last one counts.
        keys = list({t: (t, v) for t, v in keys}.values())
        self.times = [t for t, _ in keys]
        self.values = [tuple(v) if isinstance(v, (tuple, list)) else (v,) for _, v in keys]
        self.scalar = not isinstance(keys[0][1], (tuple, list))
        n = len(keys)
        self.tangents = []
        for k in range(n):
            tangent = []
            for c in range(len(self.values[0])):
                if k == 0 or k == n - 1:
                    tangent.append(0.0)
                    continue
                h0, h1 = self.times[k] - self.times[k - 1], self.times[k + 1] - self.times[k]
                s0 = (self.values[k][c] - self.values[k - 1][c]) / h0
                s1 = (self.values[k + 1][c] - self.values[k][c]) / h1
                if s0 * s1 <= 0:
                    tangent.append(0.0)
                else:
                    w0, w1 = 2 * h1 + h0, h1 + 2 * h0
                    tangent.append((w0 + w1) / (w0 / s0 + w1 / s1))
            self.tangents.append(tangent)

    def at(self, t):
        times = self.times
        if t <= times[0]:
            out = self.values[0]
        elif t >= times[-1]:
            out = self.values[-1]
        else:
            k = next(i for i in range(len(times) - 1) if t < times[i + 1])
            h = times[k + 1] - times[k]
            u = (t - times[k]) / h
            h00, h10 = (1 + 2 * u) * (1 - u) ** 2, u * (1 - u) ** 2
            h01, h11 = u * u * (3 - 2 * u), u * u * (u - 1)
            out = tuple(h00 * a + h10 * h * ma + h01 * b + h11 * h * mb for a, b, ma, mb in zip(self.values[k], self.values[k + 1], self.tangents[k], self.tangents[k + 1]))
        return out[0] if self.scalar else out


class Clip:
    """A clip: `sample(t)` gives its controls at `t` seconds."""

    def __init__(self, name, duration, sample, events=(), loop=False):
        self.name, self.duration, self.sample, self.loop = name, duration, sample, loop
        self.events = sorted(events)


def follow(values, hertz, damping):
    """What a damped spring does chasing `values`, tuples at SPRING_RATE."""
    k1 = damping / (math.pi * hertz)
    k2 = 1 / (2 * math.pi * hertz) ** 2
    dt = 1 / SPRING_RATE
    y, v = list(values[0]), [0.0] * len(values[0])
    out = []
    for x in values:
        for c in range(len(y)):
            y[c] += dt * v[c]
            v[c] += dt * (x[c] - y[c] - k1 * v[c]) / k2
        out.append(tuple(y))
    return out


def table_at(table, t):
    at = max(0.0, min(len(table) - 1.0, t * SPRING_RATE))
    k = min(int(at), len(table) - 2)
    u = at - k
    return tuple(a + (b - a) * u for a, b in zip(table[k], table[k + 1]))


def keyed(name, duration, keys, events=()):
    """A clip from `[(seconds, {control: value})]`. The first key must set everything it uses."""
    written = {}
    for t, pose in sorted(keys, key=lambda key: key[0]):
        for control, value in pose.items():
            written.setdefault(control, []).append((t, value))
    tracks = {control: Track(k) for control, k in written.items()}
    steps = round(duration * SPRING_RATE) + 2
    tables = {control: follow([track.at(k / SPRING_RATE) for k in range(steps)], *SPRINGS[control])
              for control, track in tracks.items() if control in SPRINGS}

    def sample(t):
        out = dict(STANDING)
        for control, track in tracks.items():
            out[control] = table_at(tables[control], t) if control in tables else track.at(t)
        return out

    clip = Clip(name, duration, sample, events)
    clip.written = written
    return clip


def settled(clip):
    """A looping clip with `SPRINGS` on it, once they've gone round enough to repeat."""
    steps = round(clip.duration * SPRING_RATE)
    samples = [clip.sample(k / SPRING_RATE) for k in range(steps)]
    tables = {}
    for control, spring in SPRINGS.items():
        values = [tuple(s[control]) for s in samples]
        tables[control] = follow(values * 4, *spring)[steps * 3:]

    def sample(t):
        out = clip.sample(t)
        for control, table in tables.items():
            out[control] = table[round((t % clip.duration) * SPRING_RATE) % steps]
        return out

    out = Clip(clip.name, clip.duration, sample, clip.events, loop=True)
    out.unfiltered = clip
    return out


def sweep(t0, t1, start, via, end, steps=10, late=1.5):
    """Keys for a blow's arc, from `start` through near `via` to `end`, each one (hand, blade,
    edge): the hand on a smooth curve and the blade turning steadily, which interpolating
    between a few keys doesn't give. `late` above 1 keeps it slow for longer and fastest
    near the end, as a cut is."""
    def turn(pose):
        return frame(Vector(pose[1]), Vector(pose[2])).to_quaternion()

    q0, qv, q1 = turn(start), turn(via), turn(end)
    if q0.dot(qv) < 0:
        qv = -qv
    if qv.dot(q1) < 0:
        q1 = -q1
    a, b, c = Vector(start[0]), Vector(via[0]), Vector(end[0])
    b = 2 * b - (a + c) / 2  # so the curve really goes through `via`
    keys = []
    for k in range(steps + 1):
        s = (k / steps) ** late
        u = s * s * (3 - 2 * s)
        q = q0.slerp(qv, u).slerp(qv.slerp(q1, u), u)
        hand = a * (1 - u) ** 2 + b * 2 * u * (1 - u) + c * u * u
        keys.append((t0 + (t1 - t0) * k / steps, {"hand_r": tuple(hand), "blade": tuple(q @ Vector((1, 0, 0))), "edge": tuple(q @ Vector((0, 1, 0)))}))
    return keys


# How far a key can be dropped from when exporting, by the kind of value it holds.
EXPORT_ERROR = {"root": 0.002, "hips": 0.002, "foot_l": 0.0012, "foot_r": 0.0012, "hand_r": 0.003, "hand_l": 0.005,
                "blade": 0.006, "edge": 0.01, "elbow_r": 0.02, "elbow_l": 0.03, "knee_l": 0.02, "knee_r": 0.02, "tuck": 0.005}
EXPORT_ANGLE_ERROR = 0.35


def as_triple(control, value):
    """A control's value as the three numbers the engine's format holds."""
    if isinstance(value, (int, float)):
        return (float(value), 0.0, 0.0)
    if control == "root":
        return (float(value[0]), 0.0, float(value[1]))
    return tuple(float(x) for x in value)


def fewest_keys(control, keys, duration):
    """As few of `keys` as keep a curve through them this close to the one through them all."""
    full = Track(keys)
    tolerance = EXPORT_ERROR.get(control, EXPORT_ANGLE_ERROR)
    moments = [k / SPRING_RATE for k in range(round(duration * SPRING_RATE) + 1)]
    wanted = [full.at(t) for t in moments]
    kept = {0, len(keys) - 1}
    while len(kept) < len(keys):
        chosen = [keys[k] for k in sorted(kept)]
        curve = Track(chosen)
        worst, at = 0.0, 0.0
        for t, value in zip(moments, wanted):
            off = max(abs(a - b) for a, b in zip(curve.at(t), value))
            if off > worst:
                worst, at = off, t
        if worst <= tolerance:
            break
        kept.add(min((k for k in range(len(keys)) if k not in kept), key=lambda k: abs(keys[k][0] - at)))
    return [keys[k] for k in sorted(kept)]


def export_clip(clip, folder):
    """Writes a clip as the engine's text format: from then on the file is the clip."""
    if clip.loop:
        # Written as functions of time: a key for every frame to choose from, before the springs.
        raw = clip.unfiltered
        frames = round(clip.duration * FPS)
        samples = [(k * clip.duration / frames, raw.sample(k * clip.duration / frames)) for k in range(frames + 1)]
        written = {control: [(t, c[control]) for t, c in samples] for control in samples[0][1] if not control.startswith("tuck")}
    else:
        written = clip.written
    lines = ["(", f"    duration: {clip.duration:.4f},"]
    if clip.loop:
        lines.append("    looping: true,")
    lines.append("    tracks: {")
    for control in sorted(written):
        keys = [(t, as_triple(control, v)) for t, v in written[control]]
        keys = list({round(t, 4): (round(t, 4), v) for t, v in sorted(keys, key=lambda key: key[0])}.values())
        lines.append(f"        {control}: [")
        for t, v in fewest_keys(control, keys, clip.duration) if len(keys) > 2 else keys:
            lines.append(f"            ({t:.4f}, ({v[0]:.4f}, {v[1]:.4f}, {v[2]:.4f})),")
        lines.append("        ],")
    lines.append("    },")
    if clip.events:
        lines.append("    events: [")
        lines += [f'        ({t:.4f}, "{name}"),' for t, name in clip.events]
        lines.append("    ],")
    lines.append(")")
    with open(f"{folder}/{clip.name}.ron", "w", encoding="utf-8", newline="\n") as file:
        file.write("\n".join(lines) + "\n")


class Rig:
    """The skeleton's rest pose, in engine space, and the solver from controls to bones."""

    def __init__(self, bones):
        self.parents = {name: b[0] for name, b in bones.items()}
        self.head = {name: Vector(b[1]) for name, b in bones.items()}
        self.tail = {name: Vector(b[2]) for name, b in bones.items()}

    def length(self, name):
        return (self.tail[name] - self.head[name]).length

    def rest_dir(self, name):
        return (self.tail[name] - self.head[name]).normalized()

    def from_parent(self, name, delta, position):
        parent = self.parents[name]
        return position[parent] + delta[parent] @ (self.head[name] - self.head[parent])

    def solve(self, c, root=None):
        """Every bone's rotation from rest (engine axes) and head position, in root space, from controls."""
        delta, position = {"root": Quaternion()}, {"root": Vector()}
        info = {}
        delta["hips"] = rot(*c["hips_rot"])
        position["hips"] = self.head["hips"] + Vector(c["hips"])
        torso = rot(*c["torso"])
        half = Quaternion().slerp(torso, 0.45)
        delta["spine"] = delta["hips"] @ half
        delta["chest"] = delta["hips"] @ torso
        position["spine"] = self.from_parent("spine", delta, position)
        position["chest"] = self.from_parent("chest", delta, position)
        head = rot(*c["head"])
        delta["neck"] = delta["chest"] @ Quaternion().slerp(head, 0.4)
        delta["head"] = delta["chest"] @ head
        position["neck"] = self.from_parent("neck", delta, position)
        position["head"] = self.from_parent("head", delta, position)
        tuck = max(0.0, min(1.0, c["tuck"]))
        root_at = Vector((c["root"][0], 0.0, c["root"][1]))

        # Legs, from the ankle back up.
        for x, sign in (("L", -1), ("R", 1)):
            lower = x.lower()
            target = Vector(c[f"foot_{lower}"]) - root_at
            if tuck > 0:
                offset = Vector(c["tuck_feet"])
                offset.x = sign * abs(offset.x)
                target = target.lerp(position["hips"] + delta["hips"] @ offset, tuck)
            thigh = f"thigh.{x}"
            shin = f"shin.{x}"
            foot = f"foot.{x}"
            toe = f"toe.{x}"
            position[thigh] = self.from_parent(thigh, delta, position)
            pole = delta["hips"] @ Vector(c[f"knee_{lower}"]) if tuck > 0.5 else Vector(c[f"knee_{lower}"])
            knee, short = two_bones(position[thigh], target, self.length(thigh), self.length(shin), pole)
            info[f"short_foot_{lower}"] = short
            rest_pole = Vector((0, 0, -1))
            ankle = knee + (target - knee).normalized() * self.length(shin)
            pole_a, pole_b = hinge_poles(knee - position[thigh], ankle - knee, pole)
            delta[thigh] = aim(self.rest_dir(thigh), rest_pole, knee - position[thigh], pole_a)
            position[shin] = knee
            delta[shin] = aim(self.rest_dir(shin), rest_pole, ankle - knee, pole_b)
            position[foot] = ankle
            base = delta["hips"] if tuck > 0.5 else Quaternion()
            delta[foot] = base @ rot(*c[f"foot_{lower}_rot"])
            position[toe] = self.from_parent(toe, delta, position)
            delta[toe] = delta[foot] @ rot(pitch=-c[f"toe_{lower}"])
            info[f"ankle_{lower}"] = ankle

        # Arms: the hand decides where the wrist goes, and the elbow follows.
        chest = delta["chest"]
        chest_at = position["chest"]
        for x, sign, target_name in (("R", 1, "hand_r"), ("L", -1, "hand_l")):
            lower = x.lower()
            palm = Vector(c[target_name])
            if tuck > 0:
                palm = palm.lerp(chest_at + chest @ Vector(c[f"tuck_{target_name}"]), tuck)
            hand, forearm, upper, clavicle = f"hand.{x}", f"forearm.{x}", f"upper_arm.{x}", f"clavicle.{x}"
            wrist_offset = self.head["weapon.R" if x == "R" else f"fingers.{x}"] - self.head[hand]
            if x == "R":
                blade = Vector(c["blade"]).normalized()
                edge = Vector(c["edge"])
                if tuck > 0:
                    blade = blade.lerp(chest @ Vector((0.25, 0.15, 0.95)), tuck).normalized()
                    edge = edge.lerp(chest @ Vector((0, 1, -0.1)), tuck)
                held = frame(blade, edge)
                hand_delta = (held @ HAND_REST.transposed()).to_quaternion()
                delta["weapon.R"] = (held @ WEAPON_REST.transposed()).to_quaternion()
                wrist = palm - hand_delta @ wrist_offset
            else:
                wrist = palm  # a loose hand just continues the forearm
                hand_delta = None
            # The collar bone gives a fifth of the way towards the hand.
            position[clavicle] = chest_at + chest @ (self.head[clavicle] - self.head["chest"])
            shoulder = position[clavicle] + chest @ (self.head[upper] - self.head[clavicle])
            rest = chest @ self.rest_dir(upper)
            pull = rest.rotation_difference((wrist - shoulder).normalized())
            delta[clavicle] = Quaternion().slerp(pull, 0.18) @ chest
            position[upper] = position[clavicle] + delta[clavicle] @ (self.head[upper] - self.head[clavicle])
            pole = Vector(c[f"elbow_{lower}"])
            elbow, short = two_bones(position[upper], wrist, self.length(upper), self.length(forearm), pole)
            info[f"short_hand_{lower}"] = short
            rest_pole = Vector((0, 0, 1))
            at_wrist = elbow + (wrist - elbow).normalized() * self.length(forearm)
            pole_a, pole_b = hinge_poles(elbow - position[upper], at_wrist - elbow, pole)
            delta[upper] = aim(self.rest_dir(upper), rest_pole, elbow - position[upper], pole_a)
            position[forearm] = elbow
            delta[forearm] = aim(self.rest_dir(forearm), rest_pole, at_wrist - elbow, pole_b)
            position[hand] = at_wrist
            if hand_delta is None:
                hand_delta = delta[forearm]
            else:
                # The forearm takes half of the hand's roll around its own axis.
                axis = (at_wrist - elbow).normalized()
                turn = delta[forearm].inverted() @ hand_delta
                if turn.w < 0:
                    turn = -turn
                local = delta[forearm].inverted() @ axis
                twist = Quaternion((turn.w, *(local * Vector(turn[1:]).dot(local)))).normalized()
                # Near half a turn the direction of the twist is a coin toss: leave it to the wrist.
                angle = 2 * math.degrees(math.acos(min(1.0, abs(twist.w))))
                delta[forearm] = delta[forearm] @ Quaternion().slerp(twist, 0.5 * max(0.0, min(1.0, (150 - angle) / 30)))
                info["twist"] = angle
                info["wrist"] = math.degrees((hand_delta @ self.rest_dir(hand)).angle(axis))
            delta[hand] = hand_delta
            delta.update(finger_rotations(x, hand_delta, FIST if x == "R" else OPEN))
        position["weapon.R"] = self.from_parent("weapon.R", delta, position)
        delta["lantern"] = delta["hips"]
        position["lantern"] = self.from_parent("lantern", delta, position)
        for name, parent in self.parents.items():
            # Joints waiting for cloth, with nothing on them yet.
            if name not in delta:
                delta[name] = delta[parent]
                position[name] = self.from_parent(name, delta, position)
        return delta, position, info


def local_rotations(rig, rest, delta, previous):
    """Each bone's pose rotation, as Blender wants it. `rest` holds armature-space rest rotations."""
    out = {}
    for name, parent in rig.parents.items():
        turned = to_blender_quat(delta[name])
        above = to_blender_quat(delta[parent]) if parent else Quaternion()
        q = rest[name].inverted() @ above.inverted() @ turned @ rest[name]
        if name in previous and previous[name].dot(q) < 0:
            q = -q
        previous[name] = q
        out[name] = q
    return out


def segment_distance(a0, a1, b0, b1):
    """Shortest distance between two segments."""
    d1, d2, r = a1 - a0, b1 - b0, a0 - b0
    aa, ee, f = d1.dot(d1), d2.dot(d2), d2.dot(r)
    c = d1.dot(r)
    b = d1.dot(d2)
    denominator = aa * ee - b * b
    s = max(0.0, min(1.0, (b * f - c * ee) / denominator)) if denominator > 1e-9 else 0.0
    t = (b * s + f) / ee
    if t < 0:
        t, s = 0.0, max(0.0, min(1.0, -c / aa))
    elif t > 1:
        t, s = 1.0, max(0.0, min(1.0, (b - c) / aa))
    return ((a0 + d1 * s) - (b0 + d2 * t)).length


def trace(rig, clip, every=0.03):
    """The sword arm over time, to find out which pose a bad figure in `report` comes from."""
    before = None
    for k in range(round(clip.duration / every) + 1):
        t = k * every
        c = clip.sample(t)
        delta, position, info = rig.solve(c)
        grip = position["weapon.R"]
        tip = grip + delta["weapon.R"] @ Vector((0, 0, -1)) * BLADE[1]
        speed = (tip - before).length / every if before else 0.0
        before = tip
        print(f"  {t:.2f} wrist {info['wrist']:3.0f} twist {info['twist']:3.0f} tip {speed:4.1f}m/s  hand ({grip.x:5.2f},{grip.y:5.2f},{grip.z:5.2f}) short {info['short_hand_r'] * 100:3.0f}cm")


def report(rig, clip):
    """One line per clip of what can go wrong without anyone looking."""
    frames = max(1, round(clip.duration * FPS))
    tip_before = None
    fastest = (0.0, 0.0)
    clearance = (9.0, 0.0, "")
    short = {"foot": 0.0, "hand": 0.0}
    wrist = (0.0, 0.0)
    twist = (0.0, 0.0)
    cut = (0.0, 0.0)
    hurting = False
    moments = dict((name, at) for at, name in clip.events)
    elbow = 180.0
    pop = (0.0, "", 0.0)
    before = {}
    slide = 0.0
    feet_before = {}
    armed = clip.name.startswith("sword_")
    for k in range(frames + 1):
        t = k / FPS
        c = clip.sample(t)
        delta, position, info = rig.solve(c)
        root = Vector((c["root"][0], 0.0, c["root"][1]))
        for part in short:
            for side in "lr":
                short[part] = max(short[part], info[f"short_{part}_{side}"])
        for name, q in delta.items():
            if name in before:
                angle = math.degrees(before[name].rotation_difference(q).angle)
                angle = min(angle, 360 - angle)  # q and -q are the same turn
                if angle > pop[0] and c["tuck"] < 0.01:
                    pop = (angle, name, t)
            before[name] = q
        for side in "lr":
            ankle = Vector(c[f"foot_{side}"])
            if side in feet_before and ankle.y < 0.1 and feet_before[side].y < 0.1 and c["tuck"] < 0.01:
                reached = info[f"ankle_{side}"] + root
                slide = max(slide, (reached - ankle).length)
            feet_before[side] = ankle
        upper = position["hand.R"] - position["forearm.R"]
        lower = position["forearm.R"] - position["upper_arm.R"]
        elbow = min(elbow, 180 - math.degrees(upper.angle(lower)))
        if c["tuck"] < 0.01 and armed:
            wrist = max(wrist, (info["wrist"], t))
            twist = max(twist, (info["twist"], t))
        if not armed:
            continue
        grip = position["weapon.R"]
        along = delta["weapon.R"] @ Vector((0, 0, -1))
        base, tip = grip + along * BLADE[0], grip + along * BLADE[1]
        if tip_before is not None and (tip - tip_before).length * FPS > fastest[0]:
            fastest = ((tip - tip_before).length * FPS, t)
        hurting = "hit_on" in moments and moments["hit_on"] <= t <= moments["hit_off"]
        if tip_before is not None and hurting and (tip - tip_before).length * FPS > 6.0:
            # A cut goes edge first: how far off, with either edge counting.
            flat = abs((delta["weapon.R"] @ Vector((0, 1, 0))).dot((tip - tip_before).normalized()))
            if clip.name not in THRUSTS:
                cut = max(cut, (math.degrees(math.acos(min(1.0, flat))), t))
        tip_before = tip
        if c["tuck"] > 0.01:
            continue
        parts = (("torso", position["hips"], position["neck"], 0.17), ("head", position["head"], position["head"] + delta["head"] @ Vector((0, 0.2, 0)), 0.12),
                 ("thigh.L", position["thigh.L"], position["shin.L"], 0.08), ("thigh.R", position["thigh.R"], position["shin.R"], 0.08))
        for name, p0, p1, radius in parts:
            gap = segment_distance(base, tip, p0, p1) - radius
            if gap < clearance[0]:
                clearance = (gap, t, name)
    line = f"[anim] {clip.name:16} {clip.duration:.2f}s  short hand {short['hand'] * 100:4.1f}cm foot {short['foot'] * 100:4.1f}cm  slide {slide * 100:4.1f}cm  elbow>={elbow:3.0f}  pop {pop[0]:3.0f}deg/f {pop[1]}@{pop[2]:.2f}"
    if armed:
        line += f"  wrist {wrist[0]:3.0f}@{wrist[1]:.2f} twist {twist[0]:3.0f}@{twist[1]:.2f} flat-cut {cut[0]:3.0f}@{cut[1]:.2f}  tip {fastest[0]:4.1f}m/s@{fastest[1]:.2f}  blade-body {clearance[0] * 100:5.1f}cm {clearance[2]}@{clearance[1]:.2f}"
    print(line)
