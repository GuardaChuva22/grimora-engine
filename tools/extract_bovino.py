"""Pulls what the engine needs out of `assets/models/animals/Bovino.glb`, without touching it:

- the four coats (`KHR_materials_variants`) as PNGs in `assets/models/animals/bovino/`;
- `Bovino_body.ron`: the formulas of the configurable body and, for each shape key, how far
  each joint's head moves at weight 1 (glTF axes: Blender's (x, y, z) is glTF's (x, z, -y)).

Run: python -I tools/extract_bovino.py
"""

import json
import pathlib
import struct

ROOT = pathlib.Path(__file__).resolve().parent.parent
MODEL = ROOT / "assets/models/animals/Bovino.glb"
OUT = ROOT / "assets/models/animals/bovino"
COATS = ["castanha", "malhada", "ruiva", "cinza"]


def main():
    data = MODEL.read_bytes()
    length = struct.unpack("<I", data[12:16])[0]
    gltf = json.loads(data[20 : 20 + length])
    rest = data[20 + length :]
    blob_len = struct.unpack("<I", rest[0:4])[0]
    blob = rest[8 : 8 + blob_len]

    OUT.mkdir(exist_ok=True)
    for image, name in zip(gltf["images"], COATS):
        view = gltf["bufferViews"][image["bufferView"]]
        start = view.get("byteOffset", 0)
        (OUT / f"pelagem_{name}.png").write_bytes(blob[start : start + view["byteLength"]])

    config = gltf["extras"]["configuracao_bovina"]
    lines = ["// Written by tools/extract_bovino.py from Bovino.glb: don't edit by hand.", "BovineBody(", "    moves: {"]
    for joint, keys in config["rest_joint_deltas"].items():
        for key, d in keys.items():
            head = (d[0], d[2], -d[1])
            if max(abs(v) for v in head) < 1e-6:
                continue
            lines.append(f'        ("{key}", "{joint}"): ({head[0]:.6f}, {head[1]:.6f}, {head[2]:.6f}),')
    lines += ["    },", ")", ""]
    (ROOT / "assets/models/animals/Bovino_body.ron").write_text("\n".join(lines), encoding="utf-8")
    print("coats and body written")


if __name__ == "__main__":
    main()
