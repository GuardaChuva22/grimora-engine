"""Generates assets/models/wanderer: the hooded wanderer, rigged and animated.

A low-poly figure (1.85 m, facing -Z) with a 17-joint skeleton and four
looping clips: idle, walk, run and jump. It carries a staff in the right hand
and a lantern on its own joint in the left, and has glowing eyes in the hood.
Everything here is original and part of the project. Standard library only:

    python tools/gen_wanderer.py
"""

import json
import math
import os
import struct

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets", "models", "wanderer")

# ---------------------------------------------------------------- skeleton
# name, parent, rest position in model space (left = -X, since it faces -Z)
JOINTS = [
    ("hips", None, (0.0, 0.95, 0.0)),
    ("spine", "hips", (0.0, 1.10, 0.0)),
    ("chest", "spine", (0.0, 1.28, 0.0)),
    ("head", "chest", (0.0, 1.50, 0.0)),
    ("upper_arm.L", "chest", (-0.21, 1.43, 0.0)),
    ("forearm.L", "upper_arm.L", (-0.24, 1.17, 0.0)),
    ("hand.L", "forearm.L", (-0.26, 0.93, 0.0)),
    ("lantern", "hand.L", (-0.26, 0.86, 0.0)),
    ("upper_arm.R", "chest", (0.21, 1.43, 0.0)),
    ("forearm.R", "upper_arm.R", (0.24, 1.17, 0.0)),
    ("hand.R", "forearm.R", (0.26, 0.93, 0.0)),
    ("thigh.L", "hips", (-0.10, 0.92, 0.0)),
    ("shin.L", "thigh.L", (-0.10, 0.50, 0.0)),
    ("foot.L", "shin.L", (-0.10, 0.09, 0.0)),
    ("thigh.R", "hips", (0.10, 0.92, 0.0)),
    ("shin.R", "thigh.R", (0.10, 0.50, 0.0)),
    ("foot.R", "shin.R", (0.10, 0.09, 0.0)),
]
J = {name: i for i, (name, _, _) in enumerate(JOINTS)}
POS = {name: p for name, _, p in JOINTS}


def srgb(r, g, b):
    """glTF vertex colors are linear; the palette is picked in sRGB."""
    return tuple(c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4 for c in (r, g, b))


CLOAK = srgb(0.36, 0.11, 0.10)
HOOD = srgb(0.30, 0.09, 0.09)
TUNIC = srgb(0.20, 0.16, 0.14)
TROUSERS = srgb(0.16, 0.14, 0.13)
LEATHER = srgb(0.22, 0.15, 0.10)
BOOTS = srgb(0.12, 0.09, 0.07)
WOOD = srgb(0.30, 0.22, 0.15)
SHADOW = srgb(0.02, 0.015, 0.015)
WHITE = (1.0, 1.0, 1.0)

CLOTH, METAL, EYES, GLOW = range(4)


# ---------------------------------------------------------------- vector math

def add(a, b):
    return tuple(x + y for x, y in zip(a, b))


def sub(a, b):
    return tuple(x - y for x, y in zip(a, b))


def mul(a, s):
    return tuple(x * s for x in a)


def dot(a, b):
    return sum(x * y for x, y in zip(a, b))


def cross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def norm(a):
    length = math.sqrt(dot(a, a))
    return mul(a, 1 / length) if length > 1e-9 else (0.0, 1.0, 0.0)


def lerp(a, b, t):
    return tuple(x + (y - x) * t for x, y in zip(a, b))


# ---------------------------------------------------------------- geometry

class Prim:
    def __init__(self):
        self.pos, self.nrm, self.col, self.joints, self.weights, self.idx = [], [], [], [], [], []


PRIMS = [Prim() for _ in range(4)]


def skin(weights):
    """{joint name: weight} -> four joint indices and normalized weights."""
    items = sorted(weights.items(), key=lambda kv: -kv[1])[:4]
    total = sum(w for _, w in items)
    joints = [J[n] for n, _ in items] + [0] * (4 - len(items))
    ws = [w / total for _, w in items] + [0.0] * (4 - len(items))
    return joints, ws


