"""Gives the flintlocks moving parts: writes `<model>_rig.glb` next to each source model, the same
meshes plus animations, which the engine turns into a skeleton with one joint per node.

The source's parts have no origins of their own, so turning one around its axle is a rotation
plus the translation that keeps the axle in place, sampled at 60 Hz. Clips, all holding at the end:

    Engatilhada  loaded: hammer back, frizzen closed over the pan
    Disparar     the hammer falls, knocks the frizzen open, with the trigger pulled
    Disparada    as that leaves it
    Sem vareta   the same, with the ramrod hidden, while the hand has it
    Engatilhar   the thumb pulls the hammer back and the frizzen closes

Standard library only:

    python -I tools/rig_guns.py
"""

import json
import math
import os
import struct
import unicodedata

HERE = os.path.dirname(os.path.abspath(__file__))
WEAPONS = os.path.join(HERE, "..", "assets", "models", "weapons")
MODELS = ["Pistola_Pederneira", "Mosquete"]

HAMMER = [
    "pe do cao",
    "pescoco do cao",
    "eixo do cao",
    "mandibula inferior",
    "mandibula superior",
    "pedra de pederneira",
    "parafuso das mandibulas",
    "cabeca torneada do cao",
    "esporao do cao",
]
FRIZZEN = ["braco da bateria", "bateria aberta", "eixo da bateria"]
TRIGGER = ["gatilho"]
ROD = ["vareta de carga", "cabeca da vareta", "ponteira da vareta"]

# Degrees around +Z, as seen from the lock's side.
HAMMER_DOWN = -30.0
FRIZZEN_SHUT = 38.0
TRIGGER_PULLED = -16.0
HIDDEN = 0.001


def plain(name):
    text = unicodedata.normalize("NFKD", name)
    return "".join(c for c in text if not unicodedata.combining(c)).lower()


def read_glb(path):
    data = open(path, "rb").read()
    length = struct.unpack_from("<I", data, 12)[0]
    document = json.loads(data[20:20 + length].decode("utf8"))
    binary = data[28 + length:]
    return document, bytearray(binary)


def write_glb(path, document, binary):
    while len(binary) % 4:
        binary.append(0)
    document["buffers"][0]["byteLength"] = len(binary)
    text = json.dumps(document, ensure_ascii=False, separators=(",", ":")).encode("utf8")
    text += b" " * (-len(text) % 4)
    total = 12 + 8 + len(text) + 8 + len(binary)
    with open(path, "wb") as out:
        out.write(struct.pack("<III", 0x46546C67, 2, total))
        out.write(struct.pack("<II", len(text), 0x4E4F534A))
        out.write(text)
        out.write(struct.pack("<II", len(binary), 0x004E4942))
        out.write(binary)


def bounds(document, binary, node):
    """Lowest and highest corner of a node's vertices, from its accessors."""
    low, high = [math.inf] * 3, [-math.inf] * 3
    for primitive in document["meshes"][node["mesh"]]["primitives"]:
        accessor = document["accessors"][primitive["attributes"]["POSITION"]]
        low = [min(a, b) for a, b in zip(low, accessor["min"])]
        high = [max(a, b) for a, b in zip(high, accessor["max"])]
    return low, high


def ease(t):
    t = max(0.0, min(1.0, t))
    return t * t * (3 - 2 * t)


def curve(keys, t):
    """Follows (seconds, value) keys, easing between them."""
    if t <= keys[0][0]:
        return keys[0][1]
    for (t0, v0), (t1, v1) in zip(keys, keys[1:]):
        if t <= t1:
            return v0 + (v1 - v0) * ease((t - t0) / (t1 - t0))
    return keys[-1][1]


# Each clip: (seconds, hammer keys, frizzen keys, trigger keys, rod scale).
CLIPS = {
    "Engatilhada": (0.02, [(0, 0.0)], [(0, FRIZZEN_SHUT)], [(0, 0.0)], 1.0),
    "Disparar": (
        0.16,
        [(0, 0.0), (0.035, HAMMER_DOWN), (0.06, HAMMER_DOWN + 3.5), (0.09, HAMMER_DOWN)],
        [(0.028, FRIZZEN_SHUT), (0.07, 0.0)],
        [(0, 0.0), (0.025, TRIGGER_PULLED)],
        1.0,
    ),
    "Disparada": (0.02, [(0, HAMMER_DOWN)], [(0, 0.0)], [(0, 0.0)], 1.0),
    "Sem vareta": (0.02, [(0, HAMMER_DOWN)], [(0, 0.0)], [(0, 0.0)], HIDDEN),
    "Engatilhar": (
        0.3,
        [(0, HAMMER_DOWN), (0.18, 4.0), (0.24, 0.0)],
        [(0.1, 0.0), (0.27, FRIZZEN_SHUT)],
        [(0, 0.0)],
        1.0,
    ),
}


