"""Import the Nyraldar macro map from the compendium site into `assets/world/nyraldar/`.

Every layer becomes an 8-bit grey PNG of category indices (255 = none), so the engine never
blends categories: a texel is one band, one biome, one nation, one water body. The legend and
the points (places, minerals, resources) go to RON files beside them, and each `--coast`
window gets a land mask cut from the base painting (the detail level, 2 texels per map unit),
because the coast must follow the painting, not the topography layer.

    python -I tools/import_world_map.py [--source <maps folder>] [--coast name,x0,y0,w,h ...]

Needs Pillow and NumPy. Deterministic: the same source gives the same files.
"""

import argparse
import json
import os
import sys

import numpy as np
from PIL import Image

Image.MAX_IMAGE_PIXELS = None

DEFAULT_SOURCE = r"C:\Users\Admin\Documents\NYRALDAR\SITE_PROJETO_GITHUB\public\maps"
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets", "world", "nyraldar")
WIDTH, HEIGHT = 8192, 4100
NONE = 255
# The test island, centred at 4476, 1681, with room around its 8 x 8 window: in the painting
# the island sits a little west of that centre and reaches past the window edge.
DEFAULT_COASTS = ["island,4468,1673,16,16"]

# Copied from utils/topographyData.ts and utils/biomeData.ts of the site.
BANDS = [
    ("sea_coast", "Costa / mar raso", (204, 239, 255), "sea", -20),
    ("sea_shallow", "Mar raso", (152, 219, 249), "sea", -120),
    ("sea_mid", "Mar", (89, 197, 243), "sea", -500),
    ("sea_deep", "Mar profundo", (0, 177, 236), "sea", -1800),
    ("sea_abyss", "Oceano profundo", (0, 155, 229), "sea", -4000),
    ("land_low", "Terras baixas", (70, 202, 110), "land", 100),
    ("land_hills", "Colinas", (126, 211, 131), "land", 400),
    ("land_plateau", "Planaltos", (168, 219, 151), "land", 800),
    ("land_high", "Terras altas", (205, 228, 170), "land", 1600),
    ("land_mountains", "Montanhas", (242, 236, 189), "land", 3000),
]
BIOMES = [
    ("pico_congelado", "Pico Congelado", (54, 54, 54)),
    ("calota_polar", "Calota Polar", (255, 255, 255)),
    ("tundra", "Tundra", (68, 140, 152)),
    ("sub_artico", "Sub-Ártico", (0, 75, 63)),
    ("deserto_frio", "Deserto Frio", (229, 198, 152)),
    ("estepe_fria", "Estepe Fria", (138, 168, 89)),
    ("floresta_temperada_decidua", "Floresta Temperada Decídua", (0, 255, 215)),
    ("oceanico", "Oceânico", (0, 171, 0)),
    ("floresta_subtropical", "Floresta Sub-Tropical", (118, 255, 41)),
    ("mediterraneo", "Temperado Seco", (255, 246, 0)),
    ("floresta_seca_tropical", "Floresta Seca Tropical", (108, 98, 196)),
    ("estepe_quente", "Estepe Quente", (255, 170, 0)),
    ("deserto_quente", "Deserto Quente", (255, 0, 0)),
    ("savana_tropical", "Savana Tropical", (0, 151, 255)),
    ("floresta_tropical", "Floresta Tropical", (13, 0, 255)),
]
CLIMATE_OCEAN = (204, 204, 204)


def pack(rgb):
    return (int(rgb[0]) << 16) | (int(rgb[1]) << 8) | int(rgb[2])


def keys_of(path, size=None):
    """Packed RGB per pixel, -1 where transparent."""
    img = Image.open(path).convert("RGBA")
    if size and img.size != size:
        sys.exit(f"{path}: expected {size}, found {img.size}")
    a = np.asarray(img).astype(np.int64)
    k = (a[..., 0] << 16) | (a[..., 1] << 8) | a[..., 2]
    k[a[..., 3] < 128] = -1
    return k