def tri(material, a, b, c, color, wa, wb, wc, inside):
    """Flat-shaded triangle, flipped if needed so it faces away from `inside`."""
    n = cross(sub(b, a), sub(c, a))
    if dot(n, sub(mul(add(add(a, b), c), 1 / 3), inside)) < 0:
        b, c, wb, wc = c, b, wc, wb
        n = mul(n, -1)
    n = norm(n)
    p = PRIMS[material]
    for v, w in ((a, wa), (b, wb), (c, wc)):
        joints, ws = skin(w)
        p.idx.append(len(p.pos))
        p.pos.append(v)
        p.nrm.append(n)
        p.col.append(color)
        p.joints.append(joints)
        p.weights.append(ws)


def quad(material, a, b, c, d, color, w, inside):
    tri(material, a, b, c, color, w[0], w[1], w[2], inside)
    tri(material, a, c, d, color, w[0], w[2], w[3], inside)


def prism(a, b, ra, rb, sides, color, wa, wb=None, squash=(1.0, 1.0), material=CLOTH, turn=0.0):
    """Tapered n-gon tube from `a` to `b` with caps. `wa`/`wb` are the joint
    weights of each end, either a dict or a function of the vertex position."""
    wb = wb if wb is not None else wa
    axis = norm(sub(b, a))
    ref = (1.0, 0.0, 0.0) if abs(axis[0]) < 0.9 else (0.0, 0.0, 1.0)
    u = norm(sub(ref, mul(axis, dot(ref, axis))))
    v = cross(axis, u)
    center = lerp(a, b, 0.5)

    def ring(c, r, weights):
        pts = []
        for k in range(sides):
            ang = turn + k / sides * math.tau
            p = add(c, add(mul(u, math.cos(ang) * r * squash[0]), mul(v, math.sin(ang) * r * squash[1])))
            pts.append((p, weights(p) if callable(weights) else weights))
        return pts

    ra_pts, rb_pts = ring(a, ra, wa), ring(b, rb, wb)
    for k in range(sides):
        (p0, w0), (p1, w1) = ra_pts[k], ra_pts[(k + 1) % sides]
        (q0, x0), (q1, x1) = rb_pts[k], rb_pts[(k + 1) % sides]
        quad(material, p0, p1, q1, q0, color, (w0, w1, x1, x0), center)
    for pts, end in ((ra_pts, a), (rb_pts, b)):
        for k in range(1, sides - 1):
            tri(material, pts[0][0], pts[k][0], pts[k + 1][0], color, pts[0][1], pts[k][1], pts[k + 1][1], center)


def box(center, half, color, weights, material=CLOTH):
    corners = [add(center, (sx * half[0], sy * half[1], sz * half[2]))
               for sx in (-1, 1) for sy in (-1, 1) for sz in (-1, 1)]
    hexa(corners, color, [weights] * 8, material, center)


def hexa(c, color, w, material=CLOTH, inside=None):
    """Six-sided solid from 8 corners indexed by bits (x, y, z)."""
    inside = inside or mul(tuple(map(sum, zip(*c))), 1 / 8)
    faces = [(0, 1, 3, 2), (4, 6, 7, 5), (0, 4, 5, 1), (2, 3, 7, 6), (0, 2, 6, 4), (1, 5, 7, 3)]
    for f in faces:
        quad(material, *(c[i] for i in f), color, [w[i] for i in f], inside)


