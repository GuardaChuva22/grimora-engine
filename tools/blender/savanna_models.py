# Savanna acacias, bushes, termite mounds, rocks and dead wood for the generated island, as GLBs with their textures embedded.
#
#   blender --background --python tools/blender/savanna_models.py -- <out_dir> [name...]
#
# Every model has its origin at the centre of its base, metres, +Y up once exported. Each file
# holds the model (`<Name>`), its LODs (`<Name>_LOD1`, `<Name>_LOD2`, with `screen_size` in the
# node's extras) and, for what the player bumps into, a collision mesh (`<Name>_COL`, never drawn).
# Foliage materials are named `foliage_*` (alpha mask, double sided, lit like a plant). Vertex
# colours darken the inner and lower parts, in the engine's sRGB space. The same name and seed
# always give the same model.

import bpy
import bmesh
import json
import math
import random
import struct
import sys
from pathlib import Path

import numpy as np
from mathutils import Vector, noise

ROOT = Path(__file__).resolve().parents[2]
TEX = ROOT / "assets" / "textures"
UP = Vector((0.0, 0.0, 1.0))

# ---------------------------------------------------------------------------- images, materials

IMAGES = {}
MATERIALS = {}


def image(name):
    """A texture and the box (u0, v0, u1, v1) its drawn pixels fill, in Blender's UV space."""
    if name in IMAGES:
        return IMAGES[name]
    path = OUT / f"{name}.png" if (OUT / f"{name}.png").exists() else TEX / f"{name}.png"
    img = bpy.data.images.load(str(path), check_existing=True)
    w, h = img.size
    px = np.array(img.pixels[:], dtype=np.float32).reshape(h, w, 4)
    solid = px[:, :, 3] > 0.5
    rows = np.where(solid.any(axis=1))[0]
    cols = np.where(solid.any(axis=0))[0]
    if len(rows) and len(cols) and solid.mean() < 0.98:
        box = (cols[0] / w, rows[0] / h, (cols[-1] + 1) / w, (rows[-1] + 1) / h)
    else:
        box = (0.0, 0.0, 1.0, 1.0)
    IMAGES[name] = (img, box, px)
    return IMAGES[name]


def derive(name, source, fn):
    """Writes a variant of a texture next to the models (the original stays as it is)."""
    img, _, px = image(source)
    out = fn(px.copy())
    h, w = out.shape[:2]
    new = bpy.data.images.new(name, w, h, alpha=True)
    new.pixels = out.reshape(-1).tolist()
    new.filepath_raw = str(OUT / f"{name}.png")
    new.file_format = "PNG"
    new.save()
    bpy.data.images.remove(new)
    return name


def material(key, tex, foliage=False):
    if key in MATERIALS:
        return key
    img = image(tex)[0]
    m = bpy.data.materials.new(key)
    m.use_nodes = True
    nodes = m.node_tree.nodes
    bsdf = nodes["Principled BSDF"]
    t = nodes.new("ShaderNodeTexImage")
    t.image = img
    t.interpolation = "Linear"
    m.node_tree.links.new(t.outputs["Color"], bsdf.inputs["Base Color"])
    if foliage:
        m.node_tree.links.new(t.outputs["Alpha"], bsdf.inputs["Alpha"])
    bsdf.inputs["Roughness"].default_value = 1.0
    bsdf.inputs["Metallic"].default_value = 0.0
    MATERIALS[key] = m
    return key


# ---------------------------------------------------------------------------- geometry


class Part:
    """Faces with every corner's position, uv, colour and normal (None = the face's own)."""

    def __init__(self):
        self.faces = []

    def add(self, mat, corners):
        self.faces.append((mat, corners))

    def extend(self, other):
        self.faces.extend(other.faces)

    def triangles(self):
        return sum(len(c) - 2 for _, c in self.faces)


def face_normal(points):
    n = Vector()
    for i in range(1, len(points) - 1):
        n += (points[i] - points[0]).cross(points[i + 1] - points[0])
    return n.normalized() if n.length > 1e-9 else UP.copy()


def smoothstep(a, b, x):
    t = min(max((x - a) / (b - a), 0.0), 1.0)
    return t * t * (3 - 2 * t)


def lerp(a, b, t):
    return a + (b - a) * t


def catmull(points, per):
    """Catmull-Rom through `points`, `per` steps between each pair."""
    if len(points) < 3 or per <= 1:
        return [p.copy() for p in points]
    out = []
    pts = [points[0] * 2 - points[1]] + list(points) + [points[-1] * 2 - points[-2]]
    for i in range(1, len(pts) - 2):
        p0, p1, p2, p3 = pts[i - 1], pts[i], pts[i + 1], pts[i + 2]
        for s in range(per):
            t = s / per
            out.append(
                0.5
                * (
                    2 * p1
                    + (-p0 + p2) * t
                    + (2 * p0 - 5 * p1 + 4 * p2 - p3) * t * t
                    + (-p0 + 3 * p1 - 3 * p2 + p3) * t * t * t
                )
            )
    out.append(points[-1].copy())
    return out


def resample_radii(radii, n):
    if n == len(radii):
        return list(radii)
    out = []
    for i in range(n):
        x = i / (n - 1) * (len(radii) - 1)
        k = min(int(x), len(radii) - 2)
        out.append(lerp(radii[k], radii[k + 1], x - k))
    return out