def classify(keys, colors, transparent=NONE, fallback=None):
    """Exact colours become their index; anti-aliased pixels take the majority of their exact
    8 neighbours (ties go to the first in reading order), repeated a few passes; what is left
    takes `fallback` (an index, or "nearest" for the nearest colour)."""
    unknown = 254
    ids = np.full(keys.shape, unknown, np.uint8)
    ids[keys == -1] = transparent
    for i, c in enumerate(colors):
        if c is not None:
            ids[keys == pack(c)] = i
    h, w = ids.shape
    for _ in range(6):
        ys, xs = np.nonzero(ids == unknown)
        if len(ys) == 0:
            break
        neigh = []
        for dy in (-1, 0, 1):
            for dx in (-1, 0, 1):
                if dx == 0 and dy == 0:
                    continue
                yy, xx = np.clip(ys + dy, 0, h - 1), np.clip(xs + dx, 0, w - 1)
                neigh.append(ids[yy, xx])
        neigh = np.stack(neigh, 1).astype(np.int32)
        valid = neigh != unknown
        votes = np.zeros(neigh.shape, np.int32)
        for j in range(8):
            votes += (neigh == neigh[:, j : j + 1]) & valid[:, j : j + 1]
        votes[~valid] = -1
        best = np.argmax(votes, 1)
        chosen = neigh[np.arange(len(ys)), best]
        ok = valid.any(1)
        if not ok.any():
            break
        ids[ys[ok], xs[ok]] = chosen[ok]
    left = ids == unknown
    if left.any():
        if fallback == "nearest":
            pal = np.array([c for c in colors], np.int64)
            k = keys[left]
            rgb = np.stack([(k >> 16) & 255, (k >> 8) & 255, k & 255], 1)
            d = ((rgb[:, None, :] - pal[None, :, :]) ** 2).sum(2)
            ids[left] = np.argmin(d, 1).astype(np.uint8)
        else:
            ids[left] = NONE if fallback is None else fallback
    return ids


def save(ids, name):
    Image.fromarray(ids, "L").save(os.path.join(OUT, name), optimize=True)
    print(f"[world] {name} {ids.shape[1]}x{ids.shape[0]}")


def ron_str(s):
    return '"' + str(s).replace("\\", "\\\\").replace('"', '\\"') + '"'


def hex_rgb(h):
    n = int(h.lstrip("#"), 16)
    return ((n >> 16) & 255, (n >> 8) & 255, n & 255)