def build_body():
    for side, sx in (("L", -1), ("R", 1)):
        x = 0.10 * sx
        thigh, shin, foot = f"thigh.{side}", f"shin.{side}", f"foot.{side}"
        # Legs: boots, shins and thighs, blended across the knee.
        box((x, 0.055, -0.035), (0.068, 0.055, 0.125), BOOTS, {foot: 1})
        prism((x, 0.26, 0), (x, 0.08, 0), 0.07, 0.072, 6, BOOTS, {shin: 1})
        prism((x, 0.53, 0), (x, 0.24, 0), 0.066, 0.062, 6, TROUSERS, {thigh: 0.4, shin: 0.6}, {shin: 1})
        prism((x, 0.98, 0), (x, 0.49, 0), 0.09, 0.07, 6, TROUSERS, {"hips": 0.3, thigh: 0.7}, {thigh: 0.6, shin: 0.4})

        # Arms: red sleeves, leather bracers and gloves, a pauldron on the shoulder.
        ax = sx
        upper, fore, hand = f"upper_arm.{side}", f"forearm.{side}", f"hand.{side}"
        prism((0.21 * ax, 1.48, 0), (0.24 * ax, 1.15, 0), 0.064, 0.054, 6, CLOAK,
              {"chest": 0.3, upper: 0.7}, {upper: 0.5, fore: 0.5})
        prism((0.24 * ax, 1.19, 0), (0.26 * ax, 0.95, 0), 0.052, 0.046, 6, LEATHER,
              {upper: 0.4, fore: 0.6}, {fore: 0.7, hand: 0.3})
        box((0.262 * ax, 0.885, 0.0), (0.036, 0.055, 0.042), LEATHER, {hand: 1})
        box((0.225 * ax, 1.455, 0.0), (0.08, 0.05, 0.085), LEATHER, {upper: 0.7, "chest": 0.3})

    # Pelvis, belt and buckle.
    prism((0, 0.86, 0), (0, 1.03, 0), 0.165, 0.17, 8, TROUSERS, {"hips": 1}, squash=(1, 0.75), turn=math.pi / 8)
    prism((0, 1.0, 0), (0, 1.07, 0), 0.178, 0.178, 8, LEATHER, {"hips": 1}, squash=(1, 0.78), turn=math.pi / 8)
    box((0, 1.035, -0.142), (0.032, 0.026, 0.01), WHITE, {"hips": 1}, material=METAL)

    # Tunic: torso blended along the spine, and a skirt whose hem follows the thighs.
    prism((0, 1.05, 0), (0, 1.29, 0), 0.165, 0.18, 8, TUNIC, {"hips": 0.3, "spine": 0.7},
          {"spine": 0.5, "chest": 0.5}, squash=(1, 0.74), turn=math.pi / 8)
    prism((0, 1.25, 0), (0, 1.49, 0), 0.185, 0.15, 8, TUNIC, {"spine": 0.4, "chest": 0.6}, {"chest": 1},
          squash=(1, 0.74), turn=math.pi / 8)

    def hem(p):
        side = max(-1.0, min(1.0, p[0] / 0.18))
        weights = {"hips": 0.45}
        if side < -0.2:
            weights["thigh.L"] = 0.55 * -side
        if side > 0.2:
            weights["thigh.R"] = 0.55 * side
        return weights

    prism((0, 1.02, 0), (0, 0.62, 0), 0.172, 0.215, 8, TUNIC, {"hips": 1}, hem, squash=(1, 0.8), turn=math.pi / 8)

    # Cloak: a slab hanging from the shoulders, flaring out behind.
    rows = [(1.49, 0.21, 0.105, {"chest": 1}), (1.22, 0.24, 0.15, {"chest": 0.5, "spine": 0.5}),
            (0.95, 0.27, 0.19, {"spine": 0.4, "hips": 0.6}), (0.68, 0.29, 0.21, {"hips": 1}),
            (0.42, 0.30, 0.235, {"hips": 1})]
    for (y0, w0, z0, k0), (y1, w1, z1, k1) in zip(rows, rows[1:]):
        t = 0.022
        corners, weights = [], []
        for sx in (-1, 1):
            for row in ((y0, w0, z0, k0), (y1, w1, z1, k1)):
                for dz in (-t, t):
                    y, w, z, k = row
                    corners.append((sx * w, y, z + dz))
                    weights.append(k)
        # Reorder from (x, row, z) to the (x, y, z) bit layout hexa expects (row 0 is the top).
        order = [1 * 2 + 0, 1 * 2 + 1, 0 * 2 + 0, 0 * 2 + 1]
        c = [corners[s * 4 + o] for s in range(2) for o in order]
        w = [weights[s * 4 + o] for s in range(2) for o in order]
        hexa(c, CLOAK, w)

    # Hood with a shadowed face, a brim and two embers for eyes.
    prism((0, 1.43, 0.02), (0, 1.82, 0.05), 0.158, 0.035, 7, HOOD, {"head": 0.6, "chest": 0.4}, {"head": 1},
          turn=-math.pi / 2)
    box((0, 1.60, -0.1), (0.072, 0.085, 0.014), SHADOW, {"head": 1})
    box((0, 1.695, -0.1), (0.088, 0.024, 0.03), HOOD, {"head": 1})
    for ex in (-0.03, 0.03):
        box((ex, 1.618, -0.115), (0.014, 0.008, 0.004), WHITE, {"head": 1}, material=EYES)

    # Staff in the right hand.
    prism((0.27, 0.06, -0.05), (0.27, 1.86, -0.05), 0.022, 0.02, 5, WOOD, {"hand.R": 1})
    box((0.27, 1.87, -0.05), (0.035, 0.045, 0.035), WOOD, {"hand.R": 1})
    # Lantern on its own joint, so it can swing.
    lx = -0.26
    box((lx, 0.855, 0), (0.006, 0.03, 0.006), WHITE, {"lantern": 1}, material=METAL)
    box((lx, 0.82, 0), (0.05, 0.012, 0.05), WHITE, {"lantern": 1}, material=METAL)
    box((lx, 0.755, 0), (0.038, 0.055, 0.038), WHITE, {"lantern": 1}, material=GLOW)
    box((lx, 0.693, 0), (0.05, 0.01, 0.05), WHITE, {"lantern": 1}, material=METAL)


