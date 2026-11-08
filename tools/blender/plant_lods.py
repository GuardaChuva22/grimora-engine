# Gives hand-made plant models their LODs and a trunk collider, leaving the model as it is.
#
#   blender --background --python tools/blender/plant_lods.py -- <out_dir> <model.glb>...
#
# Writes `<out_dir>/<model>.glb`: every node and byte of the source, plus
#   `<model>_LOD1`  the same parts without the finest twigs and dry sprigs, and half the leaf cards;
#   `<model>_LOD2`  an impostor: the model rendered from the front, the side and above (in
#                   Blender, unlit, as the model is) on three crossed cards;
#   `<model>_COL`   the trunk (nodes named `Tronco...`/`Caule...`), what the engine collides with.
# Parts are told apart by their node names, as the model kit writes them ("Galharia terminal",
# "Ramos finos", "Ponteiras", "Galhos secos", "Bainhas"), and leaves by their alpha-tested materials.
# Run it again whenever a source model changes.

import bpy
import json
import math
import struct
import sys
import unicodedata
from pathlib import Path

import numpy as np

VIEW = 256  # pixels per impostor view
LOD1_SCREEN = 0.25
LOD2_SCREEN = 0.08
THIN = ("terminal", "seco", "finos", "ponteira", "bainha")
TRUNK = ("tronco", "caule")


def plain(name):
    return unicodedata.normalize("NFKD", name or "").encode("ascii", "ignore").decode().lower()


# ---------------------------------------------------------------------------- GLB in and out


class Glb:
    def __init__(self, path):
        data = Path(path).read_bytes()
        n = struct.unpack_from("<I", data, 12)[0]
        self.doc = json.loads(data[20 : 20 + n])
        rest = data[20 + n :]
        m = struct.unpack_from("<I", rest, 0)[0]
        self.bin = bytearray(rest[8 : 8 + m])

    def add_view(self, raw, target=None):
        while len(self.bin) % 4:
            self.bin.append(0)
        view = {"buffer": 0, "byteOffset": len(self.bin), "byteLength": len(raw)}
        if target:
            view["target"] = target
        self.bin.extend(raw)
        self.doc["bufferViews"].append(view)
        return len(self.doc["bufferViews"]) - 1

    def add_accessor(self, array, kind, normalized=False):
        array = np.ascontiguousarray(array)
        comp = {np.dtype("float32"): 5126, np.dtype("uint32"): 5125, np.dtype("uint16"): 5123}[array.dtype]
        target = 34963 if kind == "SCALAR" else 34962
        acc = {"bufferView": self.add_view(array.tobytes(), target), "componentType": comp, "count": len(array), "type": kind}
        if kind == "VEC3" and comp == 5126:
            acc["min"] = array.min(axis=0).tolist()
            acc["max"] = array.max(axis=0).tolist()
        self.doc["accessors"].append(acc)
        return len(self.doc["accessors"]) - 1

    def read(self, index):
        acc = self.doc["accessors"][index]
        view = self.doc["bufferViews"][acc["bufferView"]]
        dtype = {5126: np.float32, 5125: np.uint32, 5123: np.uint16, 5121: np.uint8}[acc["componentType"]]
        width = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}[acc["type"]]
        start = view.get("byteOffset", 0) + acc.get("byteOffset", 0)
        stride = view.get("byteStride")
        size = np.dtype(dtype).itemsize * width
        buf = bytes(self.bin)
        if stride and stride != size:
            rows = [np.frombuffer(buf, dtype, width, start + i * stride) for i in range(acc["count"])]
            return np.array(rows)
        out = np.frombuffer(buf, dtype, acc["count"] * width, start).copy()
        return out.reshape(-1, width) if width > 1 else out.copy()

    def write(self, path):
        while len(self.bin) % 4:
            self.bin.append(0)
        self.doc["buffers"] = [{"byteLength": len(self.bin)}]
        text = json.dumps(self.doc, separators=(",", ":")).encode()
        text += b" " * ((4 - len(text) % 4) % 4)
        body = struct.pack("<II", len(text), 0x4E4F534A) + text + struct.pack("<II", len(self.bin), 0x004E4942) + bytes(self.bin)
        Path(path).write_bytes(struct.pack("<III", 0x46546C67, 2, 12 + len(body)) + body)


