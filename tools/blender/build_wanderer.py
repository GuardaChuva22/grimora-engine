"""Builds assets/models/wanderer in Blender: the standard body, in plain shorts.

Armor and clothes are meant to go on top, skinned to this same skeleton, whose
`weapon.R`, `lantern`, `cape.*` and `tabard.*` joints wait for them with no skin
of their own. Mesh, rig, weights, a dithered 15-bit texture atlas and vertex-color
grime all come from this script, with a fixed seed. The result is also saved as wanderer.blend,
so poses and shapes can be touched up by hand. Positions here are engine
coordinates (x right, y up, facing -z) and meters.

    blender --background --python tools/blender/build_wanderer.py
    blender --background --python tools/blender/build_wanderer.py -- --preview <folder>

The clips are still the ones from tools/gen_wanderer.py, carried over to this rig.
"""

import json
import math
import os
import sys

import bpy
import numpy as np
from mathutils import Matrix, Quaternion, Vector

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
import gen_wanderer as old  # noqa: E402

sys.path.insert(0, HERE)
import anim_kit  # noqa: E402
import sword_moves  # noqa: E402

OUT = os.path.join(HERE, "..", "..", "assets", "models", "wanderer")
SWORD_SCALE = 0.263  # espada_reta.glb is 3.6 units long
EVENTS = {}  # clip: [(seconds, name)], for those made with anim_kit
ARMED_OLD_CLIPS = ("attack", "hurt", "die")
ARM_ANGLE = math.radians(35.0)  # the rest pose holds the arms this far from straight down
FINGER_JOINT = 0.032  # from the palm to where the fingers bend the second time
SIDES = ((-1, "L"), (1, "R"))


def to_blender(p):
    return Vector((p[0], -p[2], p[1]))


def v3(*p):
    return Vector(p)


# ---------------------------------------------------------------- skeleton
BONES = {}  # name: (parent, head, tail), parents first


def bone(name, parent, head, tail):
    BONES[name] = (parent, v3(*head), v3(*tail))


def arm_points(s):
    direction = v3(s * math.sin(ARM_ANGLE), -math.cos(ARM_ANGLE), 0.0)
    shoulder = v3(s * 0.21, 1.50, 0.0)
    elbow = shoulder + direction * 0.30
    wrist = elbow + direction * 0.27
    palm = wrist + direction * 0.09
    tip = palm + direction * 0.09
    return direction, shoulder, elbow, wrist, palm, tip


def inward_of(s):
    """Where a palm at rest faces, and fingers curl."""
    return v3(math.cos(ARM_ANGLE) * -s, -math.sin(ARM_ANGLE), 0.0)


def thumb_root(s):
    direction, _, _, wrist, _, _ = arm_points(s)
    return wrist + direction * 0.03 + v3(0, 0, -0.036)


def grip_point():
    """The middle of what the right hand holds, against the palm at the fingers' base."""
    direction, _, _, _, palm, _ = arm_points(1)
    return palm + direction * 0.002 + inward_of(1) * 0.036


def build_skeleton():
    # On the ground under the hips: clips that travel key it, and the engine moves
    # the character by it (see `Animator::root_delta`).
    bone("root", None, (0, 0, 0), (0, 0.2, 0))
    bone("hips", "root", (0, 0.95, 0), (0, 1.10, 0))
    bone("spine", "hips", (0, 1.10, 0), (0, 1.28, 0))
    bone("chest", "spine", (0, 1.28, 0), (0, 1.54, 0))
    bone("neck", "chest", (0, 1.54, 0), (0, 1.62, 0))
    bone("head", "neck", (0, 1.62, 0), (0, 1.86, 0))
    for s, x in SIDES:
        direction, shoulder, elbow, wrist, palm, tip = arm_points(s)
        bone(f"clavicle.{x}", "chest", (s * 0.04, 1.50, -0.01), shoulder)
        bone(f"upper_arm.{x}", f"clavicle.{x}", shoulder, elbow)
        bone(f"forearm.{x}", f"upper_arm.{x}", elbow, wrist)
        bone(f"hand.{x}", f"forearm.{x}", wrist, palm)
        # The fingers bend twice as one, enough to close around a grip.
        joint = palm + direction * FINGER_JOINT
        bone(f"fingers.{x}", f"hand.{x}", palm, joint)
        bone(f"fingertips.{x}", f"fingers.{x}", joint, tip)
        thumb = thumb_root(s)
        bone(f"thumb.{x}", f"hand.{x}", thumb, thumb + direction * 0.065)
        bone(f"thigh.{x}", "hips", (s * 0.095, 0.92, 0), (s * 0.084, 0.50, 0.005))
        bone(f"shin.{x}", f"thigh.{x}", (s * 0.084, 0.50, 0.005), (s * 0.06, 0.09, 0.035))
        bone(f"foot.{x}", f"shin.{x}", (s * 0.06, 0.09, 0.035), (s * 0.062, 0.035, -0.11))
        bone(f"toe.{x}", f"foot.{x}", (s * 0.062, 0.035, -0.11), (s * 0.064, 0.03, -0.205))
    # The grip runs across the palm's inner side; its tail is where the blade points.
    grip = grip_point()
    bone("weapon.R", "hand.R", grip, grip + v3(0, 0, -0.2))
    # The lantern hangs from the belt, leaving both hands for the sword.
    bone("lantern", "hips", (-0.235, 0.93, -0.06), (-0.235, 0.82, -0.06))
    for x, c in (("L", -0.2), ("C", 0.0), ("R", 0.2)):
        bone(f"cape.{x}.1", "chest", (c, 1.40, 0.18), (c, 1.12, 0.18))
        bone(f"cape.{x}.2", f"cape.{x}.1", (c, 1.12, 0.18), (c, 0.82, 0.19))
        bone(f"cape.{x}.3", f"cape.{x}.2", (c, 0.82, 0.19), (c, 0.45, 0.22))
    for x, z in (("F", -0.15), ("B", 0.15)):
        bone(f"tabard.{x}.1", "hips", (0, 0.99, z), (0, 0.77, z * 1.1))
        bone(f"tabard.{x}.2", f"tabard.{x}.1", (0, 0.77, z * 1.1), (0, 0.54, z * 1.2))


# ---------------------------------------------------------------- texture atlas
GRID = 2
CELL_PX = 256
ATLAS_PX = GRID * CELL_PX
SKIN, TORSO, FACE, LINEN = range(4)
SKIN_TONE = (0.70, 0.56, 0.47)


def blotches(rng, cells):
    """Smooth noise from 0 to 1, one CELL_PX square."""
    grid = rng.random((cells + 1, cells + 1))
    at = np.linspace(0, cells, CELL_PX, endpoint=False)
    i = at.astype(int)
    f = at - i
    f = f * f * (3 - 2 * f)
    left = grid[i][:, i] * (1 - f)[:, None] + grid[i + 1][:, i] * f[:, None]
    right = grid[i][:, i + 1] * (1 - f)[:, None] + grid[i + 1][:, i + 1] * f[:, None]
    return left * (1 - f)[None, :] + right * f[None, :]


def cloudy(rng):
    return blotches(rng, 4) * 0.5 + blotches(rng, 12) * 0.3 + blotches(rng, 40) * 0.2


def over(rgb, mask, color):
    return rgb * (1 - mask[..., None]) + np.array(color) * mask[..., None]