def tube(part, mat, pts, radii, sides, shade, tex=(0.6, 1.2), flute=None, cap=False, v0=0.0):
    """A faceted tube along `pts`, bark running along it. `shade(point)` gives the colour."""
    n = len(pts)
    tangents = [(pts[min(i + 1, n - 1)] - pts[max(i - 1, 0)]).normalized() for i in range(n)]
    ref = UP if abs(tangents[0].z) < 0.95 else Vector((1.0, 0.0, 0.0))
    normal = tangents[0].cross(ref).normalized()
    frames = []
    for i in range(n):
        if i:
            normal = (tangents[i - 1].rotation_difference(tangents[i]) @ normal).normalized()
        frames.append((normal, tangents[i].cross(normal)))
    around = max(1, round(2 * math.pi * radii[0] / tex[0]))
    s = [0.0]
    for i in range(1, n):
        s.append(s[-1] + (pts[i] - pts[i - 1]).length)
    rings = []
    for i in range(n):
        nrm, bi = frames[i]
        dr = (radii[min(i + 1, n - 1)] - radii[max(i - 1, 0)]) / max(s[min(i + 1, n - 1)] - s[max(i - 1, 0)], 1e-6)
        ring = []
        for j in range(sides + 1):
            a = 2 * math.pi * (j % sides) / sides
            d = nrm * math.cos(a) + bi * math.sin(a)
            r = radii[i] * (flute(i / max(n - 1, 1), a, pts[i]) if flute else 1.0)
            p = pts[i] + d * r
            ring.append((p, (j / sides * around, v0 + s[i] / tex[1]), (d - tangents[i] * dr).normalized()))
        rings.append(ring)
    for i in range(n - 1):
        for j in range(sides):
            c = [rings[i][j], rings[i][j + 1], rings[i + 1][j + 1], rings[i + 1][j]]
            if radii[i + 1] < 1e-4:
                c = c[:3]
            part.add(mat, [(p, uv, shade(p), nr) for p, uv, nr in c])
    if cap and radii[-1] > 1e-4:
        tip = pts[-1] + tangents[-1] * radii[-1] * 0.4
        ring = rings[-1]
        for j in range(sides):
            c = [ring[j][0], ring[j + 1][0], tip]
            part.add(mat, [(p, (0.5 + 0.3 * math.cos(k), 0.5 + 0.3 * math.sin(k)), shade(p), None) for k, p in enumerate(c)])


def card(part, mat, tex, origin, xaxis, yaxis, width, height, anchor, shade, normal, flip=False):
    """An alpha-cut picture: `xaxis` along the image's width, `yaxis` up it; `anchor` 0..1 in it."""
    u0, v0, u1, v1 = image(tex)[1]
    corners = []
    for a, b in ((0, 0), (1, 0), (1, 1), (0, 1)):
        p = origin + xaxis * ((a - anchor[0]) * width) + yaxis * ((b - anchor[1]) * height)
        u = u0 + (u1 - u0) * ((1 - a) if flip else a)
        corners.append((p, (u, v0 + (v1 - v0) * b), shade(p), normal(p)))
    part.add(mat, corners)


def aspect(tex):
    """Height over width of the drawn part of a picture."""
    img, (u0, v0, u1, v1), _ = image(tex)
    w, h = img.size
    return ((v1 - v0) * h) / ((u1 - u0) * w)


def stem_at(tex):
    """Where a branch picture's stem leaves its left edge, as 0..1 of the drawn height."""
    img, (u0, v0, u1, v1), px = image(tex)
    w, h = img.size
    col = int(u0 * w) + 1
    rows = np.where(px[:, col, 3] > 0.5)[0]
    if not len(rows):
        return 0.5
    return (rows.mean() / h - v0) / (v1 - v0)


def horizontal(angle):
    return Vector((math.cos(angle), math.sin(angle), 0.0))


def dome_normal(center, radii, lift):
    def fn(p):
        d = p - center
        n = Vector((d.x / radii[0], d.y / radii[1], d.z / radii[2]))
        return (n.normalized() + UP * lift).normalized()

    return fn


# ---------------------------------------------------------------------------- tree skeletons


class Branch:
    def __init__(self, points, r0, r1, level):
        self.points = points
        self.r0 = r0
        self.r1 = r1
        self.level = level

    def build(self, part, mat, sides, per, shade, tex=(0.6, 1.2), flute=None, cap=False, flare=0.0):
        pts = catmull(self.points, per)
        radii = [lerp(self.r0, self.r1, i / (len(pts) - 1)) for i in range(len(pts))]
        if flare:
            radii = [r * (1 + flare * (1 - smoothstep(-0.4, 0.6, p.z))) for r, p in zip(radii, pts)]
        tube(part, mat, pts, radii, sides, shade, tex, flute, cap)


def crooked(rng, a, b, bends, amount, rise=0.0):
    """Control points from a to b with sideways kinks (and an upward bow by `rise`)."""
    pts = [a.copy()]
    d = b - a
    side = d.cross(UP)
    if side.length < 1e-6:
        side = Vector((1.0, 0.0, 0.0))
    side.normalize()
    other = d.normalized().cross(side)
    for k in range(1, bends + 1):
        t = k / (bends + 1)
        off = side * rng.uniform(-amount, amount) + other * rng.uniform(-amount, amount) * 0.6
        pts.append(a + d * t + off * d.length + UP * (math.sin(t * math.pi) * rise))
    pts.append(b.copy())
    return pts


def poisson_disc(rng, rx, ry, spacing, tries=600):
    pts = []
    for _ in range(tries):
        x, y = rng.uniform(-1, 1), rng.uniform(-1, 1)
        if x * x + y * y > 1:
            continue
        p = Vector((x * rx, y * ry, 0.0))
        if all((p - q).length >= spacing for q in pts):
            pts.append(p)
    return pts


def kmeans(points, k, rng, steps=8):
    centers = rng.sample(points, min(k, len(points)))
    groups = []
    for _ in range(steps):
        groups = [[] for _ in centers]
        for p in points:
            i = min(range(len(centers)), key=lambda c: (p - centers[c]).length)
            groups[i].append(p)
        centers = [sum(g, Vector()) / len(g) if g else centers[i] for i, g in enumerate(groups)]
    return [(c, g) for c, g in zip(centers, groups) if g]


# ---------------------------------------------------------------------------- acacia