class Writer:
    def __init__(self, document, binary):
        self.document, self.binary = document, binary

    def accessor(self, values, kind):
        while len(self.binary) % 4:
            self.binary.append(0)
        offset = len(self.binary)
        flat = [x for value in values for x in (value if isinstance(value, (list, tuple)) else [value])]
        self.binary.extend(struct.pack(f"<{len(flat)}f", *flat))
        views = self.document["bufferViews"]
        views.append({"buffer": 0, "byteOffset": offset, "byteLength": len(flat) * 4})
        accessor = {"bufferView": len(views) - 1, "componentType": 5126, "count": len(values), "type": kind}
        if kind == "SCALAR":
            accessor["min"], accessor["max"] = [min(values)], [max(values)]
        self.document["accessors"].append(accessor)
        return len(self.document["accessors"]) - 1


def around(pivot, degrees):
    """The rotation and translation that turn a part around an axle along Z."""
    half = math.radians(degrees) * 0.5
    c, s = math.cos(2 * half), math.sin(2 * half)
    moved = (pivot[0] * c - pivot[1] * s, pivot[0] * s + pivot[1] * c, pivot[2])
    return (0.0, 0.0, math.sin(half), math.cos(half)), (pivot[0] - moved[0], pivot[1] - moved[1], 0.0)


def rig(name):
    document, binary = read_glb(os.path.join(WEAPONS, f"{name}.glb"))
    nodes = {plain(node.get("name", "")): (i, node) for i, node in enumerate(document["nodes"])}
    missing = [part for part in HAMMER + FRIZZEN + TRIGGER + ROD if part not in nodes and part != "ponteira da vareta" and part != "cabeca da vareta"]
    assert not missing, missing

    def center_of(part):
        low, high = bounds(document, binary, nodes[part][1])
        return [(a + b) * 0.5 for a, b in zip(low, high)]

    hammer_axle = center_of("eixo do cao")
    frizzen_axle = center_of("eixo da bateria")
    low, high = bounds(document, binary, nodes["gatilho"][1])
    trigger_axle = [(low[0] + high[0]) * 0.5, high[1], 0.0]

    writer = Writer(document, binary)
    document["animations"] = []
    for clip, (seconds, hammer, frizzen, trigger, rod) in CLIPS.items():
        count = max(2, round(seconds * 60) + 1)
        times = [seconds * i / (count - 1) for i in range(count)]
        time_accessor = writer.accessor(times, "SCALAR")
        samplers, channels = [], []

        def channel(node, path, accessor):
            samplers.append({"input": time_accessor, "output": accessor, "interpolation": "LINEAR"})
            channels.append({"sampler": len(samplers) - 1, "target": {"node": node, "path": path}})

        for parts, axle, keys in [(HAMMER, hammer_axle, hammer), (FRIZZEN, frizzen_axle, frizzen), (TRIGGER, trigger_axle, trigger)]:
            turns = [around(axle, curve(keys, t)) for t in times]
            rotations = writer.accessor([turn[0] for turn in turns], "VEC4")
            translations = writer.accessor([turn[1] for turn in turns], "VEC3")
            for part in parts:
                channel(nodes[part][0], "rotation", rotations)
                channel(nodes[part][0], "translation", translations)
        scales = writer.accessor([(rod, rod, rod)] * count, "VEC3")
        for part in ROD:
            if part in nodes:
                channel(nodes[part][0], "scale", scales)
        document["animations"].append({"name": clip, "samplers": samplers, "channels": channels})

    write_glb(os.path.join(WEAPONS, f"{name}_rig.glb"), document, binary)
    print(f"{name}: hammer axle {hammer_axle}, frizzen axle {frizzen_axle}")


if __name__ == "__main__":
    for model in MODELS:
        rig(model)