def skin(rng):
    """Weathered skin: uneven tone, grime, and a few old marks."""
    rgb = np.array(SKIN_TONE) * (0.86 + 0.28 * cloudy(rng))[..., None]
    rgb = over(rgb, np.clip((cloudy(rng) - 0.5) * 1.6, 0, 0.22), (0.5, 0.31, 0.25))
    rgb = over(rgb, np.clip((cloudy(rng) - 0.62) * 2.5, 0, 0.22), (0.2, 0.15, 0.12))
    return rgb + (rng.random((CELL_PX, CELL_PX, 1)) - 0.5) * 0.025


def soft_line(rgb, points, width, color, strength):
    """A soft stroke along (x, y) pixel points, for painted anatomy."""
    y, x = np.mgrid[0:CELL_PX, 0:CELL_PX]
    mask = np.zeros((CELL_PX, CELL_PX))
    for (x0, y0), (x1, y1) in zip(points, points[1:]):
        dx, dy = x1 - x0, y1 - y0
        t = np.clip(((x - x0) * dx + (y - y0) * dy) / max(dx * dx + dy * dy, 1), 0, 1)
        distance = np.hypot(x - x0 - t * dx, y - y0 - t * dy)
        mask = np.maximum(mask, np.clip(1 - distance / width, 0, 1))
    return over(rgb, mask * strength, color)


def paint_torso(rng):
    bare = skin(rng)
    rgb = bare.copy()
    dark = (0.2, 0.13, 0.1)
    # The front is the middle of the cell. Mirrored strokes: collarbones, chest, ribs.
    for side in (-1, 1):
        at = lambda u, v: (128 + side * u, 31 + 0.877 * v)  # noqa: E731
        rgb = soft_line(rgb, [at(3, 26), at(20, 22), at(44, 30)], 3, dark, 0.45)
        rgb = soft_line(rgb, [at(3, 88), at(20, 98), at(40, 92), at(50, 72)], 5, dark, 0.5)
        rgb = soft_line(rgb, [at(16, 124), at(30, 120)], 2.5, dark, 0.25)
        rgb = soft_line(rgb, [at(15, 144), at(28, 141)], 2.5, dark, 0.25)
        rgb = soft_line(rgb, [at(14, 165), at(26, 163)], 2.5, dark, 0.25)
        rgb = soft_line(rgb, [at(32, 110), at(30, 180), at(14, 226)], 3, dark, 0.28)
        rgb = soft_line(rgb, [at(22, 84), at(23, 85)], 2.2, (0.3, 0.16, 0.13), 0.75)
    rgb = soft_line(rgb, [(128, 66), (128, 108)], 2.5, dark, 0.3)
    rgb = soft_line(rgb, [(128, 119), (128, 206)], 2.5, dark, 0.32)
    rgb = soft_line(rgb, [(128, 217), (128, 220)], 3, dark, 0.8)
    # The back meets at the seam: the spine and the shoulder blades.
    for u in (0, 255):
        rgb = soft_line(rgb, [(u, 16), (u, 240)], 4, dark, 0.32)
    for u in (28, 228):
        rgb = soft_line(rgb, [(u - 8, 46), (u, 74), (u + 8, 102)], 4, dark, 0.3)
    return bare * 0.6 + rgb * 0.4


def paint_face(rng):
    """Mapped as a cylinder around the head, with the face in the middle: 16 pixels
    per column, so the eye sits between columns 6 and 7. Shadows rather than outlines."""
    rgb = skin(rng)
    row = lambda y: 4 + (1.862 - y) / 0.262 * 248  # noqa: E731
    shadow = (0.17, 0.1, 0.08)
    for side in (-1, 1):
        at = lambda u, y: (128 + side * u, row(y))  # noqa: E731
        # The socket, then a small tired eye set deep in it.
        rgb = soft_line(rgb, [at(13, 1.748), at(34, 1.748)], 9, shadow, 0.6)
        rgb = soft_line(rgb, [at(18, 1.745), at(29, 1.745)], 2.4, (0.5, 0.44, 0.37), 0.85)
        rgb = soft_line(rgb, [at(23, 1.745), at(24, 1.745)], 2.2, (0.05, 0.04, 0.035), 1.0)
        rgb = soft_line(rgb, [at(16, 1.75), at(31, 1.75)], 1.7, (0.07, 0.05, 0.04), 0.8)
        # A heavy brow, the side of the nose, the hollow of the cheek, the jaw's edge.
        rgb = soft_line(rgb, [at(11, 1.766), at(36, 1.765)], 3.4, (0.11, 0.08, 0.06), 0.8)
        rgb = soft_line(rgb, [at(9, 1.74), at(11, 1.705)], 3.5, shadow, 0.35)
        rgb = soft_line(rgb, [at(36, 1.705), at(30, 1.668)], 9, shadow, 0.14)
        rgb = soft_line(rgb, [at(4, 1.695), at(9, 1.698)], 2.4, shadow, 0.65)
    # The mouth: a dark line, a lit lower lip, the shadow beneath it.
    rgb = soft_line(rgb, [(113, row(1.674)), (143, row(1.674))], 1.8, (0.14, 0.07, 0.06), 0.8)
    rgb = soft_line(rgb, [(118, row(1.666)), (138, row(1.666))], 2.8, (0.5, 0.31, 0.27), 0.4)
    rgb = soft_line(rgb, [(119, row(1.656)), (137, row(1.656))], 3, shadow, 0.45)
    # Short dark hair, cropped with a knife, down to the nape; stubble on the jaw.
    y, x = np.mgrid[0:CELL_PX, 0:CELL_PX]
    away = np.abs(x - 128) / 128
    hairline = np.interp(away, [0, 0.25, 0.45, 0.62, 0.8, 1], [row(1.80), row(1.803), row(1.775), row(1.755), row(1.69), row(1.662)])
    hair = np.clip((hairline + (blotches(rng, 40) - 0.5) * 12 - y) / 3, 0, 1)
    rgb = over(rgb, hair, (0.1, 0.08, 0.065))
    rgb += (np.clip(rng.random((CELL_PX, CELL_PX)) - 0.72, 0, 1) * hair * 0.16)[..., None]
    jaw = np.clip((y - row(1.695)) / 30, 0, 1) * np.clip(1.15 - away * 2.4, 0, 1) * 0.16
    return over(rgb, jaw, (0.11, 0.09, 0.08))