# ---------------------------------------------------------------- animation

def quat(x=0.0, y=0.0, z=0.0):
    """Rotation by yaw y, then pitch x, then roll z (q = qy * qx * qz)."""
    def axis(ax, a):
        s = math.sin(a / 2)
        return (ax[0] * s, ax[1] * s, ax[2] * s, math.cos(a / 2))

    def qmul(a, b):
        ax, ay, az, aw = a
        bx, by, bz, bw = b
        return (aw * bx + ax * bw + ay * bz - az * by, aw * by - ax * bz + ay * bw + az * bx,
                aw * bz + ax * by - ay * bx + az * bw, aw * bw - ax * bx - ay * by - az * bz)

    return qmul(qmul(axis((0, 1, 0), y), axis((1, 0, 0), x)), axis((0, 0, 1), z))


def walk(p, run=False):
    s, c = math.sin(p), math.cos(p)
    k = 1.0 if not run else 2.0
    thigh, knee, arm = (0.42, 0.65, 0.38) if not run else (0.85, 1.3, 0.75)
    upper_r = (0.12 if not run else 0.5) * s
    fore_r = 0.35 if not run else 0.9
    fore_l = 0.15 if not run else 1.1
    pose = {
        "hips": dict(y=-0.10 * k * s, t=(0, (0.955 if not run else 0.925) - (0.04 if not run else 0.06) * s * s, 0)),
        "spine": dict(x=-0.06 if not run else -0.22),
        "chest": dict(y=0.16 * k * s, x=0.0 if not run else -0.04),
        "head": dict(y=-0.06 * k * s, x=0.02 if not run else 0.2),
        "thigh.L": dict(x=thigh * s + (0.0 if not run else 0.1)),
        "thigh.R": dict(x=-thigh * s + (0.0 if not run else 0.1)),
        "shin.L": dict(x=-(0.08 + knee * max(0.0, c)) - (0 if not run else 0.2)),
        "shin.R": dict(x=-(0.08 + knee * max(0.0, -c)) - (0 if not run else 0.2)),
        "foot.L": dict(x=0.2 * max(0.0, c)),
        "foot.R": dict(x=0.2 * max(0.0, -c)),
        "upper_arm.L": dict(x=-arm * s, z=-0.05),
        "forearm.L": dict(x=fore_l + 0.1 * max(0.0, -s)),
        "upper_arm.R": dict(x=upper_r, z=0.08),
        "forearm.R": dict(x=fore_r),
        # Keep the staff upright and let the lantern swing behind the hand.
        "hand.R": dict(x=-(upper_r + fore_r)),
        "lantern": dict(x=-0.8 * (-arm * s + fore_l) + 0.15 * math.sin(p - 1.2)),
    }
    return pose