def dzi_crop(folder, level, x0, y0, w, h, tile=254, overlap=1):
    out = Image.new("RGB", (w, h))
    for ty in range(y0 // tile, (y0 + h - 1) // tile + 1):
        for tx in range(x0 // tile, (x0 + w - 1) // tile + 1):
            t = Image.open(os.path.join(folder, str(level), f"{tx}_{ty}.jpeg")).convert("RGB")
            ox = tx * tile - (overlap if tx > 0 else 0)
            oy = ty * tile - (overlap if ty > 0 else 0)
            out.paste(t, (ox - x0, oy - y0))
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--source", default=DEFAULT_SOURCE)
    ap.add_argument("--coast", action="append", help="name,x0,y0,w,h in map units")
    args = ap.parse_args()
    src = args.source
    os.makedirs(OUT, exist_ok=True)
    full = (WIDTH, HEIGHT)

    topo = classify(keys_of(os.path.join(src, "overlays/world/topography.png"), full),
                    [b[2] for b in BANDS], fallback="nearest")
    save(topo, "topography.png")

    climate_colors = [b[2] for b in BIOMES] + [CLIMATE_OCEAN]
    climate = classify(keys_of(os.path.join(src, "overlays/world/climate.png"), full), climate_colors)
    climate[climate == len(BIOMES)] = NONE
    save(climate, "climate.png")

    regions = json.load(open(os.path.join(src, "data/political-regions.json"), encoding="utf8"))
    bodies = json.load(open(os.path.join(src, "data/water-bodies.json"), encoding="utf8"))
    if len(regions) >= 254 or len(bodies) >= 254:
        sys.exit("too many categories for 8 bits")
    pol_keys = keys_of(os.path.join(src, "generated/overlays/political_mask.png"))
    political = classify(pol_keys, [tuple(r["color"]) for r in regions])
    save(political, "political.png")

    body_colors = [hex_rgb(b["cor"]) for b in bodies]
    water = classify(keys_of(os.path.join(src, "overlays/world/water.png"), full), body_colors)
    rivers = classify(keys_of(os.path.join(src, "overlays/world/rivers.png"), full), body_colors)
    water[rivers != NONE] = rivers[rivers != NONE]
    save(water, "water.png")

    # Nation names: the water bodies list the nations on their coasts by id and name.
    names = {}
    for b in bodies:
        for n in b.get("nacoes", []):
            names[n["id"]] = n["nome"]

    with open(os.path.join(OUT, "legend.ron"), "w", encoding="utf8", newline="\n") as f:
        f.write("// Written by tools/import_world_map.py: the index of each category is its place here.\n(\n")
        f.write(f"    width: {WIDTH},\n    height: {HEIGHT},\n    km_per_unit: {2000 / 948!r},\n")
        f.write("    layers: [\n")
        f.write('        (kind: Topography, file: "topography.png", scale: 1),\n')
        f.write('        (kind: Climate, file: "climate.png", scale: 1),\n')
        f.write('        (kind: Political, file: "political.png", scale: 2),\n')
        f.write('        (kind: Water, file: "water.png", scale: 1),\n    ],\n')
        f.write("    bands: [\n")
        for i, n, c, k, e in BANDS:
            f.write(f"        (id: {ron_str(i)}, name: {ron_str(n)}, color: {c}, land: {str(k == 'land').lower()}, elevation: {e}),\n")
        f.write("    ],\n    biomes: [\n")
        for i, n, c in BIOMES:
            f.write(f"        (id: {ron_str(i)}, name: {ron_str(n)}, color: {c}),\n")
        f.write("    ],\n    nations: [\n")
        for r in regions:
            name = names.get(r["id"]) or r["id"].replace("_", " ").title()
            f.write(f"        (id: {ron_str(r['id'])}, name: {ron_str(name)}, color: {tuple(r['color'])}),\n")
        f.write("    ],\n    waters: [\n")
        for b, c in zip(bodies, body_colors):
            f.write(f"        (id: {ron_str(b['id'])}, name: {ron_str(b.get('nome') or '')}, kind: {ron_str(b['tipo'])}, color: {c}),\n")
        f.write("    ],\n)\n")
    print("[world] legend.ron")

    points = []
    pois = json.load(open(os.path.join(src, "generated/poi-data.json"), encoding="utf8"))["pois"]
    for p in pois:
        points.append(("place", p["type"], p["name"], p["x"], p["y"]))
    for file, group in [("minerals-curated.json", "mineral"), ("natural-resources.json", "resource"),
                        ("natural-resources-hidricos.json", "resource")]:
        for p in json.load(open(os.path.join(src, "data", file), encoding="utf8")):
            points.append((group, p["type"], p.get("name", ""), p["x"], p["y"]))
    points.sort(key=lambda p: (p[4], p[3], p[0], p[1], p[2]))
    with open(os.path.join(OUT, "points.ron"), "w", encoding="utf8", newline="\n") as f:
        f.write("// Written by tools/import_world_map.py: places, minerals and resources, in map units.\n[\n")
        for g, t, n, x, y in points:
            f.write(f"    (group: {g.capitalize()}, kind: {ron_str(t)}, name: {ron_str(n)}, x: {float(x)!r}, y: {float(y)!r}),\n")
        f.write("]\n")
    print(f"[world] points.ron {len(points)}")

    # Coast windows: the sea is painted a bright pink and the land dark teal inside a darker
    # outline, so land is where red is low (the outline counts as land).
    for spec in args.coast or DEFAULT_COASTS:
        name, x0, y0, w, h = spec.split(",")
        x0, y0, w, h = (int(v) for v in (x0, y0, w, h))
        art = dzi_crop(os.path.join(src, "world/detail/detail_files"), 14, x0 * 2, y0 * 2, w * 2, h * 2)
        a = np.asarray(art).astype(np.int32)
        mask = np.where(a[..., 0] < 0x68, 1, 0).astype(np.uint8)
        save(mask, f"coast_{name}.png")
        art.save(os.path.join(OUT, f"art_{name}.png"))
        print(f"[world] coast {name}: {int(mask.sum())} land texels of {mask.size}")


if __name__ == "__main__":
    main()