def paint_linen(rng):
    """Plain, dirty linen shorts."""
    u = np.arange(CELL_PX)
    weave = 0.9 + 0.1 * ((u[None, :] + (u[:, None] // 2)) % 2)
    rgb = np.array((0.4, 0.34, 0.255)) * ((0.72 + 0.42 * cloudy(rng)) * weave)[..., None]
    rgb = over(rgb, np.clip((cloudy(rng) - 0.52) * 4, 0, 0.55), (0.15, 0.115, 0.08))
    rgb[12:16] *= 0.55  # the drawstring's fold
    return rgb


def paint_atlas():
    rng = np.random.default_rng(1994)
    atlas = np.zeros((ATLAS_PX, ATLAS_PX, 3))
    for cell, paint in zip((SKIN, TORSO, FACE, LINEN), (skin, paint_torso, paint_face, paint_linen)):
        row, col = cell // GRID, cell % GRID
        atlas[row * CELL_PX:(row + 1) * CELL_PX, col * CELL_PX:(col + 1) * CELL_PX] = paint(rng)
    # Down to 15 bits with the console's ordered dither.
    bayer = np.array([[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]]) / 16.0 - 0.47
    y, x = np.mgrid[0:ATLAS_PX, 0:ATLAS_PX]
    atlas = np.floor(np.clip(atlas, 0, 1) * 31 + 0.5 + bayer[y % 4, x % 4][..., None])
    atlas = np.clip(atlas, 0, 31) / 31
    image = bpy.data.images.new("wanderer_atlas", ATLAS_PX, ATLAS_PX, alpha=False)
    pixels = np.ones((ATLAS_PX, ATLAS_PX, 4))
    pixels[..., :3] = atlas[::-1]
    image.pixels.foreach_set(pixels.ravel().astype(np.float32))
    image.file_format = "PNG"
    image.update()
    image.pack()
    return image


def cell_uv(cell, u, v):
    """`u` wraps around a part and `v` runs from its top (0) to its hem (1)."""
    inset = 2 / CELL_PX
    row, col = cell // GRID, cell % GRID
    return (col + inset + u * (1 - 2 * inset)) / GRID, 1 - (row + inset + v * (1 - 2 * inset)) / GRID


# ---------------------------------------------------------------- mesh building
CLOTH = 0
VERTS, WEIGHTS, SHADES, FACES = [], [], [], []


def ellipse(cx, y, cz, rx, rz, n, span=math.tau):
    """A ring starting at the back, so the middle of the texture faces forward."""
    count = n if span >= math.tau else n + 1
    step = span / n
    start = math.pi / 2 - (0 if span >= math.tau else span / 2)
    return [v3(cx + rx * math.cos(start + i * step), y, cz + rz * math.sin(start + i * step)) for i in range(count)]


def around(center, axis, ru, rv, n, front=v3(0, 0, 1)):
    """A ring around an axis; `ru` is the radius along `front`."""
    axis = axis.normalized()
    u = (front - axis * front.dot(axis)).normalized()
    w = u.cross(axis)
    return [center + u * (ru * math.cos(i / n * math.tau)) + w * (rv * math.sin(i / n * math.tau)) for i in range(n)]


def loft(rings, weights, cell, material=CLOTH, closed=True, smooth=True, shade=1.0, inside=None, caps=(False, False), vs=None):
    """Skins rings given from top to bottom. `weights` is one dict per ring or a
    function of (point, ring, column). `inside` says which way is in: a function
    of the ring index, by default the middle of that ring. `cell` can be a function
    of the band's index. Returns the first vertex of the first ring."""
    n = len(rings[0])
    columns = n if closed else n - 1
    lengths = [0.0]
    for a, b in zip(rings, rings[1:]):
        lengths.append(lengths[-1] + (sum(a, Vector()) / n - sum(b, Vector()) / n).length)
    total = lengths[-1] or 1.0
    if vs:
        lengths, total = vs, 1.0
    first = len(VERTS)
    for k, ring in enumerate(rings):
        for i, point in enumerate(ring):
            VERTS.append(point)
            WEIGHTS.append(weights(point, k, i) if callable(weights) else weights[k])
            SHADES.append(shade(k, point) if callable(shade) else shade)

    def at(k, i):
        return first + k * n + i % n

    def middle(k):
        return inside(k) if inside else sum(rings[k], Vector()) / n

    for k in range(len(rings) - 1):
        here = cell(k) if callable(cell) else cell
        for i in range(columns):
            ids = [at(k, i), at(k, i + 1), at(k + 1, i + 1), at(k + 1, i)]
            uvs = [
                cell_uv(here, i / columns, lengths[k] / total),
                cell_uv(here, (i + 1) / columns, lengths[k] / total),
                cell_uv(here, (i + 1) / columns, lengths[k + 1] / total),
                cell_uv(here, i / columns, lengths[k + 1] / total),
            ]
            points = [VERTS[j] for j in ids]
            normal = (points[1] - points[0]).cross(points[3] - points[0])
            if normal.length < 1e-9:
                normal = (points[2] - points[1]).cross(points[0] - points[1])
            center = sum(points, Vector()) / 4
            if normal.dot(center - (middle(k) + middle(k + 1)) / 2) < 0:
                ids.reverse()
                uvs.reverse()
            FACES.append((ids, uvs, material, smooth(k) if callable(smooth) else smooth))
    for end, on in zip((0, len(rings) - 1), caps):
        if on:
            ids = [at(end, i) for i in range(n)]
            away = sum(rings[end], Vector()) / n - sum(rings[1 if end == 0 else end - 1], Vector()) / n
            normal = (VERTS[ids[1]] - VERTS[ids[0]]).cross(VERTS[ids[2]] - VERTS[ids[0]])
            if normal.dot(away) < 0:
                ids.reverse()
            last = cell(len(rings) - 2) if callable(cell) else cell
            FACES.append((ids, [cell_uv(last, 0.5, 0.5)] * n, material, False))
    return first


def align(previous, ring):
    """`ring` turned and flipped to line up with `previous`, so a tube doesn't twist."""
    n = len(ring)
    best, least = ring, float("inf")
    for candidate in (ring, ring[::-1]):
        for turn in range(n):
            turned = candidate[turn:] + candidate[:turn]
            total = sum((a - b).length for a, b in zip(previous, turned))
            if total < least:
                best, least = turned, total
    return best


def bridge(loop, first_ring, cell):
    """Stitches a loop of existing vertices to the first ring of a loft, already lined up."""
    n = len(loop)
    middle = (sum((VERTS[i] for i in loop), Vector()) + sum((VERTS[first_ring + i] for i in range(n)), Vector())) / (2 * n)
    for i in range(n):
        ids = [loop[i], loop[(i + 1) % n], first_ring + (i + 1) % n, first_ring + i]
        uvs = [cell_uv(cell, i / n, 0.0), cell_uv(cell, (i + 1) / n, 0.0), cell_uv(cell, (i + 1) / n, 0.03), cell_uv(cell, i / n, 0.03)]
        a, b, c = (VERTS[j] for j in ids[:3])
        center = sum((VERTS[j] for j in ids), Vector()) / 4
        if (b - a).cross(c - a).dot(center - middle) < 0:
            ids.reverse()
            uvs.reverse()
        FACES.append((ids, uvs, CLOTH, True))


def mirrored(half):
    """A full ring of 12 from 7 points: back middle, around +x, front middle."""
    points = [(p[0], p[1], p[2]) for p in half]
    return points + [(-x, y, z) for x, y, z in reversed(points[1:-1])]


def row(y, half):
    return [(p[0], p[1], p[2]) if len(p) == 3 else (p[0], y, p[1]) for p in half]


# The trunk, top to bottom: the cell of the band above each row, its
# joints, and half a ring. Columns: 0 back, 3 side, 6 front. Forward is -z.
def section(y, rx, front, back, power=0.8, socket_y=None, push=None):
    """Half a ring of the trunk: `rx` to the side, `front` (negative) and `back` the
    reach along z, squarer as `power` drops. `push` nudges columns: {column: (dx, dz)}."""
    half = []
    for k in range(7):
        a = math.radians(30 * k)
        x = rx * abs(math.sin(a)) ** power
        z = back * math.cos(a) ** power if math.cos(a) >= 0 else front * abs(math.cos(a)) ** power
        if push and k in push:
            x, z = x + push[k][0], z + push[k][1]
        half.append((x, socket_y if socket_y and k == 3 else y, z))
    return half


# The trunk, top to bottom: the cell of the band above each row, its joints, and
# half a ring. The neck rises from behind the shoulders, the chest is deep, the
# waist narrow, the seat behind.
TRUNK = [
    (TORSO, {"neck": 0.8, "chest": 0.2}, section(1.565, 0.075, -0.03, 0.09, power=1.0)),
    (TORSO, {"chest": 1}, section(1.535, 0.118, -0.03, 0.105, power=0.9)),
    (TORSO, {"chest": 1}, section(1.50, 0.192, -0.04, 0.115, socket_y=1.51)),
    (TORSO, {"chest": 1}, section(1.435, 0.192, -0.07, 0.13, push={5: (0.0, -0.012)})),
    (TORSO, {"chest": 1}, section(1.36, 0.17, -0.115, 0.135, push={4: (0.01, -0.008), 5: (0.0, -0.014), 2: (0.012, 0.0)})),
    (TORSO, {"chest": 0.85, "spine": 0.15}, section(1.29, 0.178, -0.116, 0.122, push={5: (0.0, -0.008)})),
    (TORSO, {"spine": 1}, section(1.19, 0.149, -0.11, 0.088)),
    (TORSO, {"spine": 0.6, "hips": 0.4}, section(1.095, 0.132, -0.11, 0.07)),
    (TORSO, {"hips": 0.9, "spine": 0.1}, section(1.03, 0.14, -0.108, 0.098)),
    (LINEN, {"hips": 1}, section(0.975, 0.162, -0.1, 0.124, power=0.75)),
    (LINEN, {"hips": 1}, section(0.91, 0.174, -0.092, 0.13, power=0.75)),
    (LINEN, {"hips": 1}, section(0.87, 0.18, -0.088, 0.108, power=0.75, push={0: (0, -0.018), 6: (0, 0.016)})),
]
SOCKET_TOP = 2  # the rows of TRUNK around the shoulder sockets
CELL_SPAN = {TORSO: (1.60, 1.03), LINEN: (1.03, 0.86)}


def build_trunk():
    """One surface from the neck to the crotch, with holes the arms and legs get
    stitched to, so that nothing merely pokes through anything else."""
    ids = []
    for r, (_, weights, half) in enumerate(TRUNK):
        ring = []
        for c, (x, y, z) in enumerate(mirrored(half)):
            if r == SOCKET_TOP + 1 and c in (3, 9):
                ring.append(None)  # the middle of a socket
                continue
            w = dict(weights)
            near_shoulder = c in (2, 3, 4, 8, 9, 10) and SOCKET_TOP - 1 <= r <= SOCKET_TOP + 2
            if near_shoulder:
                clavicle = f"clavicle.{'R' if c < 6 else 'L'}"
                share = {SOCKET_TOP - 1: 0.2, SOCKET_TOP: 0.5, SOCKET_TOP + 1: 0.5, SOCKET_TOP + 2: 0.2}[r]
                w = {"chest": 1 - share, clavicle: share}
            VERTS.append(v3(x, y, z))
            WEIGHTS.append(w)
            SHADES.append(0.88 if r == 0 else 0.9 if near_shoulder and r == SOCKET_TOP + 2 else 1.0)
            ring.append(len(VERTS) - 1)
        ids.append(ring)

    def v_of(cell, y):
        top, bottom = CELL_SPAN[cell]
        return max(0.0, min(1.0, (top - y) / (top - bottom)))

    holes = {(SOCKET_TOP + a, c) for a in (0, 1) for c in (2, 3, 8, 9)}
    for r in range(len(TRUNK) - 1):
        cell = TRUNK[r + 1][0]
        for c in range(12):
            if (r, c) in holes:
                continue
            quad = [ids[r][c], ids[r][(c + 1) % 12], ids[r + 1][(c + 1) % 12], ids[r + 1][c]]
            uvs = [cell_uv(cell, u / 12, v_of(cell, VERTS[i].y)) for u, i in zip((c, c + 1, c + 1, c), quad)]
            a, b, d = VERTS[quad[0]], VERTS[quad[1]], VERTS[quad[3]]
            center = sum((VERTS[i] for i in quad), Vector()) / 4
            if (b - a).cross(d - a).dot(center - v3(0, center.y, 0)) < 0:
                quad.reverse()
                uvs.reverse()
            FACES.append((quad, uvs, CLOTH, True))
    # The loops the limbs attach to.
    top, mid, low = ids[SOCKET_TOP], ids[SOCKET_TOP + 1], ids[SOCKET_TOP + 2]
    sockets = {"R": [top[2], top[3], top[4], mid[4], low[4], low[3], low[2], mid[2]],
               "L": [top[8], top[9], top[10], mid[10], low[10], low[9], low[8], mid[8]]}
    bottom = ids[-1]
    VERTS.append(v3(0, 0.835, 0.012))
    WEIGHTS.append({"hips": 1})
    SHADES.append(0.75)
    crotch = len(VERTS) - 1
    groins = {"R": [bottom[i] for i in range(7)] + [crotch], "L": [bottom[i] for i in (6, 7, 8, 9, 10, 11, 0)] + [crotch]}
    return ids[0], sockets, groins


def build_leg(s, x, groin):
    thigh, shin, foot, toe = f"thigh.{x}", f"shin.{x}", f"foot.{x}", f"toe.{x}"
    # The leg narrows inward to the ankle; the knee is deep and the calf sits well
    # behind the shin.
    rows = ((0.80, 0.089, 0.099, 0.001), (0.775, 0.088, 0.097, 0.0), (0.69, 0.081, 0.089, -0.002), (0.60, 0.067, 0.072, 0.004),
            (0.525, 0.051, 0.062, 0.008), (0.465, 0.056, 0.068, 0.022), (0.38, 0.062, 0.082, 0.048), (0.28, 0.047, 0.074, 0.056),
            (0.18, 0.037, 0.056, 0.052), (0.105, 0.031, 0.047, 0.044))
    centre = lambda y: s * float(np.interp(y, (0.105, 0.525, 0.80), (0.06, 0.083, 0.097)))  # noqa: E731
    rings = [align([VERTS[i] for i in groin], ellipse(centre(rows[0][0]), rows[0][0], rows[0][3], rows[0][1], rows[0][2], 8))]
    for y, rx, rz, z in rows[1:]:
        rings.append(align(rings[-1], ellipse(centre(y), y, z, rx, rz, 8)))
    cx = s * 0.06
    # The foot, bending forward from the ankle: back of each ring first, then around
    # to the top of the instep. The last ones stand up across the toes.
    foot_rows = (((0, .012, .092), (.032, .02, .084), (.038, .05, .05), (.032, .078, .002), (0, .088, -.014)),
                 ((0, 0, .02), (.038, 0, .015), (.048, .032, -.03), (.036, .055, -.06), (0, .062, -.066)),
                 ((0, 0, -.11), (.047, 0, -.115), (.053, .024, -.13), (.038, .04, -.136), (0, .044, -.137)),
                 ((0, 0, -.185), (.045, 0, -.192), (.048, .013, -.202), (.032, .026, -.208), (0, .028, -.21)))
    for half in foot_rows:
        full = half + tuple((-a, b, c) for a, b, c in reversed(half[1:-1]))
        rings.append(align(rings[-1], [v3(cx + a, b, c) for a, b, c in full]))
    weights = [{"hips": 0.25, thigh: 0.75}, {thigh: 0.95, "hips": 0.05}, {thigh: 1}, {thigh: 0.9, shin: 0.1}, {thigh: 0.5, shin: 0.5},
               {shin: 0.9, thigh: 0.1}, {shin: 1}, {shin: 1}, {shin: 1}, {shin: 0.5, foot: 0.5},
               {foot: 1}, {foot: 1}, {foot: 0.6, toe: 0.4}, {toe: 1}]
    first = loft(rings, weights, lambda k: LINEN if k < 1 else SKIN, shade=lambda k, p: 0.9 if k > 9 else 1.0, caps=(False, True))
    bridge(groin, first, LINEN)


def build_arm(s, x, socket):
    direction, shoulder, elbow, wrist, palm, tip = arm_points(s)
    upper, fore, hand, fingers, clavicle = f"upper_arm.{x}", f"forearm.{x}", f"hand.{x}", f"fingers.{x}", f"clavicle.{x}"
    # First the socket pushed outward, then rings that turn to follow the arm: the
    # top flows over the deltoid and the underside makes the armpit.
    rings = [[VERTS[i] + v3(s * 0.04, 0.0, -VERTS[i].z * 0.07) for i in socket]]
    rows = ((0.10, 0.072, 0.066), (0.165, 0.063, 0.059), (0.235, 0.052, 0.049), (0.30, 0.044, 0.041), (0.36, 0.05, 0.045),
            (0.44, 0.041, 0.036), (0.525, 0.029, 0.025), (0.575, 0.027, 0.021))
    for t, ru, rv in rows:
        rings.append(align(rings[-1], around(shoulder + direction * t, direction, ru, rv, 8)))
    rings.append(align(rings[-1], around(wrist + direction * 0.035, direction, 0.043, 0.021, 8)))
    rings.append(align(rings[-1], around(palm, direction, 0.047, 0.016, 8)))
    weights = [{clavicle: 0.5, upper: 0.5}, {upper: 0.85, clavicle: 0.15}, {upper: 1}, {upper: 0.95, fore: 0.05}, {upper: 0.5, fore: 0.5},
               {fore: 0.95, upper: 0.05}, {fore: 1}, {fore: 1}, {fore: 0.5, hand: 0.5}, {hand: 1}, {hand: 0.8, fingers: 0.2}]
    first = loft(rings, weights, SKIN, caps=(False, True))
    bridge(socket, first, SKIN)
    # Four fingers and a thumb, rooted inside the palm and curled a little inward. Each
    # finger bends at FINGER_JOINT, on a ring that takes from both bones so it doesn't crease.
    tips = f"fingertips.{x}"
    inward = inward_of(s)
    for offset, length in ((-0.034, 0.096), (-0.0115, 0.11), (0.0115, 0.104), (0.034, 0.086)):
        knuckle = palm + v3(0, 0, offset) - direction * 0.008
        bend = palm + v3(0, 0, offset) + direction * FINGER_JOINT + inward * 0.003
        rest = length - FINGER_JOINT - 0.008
        # The last joint stays bent, relaxed or holding.
        lean = (direction + inward * 0.27).normalized()
        curled = (direction + inward * 1.1).normalized()
        last = bend + lean * rest * 0.55
        end = last + curled * rest * 0.45
        tube = [around(knuckle, direction, 0.0105, 0.012, 4), around(bend, direction, 0.0098, 0.0108, 4),
                around(last, lean + curled, 0.009, 0.0098, 4), around(end, curled, 0.0072, 0.0078, 4)]
        loft(tube, [{hand: 0.5, fingers: 0.5}, {fingers: 0.5, tips: 0.5}, {tips: 1}, {tips: 1}], SKIN, smooth=False, caps=(False, True), shade=0.94)
    thumb = f"thumb.{x}"
    start = thumb_root(s)
    middle = start + direction * 0.035 + v3(0, 0, -0.02) + inward * 0.008
    end = middle + direction * 0.03 + v3(0, 0, -0.004) + inward * 0.012
    tube = [around(start, middle - start, 0.015, 0.015, 5), around(middle, end - start, 0.012, 0.012, 5), around(end, end - middle, 0.008, 0.008, 5)]
    loft(tube, [{hand: 0.5, thumb: 0.5}, {thumb: 1}, {thumb: 1}], SKIN, smooth=False, caps=(True, True), shade=0.94)


# The skull and face, one half: columns 0 back, 3 at the ear, 6 and 7 around the eye,
# 8 down the middle of the face. Forward is -z. The face needs these extra columns:
# with fewer, smooth shading melts it into an egg.
HEAD = (
    (1.862, ((0, .02), (.009, .019), (.016, .012), (.02, .002), (.018, -.007), (.015, -.012), (.01, -.017), (.005, -.018), (0, -.019))),
    (1.85, ((0, .06), (.026, .055), (.046, .035), (.056, .005), (.052, -.02), (.044, -.035), (.03, -.048), (.014, -.053), (0, -.055))),
    (1.825, ((0, .09), (.04, .083), (.068, .055), (.08, .01), (.076, -.025), (.064, -.052), (.044, -.07), (.02, -.08), (0, -.083))),
    (1.795, ((0, .105), (.048, .097), (.08, .065), (.09, .012), (.086, -.03), (.073, -.062), (.05, -.085), (.023, -.095), (0, -.098))),
    (1.77, ((0, .107), (.05, .098), (.082, .066), (.091, .012), (.087, -.03), (.075, -.066), (.052, -.093), (.024, -.106), (0, -.107))),
    (1.75, ((0, .104), (.048, .095), (.08, .063), (.089, .012), (.085, -.03), (.074, -.064), (.05, -.08), (.022, -.086), (0, -.1))),
    (1.73, ((0, .097), (.045, .089), (.076, .058), (.085, .012), (.082, -.03), (.074, -.066), (.052, -.088), (.022, -.09), (0, -.109))),
    (1.71, ((0, .085), (.04, .079), (.068, .05), (.08, .01), (.079, -.03), (.07, -.066), (.047, -.09), (.02, -.094), (0, -.12))),
    (1.69, ((0, .07), (.036, .066), (.06, .042), (.074, .008), (.074, -.028), (.065, -.062), (.043, -.085), (.018, -.094), (0, -.097))),
    (1.672, ((0, .062), (.034, .058), (.056, .036), (.07, .006), (.07, -.028), (.06, -.06), (.038, -.081), (.016, -.092), (0, -.096))),
    (1.655, ((0, .06), (.034, .055), (.054, .032), (.066, .004), (.066, -.026), (.055, -.056), (.034, -.077), (.014, -.085), (0, -.086))),
    (1.635, ((0, .058), (.034, .052), (.052, .028), (.06, .002), (.056, -.024), (.045, -.052), (.03, -.074), (.014, -.085), (0, -.09))),
    (1.618, ((0, .06), (.036, .054), (.054, .03), (.06, .004), (.052, -.018), (.04, -.036), (.027, -.052), (.013, -.06), (0, -.063))),
    (1.60, ((0, .063), (.036, .056), (.056, .032), (.062, .008), (.057, -.01), (.045, -.025), (.03, -.034), (.015, -.04), (0, -.042))),
)


def build_head(torso_top):
    # Set a little lower than drawn, for a shorter, thicker neck under the jaw.
    drop = 0.02
    rings = [[v3(x * (1.03 if k < len(HEAD) - 2 else 1.12), y - drop, z * 1.12 if z > 0 else z) for x, y, z in mirrored(row(y, half))]
             for k, (y, half) in enumerate(HEAD)]
    last = len(HEAD) - 1

    def weights(_, k, __):
        return {"head": 1} if k < last - 1 else {"head": 0.9, "neck": 0.1} if k == last - 1 else {"head": 0.45, "neck": 0.55}

    def shade(k, _):
        return 0.8 if k >= last - 1 else 0.92 if k == last - 2 else 1.0

    first = loft(rings, weights, FACE, shade=shade, vs=[(1.862 - y) / 0.262 for y, _ in HEAD],
                 inside=lambda k: v3(0, HEAD[k][0], 0), caps=(True, False))
    # Sixteen columns down to the trunk's twelve, with triangles under the jaw.
    neck = [first + last * 16 + i for i in range(16)]
    i = j = 0
    while i < 16 or j < 12:
        if i < 16 and (j >= 12 or (i + 1) / 16 <= (j + 1) / 12):
            tri = [neck[i], neck[(i + 1) % 16], torso_top[j % 12]]
            us = [i / 16, (i + 1) / 16, j / 12]
            i += 1
        else:
            tri = [neck[i % 16], torso_top[(j + 1) % 12], torso_top[j]]
            us = [i / 16, (j + 1) / 12, j / 12]
            j += 1
        vs = [0.0 if v in neck else 0.06 for v in tri]
        uvs = [cell_uv(TORSO, u, v) for u, v in zip(us, vs)]
        a, b, c = (VERTS[v] for v in tri)
        middle = (a + b + c) / 3
        if (b - a).cross(c - a).dot(middle - v3(0, middle.y, 0)) < 0:
            tri.reverse()
            uvs.reverse()
        FACES.append((tri, uvs, CLOTH, True))
    # Ears: small, set back and tilted, with a darker fold.
    for s in (-1, 1):
        ear = [[v3(s * (0.08 + (x - 0.08) * size), y, z + (cz - z) * (1 - size)) for x, z in ((0.08, 0.002), (0.095, 0.008), (0.093, 0.028), (0.078, 0.024))]
               for y, size, cz in ((1.738, 0.45, 0.024), (1.722, 1.0, 0.02), (1.702, 0.9, 0.012), (1.684, 0.5, 0.006))]
        loft(ear, lambda p, k, i: {"head": 1}, SKIN, shade=0.66, caps=(True, True))


def build_body():
    torso_top, sockets, groins = build_trunk()
    build_head(torso_top)
    for s, x in SIDES:
        build_arm(s, x, sockets[x])
        build_leg(s, x, groins[x])


# ---------------------------------------------------------------- Blender objects

def make_material(name, image, metallic, roughness):
    material = bpy.data.materials.new(name)
    material.use_nodes = True
    material.use_backface_culling = False
    nodes = material.node_tree.nodes
    shader = nodes["Principled BSDF"]
    shader.inputs["Metallic"].default_value = metallic
    shader.inputs["Roughness"].default_value = roughness
    texture = nodes.new("ShaderNodeTexImage")
    texture.image = image
    texture.interpolation = "Closest"
    material.node_tree.links.new(texture.outputs["Color"], shader.inputs["Base Color"])
    return material


def make_armature():
    data = bpy.data.armatures.new("wanderer_rig")
    armature = bpy.data.objects.new("wanderer", data)
    bpy.context.scene.collection.objects.link(armature)
    bpy.context.view_layer.objects.active = armature
    bpy.ops.object.mode_set(mode="EDIT")
    for name, (parent, head, tail) in BONES.items():
        edit = data.edit_bones.new(name)
        edit.head, edit.tail = to_blender(head), to_blender(tail)
        if parent:
            edit.parent = data.edit_bones[parent]
    bpy.ops.object.mode_set(mode="OBJECT")
    data.display_type = "STICK"
    return armature


def make_mesh(armature, image):
    mesh = bpy.data.meshes.new("body")
    mesh.from_pydata([to_blender(p) for p in VERTS], [], [ids for ids, _, _, _ in FACES])
    mesh.update()
    uv = mesh.uv_layers.new(name="UVMap")
    colors = mesh.color_attributes.new(name="grime", type="FLOAT_COLOR", domain="POINT")
    for i, shade in enumerate(SHADES):
        # Darken low-down and crevices; linear, since it multiplies the texture.
        value = max(0.0, shade) ** 2.2
        colors.data[i].color = (value, value, value, 1.0)
    mesh.color_attributes.active_color = colors
    mesh.color_attributes.render_color_index = 0
    for polygon, (_, uvs, material, smooth) in zip(mesh.polygons, FACES):
        polygon.material_index = material
        polygon.use_smooth = smooth
        for loop, value in zip(polygon.loop_indices, uvs):
            uv.data[loop].uv = value
    mesh.materials.append(make_material("skin", image, 0.0, 0.9))
    body = bpy.data.objects.new("body", mesh)
    bpy.context.scene.collection.objects.link(body)
    body.parent = armature
    groups = {name: body.vertex_groups.new(name=name) for name in BONES}
    for i, weights in enumerate(WEIGHTS):
        total = sum(weights.values())
        for name, weight in weights.items():
            groups[name].add([i], weight / total, "REPLACE")
    modifier = body.modifiers.new("rig", "ARMATURE")
    modifier.object = armature
    return body


# ---------------------------------------------------------------- clips

def old_quat(x=0.0, y=0.0, z=0.0):
    q = old.quat(x, y, z)
    return Quaternion((q[3], q[0], -q[2], q[1]))


GAIT = {"idle": 0.03, "walk": 0.14, "run": 0.5, "jump": 0.45, "attack": 0.18, "hurt": 0.3, "die": 0.12}


def deltas(pose, clip, p):
    """How much each new bone has turned from rest, in armature space."""
    old_rot = {}
    for name, parent, _ in old.JOINTS:
        f = pose.get(name, {})
        local = old_quat(f.get("x", 0), f.get("y", 0), f.get("z", 0))
        old_rot[name] = old_rot[parent] @ local if parent else local
    d = {name: old_rot[name] for name in ("hips", "spine", "chest", "head")}
    d["root"] = Quaternion()
    d["neck"] = old_rot["chest"].slerp(old_rot["head"], 0.4)
    for s, x in SIDES:
        back_to_rest = old_quat(z=s * ARM_ANGLE).inverted()
        arm = old_rot[f"upper_arm.{x}"]
        d[f"clavicle.{x}"] = old_rot["chest"] @ Quaternion().slerp(old_rot["chest"].inverted() @ arm, 0.15)
        d[f"upper_arm.{x}"] = arm @ back_to_rest
        d[f"forearm.{x}"] = old_rot[f"forearm.{x}"] @ back_to_rest
        d[f"hand.{x}"] = d[f"forearm.{x}"]  # the old staff hand bent to keep the staff upright
        # The clips that come with a sword in hand close it; nobody else's does.
        curl = anim_kit.FIST if x == "R" and clip in ARMED_OLD_CLIPS else anim_kit.OPEN
        d.update(anim_kit.finger_rotations(x, d[f"hand.{x}"], curl))
        for leg in ("thigh", "shin", "foot"):
            d[f"{leg}.{x}"] = old_rot[f"{leg}.{x}"]
        d[f"toe.{x}"] = d[f"foot.{x}"]
    d["weapon.R"] = d["hand.R"] @ anim_kit.WEAPON_IN_HAND
    gait = GAIT[clip]
    d["lantern"] = old_rot["hips"] @ old_quat(x=gait * 0.7 * math.sin(p * 2 - 0.9))
    # Joints for future cloth: it trails the motion and the legs push the panels.
    for x, lag in (("L", 0.25), ("C", 0.0), ("R", -0.25)):
        turn = old_rot["chest"]
        for i in range(1, 4):
            swing = gait * (0.22 + 0.12 * i) + gait * 0.26 * math.sin(p * 2 - i * 0.9 + lag)
            turn = turn @ old_quat(x=-swing)
            d[f"cape.{x}.{i}"] = turn
    knees = [pose.get(f"thigh.{x}", {}).get("x", 0.0) for _, x in SIDES]
    forward = max(0.0, *knees) * 0.8
    backward = min(0.0, *knees) * 0.8
    d["tabard.F.1"] = old_rot["hips"] @ old_quat(x=forward)
    d["tabard.F.2"] = d["tabard.F.1"] @ old_quat(x=-forward * 0.3)
    d["tabard.B.1"] = old_rot["hips"] @ old_quat(x=backward)
    d["tabard.B.2"] = d["tabard.B.1"] @ old_quat(x=-backward * 0.3)
    return d


def make_clips(armature):
    rest = {b.name: b.matrix_local.to_quaternion() for b in armature.data.bones}
    armature.animation_data_create()
    made = {}
    for clip, duration, pose_at in old.CLIPS:
        action = bpy.data.actions.new(clip)
        action.use_fake_user = True
        armature.animation_data.action = action
        steps = round(duration * 30)
        previous = {}
        for k in range(steps + 1):
            p = math.tau * k / steps
            pose = pose_at(p)
            d = deltas(pose, clip, p)
            for name, (parent, _, _) in BONES.items():
                turned = d[name]
                above = d[parent] if parent else Quaternion()
                q = rest[name].inverted() @ above.inverted() @ turned @ rest[name]
                if name in previous and previous[name].dot(q) < 0:
                    q = -q
                previous[name] = q
                pose_bone = armature.pose.bones[name]
                pose_bone.rotation_mode = "QUATERNION"
                pose_bone.rotation_quaternion = q
                pose_bone.keyframe_insert("rotation_quaternion", frame=k * 2)
            t = pose["hips"].get("t", (0, 0.95, 0))
            hips = armature.pose.bones["hips"]
            hips.location = rest["hips"].inverted() @ to_blender((t[0], t[1] - 0.95, t[2]))
            hips.keyframe_insert("location", frame=k * 2)
        made[clip] = (action, steps / 30)
        track = armature.animation_data.nla_tracks.new()
        track.name = clip
        track.strips.new(clip, 0, action)
        armature.animation_data.action = None
    make_moves(armature, rest, made)
    for pose_bone in armature.pose.bones:
        pose_bone.rotation_quaternion = Quaternion()
        pose_bone.location = Vector()
    return made


def make_moves(armature, rest, made):
    """The clips written with anim_kit, keyed on every frame at 60 Hz."""
    rig = anim_kit.Rig(BONES)
    for clip in sword_moves.clips():
        action = bpy.data.actions.new(clip.name)
        action.use_fake_user = True
        armature.animation_data.action = action
        frames = round(clip.duration * anim_kit.FPS)
        previous = {}
        for k in range(frames + 1):
            controls = clip.sample(k / anim_kit.FPS)
            delta, position, _ = rig.solve(controls)
            rotations = anim_kit.local_rotations(rig, rest, delta, previous)
            for name, q in rotations.items():
                pose_bone = armature.pose.bones[name]
                pose_bone.rotation_mode = "QUATERNION"
                pose_bone.rotation_quaternion = q
                pose_bone.keyframe_insert("rotation_quaternion", frame=k)
            hips = armature.pose.bones["hips"]
            hips.location = rest["hips"].inverted() @ to_blender(position["hips"] - v3(*BONES["hips"][1]))
            hips.keyframe_insert("location", frame=k)
            root = armature.pose.bones["root"]
            root.location = rest["root"].inverted() @ to_blender((controls["root"][0], 0.0, controls["root"][1]))
            root.keyframe_insert("location", frame=k)
        made[clip.name] = (action, frames / anim_kit.FPS)
        EVENTS[clip.name] = list(clip.events)
        track = armature.animation_data.nla_tracks.new()
        track.name = clip.name
        track.strips.new(clip.name, 0, action)
        armature.animation_data.action = None
        anim_kit.report(rig, clip)


def export(path, clips):
    bpy.ops.object.select_all(action="DESELECT")
    bpy.ops.export_scene.gltf(
        filepath=path, export_format="GLTF_SEPARATE", export_yup=True, export_animations=True,
        export_animation_mode="NLA_TRACKS", export_frame_step=1, export_force_sampling=True, export_skins=True,
        export_def_bones=False, export_vertex_color="ACTIVE", export_all_vertex_colors=False,
        export_active_vertex_color_when_no_material=True, export_materials="EXPORT", export_cameras=False,
        export_lights=False, export_extras=False)
    with open(path, encoding="utf-8") as f:
        doc = json.load(f)
    for animation in doc.get("animations", []):
        clip = animation["name"]
        if clip in old.EVENTS:
            duration = clips[clip][1]
            animation["extras"] = {"events": [{"time": duration * at, "name": n} for at, n in old.EVENTS[clip]]}
        elif EVENTS.get(clip):
            animation["extras"] = {"events": [{"time": at, "name": n} for at, n in EVENTS[clip]]}
    doc["asset"]["generator"] = "Grimora tools/blender/build_wanderer.py"
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        json.dump(doc, f, indent=1)
    triangles = sum(len(p.vertices) - 2 for p in bpy.data.meshes["body"].polygons)
    print(f"[wanderer] {triangles} triangles, {len(VERTS)} vertices, {len(BONES)} joints")
    print("[wanderer] clips:", [a["name"] for a in doc.get("animations", [])])
    print("[wanderer] attributes:", sorted(doc["meshes"][0]["primitives"][0]["attributes"]))
    print("[wanderer] samplers:", doc.get("samplers"))


def sword_in_hand(armature):
    """The sword on the weapon joint, for pictures. Returns what to remove afterwards."""
    scene = bpy.context.scene
    scene.render.engine = "BLENDER_WORKBENCH"
    scene.display.shading.light = "STUDIO"
    scene.display.shading.color_type = "MATERIAL"
    scene.display.shading.show_backface_culling = False
    bpy.ops.import_scene.gltf(filepath=os.path.join(HERE, "..", "..", "assets", "models", "weapons", "espada_reta.glb"))
    sword = [o for o in bpy.context.selected_objects if o.type == "MESH"]
    holder = bpy.data.objects.new("holder", None)
    scene.collection.objects.link(holder)
    holder.parent = armature
    holder.parent_type = "BONE"
    holder.parent_bone = "weapon.R"
    bone_data = armature.pose.bones["weapon.R"]
    holder.matrix_parent_inverse = Matrix.Identity(4)
    # The bone's parent space starts at its tail; the grip is at its head.
    holder.location = (0, -bone_data.length, 0)
    holder.scale = (SWORD_SCALE,) * 3
    # The imported blade runs along z and its edges along x: along the bone, edges to its z.
    holder.rotation_mode = "QUATERNION"
    holder.rotation_quaternion = Matrix(((0, 1, 0), (0, 0, 1), (1, 0, 0))).to_quaternion()
    for o in sword:
        o.parent = holder
        o.matrix_parent_inverse = Matrix.Identity(4)
    return sword + [holder]


def closeup(folder, armature, clips, name):
    """The hand on the grip from four sides, at a clip's first frame: `hand_<n>.png`."""
    scene = bpy.context.scene
    added = sword_in_hand(armature)
    scene.render.resolution_x, scene.render.resolution_y = 420, 420
    camera = bpy.data.objects.new("near", bpy.data.cameras.new("near"))
    camera.data.lens = 70
    scene.collection.objects.link(camera)
    scene.camera = camera
    for track in armature.animation_data.nla_tracks:
        track.mute = True
    armature.animation_data.action = clips[name][0]
    scene.frame_set(0)
    bone_data = armature.pose.bones["weapon.R"]
    target = armature.matrix_world @ bone_data.head
    for k, offset in enumerate(((0.6, 0.0, 0.05), (-0.6, 0.0, 0.05), (0.0, 0.6, 0.1), (0.15, -0.1, -0.6))):
        camera.location = target + Vector(offset)
        camera.rotation_mode = "QUATERNION"
        camera.rotation_quaternion = (target - camera.location).to_track_quat("-Z", "Y")
        scene.render.filepath = os.path.join(folder, f"hand_{k}.png")
        bpy.ops.render.render(write_still=True)
    armature.animation_data.action = None
    for track in armature.animation_data.nla_tracks:
        track.mute = False
    for o in added + [camera]:
        bpy.data.objects.remove(o)


def strips(folder, armature, clips, names):
    """A sheet per clip of 8 frames, closer together early where blows happen: `<clip>.png`."""
    scene = bpy.context.scene
    added = sword_in_hand(armature)
    scene.render.resolution_x, scene.render.resolution_y = 300, 360
    camera = bpy.data.objects.new("strip", bpy.data.cameras.new("strip"))
    camera.data.lens = 45
    scene.collection.objects.link(camera)
    scene.camera = camera
    for track in armature.animation_data.nla_tracks:
        track.mute = True
    for name in names:
        action, duration = clips[name]
        armature.animation_data.action = action
        count = 8
        columns = []
        for k in range(count):
            frame = round(duration * 60 * (k / (count - 1)) ** 1.7)
            scene.frame_set(frame)
            hips = armature.matrix_world @ armature.pose.bones["root"].head
            target = Vector((hips.x, hips.y, 0.95))
            camera.location = target + Vector((3.4, -1.3, 0.5))
            camera.rotation_mode = "QUATERNION"
            camera.rotation_quaternion = (target - camera.location).to_track_quat("-Z", "Y")
            path = os.path.join(folder, f"_{name}_{k}.png")
            scene.render.filepath = path
            bpy.ops.render.render(write_still=True)
            image = bpy.data.images.load(path)
            w, h = image.size
            pixels = np.empty(w * h * 4, dtype=np.float32)
            image.pixels.foreach_get(pixels)
            columns.append(pixels.reshape(h, w, 4))
            bpy.data.images.remove(image)
            os.remove(path)
        top = np.concatenate(columns[:4], axis=1)
        bottom = np.concatenate(columns[4:], axis=1)
        sheet = np.concatenate([bottom, top], axis=0)
        out = bpy.data.images.new(name, sheet.shape[1], sheet.shape[0])
        out.pixels.foreach_set(sheet.ravel())
        out.filepath_raw = os.path.join(folder, f"{name}.png")
        out.file_format = "PNG"
        out.save()
    armature.animation_data.action = None
    for track in armature.animation_data.nla_tracks:
        track.mute = False
    for o in added + [camera]:
        bpy.data.objects.remove(o)


def preview(folder, armature, clips):
    """A quick sheet of stills, for looking at shapes and weights."""
    scene = bpy.context.scene
    scene.render.engine = "BLENDER_WORKBENCH"
    scene.display.shading.light = "STUDIO"
    scene.display.shading.color_type = "TEXTURE"
    scene.display.shading.show_backface_culling = False
    scene.render.resolution_x, scene.render.resolution_y = 380, 560
    camera = bpy.data.objects.new("preview", bpy.data.cameras.new("preview"))
    camera.data.lens = 85
    scene.collection.objects.link(camera)
    scene.camera = camera
    views = (("front", None, 0, (1.2, 5.2, 1.05)), ("back", None, 0, (-1.6, -5.0, 1.15)),
             ("walk", "run", 9, (5.2, 1.5, 1.0)), ("attack", "attack", 15, (2.6, 4.6, 1.15)),
             ("face", None, 0, (0.0, 1.6, 1.74)), ("face_34", None, 0, (0.8, 1.35, 1.74)), ("face_side", None, 0, (1.6, 0.0, 1.74)))
    for name, clip, frame, at in views:
        armature.animation_data.action = clips[clip][0] if clip else None
        for track in armature.animation_data.nla_tracks:
            track.mute = True
        if not clip:
            for pose_bone in armature.pose.bones:
                pose_bone.rotation_quaternion = Quaternion()
                pose_bone.location = Vector()
        scene.frame_set(frame)
        camera.location = at
        target = Vector((0, 0, 1.74 if name.startswith("face") else 0.95))
        camera.rotation_mode = "QUATERNION"
        camera.rotation_quaternion = (target - Vector(at)).to_track_quat("-Z", "Y")
        scene.render.filepath = os.path.join(folder, f"{name}.png")
        bpy.ops.render.render(write_still=True)
    armature.animation_data.action = None
    for track in armature.animation_data.nla_tracks:
        track.mute = False
    bpy.data.objects.remove(camera)


def main():
    args = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    if "--export-clips" in args:  # the moveset as text; overwrites what the animator saved
        folder = os.path.join(OUT, "wanderer_clips")
        os.makedirs(folder, exist_ok=True)
        for clip in sword_moves.clips():
            anim_kit.export_clip(clip, folder)
        return
    if "--metrics" in args:  # just the report on the clips, in a second or two
        build_skeleton()
        rig = anim_kit.Rig(BONES)
        names = args[args.index("--metrics") + 1:]
        for clip in sword_moves.clips():
            anim_kit.report(rig, clip)
            if clip.name in names:
                anim_kit.trace(rig, clip)
        return
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.context.preferences.filepaths.save_version = 0
    scene = bpy.context.scene
    scene.render.fps = 60
    build_skeleton()
    image = paint_atlas()
    build_body()
    armature = make_armature()
    make_mesh(armature, image)
    clips = make_clips(armature)
    out = os.path.normpath(OUT)
    os.makedirs(out, exist_ok=True)
    export(os.path.join(out, "wanderer.gltf"), clips)
    if "--preview" in args:
        preview(args[args.index("--preview") + 1], armature, clips)
    if "--strips" in args:
        at = args.index("--strips")
        strips(args[at + 1], armature, clips, args[at + 2:])
    if "--closeup" in args:
        at = args.index("--closeup")
        closeup(args[at + 1], armature, clips, args[at + 2])
    bpy.ops.wm.save_as_mainfile(filepath=os.path.join(out, "wanderer.blend"), compress=True)


main()