def idle(p):
    s = math.sin(p)
    return {
        "hips": dict(t=(0, 0.95 - 0.006 * (1 - math.cos(p)), 0)),
        "spine": dict(x=-0.03),
        "chest": dict(x=0.025 * s),
        "head": dict(x=0.04 + 0.02 * math.sin(2 * p), y=0.12 * math.sin(p + 1.0)),
        "thigh.L": dict(z=-0.04),
        "thigh.R": dict(z=0.04),
        "upper_arm.L": dict(z=-0.05 - 0.015 * s, x=0.04),
        "forearm.L": dict(x=0.12),
        "lantern": dict(x=-0.14 + 0.05 * s),
        "upper_arm.R": dict(x=0.05, z=0.06),
        "forearm.R": dict(x=0.30),
        "hand.R": dict(x=-0.35),
    }


def jump(p):
    s = 0.05 * math.sin(p)
    return {
        "hips": dict(t=(0, 0.95, 0)),
        "spine": dict(x=0.05),
        "head": dict(x=0.1),
        "thigh.L": dict(x=0.7 + s),
        "shin.L": dict(x=-1.1),
        "foot.L": dict(x=0.2),
        "thigh.R": dict(x=-0.15 - s),
        "shin.R": dict(x=-0.6),
        "upper_arm.L": dict(x=0.2, z=-0.7 - s),
        "forearm.L": dict(x=0.4),
        "lantern": dict(x=-0.5),
        "upper_arm.R": dict(x=0.1, z=0.45 + s),
        "forearm.R": dict(x=0.6),
        "hand.R": dict(x=-0.7),
    }


CLIPS = [
    ("idle", 3.2, idle),
    ("walk", 0.8, walk),
    ("run", 0.58, lambda p: walk(p, run=True)),
    ("jump", 0.8, jump),
]
KEYS = 24


# ---------------------------------------------------------------- glTF