def node_matrix(node):
    if "matrix" in node:
        return np.array(node["matrix"], dtype=np.float64).reshape(4, 4).T
    t = node.get("translation", [0, 0, 0])
    x, y, z, w = node.get("rotation", [0, 0, 0, 1])
    s = node.get("scale", [1, 1, 1])
    r = np.array([
        [1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
        [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
        [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)],
    ])
    m = np.eye(4)
    m[:3, :3] = r * np.array(s)
    m[:3, 3] = t
    return m


def drawn_nodes(glb):
    """(node index, world matrix) of every node with a mesh in the default scene."""
    doc = glb.doc
    scene = doc["scenes"][doc.get("scene", 0)]
    out = []
    stack = [(i, np.eye(4)) for i in scene["nodes"]]
    while stack:
        i, parent = stack.pop()
        world = parent @ node_matrix(doc["nodes"][i])
        if "mesh" in doc["nodes"][i]:
            out.append((i, world))
        stack.extend((c, world) for c in doc["nodes"][i].get("children", []))
    return out


def leafy(glb, primitive):
    mats = glb.doc.get("materials", [])
    m = mats[primitive["material"]] if "material" in primitive and primitive["material"] < len(mats) else {}
    return m.get("alphaMode", "OPAQUE") != "OPAQUE"


def cards(indices):
    """Groups triangles into the cards they make (triangles sharing vertices)."""
    tris = indices.reshape(-1, 3)
    parent = {}

    def find(a):
        while parent.setdefault(a, a) != a:
            parent[a] = parent[parent[a]]
            a = parent[a]
        return a

    for a, b, c in tris:
        ra, rb, rc = find(int(a)), find(int(b)), find(int(c))
        parent[rb] = ra
        parent[find(rc)] = ra
    groups = {}
    for t, (a, _, _) in enumerate(tris):
        groups.setdefault(find(int(a)), []).append(t)
    return list(groups.values())


# ---------------------------------------------------------------------------- impostor views


def render_views(src, work):
    """Front, side and top orthographic pictures of the model, as glTF-space frames."""
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=str(src))
    meshes = [o for o in bpy.context.scene.objects if o.type == "MESH"]
    pts = np.array([list(o.matrix_world @ v.co) for o in meshes for v in o.data.vertices])
    lo, hi = pts.min(axis=0), pts.max(axis=0)
    cx, cy = (lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2
    height = hi[2]
    side = max(hi[0] - lo[0], hi[1] - lo[1], height) * 1.03
    top = max(hi[0] - lo[0], hi[1] - lo[1]) * 1.03
    scene = bpy.context.scene
    for engine in ("BLENDER_EEVEE", "BLENDER_EEVEE_NEXT"):
        try:
            scene.render.engine = engine
            break
        except TypeError:
            continue
    scene.render.film_transparent = True
    scene.render.resolution_x = scene.render.resolution_y = VIEW
    scene.render.image_settings.file_format = "PNG"
    scene.render.image_settings.color_mode = "RGBA"
    scene.view_settings.view_transform = "Standard"
    if scene.world is None:
        scene.world = bpy.data.worlds.new("w")
    cam_data = bpy.data.cameras.new("cam")
    cam_data.type = "ORTHO"
    cam = bpy.data.objects.new("cam", cam_data)
    scene.collection.objects.link(cam)
    scene.camera = cam
    views = {
        "front": ((cx, lo[1] - 50, side / 2), (math.pi / 2, 0, 0), side),
        "side": ((hi[0] + 50, cy, side / 2), (math.pi / 2, 0, math.pi / 2), side),
        "top": ((cx, cy, height + 50), (0, 0, 0), top),
    }
    pictures = {}
    for name, (loc, rot, scale) in views.items():
        cam.location = loc
        cam.rotation_euler = rot
        cam_data.ortho_scale = scale
        cam_data.clip_end = 500
        path = work / f"{src.stem}_{name}.png"
        scene.render.filepath = str(path)
        bpy.ops.render.render(write_still=True)
        img = bpy.data.images.load(str(path))
        pictures[name] = np.array(img.pixels[:], dtype=np.float32).reshape(VIEW, VIEW, 4)[::-1]
    # Blender (x, y, z) is glTF (x, z, -y).
    frame = {"cx": cx, "cz": -cy, "side": side, "top": top, "height": height}
    return pictures, frame


def atlas_png(pictures, path):
    """The three views side by side; colour bleeds under the cut-out so mips keep no dark rim."""
    strip = np.concatenate([pictures["front"], pictures["side"], pictures["top"]], axis=1)
    img = bpy.data.images.new("atlas", strip.shape[1], strip.shape[0], alpha=True)
    img.pixels = strip[::-1].reshape(-1).tolist()
    img.filepath_raw = str(path)
    img.file_format = "PNG"
    img.save()
    return Path(path).read_bytes()


def impostor(glb, png, frame, crown_y, name):
    cx, cz, s, t, h = frame["cx"], frame["cz"], frame["side"], frame["top"], frame["height"]
    third = 1 / 3
    # glTF UVs: v = 0 at the top of the picture.
    quads = [
        # front: plane through the middle, facing +z; u along +x
        ([(cx - s / 2, 0, cz), (cx + s / 2, 0, cz), (cx + s / 2, s, cz), (cx - s / 2, s, cz)], 0.0),
        # side: plane through the middle, facing +x; u along -z
        ([(cx, 0, cz + s / 2), (cx, 0, cz - s / 2), (cx, s, cz - s / 2), (cx, s, cz + s / 2)], third),
    ]
    pos, uv, nrm, col, idx = [], [], [], [], []
    for corners, u0 in quads:
        base = len(pos)
        for k, p in enumerate(corners):
            pos.append(p)
            uv.append((u0 + third * (k in (1, 2)), 1.0 - (p[1] / s)))
            d = np.array([p[0] - cx, max(p[1] - h * 0.5, 0.0) * 0.6 + h * 0.3, p[2] - cz])
            nrm.append(d / np.linalg.norm(d))
            col.append((1.0, 1.0, 1.0, 1.0))
        idx += [base, base + 1, base + 2, base, base + 2, base + 3]
    # top: horizontal at the crown, u along +x, picture top towards -z
    base = len(pos)
    for k, (dx, dz) in enumerate(((-0.5, 0.5), (0.5, 0.5), (0.5, -0.5), (-0.5, -0.5))):
        p = (cx + dx * t, crown_y, cz + dz * t)
        pos.append(p)
        uv.append((2 * third + third * (dx + 0.5), 0.5 + dz))
        nrm.append((0.0, 1.0, 0.0))
        col.append((1.0, 1.0, 1.0, 1.0))
    idx += [base, base + 1, base + 2, base, base + 2, base + 3]
    doc = glb.doc
    image = glb.add_view(png)
    doc.setdefault("images", []).append({"bufferView": image, "mimeType": "image/png", "name": f"{name}_impostor"})
    doc.setdefault("samplers", []).append({"magFilter": 9729, "minFilter": 9987, "wrapS": 33071, "wrapT": 33071})
    doc.setdefault("textures", []).append({"sampler": len(doc["samplers"]) - 1, "source": len(doc["images"]) - 1})
    doc.setdefault("materials", []).append({
        "name": "foliage_impostor", "doubleSided": True, "alphaMode": "MASK", "alphaCutoff": 0.5,
        "pbrMetallicRoughness": {"baseColorTexture": {"index": len(doc["textures"]) - 1}, "metallicFactor": 0.0, "roughnessFactor": 1.0},
    })
    prim = {
        "attributes": {
            "POSITION": glb.add_accessor(np.array(pos, np.float32), "VEC3"),
            "NORMAL": glb.add_accessor(np.array(nrm, np.float32), "VEC3"),
            "TEXCOORD_0": glb.add_accessor(np.array(uv, np.float32), "VEC2"),
            "COLOR_0": glb.add_accessor(np.array(col, np.float32), "VEC4"),
        },
        "indices": glb.add_accessor(np.array(idx, np.uint32), "SCALAR"),
        "material": len(doc["materials"]) - 1,
    }
    doc["meshes"].append({"name": f"{name}_LOD2", "primitives": [prim]})
    return len(doc["meshes"]) - 1


# ---------------------------------------------------------------------------- per model


def augment(src, out_dir, work):
    pictures, frame = render_views(src, work)
    glb = Glb(src)
    doc = glb.doc
    name = src.stem
    scene = doc["scenes"][doc.get("scene", 0)]
    drawn = drawn_nodes(glb)

    lod1_children, col_children = [], []
    leaf_heights = []
    kept = dropped = 0
    for i, world in drawn:
        node = doc["nodes"][i]
        label = plain(node.get("name"))
        mesh = doc["meshes"][node["mesh"]]
        if label.startswith(TRUNK):
            col_children.append({"name": f"{node.get('name')} (col)", "mesh": node["mesh"], "matrix": world.T.reshape(-1).tolist()})
        if any(w in label for w in THIN):
            continue
        prims = []
        for p in mesh["primitives"]:
            if leafy(glb, p) and "indices" in p:
                indices = glb.read(p["indices"]).astype(np.uint32).reshape(-1)
                ys = glb.read(p["attributes"]["POSITION"])[:, 1]
                leaf_heights.append(np.median(ys) + world[1, 3])
                groups = cards(indices)
                tris = indices.reshape(-1, 3)
                keep = [t for k, g in enumerate(groups) if k % 2 == 0 for t in g]
                kept += len(keep)
                dropped += len(tris) - len(keep)
                q = dict(p)
                q["indices"] = glb.add_accessor(tris[sorted(keep)].reshape(-1), "SCALAR")
                prims.append(q)
            else:
                prims.append(p)
        doc["meshes"].append({"name": f"{mesh.get('name')} (lod1)", "primitives": prims})
        lod1_children.append({"name": f"{node.get('name')} (lod1)", "mesh": len(doc["meshes"]) - 1, "matrix": world.T.reshape(-1).tolist()})

    height = frame["height"]
    crown_y = float(np.mean(leaf_heights)) if leaf_heights else height * 0.8
    png = atlas_png(pictures, work / f"{name}_atlas.png")
    lod2_mesh = impostor(glb, png, frame, crown_y, name)
    wind = round(0.08 / (height * height), 5)

    def group(label, children, extras):
        first = len(doc["nodes"])
        doc["nodes"].extend(children)
        doc["nodes"].append({"name": label, "children": list(range(first, first + len(children))), "extras": extras})
        scene["nodes"].append(len(doc["nodes"]) - 1)

    group(f"{name}_LOD1", lod1_children, {"screen_size": LOD1_SCREEN, "wind": wind})
    group(f"{name}_LOD2", [{"name": f"{name} impostor", "mesh": lod2_mesh}], {"screen_size": LOD2_SCREEN})
    if col_children:
        group(f"{name}_COL", col_children, {})
    doc["asset"]["generator"] = doc["asset"].get("generator", "") + " + Grimora tools/blender/plant_lods.py"
    glb.write(out_dir / f"{name}.glb")
    tri = lambda meshes: sum(glb.read(p["indices"]).size // 3 for m in meshes for p in m["primitives"])
    lod0 = tri([doc["meshes"][doc["nodes"][i]["mesh"]] for i, _ in drawn])
    lod1 = tri([doc["meshes"][c["mesh"]] for c in lod1_children])
    col = tri([doc["meshes"][c["mesh"]] for c in col_children])
    print(f"[plant_lods] {name}: {lod0} / {lod1} / 6 triangles, collider {col}, crown at {crown_y:.1f} m of {height:.1f}")


argv = sys.argv[sys.argv.index("--") + 1 :]
OUT = Path(argv[0]).resolve()
OUT.mkdir(parents=True, exist_ok=True)
WORK = OUT / "_impostors"
WORK.mkdir(exist_ok=True)
for path in argv[1:]:
    augment(Path(path).resolve(), OUT, WORK)
