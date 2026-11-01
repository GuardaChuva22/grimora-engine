"""Generates assets/models/banner: a war banner on a wooden post.

Exercises glTF features the engine supports: an alpha-masked, double-sided
cloth with a torn swallowtail edge and moth holes, and an emissive rune.
Everything here is original and part of the project. Standard library only:

    python tools/gen_banner.py
"""

import json
import math
import os
import struct
import zlib

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets", "models", "banner")
TEX_W, TEX_H = 128, 256


# ---------------------------------------------------------------- helpers

def hash1(i, seed=0):
    x = (i * 374761393 + seed * 668265263) & 0xFFFFFFFF
    x = ((x ^ (x >> 13)) * 1274126177) & 0xFFFFFFFF
    return ((x ^ (x >> 16)) & 0xFFFF) / 0xFFFF


def noise1(x, seed=0):
    i = math.floor(x)
    f = x - i
    f = f * f * (3 - 2 * f)
    return hash1(i, seed) * (1 - f) + hash1(i + 1, seed) * f


def clamp01(x):
    return max(0.0, min(1.0, x))


def write_png(path, width, height, channels, pixel):
    rows = bytearray()
    for y in range(height):
        rows.append(0)
        for x in range(width):
            rows.extend(pixel(x, y))
    color_type = {3: 2, 4: 6}[channels]

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    header = struct.pack(">IIBBBBB", width, height, 8, color_type, 0, 0, 0)
    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(bytes(rows), 9))
                + chunk(b"IEND", b""))