def main():
    build_body()
    os.makedirs(OUT, exist_ok=True)
    blob = bytearray()
    views, accessors = [], []

    def add_view(data, target=None):
        while len(blob) % 4:
            blob.append(0)
        view = {"buffer": 0, "byteOffset": len(blob), "byteLength": len(data)}
        if target:
            view["target"] = target
        views.append(view)
        blob.extend(data)
        return len(views) - 1

    def add_accessor(fmt, component, values, kind, target=None, bounds=False, normalized=False):
        flat = [c for v in values for c in v] if isinstance(values[0], (tuple, list)) else list(values)
        view = add_view(struct.pack(f"<{len(flat)}{fmt}", *flat), target)
        acc = {"bufferView": view, "componentType": component, "count": len(values), "type": kind}
        if bounds:
            acc["min"] = [min(v[i] for v in values) for i in range(len(values[0]))]
            acc["max"] = [max(v[i] for v in values) for i in range(len(values[0]))]
        if normalized:
            acc["normalized"] = True
        accessors.append(acc)
        return len(accessors) - 1

    primitives = []
    for material, p in enumerate(PRIMS):
        if not p.pos:
            continue
        attributes = {
            "POSITION": add_accessor("f", 5126, p.pos, "VEC3", 34962, bounds=True),
            "NORMAL": add_accessor("f", 5126, p.nrm, "VEC3", 34962),
            "COLOR_0": add_accessor("f", 5126, p.col, "VEC3", 34962),
            "JOINTS_0": add_accessor("B", 5121, p.joints, "VEC4", 34962),
            "WEIGHTS_0": add_accessor("f", 5126, p.weights, "VEC4", 34962),
        }
        indices = add_accessor("H", 5123, p.idx, "SCALAR", 34963)
        primitives.append({"attributes": attributes, "indices": indices, "material": material})

    # Nodes: 0 = model root, 1.. = joints, last = the skinned mesh.
    joint_node = {name: i + 1 for i, (name, _, _) in enumerate(JOINTS)}
    nodes = [{"name": "wanderer", "children": [1, len(JOINTS) + 1]}]
    for name, parent, pos in JOINTS:
        local = pos if parent is None else sub(pos, POS[parent])
        children = [joint_node[n] for n, par, _ in JOINTS if par == name]
        node = {"name": name, "translation": list(local)}
        if children:
            node["children"] = children
        nodes.append(node)
    nodes.append({"name": "body", "mesh": 0, "skin": 0})

    # Joints are only translated at rest, so the inverse bind is a translation.
    inverse_binds = [(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, -p[0], -p[1], -p[2], 1) for _, _, p in JOINTS]
    skin = {"joints": list(joint_node.values()), "skeleton": 1,
            "inverseBindMatrices": add_accessor("f", 5126, inverse_binds, "MAT4")}

    animations = []
    for name, duration, pose_at in CLIPS:
        times = [duration * k / KEYS for k in range(KEYS + 1)]
        time_acc = add_accessor("f", 5126, times, "SCALAR", bounds=False)
        accessors[time_acc]["min"], accessors[time_acc]["max"] = [0.0], [duration]
        frames = [pose_at(math.tau * k / KEYS) for k in range(KEYS + 1)]
        samplers, channels = [], []
        for joint in frames[0]:
            rotations = [quat(f[joint].get("x", 0), f[joint].get("y", 0), f[joint].get("z", 0)) for f in frames]
            samplers.append({"input": time_acc, "output": add_accessor("f", 5126, rotations, "VEC4")})
            channels.append({"sampler": len(samplers) - 1, "target": {"node": joint_node[joint], "path": "rotation"}})
            if "t" in frames[0][joint]:
                translations = [f[joint]["t"] for f in frames]
                samplers.append({"input": time_acc, "output": add_accessor("f", 5126, translations, "VEC3")})
                channels.append(
                    {"sampler": len(samplers) - 1, "target": {"node": joint_node[joint], "path": "translation"}})
        animations.append({"name": name, "samplers": samplers, "channels": channels})

    with open(os.path.join(OUT, "wanderer.bin"), "wb") as f:
        f.write(blob)
    doc = {
        "asset": {"version": "2.0", "generator": "Grimora tools/gen_wanderer.py"},
        "extensionsUsed": ["KHR_materials_emissive_strength"],
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": nodes,
        "meshes": [{"name": "wanderer", "primitives": primitives}],
        "skins": [skin],
        "animations": animations,
        "materials": [
            {"name": "cloth", "pbrMetallicRoughness": {"metallicFactor": 0, "roughnessFactor": 0.9}},
            {"name": "metal", "pbrMetallicRoughness": {"baseColorFactor": [0.32, 0.3, 0.27, 1], "metallicFactor": 0.85,
                                                       "roughnessFactor": 0.45}},
            {"name": "eyes", "pbrMetallicRoughness": {"baseColorFactor": [0, 0, 0, 1]},
             "emissiveFactor": [1.0, 0.5, 0.18],
             "extensions": {"KHR_materials_emissive_strength": {"emissiveStrength": 12.0}}},
            {"name": "lantern", "pbrMetallicRoughness": {"baseColorFactor": [1, 0.8, 0.5, 1], "metallicFactor": 0},
             "emissiveFactor": [1.0, 0.62, 0.28],
             "extensions": {"KHR_materials_emissive_strength": {"emissiveStrength": 6.0}}},
        ],
        "buffers": [{"uri": "wanderer.bin", "byteLength": len(blob)}],
        "bufferViews": views,
        "accessors": accessors,
    }
    with open(os.path.join(OUT, "wanderer.gltf"), "w") as f:
        json.dump(doc, f, indent=1)
    vertices = sum(len(p.pos) for p in PRIMS)
    print(f"wrote {os.path.normpath(OUT)}: {vertices} vertices, {len(JOINTS)} joints, {len(CLIPS)} clips")


if __name__ == "__main__":
    main()