def acacia(name, seed, height, width, trunk, limbs, lean, tiers=1, squash=0.85):
    """Flat-topped umbrella acacia: a short trunk forking into spreading limbs under a crown of pads."""
    rng = random.Random(seed)
    bark = material("bark_acacia", "casca_acacia")
    crown_mat = material("foliage_acacia_crown", "copa_acacia_plana", foliage=True)
    spray_mat = material("foliage_acacia_spray", "galho_acacia", foliage=True)

    lean_dir = horizontal(rng.uniform(0, 2 * math.pi))
    fork = Vector((0, 0, trunk)) + lean_dir * (trunk * math.tan(math.radians(lean)))
    trunk_r = 0.11 + height * 0.022
    crown_c = Vector((fork.x, fork.y, 0)) + lean_dir * width * 0.06
    rx, ry = width * 0.5, width * 0.5 * squash
    top = height

    # Pads: discs of foliage filling the crown's ellipse, the middle a little higher.
    pad_r = width * 0.15
    pads = []
    for p in poisson_disc(rng, rx - pad_r * 0.55, ry - pad_r * 0.55, pad_r * 1.05):
        d = math.hypot(p.x / rx, p.y / ry)
        z = top - 0.45 - 0.55 * d * d + rng.uniform(-0.15, 0.15)
        pads.append((crown_c + Vector((p.x, p.y, z)), pad_r * rng.uniform(0.85, 1.15)))
    if tiers > 1:
        for p in poisson_disc(rng, rx * 0.75, ry * 0.75, pad_r * 1.3)[:5]:
            pads.append((crown_c + Vector((p.x, p.y, top - 1.7 + rng.uniform(-0.2, 0.2))), pad_r * 0.85))

    # Limbs: from the fork out under the crown, each feeding the pads nearest to its end.
    base_a = rng.uniform(0, 2 * math.pi)
    branches = [Branch(crooked(rng, Vector((0, 0, -0.45)), fork, 2, 0.06), trunk_r, trunk_r * 0.82, 0)]
    limb_ends = []
    for k in range(limbs):
        a = base_a + 2 * math.pi * k / limbs + rng.uniform(-0.35, 0.35)
        reach = rng.uniform(0.38, 0.55)
        end = crown_c + Vector((math.cos(a) * rx * reach, math.sin(a) * ry * reach, top - 1.5 - rng.uniform(0, 0.5)))
        start = fork + horizontal(a) * trunk_r * 0.3 - UP * 0.15
        rise = 0.25 * (end - start).length * rng.uniform(0.0, 0.4)
        r0 = trunk_r * (0.85 if trunk < 1.4 else 0.68)
        branches.append(Branch(crooked(rng, start, end, 2, 0.1, rise), r0, r0 * 0.45, 1))
        limb_ends.append(end)
    owner = {i: min(range(len(limb_ends)), key=lambda k: (pads[i][0] - limb_ends[k]).length) for i in range(len(pads))}
    twigs = []
    for k, end in enumerate(limb_ends):
        mine = [pads[i][0] for i in owner if owner[i] == k]
        if not mine:
            continue
        r_limb = branches[1 + k].r1
        for center, group in kmeans(mine, 2 if len(mine) > 2 else 1, rng):
            sub_end = center - UP * 0.55
            sub_end = end + (sub_end - end) * 0.55
            branches.append(Branch(crooked(rng, end, sub_end, 1, 0.12), r_limb * 0.8, r_limb * 0.5, 2))
            for p in group:
                tip = p - UP * 0.18
                twigs.append(Branch(crooked(rng, sub_end, tip, 1, 0.15), r_limb * 0.42, 0.012, 3))

    def bark_shade(p):
        return (lerp(0.55, 1.0, smoothstep(-0.3, 1.2, p.z)) * lerp(1.0, 0.82, smoothstep(top - 2.2, top - 0.6, p.z)),) * 3 + (1.0,)

    crown_mid = crown_c + Vector((0, 0, top - 0.9))
    normal = dome_normal(crown_mid, (rx, ry, 1.2), 0.9)
    tints = [rng.uniform(0.9, 1.04) for _ in pads]

    def leaf_shade(tint):
        def fn(p):
            v = tint * lerp(0.58, 1.0, smoothstep(top - 2.0, top - 0.1, p.z))
            v *= lerp(0.86, 1.0, smoothstep(0.0, 0.7, math.hypot((p.x - crown_c.x) / rx, (p.y - crown_c.y) / ry)))
            return (v, v, v * 0.97, 1.0)

        return fn

    def pad_cards(part, center, r, tint, sprays, verticals):
        out = horizontal(math.atan2(center.y - crown_c.y, center.x - crown_c.x)) if (center - crown_mid).xy.length > 0.3 else horizontal(rng.uniform(0, 6.3))
        shade = leaf_shade(tint)
        a0 = math.atan2(out.y, out.x)
        w = 2 * r * 1.2
        h = w * aspect("copa_acacia_plana")
        for k in range(verticals):
            yaw = a0 + math.pi * k / verticals + rng.uniform(-0.25, 0.25)
            x = horizontal(yaw)
            card(part, crown_mat, "copa_acacia_plana", center - UP * (h * 0.42), x, UP, w, h, (0.5, 0.0), shade, normal, flip=rng.random() < 0.5)
        for k in range(sprays):
            yaw = a0 + (k - (sprays - 1) / 2) * 1.0 + rng.uniform(-0.3, 0.3)
            x = horizontal(yaw)
            tilt = math.radians(rng.uniform(4, 16))
            x = (x * math.cos(tilt) + UP * math.sin(tilt)).normalized()
            y = UP.cross(horizontal(yaw)).normalized()
            length = r * 1.75
            wide = length * aspect("galho_acacia") * 2.2
            origin = center + UP * rng.uniform(0.0, 0.25) - horizontal(yaw) * r * 0.25
            card(part, spray_mat, "galho_acacia", origin, x, y, length, wide, (0.0, 0.5), shade, normal, flip=False)

    lod0, lod1, lod2 = Part(), Part(), Part()
    sides0 = {0: 8, 1: 6, 2: 5, 3: 3}
    for b in branches:
        b.build(lod0, bark, sides0[b.level], 3, bark_shade, flare=0.35 if b.level == 0 else 0.0)
    for b in twigs:
        b.build(lod0, bark, sides0[3], 2, bark_shade)
    for (center, r), tint in zip(pads, tints):
        pad_cards(lod0, center, r, tint, 3, 2)

    sides1 = {0: 6, 1: 4, 2: 3}
    for b in branches:
        b.build(lod1, bark, sides1[b.level], 2, bark_shade, flare=0.3 if b.level == 0 else 0.0)
    for center, group in kmeans([c for c, _ in pads], max(4, len(pads) // 3), rng):
        r = pad_r * math.sqrt(len(group)) * 0.95
        z = sum((p.z for p in group)) / len(group)
        pad_cards(lod1, Vector((center.x, center.y, z)), r, 0.97, 2, 2)

    for b in branches[: 1 + limbs]:
        b.build(lod2, bark, 3, 1, bark_shade)
    shade = leaf_shade(0.97)
    w = width * 1.1
    h = w * aspect("copa_acacia_plana") * 0.9
    for k in range(2):
        x = horizontal(base_a + math.pi / 2 * k)
        card(lod2, crown_mat, "copa_acacia_plana", crown_c + UP * (top - 0.45 - h * 0.6), x, UP, w, h, (0.5, 0.0), shade, normal)
    for k in range(3):
        yaw = base_a + 2 * math.pi * k / 3
        y = UP.cross(horizontal(yaw))
        card(lod2, spray_mat, "galho_acacia", crown_c + UP * (top - 0.5), horizontal(yaw), y, width * 0.55, width * 0.5, (0.0, 0.5), shade, normal)

    col = Part()
    collision_tubes(col, branches[: 1 + limbs], 2.4)
    return {"lods": [lod0, lod1, lod2], "screen": [0.3, 0.08], "col": col, "wind": 0.1 / (height * height)}


def collision_tubes(part, branches, below):
    """The parts of the trunk and limbs a walker reaches, as closed prisms."""
    for b in branches:
        pts = catmull(b.points, 2)
        radii = [lerp(b.r0, b.r1, i / (len(pts) - 1)) for i in range(len(pts))]
        keep = [i for i, p in enumerate(pts) if p.z < below]
        if len(keep) < 2:
            continue
        sel = pts[: keep[-1] + 2] if keep[-1] + 1 < len(pts) else pts
        rad = radii[: len(sel)]
        tube(part, "col", sel, [r * 1.05 for r in rad], 6, lambda p: (1, 1, 1, 1), cap=True)


def bleach(px):
    """Bark weathered to grey: dead wood in the sun."""
    rgb = px[:, :, :3]
    grey = rgb.mean(axis=2, keepdims=True)
    px[:, :, :3] = np.clip((grey * 0.8 + rgb * 0.2) * np.array([1.0, 0.97, 0.92], dtype=np.float32) * 1.45 + 0.04, 0, 1)
    return px


# ---------------------------------------------------------------------------- bushes


def bush(name, seed, tex, height, width, thorny):
    rng = random.Random(seed)
    leaf = material(f"foliage_{tex}", tex, foliage=True)
    wood = material("bark_acacia", "casca_acacia")
    c = Vector((0, 0, height * 0.45))
    normal = dome_normal(c, (width * 0.5, width * 0.5, height * 0.6), 0.6)
    tints = {}

    def shade_for(k):
        t = tints.setdefault(k, rng.uniform(0.9, 1.05))

        def fn(p):
            v = t * (0.82 if thorny else 0.95) * lerp(0.5, 1.0, smoothstep(0.0, height * 0.8, p.z))
            return (v, v, v, 1.0)

        return fn

    h_per_w = aspect(tex)

    def cards(part, crossing, rim):
        a0 = rng.uniform(0, 6.3)
        for k in range(crossing):
            yaw = a0 + math.pi * k / crossing + rng.uniform(-0.2, 0.2)
            w = width * rng.uniform(0.85, 1.0)
            off = horizontal(rng.uniform(0, 6.3)) * width * 0.06
            card(part, leaf, tex, off - UP * 0.04, horizontal(yaw), UP, w, w * h_per_w, (0.5, 0.0), shade_for(k), normal, flip=k % 2 == 1)
        for k in range(rim):
            yaw = a0 + 2 * math.pi * (k + 0.5) / rim
            out = horizontal(yaw)
            tang = UP.cross(out).normalized()
            lean = math.radians(rng.uniform(20, 35))
            y = (UP * math.cos(lean) + out * math.sin(lean)).normalized()
            w = width * rng.uniform(0.55, 0.7)
            card(part, leaf, tex, out * width * 0.22 - UP * 0.04, tang, y, w, w * h_per_w, (0.5, 0.0), shade_for(10 + k), normal, flip=k % 2 == 0)

    def wood_shade(p):
        v = lerp(0.5, 0.85, smoothstep(0, height, p.z))
        return (v, v, v, 1.0)

    lod0, lod1, lod2 = Part(), Part(), Part()
    if thorny:
        for k in range(6):
            a = rng.uniform(0, 6.3)
            end = horizontal(a) * width * rng.uniform(0.2, 0.38) + UP * height * rng.uniform(0.45, 0.7)
            Branch(crooked(rng, Vector((0, 0, -0.1)), end, 1, 0.15), 0.045, 0.012, 3).build(lod0, wood, 3, 2, wood_shade)
    cards(lod0, 5, 6)
    cards(lod1, 3, 2)
    cards(lod2, 2, 0)
    return {"lods": [lod0, lod1, lod2], "screen": [0.12, 0.035], "col": None, "wind": 0.05 / (height * height)}


# ---------------------------------------------------------------------------- solid masses


def solid_from(bm_build, target_tris, voxel=None):
    """Builds a bmesh, optionally unions it by voxel remesh, decimates; returns (verts, faces)."""
    me = bpy.data.meshes.new("tmp")
    bm = bm_build()
    bm.to_mesh(me)
    bm.free()
    ob = bpy.data.objects.new("tmp", me)
    bpy.context.scene.collection.objects.link(ob)
    bpy.context.view_layer.objects.active = ob
    if voxel:
        m = ob.modifiers.new("remesh", "REMESH")
        m.mode = "VOXEL"
        m.voxel_size = voxel
        bpy.ops.object.modifier_apply(modifier=m.name)
    return ob


def decimated(ob, target_tris):
    tris = sum(len(p.vertices) - 2 for p in ob.data.polygons)
    copy = ob.copy()
    copy.data = ob.data.copy()
    bpy.context.scene.collection.objects.link(copy)
    bpy.context.view_layer.objects.active = copy
    if tris > target_tris:
        m = copy.modifiers.new("dec", "DECIMATE")
        m.ratio = target_tris / tris
        m.use_collapse_triangulate = True
        bpy.ops.object.modifier_apply(modifier=m.name)
    me = copy.data
    me.calc_loop_triangles()
    verts = [Vector(v.co) for v in me.vertices]
    faces = [list(t.vertices) for t in me.loop_triangles]
    bpy.data.objects.remove(copy)
    return verts, faces


def displace(ob, fn):
    me = ob.data
    for v in me.vertices:
        v.co = fn(Vector(v.co), Vector(v.normal))


def box_uv(p, n, tile, offset):
    a = (abs(n.x), abs(n.y), abs(n.z))
    if a[2] >= a[0] and a[2] >= a[1]:
        return ((p.x + offset[0]) / tile, (p.y + offset[1]) / tile)
    if a[0] >= a[1]:
        return ((p.y + offset[0]) / tile * (1 if n.x > 0 else -1), (p.z + offset[1]) / tile)
    return ((p.x + offset[0]) / tile * (-1 if n.y > 0 else 1), (p.z + offset[1]) / tile)


def add_faceted(part, mat, verts, faces, uv, shade):
    for f in faces:
        pts = [verts[i] for i in f]
        n = face_normal(pts)
        uvs = uv(pts, n)
        part.add(mat, [(p, uvs[k], shade(p, n), n) for k, p in enumerate(pts)])


# ---------------------------------------------------------------------------- termite mounds


def mound(name, seed, spires, base_r, base_h, tris):
    """Spires of red clay grown together: (x, y, radius, height, lean) each, fluted by the rain."""
    rng = random.Random(seed)
    clay = material("clay_mound", "cupinzeiro_barro")

    def build():
        bm = bmesh.new()
        dome = bmesh.ops.create_uvsphere(bm, u_segments=16, v_segments=10, radius=1.0)
        for v in dome["verts"]:
            v.co = Vector((v.co.x * base_r, v.co.y * base_r * 0.9, v.co.z * base_h - 0.2))
        for x, y, r, h, lean in spires:
            ring_n = 12
            prev = None
            first = None
            lean_d = horizontal(rng.uniform(0, 6.3)) * lean
            steps = 9
            for i in range(steps + 1):
                t = i / steps
                rr = r * max(1 - t ** 1.8, 0.0) ** 0.55 * (1 + 0.1 * math.sin(t * 9 + x)) + 0.03
                cz = -0.3 + t * (h + 0.3)
                cx, cy = x + lean_d.x * t * t * h, y + lean_d.y * t * t * h
                ring = []
                for j in range(ring_n):
                    a = 2 * math.pi * j / ring_n
                    f = rr * (1 + 0.16 * math.sin(5 * a + x * 3) * (1 - t * 0.5))
                    ring.append(bm.verts.new((cx + math.cos(a) * f, cy + math.sin(a) * f, cz)))
                if prev:
                    for j in range(ring_n):
                        bm.faces.new((prev[j], prev[(j + 1) % ring_n], ring[(j + 1) % ring_n], ring[j]))
                else:
                    first = ring
                prev = ring
            tip = bm.verts.new((cx, cy, h + 0.02))
            for j in range(ring_n):
                bm.faces.new((prev[j], prev[(j + 1) % ring_n], tip))
            bm.faces.new(list(reversed(first)))
        return bm

    scale = max(base_r, max(s[3] for s in spires))
    ob = solid_from(build, tris, voxel=0.05 * scale)

    def push(p, n):
        streak = noise.noise(Vector((p.x * 2.2, p.y * 2.2, p.z * 0.35 + seed)))
        lump = noise.noise(Vector((p.x * 0.9 + 7, p.y * 0.9, p.z * 0.9)))
        q = p + n * (0.07 * streak + 0.1 * lump) * min(scale, 2.5) * 0.6
        if q.z < -0.15:
            q.z = -0.15 - (q.z + 0.15) * 0.1
        return q

    displace(ob, push)
    height = max(s[3] for s in spires)

    def shade(p, n):
        v = 0.8 * lerp(0.55, 1.0, smoothstep(-0.1, height * 0.5, p.z)) * lerp(0.72, 1.0, smoothstep(-0.6, 0.2, n.z))
        return (v * 0.92, v * 0.9, v * 0.92, 1.0)

    offset = (rng.uniform(0, 1), rng.uniform(0, 1))

    def uv(pts, n):
        out = []
        for p in pts:
            a = math.atan2(p.y, p.x)
            out.append([a / (2 * math.pi) * 2 * math.pi * base_r / 1.4, p.z / 1.4 + offset[1]])
        us = [u for u, _ in out]
        span = 2 * math.pi * base_r / 1.4
        if max(us) - min(us) > span / 2:
            out = [[u + span if u < 0 else u, v] for u, v in out]
        return [tuple(o) for o in out]

    lods = []
    for target in (tris, tris // 3, max(40, tris // 10)):
        part = Part()
        v, f = decimated(ob, target)
        add_faceted(part, clay, v, f, uv, shade)
        lods.append(part)
    col = Part()
    v, f = decimated(ob, 120)
    add_faceted(col, "col", v, f, lambda pts, n: [(0, 0)] * len(pts), lambda p, n: (1, 1, 1, 1))
    bpy.data.objects.remove(ob)
    return {"lods": lods, "screen": [0.15, 0.04], "col": col, "wind": 0.0}


# ---------------------------------------------------------------------------- laterite rocks


def boulder_object(rng, size, seed, cuts, flat=0.0):
    sx, sy, sz = size

    def build():
        bm = bmesh.new()
        bmesh.ops.create_icosphere(bm, subdivisions=3, radius=1.0)
        for v in bm.verts:
            p = v.co
            n = p.normalized()
            bump = 0.09 * noise.noise(n * 1.4 + Vector((seed, 0, 0))) + 0.045 * noise.noise(n * 3.6 + Vector((0, seed, 0)))
            p = n * (1 + bump)
            v.co = Vector((p.x * sx, p.y * sy, p.z * sz))
        # Fractures: flat faces where the rock split or weathered along a joint.
        for _ in range(cuts):
            d = Vector((rng.uniform(-1, 1), rng.uniform(-1, 1), rng.uniform(-0.3, 1.0))).normalized()
            depth = rng.uniform(0.55, 0.8)
            co = Vector((d.x * sx, d.y * sy, d.z * sz)) * depth
            geom = bm.verts[:] + bm.edges[:] + bm.faces[:]
            res = bmesh.ops.bisect_plane(bm, geom=geom, dist=1e-4, plane_co=co, plane_no=d, clear_outer=True)
            edges = [e for e in res["geom_cut"] if isinstance(e, bmesh.types.BMEdge)]
            if edges:
                bmesh.ops.holes_fill(bm, edges=edges)
        # Sit it in the ground: everything under the cut is buried.
        geom = bm.verts[:] + bm.edges[:] + bm.faces[:]
        res = bmesh.ops.bisect_plane(bm, geom=geom, dist=1e-4, plane_co=Vector((0, 0, -sz * (0.35 + flat))), plane_no=Vector((0, 0, -1)), clear_outer=True)
        edges = [e for e in res["geom_cut"] if isinstance(e, bmesh.types.BMEdge)]
        if edges:
            bmesh.ops.holes_fill(bm, edges=edges)
        bmesh.ops.triangulate(bm, faces=bm.faces[:])
        lift = sz * (0.35 + flat)
        for v in bm.verts:
            v.co.z += lift - 0.12
        return bm

    return solid_from(build, 0)


def rocks(name, seed, pieces, tris):
    """Laterite boulders, alone or piled: (x, y, z, sx, sy, sz, yaw, cuts, flat) each."""
    rng = random.Random(seed)
    stone = material("rock_laterite", "rocha_laterita")
    obs = []
    for k, (x, y, z, sx, sy, sz, yaw, cuts, flat) in enumerate(pieces):
        ob = boulder_object(rng, (sx, sy, sz), seed * 31 + k, cuts, flat)
        c, s = math.cos(math.radians(yaw)), math.sin(math.radians(yaw))
        for v in ob.data.vertices:
            p = v.co
            v.co = Vector((p.x * c - p.y * s + x, p.x * s + p.y * c + y, p.z + z))
        obs.append(ob)
    total = sum(max(p[3], p[4]) * max(p[3], p[4]) for p in pieces)
    top = max(p[2] + p[5] * 1.6 for p in pieces)

    def shade(p, n):
        v = 0.86 * lerp(0.55, 1.0, smoothstep(-0.1, min(top, 1.5) * 0.7, p.z)) * lerp(0.68, 1.0, smoothstep(-0.7, 0.3, n.z))
        return (v, v, v, 1.0)

    offs = [(rng.uniform(0, 3), rng.uniform(0, 3)) for _ in obs]
    lods = [Part(), Part(), Part()]
    col = Part()
    for k, ob in enumerate(obs):
        share = max(pieces[k][3], pieces[k][4]) ** 2 / total
        for li, target in enumerate((tris, tris // 3, tris // 9)):
            v, f = decimated(ob, max(12 if li == 2 else 24, int(target * share)))
            add_faceted(lods[li], stone, v, f, lambda pts, n, o=offs[k]: [box_uv(p, n, 1.6, o) for p in pts], shade)
        v, f = decimated(ob, max(16, int(90 * share)))
        add_faceted(col, "col", v, f, lambda pts, n: [(0, 0)] * len(pts), lambda p, n: (1, 1, 1, 1))
        bpy.data.objects.remove(ob)
    return {"lods": lods, "screen": [0.12, 0.035], "col": col, "wind": 0.0}


# ---------------------------------------------------------------------------- dead wood


def dead_tree(name, seed, height, trunk, limbs, broken=False):
    rng = random.Random(seed)
    wood = material("bark_dead", derive("casca_acacia_morta", "casca_acacia", bleach))
    lean = horizontal(rng.uniform(0, 6.3))
    fork = Vector((0, 0, trunk)) + lean * trunk * 0.12
    trunk_r = 0.12 + height * 0.018
    branches = [Branch(crooked(rng, Vector((0, 0, -0.4)), fork, 2, 0.07), trunk_r, trunk_r * 0.75, 0)]
    if not broken:
        a = rng.uniform(0, 6.3)
        for k in range(limbs):
            a += (2 * math.pi / limbs + rng.uniform(-0.4, 0.4)) if k else 0.0
            elev = math.radians(rng.uniform(28, 55))
            length = rng.uniform(0.6, 0.9) * (height - trunk) * 1.5
            d = horizontal(a) * math.cos(elev) + UP * math.sin(elev)
            end = fork + d * length
            b = Branch(crooked(rng, fork - UP * 0.1, end, 2, 0.12), trunk_r * 0.65, 0.02, 1)
            branches.append(b)
            for _ in range(2):
                pts = catmull(b.points, 2)
                t = rng.uniform(0.35, 0.7)
                s = pts[int(t * (len(pts) - 1))]
                dd = (d + horizontal(a + rng.choice((-1, 1)) * 1.2) * 0.8 + UP * 0.3).normalized()
                branches.append(Branch(crooked(rng, s, s + dd * length * rng.uniform(0.3, 0.5), 1, 0.15), lerp(b.r0, b.r1, t) * 0.6, 0.012, 2))
    else:
        # A snag: the trunk snapped, two short stubs left.
        branches[0] = Branch(crooked(rng, Vector((0, 0, -0.4)), fork + UP * height * 0.3, 2, 0.05), trunk_r * 1.1, trunk_r * 0.7, 0)
        top = branches[0].points[-1]
        for k in range(2):
            a = rng.uniform(0, 6.3)
            d = (horizontal(a) + UP * 0.6).normalized()
            s = top - UP * rng.uniform(0.6, 1.4)
            branches.append(Branch([s, s + d * 0.7, s + d * 1.1 + UP * 0.1], trunk_r * 0.4, trunk_r * 0.22, 1))

    def shade(p):
        v = lerp(0.55, 1.0, smoothstep(-0.3, 1.2, p.z))
        return (v, v, v, 1.0)

    lod0, lod1, lod2 = Part(), Part(), Part()
    for b in branches:
        b.build(lod0, wood, {0: 8, 1: 5, 2: 3}[b.level], 3, shade, flare=0.3 if b.level == 0 else 0, cap=broken)
        if b.level < 2:
            b.build(lod1, wood, {0: 5, 1: 3}[b.level], 2, shade, flare=0.25 if b.level == 0 else 0, cap=broken)
        if b.level == 0 or (b.level == 1 and not broken):
            b.build(lod2, wood, 3, 1, shade)
    col = Part()
    collision_tubes(col, [b for b in branches if b.level < 2], 2.4)
    return {"lods": [lod0, lod1, lod2], "screen": [0.15, 0.04], "col": col, "wind": 0.0}


def fallen_log(name, seed, length, radius):
    rng = random.Random(seed)
    wood = material("bark_dead", derive("casca_acacia_morta", "casca_acacia", bleach))
    yaw = 0.0
    a = Vector((-length / 2, 0, radius * 0.75))
    b = Vector((length / 2, rng.uniform(-0.4, 0.4), radius * 0.6))
    main = Branch(crooked(rng, a, b, 2, 0.04), radius, radius * 0.7, 0)
    pts = catmull(main.points, 3)
    stubs = []
    for t in (0.3, 0.62, 0.85):
        s = pts[int(t * (len(pts) - 1))]
        d = (Vector((rng.uniform(-0.2, 0.5), rng.choice((-1, 1)) * rng.uniform(0.4, 1.0), rng.uniform(0.3, 0.9)))).normalized()
        r = lerp(radius, radius * 0.7, t) * rng.uniform(0.35, 0.5)
        stubs.append(Branch([s, s + d * rng.uniform(0.5, 1.1)], r, r * 0.7, 1))
    # The fork it broke from: a limb still attached at the thin end.
    s = pts[-1]
    d = Vector((0.8, rng.choice((-1, 1)) * 0.6, 0.05)).normalized()
    stubs.append(Branch(crooked(rng, s - Vector((0.3, 0, 0)), s + d * length * 0.35, 1, 0.08), radius * 0.55, radius * 0.18, 1))

    def shade(p):
        v = lerp(0.55, 1.0, smoothstep(0.0, radius * 1.6, p.z))
        return (v, v, v, 1.0)

    lod0, lod1, lod2 = Part(), Part(), Part()
    main.build(lod0, wood, 8, 3, shade, cap=True)
    main.build(lod1, wood, 5, 2, shade, cap=True)
    main.build(lod2, wood, 4, 1, shade, cap=True)
    for st in stubs:
        st.build(lod0, wood, 5, 1, shade, cap=True)
        st.build(lod1, wood, 3, 1, shade, cap=True)
    # Base cap at the root end.
    col = Part()
    tube(col, "col", catmull(main.points, 1), [radius * 1.02] * len(main.points), 6, lambda p: (1, 1, 1, 1), cap=True)
    return {"lods": [lod0, lod1, lod2], "screen": [0.12, 0.035], "col": col, "wind": 0.0}


# ---------------------------------------------------------------------------- the set


def models():
    return {
        # name: builder
        "Acacia-01-Guarda-Sol": lambda n: acacia(n, 11, 7.2, 10.5, 2.4, 3, 4.0),
        "Acacia-02-Forquilha": lambda n: acacia(n, 23, 6.4, 9.5, 0.9, 3, 9.0, squash=0.8),
        "Acacia-03-Inclinada": lambda n: acacia(n, 37, 8.2, 9.0, 3.0, 3, 14.0, tiers=2, squash=0.75),
        "Acacia-04-Jovem": lambda n: acacia(n, 41, 4.6, 6.5, 1.5, 2, 6.0, squash=0.9),
        "Arbusto-Espinhoso-01-Baixo": lambda n: bush(n, 2, "arbusto_espinhoso", 1.1, 2.6, True),
        "Arbusto-Espinhoso-02-Alto": lambda n: bush(n, 4, "arbusto_espinhoso", 1.6, 2.0, True),
        "Arbusto-Verde-01-Redondo": lambda n: bush(n, 6, "arbusto_verde", 1.2, 1.9, False),
        "Arbusto-Verde-02-Largo": lambda n: bush(n, 7, "arbusto_verde", 0.9, 2.4, False),
        "Cupinzeiro-01-Catedral": lambda n: mound(n, 1, [(0.0, 0.0, 0.62, 3.4, 0.05), (0.62, 0.3, 0.36, 2.3, 0.18), (-0.55, 0.38, 0.32, 1.9, 0.22), (0.15, -0.62, 0.28, 1.5, 0.15), (-0.5, -0.4, 0.2, 1.0, 0.3)], 1.3, 0.65, 800),
        "Cupinzeiro-02-Cupula": lambda n: mound(n, 2, [(0.15, 0.05, 0.4, 1.75, 0.08), (-0.5, -0.25, 0.22, 1.2, 0.25)], 1.25, 0.95, 500),
        "Cupinzeiro-03-Torres": lambda n: mound(n, 3, [(-0.5, 0.1, 0.42, 2.4, 0.1), (0.35, -0.2, 0.38, 2.0, 0.12), (0.2, 0.55, 0.3, 1.3, 0.2), (0.8, 0.4, 0.22, 0.9, 0.3)], 1.4, 0.55, 700),
        "Laterita-01-Seixo": lambda n: rocks(n, 1, [(0, 0, 0, 0.45, 0.35, 0.3, 0, 2, 0.0)], 80),
        "Laterita-02-Matacao": lambda n: rocks(n, 2, [(0, 0, 0, 0.95, 0.8, 0.7, 20, 3, 0.0)], 160),
        "Laterita-03-Bloco-Grande": lambda n: rocks(n, 3, [(0, 0, 0, 1.8, 1.5, 1.3, 0, 4, 0.05)], 260),
        "Laterita-04-Laje": lambda n: rocks(n, 4, [(0, 0, 0, 2.2, 1.5, 0.55, 10, 3, 0.15), (1.1, 0.5, 0.45, 0.7, 0.6, 0.45, 40, 2, 0.0)], 220),
        "Laterita-05-Pilha": lambda n: rocks(n, 5, [(0, 0, 0, 1.2, 1.0, 0.85, 0, 3, 0.05), (0.15, 0.05, 1.2, 0.8, 0.7, 0.6, 50, 3, 0.1), (0.3, -0.05, 2.05, 0.45, 0.4, 0.38, 10, 2, 0.1)], 320),
        "Laterita-06-Kopje": lambda n: rocks(n, 6, [
            (0, 0, 0, 2.3, 1.9, 1.6, 0, 3, 0.05), (3.4, 0.9, 0, 1.5, 1.25, 1.05, 30, 2, 0.05), (-3.1, 1.3, 0, 1.35, 1.15, 0.95, 70, 2, 0.05),
            (0.9, -2.9, 0, 1.2, 0.95, 0.8, 15, 2, 0.1), (0.5, 0.25, 2.25, 1.15, 0.95, 0.8, 45, 2, 0.1), (-2.2, -1.9, 0, 0.8, 0.7, 0.5, 0, 2, 0.0),
            (2.3, -1.4, 0.0, 0.85, 0.7, 0.6, 80, 2, 0.05), (4.6, -0.8, 0, 0.5, 0.45, 0.35, 0, 1, 0.0), (-0.6, 2.6, 0, 0.6, 0.5, 0.4, 20, 1, 0.0)], 1000),
        "Arvore-Morta-01-Galhada": lambda n: dead_tree(n, 13, 6.0, 1.8, 3),
        "Arvore-Morta-02-Toco": lambda n: dead_tree(n, 17, 4.0, 1.6, 0, broken=True),
        "Tronco-Caido-01": lambda n: fallen_log(n, 19, 4.6, 0.3),
    }


# ---------------------------------------------------------------------------- output


def to_object(name, part, collection):
    verts, faces, mats, uvs, cols, nrms = [], [], [], [], [], []
    keys = list(dict.fromkeys(m for m, _ in part.faces))
    for m, corners in part.faces:
        pts = [c[0] for c in corners]
        fn = face_normal(pts)
        idx = []
        for p, uv, col, n in corners:
            idx.append(len(verts))
            verts.append(tuple(p))
            uvs.append(uv)
            cols.append(col)
            nrms.append(tuple(n) if n is not None else tuple(fn))
        faces.append(idx)
        mats.append(keys.index(m))
    me = bpy.data.meshes.new(name)
    me.from_pydata(verts, [], faces)
    for k in keys:
        if k in MATERIALS:
            me.materials.append(MATERIALS[k])
    me.polygons.foreach_set("material_index", mats)
    uv = me.uv_layers.new(name="UVMap")
    uv.data.foreach_set("uv", [c for u in uvs for c in u])
    col = me.color_attributes.new(name="Color", type="FLOAT_COLOR", domain="CORNER")
    col.data.foreach_set("color", [c for cc in cols for c in cc])
    me.color_attributes.active_color = col
    me.normals_split_custom_set(nrms)
    ob = bpy.data.objects.new(name, me)
    collection.objects.link(ob)
    return ob


def export(name, model):
    for ob in list(bpy.data.objects):
        bpy.data.objects.remove(ob)
    coll = bpy.context.scene.collection
    obs = [to_object(name, model["lods"][0], coll)]
    obs[0]["wind"] = round(model["wind"], 5)
    for k, part in enumerate(model["lods"][1:], 1):
        ob = to_object(f"{name}_LOD{k}", part, coll)
        ob["screen_size"] = model["screen"][k - 1]
        obs.append(ob)
    if model.get("col") and model["col"].faces:
        obs.append(to_object(f"{name}_COL", model["col"], coll))
    for ob in bpy.data.objects:
        ob.select_set(ob in obs)
    path = OUT / f"{name}.glb"
    bpy.ops.export_scene.gltf(
        filepath=str(path), export_format="GLB", use_selection=True, export_yup=True, export_apply=True,
        export_normals=True, export_vertex_color="ACTIVE", export_extras=True, export_materials="EXPORT",
        export_cameras=False, export_lights=False, export_animations=False,
    )
    fix_materials(path)
    tris = [p.triangles() for p in model["lods"]]
    print(f"[savanna] {name}: lods {tris} triangles, collider {model['col'].triangles() if model.get('col') else 0}")
    return {"name": name, "triangles": tris}


def fix_materials(path):
    """Foliage is alpha-tested and seen from both sides; everything else opaque and culled."""
    data = path.read_bytes()
    n = struct.unpack_from("<I", data, 12)[0]
    doc = json.loads(data[20 : 20 + n])
    rest = data[20 + n :]
    for m in doc.get("materials", []):
        leafy = m.get("name", "").startswith("foliage")
        m["doubleSided"] = leafy
        if leafy:
            m["alphaMode"] = "MASK"
            m["alphaCutoff"] = 0.5
        else:
            m.pop("alphaMode", None)
            m.pop("alphaCutoff", None)
    doc["asset"]["generator"] = "Grimora tools/blender/savanna_models.py"
    text = json.dumps(doc, separators=(",", ":")).encode()
    text += b" " * ((4 - len(text) % 4) % 4)
    body = struct.pack("<II", len(text), 0x4E4F534A) + text + rest
    path.write_bytes(struct.pack("<III", 0x46546C67, 2, 12 + len(body)) + body)


argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
OUT = Path(argv[0]).resolve() if argv else ROOT / "screenshots" / "review_savanna"
OUT.mkdir(parents=True, exist_ok=True)
wanted = argv[1:]
bpy.ops.wm.read_factory_settings(use_empty=True)
report = []
for name, build in models().items():
    if wanted and not any(name.startswith(w) for w in wanted):
        continue
    report.append(export(name, build(name)))
old = OUT / "report.json"
merged = {r["name"]: r for r in (json.loads(old.read_text()) if old.exists() else [])}
merged.update({r["name"]: r for r in report})
old.write_text(json.dumps(list(merged.values()), indent=1))
