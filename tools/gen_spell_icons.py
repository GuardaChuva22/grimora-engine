"""Draws the pictures of the ice spells in assets/items/icons, as 48 px pixel art.

Every shape is a polygon, drawn four times finer and reduced to a short palette of ice colors,
with a dark outline around what's drawn. Standard library only:

    python -I tools/gen_spell_icons.py
"""

import math
import os
import struct
import zlib

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets", "items", "icons")
SIZE = 48
FINE = 4

# Shades of ice, from the deepest to the brightest.
SHADES = [(18, 44, 96), (34, 88, 168), (70, 148, 226), (134, 206, 250), (214, 244, 255), (255, 255, 255)]
OUTLINE = (6, 12, 34)


def new_canvas():
    return [[None] * (SIZE * FINE) for _ in range(SIZE * FINE)]


def polygon(canvas, points, shade):
    """Fills a polygon, in 48 px units, with a shade from 0 to 5."""
    points = [(x * FINE, y * FINE) for x, y in points]
    low = max(0, int(min(y for _, y in points)))
    high = min(SIZE * FINE - 1, int(max(y for _, y in points)) + 1)
    for row in range(low, high + 1):
        y = row + 0.5
        crossings = []
        for (x0, y0), (x1, y1) in zip(points, points[1:] + points[:1]):
            if (y0 <= y) != (y1 <= y):
                crossings.append(x0 + (y - y0) / (y1 - y0) * (x1 - x0))
        crossings.sort()
        for a, b in zip(crossings[::2], crossings[1::2]):
            for column in range(max(0, int(a + 0.5)), min(SIZE * FINE, int(b + 0.5))):
                canvas[row][column] = shade


def shard(canvas, base, tip, width, light=3, dark=1, core=None):
    """A crystal from `base` to its `tip`, lit along one side."""
    (bx, by), (tx, ty) = base, tip
    length = math.hypot(tx - bx, ty - by) or 1
    nx, ny = -(ty - by) / length * width, (tx - bx) / length * width
    shoulder = (bx + (tx - bx) * 0.3, by + (ty - by) * 0.3)
    polygon(canvas, [base, (shoulder[0] + nx, shoulder[1] + ny), tip], dark)
    polygon(canvas, [base, tip, (shoulder[0] - nx, shoulder[1] - ny)], light)
    if core is not None:
        polygon(canvas, [(shoulder[0], shoulder[1]), (shoulder[0] - nx * 0.35, shoulder[1] - ny * 0.35), tip], core)


def reduced(canvas):
    """The picture at its real size: each pixel takes the most common shade under it."""
    out = []
    for y in range(SIZE):
        row = []
        for x in range(SIZE):
            counts = {}
            for fy in range(FINE):
                for fx in range(FINE):
                    shade = canvas[y * FINE + fy][x * FINE + fx]
                    if shade is not None:
                        counts[shade] = counts.get(shade, 0) + 1
            covered = sum(counts.values())
            row.append(max(counts, key=counts.get) if covered * 2 >= FINE * FINE else None)
        out.append(row)
    return out


def write(name, canvas):
    pixels = reduced(canvas)
    raw = bytearray()
    for y in range(SIZE):
        raw.append(0)
        for x in range(SIZE):
            shade = pixels[y][x]
            if shade is not None:
                raw += bytes(SHADES[shade]) + b"\xff"
                continue
            near = any(
                0 <= y + dy < SIZE and 0 <= x + dx < SIZE and pixels[y + dy][x + dx] is not None
                for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1))
            )
            raw += bytes(OUTLINE) + b"\xff" if near else b"\0\0\0\0"

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    header = struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0)
    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + chunk(b"IEND", b"")
    with open(os.path.join(OUT, name + ".png"), "wb") as f:
        f.write(png)


def spear():
    c = new_canvas()
    # The shaft, corner to corner, with the head in the top right.
    polygon(c, [(5, 41), (7, 43), (31, 19), (29, 17)], 2)
    polygon(c, [(5, 41), (6, 42), (30, 18), (29, 17)], 3)
    for base, tip in (((30, 24), (24, 31)), ((24, 18), (17, 25)), ((16, 34), (12, 30))):
        shard(c, base, tip, 1.8, 1, 0)
    shard(c, (25, 23), (44, 4), 5.0, 4, 2, 5)
    # A glint and the cold around it.
    for x, y, r in ((13, 15, 2.2), (38, 30, 1.8), (22, 41, 1.4)):
        polygon(c, [(x, y - r * 2), (x + r * 0.5, y), (x, y + r * 2), (x - r * 0.5, y)], 4)
        polygon(c, [(x - r * 2, y), (x, y - r * 0.5), (x + r * 2, y), (x, y + r * 0.5)], 4)
    return c


def stomp():
    c = new_canvas()
    # Spikes opening into a cone, away from the foot at the bottom.
    for base, tip, width, light in (
        ((24, 31), (24, 4), 4.2, 4),
        ((19, 32), (10, 8), 3.6, 3),
        ((29, 32), (38, 8), 3.6, 3),
        ((15, 35), (3, 19), 2.9, 2),
        ((33, 35), (45, 19), 2.9, 2),
    ):
        shard(c, base, tip, width, light, max(light - 2, 0), 5 if light == 4 else None)
    # The boot coming down, seen from the front.
    polygon(c, [(17, 28), (31, 28), (32, 37), (34, 44), (14, 44), (16, 37)], 1)
    polygon(c, [(17, 28), (24, 28), (24, 44), (14, 44), (16, 37)], 2)
    polygon(c, [(14, 41), (34, 41), (34, 44), (14, 44)], 0)
    polygon(c, [(16.5, 32), (31.5, 32), (31.5, 33.5), (16.5, 33.5)], 3)
    # The ground cracking on both sides.
    polygon(c, [(2, 44), (13, 42), (13, 45), (3, 46)], 3)
    polygon(c, [(35, 42), (46, 44), (45, 46), (35, 45)], 3)
    return c


def blast():
    c = new_canvas()
    middle = (24, 24)
    for k in range(12):
        angle = k / 12 * math.tau + 0.13
        long = k % 2 == 0
        reach = 22.0 if long else 14.5
        base = (middle[0] + math.cos(angle) * 5, middle[1] + math.sin(angle) * 5)
        tip = (middle[0] + math.cos(angle) * reach, middle[1] + math.sin(angle) * reach)
        shard(c, base, tip, 3.4 if long else 2.4, 3 if long else 2, 1 if long else 0, 4 if long else None)
    # The heart of it.
    ring = [(middle[0] + math.cos(k / 8 * math.tau) * 8, middle[1] + math.sin(k / 8 * math.tau) * 8) for k in range(8)]
    polygon(c, ring, 3)
    ring = [(middle[0] + math.cos(k / 8 * math.tau + 0.4) * 5.5, middle[1] + math.sin(k / 8 * math.tau + 0.4) * 5.5) for k in range(8)]
    polygon(c, ring, 4)
    polygon(c, [(24, 18), (25.4, 24), (24, 30), (22.6, 24)], 5)
    polygon(c, [(18, 24), (24, 22.6), (30, 24), (24, 25.4)], 5)
    return c


if __name__ == "__main__":
    write("lanca_de_gelo", spear())
    write("pisao_de_gelo", stomp())
    write("explosao_de_gelo", blast())