def seg_dist(px, py, ax, ay, bx, by):
    dx, dy = bx - ax, by - ay
    t = clamp01(((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy))
    return math.hypot(px - ax - dx * t, py - ay - dy * t)


# ---------------------------------------------------------------- textures
# UVs: u across the cloth, v from the top (0) to the bottom tip (1). Shapes
# are laid out in meters on the 1.1 x 2.0 m cloth so they keep their proportions.
CLOTH_W, CLOTH_H = 1.1, 2.0
PX_PER_M = TEX_W / CLOTH_W

HOLES = [(0.30, 1.24, 0.04), (0.78, 1.10, 0.03), (0.68, 1.42, 0.05)]  # x, y, radius in meters


def cloth_alpha(u, v):
    x, y = u * CLOTH_W, v * CLOTH_H
    # Swallowtail: long points at the sides, a notch in the middle, ragged threads.
    edge = 0.74 + 0.24 * abs(2 * u - 1) + 0.03 * (noise1(u * 19, 1) - 0.5) + 0.012 * (noise1(u * 53, 2) - 0.5)
    a = clamp01((edge - v) * CLOTH_H * PX_PER_M + 0.5)
    for hx, hy, r in HOLES:
        ang = math.atan2(y - hy, x - hx)
        rr = r * (0.75 + 0.5 * noise1(ang * 3 + 10, 3))
        a = min(a, clamp01((math.hypot(x - hx, y - hy) - rr) * PX_PER_M + 0.5))
    return a


RUNE_CENTER, RUNE_RADIUS = (0.55, 0.62), 0.3
RUNE = [  # strokes in meters
    ((0.55, 0.38), (0.55, 0.86)),
    ((0.55, 0.46), (0.40, 0.60)),
    ((0.55, 0.46), (0.70, 0.60)),
    ((0.55, 0.64), (0.42, 0.76)),
    ((0.55, 0.64), (0.68, 0.76)),
]


def rune_glow(u, v):
    x, y = u * CLOTH_W, v * CLOTH_H
    d = abs(math.hypot(x - RUNE_CENTER[0], y - RUNE_CENTER[1]) - RUNE_RADIUS)
    for (ax, ay), (bx, by) in RUNE:
        d = min(d, seg_dist(x, y, ax, ay, bx, by))
    core = clamp01((0.018 - d) * PX_PER_M + 0.5)
    halo = math.exp(-d * 30) * 0.3
    return clamp01(core + halo)


def color_pixel(x, y):
    u, v = (x + 0.5) / TEX_W, (y + 0.5) / TEX_H
    weave = 0.92 + 0.08 * hash1(x * 977 + y * 131, 4)
    fold = 0.85 + 0.15 * math.sin(u * math.pi * 4.4)
    grime = 0.75 + 0.25 * (1 - v) ** 0.5
    shade = weave * fold * grime
    border = u < 0.07 or u > 0.93 or v < 0.05
    base = (0.55, 0.42, 0.18) if border else (0.40, 0.06, 0.07)
    rune = rune_glow(u, v)
    # The rune is stitched in pale thread so it reads even unlit.
    r, g, b = (c * shade * (1 - rune) + t * rune for c, t in zip(base, (0.85, 0.62, 0.36)))
    a = cloth_alpha(u, v)
    return bytes(int(clamp01(c) ** (1 / 2.2) * 255 + 0.5) for c in (r, g, b)) + bytes([int(a * 255 + 0.5)])


def rune_pixel(x, y):
    u, v = (x + 0.5) / TEX_W, (y + 0.5) / TEX_H
    g = rune_glow(u, v)
    return bytes(int(g ** (1 / 2.2) * 255 + 0.5) for _ in range(3))


# ---------------------------------------------------------------- geometry

class Prim:
    def __init__(self):
        self.pos, self.nrm, self.uv, self.idx = [], [], [], []


def add_box(p, lo, hi):
    x0, y0, z0 = lo
    x1, y1, z1 = hi
    faces = [
        ((1, 0, 0), [(x1, y0, z1), (x1, y0, z0), (x1, y1, z0), (x1, y1, z1)]),
        ((-1, 0, 0), [(x0, y0, z0), (x0, y0, z1), (x0, y1, z1), (x0, y1, z0)]),
        ((0, 1, 0), [(x0, y1, z1), (x1, y1, z1), (x1, y1, z0), (x0, y1, z0)]),
        ((0, -1, 0), [(x0, y0, z0), (x1, y0, z0), (x1, y0, z1), (x0, y0, z1)]),
        ((0, 0, 1), [(x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1)]),
        ((0, 0, -1), [(x1, y0, z0), (x0, y0, z0), (x0, y1, z0), (x1, y1, z0)]),
    ]
    for n, quad in faces:
        base = len(p.pos)
        p.pos += quad
        p.nrm += [n] * 4
        p.uv += [(0, 0)] * 4
        p.idx += [base, base + 1, base + 2, base, base + 2, base + 3]


def cloth():
    p = Prim()
    cols, rows = 8, 16
    width, top, height = 1.1, 2.88, 2.0

    def point(u, v):
        x = (u - 0.5) * width
        # Folds across, plus a gentle billow that grows towards the bottom.
        z = 0.08 + 0.035 * math.sin(u * math.pi * 4.4) * v + 0.06 * v * v
        return (x, top - v * height, z)

    for r in range(rows + 1):
        for c in range(cols + 1):
            u, v = c / cols, r / rows
            p.pos.append(point(u, v))
            p.uv.append((u, v))
            e = 1e-3
            du = [a - b for a, b in zip(point(u + e, v), point(u - e, v))]
            dv = [a - b for a, b in zip(point(u, v + e), point(u, v - e))]
            # Front faces +Z: cross(down the cloth, across the cloth).
            n = (dv[1] * du[2] - dv[2] * du[1], dv[2] * du[0] - dv[0] * du[2], dv[0] * du[1] - dv[1] * du[0])
            length = math.sqrt(sum(c * c for c in n))
            p.nrm.append(tuple(c / length for c in n))
    for r in range(rows):
        for c in range(cols):
            a = r * (cols + 1) + c
            b, d = a + 1, a + cols + 1
            p.idx += [a, d, b, b, d, d + 1]
    return p


def main():
    os.makedirs(OUT, exist_ok=True)
    write_png(os.path.join(OUT, "banner_color.png"), TEX_W, TEX_H, 4, color_pixel)
    write_png(os.path.join(OUT, "banner_rune.png"), TEX_W, TEX_H, 3, rune_pixel)

    wood = Prim()
    add_box(wood, (-0.05, 0.0, -0.05), (0.05, 3.2, 0.05))
    add_box(wood, (-0.68, 2.88, 0.04), (0.68, 2.96, 0.12))
    prims = [(wood, 0), (cloth(), 1)]

    blob = bytearray()
    views, accessors, gltf_prims = [], [], []

    def add_view(data, target):
        while len(blob) % 4:
            blob.append(0)
        views.append({"buffer": 0, "byteOffset": len(blob), "byteLength": len(data), "target": target})
        blob.extend(data)
        return len(views) - 1

    def add_accessor(values, kind, bounds=False):
        flat = [c for v in values for c in v]
        view = add_view(struct.pack(f"<{len(flat)}f", *flat), 34962)
        acc = {"bufferView": view, "componentType": 5126, "count": len(values), "type": kind}
        if bounds:
            acc["min"] = [min(v[i] for v in values) for i in range(3)]
            acc["max"] = [max(v[i] for v in values) for i in range(3)]
        accessors.append(acc)
        return len(accessors) - 1

    for p, material in prims:
        attributes = {
            "POSITION": add_accessor(p.pos, "VEC3", bounds=True),
            "NORMAL": add_accessor(p.nrm, "VEC3"),
            "TEXCOORD_0": add_accessor(p.uv, "VEC2"),
        }
        view = add_view(struct.pack(f"<{len(p.idx)}H", *p.idx), 34963)
        accessors.append({"bufferView": view, "componentType": 5123, "count": len(p.idx), "type": "SCALAR"})
        gltf_prims.append({"attributes": attributes, "indices": len(accessors) - 1, "material": material})

    with open(os.path.join(OUT, "banner.bin"), "wb") as f:
        f.write(blob)

    doc = {
        "asset": {"version": "2.0", "generator": "Grimora tools/gen_banner.py"},
        "extensionsUsed": ["KHR_materials_emissive_strength"],
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"name": "banner", "mesh": 0}],
        "meshes": [{"name": "banner", "primitives": gltf_prims}],
        "materials": [
            {
                "name": "wood",
                "pbrMetallicRoughness": {"baseColorFactor": [0.09, 0.05, 0.03, 1], "metallicFactor": 0,
                                         "roughnessFactor": 0.9},
            },
            {
                "name": "cloth",
                "doubleSided": True,
                "alphaMode": "MASK",
                "alphaCutoff": 0.5,
                "pbrMetallicRoughness": {"baseColorTexture": {"index": 0}, "metallicFactor": 0,
                                         "roughnessFactor": 0.85},
                "emissiveTexture": {"index": 1},
                "emissiveFactor": [1.0, 0.45, 0.12],
                "extensions": {"KHR_materials_emissive_strength": {"emissiveStrength": 6.0}},
            },
        ],
        "textures": [{"source": 0, "sampler": 0}, {"source": 1, "sampler": 0}],
        "samplers": [{"magFilter": 9729, "minFilter": 9987, "wrapS": 33071, "wrapT": 33071}],
        "images": [{"uri": "banner_color.png"}, {"uri": "banner_rune.png"}],
        "buffers": [{"uri": "banner.bin", "byteLength": len(blob)}],
        "bufferViews": views,
        "accessors": accessors,
    }
    with open(os.path.join(OUT, "banner.gltf"), "w") as f:
        json.dump(doc, f, indent=1)
    print("wrote", os.path.normpath(OUT))


if __name__ == "__main__":
    main()
